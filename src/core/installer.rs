use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use crate::core::safety::SafetyGuard;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstallerPackage {
    pub file_name: String,
    pub path: PathBuf,
    pub size: u64,
    pub extension: String,
    pub source_location: String,
}

pub struct InstallerScanner {
    guard: SafetyGuard,
}

impl InstallerScanner {
    pub fn new() -> Self {
        Self {
            guard: SafetyGuard::new(),
        }
    }

    pub fn scan_all(&self) -> Vec<InstallerPackage> {
        let mut targets = Vec::new();
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(profile);
            targets.push(("Downloads", p.join("Downloads")));
            targets.push(("Desktop", p.join("Desktop")));
        }
        let mut results = Vec::new();
        for (source_name, dir) in targets {
            if dir.exists() && dir.is_dir() {
                self.scan_folder(&dir, source_name, &mut results);
            }
        }
        results.sort_by(|a, b| b.size.cmp(&a.size));
        results
    }

    pub fn scan_custom_path(&self, dir: &Path) -> Vec<InstallerPackage> {
        let mut results = Vec::new();
        if dir.exists() && dir.is_dir() {
            let source_name = dir
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Custom".to_string());
            self.scan_folder(dir, &source_name, &mut results);
        }
        results.sort_by(|a, b| b.size.cmp(&a.size));
        results
    }

    fn scan_folder(&self, folder: &Path, source_name: &str, results: &mut Vec<InstallerPackage>) {
        for entry in WalkDir::new(folder).min_depth(1).max_depth(2).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(ext) = path.extension().and_then(|s| s.to_str()) else { continue };
            let ext_lower = ext.to_lowercase();
            let mut matched = matches!(ext_lower.as_str(), "msi" | "iso" | "msix");

            if !matched && matches!(ext_lower.as_str(), "exe" | "zip") {
                let name_lower = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_lowercase();
                matched = name_lower.contains("setup")
                    || name_lower.contains("install")
                    || name_lower.contains("update")
                    || name_lower.contains("patch")
                    || name_lower.contains("init")
                    || name_lower.ends_with("-x64.exe")
                    || name_lower.ends_with("_x64.exe")
                    || name_lower.ends_with("-win64.exe")
                    || name_lower.ends_with("_win64.exe")
                    || name_lower.ends_with("-windows-x86_64.exe");
            }

            if !matched || self.guard.is_safe_to_delete(path).is_err() {
                continue;
            }

            if let Ok(meta) = fs::metadata(path) {
                let size = meta.len();
                if size > 5 * 1024 * 1024 {
                    results.push(InstallerPackage {
                        file_name: path
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Unknown".to_string()),
                        path: path.to_path_buf(),
                        size,
                        extension: ext_lower,
                        source_location: source_name.to_string(),
                    });
                }
            }
        }
    }

    pub fn clean_installers(&self, items: &[InstallerPackage], dry_run: bool) -> (u64, usize) {
        let paths: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
        self.clean_by_paths(&paths, dry_run)
    }

    /// 清理选中安装包：回收站优先 → 原生删除 → Shell 回收站兜底，最终以磁盘事实核验
    pub fn clean_by_paths(&self, paths: &[PathBuf], dry_run: bool) -> (u64, usize) {
        let targets: Vec<(PathBuf, u64)> = paths
            .iter()
            .filter(|p| p.exists() && p.is_file() && self.guard.is_safe_to_delete(p).is_ok())
            .map(|p| (p.clone(), fs::metadata(p).map(|m| m.len()).unwrap_or(0)))
            .collect();

        if targets.is_empty() {
            return (0, 0);
        }

        let total_size: u64 = targets.iter().map(|(_, s)| *s).sum();
        if dry_run {
            return (total_size, targets.len());
        }

        // 第一轮：系统回收站（trash 使用 IFileOperation，正常用户权限即可）
        let mut remaining: Vec<(PathBuf, u64)> = Vec::new();
        for (p, size) in &targets {
            if !(trash::delete(p).is_ok() && !p.exists()) {
                remaining.push((p.clone(), *size));
            }
        }

        // 第二轮：解除只读后原生删除
        let mut still: Vec<(PathBuf, u64)> = Vec::new();
        for (p, size) in remaining {
            clear_readonly(&p);
            if !(fs::remove_file(&p).is_ok() && !p.exists()) {
                still.push((p, size));
            }
        }

        // 第三轮：Shell COM 回收站动词（批量一次进程）
        if !still.is_empty() {
            shell_com_delete_files(&still);
        }

        // 磁盘事实核验
        let mut freed_bytes = 0u64;
        let mut freed_count = 0usize;
        for (p, size) in &targets {
            if !p.exists() {
                freed_bytes += *size;
                freed_count += 1;
                self.guard.log_operation("DELETE_INSTALLER", p, *size);
            }
        }
        (freed_bytes, freed_count)
    }
}

pub fn clear_readonly(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

/// 使用 Shell.Application 的 delete 动词批量删除文件（等价于资源管理器中删除到回收站）
pub fn shell_com_delete_files(items: &[(PathBuf, u64)]) {
    let payload: Vec<serde_json::Value> = items
        .iter()
        .filter_map(|(p, _)| {
            let dir = p.parent()?.to_string_lossy().to_string();
            let name = p.file_name()?.to_string_lossy().to_string();
            Some(serde_json::json!({ "dir": dir, "name": name }))
        })
        .collect();

    if payload.is_empty() {
        return;
    }

    let json = serde_json::json!({ "targets": payload }).to_string();
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$payload = $env:WINSWEEP_PAYLOAD | ConvertFrom-Json
$shell = New-Object -ComObject Shell.Application
foreach ($t in $payload.targets) {
    $full = Join-Path $t.dir $t.name
    if (-not (Test-Path -LiteralPath $full)) { continue }
    try {
        $folder = $shell.Namespace($t.dir)
        if ($folder) {
            $item = $folder.ParseName($t.name)
            if ($item) { $item.InvokeVerb('delete') }
        }
    } catch { }
}
Start-Sleep -Milliseconds 600
"#;

    let encoded = crate::core::safety::encode_powershell_script(script);
    let _ = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-EncodedCommand")
        .arg(encoded)
        .env("WINSWEEP_PAYLOAD", &json)
        .output();
}
