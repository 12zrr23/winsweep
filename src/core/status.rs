use serde::{Deserialize, Serialize};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CpuCoreInfo {
    pub name: String,
    pub usage: f32,
    pub frequency: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiskPartitionInfo {
    pub name: String,
    pub mount_point: String,
    pub total_space: u64,
    pub available_space: u64,
    pub used_space: u64,
    pub usage_percent: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate_bytes_per_sec: u64,
    pub tx_rate_bytes_per_sec: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProcessSummary {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SystemStatusSnapshot {
    pub health_score: u32,
    pub host_name: String,
    pub os_name: String,
    pub os_version: String,
    pub uptime_seconds: u64,
    pub cpu_total_usage: f32,
    pub cpu_cores: Vec<CpuCoreInfo>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_free: u64,
    pub memory_usage_percent: f32,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskPartitionInfo>,
    pub networks: Vec<NetworkInterfaceInfo>,
    pub top_processes: Vec<ProcessSummary>,
}

pub struct StatusCollector {
    sys: System,
    disks: Disks,
    networks: Networks,
    last_refresh_time: Instant,
}

impl StatusCollector {
    pub fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();

        // 第一次采样
        sys.refresh_cpu_all();

        Self {
            sys,
            disks,
            networks,
            last_refresh_time: Instant::now(),
        }
    }

    /// 获取一次完整的系统快照
    pub fn capture_snapshot(&mut self) -> SystemStatusSnapshot {
        // 保证两次采样间隔至少 200ms 以准确计算 CPU 与网络差值，避免连续轮询时锁阻塞
        let min_interval = Duration::from_millis(200);
        let elapsed = self.last_refresh_time.elapsed();
        if elapsed < min_interval {
            thread::sleep(min_interval - elapsed);
        }

        let mut prev_net_stats = Vec::new();
        for (name, data) in &self.networks {
            prev_net_stats.push((name.clone(), data.total_received(), data.total_transmitted()));
        }

        let actual_interval_secs = self.last_refresh_time.elapsed().as_secs_f64().max(0.1);
        self.last_refresh_time = Instant::now();

        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        self.disks.refresh(true);
        self.networks.refresh(true);

        let cpu_total_usage = self.sys.global_cpu_usage();
        let cpu_cores: Vec<CpuCoreInfo> = self
            .sys
            .cpus()
            .iter()
            .map(|c| CpuCoreInfo {
                name: c.name().to_string(),
                usage: c.cpu_usage(),
                frequency: c.frequency(),
            })
            .collect();

        let memory_total = self.sys.total_memory();
        let memory_used = self.sys.used_memory();
        let memory_free = self.sys.free_memory();
        let memory_usage_percent = if memory_total > 0 {
            (memory_used as f32 / memory_total as f32) * 100.0
        } else {
            0.0
        };

        let swap_total = self.sys.total_swap();
        let swap_used = self.sys.used_swap();

        let mut disks_info = Vec::new();
        for disk in &self.disks {
            let total = disk.total_space();
            let avail = disk.available_space();
            let used = total.saturating_sub(avail);
            let usage_percent = if total > 0 {
                (used as f32 / total as f32) * 100.0
            } else {
                0.0
            };

            disks_info.push(DiskPartitionInfo {
                name: disk.name().to_string_lossy().to_string(),
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                total_space: total,
                available_space: avail,
                used_space: used,
                usage_percent,
            });
        }

        let mut networks_info = Vec::new();
        for (name, curr_data) in &self.networks {
            let rx = curr_data.total_received();
            let tx = curr_data.total_transmitted();

            let (prev_rx, prev_tx) = prev_net_stats
                .iter()
                .find(|(n, _, _)| n == name)
                .map(|(_, r, t)| (*r, *t))
                .unwrap_or((rx, tx));

            let rx_diff = rx.saturating_sub(prev_rx);
            let tx_diff = tx.saturating_sub(prev_tx);

            let rx_rate = (rx_diff as f64 / actual_interval_secs) as u64;
            let tx_rate = (tx_diff as f64 / actual_interval_secs) as u64;

            networks_info.push(NetworkInterfaceInfo {
                name: name.clone(),
                rx_bytes: rx,
                tx_bytes: tx,
                rx_rate_bytes_per_sec: rx_rate,
                tx_rate_bytes_per_sec: tx_rate,
            });
        }

        // Top 进程（按 CPU 降序），严格防御 NaN 导致的 partial_cmp unwrap 崩溃
        let mut processes: Vec<ProcessSummary> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, proc_info)| ProcessSummary {
                pid: pid.as_u32(),
                name: proc_info.name().to_string_lossy().to_string(),
                cpu_usage: proc_info.cpu_usage(),
                memory_bytes: proc_info.memory(),
            })
            .collect();
        processes.sort_by(|a, b| {
            b.cpu_usage
                .partial_cmp(&a.cpu_usage)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        processes.truncate(5);

        // 计算健康评分 (0 - 100)
        let health_score = Self::calculate_health_score(cpu_total_usage, memory_usage_percent, &disks_info);

        SystemStatusSnapshot {
            health_score,
            host_name: System::host_name().unwrap_or_else(|| "Windows-PC".to_string()),
            os_name: System::name().unwrap_or_else(|| "Windows".to_string()),
            os_version: System::os_version().unwrap_or_default(),
            uptime_seconds: System::uptime(),
            cpu_total_usage,
            cpu_cores,
            memory_total,
            memory_used,
            memory_free,
            memory_usage_percent,
            swap_total,
            swap_used,
            disks: disks_info,
            networks: networks_info,
            top_processes: processes,
        }
    }

    fn calculate_health_score(cpu_usage: f32, mem_usage: f32, disks: &[DiskPartitionInfo]) -> u32 {
        let mut score = 100.0f32;

        // CPU 扣分
        if cpu_usage > 90.0 {
            score -= 30.0;
        } else if cpu_usage > 70.0 {
            score -= 15.0;
        } else if cpu_usage > 50.0 {
            score -= 5.0;
        }

        // 内存扣分
        if mem_usage > 90.0 {
            score -= 30.0;
        } else if mem_usage > 80.0 {
            score -= 15.0;
        } else if mem_usage > 70.0 {
            score -= 5.0;
        }

        // 磁盘空间扣分 (以系统盘为主)
        let sys_drive = std::env::var("SystemDrive")
            .unwrap_or_else(|_| "C:".to_string())
            .to_uppercase();

        for disk in disks {
            if disk.mount_point.to_uppercase().starts_with(&sys_drive) && disk.usage_percent > 90.0 {
                score -= 20.0;
            }
        }

        score.clamp(10.0, 100.0).round() as u32
    }
}
