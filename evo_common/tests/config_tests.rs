//! Config auto-discovery tests (T024).
//!
//! Tests for `load_config_dir()`: axis file discovery, NN↔id validation,
//! duplicate detection, missing axes error, unknown fields rejection,
//! legacy `[[axes]]` rejection, numeric bounds validation (FR-054).

use evo_common::config::{load_config_dir, ConfigError};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Create a minimal config.toml in the given directory.
fn write_config_toml(dir: &Path) {
    fs::write(
        dir.join("config.toml"),
        r#"
[watchdog]
max_restarts = 5

[hal]
[cu]
[re]
[mqtt]
[grpc]
[api]
[dashboard]
[diagnostic]
"#,
    )
    .unwrap();
}

/// Create a minimal machine.toml in the given directory.
fn write_machine_toml(dir: &Path) {
    fs::write(
        dir.join("machine.toml"),
        r#"
[machine]
name = "Test Machine"

[global_safety]
default_safe_stop = "SS1"
safety_stop_timeout = 5.0
recovery_authorization_required = true

[service_bypass]
bypass_axes = [1]
max_service_velocity = 50.0
"#,
    )
    .unwrap();
}

/// Create a valid per-axis TOML for the given axis id and name.
fn write_axis_toml(dir: &Path, nn: u8, name: &str) {
    let fname = format!("axis_{:02}_{}.toml", nn, name);
    let content = format!(
        r#"
[axis]
id = {nn}
name = "{name}"
type = "linear"

[kinematics]
max_velocity = 500.0
safe_reduced_speed_limit = 50.0
min_pos = -100.0
max_pos = 1000.0
in_position_window = 0.05

[control]
kp = 100.0
ki = 20.0
kd = 0.5
out_max = 100.0
lag_error_limit = 0.5
lag_policy = "Unwanted"

[safe_stop]
category = "SS1"
max_decel_safe = 10000.0

[homing]
method = "HomeSensor"
speed = 20.0
timeout = 30.0
"#,
        nn = nn,
        name = name,
    );
    fs::write(dir.join(&fname), content).unwrap();
}

// ─── Tests ──────────────────────────────────────────────────────────

/// Test: load_config_dir succeeds with valid config directory.
#[test]
fn load_config_dir_success() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);
    write_machine_toml(dir);
    write_axis_toml(dir, 1, "x");
    write_axis_toml(dir, 2, "y");

    let full = load_config_dir(dir).expect("should load successfully");
    assert_eq!(full.axes.len(), 2);
    assert_eq!(full.axes[0].axis.id, 1);
    assert_eq!(full.axes[1].axis.id, 2);
    assert_eq!(full.machine.machine.name, "Test Machine");
    assert_eq!(full.system.watchdog.max_restarts, 5);
}

/// Test: axis files auto-discovered and sorted by NN.
#[test]
fn axis_discovery_sorted() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);
    write_machine_toml(dir);
    // Create axes out of order.
    write_axis_toml(dir, 3, "z");
    write_axis_toml(dir, 1, "x");
    write_axis_toml(dir, 2, "y");

    let full = load_config_dir(dir).expect("should load");
    assert_eq!(full.axes.len(), 3);
    assert_eq!(full.axes[0].axis.id, 1);
    assert_eq!(full.axes[1].axis.id, 2);
    assert_eq!(full.axes[2].axis.id, 3);
}

/// Test: axis NN↔id mismatch is rejected.
#[test]
fn axis_id_mismatch() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);
    write_machine_toml(dir);

    // Create axis_01_x.toml but with id = 2 inside.
    let content = r#"
[axis]
id = 2
name = "X-Axis"
type = "linear"

[kinematics]
max_velocity = 500.0
safe_reduced_speed_limit = 50.0
min_pos = -100.0
max_pos = 1000.0

[control]
kp = 100.0
ki = 20.0
kd = 0.5
out_max = 100.0
lag_error_limit = 0.5
lag_policy = "Unwanted"

[safe_stop]
category = "SS1"
max_decel_safe = 10000.0

[homing]
method = "HomeSensor"
speed = 20.0
timeout = 30.0
"#;
    fs::write(dir.join("axis_01_x.toml"), content).unwrap();

    let result = load_config_dir(dir);
    assert!(
        matches!(result, Err(ConfigError::AxisIdMismatch { expected: 1, found: 2, .. })),
        "expected AxisIdMismatch"
    );
}

/// Test: duplicate axis NN is rejected.
#[test]
fn duplicate_axis_id() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);
    write_machine_toml(dir);

    // Two files with NN=01.
    write_axis_toml(dir, 1, "x");
    // Create a second file with NN=01 but different name.
    let content = r#"
[axis]
id = 1
name = "Duplicate"
type = "linear"

[kinematics]
max_velocity = 500.0
safe_reduced_speed_limit = 50.0
min_pos = -100.0
max_pos = 1000.0

[control]
kp = 100.0
ki = 20.0
kd = 0.5
out_max = 100.0
lag_error_limit = 0.5
lag_policy = "Unwanted"

[safe_stop]
category = "SS1"
max_decel_safe = 10000.0

[homing]
method = "HomeSensor"
speed = 20.0
timeout = 30.0
"#;
    fs::write(dir.join("axis_01_duplicate.toml"), content).unwrap();

    let result = load_config_dir(dir);
    assert!(
        matches!(result, Err(ConfigError::DuplicateAxisId(1))),
        "expected DuplicateAxisId"
    );
}

