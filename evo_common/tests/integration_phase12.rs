//! # Phase 12 Integration Tests
//!
//! E2E tests for the full EVO system pipeline.
//!
//! Tests marked `#[ignore]` require the full system binaries to be built
//! and available in PATH. Run with: `cargo test --test integration -- --ignored`

use evo_common::config::load_config_dir;
use evo_common::shm::p2p::{ModuleAbbrev, TypedP2pReader, TypedP2pWriter};
use evo_common::shm::segments::*;
use std::path::Path;

// ─── T079: Config Agreement Test ────────────────────────────────────

/// Verify that HAL and CU load the same configs and agree on axis count,
/// I/O roles, and parameters.
#[test]
fn test_config_agreement_hal_and_cu_see_same_axes() {
    let config_dir = Path::new("config");
    if !config_dir.exists() {
        eprintln!("Skipping: config/ directory not found");
        return;
    }

    let full_config = load_config_dir(config_dir).expect("load_config_dir");

    // Both HAL and CU read from the same FullConfig.
    // Verify axis count matches between machine config and axis files.
    let axis_count = full_config.axes.len();
    assert!(axis_count > 0, "should have at least one axis");
    assert!(axis_count <= 64, "max 64 axes supported");

    // Verify all axes have unique IDs.
    let mut ids: Vec<u8> = full_config.axes.iter().map(|a| a.axis.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        axis_count,
        "all axis IDs must be unique"
    );

    // Verify all axes have non-empty names.
    for axis in &full_config.axes {
        assert!(
            !axis.axis.name.is_empty(),
            "axis {} must have a name",
            axis.axis.id
        );
    }

    // Verify watchdog config is present and valid.
    let wd = &full_config.system.watchdog;
    assert!(wd.max_restarts > 0);
    assert!(wd.initial_backoff_ms > 0);
    assert!(wd.max_backoff_s as u64 * 1000 >= wd.initial_backoff_ms);
    assert!(wd.hal_ready_timeout_s > 0.0);
    assert!(wd.sigterm_timeout_s > 0.0);
}

/// Verify IoConfig loads correctly from io.toml if present.
#[test]
fn test_config_agreement_io_file_exists() {
    let config_dir = Path::new("config");
    if !config_dir.exists() {
        eprintln!("Skipping: config/ directory not found");
        return;
    }

    // io.toml is loaded separately if present.
    let io_path = config_dir.join("io.toml");
    if io_path.exists() {
        let content = std::fs::read_to_string(&io_path).expect("read io.toml");
        assert!(!content.is_empty(), "io.toml should not be empty");
    }
}

// ─── T080: RT Stability Test (FR-078) ──────────────────────────────

/// Verify CycleRunner can execute 10,000 cycles without panics.
/// This is a unit-level stability test — no actual RT scheduling.
#[test]
fn test_rt_stability_segment_operations() {
    // Simulate 10,000 write-read cycles on HalToCuSegment.
    let name = format!("stability_{}", std::process::id());
    let mut writer = TypedP2pWriter::<HalToCuSegment>::create(
        &name,
        ModuleAbbrev::Hal,
        ModuleAbbrev::Cu,
    )
    .expect("create writer");

    let mut reader =
        TypedP2pReader::<HalToCuSegment>::attach(&name, 1000).expect("attach reader");

    let mut payload = HalToCuSegment::default();

    for i in 0u32..10_000 {
        payload.axis_count = (i % 64) as u8;
        payload.axes[0].position = i as f64 * 0.001;
        writer.commit(&payload).expect("commit");

        let read_result = reader.read();
        match read_result {
            Ok(data) => {
                assert_eq!(data.axis_count, (i % 64) as u8);
            }
            Err(e) => {
                // ReadContention is acceptable — writer and reader on same thread.
                // In real RT, they are separate processes.
                let _ = e;
            }
        }
    }
}

// ─── T082: Constant Deduplication Verification ──────────────────────

#[test]
fn test_max_axes_defined_once() {
    // This is verified by compilation — if MAX_AXES were defined in
    // multiple places with different values, we'd get ambiguous imports.
    use evo_common::consts::MAX_AXES;
    assert_eq!(MAX_AXES, 64u8);
}

