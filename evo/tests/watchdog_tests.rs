//! # Watchdog Unit Tests
//!
//! Tests for the EVO supervisor functions. Since the watchdog spawns
//! real binaries (evo_hal, evo_control_unit), full E2E tests are
//! in Phase 12. These tests cover:
//!
//! - SHM segment listing and cleanup
//! - Heartbeat checking via raw file reads
//! - Orphan detection via flock probing
//! - WatchdogTrait and associated types

use evo_common::shm::p2p::{TypedP2pWriter, ModuleAbbrev};
use evo_common::shm::segments::HalToCuSegment;
use evo_common::watchdog::{HealthStatus, ManagedModule, Watchdog, WatchdogError};
use std::path::Path;

// ─── Helpers ────────────────────────────────────────────────────────

/// Generate a unique SHM segment name for test isolation.
fn test_seg_name(suffix: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static CTR: AtomicU32 = AtomicU32::new(0);
    let id = CTR.fetch_add(1, Ordering::Relaxed);
    // NOTE: TypedP2pWriter prepends "/evo_" prefix automatically (SHM_PREFIX).
    // The name we pass here is the "logical" name; the file in /dev/shm/
    // will be evo_<name>.
    format!("wd_test_{id}_{suffix}")
}

// ─── list_evo_segments logic ────────────────────────────────────────

#[test]
fn test_list_evo_segments_finds_created_segments() {
    // Create a temporary P2P writer, verify segment appears in /dev/shm.
    let name = test_seg_name("list");
    let writer = TypedP2pWriter::<HalToCuSegment>::create(
        &name,
        ModuleAbbrev::Hal,
        ModuleAbbrev::Cu,
    )
    .expect("create writer");

    let file_name = format!("evo_{name}");
    let path = format!("/dev/shm/{file_name}");
    assert!(
        std::path::Path::new(&path).exists(),
        "segment file should exist in /dev/shm at {path}"
    );

    // List all evo_* segments.
    let segments = list_evo_segments();
    assert!(
        segments.contains(&file_name),
        "list_evo_segments should find our segment; got: {segments:?}"
    );

    drop(writer);
}

#[test]
fn test_list_evo_segments_ignores_non_evo() {
    let segments = list_evo_segments();
    for seg in &segments {
        assert!(
            seg.starts_with("evo_"),
            "all listed segments must start with evo_: got {seg}"
        );
    }
}

// ─── Heartbeat check ───────────────────────────────────────────────

#[test]
fn test_check_heartbeat_returns_false_for_zero_heartbeat() {
    // A freshly-created writer has heartbeat=0 initially (before first commit).
    let name = test_seg_name("hb_zero");
    let _writer = TypedP2pWriter::<HalToCuSegment>::create(
        &name,
        ModuleAbbrev::Hal,
        ModuleAbbrev::Cu,
    )
    .expect("create writer");

    let path = format!("/dev/shm/evo_{name}");
    // Before any commit, heartbeat should be 0.
    let has_heartbeat = check_heartbeat(&path);
    assert!(
        !has_heartbeat,
        "heartbeat should be 0 (false) before first commit"
    );
}

#[test]
fn test_check_heartbeat_returns_true_after_commit() {
    let name = test_seg_name("hb_one");
    let mut writer = TypedP2pWriter::<HalToCuSegment>::create(
        &name,
        ModuleAbbrev::Hal,
        ModuleAbbrev::Cu,
    )
    .expect("create writer");

    // Write + commit to increment heartbeat.
    let mut payload = HalToCuSegment::default();
    payload.axis_count = 2;
    writer.commit(&payload).expect("commit");

    let path = format!("/dev/shm/evo_{name}");
    let has_heartbeat = check_heartbeat(&path);
    assert!(
        has_heartbeat,
        "heartbeat should be > 0 after commit"
    );
}

#[test]
fn test_check_heartbeat_returns_false_for_nonexistent() {
    assert!(!check_heartbeat("/dev/shm/evo_nonexistent_test_xyz"));
}

// ─── Orphan cleanup via flock probe ─────────────────────────────────

#[test]
fn test_flock_probe_detects_live_writer() {
    let name = test_seg_name("flock_live");
    let _writer = TypedP2pWriter::<HalToCuSegment>::create(
        &name,
        ModuleAbbrev::Hal,
        ModuleAbbrev::Cu,
    )
    .expect("create writer");

    // Writer is alive — flock(LOCK_EX|LOCK_NB) should FAIL (writer holds LOCK_EX).
    // Note: The P2P writer holds flock on the .lock file, not the data file.
    // So probing the data file itself won't detect the writer. We probe the lock file.
    let path = format!("/dev/shm/evo_{name}.lock");
    let is_orphan = probe_orphan(&path);
    assert!(
        !is_orphan,
        "segment with live writer should NOT be detected as orphan"
    );
}

