//! Command arbitration (RE vs RPC) (T043, T078, T100).
//!
//! Reads commands from evo_re_cu and evo_rpc_cu, enforces source lock
//! rules, and routes to appropriate state machines.
//!
//! This module processes the per-cycle command inputs and dispatches
//! them to the appropriate axis state machines.
//!
//! ## Hot-reload (T100 / FR-144–FR-147)
//!
//! `handle_reload_config` accepts `ReloadConfig` only when
//! `SafetyState == SafetyStop`, delegates to `atomic_config_swap`,
//! and returns a `ReloadOutcome` that the cycle writer maps to
//! an updated `evo_cu_mqt` snapshot.

use evo_common::control_unit::shm::{ReCommandType, RpcCommand, RpcCommandType};
use evo_common::control_unit::state::SafetyState;

use crate::config::{atomic_config_swap, LoadedConfig, ReloadResult};

/// Decoded command from either RE or RPC source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AxisCommand {
    /// No command.
    Nop,
    /// Enable axis power.
    EnableAxis { axis_id: u8 },
    /// Disable axis power.
    DisableAxis { axis_id: u8 },
    /// Move to absolute position.
    MoveAbsolute {
        axis_id: u8,
        position: f64,
        velocity: f64,
        acceleration: f64,
        deceleration: f64,
    },
    /// Move relative.
    MoveRelative {
        axis_id: u8,
        distance: f64,
        velocity: f64,
        acceleration: f64,
        deceleration: f64,
    },
    /// Move at constant velocity.
    MoveVelocity {
        axis_id: u8,
        velocity: f64,
        acceleration: f64,
    },
    /// Stop axis (controlled).
    Stop { axis_id: u8 },
    /// Emergency stop.
    EmergencyStop { axis_id: u8 },
    /// Start homing.
    Home { axis_id: u8 },
    /// Set operational mode.
    SetMode { axis_id: u8, mode: u8 },
    /// Couple axes.
    Couple { axis_id: u8 },
    /// Decouple axes.
    Decouple { axis_id: u8 },
    /// Gear change.
    GearChange { axis_id: u8, gear: u32 },
    /// Allow manual mode.
    AllowManualMode { axis_id: u8 },
    /// Jog positive.
    JogPositive { axis_id: u8, speed: f64 },
    /// Jog negative.
    JogNegative { axis_id: u8, speed: f64 },
    /// Jog stop.
    JogStop { axis_id: u8 },
    /// Reset error on axis.
    ResetError { axis_id: u8 },
    /// Set machine state (global command).
    SetMachineState { target_state: u8 },
    /// Acquire source lock.
    AcquireLock { axis_id: u8 },
    /// Release source lock.
    ReleaseLock { axis_id: u8 },
    /// Reload config (during SAFETY_STOP only, FR-145).
    ReloadConfig,
}

/// Source of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOrigin {
    RecipeExecutor,
    RpcApi,
}

/// Decode an RE command type into an AxisCommand.
///
/// This only translates the command type; axis-specific parameters
/// are extracted from the ReCommand payload by the caller.
pub fn decode_re_command(cmd_type: u8) -> Option<ReCommandType> {
    ReCommandType::from_u8(cmd_type)
}

/// Decode an RPC command type into an RpcCommandType.
pub fn decode_rpc_command(cmd_type: u8) -> Option<RpcCommandType> {
    RpcCommandType::from_u8(cmd_type)
}

/// Check if a command requires source lock on the target axis.
pub const fn requires_source_lock(cmd: &AxisCommand) -> bool {
    matches!(
        cmd,
        AxisCommand::EnableAxis { .. }
            | AxisCommand::DisableAxis { .. }
            | AxisCommand::MoveAbsolute { .. }
            | AxisCommand::MoveRelative { .. }
            | AxisCommand::MoveVelocity { .. }
            | AxisCommand::Home { .. }
            | AxisCommand::SetMode { .. }
            | AxisCommand::Couple { .. }
            | AxisCommand::Decouple { .. }
            | AxisCommand::GearChange { .. }
            | AxisCommand::JogPositive { .. }
            | AxisCommand::JogNegative { .. }
    )
}

