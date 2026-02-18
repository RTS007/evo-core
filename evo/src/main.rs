//! # EVO System Supervisor (Watchdog)
//!
//! Spawns HAL→CU in order, monitors via waitpid, restarts with exponential
//! backoff, and performs graceful shutdown with SHM cleanup.
//!
//! # Usage
//!
//! ```bash
//! evo --config-dir config/
//! evo --config-dir config/ --verbose
//! ```
//!
//! # Startup sequence
//!
//! 1. Load `config.toml` → `WatchdogConfig`
//! 2. Clean up orphan SHM segments (`/dev/shm/evo_*`)
//! 3. Spawn HAL (`evo_hal --config-dir <DIR> --simulate`)
//! 4. Wait for `evo_hal_cu` segment with heartbeat > 0
//! 5. Spawn CU (`evo_control_unit --config-dir <DIR>`)
//! 6. Enter monitoring loop (waitpid + optional heartbeat check)
//!
//! # Shutdown
//!
//! On SIGTERM/SIGINT: send SIGTERM to CU first, then HAL (reverse order).
//! Wait up to `sigterm_timeout_s`, then escalate to SIGKILL.
//! Clean up all `evo_*` SHM segments.

use clap::Parser;
use evo_common::config::{load_config_dir, WatchdogConfig};
use nix::sys::signal::{self, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::EnvFilter;

/// EVO System Supervisor — process manager and watchdog
#[derive(Parser, Debug)]
#[command(name = "evo")]
#[command(author = "RTS007")]
#[command(version)]
#[command(about = "EVO System Supervisor: spawns, monitors, and restarts HAL + CU")]
struct Args {
    /// Path to unified config directory.
    #[arg(long, value_name = "DIR", default_value = "config")]
    config_dir: PathBuf,

    /// Force simulation mode for HAL.
    #[arg(short = 's', long)]
    simulate: bool,

    /// Enable verbose logging (DEBUG level).
    #[arg(short, long)]
    verbose: bool,

    /// Output logs in JSON format.
    #[arg(long)]
    json: bool,
}

// ─── Signal handling ────────────────────────────────────────────────

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn install_signal_handler() {
    // Use a simple atomic flag — safe for signal context.
    let handler: extern "C" fn(libc::c_int) = signal_handler;
    unsafe {
        libc::signal(libc::SIGTERM, handler as libc::sighandler_t);
        libc::signal(libc::SIGINT, handler as libc::sighandler_t);
    }
}

extern "C" fn signal_handler(_sig: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

// ─── Main ───────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();
    setup_tracing(&args);

    info!("EVO System Supervisor v{} starting...", env!("CARGO_PKG_VERSION"));
    install_signal_handler();

    if let Err(e) = run(&args) {
        error!("FATAL: {e}");
        std::process::exit(1);
    }

    info!("EVO System Supervisor shutdown complete");
}

fn run(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load config.
    let full_config = load_config_dir(&args.config_dir)?;
    let wd = &full_config.system.watchdog;
    info!("Watchdog config: max_restarts={}, backoff={}ms→{}s, hal_ready_timeout={}s",
        wd.max_restarts, wd.initial_backoff_ms, wd.max_backoff_s, wd.hal_ready_timeout_s);

    // 2. Clean up orphan SHM segments.
    cleanup_orphan_shm();

    // 3. Enter the supervisor loop.
    let mut restart_count: u32 = 0;
    let mut backoff_ms: u64 = wd.initial_backoff_ms;

    loop {
        if SHUTDOWN.load(Ordering::SeqCst) {
            info!("Shutdown requested before spawn");
            return Ok(());
        }

        // Spawn HAL.
        info!("Spawning HAL (attempt {})", restart_count + 1);
        let mut hal = spawn_hal(&args.config_dir, args.simulate)?;
        let hal_pid = hal.id();
        info!("HAL spawned (PID={})", hal_pid);

        // Wait for HAL to create evo_hal_cu segment.
        if !wait_for_segment("hal_cu", wd.hal_ready_timeout_s) {
            warn!("HAL did not create evo_hal_cu within {}s, killing", wd.hal_ready_timeout_s);
            let _ = terminate_child(&mut hal, wd.sigterm_timeout_s);
            restart_count += 1;
            if restart_count >= wd.max_restarts {
                error!("CRITICAL: max restarts ({}) exhausted", wd.max_restarts);
                return Err("max restarts exhausted".into());
            }
            std::thread::sleep(Duration::from_millis(backoff_ms));
            backoff_ms = (backoff_ms * 2).min(wd.max_backoff_s * 1000);
            continue;
        }
        info!("HAL ready (evo_hal_cu segment active)");

        // Spawn CU.
        info!("Spawning CU");
        let mut cu = spawn_cu(&args.config_dir)?;
        let cu_pid = cu.id();
        info!("CU spawned (PID={})", cu_pid);

        // Monitor both processes.
        let stable_start = Instant::now();
        let result = monitor_children(&mut hal, &mut cu, wd);

        match result {
            MonitorResult::Shutdown => {
                info!("Shutdown signal received, stopping children...");
                graceful_shutdown(&mut cu, &mut hal, wd.sigterm_timeout_s);
                cleanup_all_shm();
                return Ok(());
            }
            MonitorResult::HalDied(status) => {
                warn!("HAL died ({status:?}), stopping CU and restarting...");
                let _ = terminate_child(&mut cu, wd.sigterm_timeout_s);
            }
            MonitorResult::CuDied(status) => {
                warn!("CU died ({status:?}), stopping HAL and restarting...");
                let _ = terminate_child(&mut hal, wd.sigterm_timeout_s);
            }
        }

        // Check if we were stable long enough to reset backoff.
        if stable_start.elapsed() >= Duration::from_secs(wd.stable_run_s) {
            restart_count = 0;
            backoff_ms = wd.initial_backoff_ms;
            debug!("Stable run reset: backoff→{}ms", backoff_ms);
        } else {
            restart_count += 1;
            if restart_count >= wd.max_restarts {
                error!("CRITICAL: max restarts ({}) exhausted", wd.max_restarts);
                cleanup_all_shm();
                return Err("max restarts exhausted".into());
            }
            info!("Restart {}/{}, backoff {}ms", restart_count, wd.max_restarts, backoff_ms);
            std::thread::sleep(Duration::from_millis(backoff_ms));
            backoff_ms = (backoff_ms * 2).min(wd.max_backoff_s * 1000);
        }

        cleanup_all_shm();
    }
}

// ─── Process Spawning (T059) ────────────────────────────────────────

fn spawn_hal(config_dir: &PathBuf, simulate: bool) -> Result<Child, Box<dyn std::error::Error>> {
    let mut cmd = Command::new("evo_hal");
    cmd.arg("--config-dir").arg(config_dir);
    if simulate {
        cmd.arg("--simulate");
    }
    let child = cmd.spawn().map_err(|e| format!("failed to spawn evo_hal: {e}"))?;
    Ok(child)
}

fn spawn_cu(config_dir: &PathBuf) -> Result<Child, Box<dyn std::error::Error>> {
    let child = Command::new("evo_control_unit")
        .arg("--config-dir")
        .arg(config_dir)
        .spawn()
        .map_err(|e| format!("failed to spawn evo_control_unit: {e}"))?;
    Ok(child)
}

// ─── Ordered Startup (T060) ────────────────────────────────────────

/// Wait for an SHM segment to appear with heartbeat > 0.
fn wait_for_segment(segment_name: &str, timeout_s: f64) -> bool {
    let path = format!("/dev/shm/evo_{segment_name}");
    let deadline = Instant::now() + Duration::from_secs_f64(timeout_s);

    while Instant::now() < deadline {
        if SHUTDOWN.load(Ordering::SeqCst) {
            return false;
        }
        if std::path::Path::new(&path).exists() {
            // Check heartbeat > 0 by reading header bytes.
            if check_heartbeat(&path) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Read the heartbeat field (offset 16, u64) from a mapped segment file.
fn check_heartbeat(path: &str) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 24]; // Read enough for magic(8) + version(4) + pad(4) + heartbeat(8)
    if file.read_exact(&mut buf).is_ok() {
        let heartbeat = u64::from_ne_bytes(buf[16..24].try_into().unwrap_or([0; 8]));
        heartbeat > 0
    } else {
        false
    }
}

// ─── Process Monitoring (T061) ──────────────────────────────────────

enum MonitorResult {
    Shutdown,
    HalDied(Option<i32>),
    CuDied(Option<i32>),
}

fn monitor_children(hal: &mut Child, cu: &mut Child, _wd: &WatchdogConfig) -> MonitorResult {
    let hal_pid = Pid::from_raw(hal.id() as i32);
    let cu_pid = Pid::from_raw(cu.id() as i32);

    loop {
        if SHUTDOWN.load(Ordering::SeqCst) {
            return MonitorResult::Shutdown;
        }

        // Check HAL.
        match waitpid(hal_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => {
                return MonitorResult::HalDied(Some(code));
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                return MonitorResult::HalDied(Some(128 + sig as i32));
            }
            _ => {}
        }

        // Check CU.
        match waitpid(cu_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => {
                return MonitorResult::CuDied(Some(code));
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                return MonitorResult::CuDied(Some(128 + sig as i32));
            }
            _ => {}
        }

        // Sleep between polls (100ms).
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ─── Restart Logic (T062) ──────────────────────────────────────────
// Integrated into the main run() loop above with exponential backoff.

// ─── Graceful Shutdown (T063) ───────────────────────────────────────

fn graceful_shutdown(cu: &mut Child, hal: &mut Child, timeout_s: f64) {
    // CU first (reverse of startup order).
    info!("Stopping CU (PID={})", cu.id());
    let _ = terminate_child(cu, timeout_s);

    // Then HAL.
    info!("Stopping HAL (PID={})", hal.id());
    let _ = terminate_child(hal, timeout_s);
}

/// Send SIGTERM, wait up to timeout_s, then escalate to SIGKILL.
fn terminate_child(child: &mut Child, timeout_s: f64) -> Result<(), String> {
    let pid = Pid::from_raw(child.id() as i32);

    // Send SIGTERM.
    if signal::kill(pid, Signal::SIGTERM).is_err() {
        // Process may already be dead.
        let _ = child.wait();
        return Ok(());
    }

    let deadline = Instant::now() + Duration::from_secs_f64(timeout_s);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return Ok(()),
            Ok(None) => {
                if Instant::now() >= deadline {
                    warn!("PID {} did not exit after SIGTERM, sending SIGKILL", child.id());
                    let _ = signal::kill(pid, Signal::SIGKILL);
                    let _ = child.wait();
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait error: {e}")),
        }
    }
}

// ─── Orphan SHM Cleanup (T064) ─────────────────────────────────────

/// Clean up orphan SHM segments left from a previous crash.
fn cleanup_orphan_shm() {
    let segments = list_evo_segments();
    if segments.is_empty() {
        debug!("No orphan SHM segments found");
        return;
    }

    info!("Found {} potential orphan SHM segment(s), cleaning...", segments.len());
    for name in &segments {
        // Probe the associated .lock file with flock(LOCK_EX|LOCK_NB).
        // The P2P writer holds LOCK_EX on the .lock file, not the data file.
        let lock_path = format!("/dev/shm/{name}.lock");
        let data_path = format!("/dev/shm/{name}");
        let is_orphan = match std::fs::File::open(&lock_path) {
            Ok(file) => {
                use std::os::unix::io::AsRawFd;
                let fd = file.as_raw_fd();
                let result = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
                if result == 0 {
                    // We got the exclusive lock → writer is dead → orphan.
                    unsafe { libc::flock(fd, libc::LOCK_UN) };
                    true
                } else {
                    // Lock held → writer is alive.
                    false
                }
            }
            Err(_) => {
                // No .lock file → check if data file exists without a writer.
                std::path::Path::new(&data_path).exists()
            }
        };

        if is_orphan {
            let shm_name = format!("/{name}");
            match nix::sys::mman::shm_unlink(shm_name.as_str()) {
                Ok(()) => info!("Cleaned orphan segment: {name}"),
                Err(e) => warn!("Failed to unlink {name}: {e}"),
            }
            // Also clean up the .lock file.
            let lock_shm_name = format!("/{name}.lock");
            let _ = nix::sys::mman::shm_unlink(lock_shm_name.as_str());
        } else {
            debug!("Segment {name} has active writer, skipping");
        }
    }
}

/// Remove all evo_* SHM segments (used during shutdown).
fn cleanup_all_shm() {
    for name in list_evo_segments() {
        let shm_name = format!("/{name}");
        match nix::sys::mman::shm_unlink(shm_name.as_str()) {
            Ok(()) => debug!("Unlinked SHM segment: {name}"),
            Err(e) => debug!("Could not unlink {name}: {e}"),
        }
        // Also clean up the .lock file.
        let lock_shm_name = format!("/{name}.lock");
        let _ = nix::sys::mman::shm_unlink(lock_shm_name.as_str());
    }
}

/// List all `evo_*` files in `/dev/shm/`.
fn list_evo_segments() -> Vec<String> {
    let mut segments = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/dev/shm") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("evo_") {
                    segments.push(name.to_string());
                }
            }
        }
    }
    segments
}

// ─── Tracing Setup ─────────────────────────────────────────────────

fn setup_tracing(args: &Args) {
    let level = if args.verbose { Level::DEBUG } else { Level::INFO };
    let filter = EnvFilter::from_default_env().add_directive(level.into());

    if args.json {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .compact()
            .init();
    }
}
