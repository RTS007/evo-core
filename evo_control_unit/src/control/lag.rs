//! Lag error monitoring.
//!
//! Computes |target_pos - actual_pos|, compares to lag_error_limit,
//! dispatches per lag_policy (Critical/Unwanted/Neutral/Desired).
//!
//! Operates independently of control algorithm selection (FR-103).

use evo_common::control_unit::error::MotionError;
use evo_common::control_unit::state::LagPolicy;

/// Result of lag error evaluation for a single axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LagResult {
    /// Absolute lag error [mm].
    pub lag_error: f64,
    /// Whether the lag error exceeds the limit.
    pub exceeded: bool,
    /// Motion error flags to set (may be empty).
    pub motion_error: MotionError,
    /// Whether a global SAFETY_STOP is required (Critical policy).
    pub trigger_safety_stop: bool,
    /// Whether the axis should be stopped (Unwanted policy).
    pub trigger_axis_stop: bool,
}

/// Evaluate lag error for a single axis.
///
/// # Arguments
/// - `target_position`: Commanded position [mm].
/// - `actual_position`: Encoder-reported position [mm].
/// - `lag_error_limit`: Maximum allowed lag [mm]. Zero or negative disables.
/// - `lag_policy`: How to react when limit is exceeded.
///
/// # Returns
/// [`LagResult`] with the appropriate error flags and stop triggers.
pub fn evaluate_lag(
    target_position: f64,
    actual_position: f64,
    lag_error_limit: f64,
    lag_policy: LagPolicy,
) -> LagResult {
    let lag_error = (target_position - actual_position).abs();

    // Desired policy suppresses lag monitoring entirely
    if matches!(lag_policy, LagPolicy::Desired) {
        return LagResult {
            lag_error,
            exceeded: false,
            motion_error: MotionError::empty(),
            trigger_safety_stop: false,
            trigger_axis_stop: false,
        };
    }

    // Check if limit is exceeded (disabled if limit <= 0)
    let exceeded = lag_error_limit > 0.0 && lag_error > lag_error_limit;

    if !exceeded {
        return LagResult {
            lag_error,
            exceeded: false,
            motion_error: MotionError::empty(),
            trigger_safety_stop: false,
            trigger_axis_stop: false,
        };
    }

    // Limit exceeded — dispatch by policy
    match lag_policy {
        LagPolicy::Critical => LagResult {
            lag_error,
            exceeded: true,
            motion_error: MotionError::LAG_EXCEED,
            trigger_safety_stop: true,
            trigger_axis_stop: true,
        },
        LagPolicy::Unwanted => LagResult {
            lag_error,
            exceeded: true,
            motion_error: MotionError::LAG_EXCEED,
            trigger_safety_stop: false,
            trigger_axis_stop: true,
        },
        LagPolicy::Neutral => LagResult {
            lag_error,
            exceeded: true,
            motion_error: MotionError::LAG_EXCEED,
            trigger_safety_stop: false,
            trigger_axis_stop: false,
        },
        LagPolicy::Desired => unreachable!(), // handled above
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_limit_no_error() {
        let r = evaluate_lag(100.0, 99.5, 1.0, LagPolicy::Unwanted);
        assert!(!r.exceeded);
        assert!(r.motion_error.is_empty());
        assert!(!r.trigger_safety_stop);
        assert!(!r.trigger_axis_stop);
        assert!((r.lag_error - 0.5).abs() < 1e-12);
    }

    #[test]
    fn critical_triggers_safety_stop() {
        let r = evaluate_lag(100.0, 97.0, 1.0, LagPolicy::Critical);
        assert!(r.exceeded);
        assert!(r.motion_error.contains(MotionError::LAG_EXCEED));
        assert!(r.trigger_safety_stop);
        assert!(r.trigger_axis_stop);
    }

    #[test]
    fn unwanted_triggers_axis_stop() {
        let r = evaluate_lag(100.0, 97.0, 1.0, LagPolicy::Unwanted);
        assert!(r.exceeded);
        assert!(r.motion_error.contains(MotionError::LAG_EXCEED));
        assert!(!r.trigger_safety_stop);
        assert!(r.trigger_axis_stop);
    }

    #[test]
    fn neutral_flags_only() {
        let r = evaluate_lag(100.0, 97.0, 1.0, LagPolicy::Neutral);
        assert!(r.exceeded);
        assert!(r.motion_error.contains(MotionError::LAG_EXCEED));
        assert!(!r.trigger_safety_stop);
        assert!(!r.trigger_axis_stop);
    }

    #[test]
    fn desired_suppresses_entirely() {
        let r = evaluate_lag(100.0, 50.0, 1.0, LagPolicy::Desired);
        assert!(!r.exceeded);
        assert!(r.motion_error.is_empty());
        assert!(!r.trigger_safety_stop);
        assert!(!r.trigger_axis_stop);
    }

    #[test]
    fn zero_limit_disables_monitoring() {
        let r = evaluate_lag(100.0, 0.0, 0.0, LagPolicy::Critical);
        assert!(!r.exceeded);
        assert!(r.motion_error.is_empty());
    }

    #[test]
    fn negative_limit_disables_monitoring() {
        let r = evaluate_lag(100.0, 0.0, -5.0, LagPolicy::Critical);
        assert!(!r.exceeded);
    }

    #[test]
    fn exact_limit_not_exceeded() {
        let r = evaluate_lag(100.0, 99.0, 1.0, LagPolicy::Unwanted);
        // |100 - 99| = 1.0, which is NOT > 1.0
        assert!(!r.exceeded);
    }
}
