//! MotionState transitions (T039) and unreferenced axis policy (T081).
//!
//! Per-axis motion lifecycle: Standstill → Accelerating → ConstantVelocity →
//! Decelerating → Standstill, with EmergencyStop and Homing.
//!
//! Implements contracts/state-machines.md §4.
//!
//! ## Unreferenced Axis Policy (FR-035)
//!
//! Unreferenced axes (referenced=false) are restricted:
//! - Only Manual(3) and Test(4) operational modes allowed.
//! - Velocity clamped to 5% of max_velocity.
//! - Software position limits disabled (no enforcement).
//! - Active commands (Position, Velocity modes) rejected with ERR_NOT_REFERENCED.

use evo_common::control_unit::state::{MotionState, OperationalMode};

/// Events that drive the MotionState machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionEvent {
    /// Motion command received (move, jog).
    StartMotion,
    /// Reached target velocity.
    ReachedVelocity,
    /// Began decelerating.
    Decelerating,
    /// Reached standstill.
    Standstill,
    /// Explicit stop command (controlled).
    Stop,
    /// Emergency stop (maximum deceleration).
    EmergencyStop,
    /// Homing command.
    StartHoming,
    /// Homing complete (reference found).
    HomingComplete,
    /// Homing failed.
    HomingFailed,
    /// Gear assist motion command.
    GearAssist,
    /// Gear assist complete.
    GearAssistComplete,
    /// Motion error detected (lag exceed, collision, etc.).
    MotionError,
    /// Error cleared + reset.
    ErrorReset,
}

/// Result of a MotionState transition attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MotionTransition {
    /// State changed.
    Ok(MotionState),
    /// Transition rejected.
    Rejected(&'static str),
}

/// Per-axis motion state machine.
#[derive(Debug, Clone)]
pub struct MotionStateMachine {
    state: MotionState,
}

impl MotionStateMachine {
    pub const fn new() -> Self {
        Self {
            state: MotionState::Standstill,
        }
    }

    #[inline]
    pub const fn state(&self) -> MotionState {
        self.state
    }

    /// Whether the axis is currently moving.
    #[inline]
    pub const fn is_moving(&self) -> bool {
        matches!(
            self.state,
            MotionState::Accelerating
                | MotionState::ConstantVelocity
                | MotionState::Decelerating
                | MotionState::Stopping
                | MotionState::EmergencyStop
                | MotionState::Homing
                | MotionState::GearAssistMotion
        )
    }

    /// Handle a motion event.
    pub fn handle_event(&mut self, event: MotionEvent) -> MotionTransition {
        use MotionEvent as E;
        use MotionState as S;

        let next = match (self.state, event) {
            // Standstill → Accelerating
            (S::Standstill, E::StartMotion) => S::Accelerating,

            // Accelerating → ConstantVelocity
            (S::Accelerating, E::ReachedVelocity) => S::ConstantVelocity,

            // Accelerating → Decelerating (short move)
            (S::Accelerating, E::Decelerating) => S::Decelerating,

            // ConstantVelocity → Decelerating
            (S::ConstantVelocity, E::Decelerating) => S::Decelerating,

            // Decelerating → Standstill
            (S::Decelerating, E::Standstill) => S::Standstill,

            // Any moving → Stopping (controlled stop)
            (S::Accelerating | S::ConstantVelocity | S::Decelerating, E::Stop) => S::Stopping,

            // Stopping → Standstill
            (S::Stopping, E::Standstill) => S::Standstill,

            // Any → EmergencyStop
            (
                S::Accelerating
                | S::ConstantVelocity
                | S::Decelerating
                | S::Stopping
                | S::Homing
                | S::GearAssistMotion,
                E::EmergencyStop,
            ) => S::EmergencyStop,

            // EmergencyStop → Standstill (once stopped)
            (S::EmergencyStop, E::Standstill) => S::Standstill,

            // Standstill → Homing
            (S::Standstill, E::StartHoming) => S::Homing,

            // Homing → Standstill (success)
            (S::Homing, E::HomingComplete) => S::Standstill,

            // Homing → MotionError (failure)
            (S::Homing, E::HomingFailed) => S::MotionError,

            // Standstill → GearAssistMotion
            (S::Standstill, E::GearAssist) => S::GearAssistMotion,

            // GearAssistMotion → Standstill
            (S::GearAssistMotion, E::GearAssistComplete) => S::Standstill,

            // Any → MotionError
            (_, E::MotionError) if self.state != S::MotionError => {
                self.state = S::MotionError;
                return MotionTransition::Ok(S::MotionError);
            }

            // MotionError → Standstill (after reset)
            (S::MotionError, E::ErrorReset) => S::Standstill,

            _ => return MotionTransition::Rejected("invalid motion transition"),
        };

        self.state = next;
        MotionTransition::Ok(next)
    }

