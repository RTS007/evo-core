//! LEVEL 2: SafetyState management (T042, part of safety monitoring).
//!
//! Global safety overlay: Safe ↔ SafeReducedSpeed → SafetyStop.
//! SafetyStop forces MachineState → SystemError.
//!
//! Implements FR-010 through FR-012.

use evo_common::control_unit::state::SafetyState;

/// Events that drive the SafetyState machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyEvent {
    /// All safety conditions OK.
    AllOk,
    /// Maintenance key + guard open → reduced speed.
    ReducedSpeed,
    /// E-Stop, light curtain, safety door, critical fault.
    SafetyStop,
    /// Recovery: explicit reset + all conditions cleared + authorization.
    Recovery,
}

/// Result of a SafetyState transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyTransition {
    Ok(SafetyState),
    Rejected(&'static str),
}

/// Global safety state machine.
#[derive(Debug, Clone)]
pub struct SafetyStateMachine {
    state: SafetyState,
}

impl SafetyStateMachine {
    pub const fn new() -> Self {
        Self {
            state: SafetyState::Safe,
        }
    }

    #[inline]
    pub const fn state(&self) -> SafetyState {
        self.state
    }

    /// Handle a safety event.
    pub fn handle_event(&mut self, event: SafetyEvent) -> SafetyTransition {
        use SafetyEvent as E;
        use SafetyState as S;

        let next = match (self.state, event) {
            // Safe ↔ SafeReducedSpeed
            (S::Safe, E::ReducedSpeed) => S::SafeReducedSpeed,
            (S::SafeReducedSpeed, E::AllOk) => S::Safe,

            // Any → SafetyStop
            (S::Safe | S::SafeReducedSpeed, E::SafetyStop) => S::SafetyStop,

            // SafetyStop → Safe (recovery)
            (S::SafetyStop, E::Recovery) => S::Safe,

            // Already in SafetyStop, stop again is idempotent
            (S::SafetyStop, E::SafetyStop) => S::SafetyStop,

            // AllOk when already Safe is no-op
            (S::Safe, E::AllOk) => S::Safe,

            _ => return SafetyTransition::Rejected("invalid safety transition"),
        };

        self.state = next;
        SafetyTransition::Ok(next)
    }

    /// Force to SafetyStop (called by error propagation).
    pub fn force_safety_stop(&mut self) {
        self.state = SafetyState::SafetyStop;
    }

    /// Whether the safety state requires emergency stop of all axes.
    #[inline]
    pub const fn requires_emergency_stop(&self) -> bool {
        matches!(self.state, SafetyState::SafetyStop)
    }

    /// Whether safe reduced speed limit applies.
    #[inline]
    pub const fn requires_reduced_speed(&self) -> bool {
        matches!(self.state, SafetyState::SafeReducedSpeed)
    }
}

// ─── T050a: SAFE_REDUCED_SPEED Velocity Clamping (FR-011) ──────────

/// Clamp a target velocity to the safe reduced speed limit.
///
/// When `SafetyState == SafeReducedSpeed`, enforce hardware speed limit
/// on all axes by clamping `TargetVelocity` before writing to HAL.
///
/// The sign (direction) of the velocity is preserved.
#[inline]
pub fn clamp_velocity_for_safety(velocity: f64, limit: f64) -> f64 {
    let abs_limit = limit.abs();
    if velocity > abs_limit {
        abs_limit
    } else if velocity < -abs_limit {
        -abs_limit
    } else {
        velocity
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use SafetyEvent as E;
    use SafetyState as S;

    #[test]
    fn initial_state_is_safe() {
        assert_eq!(SafetyStateMachine::new().state(), S::Safe);
    }

    #[test]
    fn safe_to_reduced_speed_and_back() {
        let mut sm = SafetyStateMachine::new();
        assert_eq!(sm.handle_event(E::ReducedSpeed), SafetyTransition::Ok(S::SafeReducedSpeed));
        assert_eq!(sm.handle_event(E::AllOk), SafetyTransition::Ok(S::Safe));
    }

    #[test]
    fn safety_stop_from_safe() {
        let mut sm = SafetyStateMachine::new();
        assert_eq!(sm.handle_event(E::SafetyStop), SafetyTransition::Ok(S::SafetyStop));
        assert!(sm.requires_emergency_stop());
    }

    #[test]
    fn safety_stop_from_reduced() {
        let mut sm = SafetyStateMachine { state: S::SafeReducedSpeed };
        assert_eq!(sm.handle_event(E::SafetyStop), SafetyTransition::Ok(S::SafetyStop));
    }

    #[test]
    fn recovery_from_safety_stop() {
        let mut sm = SafetyStateMachine { state: S::SafetyStop };
        assert_eq!(sm.handle_event(E::Recovery), SafetyTransition::Ok(S::Safe));
    }

    #[test]
    fn safety_stop_is_idempotent() {
        let mut sm = SafetyStateMachine { state: S::SafetyStop };
        assert_eq!(sm.handle_event(E::SafetyStop), SafetyTransition::Ok(S::SafetyStop));
    }

    #[test]
    fn requires_reduced_speed() {
        assert!(!SafetyStateMachine { state: S::Safe }.requires_reduced_speed());
        assert!(SafetyStateMachine { state: S::SafeReducedSpeed }.requires_reduced_speed());
        assert!(!SafetyStateMachine { state: S::SafetyStop }.requires_reduced_speed());
    }

    // ── T050a: velocity clamping tests ──

    #[test]
    fn clamp_velocity_within_limit() {
        assert_eq!(super::clamp_velocity_for_safety(5.0, 10.0), 5.0);
        assert_eq!(super::clamp_velocity_for_safety(-5.0, 10.0), -5.0);
    }

    #[test]
    fn clamp_velocity_exceeds_limit() {
        assert_eq!(super::clamp_velocity_for_safety(50.0, 10.0), 10.0);
        assert_eq!(super::clamp_velocity_for_safety(-50.0, 10.0), -10.0);
    }

    #[test]
    fn clamp_velocity_at_boundary() {
        assert_eq!(super::clamp_velocity_for_safety(10.0, 10.0), 10.0);
        assert_eq!(super::clamp_velocity_for_safety(-10.0, 10.0), -10.0);
    }
}