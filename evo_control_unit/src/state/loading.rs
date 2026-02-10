//! LoadingState per-axis management (T072, T073).
//!
//! Config-driven loading mode: LoadingBlocked or LoadingManualAllowed
//! based on per-axis configuration flags.
//!
//! Loading behavior is determined by config (I-LD-1) and triggered at runtime
//! by a global loading trigger (I-LD-2).

use evo_common::control_unit::state::LoadingState;

/// Per-axis loading configuration (from CuAxisConfig).
#[derive(Debug, Clone, Copy)]
pub struct AxisLoadingConfig {
    /// If true, axis is blocked during loading (no motion).
    pub loading_blocked: bool,
    /// If true, manual motion is allowed during loading (reduced speed).
    pub loading_manual: bool,
}

impl Default for AxisLoadingConfig {
    fn default() -> Self {
        Self {
            loading_blocked: false,
            loading_manual: false,
        }
    }
}

/// Per-axis loading state machine.
#[derive(Debug, Clone)]
pub struct LoadingStateMachine {
    state: LoadingState,
    config: AxisLoadingConfig,
}

impl LoadingStateMachine {
    /// Create a new loading state machine in Production mode.
    pub fn new(config: AxisLoadingConfig) -> Self {
        Self {
            state: LoadingState::Production,
            config,
        }
    }

    /// Current loading state.
    #[inline]
    pub fn state(&self) -> LoadingState {
        self.state
    }

    /// Trigger loading mode on this axis.
    ///
    /// Transitions from Production → ReadyForLoading → LoadingBlocked or LoadingManualAllowed
    /// based on per-axis config (I-LD-1).
    pub fn trigger_loading(&mut self) {
        if self.state != LoadingState::Production {
            return; // Already in loading
        }

        if self.config.loading_blocked {
            self.state = LoadingState::LoadingBlocked;
        } else if self.config.loading_manual {
            self.state = LoadingState::LoadingManualAllowed;
        } else {
            self.state = LoadingState::ReadyForLoading;
        }
    }

    /// Exit loading mode — return to Production.
    pub fn end_loading(&mut self) {
        self.state = LoadingState::Production;
    }

    /// Returns true if motion commands should be rejected on this axis (T073).
    #[inline]
    pub fn is_motion_blocked(&self) -> bool {
        matches!(self.state, LoadingState::LoadingBlocked)
    }

    /// Returns true if only manual (reduced speed) motion is allowed (T073).
    #[inline]
    pub fn is_manual_only(&self) -> bool {
        matches!(self.state, LoadingState::LoadingManualAllowed)
    }

    /// Check if motion commands should be rejected on this axis (T073).
    ///
    /// Returns `true` if motion is blocked by loading state.
    #[inline]
    pub fn check_motion_allowed(&self) -> bool {
        !self.is_motion_blocked()
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_production() {
        let sm = LoadingStateMachine::new(AxisLoadingConfig::default());
        assert_eq!(sm.state(), LoadingState::Production);
        assert!(!sm.is_motion_blocked());
        assert!(!sm.is_manual_only());
    }

    #[test]
    fn trigger_loading_blocked() {
        let mut sm = LoadingStateMachine::new(AxisLoadingConfig {
            loading_blocked: true,
            loading_manual: false,
        });
        sm.trigger_loading();
        assert_eq!(sm.state(), LoadingState::LoadingBlocked);
        assert!(sm.is_motion_blocked());
        assert!(!sm.is_manual_only());
        assert!(!sm.check_motion_allowed());
    }

    #[test]
    fn trigger_loading_manual_allowed() {
        let mut sm = LoadingStateMachine::new(AxisLoadingConfig {
            loading_blocked: false,
            loading_manual: true,
        });
        sm.trigger_loading();
        assert_eq!(sm.state(), LoadingState::LoadingManualAllowed);
        assert!(!sm.is_motion_blocked());
        assert!(sm.is_manual_only());
        assert!(sm.check_motion_allowed());
    }

    #[test]
    fn trigger_loading_ready() {
        let mut sm = LoadingStateMachine::new(AxisLoadingConfig {
            loading_blocked: false,
            loading_manual: false,
        });
        sm.trigger_loading();
        assert_eq!(sm.state(), LoadingState::ReadyForLoading);
    }

    #[test]
    fn end_loading_returns_to_production() {
        let mut sm = LoadingStateMachine::new(AxisLoadingConfig {
            loading_blocked: true,
            loading_manual: false,
        });
        sm.trigger_loading();
        assert_eq!(sm.state(), LoadingState::LoadingBlocked);
        sm.end_loading();
        assert_eq!(sm.state(), LoadingState::Production);
    }

    #[test]
    fn double_trigger_is_noop() {
        let mut sm = LoadingStateMachine::new(AxisLoadingConfig {
            loading_blocked: true,
            loading_manual: false,
        });
        sm.trigger_loading();
        let state_after = sm.state();
        sm.trigger_loading(); // second trigger
        assert_eq!(sm.state(), state_after);
    }
}
