use std::process::Command;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OptimizationTaskResult {
    pub task_id: String,
    pub task_name: String,
    pub success: bool,
    pub message: String,
}

pub struct SystemOptimizer;

impl SystemOptimizer {
    pub fn new() -> Self {
        Self
    }

    /// 刷新 Windows 本地 DNS 解析缓存
    pub fn flush_dns(&self) -> OptimizationTaskResult {
        let output = Command::new("ipconfig")
            .arg("/flushdns")
            .output();

        match output {
            Ok(out) if out.status.success() => OptimizationTaskResult {
                task_id: "flush_dns".to_string(),
                task_name: "Flush DNS Cache".to_string(),
                success: true,
                message: "Successfully flushed the DNS Resolver Cache.".to_string(),
            },
            Ok(out) => {
                let err_msg = Self::decode_output(&out.stderr, &out.stdout);
                OptimizationTaskResult {
                    task_id: "flush_dns".to_string(),
                    task_name: "Flush DNS Cache".to_string(),
                    success: false,
                    message: if err_msg.is_empty() { "Command failed with non-zero exit code".to_string() } else { err_msg },
                }
            }
            Err(e) => OptimizationTaskResult {
                task_id: "flush_dns".to_string(),
                task_name: "Flush DNS Cache".to_string(),
                success: false,
                message: format!("Failed to run ipconfig: {}", e),
            },
        }
    }

    /// 刷新 ARP 网络协议缓存
    pub fn flush_arp(&self) -> OptimizationTaskResult {
        let output = Command::new("arp")
            .arg("-d")
            .arg("*")
            .output();

        match output {
            Ok(out) if out.status.success() => OptimizationTaskResult {
                task_id: "flush_arp".to_string(),
                task_name: "Flush ARP Cache".to_string(),
                success: true,
                message: "Successfully cleared ARP cache entries.".to_string(),
            },
            Ok(out) => {
                let err_msg = Self::decode_output(&out.stderr, &out.stdout);
                OptimizationTaskResult {
                    task_id: "flush_arp".to_string(),
                    task_name: "Flush ARP Cache".to_string(),
                    success: false,
                    message: if err_msg.is_empty() {
                        "Failed to clear ARP cache (Administrator privilege may be required)".to_string()
                    } else {
                        err_msg
                    },
                }
            }
            Err(e) => OptimizationTaskResult {
                task_id: "flush_arp".to_string(),
                task_name: "Flush ARP Cache".to_string(),
                success: false,
                message: format!("Failed to clear ARP cache: {}", e),
            },
        }
    }

    /// 刷新 Windows 桌面与资源管理器图标缓存
    pub fn refresh_icon_cache(&self) -> OptimizationTaskResult {
        let output = Command::new("ie4uinit.exe")
            .arg("-show")
            .output();

        match output {
            Ok(_) => OptimizationTaskResult {
                task_id: "refresh_icon_cache".to_string(),
                task_name: "Refresh Explorer Icon Cache".to_string(),
                success: true,
                message: "Explorer icon database refresh signal dispatched.".to_string(),
            },
            Err(e) => OptimizationTaskResult {
                task_id: "refresh_icon_cache".to_string(),
                task_name: "Refresh Explorer Icon Cache".to_string(),
                success: false,
                message: format!("Failed to refresh icon cache: {}", e),
            },
        }
    }

    /// 运行所有常规优化任务
    pub fn run_all_optimizations(&self) -> Vec<OptimizationTaskResult> {
        vec![
            self.flush_dns(),
            self.flush_arp(),
            self.refresh_icon_cache(),
        ]
    }

    fn decode_output(stderr: &[u8], stdout: &[u8]) -> String {
        let bytes = if !stderr.is_empty() { stderr } else { stdout };
        let s = String::from_utf8_lossy(bytes).trim().to_string();
        if s.contains('\u{FFFD}') {
            // 中文 Windows 常见 GBK 编码场景处理
            s.chars().filter(|c| !c.is_control() && *c != '\u{FFFD}').collect()
        } else {
            s
        }
    }
}
