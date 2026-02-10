//! Integration tests for hot-reload (T101 / FR-144–FR-147).
//!
//! Verifies the full reload pipeline:
//! - RELOAD_CONFIG accepted only during SAFETY_STOP (FR-145).
//! - Atomic config swap with rollback on failure (FR-146).
//! - Scope violations (axis count/ID/coupling topology) are rejected.
//! - Updated config is reflected in MQT snapshot fields.

use evo_common::control_unit::state::SafetyState;
use evo_control_unit::command::arbitration::{handle_reload_config, ReloadOutcome};
use evo_control_unit::config::load_config_from_strings;

// ─── Helpers ────────────────────────────────────────────────────────

fn cu_toml() -> &'static str {
    r#"
cycle_time_us = 1000
max_axes = 64
machine_config_path = "machine.toml"
io_config_path = "io.toml"
"#
}

fn machine_1axis() -> &'static str {
    r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 500.0
[axes.control]
kp = 100.0
ki = 50.0
kd = 10.0
"#
}

fn machine_2axis() -> &'static str {
    r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 500.0
[axes.control]
kp = 100.0

[[axes]]
axis_id = 2
name = "Y-Axis"
max_velocity = 400.0
[axes.control]
kp = 80.0
"#
}

fn io_1axis() -> &'static str {
    r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
"#
}

fn io_2axis() -> &'static str {
    r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
[Axis2]
io = [
    { type = "di", role = "LimitMin2", pin = 32, logic = "NC" },
    { type = "di", role = "LimitMax2", pin = 33, logic = "NC" },
]
"#
}

fn coupled_machine() -> &'static str {
    r#"
[[axes]]
axis_id = 1
name = "Master"
max_velocity = 500.0

[[axes]]
axis_id = 2
name = "Slave"
max_velocity = 500.0
[axes.coupling]
master_axis = 1
slave_axes = []
coupling_ratio = 2.0
"#
}

// ─── FR-145: SAFETY_STOP gate ───────────────────────────────────────

#[test]
fn reload_rejected_in_safe_state() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();
    let outcome = handle_reload_config(SafetyState::Safe, &mut config, machine_1axis(), io_1axis());
    assert!(
        matches!(outcome, ReloadOutcome::Denied(ref msg) if msg.contains("ERR_RELOAD_DENIED")),
        "Safe state should reject reload, got: {outcome:?}"
    );
}

#[test]
fn reload_rejected_in_reduced_speed() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();
    let outcome = handle_reload_config(
        SafetyState::SafeReducedSpeed,
        &mut config,
        machine_1axis(),
        io_1axis(),
    );
    assert!(
        matches!(outcome, ReloadOutcome::Denied(ref msg) if msg.contains("ERR_RELOAD_DENIED")),
        "ReducedSpeed should reject reload, got: {outcome:?}"
    );
}

#[test]
fn reload_accepted_in_safety_stop() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        machine_1axis(),
        io_1axis(),
    );
    assert_eq!(outcome, ReloadOutcome::Accepted);
}

// ─── FR-146: Atomic swap with rollback ─────────────────────────────

#[test]
fn reload_updates_pid_gains() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();
    assert_eq!(config.machine.axes[0].control.kp, 100.0);

    let updated = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 500.0
[axes.control]
kp = 300.0
ki = 150.0
kd = 25.0
"#;
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        updated,
        io_1axis(),
    );
    assert_eq!(outcome, ReloadOutcome::Accepted);
    assert_eq!(config.machine.axes[0].control.kp, 300.0);
    assert_eq!(config.machine.axes[0].control.ki, 150.0);
    assert_eq!(config.machine.axes[0].control.kd, 25.0);
}

#[test]
fn reload_updates_max_velocity() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();
    assert_eq!(config.machine.axes[0].max_velocity, 500.0);

    let updated = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 750.0
"#;
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        updated,
        io_1axis(),
    );
    assert_eq!(outcome, ReloadOutcome::Accepted);
    assert_eq!(config.machine.axes[0].max_velocity, 750.0);
}

#[test]
fn reload_rollback_on_invalid_toml() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();
    let original_kp = config.machine.axes[0].control.kp;

    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        "{{completely broken toml",
        io_1axis(),
    );
    assert!(matches!(outcome, ReloadOutcome::Failed(_)));
    // Config must be unchanged (FR-146 rollback).
    assert_eq!(config.machine.axes[0].control.kp, original_kp);
    assert_eq!(config.machine.axes.len(), 1);
}

#[test]
fn reload_rollback_on_axis_count_change() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();

    // Attempt to add axis 2 — non-reloadable scope.
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        machine_2axis(),
        io_2axis(),
    );
    assert!(
        matches!(outcome, ReloadOutcome::Failed(ref msg) if msg.contains("axis count")),
        "expected scope violation, got: {outcome:?}"
    );
    assert_eq!(config.machine.axes.len(), 1, "rollback failed");
}

