use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use tiny_http::{Header, Method, Response, Server};
use crate::core::analyzer::DiskAnalyzer;
use crate::core::cleaner::SystemCleaner;
use crate::core::installer::InstallerScanner;
use crate::core::optimizer::SystemOptimizer;
use crate::core::purge::ProjectPurgeScanner;
use crate::core::status::StatusCollector;
use crate::core::uninstall::AppUninstaller;
use crate::core::migrate::{self, JobState, Migrator};

pub struct GuiServer {
    port: u16,
}

type JobRef = Arc<Mutex<JobState>>;

impl GuiServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub fn start(&self) {
        // 智能端口绑定：如果指定端口被占用，自动递增重试
        let mut server = None;
        let mut actual_port = self.port;
        for p in self.port..self.port + 10 {
            let addr = format!("127.0.0.1:{}", p);
            match Server::http(&addr) {
                Ok(s) => {
                    server = Some(s);
                    actual_port = p;
                    break;
                }
                Err(e) => {
                    eprintln!("Port {} is unavailable ({}), trying next...", p, e);
                }
            }
        }

        let server = match server {
            Some(s) => Arc::new(s),
            None => {
                eprintln!("Failed to bind to any port in range {}..{}", self.port, self.port + 10);
                return;
            }
        };

        let status_collector = Arc::new(Mutex::new(StatusCollector::new()));
        let migrate_job: JobRef = Arc::new(Mutex::new(JobState::default()));
        let url = format!("http://127.0.0.1:{}", actual_port);
        println!("WinSweep GUI Dashboard running at: {}", url);

        // 尝试自动调起默认浏览器打开 GUI
        let _ = std::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(format!("Start-Process '{}'", url))
            .spawn();

