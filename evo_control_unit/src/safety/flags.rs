//! AxisSafetyState flag evaluation (T049, FR-080, FR-081).
//!
//! Aggregates all peripheral flags per axis per cycle.
//! Motion is blocked when ANY flag is `false` (FR-081).

use evo_common::control_unit::error::{MotionError, PowerError};
use evo_common::control_unit::safety::AxisSafetyState;
use evo_common::io::registry::IoRegistry;
use evo_common::io::role::IoRole;

use super::peripherals::PeripheralsEvaluation;

/// Input for per-axis safety flag evaluation.
#[derive(Debug, Clone, Copy)]
pub struct SafetyFlagInput {
    /// Current axis position [user units].
    pub position: f64,
    /// Software minimum position limit.
    pub min_pos: f64,
    /// Software maximum position limit.
    pub max_pos: f64,
    /// In-position window tolerance.
    pub in_position_window: f64,
    /// Whether axis has been referenced (homed).
    pub referenced: bool,
    /// Current gearbox state ok (valid gear engaged or no gearbox).
    pub gearbox_ok: bool,
}

/// Evaluate per-axis `AxisSafetyState` flags by combining peripheral
/// results with limit switch, soft limit, motion enable, and gearbox checks.
///
/// Returns the updated flags and any additional error flags to set.
pub fn evaluate_axis_safety(
    peripheral_eval: &PeripheralsEvaluation,
    input: &SafetyFlagInput,
    registry: &IoRegistry,
    di_bank: &[u64; 16],
    axis_id: u8,
    has_motion_enable: bool,
) -> (AxisSafetyState, PowerError, MotionError) {
    let mut power_errors = peripheral_eval.errors;
    let mut motion_errors = MotionError::empty();

    // ── Peripheral flags (T045-T048) ──
    let tailstock_ok = peripheral_eval.tailstock_ok;
    let lock_pin_ok = peripheral_eval.lock_pin_ok;
    let brake_ok = peripheral_eval.brake_ok;
    let guard_ok = peripheral_eval.guard_ok;

    // ── Limit switches (FR-080) ──
    let limit_min = registry
        .read_di(&IoRole::LimitMin(axis_id), di_bank)
        .unwrap_or(false);
    let limit_max = registry
        .read_di(&IoRole::LimitMax(axis_id), di_bank)
        .unwrap_or(false);
    let limit_switch_ok = !limit_min && !limit_max;
    if !limit_switch_ok {
        motion_errors |= MotionError::HARD_LIMIT;
    }

    // ── Software limits (FR-080) ──
    let soft_limit_ok = if input.referenced {
        let lower = input.min_pos - input.in_position_window;
        let upper = input.max_pos + input.in_position_window;
        input.position >= lower && input.position <= upper
    } else {
        // Not referenced — can't check software limits.
        true
    };
    if !soft_limit_ok {
        motion_errors |= MotionError::SOFT_LIMIT;
    }

    // ── Motion enable (FR-021, FR-080) ──
    let motion_enable_ok = if has_motion_enable {
        let enabled = registry
            .read_di(&IoRole::Enable(axis_id), di_bank)
            .unwrap_or(false);
        if !enabled {
            power_errors |= PowerError::MOTION_ENABLE_LOST;
        }
        enabled
    } else {
        // No motion enable input configured → always ok.
        true
    };

    // ── Gearbox (FR-080) ──
    let gearbox_ok = input.gearbox_ok;

    let flags = AxisSafetyState {
        tailstock_ok,
        lock_pin_ok,
        brake_ok,
        guard_ok,
        limit_switch_ok,
        soft_limit_ok,
        motion_enable_ok,
        gearbox_ok,
    };

    (flags, power_errors, motion_errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_common::io::config::IoConfig;

    fn empty_registry() -> IoRegistry {
        let cfg = IoConfig {
            groups: Default::default(),
        };
        IoRegistry::from_config(&cfg).unwrap()
    }

    fn default_input() -> SafetyFlagInput {
        SafetyFlagInput {
            position: 50.0,
            min_pos: 0.0,
            max_pos: 100.0,
            in_position_window: 0.1,
            referenced: true,
            gearbox_ok: true,
        }
    }

    fn all_ok_eval() -> PeripheralsEvaluation {
        PeripheralsEvaluation {
            tailstock_ok: true,
            lock_pin_ok: true,
            brake_ok: true,
            guard_ok: true,
            errors: PowerError::empty(),
        }
    }

    #[test]
    fn all_flags_ok_when_everything_safe() {
        let reg = empty_registry();
        let di = [0u64; 16];
        let eval = all_ok_eval();
        let input = default_input();
        let (flags, pe, me) = evaluate_axis_safety(&eval, &input, &reg, &di, 1, false);
        assert!(flags.all_ok());
        assert!(pe.is_empty());
        assert!(me.is_empty());
    }

    #[test]
    fn tailstock_not_ok_blocks_motion() {
        let reg = empty_registry();
        let di = [0u64; 16];
        let mut eval = all_ok_eval();
        eval.tailstock_ok = false;
        eval.errors = PowerError::DRIVE_TAIL_OPEN;
        let input = default_input();
        let (flags, pe, _) = evaluate_axis_safety(&eval, &input, &reg, &di, 1, false);
        assert!(!flags.all_ok());
        assert!(!flags.tailstock_ok);
        assert!(pe.contains(PowerError::DRIVE_TAIL_OPEN));
    }

    #[test]
    fn soft_limit_exceeded() {
        let reg = empty_registry();
        let di = [0u64; 16];
        let eval = all_ok_eval();
        let input = SafetyFlagInput {
            position: 150.0, // beyond max_pos
            ..default_input()
        };
        let (flags, _, me) = evaluate_axis_safety(&eval, &input, &reg, &di, 1, false);
        assert!(!flags.soft_limit_ok);
        assert!(me.contains(MotionError::SOFT_LIMIT));
    }

    #[test]
    fn soft_limit_not_checked_when_unreferenced() {
        let reg = empty_registry();
        let di = [0u64; 16];
        let eval = all_ok_eval();
        let input = SafetyFlagInput {
            position: 150.0,
            referenced: false,
            ..default_input()
        };
        let (flags, _, me) = evaluate_axis_safety(&eval, &input, &reg, &di, 1, false);
        assert!(flags.soft_limit_ok); // not checked
        assert!(!me.contains(MotionError::SOFT_LIMIT));
    }

    #[test]
    fn motion_enable_lost_without_binding() {
        let reg = empty_registry();
        let di = [0u64; 16];
        let eval = all_ok_eval();
        let input = default_input();
        // has_motion_enable=true but no registry binding → read_di returns false → lost.
        let (flags, pe, _) = evaluate_axis_safety(&eval, &input, &reg, &di, 1, true);
        assert!(!flags.motion_enable_ok);
        assert!(pe.contains(PowerError::MOTION_ENABLE_LOST));
    }

    #[test]
    fn gearbox_not_ok() {
        let reg = empty_registry();
        let di = [0u64; 16];
        let eval = all_ok_eval();
        let input = SafetyFlagInput {
            gearbox_ok: false,
            ..default_input()
        };
        let (flags, _, _) = evaluate_axis_safety(&eval, &input, &reg, &di, 1, false);
        assert!(!flags.gearbox_ok);
        assert!(!flags.all_ok());
    }
}
