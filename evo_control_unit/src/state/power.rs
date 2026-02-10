//! PowerState transitions with POWERING_ON/OFF sequences (T037, T038).
//!
//! Multi-step enable sequence (check safety → enable drive → release brake →
//! verify position → Standby) and disable sequence.
//!
//! Implements contracts/state-machines.md §3 with invariants I-PW-1 through I-PW-4.

use evo_common::control_unit::state::PowerState;

// ─── Power-On Sequence (FR-021) ─────────────────────────────────────

/// Steps of the POWERING_ON sequence (FR-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerOnStep {
    /// Step 0: Check safety flags (all must be OK).
    CheckSafety = 0,
    /// Step 1: Send drive enable command.
    EnableDrive = 1,
    /// Step 2: Wait drive_ready (timeout: 5s).
    WaitDriveReady = 2,
    /// Step 3: Release brake (if BrakeConfig present).
    ReleaseBrake = 3,
    /// Step 4: Wait brake released confirmation.
    WaitBrakeReleased = 4,
    /// Step 5: For gravity axes: check position stable.
    CheckPositionStable = 5,
    /// Step 6: Zero PID integral and filter states.
    ResetControlState = 6,
    /// Step 7: Transition to Standby.
    Complete = 7,
}

impl PowerOnStep {
    /// Advance to the next step.
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::CheckSafety => Some(Self::EnableDrive),
            Self::EnableDrive => Some(Self::WaitDriveReady),
            Self::WaitDriveReady => Some(Self::ReleaseBrake),
            Self::ReleaseBrake => Some(Self::WaitBrakeReleased),
            Self::WaitBrakeReleased => Some(Self::CheckPositionStable),
            Self::CheckPositionStable => Some(Self::ResetControlState),
            Self::ResetControlState => Some(Self::Complete),
            Self::Complete => None,
        }
    }
}

/// Steps of the POWERING_OFF sequence (FR-022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerOffStep {
    /// Step 0: Check position for locking pin insertion.
    CheckLockPosition = 0,
    /// Step 1: Engage brake, wait for confirmation.
    EngageBrake = 1,
    /// Step 2: Verify position held.
    VerifyPosition = 2,
    /// Step 3: Reduce drive torque gradually.
    ReduceTorque = 3,
    /// Step 4: Disable drive.
    DisableDrive = 4,
    /// Step 5: Extend locking pin (if applicable).
    ExtendLockPin = 5,
    /// Step 6: Transition to PowerOff.
    Complete = 6,
}

impl PowerOffStep {
    /// Advance to the next step.
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::CheckLockPosition => Some(Self::EngageBrake),
            Self::EngageBrake => Some(Self::VerifyPosition),
            Self::VerifyPosition => Some(Self::ReduceTorque),
            Self::ReduceTorque => Some(Self::DisableDrive),
            Self::DisableDrive => Some(Self::ExtendLockPin),
            Self::ExtendLockPin => Some(Self::Complete),
            Self::Complete => None,
        }
    }
}

// ─── Sequence Tracker ───────────────────────────────────────────────

/// Tracks the current step of POWERING_ON or POWERING_OFF sequences
/// with per-step elapsed time for timeout detection (T038).
#[derive(Debug, Clone, Copy)]
pub struct SequenceTracker {
    /// Current step index (either PowerOnStep or PowerOffStep as u8).
    pub step: u8,
    /// Cycles spent in current step.
    pub step_cycles: u32,
}

impl SequenceTracker {
    pub const fn new() -> Self {
        Self {
            step: 0,
            step_cycles: 0,
        }
    }

    /// Advance to next step, resetting the timer.
    pub fn advance(&mut self) {
        self.step += 1;
        self.step_cycles = 0;
    }

    /// Tick one cycle.
    pub fn tick(&mut self) {
        self.step_cycles = self.step_cycles.saturating_add(1);
    }

    /// Check if current step has exceeded the given timeout (in cycles).
    pub const fn timed_out(&self, timeout_cycles: u32) -> bool {
        timeout_cycles > 0 && self.step_cycles >= timeout_cycles
    }
}

// ─── Power Event ────────────────────────────────────────────────────

/// Events that drive the PowerState machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    /// Enable command received + no safety block.
    Enable,
    /// Current sequence step completed.
    StepComplete,
    /// Sequence step timed out.
    StepTimeout,
    /// Motion command received (Standby → Motion).
    MotionCommand,
    /// Motion complete + standstill detected (Motion → Standby).
    MotionComplete,
    /// Disable command received.
    Disable,
    /// CRITICAL drive fault.
    DriveFault,
    /// Error cleared + reset command.
    ErrorReset,
    /// Service NoBrake command (PowerOff → NoBrake).
    NoBrakeEnter,
    /// End NoBrake command (NoBrake → PowerOff).
    NoBrakeExit,
}

