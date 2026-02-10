//! LEVEL 1: MachineState transitions (T036).
//!
//! Global machine lifecycle: Stopped → Starting → Idle ↔ Manual/Active → Service → SystemError.
//!
//! Implements the transition table from contracts/state-machines.md §1 with
//! guards and invariants I-MS-1 through I-MS-4.

use evo_common::control_unit::state::MachineState;

/// Result of a MachineState transition attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionResult {
    /// Transition succeeded — new state.
    Ok(MachineState),
    /// Transition rejected — reason.
    Rejected(&'static str),
}

/// Machine-level event that can trigger a state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineEvent {
    /// Power-on event received.
    PowerOn,
    /// Config valid, SHM connected, HAL alive.
    InitComplete,
    /// Config invalid or SHM mismatch during starting.
    InitFailed,
    /// First manual command received.
    ManualCommand,
    /// Manual timeout or explicit stop.
    ManualStop,
    /// RE sends first command.
    RecipeStart,
    /// RE sends Nop + all axes in position.
    RecipeComplete,
    /// RE sends Nop + manual command pending.
    RecipeCompleteManualPending,
    /// Service authorization flag set.
    ServiceAuthorize,
    /// Service deauthorized.
    ServiceDeauthorize,
    /// Critical fault or unrecoverable error (→ SystemError).
    CriticalFault,
    /// SafetyState==Safe + all errors cleared + operator reset + authorization.
    ErrorRecovery,
    /// Unrecoverable fault + explicit full-reset.
    FullReset,
}

/// MachineState manager holding the current state (I-MS-1).
#[derive(Debug, Clone)]
pub struct MachineStateMachine {
    state: MachineState,
}

impl MachineStateMachine {
    /// Create a new machine state machine in Stopped state.
    pub const fn new() -> Self {
        Self {
            state: MachineState::Stopped,
        }
    }

    /// Current state.
    #[inline]
    pub const fn state(&self) -> MachineState {
        self.state
    }

    /// Attempt a transition given an event.
    ///
    /// Returns `TransitionResult::Ok(new_state)` on success,
    /// `TransitionResult::Rejected(reason)` if the transition is not valid.
    pub fn handle_event(&mut self, event: MachineEvent) -> TransitionResult {
        use MachineEvent::*;
        use MachineState::*;

        let next = match (self.state, event) {
            // Stopped → Starting
            (Stopped, PowerOn) => Starting,

            // Starting → Idle (success) or SystemError (failure)
            (Starting, InitComplete) => Idle,
            (Starting, InitFailed) => SystemError,

            // Idle → Manual
            (Idle, ManualCommand) => Manual,
            // Manual → Idle
            (Manual, ManualStop) => Idle,

            // Idle/Manual → Active
            (Idle, RecipeStart) => Active,
            (Manual, RecipeStart) => Active,

            // Active → Idle/Manual
            (Active, RecipeComplete) => Idle,
            (Active, RecipeCompleteManualPending) => Manual,

            // any → Service (when authorized)
            (_, ServiceAuthorize) if self.state != SystemError => Service,

            // Service → Idle
            (Service, ServiceDeauthorize) => Idle,

            // any → SystemError (critical fault)
            (_, CriticalFault) => {
                self.state = SystemError;
                return TransitionResult::Ok(SystemError);
            }

            // SystemError → Idle (recovery, I-MS-2)
            (SystemError, ErrorRecovery) => Idle,

            // SystemError → Stopped (unrecoverable, I-MS-2)
            (SystemError, FullReset) => Stopped,

            // All other combinations are invalid.
            _ => {
                return TransitionResult::Rejected(invalid_transition_reason(self.state, event));
            }
        };

        self.state = next;
        TransitionResult::Ok(next)
    }

    /// Force state to SystemError (e.g., from safety subsystem).
    #[inline]
    pub fn force_system_error(&mut self) {
        self.state = MachineState::SystemError;
    }

    /// Check if the machine is in a state that allows axis motion commands.
    #[inline]
    pub const fn allows_motion(&self) -> bool {
        matches!(
            self.state,
            MachineState::Manual | MachineState::Active | MachineState::Service
        )
    }

    /// Check if the machine is in service mode (I-PW-3: NoBrake only in Service).
    #[inline]
    pub const fn is_service(&self) -> bool {
        matches!(self.state, MachineState::Service)
    }
}

