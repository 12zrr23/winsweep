use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use chrono::Local;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WhitelistConfig {
    pub paths: Vec<String>,
}

impl Default for WhitelistConfig {
    fn default() -> Self {
        Self {
            paths: vec![
                r"C:\Windows".to_string(),
                r"C:\Program Files\Windows Defender".to_string(),
            ],
        }
    }
}

pub struct SafetyGuard {
    blacklist: Vec<PathBuf>,
    protected_roots: Vec<PathBuf>,
    whitelist: Vec<PathBuf>,
    log_file_path: PathBuf,
}

impl SafetyGuard {
    pub fn new() -> Self {
        let mut blacklist = Vec::new();

        // 核心 Windows 系统目录及其关键子目录
        if let Ok(windir) = std::env::var("WINDIR") {
            let winpath = PathBuf::from(&windir);
            blacklist.push(Self::normalize_path(&winpath));
            blacklist.push(Self::normalize_path(&winpath.join("System32")));
            blacklist.push(Self::normalize_path(&winpath.join("SysWOW64")));
            blacklist.push(Self::normalize_path(&winpath.join("WinSxS")));
            blacklist.push(Self::normalize_path(&winpath.join("System")));
        } else {
            blacklist.push(Self::normalize_path(Path::new(r"C:\Windows")));
            blacklist.push(Self::normalize_path(Path::new(r"C:\Windows\System32")));
            blacklist.push(Self::normalize_path(Path::new(r"C:\Windows\SysWOW64")));
            blacklist.push(Self::normalize_path(Path::new(r"C:\Windows\WinSxS")));
        }

        // 核心安全软件保护
        if let Ok(prog) = std::env::var("ProgramFiles") {
            blacklist.push(Self::normalize_path(&PathBuf::from(&prog).join("Windows Defender")));
        }
        if let Ok(prog_x86) = std::env::var("ProgramFiles(x86)") {
            blacklist.push(Self::normalize_path(&PathBuf::from(&prog_x86).join("Windows Defender")));
        }

        let mut protected_roots = Vec::new();
        // 用户根目录如 C:\Users\Administrator 自身不可直接删除
        if let Ok(profile) = std::env::var("USERPROFILE") {
            protected_roots.push(Self::normalize_path(Path::new(&profile)));
        }

        // Program Files / ProgramData 自身也不可作为单个实体被直接删除
        if let Ok(prog) = std::env::var("ProgramFiles") {
            protected_roots.push(Self::normalize_path(Path::new(&prog)));
        }
        if let Ok(prog_data) = std::env::var("ProgramData") {
            protected_roots.push(Self::normalize_path(Path::new(&prog_data)));
        }

        // 加载用户白名单
        let config_dir = Self::get_config_dir();
        let _ = fs::create_dir_all(&config_dir);

        let whitelist_file = config_dir.join("whitelist.json");
        let mut whitelist = Vec::new();
        if let Ok(content) = fs::read_to_string(&whitelist_file) {
            if let Ok(cfg) = serde_json::from_str::<WhitelistConfig>(&content) {
                for p in cfg.paths {
                    whitelist.push(Self::normalize_path(Path::new(&p)));
                }
            }
        }

        let log_file_path = config_dir.join("operations.log");

        Self {
            blacklist,
            protected_roots,
            whitelist,
            log_file_path,
        }
    }

    /// 路径归一化：剥除 Windows `\\?\` 与 `\\?\UNC\` 前缀，并统一小写处理，彻底规避大小写与 UNC 前缀导致的匹配失效
    pub fn normalize_path(path: &Path) -> PathBuf {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let s = canonical.to_string_lossy();
        let stripped = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{}", rest)
        } else if let Some(rest) = s.strip_prefix(r"\\?\") {
            rest.to_string()
        } else {
            s.to_string()
        };
        PathBuf::from(stripped.to_lowercase())
    }

    pub fn get_config_dir() -> PathBuf {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            PathBuf::from(local_app_data).join("winsweep")
        } else if let Ok(profile) = std::env::var("USERPROFILE") {
            PathBuf::from(profile).join(".config").join("winsweep")
        } else {
            PathBuf::from(".winsweep")
        }
    }

    /// 严格校验路径是否可以被清理或删除
    pub fn is_safe_to_delete(&self, path: &Path) -> Result<(), String> {
        let norm_path = Self::normalize_path(path);
        let path_str = norm_path.to_string_lossy();

        // 1. 绝对不允许删除驱动器盘符根目录，例如 C:\ 或 D:\ 或 \\?\C:
        let trimmed = path_str.trim_end_matches(['\\', '/'].as_ref());
        if (trimmed.len() <= 2 && trimmed.ends_with(':')) || path.parent().is_none() {
            return Err(format!("BLOCKED: Cannot delete drive root '{:?}'", path));
        }

        // 2. 系统级核心黑名单校验（严格阻断其本身与任何子项）
        for black in &self.blacklist {
            if norm_path == *black || norm_path.starts_with(black) {
                return Err(format!("BLOCKED by Blacklist: '{:?}' is a protected Windows core path", path));
            }
        }

        // 3. 根目录保护（仅自身不可删除，子项如 Temp/Downloads 可安全清理）
        for root in &self.protected_roots {
            if norm_path == *root {
                return Err(format!("BLOCKED: Cannot delete protected root directory '{:?}'", path));
            }
        }

        // 4. 用户白名单校验
        for white in &self.whitelist {
            if norm_path == *white || norm_path.starts_with(white) {
                return Err(format!("SKIPPED by Whitelist: '{:?}' is in user protected whitelist", path));
            }
        }

        Ok(())
    }

    /// 安全删除操作（默认移入回收站）
    #[allow(dead_code)]
    pub fn safe_delete(&self, path: &Path, permanent: bool) -> Result<(), String> {
        self.is_safe_to_delete(path)?;

        let file_size = if path.is_file() {
            fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        if permanent {
            if path.is_dir() {
                fs::remove_dir_all(path).map_err(|e| format!("Failed to remove dir: {}", e))?;
            } else {
                fs::remove_file(path).map_err(|e| format!("Failed to remove file: {}", e))?;
            }
            self.log_operation("PERMANENT_DELETE", path, file_size);
        } else {
            trash::delete(path).map_err(|e| format!("Failed to move to trash: {}", e))?;
            self.log_operation("MOVE_TO_TRASH", path, file_size);
        }

        Ok(())
    }

    /// 记录审计日志
    pub fn log_operation(&self, action: &str, path: &Path, bytes_freed: u64) {
        if let Some(parent) = self.log_file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file_path)
        {
            let now = Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(
                file,
                "[{}] action={}, bytes={}, path={:?}",
                now, action, bytes_freed, path
            );
        }
    }
}

/// 将脚本转为 PowerShell -EncodedCommand 所需的 UTF-16LE Base64 编码
pub fn encode_powershell_script(script: &str) -> String {
    let utf16_bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;
    while i < utf16_bytes.len() {
        let b0 = utf16_bytes[i];
        let b1 = if i + 1 < utf16_bytes.len() { utf16_bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < utf16_bytes.len() { utf16_bytes[i + 2] } else { 0 };

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);

        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);

        if i + 1 < utf16_bytes.len() {
            result.push(CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }

        if i + 2 < utf16_bytes.len() {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }

        i += 3;
    }
    result
}