/// Result of a PowerState transition attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerTransition {
    /// State changed.
    Ok(PowerState),
    /// Transition rejected.
    Rejected(&'static str),
    /// Sequence step advanced (still in same power state).
    StepAdvanced(u8),
}

// ─── PowerState Machine ─────────────────────────────────────────────

/// Per-axis power state machine.
#[derive(Debug, Clone)]
pub struct PowerStateMachine {
    state: PowerState,
    /// Sequence tracker for POWERING_ON / POWERING_OFF.
    pub sequence: SequenceTracker,
    /// Whether the axis has a brake configured.
    pub has_brake: bool,
    /// Whether the axis has a locking pin.
    pub has_lock_pin: bool,
    /// Whether the axis is gravity-affected (needs position stability check).
    pub is_gravity_axis: bool,
}

impl PowerStateMachine {
    /// Create a new PowerState machine in PowerOff state.
    pub const fn new(has_brake: bool, has_lock_pin: bool, is_gravity_axis: bool) -> Self {
        Self {
            state: PowerState::PowerOff,
            sequence: SequenceTracker::new(),
            has_brake,
            has_lock_pin,
            is_gravity_axis,
        }
    }

    /// Current state.
    #[inline]
    pub const fn state(&self) -> PowerState {
        self.state
    }

    /// Whether motion output is allowed (I-PW-1).
    #[inline]
    pub const fn allows_motion_output(&self) -> bool {
        matches!(self.state, PowerState::Motion)
    }

    /// Handle a power event.
    pub fn handle_event(
        &mut self,
        event: PowerEvent,
        is_service: bool,
    ) -> PowerTransition {
        use PowerEvent::*;
        use PowerState::*;

        match (self.state, event) {
            // PowerOff → PoweringOn
            (PowerOff, Enable) => {
                self.state = PoweringOn;
                self.sequence = SequenceTracker::new();
                PowerTransition::Ok(PoweringOn)
            }

            // PoweringOn: step complete → advance or finish
            (PoweringOn, StepComplete) => {
                let current_step = self.sequence.step;

                // Skip steps that don't apply to this axis.
                let final_step = PowerOnStep::Complete as u8;

                if current_step >= final_step {
                    // Sequence complete → Standby (I-PW-4: reset control state).
                    self.state = Standby;
                    self.sequence = SequenceTracker::new();
                    return PowerTransition::Ok(Standby);
                }

                // Advance to next step.
                self.sequence.advance();
                // Skip brake steps if no brake.
                self.skip_inapplicable_on_steps();
                PowerTransition::StepAdvanced(self.sequence.step)
            }

            // PoweringOn: timeout → PowerError
            (PoweringOn, StepTimeout) => {
                self.state = PowerError;
                PowerTransition::Ok(PowerError)
            }

            // PoweringOn: drive fault → PowerError
            (PoweringOn, DriveFault) => {
                self.state = PowerError;
                PowerTransition::Ok(PowerError)
            }

            // Standby → Motion
            (Standby, MotionCommand) => {
                self.state = Motion;
                PowerTransition::Ok(Motion)
            }

            // Motion → Standby
            (Motion, MotionComplete) => {
                self.state = Standby;
                PowerTransition::Ok(Standby)
            }

            // Standby/Motion → PoweringOff
            (Standby, Disable) | (Motion, Disable) => {
                self.state = PoweringOff;
                self.sequence = SequenceTracker::new();
                PowerTransition::Ok(PoweringOff)
            }

            // PoweringOff: step complete → advance or finish
            (PoweringOff, StepComplete) => {
                let final_step = PowerOffStep::Complete as u8;

                if self.sequence.step >= final_step {
                    self.state = PowerOff;
                    self.sequence = SequenceTracker::new();
                    return PowerTransition::Ok(PowerOff);
                }

                self.sequence.advance();
                self.skip_inapplicable_off_steps();
                PowerTransition::StepAdvanced(self.sequence.step)
            }

            // PoweringOff: timeout → PowerError
            (PoweringOff, StepTimeout) => {
                self.state = PowerError;
                PowerTransition::Ok(PowerError)
            }

            // any → PowerError on drive fault
            (_, DriveFault) => {
                self.state = PowerError;
                PowerTransition::Ok(PowerError)
            }

            // PowerError → PowerOff (after error reset)
            (PowerError, ErrorReset) => {
                self.state = PowerOff;
                self.sequence = SequenceTracker::new();
                PowerTransition::Ok(PowerOff)
            }

            // PowerOff → NoBrake (Service mode only, I-PW-3)
            (PowerOff, NoBrakeEnter) if is_service => {
                self.state = NoBrake;
                PowerTransition::Ok(NoBrake)
            }
            (PowerOff, NoBrakeEnter) => {
                PowerTransition::Rejected("NoBrake only available in Service mode")
            }

            // NoBrake → PowerOff
            (NoBrake, NoBrakeExit) => {
                self.state = PowerOff;
                PowerTransition::Ok(PowerOff)
            }

            _ => PowerTransition::Rejected("invalid power transition"),
        }
    }

