//! GearboxState transitions (T076).
//!
//! Per-axis gearbox: Unknown → Neutral/GearN → Shifting → GearN/GearboxError.
//! NO_GEARSTEP is CRITICAL → SAFETY_STOP (I-GB-2).
//!
//! Gear change ONLY when MotionState == Standstill (I-GB-1).

use evo_common::control_unit::error::GearboxError;
use evo_common::control_unit::state::{GearboxState, MotionState};

/// Event that can trigger a GearboxState transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GearboxEvent {
    /// Gear sensor detects a valid gear number (1-4).
    GearDetected(u8),
    /// Gear sensor reads neutral.
    NeutralDetected,
    /// Gear change requested to target gear.
    ShiftRequested(u8),
    /// Shift completed successfully — new gear confirmed.
    ShiftComplete(u8),
    /// Shift timed out.
    ShiftTimeout,
    /// Sensor conflict detected.
    SensorConflict,
    /// Gear lost during motion (CRITICAL).
    GearLostDuringMotion,
}

/// Result of a gearbox transition attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GearboxTransition {
    /// Transition accepted — new state.
    Ok(GearboxState),
    /// Transition rejected — reason.
    Rejected(&'static str),
    /// Critical error — requires SAFETY_STOP.
    Critical(GearboxError),
}

/// Per-axis gearbox state machine.
#[derive(Debug, Clone)]
pub struct GearboxStateMachine {
    state: GearboxState,
    /// Target gear during shifting.
    target_gear: u8,
    /// Shift timeout counter [cycles].
    shift_timeout_cycles: u32,
    /// Max shift timeout [cycles].
    max_shift_timeout: u32,
}

impl GearboxStateMachine {
    /// Create a new gearbox machine.
    ///
    /// - `has_gearbox`: If false, state is permanently NoGearbox.
    /// - `max_shift_timeout`: Max cycles before shift timeout error.
    pub fn new(has_gearbox: bool, max_shift_timeout: u32) -> Self {
        Self {
            state: if has_gearbox {
                GearboxState::Unknown
            } else {
                GearboxState::NoGearbox
            },
            target_gear: 0,
            shift_timeout_cycles: 0,
            max_shift_timeout,
        }
    }

    /// Current gearbox state.
    #[inline]
    pub fn state(&self) -> GearboxState {
        self.state
    }

    /// Returns true if a valid gear is engaged and ready for motion.
    #[inline]
    pub fn is_ready(&self) -> bool {
        self.state.is_gear_engaged() || matches!(self.state, GearboxState::NoGearbox)
    }

    /// Handle a gearbox event.
    ///
    /// `motion_state` is passed to enforce I-GB-1 (shift only in Standstill).
    pub fn handle_event(
        &mut self,
        event: GearboxEvent,
        motion_state: MotionState,
    ) -> GearboxTransition {
        use GearboxEvent::*;

        match event {
            GearDetected(gear) => {
                if let Some(gs) = GearboxState::from_gear_number(gear) {
                    self.state = gs;
                    GearboxTransition::Ok(gs)
                } else {
                    GearboxTransition::Rejected("invalid gear number")
                }
            }

            NeutralDetected => {
                self.state = GearboxState::Neutral;
                GearboxTransition::Ok(GearboxState::Neutral)
            }

            ShiftRequested(target) => {
                // I-GB-1: Only shift in Standstill
                if motion_state != MotionState::Standstill {
                    return GearboxTransition::Rejected("gear shift requires Standstill");
                }

                if GearboxState::from_gear_number(target).is_none() {
                    return GearboxTransition::Rejected("invalid target gear");
                }

                self.target_gear = target;
                self.shift_timeout_cycles = 0;
                self.state = GearboxState::Shifting;
                GearboxTransition::Ok(GearboxState::Shifting)
            }

            ShiftComplete(gear) => {
                if self.state != GearboxState::Shifting {
                    return GearboxTransition::Rejected("not currently shifting");
                }

                if let Some(gs) = GearboxState::from_gear_number(gear) {
                    self.state = gs;
                    GearboxTransition::Ok(gs)
                } else {
                    self.state = GearboxState::GearboxError;
                    GearboxTransition::Ok(GearboxState::GearboxError)
                }
            }

            ShiftTimeout => {
                if self.state == GearboxState::Shifting {
                    self.state = GearboxState::GearboxError;
                    GearboxTransition::Ok(GearboxState::GearboxError)
                } else {
                    GearboxTransition::Rejected("not shifting")
                }
            }

            SensorConflict => {
                self.state = GearboxState::GearboxError;
                GearboxTransition::Ok(GearboxState::GearboxError)
            }

            GearLostDuringMotion => {
                // I-GB-2: NO_GEARSTEP is CRITICAL → SAFETY_STOP
                self.state = GearboxState::GearboxError;
                GearboxTransition::Critical(GearboxError::NO_GEARSTEP)
            }
        }
    }