#[test]
fn test_flock_probe_detects_dead_writer() {
    let name = test_seg_name("flock_dead");

    // Create writer then drop it — the flock is released on drop.
    {
        let _writer = TypedP2pWriter::<HalToCuSegment>::create(
            &name,
            ModuleAbbrev::Hal,
            ModuleAbbrev::Cu,
        )
        .expect("create writer");
    } // writer dropped here, flock released.

    // The segment file still exists (shm_unlink removes name but we can
    // re-check). Actually the writer's Drop does shm_unlink. Let's create
    // a segment manually via shm_open so it persists.
    // Instead, let's test with a known state: create the file manually.
    // This is tricky because TypedP2pWriter does shm_unlink on Drop.
    // We'll test the inverse: the live-writer test above proves the probe
    // works. For the dead-writer case, let's create a bare file.

    use std::io::Write;
    let bare_path = format!("/dev/shm/evo_wd_test_bare_{}", std::process::id());
    {
        let mut f = std::fs::File::create(&bare_path).expect("create bare file");
        f.write_all(&[0u8; 128]).expect("write zeros");
    }

    // No flock held on this file → probe should detect orphan.
    let is_orphan = probe_orphan(&bare_path);
    assert!(is_orphan, "bare file with no flock should be detected as orphan");

    // Clean up.
    let _ = std::fs::remove_file(&bare_path);
}

// ─── WatchdogTrait types ────────────────────────────────────────────

#[test]
fn test_health_status_variants() {
    let h = HealthStatus::Healthy;
    assert_eq!(h, HealthStatus::Healthy);

    let s = HealthStatus::Stale { age_secs: 5 };
    match s {
        HealthStatus::Stale { age_secs } => assert_eq!(age_secs, 5),
        _ => panic!("expected Stale"),
    }

    let d = HealthStatus::Dead { exit_code: Some(137) };
    match d {
        HealthStatus::Dead { exit_code } => assert_eq!(exit_code, Some(137)),
        _ => panic!("expected Dead"),
    }

    assert_eq!(HealthStatus::Unknown, HealthStatus::Unknown);
}

#[test]
fn test_managed_module_enum() {
    assert_ne!(ManagedModule::Hal, ManagedModule::Cu);
    assert_eq!(ManagedModule::Hal, ManagedModule::Hal);

    // All variants exist.
    let _all = [
        ManagedModule::Hal,
        ManagedModule::Cu,
        ManagedModule::RecipeExecutor,
        ManagedModule::Grpc,
        ManagedModule::Mqtt,
    ];
}

#[test]
fn test_watchdog_error_display() {
    let e = WatchdogError::SpawnFailed {
        module: ManagedModule::Hal,
        reason: "binary not found".into(),
    };
    let msg = format!("{e}");
    assert!(msg.contains("Hal"), "error should mention module: {msg}");
    assert!(msg.contains("binary not found"), "error should contain reason: {msg}");

    let e2 = WatchdogError::RestartsExhausted {
        module: ManagedModule::Cu,
        max: 5,
    };
    let msg2 = format!("{e2}");
    assert!(msg2.contains("5"), "should show max count: {msg2}");
}

/// Verify the trait is object-safe (can be used as dyn Watchdog).
#[test]
fn test_watchdog_trait_is_object_safe() {
    struct DummyWatchdog;
    impl Watchdog for DummyWatchdog {
        fn spawn_module(&mut self, _module: ManagedModule, _config_dir: &Path) -> Result<u32, WatchdogError> {
            Ok(12345)
        }
        fn health_check(&self, _module: ManagedModule) -> HealthStatus {
            HealthStatus::Unknown
        }
        fn restart_module(&mut self, _module: ManagedModule) -> Result<u32, WatchdogError> {
            Err(WatchdogError::Other("not implemented".into()))
        }
        fn shutdown_all(&mut self) -> Result<(), WatchdogError> {
            Ok(())
        }
    }

    let mut wd: Box<dyn Watchdog> = Box::new(DummyWatchdog);
    let pid = wd.spawn_module(ManagedModule::Hal, Path::new("/tmp")).unwrap();
    assert_eq!(pid, 12345);
    assert_eq!(wd.health_check(ManagedModule::Cu), HealthStatus::Unknown);
    assert!(wd.shutdown_all().is_ok());
}

// ─── Reimplemented helpers (matching evo/src/main.rs logic) ─────────
// We reimplement the pure functions here rather than importing from the
// binary crate, since evo is a binary (not a lib).

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

fn check_heartbeat(path: &str) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 24];
    if file.read_exact(&mut buf).is_ok() {
        let heartbeat = u64::from_ne_bytes(buf[16..24].try_into().unwrap_or([0; 8]));
        heartbeat > 0
    } else {
        false
    }
}

fn probe_orphan(path: &str) -> bool {
    match std::fs::File::open(path) {
        Ok(file) => {
            use std::os::unix::io::AsRawFd;
            let fd = file.as_raw_fd();
            let result = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            if result == 0 {
                unsafe { libc::flock(fd, libc::LOCK_UN) };
                true
            } else {
                false
            }
        }
        Err(_) => false,
    }
}