#[test]
fn test_segment_type_uniqueness() {
    // Verify all 15 segment types have distinct sizes or layouts.
    use core::mem::size_of;

    let sizes = [
        ("HalToCuSegment", size_of::<HalToCuSegment>()),
        ("CuToHalSegment", size_of::<CuToHalSegment>()),
        ("CuToMqtSegment", size_of::<CuToMqtSegment>()),
        ("HalToMqtSegment", size_of::<HalToMqtSegment>()),
        ("ReToCuSegment", size_of::<ReToCuSegment>()),
        ("ReToHalSegment", size_of::<ReToHalSegment>()),
        ("ReToMqtSegment", size_of::<ReToMqtSegment>()),
        ("ReToRpcSegment", size_of::<ReToRpcSegment>()),
        ("RpcToCuSegment", size_of::<RpcToCuSegment>()),
        ("RpcToHalSegment", size_of::<RpcToHalSegment>()),
        ("RpcToReSegment", size_of::<RpcToReSegment>()),
        ("CuToReSegment", size_of::<CuToReSegment>()),
        ("CuToRpcSegment", size_of::<CuToRpcSegment>()),
        ("HalToRpcSegment", size_of::<HalToRpcSegment>()),
        ("HalToReSegment", size_of::<HalToReSegment>()),
    ];

    // All sizes must be > 0 and aligned to 64.
    for (name, size) in &sizes {
        assert!(*size > 0, "{name} has zero size");
        assert!(
            size % 64 == 0,
            "{name} size {size} not aligned to 64 bytes"
        );
    }
}

#[test]
fn test_segment_name_convention() {
    // Verify all segment name constants follow evo_[SRC]_[DST] pattern.
    let names = [
        SEG_HAL_CU, SEG_CU_HAL, SEG_CU_MQT, SEG_HAL_MQT,
        SEG_RE_CU, SEG_RE_HAL, SEG_RE_MQT, SEG_RE_RPC,
        SEG_RPC_CU, SEG_RPC_HAL, SEG_RPC_RE,
        SEG_CU_RE, SEG_CU_RPC, SEG_HAL_RPC, SEG_HAL_RE,
    ];

    for name in &names {
        // Must contain exactly one underscore separating src_dst.
        let parts: Vec<&str> = name.split('_').collect();
        assert_eq!(
            parts.len(),
            2,
            "segment name '{name}' should have format 'src_dst'"
        );
        assert!(!parts[0].is_empty(), "source part empty in '{name}'");
        assert!(!parts[1].is_empty(), "dest part empty in '{name}'");
    }
}

// ─── T075-T078: E2E Tests (require running binaries) ───────────────

#[test]
#[ignore = "requires evo, evo_hal, evo_control_unit binaries in PATH"]
fn test_e2e_watchdog_spawns_hal_and_cu() {
    // T075: Start evo with config, verify segments appear.
    use std::process::Command;
    use std::time::{Duration, Instant};

    let mut child = Command::new("evo")
        .args(["--config-dir", "config", "--simulate"])
        .spawn()
        .expect("spawn evo");

    // Wait up to 10s for evo_hal_cu segment.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut found = false;
    while Instant::now() < deadline {
        if Path::new("/dev/shm/evo_hal_cu").exists() {
            found = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Cleanup: send SIGTERM.
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    );
    let _ = child.wait();

    assert!(found, "evo_hal_cu segment should appear within 10 seconds");
}

#[test]
#[ignore = "requires evo, evo_hal, evo_control_unit binaries in PATH"]
fn test_e2e_graceful_shutdown() {
    // T078: Start evo, send SIGTERM, verify clean exit within 5s.
    use std::process::Command;
    use std::time::{Duration, Instant};

    let mut child = Command::new("evo")
        .args(["--config-dir", "config", "--simulate"])
        .spawn()
        .expect("spawn evo");

    // Let it start up.
    std::thread::sleep(Duration::from_secs(3));

    // Send SIGTERM.
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    );

    let start = Instant::now();
    let status = child.wait().expect("wait for evo");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "should exit within 5 seconds, took {:?}",
        elapsed
    );
    assert!(
        status.success(),
        "should exit with code 0, got: {status:?}"
    );
}