/// Test: no axis files → NoAxesDefined.
#[test]
fn no_axes_defined() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);
    write_machine_toml(dir);
    // No axis files.

    let result = load_config_dir(dir);
    assert!(
        matches!(result, Err(ConfigError::NoAxesDefined)),
        "expected NoAxesDefined"
    );
}

/// Test: unknown fields in axis TOML rejected (deny_unknown_fields).
#[test]
fn unknown_field_rejected() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);
    write_machine_toml(dir);

    let content = r#"
[axis]
id = 1
name = "X-Axis"
type = "linear"
bogus_field = "should fail"

[kinematics]
max_velocity = 500.0
safe_reduced_speed_limit = 50.0
min_pos = -100.0
max_pos = 1000.0

[control]
kp = 100.0
ki = 20.0
kd = 0.5
out_max = 100.0
lag_error_limit = 0.5
lag_policy = "Unwanted"

[safe_stop]
category = "SS1"
max_decel_safe = 10000.0

[homing]
method = "HomeSensor"
speed = 20.0
timeout = 30.0
"#;
    fs::write(dir.join("axis_01_x.toml"), content).unwrap();

    let result = load_config_dir(dir);
    assert!(
        matches!(result, Err(ConfigError::UnknownField(_))),
        "expected UnknownField"
    );
}

/// Test: legacy [[axes]] array format in machine.toml is rejected.
#[test]
fn legacy_axes_array_rejected() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);

    // Machine.toml with legacy [[axes]] array.
    let content = r#"
[machine]
name = "Bad Machine"

[global_safety]
default_safe_stop = "SS1"
safety_stop_timeout = 5.0
recovery_authorization_required = true

[service_bypass]
bypass_axes = [1]
max_service_velocity = 50.0

[[axes]]
axis_id = 1
name = "Legacy"
"#;
    fs::write(dir.join("machine.toml"), content).unwrap();
    write_axis_toml(dir, 1, "x");

    let result = load_config_dir(dir);
    // Should fail because machine.toml has deny_unknown_fields and [[axes]] is unknown.
    assert!(result.is_err(), "legacy [[axes]] should be rejected");
}

/// Test: numeric bounds validation — kp out of range.
#[test]
fn numeric_bounds_kp_out_of_range() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);
    write_machine_toml(dir);

    let content = r#"
[axis]
id = 1
name = "X"
type = "linear"

[kinematics]
max_velocity = 500.0
safe_reduced_speed_limit = 50.0
min_pos = -100.0
max_pos = 1000.0

[control]
kp = 999999.0
ki = 20.0
kd = 0.5
out_max = 100.0
lag_error_limit = 0.5
lag_policy = "Unwanted"

[safe_stop]
category = "SS1"
max_decel_safe = 10000.0

[homing]
method = "HomeSensor"
speed = 20.0
timeout = 30.0
"#;
    fs::write(dir.join("axis_01_x.toml"), content).unwrap();

    let result = load_config_dir(dir);
    assert!(
        matches!(result, Err(ConfigError::ValidationError(_))),
        "expected ValidationError for kp out of range"
    );
}

/// Test: watchdog config validation — bounds.
#[test]
fn watchdog_bounds_validation() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    // Watchdog with out-of-range max_restarts.
    fs::write(
        dir.join("config.toml"),
        r#"
[watchdog]
max_restarts = 0

[hal]
[cu]
[re]
[mqtt]
[grpc]
[api]
[dashboard]
[diagnostic]
"#,
    )
    .unwrap();
    write_machine_toml(dir);
    write_axis_toml(dir, 1, "x");

    let result = load_config_dir(dir);
    assert!(
        matches!(result, Err(ConfigError::ValidationError(_))),
        "expected ValidationError for watchdog bounds"
    );
}

/// Test: load actual config/ directory with 8 axes.
#[test]
fn load_actual_config_directory() {
    // Use the real config/ directory.
    let config_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("config");

    if !config_dir.join("config.toml").exists() {
        // Skip if config directory not available.
        return;
    }

    let full = load_config_dir(&config_dir).expect("should load real config");
    assert_eq!(full.axes.len(), 8, "should have 8 axes");
    assert_eq!(full.axes[0].axis.name, "X-Axis");
    assert_eq!(full.axes[7].axis.name, "Tailstock");
    assert_eq!(full.machine.machine.name, "Test 8-Axis CNC");
}

/// Test: min_pos >= max_pos is rejected.
#[test]
fn position_range_validation() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    write_config_toml(dir);
    write_machine_toml(dir);

    let content = r#"
[axis]
id = 1
name = "X"
type = "linear"

[kinematics]
max_velocity = 500.0
safe_reduced_speed_limit = 50.0
min_pos = 1000.0
max_pos = 100.0

[control]
kp = 100.0
ki = 20.0
kd = 0.5
out_max = 100.0
lag_error_limit = 0.5
lag_policy = "Unwanted"

[safe_stop]
category = "SS1"
max_decel_safe = 10000.0

[homing]
method = "HomeSensor"
speed = 20.0
timeout = 30.0
"#;
    fs::write(dir.join("axis_01_x.toml"), content).unwrap();

    let result = load_config_dir(dir);
    assert!(
        matches!(result, Err(ConfigError::ValidationError(_))),
        "expected ValidationError for min_pos >= max_pos"
    );
}
