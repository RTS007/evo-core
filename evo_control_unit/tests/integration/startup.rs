//! Integration test: startup sequence (T089).
//!
//! Validates: config loading from TOML strings → all validations pass →
//! MachineState Starting → Idle transition with mock axis state.

use evo_common::control_unit::state::MachineState;

use evo_control_unit::config::load_config_from_strings;
use evo_control_unit::cycle::RuntimeState;
use evo_control_unit::state::machine::{MachineEvent, MachineStateMachine, TransitionResult};

// ── Minimal config TOML ─────────────────────────────────────────────

const CU_TOML: &str = r#"
cycle_time_us = 1000
max_axes = 2
machine_config_path = "test_machine.toml"
io_config_path = "test_io.toml"
"#;

const MACHINE_TOML: &str = r#"
[[axes]]
axis_id = 1
name = "X"
max_velocity = 500.0

[axes.control]
kp = 100.0
out_max = 50.0
lag_error_limit = 1.0

[axes.homing]
method = "NoHoming"

[[axes]]
axis_id = 2
name = "Y"
max_velocity = 500.0

[axes.control]
kp = 100.0
out_max = 50.0
lag_error_limit = 1.0

[axes.homing]
method = "NoHoming"
"#;

const IO_TOML: &str = r#"
[Safety]
name = "Safety circuits"
io = [
    { type = "di", role = "EStop", pin = 0, logic = "NC" },
]

[Axes]
name = "Axis limit switches"
io = [
    { type = "di", role = "LimitMin1", pin = 1, logic = "NO" },
    { type = "di", role = "LimitMax1", pin = 2, logic = "NO" },
    { type = "di", role = "LimitMin2", pin = 3, logic = "NO" },
    { type = "di", role = "LimitMax2", pin = 4, logic = "NO" },
]
"#;

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn config_loads_and_validates() {
    let config = load_config_from_strings(CU_TOML, MACHINE_TOML, IO_TOML);
    assert!(config.is_ok(), "config failed: {:?}", config.err());

    let cfg = config.unwrap();
    assert_eq!(cfg.cu_config.cycle_time_us, 1000);
    assert_eq!(cfg.cu_config.max_axes, 2);
    assert_eq!(cfg.machine.axes.len(), 2);
    assert_eq!(cfg.machine.axes[0].name, "X");
    assert_eq!(cfg.machine.axes[1].name, "Y");
}

#[test]
fn machine_state_stopped_to_idle() {
    let mut msm = MachineStateMachine::new();
    assert_eq!(msm.state(), MachineState::Stopped);

    // Transition to Starting.
    let result = msm.handle_event(MachineEvent::PowerOn);
    assert_eq!(result, TransitionResult::Ok(MachineState::Starting));

    // All axes initialized → transition to Idle.
    let result = msm.handle_event(MachineEvent::InitComplete);
    assert_eq!(result, TransitionResult::Ok(MachineState::Idle));
}

#[test]
fn runtime_state_initializes_for_2_axes() {
    let state = RuntimeState::new(2);
    assert_eq!(state.axis_count, 2);
    assert_eq!(state.machine_state, MachineState::Stopped);

    // All 64 slots are zeroed even though only 2 are active.
    for i in 0..64 {
        assert_eq!(state.axes[i].actual_position, 0.0);
        assert_eq!(state.axes[i].power_state, 0);
    }
}

#[test]
fn startup_full_sequence_config_to_runtime() {
    // End-to-end: load config → validate → init runtime state → start machine
    let cfg = load_config_from_strings(CU_TOML, MACHINE_TOML, IO_TOML)
        .expect("config load");

    let axis_count = cfg.machine.axes.len() as u8;
    let state = RuntimeState::new(axis_count);
    assert_eq!(state.axis_count, 2);

    // Machine state machine goes through startup.
    let mut msm = MachineStateMachine::new();
    msm.handle_event(MachineEvent::PowerOn);
    msm.handle_event(MachineEvent::InitComplete);
    assert_eq!(msm.state(), MachineState::Idle);

    // Verify axis configs loaded correctly.
    assert_eq!(cfg.machine.axes[0].max_velocity, 500.0);
    assert_eq!(cfg.machine.axes[1].max_velocity, 500.0);
}

#[test]
fn startup_rejects_invalid_config() {
    // Missing required EStop DI.
    let bad_io = r#"
[Axes]
io = [
    { type = "di", role = "LimitMin1", pin = 0, logic = "NO" },
]
"#;
    let result = load_config_from_strings(CU_TOML, MACHINE_TOML, bad_io);
    assert!(result.is_err(), "should fail without EStop");

    let err_msg = format!("{}", result.err().unwrap());
    assert!(
        err_msg.contains("EStop") || err_msg.contains("completeness"),
        "error should mention EStop or completeness: {err_msg}"
    );
}

#[test]
fn startup_init_failed_goes_to_system_error() {
    let mut msm = MachineStateMachine::new();
    msm.handle_event(MachineEvent::PowerOn);
    assert_eq!(msm.state(), MachineState::Starting);

    // Init failure → SystemError.
    let result = msm.handle_event(MachineEvent::InitFailed);
    assert_eq!(result, TransitionResult::Ok(MachineState::SystemError));
}

#[test]
fn idle_to_manual_to_idle() {
    let mut msm = MachineStateMachine::new();
    msm.handle_event(MachineEvent::PowerOn);
    msm.handle_event(MachineEvent::InitComplete);
    assert_eq!(msm.state(), MachineState::Idle);

    msm.handle_event(MachineEvent::ManualCommand);
    assert_eq!(msm.state(), MachineState::Manual);

    msm.handle_event(MachineEvent::ManualStop);
    assert_eq!(msm.state(), MachineState::Idle);
}

#[test]
fn reference_8axis_config_parses() {
    // T092: Validate the reference test fixtures parse correctly.
    let cu_toml = include_str!("../fixtures/test_cu.toml");
    let machine_toml = include_str!("../fixtures/test_8axis.toml");
    let io_toml = include_str!("../fixtures/test_io.toml");

    let result = load_config_from_strings(cu_toml, machine_toml, io_toml);
    assert!(result.is_ok(), "reference config parse failed: {:?}", result.err());

    let cfg = result.unwrap();
    assert_eq!(cfg.cu_config.cycle_time_us, 1000);
    assert_eq!(cfg.cu_config.max_axes, 8);
    assert_eq!(cfg.machine.axes.len(), 8);

    // Verify axis names.
    assert_eq!(cfg.machine.axes[0].name, "X-Axis");
    assert_eq!(cfg.machine.axes[7].name, "Tailstock");

    // Verify coupling: axis 8 is slave to axis 3.
    let tailstock = &cfg.machine.axes[7];
    assert!(tailstock.coupling.is_some());
    let coupling = tailstock.coupling.as_ref().unwrap();
    assert_eq!(coupling.master_axis, Some(3));
}
