use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::core::safety::SafetyGuard;

// ---------- 数据模型 ----------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VolumeInfo {
    pub mount: String,
    pub label: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub serial: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MigrationPlan {
    pub source: String,
    pub dest: String,
    pub source_bytes: u64,
    pub source_files: u64,
    pub target_free_bytes: u64,
    pub required_bytes: u64,
    pub source_volume: String,
    pub target_volume: String,
    pub method: String,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub can_run: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MigrationRecord {
    pub id: String,
    pub at: String,
    pub source: String,
    pub dest: String,
    pub bytes: u64,
    pub files: u64,
    pub status: String,
    pub note: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct JobState {
    pub running: bool,
    pub stage: String,
    pub message: String,
    pub percent: u8,
    pub finished: bool,
    pub success: bool,
    pub record_id: String,
}

// ---------- Win32：卷序列号 ----------

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetVolumePathNameW(
        lpsz_file_name: *const u16,
        lpsz_volume_path_name: *mut u16,
        cch_buffer_length: u32,
    ) -> i32;
    fn GetDiskFreeSpaceExW(
        lp_directory_name: *const u16,
        lp_free_bytes_available_to_caller: *mut u64,
        lp_total_number_of_bytes: *mut u64,
        lp_total_number_of_free_bytes: *mut u64,
    ) -> i32;
    fn GetVolumeInformationW(
        lp_root: *const u16,
        lp_volume_name: *mut u16,
        n_volume_name_size: u32,
        lp_volume_serial: *mut u32,
        lp_max_component_len: *mut u32,
        lp_file_system_flags: *mut u32,
        lp_file_system_name: *mut u16,
        n_file_system_name_size: u32,
    ) -> i32;
}

/// 取路径所属的真实卷根（目录挂载点会解析成对应卷的 GUID 路径）
fn volume_root_of(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 512];
    let ok = unsafe { GetVolumePathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if ok == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
}

/// 目标路径所在卷的可用/总空间（按路径取，目录挂载点也准确）
fn free_space_of(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut avail: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut total_free)
    };
    if ok == 0 { None } else { Some((avail, total)) }
}

fn volume_serial_of(path: &Path) -> u32 {
    use std::os::windows::ffi::OsStrExt;

    // 优先用系统 API 解析真实卷根，这样 U 盘/目录挂载点/VHD 都能识别
    let root = match volume_root_of(path) {
        Some(r) => r,
        None => {
            let mut r = path.components().next().map(|c| match c {
                Component::Prefix(p) => PathBuf::from(p.as_os_str()),
                _ => PathBuf::from("\\"),
            }).unwrap_or_else(|| PathBuf::from("\\"));
            let s = r.to_string_lossy().to_string();
            if !s.ends_with('\\') {
                r = PathBuf::from(format!("{}\\", s));
            }
            r
        }
    };

    let wide: Vec<u16> = root
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut serial: u32 = 0;
    let mut max_len: u32 = 0;
    let mut flags: u32 = 0;
    let mut fs_name = [0u16; 32];
    let ok = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            &mut max_len,
            &mut flags,
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    };
    if ok == 0 {
        0
    } else {
        serial
    }
}

// ---------- 工具 ----------

pub fn volumes() -> Vec<VolumeInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut out: Vec<VolumeInfo> = Vec::new();
    for d in &disks {
        let mount = d.mount_point().to_string_lossy().to_string();
        if mount.is_empty() {
            continue;
        }
        let serial = volume_serial_of(d.mount_point());
        out.push(VolumeInfo {
            label: d.name().to_string_lossy().to_string(),
            mount,
            total_bytes: d.total_space(),
            free_bytes: d.available_space(),
            serial,
        });
    }
    out.sort_by(|a, b| a.mount.cmp(&b.mount));
    out
}

fn dir_stats(path: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    for e in WalkDir::new(path).follow_links(false).into_iter().filter_map(|x| x.ok()) {
        if e.file_type().is_file() {
            if let Ok(m) = e.metadata() {
                bytes += m.len();
                files += 1;
            }
        }
    }
    (bytes, files)
}

fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\");
    let s = s.trim_end_matches('\\').to_string();
    s.to_lowercase()
}

fn is_under(child: &str, parent: &str) -> bool {
    child == parent || child.starts_with(&format!("{}\\", parent))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect()
}

fn config_dir() -> PathBuf {
    SafetyGuard::get_config_dir()
}

fn manifest_dir() -> PathBuf {
    config_dir().join("migrations")
}