    /// Tick the sequence tracker (call once per cycle during PoweringOn/Off).
    pub fn tick_sequence(&mut self) {
        if matches!(
            self.state,
            PowerState::PoweringOn | PowerState::PoweringOff
        ) {
            self.sequence.tick();
        }
    }

    /// Force transition to PowerError (e.g., from safety subsystem).
    pub fn force_error(&mut self) {
        self.state = PowerState::PowerError;
    }

    /// Skip inapplicable POWERING_ON steps.
    fn skip_inapplicable_on_steps(&mut self) {
        loop {
            match power_on_step_from_u8(self.sequence.step) {
                Some(PowerOnStep::ReleaseBrake) | Some(PowerOnStep::WaitBrakeReleased)
                    if !self.has_brake =>
                {
                    self.sequence.advance();
                }
                Some(PowerOnStep::CheckPositionStable) if !self.is_gravity_axis => {
                    self.sequence.advance();
                }
                _ => break,
            }
        }
    }

    /// Skip inapplicable POWERING_OFF steps.
    fn skip_inapplicable_off_steps(&mut self) {
        loop {
            match power_off_step_from_u8(self.sequence.step) {
                Some(PowerOffStep::CheckLockPosition) | Some(PowerOffStep::ExtendLockPin)
                    if !self.has_lock_pin =>
                {
                    self.sequence.advance();
                }
                Some(PowerOffStep::EngageBrake) if !self.has_brake => {
                    self.sequence.advance();
                }
                _ => break,
            }
        }
    }
}

fn power_on_step_from_u8(v: u8) -> Option<PowerOnStep> {
    match v {
        0 => Some(PowerOnStep::CheckSafety),
        1 => Some(PowerOnStep::EnableDrive),
        2 => Some(PowerOnStep::WaitDriveReady),
        3 => Some(PowerOnStep::ReleaseBrake),
        4 => Some(PowerOnStep::WaitBrakeReleased),
        5 => Some(PowerOnStep::CheckPositionStable),
        6 => Some(PowerOnStep::ResetControlState),
        7 => Some(PowerOnStep::Complete),
        _ => None,
    }
}