#[test]
fn reload_rollback_on_axis_id_change() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();

    let changed_id = r#"
[[axes]]
axis_id = 5
name = "X-Axis"
max_velocity = 500.0
"#;
    let io5 = r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis5]
io = [
    { type = "di", role = "LimitMin5", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax5", pin = 31, logic = "NC" },
]
"#;
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        changed_id,
        io5,
    );
    assert!(
        matches!(outcome, ReloadOutcome::Failed(ref msg) if msg.contains("ID changed")),
        "expected ID scope violation, got: {outcome:?}"
    );
    assert_eq!(config.machine.axes[0].axis_id, 1, "rollback failed");
}

#[test]
fn reload_rollback_on_coupling_topology_change() {
    let mut config =
        load_config_from_strings(cu_toml(), coupled_machine(), io_2axis()).unwrap();

    // Remove coupling on axis 2 — topology change.
    let no_coupling = r#"
[[axes]]
axis_id = 1
name = "Master"
max_velocity = 500.0

[[axes]]
axis_id = 2
name = "Slave"
max_velocity = 500.0
"#;
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        no_coupling,
        io_2axis(),
    );
    assert!(
        matches!(outcome, ReloadOutcome::Failed(ref msg) if msg.contains("coupling")),
        "expected coupling scope violation, got: {outcome:?}"
    );
    // Coupling still present on axis 2.
    assert!(config.machine.axes[1].coupling.is_some(), "rollback failed");
}

// ─── FR-144: Multiple successive reloads ────────────────────────────

#[test]
fn successive_reloads_accumulate_changes() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();

    // First reload: change velocity.
    let v1 = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 600.0
[axes.control]
kp = 100.0
ki = 50.0
kd = 10.0
"#;
    let r1 = handle_reload_config(SafetyState::SafetyStop, &mut config, v1, io_1axis());
    assert_eq!(r1, ReloadOutcome::Accepted);
    assert_eq!(config.machine.axes[0].max_velocity, 600.0);

    // Second reload: change PID gains.
    let v2 = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 600.0
[axes.control]
kp = 200.0
ki = 75.0
kd = 15.0
"#;
    let r2 = handle_reload_config(SafetyState::SafetyStop, &mut config, v2, io_1axis());
    assert_eq!(r2, ReloadOutcome::Accepted);
    assert_eq!(config.machine.axes[0].max_velocity, 600.0);
    assert_eq!(config.machine.axes[0].control.kp, 200.0);
    assert_eq!(config.machine.axes[0].control.ki, 75.0);
}

// ─── FR-147: Timing constraint ──────────────────────────────────────

#[test]
fn reload_completes_within_120ms() {
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();

    let updated = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 750.0
[axes.control]
kp = 300.0
"#;

    let start = std::time::Instant::now();
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        updated,
        io_1axis(),
    );
    let elapsed = start.elapsed();

    assert_eq!(outcome, ReloadOutcome::Accepted);
    assert!(
        elapsed.as_millis() < 120,
        "FR-147: reload took {}ms, limit is 120ms",
        elapsed.as_millis()
    );
}

// ─── MQT snapshot reflects reload ──────────────────────────────────

#[test]
fn mqt_snapshot_reflects_updated_axis_count() {
    // After reload, the axis_count in config stays 1 (same),
    // so MQT snapshot written by cycle would use updated config.
    let mut config = load_config_from_strings(cu_toml(), machine_1axis(), io_1axis()).unwrap();
    assert_eq!(config.machine.axes.len(), 1);

    let updated = r#"
[[axes]]
axis_id = 1
name = "X-Axis-Updated"
max_velocity = 999.0
"#;
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        updated,
        io_1axis(),
    );
    assert_eq!(outcome, ReloadOutcome::Accepted);
    // Verify config reflects new values that MQT snapshot would pick up.
    assert_eq!(config.machine.axes[0].max_velocity, 999.0);
    assert_eq!(config.machine.axes[0].name, "X-Axis-Updated");
    assert_eq!(config.machine.axes.len(), 1);
}

// ─── 2-axis config reload ──────────────────────────────────────────

#[test]
fn reload_2axis_config_updates_both_axes() {
    let mut config =
        load_config_from_strings(cu_toml(), machine_2axis(), io_2axis()).unwrap();
    assert_eq!(config.machine.axes[0].control.kp, 100.0);
    assert_eq!(config.machine.axes[1].control.kp, 80.0);

    let updated = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 500.0
[axes.control]
kp = 250.0

[[axes]]
axis_id = 2
name = "Y-Axis"
max_velocity = 400.0
[axes.control]
kp = 180.0
"#;
    let outcome = handle_reload_config(
        SafetyState::SafetyStop,
        &mut config,
        updated,
        io_2axis(),
    );
    assert_eq!(outcome, ReloadOutcome::Accepted);
    assert_eq!(config.machine.axes[0].control.kp, 250.0);
    assert_eq!(config.machine.axes[1].control.kp, 180.0);
}
