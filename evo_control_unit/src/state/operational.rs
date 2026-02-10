//! OperationalMode management (T040, T059).
//!
//! Per-axis control mode: Position, Velocity, Torque, Manual, Test.
//! Mode changes only when Standstill + Standby. Coupled slaves locked to master mode.
//!
//! T059: Slave mode mirror — actively lock slave mode to match master (FR-041).
//!
//! Implements contracts/state-machines.md §5 and FR-040 through FR-042.

use evo_common::control_unit::state::{CouplingState, MotionState, OperationalMode, PowerState};

/// Events that trigger mode changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeEvent {
    /// Request to change operational mode.
    SetMode(OperationalMode),
}

/// Result of a mode change attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeTransition {
    /// Mode changed.
    Ok(OperationalMode),
    /// Mode change rejected.
    Rejected(&'static str),
}

/// Per-axis operational mode manager.
#[derive(Debug, Clone)]
pub struct OperationalModeMachine {
    mode: OperationalMode,
}

impl OperationalModeMachine {
    pub const fn new() -> Self {
        Self {
            mode: OperationalMode::Position,
        }
    }

    #[inline]
    pub const fn mode(&self) -> OperationalMode {
        self.mode
    }

    /// Attempt a mode change.
    ///
    /// Mode changes are only allowed when:
    /// - MotionState is Standstill (no active motion)
    /// - PowerState is Standby (drive ready, not in motion)
    /// - Axis is not a coupled slave (FR-042)
    pub fn set_mode(
        &mut self,
        new_mode: OperationalMode,
        motion_state: MotionState,
        power_state: PowerState,
        coupling_state: CouplingState,
    ) -> ModeTransition {
        // Guard: must be standstill.
        if motion_state != MotionState::Standstill {
            return ModeTransition::Rejected("mode change requires Standstill");
        }

        // Guard: must be standby.
        if power_state != PowerState::Standby {
            return ModeTransition::Rejected("mode change requires Standby");
        }

        // Guard: coupled slaves cannot change mode (FR-042).
        if matches!(
            coupling_state,
            CouplingState::SlaveCoupled | CouplingState::SlaveModulated
        ) {
            return ModeTransition::Rejected(
                "coupled slave cannot change mode (FR-042)",
            );
        }

        // Same mode = no-op success.
        if self.mode == new_mode {
            return ModeTransition::Ok(new_mode);
        }

        self.mode = new_mode;
        ModeTransition::Ok(new_mode)
    }

    /// Force mode (used when coupling overrides slave mode).
    pub fn force_mode(&mut self, mode: OperationalMode) {
        self.mode = mode;
    }

    /// Mirror master mode to slave (T059, FR-041).
    ///
    /// Called every cycle for coupled slave axes. If the master's mode
    /// differs from this slave's mode, force-update to match.
    ///
    /// Returns `true` if the mode was changed.
    pub fn mirror_master_mode(
        &mut self,
        master_mode: OperationalMode,
        coupling_state: CouplingState,
    ) -> bool {
        if matches!(
            coupling_state,
            CouplingState::SlaveCoupled | CouplingState::SlaveModulated
        ) && self.mode != master_mode
        {
            self.mode = master_mode;
            true
        } else {
            false
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_mode_is_position() {
        assert_eq!(OperationalModeMachine::new().mode(), OperationalMode::Position);
    }

    #[test]
    fn change_mode_when_standstill_and_standby() {
        let mut sm = OperationalModeMachine::new();
        assert_eq!(
            sm.set_mode(
                OperationalMode::Velocity,
                MotionState::Standstill,
                PowerState::Standby,
                CouplingState::Uncoupled,
            ),
            ModeTransition::Ok(OperationalMode::Velocity)
        );
        assert_eq!(sm.mode(), OperationalMode::Velocity);
    }

    #[test]
    fn reject_mode_change_when_moving() {
        let mut sm = OperationalModeMachine::new();
        assert!(matches!(
            sm.set_mode(
                OperationalMode::Velocity,
                MotionState::Accelerating,
                PowerState::Standby,
                CouplingState::Uncoupled,
            ),
            ModeTransition::Rejected(_)
        ));
    }

    #[test]
    fn reject_mode_change_when_not_standby() {
        let mut sm = OperationalModeMachine::new();
        assert!(matches!(
            sm.set_mode(
                OperationalMode::Velocity,
                MotionState::Standstill,
                PowerState::Motion,
                CouplingState::Uncoupled,
            ),
            ModeTransition::Rejected(_)
        ));
    }

    #[test]
    fn reject_mode_change_when_coupled_slave() {
        let mut sm = OperationalModeMachine::new();
        assert!(matches!(
            sm.set_mode(
                OperationalMode::Velocity,
                MotionState::Standstill,
                PowerState::Standby,
                CouplingState::SlaveCoupled,
            ),
            ModeTransition::Rejected(_)
        ));
    }

    #[test]
    fn same_mode_is_noop() {
        let mut sm = OperationalModeMachine::new();
        assert_eq!(
            sm.set_mode(
                OperationalMode::Position,
                MotionState::Standstill,
                PowerState::Standby,
                CouplingState::Uncoupled,
            ),
            ModeTransition::Ok(OperationalMode::Position)
        );
    }

    #[test]
    fn force_mode_overrides() {
        let mut sm = OperationalModeMachine::new();
        sm.force_mode(OperationalMode::Torque);
        assert_eq!(sm.mode(), OperationalMode::Torque);
    }

    // ── T059: Slave mode mirror ─────────────────────────────────────

    #[test]
    fn mirror_master_mode_when_coupled_slave() {
        let mut sm = OperationalModeMachine::new();
        assert_eq!(sm.mode(), OperationalMode::Position);

        // Mirror master's Velocity mode when slave is coupled.
        let changed = sm.mirror_master_mode(
            OperationalMode::Velocity,
            CouplingState::SlaveCoupled,
        );
        assert!(changed);
        assert_eq!(sm.mode(), OperationalMode::Velocity);
    }

    #[test]
    fn mirror_master_mode_noop_when_same() {
        let mut sm = OperationalModeMachine::new();
        let changed = sm.mirror_master_mode(
            OperationalMode::Position,
            CouplingState::SlaveCoupled,
        );
        assert!(!changed); // already same mode
    }

    #[test]
    fn mirror_master_mode_ignored_when_uncoupled() {
        let mut sm = OperationalModeMachine::new();
        let changed = sm.mirror_master_mode(
            OperationalMode::Torque,
            CouplingState::Uncoupled,
        );
        assert!(!changed);
        assert_eq!(sm.mode(), OperationalMode::Position); // unchanged
    }
}