    /// Tick shift timeout counter. Returns `true` if timeout occurred.
    ///
    /// Call once per cycle when state is Shifting.
    pub fn tick_shift_timeout(&mut self) -> bool {
        if self.state != GearboxState::Shifting {
            return false;
        }

        self.shift_timeout_cycles += 1;
        if self.shift_timeout_cycles >= self.max_shift_timeout {
            self.state = GearboxState::GearboxError;
            true
        } else {
            false
        }
    }

    /// Reset gearbox error — return to Unknown for re-detection.
    pub fn reset_error(&mut self) {
        if self.state == GearboxState::GearboxError {
            self.state = GearboxState::Unknown;
            self.shift_timeout_cycles = 0;
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_gearbox_is_always_ready() {
        let sm = GearboxStateMachine::new(false, 1000);
        assert_eq!(sm.state(), GearboxState::NoGearbox);
        assert!(sm.is_ready());
    }

    #[test]
    fn initial_unknown_not_ready() {
        let sm = GearboxStateMachine::new(true, 1000);
        assert_eq!(sm.state(), GearboxState::Unknown);
        assert!(!sm.is_ready());
    }

    #[test]
    fn detect_gear() {
        let mut sm = GearboxStateMachine::new(true, 1000);
        let r = sm.handle_event(GearboxEvent::GearDetected(2), MotionState::Standstill);
        assert!(matches!(r, GearboxTransition::Ok(GearboxState::Gear2)));
        assert!(sm.is_ready());
    }

    #[test]
    fn shift_only_in_standstill() {
        let mut sm = GearboxStateMachine::new(true, 1000);
        sm.handle_event(GearboxEvent::GearDetected(1), MotionState::Standstill);
        let r = sm.handle_event(GearboxEvent::ShiftRequested(2), MotionState::Accelerating);
        assert!(matches!(r, GearboxTransition::Rejected(_)));
        assert_eq!(sm.state(), GearboxState::Gear1);
    }

    #[test]
    fn shift_success() {
        let mut sm = GearboxStateMachine::new(true, 1000);
        sm.handle_event(GearboxEvent::GearDetected(1), MotionState::Standstill);
        sm.handle_event(GearboxEvent::ShiftRequested(2), MotionState::Standstill);
        assert_eq!(sm.state(), GearboxState::Shifting);
        let r = sm.handle_event(GearboxEvent::ShiftComplete(2), MotionState::Standstill);
        assert!(matches!(r, GearboxTransition::Ok(GearboxState::Gear2)));
    }

    #[test]
    fn shift_timeout() {
        let mut sm = GearboxStateMachine::new(true, 5);
        sm.handle_event(GearboxEvent::GearDetected(1), MotionState::Standstill);
        sm.handle_event(GearboxEvent::ShiftRequested(2), MotionState::Standstill);
        for _ in 0..4 {
            assert!(!sm.tick_shift_timeout());
        }
        assert!(sm.tick_shift_timeout());
        assert_eq!(sm.state(), GearboxState::GearboxError);
    }

    #[test]
    fn gear_lost_during_motion_is_critical() {
        let mut sm = GearboxStateMachine::new(true, 1000);
        sm.handle_event(GearboxEvent::GearDetected(1), MotionState::Standstill);
        let r = sm.handle_event(GearboxEvent::GearLostDuringMotion, MotionState::ConstantVelocity);
        assert!(matches!(r, GearboxTransition::Critical(_)));
        assert_eq!(sm.state(), GearboxState::GearboxError);
    }

    #[test]
    fn sensor_conflict_goes_to_error() {
        let mut sm = GearboxStateMachine::new(true, 1000);
        sm.handle_event(GearboxEvent::SensorConflict, MotionState::Standstill);
        assert_eq!(sm.state(), GearboxState::GearboxError);
    }

    #[test]
    fn reset_error() {
        let mut sm = GearboxStateMachine::new(true, 1000);
        sm.handle_event(GearboxEvent::SensorConflict, MotionState::Standstill);
        assert_eq!(sm.state(), GearboxState::GearboxError);
        sm.reset_error();
        assert_eq!(sm.state(), GearboxState::Unknown);
    }
}