        loop {
            match server.recv() {
                Ok(request) => {
                    let collector_clone = Arc::clone(&status_collector);
                    let job_clone = Arc::clone(&migrate_job);
                    thread::spawn(move || {
                        Self::handle_request(request, collector_clone, job_clone);
                    });
                }
                Err(e) => {
                    eprintln!("Error receiving request: {}", e);
                    break;
                }
            }
        }
    }

    fn url_decode(s: &str) -> String {
        let mut bytes = Vec::new();
        let mut chars = s.bytes();
        while let Some(b) = chars.next() {
            if b == b'%' {
                let h1 = chars.next().unwrap_or(b'0');
                let h2 = chars.next().unwrap_or(b'0');
                if let Ok(hex_byte) = u8::from_str_radix(
                    &format!("{}{}", h1 as char, h2 as char),
                    16,
                ) {
                    bytes.push(hex_byte);
                }
            } else if b == b'+' {
                bytes.push(b' ');
            } else {
                bytes.push(b);
            }
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn get_query_param(url: &str, key: &str) -> Option<String> {
        if let Some(pos) = url.find('?') {
            let query = &url[pos + 1..];
            for pair in query.split('&') {
                let mut parts = pair.splitn(2, '=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    if k == key {
                        return Some(Self::url_decode(v));
                    }
                }
            }
        }
        None
    }

    fn parse_src_tgt(body: &str) -> (Option<String>, Option<String>) {
        serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .map(|v| {
                (
                    v["source"].as_str().map(|s| s.to_string()),
                    v["target"].as_str().map(|s| s.to_string()),
                )
            })
            .unwrap_or((None, None))
    }

    fn finish_job(job: &JobRef, ok: bool, rec_id: &str, msg: &str) {
        if let Ok(mut g) = job.lock() {
            g.running = false;
            g.finished = true;
            g.success = ok;
            g.stage = if ok { "done".to_string() } else { "failed".to_string() };
            g.message = msg.to_string();
            g.percent = 100;
            g.record_id = rec_id.to_string();
        }
    }

    fn handle_request(mut request: tiny_http::Request, collector: Arc<Mutex<StatusCollector>>, job: JobRef) {
        let full_url = request.url().to_string();
        let url_path = full_url.split('?').next().unwrap_or("/").to_string();
        let method = request.method().clone();

        // 读取 POST 请求体
        let mut post_body = String::new();
        if method == Method::Post {
            let _ = request.as_reader().read_to_string(&mut post_body);
        }

        // 处理 CORS 预检
        if method == Method::Options {
            let response = Response::empty(204)
                .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
                .with_header(Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]).unwrap())
                .with_header(Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"*"[..]).unwrap());
            let _ = request.respond(response);
            return;
        }

        // 首页 HTML
        if method == Method::Get && (url_path == "/" || url_path == "/index.html") {
            let html = include_str!("../ui/index.html");
            let response = Response::from_string(html)
                .with_status_code(200)
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap())
                .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
            let _ = request.respond(response);
            return;
        }

        // API 路由
        let (status_code, content_type, body) = match (method, url_path.as_str()) {
            (Method::Get, "/api/status") => {
                let mut guard = match collector.lock() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                let snapshot = guard.capture_snapshot();
                (200, "application/json", serde_json::to_string(&snapshot).unwrap_or_default())
            }

            (Method::Get, "/api/clean/scan") => {
                let cleaner = SystemCleaner::new();
                let summaries = cleaner.scan_summaries();
                (200, "application/json", serde_json::to_string(&summaries).unwrap_or_default())
            }

            (Method::Post, "/api/clean/execute") => {
                let cleaner = SystemCleaner::new();
                let categories: Vec<String> = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&post_body) {
                    v["categories"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|str| str.to_string())).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let (freed, count) = if categories.is_empty() {
                    cleaner.clean_all(false)
                } else {
                    cleaner.clean_selected(&categories, false)
                };
                let res = serde_json::json!({ "freed_bytes": freed, "freed_count": count });
                (200, "application/json", res.to_string())
            }

            (Method::Post, "/api/clean/admin") => {
                let cleaner = SystemCleaner::new();
                let success = cleaner.clean_as_admin();
                let res = serde_json::json!({ "success": success });
                (200, "application/json", res.to_string())
            }

            (Method::Get, "/api/purge/scan") => {
                let purge = ProjectPurgeScanner::new();
                let items = if let Some(path_str) = Self::get_query_param(&full_url, "path") {
                    purge.scan_path(&PathBuf::from(path_str))
                } else {
                    purge.scan_default_workspace()
                };
                (200, "application/json", serde_json::to_string(&items).unwrap_or_default())
            }

            (Method::Post, "/api/purge/execute") => {
                let purge = ProjectPurgeScanner::new();
                let paths: Vec<PathBuf> = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&post_body) {
                    v["paths"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(PathBuf::from)).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let (freed, count) = if paths.is_empty() {
                    let items = purge.scan_default_workspace();
                    let eligible: Vec<_> = items.into_iter().filter(|a| !a.is_recently_active).collect();
                    purge.purge_artifacts(&eligible, false)
                } else {
                    purge.purge_by_paths(&paths, false)
                };
                let res = serde_json::json!({ "freed_bytes": freed, "freed_count": count });
                (200, "application/json", res.to_string())
            }

            (Method::Get, "/api/installer/scan") => {
                let installer = InstallerScanner::new();
                let items = if let Some(path_str) = Self::get_query_param(&full_url, "path") {
                    installer.scan_custom_path(&PathBuf::from(path_str))
                } else {
                    installer.scan_all()
                };
                (200, "application/json", serde_json::to_string(&items).unwrap_or_default())
            }

            (Method::Post, "/api/installer/execute") => {
                let installer = InstallerScanner::new();
                let paths: Vec<PathBuf> = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&post_body) {
                    v["paths"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(PathBuf::from)).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let (freed, count) = if paths.is_empty() {
                    let items = installer.scan_all();
                    installer.clean_installers(&items, false)
                } else {
                    installer.clean_by_paths(&paths, false)
                };
                let res = serde_json::json!({ "freed_bytes": freed, "freed_count": count });
                (200, "application/json", res.to_string())
            }

            (Method::Get, "/api/optimize") | (Method::Post, "/api/optimize") => {
                let optimizer = SystemOptimizer::new();
                let res = optimizer.run_all_optimizations();
                (200, "application/json", serde_json::to_string(&res).unwrap_or_default())
            }

            (Method::Get, "/api/uninstall/list") => {
                let uninstaller = AppUninstaller::new();
                let apps = if let Some(q) = Self::get_query_param(&full_url, "q") {
                    uninstaller.search_apps(&q)
                } else {
                    uninstaller.list_installed_apps()
                };
                (200, "application/json", serde_json::to_string(&apps).unwrap_or_default())
            }

            (Method::Get, "/api/analyze") | (Method::Post, "/api/analyze") => {
                let analyzer = DiskAnalyzer::new();
                let target = Self::get_query_param(&full_url, "path")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::var("USERPROFILE")
                            .map(PathBuf::from)
                            .unwrap_or_else(|_| PathBuf::from("."))
                    });
                let report = analyzer.analyze_path(&target, 15);
                (200, "application/json", serde_json::to_string(&report).unwrap_or_default())
            }

            (Method::Post, "/api/uninstall/run") => {
                let v: serde_json::Value = serde_json::from_str(&post_body).unwrap_or_default();
                let name = v["name"].as_str().unwrap_or_default().to_string();
                let cmd = v["uninstall_string"].as_str().unwrap_or_default().to_string();
                if name.is_empty() || cmd.is_empty() {
                    (400, "application/json", serde_json::json!({ "error": "缺少应用名称或卸载命令" }).to_string())
                } else {
                    let u = AppUninstaller::new();
                    match u.run_uninstall(&name, &cmd) {
                        Ok(msg) => (200, "application/json", serde_json::json!({ "ok": true, "message": msg }).to_string()),
                        Err(e) => (400, "application/json", serde_json::json!({ "ok": false, "error": e }).to_string()),
                    }
                }
            }

            // ---------- 文件迁移 ----------
            (Method::Get, "/api/migrate/presets") => {
                let mut out: Vec<serde_json::Value> = Vec::new();
                if let Ok(profile) = std::env::var("USERPROFILE") {
                    let base = PathBuf::from(&profile);
                    for name in ["Downloads", "Documents", "Desktop", "Videos", "Pictures", "Music"] {
                        let p = base.join(name);
                        if p.exists() {
                            out.push(serde_json::json!({ "label": name, "path": p.to_string_lossy() }));
                        }
                    }
                    for (label, rel) in [
                        ("Docker 数据", "AppData\\Local\\Docker"),
                        ("npm 缓存", "AppData\\Local\\npm-cache"),
                        ("pip 缓存", "AppData\\Local\\pip"),
                    ] {
                        let p = base.join(rel);
                        if p.exists() {
                            out.push(serde_json::json!({ "label": label, "path": p.to_string_lossy() }));
                        }
                    }
                }
                (200, "application/json", serde_json::to_string(&out).unwrap_or_default())
            }

            (Method::Get, "/api/migrate/volumes") => {
                let v = migrate::volumes();
                (200, "application/json", serde_json::to_string(&v).unwrap_or_default())
            }

            (Method::Post, "/api/migrate/plan") => {
                let (src, tgt) = Self::parse_src_tgt(&post_body);
                let m = Migrator::new();
                match (src, tgt) {
                    (Some(s), Some(t)) => match m.plan(&PathBuf::from(s), &PathBuf::from(t)) {
                        Ok(plan) => (200, "application/json", serde_json::to_string(&plan).unwrap_or_default()),
                        Err(e) => (400, "application/json", serde_json::json!({ "error": e }).to_string()),
                    },
                    _ => (400, "application/json", serde_json::json!({ "error": "缺少 source 或 target" }).to_string()),
                }
            }

            (Method::Post, "/api/migrate/execute") => {
                let (src, tgt) = Self::parse_src_tgt(&post_body);
                let mut st = match job.lock() { Ok(g) => g, Err(p) => p.into_inner() };
                if st.running {
                    (409, "application/json", serde_json::json!({ "error": "已有迁移任务在进行" }).to_string())
                } else {
                    match (src, tgt) {
                        (Some(s), Some(t)) => {
                            *st = JobState {
                                running: true,
                                stage: "plan".into(),
                                message: "正在预检…".into(),
                                percent: 2,
                                finished: false,
                                success: false,
                                record_id: String::new(),
                            };
                            drop(st);
                            let job2 = Arc::clone(&job);
                            thread::spawn(move || {
                                let m = Migrator::new();
                                let plan = match m.plan(&PathBuf::from(&s), &PathBuf::from(&t)) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        Self::finish_job(&job2, false, "", &e);
                                        return;
                                    }
                                };
                                if !plan.can_run {
                                    Self::finish_job(&job2, false, "", &plan.blockers.join("；"));
                                    return;
                                }
                                let mut cb = |stage: &str, msg: &str, pct: u8| {
                                    if let Ok(mut g) = job2.lock() {
                                        g.stage = stage.to_string();
                                        g.message = msg.to_string();
                                        g.percent = pct;
                                    }
                                };
                                match m.execute(&plan, &mut cb) {
                                    Ok(rec) => Self::finish_job(&job2, true, &rec.id, "迁移完成"),
                                    Err(e) => Self::finish_job(&job2, false, "", &e),
                                }
                            });
                            (200, "application/json", serde_json::json!({ "started": true }).to_string())
                        }
                        _ => (400, "application/json", serde_json::json!({ "error": "缺少 source 或 target" }).to_string()),
                    }
                }
            }

            (Method::Get, "/api/migrate/status") => {
                let st = match job.lock() { Ok(g) => g.clone(), Err(p) => p.into_inner().clone() };
                (200, "application/json", serde_json::to_string(&st).unwrap_or_default())
            }

            (Method::Get, "/api/migrate/list") => {
                let list = migrate::list_migrations();
                (200, "application/json", serde_json::to_string(&list).unwrap_or_default())
            }

            (Method::Post, "/api/migrate/undo") => {
                let id = serde_json::from_str::<serde_json::Value>(&post_body)
                    .ok()
                    .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                let m = Migrator::new();
                let mut cb = |_s: &str, _m: &str, _p: u8| {};
                match m.undo(&id, &mut cb) {
                    Ok(_) => (200, "application/json", serde_json::json!({ "ok": true }).to_string()),
                    Err(e) => (400, "application/json", serde_json::json!({ "error": e }).to_string()),
                }
            }

            _ => (404, "text/plain", "Not Found".to_string()),
        };

        let ct_header = format!("{}; charset=utf-8", content_type);
        let response = Response::from_string(body)
            .with_status_code(status_code)
            .with_header(Header::from_bytes(&b"Content-Type"[..], ct_header.as_bytes()).unwrap())
            .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
        let _ = request.respond(response);
    }
}
