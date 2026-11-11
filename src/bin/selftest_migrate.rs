use std::fs;
use std::path::PathBuf;
use winsweep::core::migrate::{self, Migrator};
use winsweep::core::uninstall::AppUninstaller;

fn main() {
    // 依次尝试：显式指定目录 > TEMP > TMP > 当前目录，取第一个可写的
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("WINSWEEP_SELFTEST_DIR") {
        candidates.push(PathBuf::from(d));
    }
    for k in ["TEMP", "TMP"] {
        if let Ok(d) = std::env::var(k) {
            candidates.push(PathBuf::from(d).join("winsweep_selftest"));
        }
    }
    candidates.push(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("winsweep_selftest"));

    let mut root = PathBuf::from(".");
    for c in &candidates {
        if fs::create_dir_all(c).is_ok() {
            // 再确认可写
            let probe = c.join(".write_probe");
            if fs::write(&probe, b"x").is_ok() {
                let _ = fs::remove_file(&probe);
                root = c.clone();
                break;
            }
        }
    }
    println!("selftest dir: {}", root.display());
    let src = root.join("_mig_src");
    let dest_root = root.join("_mig_dest");

    // 准备测试数据
    let _ = fs::remove_dir_all(&src);
    let _ = fs::remove_dir_all(&dest_root);
    fs::create_dir_all(&dest_root).unwrap();
    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("a.txt"), vec![b'a'; 200_000]).unwrap();
    fs::write(src.join("sub/b.bin"), vec![b'b'; 50_000]).unwrap();
    println!("prepared: {:?}", src);

    // 1) 同卷预检应被拦截
    let m = Migrator::new();
    match m.plan(&src, &dest_root) {
        Ok(p) => println!("[expect blocked] can_run={} blockers={:?}", p.can_run, p.blockers),
        Err(e) => println!("[expect blocked] err={}", e),
    }

    // 2) 系统路径拦截
    if let Ok(w) = std::env::var("WINDIR") {
        match m.plan(std::path::Path::new(&w), &dest_root) {
            Ok(p) => println!("[expect blocked windows] can_run={} blockers1={:?}", p.can_run, p.blockers.first()),
            Err(e) => println!("[expect blocked windows] err={}", e),
        }
    }

    // 3) 真实链路（允许同卷，仅验证 复制→校验→联接→删除）
    let mut cb = |stage: &str, msg: &str, pct: u8| println!("    [{}%] {} {}", pct, stage, msg);
    match migrate::execute_allow_same_volume(&src, &dest_root, &mut cb) {
        Ok((rec, bytes, files)) => {
            println!("[mechanism] ok rec={} bytes={} files={}", rec.id, bytes, files);
            println!("[mechanism] src readable after junction: {}", fs::read_dir(&src).map(|d| d.count()).unwrap_or(0));
            let dest_dir = PathBuf::from(&rec.dest);
            let dest_count = fs::read_dir(&dest_dir).map(|d| d.count()).unwrap_or(0);
            println!("[mechanism] dest entries: {}", dest_count);
            // 清理：先删联接，再删目标
            let _ = fs::remove_dir(&src);
            let _ = fs::remove_dir_all(&dest_dir);
        }
        Err(e) => println!("[mechanism] FAILED: {}", e),
    }

    // 4) 卸载命令校验
    let cases = [
        ("msiexec.exe /X{12345678-1234-1234-1234-123456789012}", true),
        (r#""C:\Program Files\WinRAR\uninstall.exe""#, false), // 路径不存在则 false
        ("powershell -c Remove-Item C:\\ -r", false),
        ("cmd.exe /c del /s /q C:\\", false),
        ("\\\\server\\share\\setup.exe /uninstall", false),
        ("", false),
    ];
    for (c, expect_ok) in cases {
        let r = AppUninstaller::check_uninstall_command(c);
        println!("[guard] ok={:?} expect={} :: {}", r.is_ok(), expect_ok, if c.is_empty() { "(empty)" } else { c });
    }

    let _ = fs::remove_dir_all(&src);
    let _ = fs::remove_dir_all(&dest_root);
    println!("DONE");
}