/// Check if a command is a motion-initiating command.
pub const fn is_motion_command(cmd: &AxisCommand) -> bool {
    matches!(
        cmd,
        AxisCommand::MoveAbsolute { .. }
            | AxisCommand::MoveRelative { .. }
            | AxisCommand::MoveVelocity { .. }
            | AxisCommand::JogPositive { .. }
            | AxisCommand::JogNegative { .. }
            | AxisCommand::Home { .. }
    )
}

/// Dispatch an RPC command into an AxisCommand (T078).
///
/// Translates the raw `RpcCommand` struct (from evo_rpc_cu SHM segment)
/// into a typed `AxisCommand` enum that can be processed by the state machines.
///
/// Returns `None` for Nop or unrecognized command types.
pub fn dispatch_rpc_command(rpc: &RpcCommand) -> Option<AxisCommand> {
    let cmd_type = RpcCommandType::from_u8(rpc.command_type)?;
    let axis_id = rpc.axis_id;

    match cmd_type {
        RpcCommandType::Nop => None,

        RpcCommandType::JogPositive => Some(AxisCommand::JogPositive {
            axis_id,
            speed: rpc.param_f64,
        }),

        RpcCommandType::JogNegative => Some(AxisCommand::JogNegative {
            axis_id,
            speed: rpc.param_f64,
        }),

        RpcCommandType::JogStop => Some(AxisCommand::JogStop { axis_id }),

        RpcCommandType::MoveAbsolute => Some(AxisCommand::MoveAbsolute {
            axis_id,
            position: rpc.param_f64,
            velocity: 0.0,       // Caller fills from axis config defaults
            acceleration: 0.0,   // Caller fills from axis config defaults
            deceleration: 0.0,   // Caller fills from axis config defaults
        }),

        RpcCommandType::EnableAxis => Some(AxisCommand::EnableAxis { axis_id }),

        RpcCommandType::DisableAxis => Some(AxisCommand::DisableAxis { axis_id }),

        RpcCommandType::HomeAxis => Some(AxisCommand::Home { axis_id }),

        RpcCommandType::ResetError => Some(AxisCommand::ResetError { axis_id }),

        RpcCommandType::SetMachineState => Some(AxisCommand::SetMachineState {
            target_state: rpc.param_u32 as u8,
        }),

        RpcCommandType::SetMode => Some(AxisCommand::SetMode {
            axis_id,
            mode: rpc.param_u32 as u8,
        }),

        RpcCommandType::GearChange => Some(AxisCommand::GearChange {
            axis_id,
            gear: rpc.param_u32,
        }),

        RpcCommandType::AcquireLock => Some(AxisCommand::AcquireLock { axis_id }),

        RpcCommandType::ReleaseLock => Some(AxisCommand::ReleaseLock { axis_id }),

        RpcCommandType::AllowManualMode => Some(AxisCommand::AllowManualMode { axis_id }),

        RpcCommandType::ReloadConfig => Some(AxisCommand::ReloadConfig),
    }
}

// ─── Hot-reload Command Handler (T100 / FR-144–FR-147) ──────────────

/// Outcome of a `RELOAD_CONFIG` command attempt.
///
/// Returned by [`handle_reload_config`] so that the cycle writer can
/// update the `evo_cu_mqt` snapshot accordingly:
/// - `Accepted` → new config is live, MQT reflects updated axes.
/// - `Denied`   → system was not in `SafetyStop`, command rejected.
/// - `Failed`   → validation/parse error, active config unchanged.
#[derive(Debug, Clone, PartialEq)]
pub enum ReloadOutcome {
    /// Config reloaded successfully (FR-146: atomic swap complete).
    Accepted,
    /// Rejected — system not in SAFETY_STOP (FR-145).
    Denied(String),
    /// Shadow parse/validation failed — active config unchanged (FR-146 rollback).
    Failed(String),
}