fn now_id() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

fn now_text() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn write_manifest(rec: &MigrationRecord) -> Result<(), String> {
    let dir = manifest_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let p = dir.join(format!("{}.json", rec.id));
    let s = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?;
    fs::write(&p, s).map_err(|e| e.to_string())
}

fn update_manifest(rec: &MigrationRecord) {
    let _ = write_manifest(rec);
}

pub fn list_migrations() -> Vec<MigrationRecord> {
    let dir = manifest_dir();
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            if let Ok(s) = fs::read_to_string(e.path()) {
                if let Ok(r) = serde_json::from_str::<MigrationRecord>(&s) {
                    out.push(r);
                }
            }
        }
    }
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}

fn find_record(id: &str) -> Option<MigrationRecord> {
    list_migrations().into_iter().find(|r| r.id == id)
}

// ---------- 预检 ----------

pub struct Migrator {
    guard: SafetyGuard,
}

impl Migrator {
    pub fn new() -> Self {
        Self { guard: SafetyGuard::new() }
    }

    pub fn plan(&self, source: &Path, target_root: &Path) -> Result<MigrationPlan, String> {
        let mut blockers: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        let src_abs = if source.is_absolute() {
            source.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(source)
        };

        if !src_abs.exists() {
            return Err(format!("源路径不存在：{}", src_abs.display()));
        }
        if !src_abs.is_dir() {
            return Err("目前只支持迁移文件夹；单个文件请直接移动".to_string());
        }

        let src_norm = norm(&src_abs);
        let tgt_root_abs = if target_root.is_absolute() {
            target_root.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(target_root)
        };
        if !tgt_root_abs.exists() {
            return Err(format!("目标盘路径不存在：{}", tgt_root_abs.display()));
        }

        // 目标可写性探测：跨盘迁移最常见的失败点是卷根没有写权限
        let probe = tgt_root_abs.join(".winsweep_write_test");
        match fs::write(&probe, b"ok") {
            Ok(_) => {
                let _ = fs::remove_file(&probe);
            }
            Err(e) => blockers.push(format!(
                "目标目录不可写（{}）：请换一个有写权限的文件夹，例如 D:\\Store，或改用管理员身份运行",
                e
            )),
        }

        // ---- 系统关键路径拦截 ----
        let mut protected: Vec<String> = Vec::new();
        if let Ok(w) = std::env::var("WINDIR") {
            protected.push(norm(Path::new(&w)));
        }
        if let Ok(p) = std::env::var("ProgramFiles") {
            protected.push(norm(Path::new(&p)));
        }
        if let Ok(p) = std::env::var("ProgramFiles(x86)") {
            protected.push(norm(Path::new(&p)));
        }
        if let Ok(p) = std::env::var("ProgramData") {
            protected.push(norm(Path::new(&p)));
            protected.push(norm(&Path::new(&p).join("Microsoft")));
        }
        if let Ok(u) = std::env::var("USERPROFILE") {
            let up = norm(Path::new(&u));
            // 用户根自身、以及 AppData 下的系统关键目录
            if src_norm == up {
                blockers.push("不能迁移整个用户目录（C:\\Users\\你），请选择其下的具体文件夹".to_string());
            }
            protected.push(norm(&Path::new(&u).join("AppData\\Local\\Microsoft")));
            protected.push(norm(&Path::new(&u).join("AppData\\Roaming\\Microsoft")));
            protected.push(norm(&Path::new(&u).join("AppData\\Local\\Packages")));
        }
        for p in &protected {
            if is_under(&src_norm, p) || is_under(p, &src_norm) {
                blockers.push(format!(
                    "该路径属于系统或程序核心目录（{}），迁移可能破坏系统，已阻止",
                    p
                ));
                break;
            }
        }

        // 盘符根目录
        if src_abs.parent().is_none() || src_norm.ends_with(':') {
            blockers.push("不能迁移整个盘符根目录".to_string());
        }

        // 安全守卫（复用黑名单/白名单）
        if let Err(e) = self.guard.is_safe_to_delete(&src_abs) {
            warnings.push(format!("安全守卫提示：{}", e));
        }

        // ---- 目标位置 ----
        let vol_name = src_abs
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "moved".to_string());
        let drive_tag = src_abs
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().replace(':', ""))
            .unwrap_or_else(|| "x".to_string());
        let dest = tgt_root_abs
            .join("WinSweepMoved")
            .join(format!("{}-{}", sanitize(&vol_name), drive_tag));

        if dest.exists() {
            blockers.push(format!("目标位置已存在同名目录：{}，请先改名或清理", dest.display()));
        }
        if is_under(&norm(&dest), &src_norm) {
            blockers.push("目标不能位于源目录内部".to_string());
        }
        if is_under(&src_norm, &norm(&dest)) {
            blockers.push("源不能位于目标目录内部".to_string());
        }

        // ---- 同卷检查（用卷序列号，subst/挂载点也能识别） ----
        let src_serial = volume_serial_of(&src_abs);
        let tgt_serial = volume_serial_of(&tgt_root_abs);
        let src_vol = src_abs.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
        let tgt_vol = tgt_root_abs.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
        if src_serial != 0 && src_serial == tgt_serial {
            blockers.push(format!(
                "源与目标是同一个卷（{} → {}），跨盘迁移才有意义",
                src_vol, tgt_vol
            ));
        }

        // ---- 容量与统计（只在没有阻断项时统计，避免大目录白扫） ----
        let (bytes, files) = if blockers.is_empty() {
            dir_stats(&src_abs)
        } else {
            (0u64, 0u64)
        };
        let target_free = free_space_of(&tgt_root_abs)
            .map(|(avail, _)| avail)
            .unwrap_or_else(|| {
                volumes()
                    .into_iter()
                    .find(|v| is_under(&norm(&tgt_root_abs), &norm(Path::new(&v.mount))))
                    .map(|v| v.free_bytes)
                    .unwrap_or(0)
            });
        let required = bytes + (bytes / 20) + 512 * 1024 * 1024; // 5% + 512MB 余量
        if target_free < required {
            blockers.push(format!(
                "目标剩余空间不足：需要约 {}，当前可用 {}",
                human(required),
                human(target_free)
            ));
        }
        if blockers.is_empty() && files == 0 {
            warnings.push("源目录里没有文件（只有空文件夹），迁移意义不大".to_string());
        }
        warnings.push("迁移完成后会在原位置建立“目录联接（Junction）”，原路径保持不变，PATH / 快捷方式 / 注册表引用继续有效。".to_string());
        warnings.push("复制并逐项校验（文件数 + 字节数）通过后才会删除源目录；任何一步失败都会保留源数据。".to_string());
        warnings.push("正在被程序占用的文件可能复制失败；建议先关闭相关程序。".to_string());

        let can_run = blockers.is_empty();

        Ok(MigrationPlan {
            source: src_abs.to_string_lossy().to_string(),
            dest: dest.to_string_lossy().to_string(),
            source_bytes: bytes,
            source_files: files,
            target_free_bytes: target_free,
            required_bytes: required,
            source_volume: src_vol,
            target_volume: tgt_vol,
            method: "robocopy 复制 → 校验 → 目录联接".to_string(),
            blockers,
            warnings,
            can_run,
        })
    }

    // ---------- 执行 ----------

    pub fn execute(&self, plan: &MigrationPlan, progress: &mut dyn FnMut(&str, &str, u8)) -> Result<MigrationRecord, String> {
        if !plan.can_run {
            return Err("预检未通过，不能执行".to_string());
        }
        let src = PathBuf::from(&plan.source);
        let dest = PathBuf::from(&plan.dest);

        progress("prepare", "创建目标目录…", 5);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目标父目录失败：{}", e))?;
        }

        let rec_id = now_id();
        let mut rec = MigrationRecord {
            id: rec_id.clone(),
            at: now_text(),
            source: plan.source.clone(),
            dest: plan.dest.clone(),
            bytes: plan.source_bytes,
            files: plan.source_files,
            status: "copying".to_string(),
            note: String::new(),
        };
        write_manifest(&rec)?;

        progress("copy", "正在复制数据（robocopy）…", 20);
        if let Err(e) = run_robocopy(&src, &dest) {
            rec.status = "failed".to_string();
            rec.note = format!("复制失败（文件可能被占用，或目标目录不可写）：{}。源数据未改动", e);
            update_manifest(&rec);
            let _ = fs::remove_dir_all(&dest);
            return Err(rec.note.clone());
        }

        progress("verify", "复制完成，正在逐项校验…", 55);
        let (src_bytes, src_files) = dir_stats(&src);
        let (dst_bytes, dst_files) = dir_stats(&dest);
        if src_bytes != dst_bytes || src_files != dst_files {
            rec.status = "failed".to_string();
            rec.note = format!(
                "校验不一致：源 {} 个文件/{}，目标 {} 个文件/{}。已保留源数据，目标副本保留在 {}",
                src_files, human(src_bytes), dst_files, human(dst_bytes), dest.display()
            );
            update_manifest(&rec);
            return Err(rec.note.clone());
        }

        // 把源目录临时改名，腾出原路径
        progress("swap", "校验通过，正在切换原路径…", 70);
        let old = PathBuf::from(format!("{}__winsweep_old", plan.source));
        if old.exists() {
            let _ = fs::remove_dir_all(&old);
        }
        if let Err(e) = fs::rename(&src, &old) {
            rec.status = "failed".to_string();
            rec.note = format!("原目录改名失败（可能被占用）：{}；源数据完好", e);
            update_manifest(&rec);
            return Err(rec.note.clone());
        }

        // 建立目录联接
        progress("link", "正在建立目录联接（Junction）…", 82);
        if !make_junction(&src, &dest) {
            // 回滚：恢复原名
            let _ = fs::rename(&old, &src);
            rec.status = "failed".to_string();
            rec.note = "建立目录联接失败，已回滚到迁移前状态".to_string();
            update_manifest(&rec);
            return Err(rec.note.clone());
        }

        // 验证联接可读
        progress("check", "验证联接可用性…", 90);
        let linked_ok = fs::read_dir(&src).is_ok();
        if !linked_ok {
            let _ = fs::remove_dir(&src); // 移除联接
            let _ = fs::rename(&old, &src);
            rec.status = "failed".to_string();
            rec.note = "联接验证失败，已回滚".to_string();
            update_manifest(&rec);
            return Err(rec.note.clone());
        }

        progress("cleanup", "正在删除原目录副本…", 95);
        let _ = fs::remove_dir_all(&old);

        rec.status = "done".to_string();
        rec.note = "迁移完成，原路径已指向新位置的目录联接".to_string();
        update_manifest(&rec);

        progress("done", "迁移完成", 100);
        Ok(rec)
    }

    /// 撤销：移除联接并把数据搬回原位置
    pub fn undo(&self, id: &str, progress: &mut dyn FnMut(&str, &str, u8)) -> Result<(), String> {
        let mut rec = find_record(id).ok_or_else(|| "找不到该迁移记录".to_string())?;
        if rec.status != "done" {
            return Err(format!("该记录状态为 {}，只有已完成的可撤销", rec.status));
        }
        let src = PathBuf::from(&rec.source);
        let dest = PathBuf::from(&rec.dest);
        if !dest.exists() {
            return Err("目标目录已不存在，无法撤销".to_string());
        }

        progress("undo-space", "检查原盘空间…", 10);
        let (bytes, _files) = dir_stats(&dest);
        let target_free = free_space_of(Path::new(&rec.source))
            .map(|(avail, _)| avail)
            .unwrap_or(0);
        if target_free < bytes + (bytes / 20) {
            return Err(format!(
                "原盘空间不足，撤销需要约 {}，当前可用 {}",
                human(bytes + bytes / 20),
                human(target_free)
            ));
        }

        progress("undo-unlink", "移除目录联接…", 25);
        if src.exists() {
            fs::remove_dir(&src).map_err(|e| format!("移除联接失败：{}", e))?;
        }

        progress("undo-copy", "正在把数据复制回原位置…", 45);
        if let Err(e) = run_robocopy(&dest, &src) {
            return Err(format!("复制回原位置失败（{}），目标数据保留", e));
        }

        progress("undo-verify", "校验数据完整性…", 75);
        let (a_bytes, a_files) = dir_stats(&dest);
        let (b_bytes, b_files) = dir_stats(&src);
        if a_bytes != b_bytes || a_files != b_files {
            return Err("回迁校验不一致，已保留目标副本，请手工核对".to_string());
        }

        progress("undo-clean", "清理目标副本…", 92);
        let _ = fs::remove_dir_all(&dest);

        rec.status = "undone".to_string();
        rec.note = "已撤销迁移，数据回到原位置".to_string();
        update_manifest(&rec);
        progress("done", "已撤销", 100);
        Ok(())
    }
}

