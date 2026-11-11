use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use crate::core::safety::SafetyGuard;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PurgeArtifact {
    pub project_name: String,
    pub artifact_type: String,
    pub path: PathBuf,
    pub size: u64,
    pub last_modified_days_ago: u64,
    pub is_recently_active: bool,
}

pub struct ProjectPurgeScanner {
    guard: SafetyGuard,
}

impl ProjectPurgeScanner {
    pub fn new() -> Self {
        Self {
            guard: SafetyGuard::new(),
        }
    }

    /// 扫描指定的若干个代码工程根目录
    pub fn scan_roots(&self, roots: &[PathBuf]) -> Vec<PurgeArtifact> {
        let mut results = Vec::new();

        for root in roots {
            if root.exists() && root.is_dir() {
                self.scan_directory(root, &mut results);
            }
        }

        results.sort_by(|a, b| b.size.cmp(&a.size));
        results
    }

    /// 默认扫描用户常见工作区与当前目录
    pub fn scan_default_workspace(&self) -> Vec<PurgeArtifact> {
        let mut roots = Vec::new();

        // 当前工作目录
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd);
        }

        if let Ok(profile) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(profile);
            let candidates = vec![
                p.join("Projects"),
                p.join("dev"),
                p.join("workspace"),
                p.join("source"),
                p.join("repos"),
                p.join("GitHub"),
            ];
            for c in candidates {
                if c.exists() && c.is_dir() {
                    roots.push(c);
                }
            }
        }

        self.scan_roots(&roots)
    }

    /// 扫描单个指定路径
    pub fn scan_path(&self, path: &Path) -> Vec<PurgeArtifact> {
        let roots = vec![path.to_path_buf()];
        self.scan_roots(&roots)
    }

    fn scan_directory(&self, root: &Path, results: &mut Vec<PurgeArtifact>) {
        let mut it = WalkDir::new(root)
            .min_depth(1)
            .max_depth(6)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                // 仅忽略版本控制核心系统、垃圾箱与系统隐藏目录，绝不可过滤 node_modules 或 target
                name != ".git"
                    && name != ".svn"
                    && name != ".hg"
                    && name != "AppData"
                    && name != "$RECYCLE.BIN"
                    && name != "System Volume Information"
            });

        while let Some(entry_res) = it.next() {
            let entry = match entry_res {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.is_dir() {
                let file_name = match path.file_name().and_then(|s| s.to_str()) {
                    Some(n) => n,
                    None => continue,
                };

                let artifact_type = Self::identify_artifact_type(path, file_name);

                if let Some(kind) = artifact_type {
                    // 一旦确认是构建产物目录，跳过继续向下遍历其内部子项，避免性能浪费
                    it.skip_current_dir();

                    // 确认安全边界（绝不清理核心路径）
                    if self.guard.is_safe_to_delete(path).is_ok() {
                        let (size, days_ago) = self.calculate_dir_stats(path);
                        let project_name = path
                            .parent()
                            .and_then(|p| p.file_name())
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Unknown".to_string());

                        results.push(PurgeArtifact {
                            project_name,
                            artifact_type: kind.to_string(),
                            path: path.to_path_buf(),
                            size,
                            last_modified_days_ago: days_ago,
                            is_recently_active: days_ago < 7,
                        });
                    }
                }
            }
        }
    }

    /// 严格识别真正的项目构建与依赖产物，彻底杜绝把普通工具软件的 bin/ 目录误杀
    fn identify_artifact_type(path: &Path, name: &str) -> Option<&'static str> {
        let parent = path.parent();

        match name {
            "node_modules" => {
                // 必须在项目工程目录下（上层包含 package.json）
                if let Some(p) = parent {
                    if p.join("package.json").exists() {
                        return Some("Node.js Modules");
                    }
                }
                None
            }
            "target" => {
                // 必须是 Rust 或 Maven/Scala 项目工程
                if let Some(p) = parent {
                    if p.join("Cargo.toml").exists() || p.join("pom.xml").exists() || p.join("build.sbt").exists() {
                        return Some("Build Target");
                    }
                }
                None
            }
            ".venv" | "venv" => {
                // 必须是真正的 Python 虚拟环境 (包含 pyvenv.cfg)
                if path.join("pyvenv.cfg").exists() {
                    return Some("Python Virtualenv");
                }
                None
            }
            "__pycache__" => Some("Python Bytecode Cache"),
            "dist" | "build" => {
                if let Some(p) = parent {
                    if p.join("package.json").exists()
                        || p.join("pyproject.toml").exists()
                        || p.join("setup.py").exists()
                        || p.join("CMakeLists.txt").exists()
                        || p.join("Makefile").exists()
                    {
                        return Some("Build Output");
                    }
                }
                None
            }
            "bin" | "obj" => {
                // 核心防误杀：绝不可直接将所有名为 bin 的文件夹当作 .NET 产物！
                // 只有在父目录存在 *.csproj / *.sln，或子目录包含 Debug / Release / net* 时才视作构建产物
                if Self::is_dotnet_artifact(path) {
                    Some(".NET Build Output")
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn is_dotnet_artifact(path: &Path) -> bool {
        if let Some(parent) = path.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    if let Some(ext) = entry.path().extension() {
                        let ext_lower = ext.to_string_lossy().to_lowercase();
                        if ext_lower == "csproj" || ext_lower == "fsproj" || ext_lower == "vbproj" || ext_lower == "sln" {
                            return true;
                        }
                    }
                }
            }
        }

        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let sub_name = entry.file_name().to_string_lossy().to_lowercase();
                if sub_name == "debug" || sub_name == "release" || sub_name.starts_with("net") {
                    return true;
                }
            }
        }

        false
    }

    fn calculate_dir_stats(&self, dir: &Path) -> (u64, u64) {
        let mut total_size = 0u64;
        let mut latest_time = SystemTime::UNIX_EPOCH;

        // 深度探测至 15 层以真实统计完整目录大小
        for entry in WalkDir::new(dir).min_depth(1).max_depth(15).into_iter().filter_map(|e| e.ok()) {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total_size += meta.len();
                }
                if let Ok(mod_time) = meta.modified() {
                    if mod_time > latest_time {
                        latest_time = mod_time;
                    }
                }
            }
        }

        let now = SystemTime::now();
        let days_ago = match now.duration_since(latest_time) {
            Ok(d) => d.as_secs() / 86400,
            Err(_) => 0,
        };

        (total_size, days_ago)
    }

    /// 执行清理
    pub fn purge_artifacts(&self, artifacts: &[PurgeArtifact], dry_run: bool) -> (u64, usize) {
        let paths: Vec<PathBuf> = artifacts.iter().map(|a| a.path.clone()).collect();
        self.purge_by_paths(&paths, dry_run)
    }

    /// 根据用户选中的路径列表执行产物清理（直接操作选中的有效路径）
    pub fn purge_by_paths(&self, paths: &[PathBuf], dry_run: bool) -> (u64, usize) {
        if paths.is_empty() {
            return (0, 0);
        }

        let valid_paths: Vec<PathBuf> = paths
            .iter()
            .filter(|p| p.exists() && self.guard.is_safe_to_delete(p).is_ok())
            .cloned()
            .collect();

        if valid_paths.is_empty() {
            return (0, 0);
        }

        if dry_run {
            let mut total_size = 0u64;
            for p in &valid_paths {
                let (size, _) = self.calculate_dir_stats(p);
                total_size += size;
            }
            return (total_size, valid_paths.len());
        }

        let mut freed_count = 0usize;
        let mut freed_bytes = 0u64;

        for p in &valid_paths {
            let (size, _) = self.calculate_dir_stats(p);

            // 1. 回收站优先
            let mut deleted = trash::delete(p).is_ok() && !p.exists();

            // 2. 原生递归删除
            if !deleted {
                deleted = std::fs::remove_dir_all(p).is_ok() && !p.exists();
            }

            // 3. Shell 回收站动词兜底（目录同样支持）
            if !deleted {
                crate::core::installer::shell_com_delete_files(&[(p.clone(), size)]);
                deleted = !p.exists();
            }

            if deleted {
                freed_count += 1;
                freed_bytes += size;
                self.guard.log_operation("PURGE_ARTIFACT", p, size);
            }
        }

        (freed_bytes, freed_count)
    }
}