/// Handle a `RELOAD_CONFIG` command (FR-144–FR-147).
///
/// # Safety-gate (FR-145)
/// Rejects the command with `ReloadOutcome::Denied` unless the global
/// safety state is `SafetyState::SafetyStop`. This ensures config changes
/// only occur while all axes are in a safe-stopped state.
///
/// # Atomic swap (FR-146)
/// Delegates to [`atomic_config_swap`] which:
/// 1. Parses + validates the shadow config.
/// 2. Checks reload scope (axis count, IDs, coupling topology unchanged).
/// 3. Swaps `active.machine` and `active.io_registry` in-place.
/// On failure, the active config is untouched.
///
/// # Timing (FR-147)
/// Zero RT-allocation: all TOML parsing uses `toml::from_str` (stack + heap
/// outside RT context). The caller must ensure this is invoked from a
/// non-RT context or that the cycle budget accommodates the parse time.
pub fn handle_reload_config(
    safety_state: SafetyState,
    active_config: &mut LoadedConfig,
    machine_toml: &str,
    io_toml: &str,
) -> ReloadOutcome {
    // FR-145: Reject unless in SAFETY_STOP.
    if safety_state != SafetyState::SafetyStop {
        return ReloadOutcome::Denied(format!(
            "ERR_RELOAD_DENIED: SafetyState is {:?}, expected SafetyStop",
            safety_state
        ));
    }

    // FR-146: Atomic swap with rollback on failure.
    match atomic_config_swap(active_config, machine_toml, io_toml) {
        ReloadResult::Success => ReloadOutcome::Accepted,
        ReloadResult::ValidationFailed(msg) => ReloadOutcome::Failed(msg),
        ReloadResult::Denied(msg) => ReloadOutcome::Denied(msg),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_re_command_types() {
        assert_eq!(decode_re_command(0), Some(ReCommandType::Nop));
        assert_eq!(decode_re_command(1), Some(ReCommandType::MoveAbsolute));
        assert_eq!(decode_re_command(7), Some(ReCommandType::EnableAxis));
        assert_eq!(decode_re_command(99), None);
    }

    #[test]
    fn decode_rpc_command_types() {
        assert_eq!(decode_rpc_command(0), Some(RpcCommandType::Nop));
        assert_eq!(decode_rpc_command(1), Some(RpcCommandType::JogPositive));
        assert_eq!(decode_rpc_command(15), Some(RpcCommandType::ReloadConfig));
        assert_eq!(decode_rpc_command(99), None);
    }

    #[test]
    fn motion_commands_require_lock() {
        assert!(requires_source_lock(&AxisCommand::MoveAbsolute {
            axis_id: 1,
            position: 0.0,
            velocity: 100.0,
            acceleration: 500.0,
            deceleration: 500.0,
        }));
        assert!(!requires_source_lock(&AxisCommand::Nop));
        assert!(!requires_source_lock(&AxisCommand::Stop { axis_id: 1 }));
    }

    #[test]
    fn is_motion_command_check() {
        assert!(is_motion_command(&AxisCommand::MoveAbsolute {
            axis_id: 1,
            position: 0.0,
            velocity: 100.0,
            acceleration: 500.0,
            deceleration: 500.0,
        }));
        assert!(is_motion_command(&AxisCommand::JogPositive {
            axis_id: 1,
            speed: 50.0,
        }));
        assert!(!is_motion_command(&AxisCommand::EnableAxis { axis_id: 1 }));
        assert!(!is_motion_command(&AxisCommand::Stop { axis_id: 1 }));
    }

    #[test]
    fn dispatch_rpc_jog_positive() {
        let rpc = RpcCommand {
            command_type: RpcCommandType::JogPositive as u8,
            axis_id: 3,
            _pad: [0; 6],
            param_f64: 25.0,
            param_u32: 0,
            sequence_id: 42,
        };
        let cmd = dispatch_rpc_command(&rpc).unwrap();
        assert!(matches!(cmd, AxisCommand::JogPositive { axis_id: 3, speed } if (speed - 25.0).abs() < 1e-12));
    }

    #[test]
    fn dispatch_rpc_nop_returns_none() {
        let rpc = RpcCommand::default();
        assert!(dispatch_rpc_command(&rpc).is_none());
    }

    #[test]
    fn dispatch_rpc_enable_axis() {
        let rpc = RpcCommand {
            command_type: RpcCommandType::EnableAxis as u8,
            axis_id: 1,
            _pad: [0; 6],
            param_f64: 0.0,
            param_u32: 0,
            sequence_id: 0,
        };
        let cmd = dispatch_rpc_command(&rpc).unwrap();
        assert!(matches!(cmd, AxisCommand::EnableAxis { axis_id: 1 }));
    }

    #[test]
    fn dispatch_rpc_set_machine_state() {
        let rpc = RpcCommand {
            command_type: RpcCommandType::SetMachineState as u8,
            axis_id: 0,
            _pad: [0; 6],
            param_f64: 0.0,
            param_u32: 3, // e.g., Active
            sequence_id: 0,
        };
        let cmd = dispatch_rpc_command(&rpc).unwrap();
        assert!(matches!(cmd, AxisCommand::SetMachineState { target_state: 3 }));
    }

    #[test]
    fn dispatch_rpc_reload_config() {
        let rpc = RpcCommand {
            command_type: RpcCommandType::ReloadConfig as u8,
            axis_id: 0,
            _pad: [0; 6],
            param_f64: 0.0,
            param_u32: 0,
            sequence_id: 0,
        };
        let cmd = dispatch_rpc_command(&rpc).unwrap();
        assert!(matches!(cmd, AxisCommand::ReloadConfig));
    }

    #[test]
    fn dispatch_invalid_rpc_returns_none() {
        let rpc = RpcCommand {
            command_type: 99,
            axis_id: 0,
            _pad: [0; 6],
            param_f64: 0.0,
            param_u32: 0,
            sequence_id: 0,
        };
        assert!(dispatch_rpc_command(&rpc).is_none());
    }

    // ── T100: RELOAD_CONFIG handler tests ──

    use crate::config::load_config_from_strings;

    fn cu_toml() -> &'static str {
        r#"
cycle_time_us = 1000
max_axes = 64
machine_config_path = "machine.toml"
io_config_path = "io.toml"
"#
    }

    fn machine_toml() -> &'static str {
        r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 500.0
"#
    }

    fn io_toml() -> &'static str {
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

    #[test]
    fn reload_denied_when_safe() {
        let mut config = load_config_from_strings(cu_toml(), machine_toml(), io_toml()).unwrap();
        let outcome = handle_reload_config(
            SafetyState::Safe,
            &mut config,
            machine_toml(),
            io_toml(),
        );
        assert!(matches!(outcome, ReloadOutcome::Denied(ref msg) if msg.contains("ERR_RELOAD_DENIED")),
            "expected Denied, got: {:?}", outcome);
    }

    #[test]
    fn reload_denied_when_reduced_speed() {
        let mut config = load_config_from_strings(cu_toml(), machine_toml(), io_toml()).unwrap();
        let outcome = handle_reload_config(
            SafetyState::SafeReducedSpeed,
            &mut config,
            machine_toml(),
            io_toml(),
        );
        assert!(matches!(outcome, ReloadOutcome::Denied(ref msg) if msg.contains("ERR_RELOAD_DENIED")),
            "expected Denied, got: {:?}", outcome);
    }

    #[test]
    fn reload_accepted_in_safety_stop() {
        let mut config = load_config_from_strings(cu_toml(), machine_toml(), io_toml()).unwrap();
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
            io_toml(),
        );
        assert_eq!(outcome, ReloadOutcome::Accepted);
        assert_eq!(config.machine.axes[0].max_velocity, 750.0);
    }

    #[test]
    fn reload_failed_invalid_toml() {
        let mut config = load_config_from_strings(cu_toml(), machine_toml(), io_toml()).unwrap();
        let outcome = handle_reload_config(
            SafetyState::SafetyStop,
            &mut config,
            "{{invalid toml",
            io_toml(),
        );
        assert!(matches!(outcome, ReloadOutcome::Failed(_)),
            "expected Failed, got: {:?}", outcome);
        // Active config unchanged
        assert_eq!(config.machine.axes[0].max_velocity, 500.0);
    }

    #[test]
    fn reload_failed_scope_violation_axis_count() {
        let mut config = load_config_from_strings(cu_toml(), machine_toml(), io_toml()).unwrap();
        let two_axes = r#"
[[axes]]
axis_id = 1
name = "X"
max_velocity = 500.0
[[axes]]
axis_id = 2
name = "Y"
max_velocity = 500.0
"#;
        let io2 = r#"
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
"#;
        let outcome = handle_reload_config(
            SafetyState::SafetyStop,
            &mut config,
            two_axes,
            io2,
        );
        assert!(matches!(outcome, ReloadOutcome::Failed(_)),
            "expected Failed, got: {:?}", outcome);
        // Active config unchanged — rollback (FR-146)
        assert_eq!(config.machine.axes.len(), 1);
    }

    #[test]
    fn reload_does_not_require_source_lock() {
        assert!(!requires_source_lock(&AxisCommand::ReloadConfig));
    }

    #[test]
    fn reload_is_not_motion_command() {
        assert!(!is_motion_command(&AxisCommand::ReloadConfig));
    }
}