fn power_off_step_from_u8(v: u8) -> Option<PowerOffStep> {
    match v {
        0 => Some(PowerOffStep::CheckLockPosition),
        1 => Some(PowerOffStep::EngageBrake),
        2 => Some(PowerOffStep::VerifyPosition),
        3 => Some(PowerOffStep::ReduceTorque),
        4 => Some(PowerOffStep::DisableDrive),
        5 => Some(PowerOffStep::ExtendLockPin),
        6 => Some(PowerOffStep::Complete),
        _ => None,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use PowerEvent::*;
    use PowerState::*;

    fn simple_axis() -> PowerStateMachine {
        PowerStateMachine::new(true, false, false)
    }

    fn no_brake_axis() -> PowerStateMachine {
        PowerStateMachine::new(false, false, false)
    }

    #[test]
    fn initial_state_is_power_off() {
        assert_eq!(simple_axis().state(), PowerOff);
    }

    #[test]
    fn enable_starts_powering_on() {
        let mut sm = simple_axis();
        assert_eq!(sm.handle_event(Enable, false), PowerTransition::Ok(PoweringOn));
    }

    #[test]
    fn full_power_on_sequence_with_brake() {
        let mut sm = simple_axis();
        sm.handle_event(Enable, false);
        assert_eq!(sm.state(), PoweringOn);
        assert_eq!(sm.sequence.step, 0); // CheckSafety

        // Step through the sequence.
        for expected_step in 1..=7 {
            let _result = sm.handle_event(StepComplete, false);
            if expected_step == 7 {
                // Complete → Standby
                break;
            }
        }

        // After all steps complete, should be Standby.
        // Let's count from start:
        let mut sm = simple_axis();
        sm.handle_event(Enable, false);
        // Steps: 0=CheckSafety, 1=EnableDrive, 2=WaitDriveReady, 3=ReleaseBrake,
        //         4=WaitBrakeReleased, 5=CheckPositionStable (skipped: not gravity),
        //         6=ResetControlState, 7=Complete → Standby
        for _ in 0..8 {
            if sm.state() == Standby {
                break;
            }
            sm.handle_event(StepComplete, false);
        }
        assert_eq!(sm.state(), Standby);
    }

    #[test]
    fn power_on_skips_brake_steps_when_no_brake() {
        let mut sm = no_brake_axis();
        sm.handle_event(Enable, false);

        // Count steps needed to reach Standby.
        let mut steps = 0;
        while sm.state() != Standby {
            sm.handle_event(StepComplete, false);
            steps += 1;
            assert!(steps < 20, "infinite loop detected");
        }
        // Without brake and without gravity, skips steps 3, 4, 5 → fewer steps.
        assert!(steps < 8, "should skip brake/gravity steps, took {steps}");
    }

    #[test]
    fn powering_on_timeout_goes_to_error() {
        let mut sm = simple_axis();
        sm.handle_event(Enable, false);
        assert_eq!(sm.handle_event(StepTimeout, false), PowerTransition::Ok(PowerError));
    }

    #[test]
    fn drive_fault_from_any_state() {
        for state in [PowerOff, PoweringOn, Standby, Motion, PoweringOff, NoBrake] {
            let mut sm = PowerStateMachine {
                state,
                ..simple_axis()
            };
            assert_eq!(
                sm.handle_event(DriveFault, false),
                PowerTransition::Ok(PowerError),
                "DriveFault from {state:?} should → PowerError"
            );
        }
    }

    #[test]
    fn standby_to_motion_and_back() {
        let mut sm = PowerStateMachine {
            state: Standby,
            ..simple_axis()
        };
        assert_eq!(sm.handle_event(MotionCommand, false), PowerTransition::Ok(Motion));
        assert_eq!(sm.handle_event(MotionComplete, false), PowerTransition::Ok(Standby));
    }

    #[test]
    fn disable_from_standby() {
        let mut sm = PowerStateMachine {
            state: Standby,
            ..simple_axis()
        };
        assert_eq!(sm.handle_event(Disable, false), PowerTransition::Ok(PoweringOff));
    }

    #[test]
    fn disable_from_motion() {
        let mut sm = PowerStateMachine {
            state: Motion,
            ..simple_axis()
        };
        assert_eq!(sm.handle_event(Disable, false), PowerTransition::Ok(PoweringOff));
    }

    #[test]
    fn power_off_sequence() {
        let mut sm = PowerStateMachine {
            state: PoweringOff,
            ..simple_axis()
        };
        let mut steps = 0;
        while sm.state() != PowerOff {
            sm.handle_event(StepComplete, false);
            steps += 1;
            assert!(steps < 20, "infinite loop");
        }
        assert_eq!(sm.state(), PowerOff);
    }

    #[test]
    fn error_reset_returns_to_power_off() {
        let mut sm = PowerStateMachine {
            state: PowerError,
            ..simple_axis()
        };
        assert_eq!(sm.handle_event(ErrorReset, false), PowerTransition::Ok(PowerOff));
    }

    #[test]
    fn no_brake_only_in_service() {
        let mut sm = simple_axis();
        assert!(matches!(
            sm.handle_event(NoBrakeEnter, false),
            PowerTransition::Rejected(_)
        ));
        assert_eq!(sm.handle_event(NoBrakeEnter, true), PowerTransition::Ok(NoBrake));
        assert_eq!(sm.handle_event(NoBrakeExit, true), PowerTransition::Ok(PowerOff));
    }

    #[test]
    fn allows_motion_output_only_in_motion() {
        for state in [PowerOff, PoweringOn, Standby, PoweringOff, NoBrake, PowerError] {
            let sm = PowerStateMachine { state, ..simple_axis() };
            assert!(!sm.allows_motion_output(), "{state:?} should NOT allow motion output");
        }
        let sm = PowerStateMachine {
            state: Motion,
            ..simple_axis()
        };
        assert!(sm.allows_motion_output());
    }

    #[test]
    fn sequence_tracker_tick_and_timeout() {
        let mut t = SequenceTracker::new();
        assert!(!t.timed_out(100));
        for _ in 0..100 {
            t.tick();
        }
        assert!(t.timed_out(100));
        assert!(!t.timed_out(101));
    }
}