fn human(b: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if b >= GB {
        format!("{:.2} GB", b as f64 / GB as f64)
    } else if b >= MB {
        format!("{:.1} MB", b as f64 / MB as f64)
    } else if b >= KB {
        format!("{:.0} KB", b as f64 / KB as f64)
    } else {
        format!("{} B", b)
    }
}

fn run_robocopy(src: &Path, dst: &Path) -> Result<(), String> {
    // /E 含空目录 /COPY:DAT 数据+属性+时间 /DCOPY:DAT 目录时间 /XJ 跳过联接避免环
    // /R:1 /W:1 重试一次 /NFL /NDL /NJH /NJS /NP 精简输出
    let attempts: [&[&str]; 2] = [
        &["/E", "/COPY:DAT", "/DCOPY:DAT", "/XJ", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NJS", "/NP"],
        &["/E", "/COPY:DAT", "/DCOPY:DAT", "/XJ", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NJS", "/NP", "/B"],
    ];
    let mut last_log = String::new();
    for args in attempts.iter() {
        match Command::new("robocopy").arg(src).arg(dst).args(*args).output() {
            Ok(o) => {
                let code = o.status.code().unwrap_or(16);
                let out = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                // robocopy 0-7 视为成功；但有的环境即使访问被拒也返回 0，
                // 因此额外要求“目标里确实出现了文件”，否则按失败处理并带上日志
                let dest_has_files = WalkDir::new(dst)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .any(|e| e.file_type().is_file());
                if code < 8 && dest_has_files {
                    return Ok(());
                }
                let brief: String = out
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(6)
                    .collect::<Vec<_>>()
                    .join(" / ");
                last_log = format!("robocopy code={} {}", code, brief);
            }
            Err(e) => last_log = format!("无法启动 robocopy：{}", e),
        }
    }
    Err(if last_log.is_empty() { "robocopy 复制失败".to_string() } else { last_log })
}
fn make_junction(link: &Path, target: &Path) -> bool {
    // 用 PowerShell 的 New-Item -ItemType Junction：不需要管理员，且对含空格路径更稳
    let link_s = link.to_string_lossy().replace('\'', "''");
    let target_s = target.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null; exit 0",
        link_s, target_s
    );
    match Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
    {
        Ok(o) if o.status.success() => return true,
        Ok(o) => {
            eprintln!(
                "[migrate] junction via PowerShell failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Err(e) => eprintln!("[migrate] cannot run powershell: {}", e),
    }

    // 回退：符号链接（需要开发者模式或管理员）
    let script2 = format!(
        "$ErrorActionPreference='Stop'; New-Item -ItemType SymbolicLink -Path '{}' -Target '{}' | Out-Null; exit 0",
        link_s, target_s
    );
    match Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script2])
        .output()
    {
        Ok(o) => {
            if o.status.success() {
                return true;
            }
            eprintln!(
                "[migrate] symlink fallback failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            false
        }
        Err(_) => false,
    }
}
/// 供自检：在允许同卷的情况下跑完整链路（仅用于本地测试二进制）
#[allow(dead_code)]
pub fn execute_allow_same_volume(
    src: &Path,
    dest_root: &Path,
    progress: &mut dyn FnMut(&str, &str, u8),
) -> Result<(MigrationRecord, u64, u64), String> {
    let m = Migrator::new();
    let (bytes, files) = dir_stats(src);
    let name = src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "t".into());
    let dest = dest_root.join("WinSweepMoved").join(format!("{}-test", sanitize(&name)));
    if dest.exists() {
        let _ = fs::remove_dir_all(&dest);
    }
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;

    let mut rec = MigrationRecord {
        id: format!("test-{}", now_id()),
        at: now_text(),
        source: src.to_string_lossy().to_string(),
        dest: dest.to_string_lossy().to_string(),
        bytes,
        files,
        status: "copying".into(),
        note: String::new(),
    };
    write_manifest(&rec)?;

    progress("copy", "copying", 30);
    if let Err(e) = run_robocopy(src, &dest) {
        return Err(e);
    }
    let (sb, sf) = dir_stats(src);
    let (db, df) = dir_stats(&dest);
    if sb != db || sf != df {
        return Err(format!("verify mismatch src {}/{} dst {}/{}", sf, sb, df, db));
    }
    progress("swap", "swapping", 65);
    let old = PathBuf::from(format!("{}__winsweep_old", src.display()));
    if old.exists() {
        let _ = fs::remove_dir_all(&old);
    }
    fs::rename(src, &old).map_err(|e| format!("rename src failed: {}", e))?;
    progress("link", "linking", 80);
    if !make_junction(src, &dest) {
        let _ = fs::rename(&old, src);
        return Err("junction failed".into());
    }
    progress("cleanup", "cleanup", 95);
    let _ = fs::remove_dir_all(&old);
    rec.status = "done".into();
    rec.note = "test".into();
    update_manifest(&rec);
    progress("done", "done", 100);
    let _ = m;
    Ok((rec, bytes, files))
}
