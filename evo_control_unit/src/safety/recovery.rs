//! Reset and authorization recovery sequence (T052, FR-122).
//!
//! After a SAFETY_STOP, recovery requires:
//! 1. Reset button press (di).
//! 2. All AxisSafetyState flags true for all axes.
//! 3. Manual authorization (if configured).
//!
//! Then: `SafetyState → Safe`, clear error flags, allow restart.

use evo_common::control_unit::safety::AxisSafetyState;
use evo_common::io::registry::IoRegistry;
use evo_common::io::role::IoRole;

/// Current step in the recovery sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStep {
    /// Not in recovery.
    Idle,
    /// Waiting for reset button press.
    WaitingReset,
    /// Waiting for all safety flags to clear.
    WaitingFlagsClear,
    /// Waiting for operator authorization.
    WaitingAuthorization,
    /// Recovery complete — safe to resume.
    Complete,
}

/// Recovery sequence manager.
#[derive(Debug)]
pub struct RecoveryManager {
    step: RecoveryStep,
    /// Whether manual authorization is required (FR-122).
    authorization_required: bool,
    /// Whether operator has provided authorization (set externally).
    authorized: bool,
}

impl RecoveryManager {
    /// Create a new recovery manager.
    pub const fn new(authorization_required: bool) -> Self {
        Self {
            step: RecoveryStep::Idle,
            authorization_required,
            authorized: false,
        }
    }

    /// Current recovery step.
    #[inline]
    pub const fn step(&self) -> RecoveryStep {
        self.step
    }

    /// Whether recovery has completed.
    #[inline]
    pub const fn is_complete(&self) -> bool {
        matches!(self.step, RecoveryStep::Complete)
    }

    /// Begin the recovery sequence (called when SAFETY_STOP is active
    /// and system wants to recover).
    pub fn begin(&mut self) {
        if self.step == RecoveryStep::Idle {
            self.step = RecoveryStep::WaitingReset;
            self.authorized = false;
        }
    }

    /// Provide operator authorization (e.g., from RPC command).
    pub fn authorize(&mut self) {
        self.authorized = true;
    }

    /// Tick the recovery sequence.
    ///
    /// `reset_pressed`: whether the reset DI is active this cycle.
    /// `all_axes_safe`: whether ALL axes have `AxisSafetyState::all_ok()`.
    pub fn tick(
        &mut self,
        reset_pressed: bool,
        all_axes_safe: bool,
    ) -> RecoveryStep {
        match self.step {
            RecoveryStep::Idle => {}
            RecoveryStep::WaitingReset => {
                if reset_pressed {
                    self.step = RecoveryStep::WaitingFlagsClear;
                }
            }
            RecoveryStep::WaitingFlagsClear => {
                if all_axes_safe {
                    if self.authorization_required {
                        self.step = RecoveryStep::WaitingAuthorization;
                    } else {
                        self.step = RecoveryStep::Complete;
                    }
                }
            }
            RecoveryStep::WaitingAuthorization => {
                if self.authorized {
                    self.step = RecoveryStep::Complete;
                }
            }
            RecoveryStep::Complete => {}
        }
        self.step
    }

    /// Check if reset button is pressed via IoRegistry.
    pub fn read_reset_button(
        registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> bool {
        // Global reset role: IoRole::EStopReset
        registry
            .read_di(&IoRole::EStopReset, di_bank)
            .unwrap_or(false)
    }

    /// Check if all axes have all safety flags ok.
    pub fn all_axes_safe(axis_safety_states: &[AxisSafetyState]) -> bool {
        axis_safety_states.iter().all(|s| s.all_ok())
    }

    /// Reset the recovery manager back to Idle.
    pub fn reset(&mut self) {
        self.step = RecoveryStep::Idle;
        self.authorized = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle() {
        let rm = RecoveryManager::new(true);
        assert_eq!(rm.step(), RecoveryStep::Idle);
        assert!(!rm.is_complete());
    }

    #[test]
    fn full_recovery_with_authorization() {
        let mut rm = RecoveryManager::new(true);
        rm.begin();
        assert_eq!(rm.step(), RecoveryStep::WaitingReset);

        // No reset yet.
        rm.tick(false, false);
        assert_eq!(rm.step(), RecoveryStep::WaitingReset);

        // Reset pressed.
        rm.tick(true, false);
        assert_eq!(rm.step(), RecoveryStep::WaitingFlagsClear);

        // Flags not clear yet.
        rm.tick(false, false);
        assert_eq!(rm.step(), RecoveryStep::WaitingFlagsClear);

        // All flags clear.
        rm.tick(false, true);
        assert_eq!(rm.step(), RecoveryStep::WaitingAuthorization);

        // Not authorized yet.
        rm.tick(false, true);
        assert_eq!(rm.step(), RecoveryStep::WaitingAuthorization);

        // Authorize.
        rm.authorize();
        rm.tick(false, true);
        assert_eq!(rm.step(), RecoveryStep::Complete);
        assert!(rm.is_complete());
    }

    #[test]
    fn recovery_without_authorization() {
        let mut rm = RecoveryManager::new(false);
        rm.begin();
        rm.tick(true, false);
        assert_eq!(rm.step(), RecoveryStep::WaitingFlagsClear);
        rm.tick(false, true);
        // No authorization required → goes directly to Complete.
        assert_eq!(rm.step(), RecoveryStep::Complete);
    }

    #[test]
    fn all_axes_safe_check() {
        let all_ok = vec![AxisSafetyState::default(), AxisSafetyState::default()];
        assert!(RecoveryManager::all_axes_safe(&all_ok));

        let one_bad = vec![
            AxisSafetyState::default(),
            AxisSafetyState {
                brake_ok: false,
                ..Default::default()
            },
        ];
        assert!(!RecoveryManager::all_axes_safe(&one_bad));
    }

    #[test]
    fn reset_clears_state() {
        let mut rm = RecoveryManager::new(true);
        rm.begin();
        rm.tick(true, true);
        assert_ne!(rm.step(), RecoveryStep::Idle);
        rm.reset();
        assert_eq!(rm.step(), RecoveryStep::Idle);
    }
}