    /// Force to EmergencyStop (called by safety subsystem).
    pub fn force_emergency_stop(&mut self) {
        if self.is_moving() {
            self.state = MotionState::EmergencyStop;
        }
    }

    /// Force to MotionError.
    pub fn force_error(&mut self) {
        self.state = MotionState::MotionError;
    }
}

// ─── Unreferenced Axis Policy (T081, FR-035) ────────────────────────

/// Fraction of max_velocity allowed for unreferenced axes.
pub const UNREFERENCED_SPEED_FRACTION: f64 = 0.05;

/// Check if the given operational mode is allowed for an unreferenced axis.
///
/// Only Manual and Test are permitted. All others are rejected.
#[inline]
pub fn is_mode_allowed_unreferenced(mode: OperationalMode) -> bool {
    matches!(mode, OperationalMode::Manual | OperationalMode::Test)
}

/// Calculate the maximum velocity for an unreferenced axis.
///
/// Returns `max_velocity * 0.05` (5% of configured max, FR-035).
#[inline]
pub fn unreferenced_velocity_limit(max_velocity: f64) -> f64 {
    max_velocity * UNREFERENCED_SPEED_FRACTION
}

/// Whether software position limits should be enforced.
///
/// Unreferenced axes have no valid position reference, so soft limits
/// are meaningless and must be disabled to avoid false triggering.
#[inline]
pub fn enforce_soft_limits(referenced: bool) -> bool {
    referenced
}

/// Check whether a motion command is allowed given reference status and mode.
///
/// Returns `Ok(())` if allowed, `Err(reason)` if rejected (FR-035).
pub fn check_unreferenced_policy(
    referenced: bool,
    mode: OperationalMode,
) -> Result<(), &'static str> {
    if referenced {
        return Ok(());
    }
    if is_mode_allowed_unreferenced(mode) {
        return Ok(());
    }
    Err("ERR_NOT_REFERENCED: unreferenced axis only allows Manual/Test mode")
}

