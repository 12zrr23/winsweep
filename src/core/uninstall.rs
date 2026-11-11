use std::path::Path;
use std::process::Command;
use serde::{Deserialize, Serialize};
use crate::core::cleaner::SystemCleaner;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InstalledApp {
    pub display_name: String,
    pub display_version: String,
    pub publisher: String,
    pub estimated_size_kb: u64,
    pub uninstall_string: String,
    pub install_location: String,
    /// 是否可安全交由 WinSweep 直接调用（false 时前端只展示、不提供按钮）
    pub can_uninstall: bool,
    /// 不可卸载的原因（给前端提示）
    pub block_reason: String,
}

pub struct AppUninstaller;

impl AppUninstaller {
    pub fn new() -> Self {
        Self
    }

    /// 列出系统中已安装的所有第三方与通用应用
    pub fn list_installed_apps(&self) -> Vec<InstalledApp> {
        let ps_script = r#"
            [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
            $paths = @(
                "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*",
                "HKLM:\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
                "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*"
            )
            Get-ItemProperty $paths -ErrorAction SilentlyContinue |
            Where-Object { $_.DisplayName -and -not $_.SystemComponent -and $_.UninstallString -and -not $_.ParentKeyName } |
            Select-Object DisplayName, DisplayVersion, Publisher, EstimatedSize, UninstallString, InstallLocation |
            ConvertTo-Json -Compress
        "#;

        let encoded_cmd = SystemCleaner::encode_powershell_script(ps_script);

        let output = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-EncodedCommand")
            .arg(encoded_cmd)
            .output();

        let mut apps = Vec::new();

        if let Ok(out) = output {
            let json_str = String::from_utf8_lossy(&out.stdout);
            for line in json_str.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
                        for item in parsed {
                            apps.push(Self::parse_app_item(&item));
                        }
                        break;
                    }
                } else if trimmed.starts_with('{') && trimmed.ends_with('}') {
                    if let Ok(single) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        apps.push(Self::parse_app_item(&single));
                        break;
                    }
                }
            }
        }

        apps.sort_by(|a, b| b.estimated_size_kb.cmp(&a.estimated_size_kb));
        apps
    }

    /// 根据关键字过滤应用
    pub fn search_apps(&self, query: &str) -> Vec<InstalledApp> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.list_installed_apps();
        }

        self.list_installed_apps()
            .into_iter()
            .filter(|app| {
                app.display_name.to_lowercase().contains(&q)
                    || app.publisher.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// 校验卸载命令是否可安全执行
    /// 规则：只接受 msiexec 或本地已存在的 .exe；拒绝网络路径、脚本解释器、shell 拼接符
    pub fn check_uninstall_command(raw: &str) -> Result<String, String> {
        let s = raw.trim();
        if s.is_empty() {
            return Err("该应用没有提供卸载命令".to_string());
        }
        let lower = s.to_lowercase();

        // 明确拒绝危险载体
        for bad in ["powershell", "pwsh", "cmd.exe", "wscript", "cscript", "mshta", "rundll32", "regsvr32", "bitsadmin"] {
            if lower.starts_with(bad) {
                return Err(format!("卸载命令来自脚本宿主（{}），为安全起见不自动执行，请到「设置 → 应用」卸载", bad));
            }
        }
        if lower.contains("http://") || lower.contains("https://") || lower.contains("\\\\") {
            return Err("卸载命令包含网络路径，已拒绝执行".to_string());
        }

        // MSI：msiexec.exe /I{GUID} 或 /X{GUID}
        if lower.starts_with("msiexec") {
            let guid_ok = s.contains("/X") || s.contains("/x") || s.contains("/I") || s.contains("/i");
            if !guid_ok {
                return Err("MSI 卸载命令缺少 /X 参数".to_string());
            }
            return Ok(s.to_string());
        }

        // 普通 exe：优先取引号内路径；否则按 ".exe" 边界切分（兼容 C:\Program Files\... 这类含空格未加引号的写法）
        let exe_path = if s.starts_with('"') {
            match s[1..].find('"') {
                Some(i) => &s[1..1 + i],
                None => return Err("卸载命令引号不匹配".to_string()),
            }
        } else if let Some(idx) = lower.find(".exe") {
            &s[..idx + 4]
        } else {
            s.split_whitespace().next().unwrap_or("")
        };

        if exe_path.is_empty() {
            return Err("无法解析卸载程序路径".to_string());
        }
        if !Path::new(exe_path).exists() {
            return Err(format!("卸载程序不存在（可能已被删除）：{}", exe_path));
        }
        let ext = Path::new(exe_path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext != "exe" {
            return Err(format!("卸载程序不是 .exe（{}），不自动执行", ext));
        }
        Ok(s.to_string())
    }

    /// 启动卸载程序（默认带界面，交由用户确认；不静默、不强制）
    pub fn run_uninstall(&self, display_name: &str, requested_cmd: &str) -> Result<String, String> {
        // 以注册表实时结果为准，防止前端传入任意命令
        let apps = self.list_installed_apps();
        let target = apps
            .iter()
            .find(|a| a.display_name == display_name && a.uninstall_string == requested_cmd)
            .ok_or_else(|| "未在注册表中找到匹配的应用与卸载命令，已拒绝执行".to_string())?;

        let cmd = Self::check_uninstall_command(&target.uninstall_string)?;

        let spawned = if cmd.to_lowercase().starts_with("msiexec") {
            // msiexec /X{GUID} —— 直接调用，Windows 会弹出卸载向导
            let rest = cmd
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            Command::new("msiexec.exe").arg(rest).spawn()
        } else {
            // 例如 "C:\Program Files\App\uninstall.exe" /S 或 -uninstall
            let (program, args) = split_command(&cmd);
            Command::new(program).args(args).spawn()
        };

        match spawned {
            Ok(child) => Ok(format!(
                "已启动「{}」的卸载程序（PID {}），请在弹出的窗口里完成卸载",
                target.display_name,
                child.id()
            )),
            Err(e) => {
                // 740 = ERROR_ELEVATION_REQUIRED：卸载程序要求管理员，改用 UAC 提权启动
                if e.raw_os_error() == Some(740) {
                    if let Ok(_) = Self::launch_elevated(&cmd) {
                        return Ok(format!(
                            "已请求以管理员身份启动「{}」的卸载程序，请在 UAC 弹窗中允许后完成卸载",
                            target.display_name
                        ));
                    }
                }
                Err(format!("启动卸载程序失败：{}", e))
            }
        }
    }

    /// 通过 ShellExecute 的 runas 动词提权启动（会弹 UAC）
    fn launch_elevated(cmd: &str) -> Result<(), String> {
        let (program, args) = split_command(cmd);
        let arg_list = args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        let script = if arg_list.is_empty() {
            format!(
                "Start-Process -FilePath '{}' -Verb RunAs",
                program.replace('\'', "''")
            )
        } else {
            format!(
                "Start-Process -FilePath '{}' -ArgumentList @({}) -Verb RunAs",
                program.replace('\'', "''"),
                arg_list
            )
        };
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn parse_app_item(v: &serde_json::Value) -> InstalledApp {
        let uninstall_string = v["UninstallString"].as_str().unwrap_or_default().trim().to_string();
        let (can, reason) = match Self::check_uninstall_command(&uninstall_string) {
            Ok(_) => (true, String::new()),
            Err(e) => (false, e),
        };
        InstalledApp {
            display_name: v["DisplayName"].as_str().unwrap_or_default().trim().to_string(),
            display_version: v["DisplayVersion"].as_str().unwrap_or_default().trim().to_string(),
            publisher: v["Publisher"].as_str().unwrap_or_default().trim().to_string(),
            estimated_size_kb: v["EstimatedSize"].as_u64().unwrap_or_default(),
            uninstall_string,
            install_location: v["InstallLocation"].as_str().unwrap_or_default().trim().to_string(),
            can_uninstall: can,
            block_reason: reason,
        }
    }
}

/// 拆分 "C:\path\a.exe" /S /Q 这类命令为 (程序, 参数)
fn split_command(cmd: &str) -> (String, Vec<String>) {
    let mut program = String::new();
    let mut rest = String::new();

    if cmd.starts_with('"') {
        if let Some(i) = cmd[1..].find('"') {
            program = cmd[1..1 + i].to_string();
            rest = cmd[2 + i..].trim().to_string();
        }
    } else if let Some(idx) = cmd.to_lowercase().find(".exe") {
        program = cmd[..idx + 4].to_string();
        rest = cmd[idx + 4..].trim().to_string();
    } else {
        match cmd.find(char::is_whitespace) {
            Some(i) => {
                program = cmd[..i].to_string();
                rest = cmd[i..].trim().to_string();
            }
            None => program = cmd.to_string(),
        }
    }

    let args: Vec<String> = if rest.is_empty() {
        Vec::new()
    } else {
        // 简单按空白切分，并去掉两端引号
        rest.split_whitespace()
            .map(|a| a.trim_matches('"').to_string())
            .collect()
    };
    (program, args)
}