fn invalid_transition_reason(state: MachineState, event: MachineEvent) -> &'static str {
    use MachineEvent::*;
    use MachineState::*;
    match (state, event) {
        (SystemError, _) => "SystemError: only ErrorRecovery or FullReset allowed",
        (_, ServiceAuthorize) => "ServiceAuthorize not allowed from SystemError",
        (Stopped, _) => "Stopped: only PowerOn allowed",
        (Starting, _) => "Starting: only InitComplete or InitFailed allowed",
        (Idle, _) => "Idle: invalid event for current state",
        (Manual, _) => "Manual: invalid event for current state",
        (Active, _) => "Active: invalid event for current state",
        (Service, _) => "Service: invalid event for current state",
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use MachineEvent::*;
    use MachineState::*;

    #[test]
    fn initial_state_is_stopped() {
        let sm = MachineStateMachine::new();
        assert_eq!(sm.state(), Stopped);
    }

    #[test]
    fn normal_startup_sequence() {
        let mut sm = MachineStateMachine::new();
        assert_eq!(sm.handle_event(PowerOn), TransitionResult::Ok(Starting));
        assert_eq!(sm.handle_event(InitComplete), TransitionResult::Ok(Idle));
    }

    #[test]
    fn startup_failure_to_system_error() {
        let mut sm = MachineStateMachine::new();
        sm.handle_event(PowerOn);
        assert_eq!(sm.handle_event(InitFailed), TransitionResult::Ok(SystemError));
    }

    #[test]
    fn idle_to_manual_and_back() {
        let mut sm = MachineStateMachine::new();
        sm.handle_event(PowerOn);
        sm.handle_event(InitComplete);
        assert_eq!(sm.handle_event(ManualCommand), TransitionResult::Ok(Manual));
        assert_eq!(sm.handle_event(ManualStop), TransitionResult::Ok(Idle));
    }

    #[test]
    fn idle_to_active_and_back() {
        let mut sm = MachineStateMachine::new();
        sm.handle_event(PowerOn);
        sm.handle_event(InitComplete);
        assert_eq!(sm.handle_event(RecipeStart), TransitionResult::Ok(Active));
        assert_eq!(sm.handle_event(RecipeComplete), TransitionResult::Ok(Idle));
    }

    #[test]
    fn manual_to_active() {
        let mut sm = MachineStateMachine::new();
        sm.handle_event(PowerOn);
        sm.handle_event(InitComplete);
        sm.handle_event(ManualCommand);
        assert_eq!(sm.handle_event(RecipeStart), TransitionResult::Ok(Active));
    }

    #[test]
    fn active_to_manual_pending() {
        let mut sm = MachineStateMachine::new();
        sm.handle_event(PowerOn);
        sm.handle_event(InitComplete);
        sm.handle_event(RecipeStart);
        assert_eq!(
            sm.handle_event(RecipeCompleteManualPending),
            TransitionResult::Ok(Manual)
        );
    }

    #[test]
    fn critical_fault_from_any_state() {
        for initial in [Stopped, Starting, Idle, Manual, Active, Service] {
            let mut sm = MachineStateMachine { state: initial };
            assert_eq!(
                sm.handle_event(CriticalFault),
                TransitionResult::Ok(SystemError),
                "CriticalFault from {initial:?} should → SystemError"
            );
        }
    }

    #[test]
    fn system_error_recovery() {
        let mut sm = MachineStateMachine {
            state: SystemError,
        };
        assert_eq!(sm.handle_event(ErrorRecovery), TransitionResult::Ok(Idle));
    }

    #[test]
    fn system_error_full_reset() {
        let mut sm = MachineStateMachine {
            state: SystemError,
        };
        assert_eq!(sm.handle_event(FullReset), TransitionResult::Ok(Stopped));
    }

    #[test]
    fn system_error_rejects_other_events() {
        let mut sm = MachineStateMachine {
            state: SystemError,
        };
        assert!(matches!(
            sm.handle_event(PowerOn),
            TransitionResult::Rejected(_)
        ));
        assert!(matches!(
            sm.handle_event(ManualCommand),
            TransitionResult::Rejected(_)
        ));
        assert!(matches!(
            sm.handle_event(RecipeStart),
            TransitionResult::Rejected(_)
        ));
    }

    #[test]
    fn service_from_idle() {
        let mut sm = MachineStateMachine { state: Idle };
        assert_eq!(sm.handle_event(ServiceAuthorize), TransitionResult::Ok(Service));
        assert_eq!(sm.handle_event(ServiceDeauthorize), TransitionResult::Ok(Idle));
    }

    #[test]
    fn invalid_transitions_rejected() {
        let mut sm = MachineStateMachine { state: Stopped };
        assert!(matches!(
            sm.handle_event(InitComplete),
            TransitionResult::Rejected(_)
        ));

        sm.state = Idle;
        assert!(matches!(
            sm.handle_event(PowerOn),
            TransitionResult::Rejected(_)
        ));
    }

    #[test]
    fn allows_motion_checks() {
        assert!(!MachineStateMachine { state: Stopped }.allows_motion());
        assert!(!MachineStateMachine { state: Starting }.allows_motion());
        assert!(!MachineStateMachine { state: Idle }.allows_motion());
        assert!(MachineStateMachine { state: Manual }.allows_motion());
        assert!(MachineStateMachine { state: Active }.allows_motion());
        assert!(MachineStateMachine { state: Service }.allows_motion());
        assert!(!MachineStateMachine { state: SystemError }.allows_motion());
    }
}