/// Clamp velocity for an unreferenced axis.
///
/// If the axis is referenced, returns the velocity unchanged.
/// If unreferenced, clamps to ±5% of max_velocity.
#[inline]
pub fn clamp_unreferenced_velocity(velocity: f64, max_velocity: f64, referenced: bool) -> f64 {
    if referenced {
        return velocity;
    }
    let limit = unreferenced_velocity_limit(max_velocity);
    velocity.clamp(-limit, limit)
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use MotionEvent as E;
    use MotionState as S;

    #[test]
    fn initial_state_is_standstill() {
        assert_eq!(MotionStateMachine::new().state(), S::Standstill);
    }

    #[test]
    fn normal_motion_cycle() {
        let mut sm = MotionStateMachine::new();
        assert_eq!(sm.handle_event(E::StartMotion), MotionTransition::Ok(S::Accelerating));
        assert_eq!(sm.handle_event(E::ReachedVelocity), MotionTransition::Ok(S::ConstantVelocity));
        assert_eq!(sm.handle_event(E::Decelerating), MotionTransition::Ok(S::Decelerating));
        assert_eq!(sm.handle_event(E::Standstill), MotionTransition::Ok(S::Standstill));
    }

    #[test]
    fn short_move_accel_to_decel() {
        let mut sm = MotionStateMachine::new();
        sm.handle_event(E::StartMotion);
        assert_eq!(sm.handle_event(E::Decelerating), MotionTransition::Ok(S::Decelerating));
    }

    #[test]
    fn controlled_stop() {
        let mut sm = MotionStateMachine { state: S::ConstantVelocity };
        assert_eq!(sm.handle_event(E::Stop), MotionTransition::Ok(S::Stopping));
        assert_eq!(sm.handle_event(E::Standstill), MotionTransition::Ok(S::Standstill));
    }

    #[test]
    fn emergency_stop_from_moving_states() {
        for state in [S::Accelerating, S::ConstantVelocity, S::Decelerating, S::Stopping, S::Homing, S::GearAssistMotion] {
            let mut sm = MotionStateMachine { state };
            assert_eq!(
                sm.handle_event(E::EmergencyStop),
                MotionTransition::Ok(S::EmergencyStop),
                "EmergencyStop from {state:?}"
            );
        }
    }

    #[test]
    fn emergency_stop_to_standstill() {
        let mut sm = MotionStateMachine { state: S::EmergencyStop };
        assert_eq!(sm.handle_event(E::Standstill), MotionTransition::Ok(S::Standstill));
    }

    #[test]
    fn homing_success() {
        let mut sm = MotionStateMachine::new();
        assert_eq!(sm.handle_event(E::StartHoming), MotionTransition::Ok(S::Homing));
        assert_eq!(sm.handle_event(E::HomingComplete), MotionTransition::Ok(S::Standstill));
    }

    #[test]
    fn homing_failure() {
        let mut sm = MotionStateMachine { state: S::Homing };
        assert_eq!(sm.handle_event(E::HomingFailed), MotionTransition::Ok(S::MotionError));
    }

    #[test]
    fn gear_assist_motion() {
        let mut sm = MotionStateMachine::new();
        assert_eq!(sm.handle_event(E::GearAssist), MotionTransition::Ok(S::GearAssistMotion));
        assert_eq!(sm.handle_event(E::GearAssistComplete), MotionTransition::Ok(S::Standstill));
    }

    #[test]
    fn motion_error_and_reset() {
        let mut sm = MotionStateMachine { state: S::Accelerating };
        assert_eq!(sm.handle_event(E::MotionError), MotionTransition::Ok(S::MotionError));
        assert_eq!(sm.handle_event(E::ErrorReset), MotionTransition::Ok(S::Standstill));
    }

    #[test]
    fn invalid_transitions_rejected() {
        let mut sm = MotionStateMachine::new();
        assert!(matches!(
            sm.handle_event(E::ReachedVelocity),
            MotionTransition::Rejected(_)
        ));
    }

    #[test]
    fn is_moving_check() {
        assert!(!MotionStateMachine { state: S::Standstill }.is_moving());
        assert!(MotionStateMachine { state: S::Accelerating }.is_moving());
        assert!(MotionStateMachine { state: S::Homing }.is_moving());
        assert!(!MotionStateMachine { state: S::MotionError }.is_moving());
    }

    // ── T081: Unreferenced axis policy tests ──

    #[test]
    fn unreferenced_mode_allowed_manual() {
        assert!(is_mode_allowed_unreferenced(OperationalMode::Manual));
    }

    #[test]
    fn unreferenced_mode_allowed_test() {
        assert!(is_mode_allowed_unreferenced(OperationalMode::Test));
    }

    #[test]
    fn unreferenced_mode_rejected_position() {
        assert!(!is_mode_allowed_unreferenced(OperationalMode::Position));
    }

    #[test]
    fn unreferenced_mode_rejected_velocity() {
        assert!(!is_mode_allowed_unreferenced(OperationalMode::Velocity));
    }

    #[test]
    fn unreferenced_mode_rejected_torque() {
        assert!(!is_mode_allowed_unreferenced(OperationalMode::Torque));
    }

    #[test]
    fn unreferenced_velocity_5_percent() {
        let max = 1000.0;
        let limit = unreferenced_velocity_limit(max);
        assert!((limit - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unreferenced_velocity_clamp() {
        let max = 500.0;
        // Referenced → no clamp.
        assert_eq!(clamp_unreferenced_velocity(500.0, max, true), 500.0);
        // Unreferenced → clamp to ±25 (5% of 500).
        assert_eq!(clamp_unreferenced_velocity(500.0, max, false), 25.0);
        assert_eq!(clamp_unreferenced_velocity(-500.0, max, false), -25.0);
        assert_eq!(clamp_unreferenced_velocity(10.0, max, false), 10.0);
    }

    #[test]
    fn soft_limits_enforcement() {
        assert!(enforce_soft_limits(true));
        assert!(!enforce_soft_limits(false));
    }

    #[test]
    fn unreferenced_policy_check() {
        // Referenced → always OK.
        assert!(check_unreferenced_policy(true, OperationalMode::Position).is_ok());
        // Unreferenced + Manual → OK.
        assert!(check_unreferenced_policy(false, OperationalMode::Manual).is_ok());
        // Unreferenced + Test → OK.
        assert!(check_unreferenced_policy(false, OperationalMode::Test).is_ok());
        // Unreferenced + Position → ERR.
        let err = check_unreferenced_policy(false, OperationalMode::Position);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("ERR_NOT_REFERENCED"));
    }
}