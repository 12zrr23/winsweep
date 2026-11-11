mod core;

use std::path::PathBuf;
use clap::{Parser, Subcommand};
use colored::*;
use core::analyzer::DiskAnalyzer;
use core::cleaner::SystemCleaner;
use core::installer::InstallerScanner;
use core::optimizer::SystemOptimizer;
use core::purge::ProjectPurgeScanner;
use core::status::StatusCollector;
use core::uninstall::AppUninstaller;

#[derive(Parser)]
#[command(name = "winsweep")]
#[command(author = env!("CARGO_PKG_AUTHORS"))]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = env!("CARGO_PKG_DESCRIPTION"), long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Output results as JSON for GUI / Agent automation
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Live system health, CPU, memory, disk I/O and network dashboard
    Status,

    /// Deep clean temporary files, browser cache and thumbnails
    Clean {
        /// Preview what would be removed without deleting
        #[arg(long)]
        dry_run: bool,
    },

    /// Purge rebuildable project artifacts (node_modules, target, .venv)
    Purge {
        /// Preview what would be removed without deleting
        #[arg(long)]
        dry_run: bool,

        /// Custom project directory to scan (default: common workspace directories)
        #[arg(long, short)]
        path: Option<String>,
    },

    /// Find and remove installer files (EXE, MSI, ISO)
    Installer {
        /// Preview what would be removed without deleting
        #[arg(long)]
        dry_run: bool,

        /// Custom directory to scan (default: Downloads and Desktop)
        #[arg(long, short)]
        path: Option<String>,
    },

    /// Flush DNS cache and optimize system services
    Optimize,

    /// List installed applications and examine leftovers
    Uninstall {
        /// Filter installed applications by name
        #[arg(long, short)]
        query: Option<String>,
    },

    /// Analyze disk usage and locate largest files
    Analyze {
        /// Target folder to analyze (default: user profile)
        path: Option<String>,
    },

    /// Migrate a folder to another drive (copy → verify → junction), keeping the original path working
    Migrate {
        /// Source folder to migrate
        source: Option<String>,
        /// Target drive root or folder, e.g. D:\ or E:\Store
        #[arg(long, short)]
        to: Option<String>,
        /// Preview only, do not touch anything
        #[arg(long)]
        dry_run: bool,
        /// List recorded migrations
        #[arg(long)]
        list: bool,
        /// Undo a migration by record id
        #[arg(long)]
        undo: Option<String>,
    },

    /// Start GUI bridge / helper
    Gui {
        /// Custom port to listen on (default: 41234)
        #[arg(long, short)]
        port: Option<u16>,
    },
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Status) {
        Commands::Status => {
            let mut collector = StatusCollector::new();
            let snapshot = collector.capture_snapshot();

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&snapshot).unwrap());
            } else {
                println!("{}", "=================================================".bright_blue());
                println!("  {} - Windows System Monitor", "WinSweep".bold().bright_green());
                println!("  Host: {} | OS: {} {}", snapshot.host_name.cyan(), snapshot.os_name, snapshot.os_version);
                let score_color = if snapshot.health_score >= 80 {
                    snapshot.health_score.to_string().bright_green()
                } else if snapshot.health_score >= 60 {
                    snapshot.health_score.to_string().bright_yellow()
                } else {
                    snapshot.health_score.to_string().bright_red()
                };
                println!("  System Health Score: {} / 100", score_color.bold());
                println!("{}", "=================================================".bright_blue());

                println!("\n{}", "[CPU]".bold().yellow());
                println!("  Total Usage: {:.1}%", snapshot.cpu_total_usage);
                println!("  Cores Count: {}", snapshot.cpu_cores.len());

                println!("\n{}", "[Memory]".bold().yellow());
                println!(
                    "  Used: {} / {} ({:.1}%)",
                    format_bytes(snapshot.memory_used),
                    format_bytes(snapshot.memory_total),
                    snapshot.memory_usage_percent
                );

                println!("\n{}", "[Disks]".bold().yellow());
                for d in &snapshot.disks {
                    println!(
                        "  Drive {}: Used {} / {} ({:.1}%)",
                        d.mount_point.cyan(),
                        format_bytes(d.used_space),
                        format_bytes(d.total_space),
                        d.usage_percent
                    );
                }

                println!("\n{}", "[Top Active Processes]".bold().yellow());
                for p in &snapshot.top_processes {
                    println!("  PID {:<6} {:<24} CPU: {:.1}%", p.pid, p.name, p.cpu_usage);
                }
            }
        }

        Commands::Clean { dry_run } => {
            let cleaner = SystemCleaner::new();
            if cli.json {
                let categories = cleaner.scan_all();
                println!("{}", serde_json::to_string_pretty(&categories).unwrap());
            } else {
                println!("{}", "Scanning system cleanup candidates...".cyan());
                let categories = cleaner.scan_all();
                let mut grand_total = 0u64;

                for cat in &categories {
                    grand_total += cat.total_size;
                    println!(
                        "  ✓ {:<28} {} items ({})",
                        cat.name.bold(),
                        cat.total_count,
                        format_bytes(cat.total_size).bright_green()
                    );
                }

                println!("\n{}", "-------------------------------------------------".bright_black());
                if dry_run {
                    println!(
                        "  [Dry-Run] Estimated cleanable: {} across {} categories",
                        format_bytes(grand_total).bold().bright_green(),
                        categories.len()
                    );
                } else {
                    let (freed, count) = cleaner.clean_all(false);
                    println!(
                        "  Successfully cleaned {} files, freed: {}",
                        count,
                        format_bytes(freed).bold().bright_green()
                    );
                }
            }
        }

        Commands::Purge { dry_run, path } => {
            let purge = ProjectPurgeScanner::new();
            let artifacts = if let Some(ref p) = path {
                purge.scan_path(&PathBuf::from(p))
            } else {
                purge.scan_default_workspace()
            };

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&artifacts).unwrap());
            } else {
                println!("{}", "Scanning project build artifacts...".cyan());
                let mut total_size = 0u64;
                for a in &artifacts {
                    total_size += a.size;
                    let tag = if a.is_recently_active {
                        "[Active <7d - Skipped by default]".yellow()
                    } else {
                        "[Eligible for Purge]".bright_green()
                    };
                    println!(
                        "  • {:<20} {:<22} {:<12} {}",
                        a.project_name.bold(),
                        a.artifact_type,
                        format_bytes(a.size),
                        tag
                    );
                }

                println!("\n{}", "-------------------------------------------------".bright_black());
                println!(
                    "  Total build artifacts found: {} ({})",
                    artifacts.len(),
                    format_bytes(total_size).bright_green()
                );
                if dry_run {
                    println!("  [Dry-Run] Pass without --dry-run to purge inactive artifacts.");
                } else {
                    let eligible: Vec<_> = artifacts.into_iter().filter(|a| !a.is_recently_active).collect();
                    let (freed, count) = purge.purge_artifacts(&eligible, false);
                    println!(
                        "  Successfully moved to Trash: {} items, freed: {}",
                        count,
                        format_bytes(freed).bold().bright_green()
                    );
                }
            }
        }

        Commands::Installer { dry_run, path } => {
            let scanner = InstallerScanner::new();
            let items = if let Some(ref p) = path {
                scanner.scan_custom_path(&PathBuf::from(p))
            } else {
                scanner.scan_all()
            };

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&items).unwrap());
            } else {
                println!("{}", "Scanning installer packages (EXE, MSI, ISO)...".cyan());
                let mut total_size = 0u64;
                for item in &items {
                    total_size += item.size;
                    println!(
                        "  • {:<36} {:<10} ({})",
                        item.file_name.bold(),
                        format_bytes(item.size).bright_green(),
                        item.source_location.cyan()
                    );
                }
                println!("\n{}", "-------------------------------------------------".bright_black());
                println!(
                    "  Total installers found: {} ({})",
                    items.len(),
                    format_bytes(total_size).bright_green()
                );
                if !dry_run && !items.is_empty() {
                    let (freed, count) = scanner.clean_installers(&items, false);
                    println!(
                        "  Moved {} installers to Recycle Bin, freed: {}",
                        count,
                        format_bytes(freed).bold().bright_green()
                    );
                }
            }
        }

        Commands::Optimize => {
            let optimizer = SystemOptimizer::new();
            let results = optimizer.run_all_optimizations();

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&results).unwrap());
            } else {
                println!("{}", "Running system optimization tasks...".cyan());
                for r in results {
                    let status_icon = if r.success { "✓".bright_green() } else { "✗".bright_red() };
                    println!("  {} {}: {}", status_icon, r.task_name.bold(), r.message);
                }
            }
        }

        Commands::Uninstall { query } => {
            let uninstaller = AppUninstaller::new();
            let apps = if let Some(ref q) = query {
                uninstaller.search_apps(q)
            } else {
                uninstaller.list_installed_apps()
            };

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&apps).unwrap());
            } else {
                if let Some(ref q) = query {
                    println!("Searching installed applications matching '{}'...", q.cyan());
                } else {
                    println!("{}", "Listing installed applications...".cyan());
                }
                for app in apps.iter().take(20) {
                    let size_str = if app.estimated_size_kb > 0 {
                        format_bytes(app.estimated_size_kb * 1024)
                    } else {
                        "Unknown".to_string()
                    };
                    println!("  • {:<32} {:<16} {}", app.display_name.bold(), app.display_version, size_str.bright_green());
                }
                if apps.len() > 20 {
                    println!("  ... and {} more apps (use --json for complete dump)", apps.len() - 20);
                }
            }
        }

        Commands::Analyze { path } => {
            let analyzer = DiskAnalyzer::new();
            let target = path
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::var("USERPROFILE")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| PathBuf::from("."))
                });

            if !target.exists() {
                eprintln!("Error: Target path {:?} does not exist.", target);
                std::process::exit(1);
            }

            println!("Analyzing disk space for {:?}...", target);
            let report = analyzer.analyze_path(&target, 15);

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                println!(
                    "  Scanned {} files, total size: {}",
                    report.total_scanned_files,
                    format_bytes(report.total_scanned_bytes).bright_green()
                );
                println!("\n  {}", "Top Large Files:".bold().yellow());
                for f in &report.largest_files {
                    println!("  • {:<12} {}", format_bytes(f.size).bright_green(), f.path.to_string_lossy());
                }
            }
        }

        Commands::Migrate { source, to, dry_run, list, undo } => {
            let migrator = core::migrate::Migrator::new();

            if list {
                let recs = core::migrate::list_migrations();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&recs).unwrap());
                } else if recs.is_empty() {
                    println!("{}", "No migrations recorded.".cyan());
                } else {
                    println!("{}", "Recorded migrations:".bold().cyan());
                    for r in recs {
                        println!(
                            "  [{}] {}  {}  ({} files, {})  {} -> {}",
                            r.id, r.at, r.status, r.files, format_bytes(r.bytes),
                            r.source, r.dest
                        );
                    }
                }
                return;
            }

            if let Some(id) = undo {
                println!("{}", format!("Undoing migration {}...", id).cyan());
                let mut cb = |stage: &str, msg: &str, pct: u8| {
                    println!("  [{:>3}%] {:<12} {}", pct, stage, msg);
                };
                match migrator.undo(&id, &mut cb) {
                    Ok(_) => println!("{}", "Undo finished.".bright_green()),
                    Err(e) => {
                        eprintln!("{}", format!("Undo failed: {}", e).bright_red());
                        std::process::exit(1);
                    }
                }
                return;
            }

            let (Some(src), Some(tgt)) = (source, to) else {
                eprintln!("Usage: winsweep migrate <source> --to <target-drive>");
                std::process::exit(2);
            };
            let plan = match migrator.plan(std::path::Path::new(&src), std::path::Path::new(&tgt)) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("{}", format!("Plan failed: {}", e).bright_red());
                    std::process::exit(1);
                }
            };

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&plan).unwrap());
            } else {
                println!("{}", "Migration plan".bold().cyan());
                println!("  source : {}", plan.source);
                println!("  dest   : {}", plan.dest);
                println!("  size   : {} ({} files)", format_bytes(plan.source_bytes), plan.source_files);
                println!("  target : {} free", format_bytes(plan.target_free_bytes));
                println!("  method : {}", plan.method);
                for w in &plan.warnings { println!("  {} {}", "!".yellow(), w); }
                for b in &plan.blockers { println!("  {} {}", "x".bright_red(), b); }
                println!(
                    "  {} {}",
                    "can run:".bold(),
                    if plan.can_run { "yes".bright_green() } else { "no".bright_red() }
                );
            }

            if dry_run {
                if !cli.json { println!("{}", "Dry-run: nothing changed.".cyan()); }
                return;
            }
            if !plan.can_run {
                std::process::exit(1);
            }
            if !cli.json {
                println!(
                    "{}",
                    "This will copy data, verify it, then replace the original folder with a junction. Continue? (y/N)"
                        .yellow()
                );
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).ok();
                if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
                    println!("{}", "Cancelled.".cyan());
                    return;
                }
            }

            let mut cb = |stage: &str, msg: &str, pct: u8| {
                println!("  [{:>3}%] {:<12} {}", pct, stage, msg);
            };
            match migrator.execute(&plan, &mut cb) {
                Ok(rec) => {
                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&rec).unwrap());
                    } else {
                        println!("{}", format!("Done. Record id: {}", rec.id).bright_green());
                        println!("  original path now points to: {}", rec.dest);
                    }
                }
                Err(e) => {
                    eprintln!("{}", format!("Migration failed: {}", e).bright_red());
                    std::process::exit(1);
                }
            }
        }

        Commands::Gui { port } => {
            let chosen_port = port.unwrap_or(41234);
            println!("{}", "Starting WinSweep GUI Dashboard...".green().bold());
            let server = core::server::GuiServer::new(chosen_port);
            server.start();
        }
    }
}
