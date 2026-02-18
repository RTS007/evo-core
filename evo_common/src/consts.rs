//! System-wide immutable constants for the EVO workspace.
//!
//! This module contains only non-configurable invariants.
//! Values that are TOML defaults belong in `crate::config`.

/// Maximum number of axes.
pub const MAX_AXES: u8 = 64;

/// Maximum number of digital inputs.
pub const MAX_DI: usize = 1024;

/// Maximum number of digital outputs.
pub const MAX_DO: usize = 1024;

/// Maximum number of analog inputs.
pub const MAX_AI: usize = 1024;

/// Maximum number of analog outputs.
pub const MAX_AO: usize = 1024;

// ─── Immutable validation bounds (FR-054) ──────────────────────────

/// Minimum Kp gain value.
pub const MIN_KP: f64 = 0.0;
/// Maximum Kp gain value.
pub const MAX_KP: f64 = 10_000.0;
/// Minimum Ki gain value.
pub const MIN_KI: f64 = 0.0;
/// Maximum Ki gain value.
pub const MAX_KI: f64 = 10_000.0;
/// Minimum Kd gain value.
pub const MIN_KD: f64 = 0.0;
/// Maximum Kd gain value.
pub const MAX_KD: f64 = 1_000.0;
/// Maximum velocity (mm/s or deg/s).
pub const MAX_VELOCITY: f64 = 100_000.0;
/// Maximum acceleration (mm/s² or deg/s²).
pub const MAX_ACCELERATION: f64 = 1_000_000.0;
/// Maximum position range (absolute value).
pub const MAX_POSITION_RANGE: f64 = 1_000_000.0;
/// Maximum out_max control output.
pub const MAX_OUT_MAX: f64 = 1_000.0;
/// Maximum lag error limit.
pub const MAX_LAG_ERROR: f64 = 100.0;
/// Maximum homing speed.
pub const MAX_HOMING_SPEED: f64 = 10_000.0;
/// Maximum homing timeout.
pub const MAX_HOMING_TIMEOUT: f64 = 300.0;
/// Maximum safe deceleration.
pub const MAX_SAFE_DECEL: f64 = 1_000_000.0;

/// Minimum cycle time in microseconds for runtime config validation.
pub const MIN_CYCLE_TIME_US: u32 = 100;
/// Maximum cycle time in microseconds for runtime config validation.
pub const MAX_CYCLE_TIME_US: u32 = 10_000;

/// Minimum manual mode timeout [s] (validation bound).
pub const MANUAL_TIMEOUT_MIN: f64 = 1.0;
/// Maximum manual mode timeout [s] (validation bound).
pub const MANUAL_TIMEOUT_MAX: f64 = 300.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_consistent() {
        assert!(MAX_AXES > 0);
        assert!((MAX_AXES as u16) <= 256);
        assert!(MAX_DI > 0);
        assert!(MAX_DO > 0);
        assert!(MAX_AI > 0);
        assert!(MAX_AO > 0);
        assert!(MIN_KP <= MAX_KP);
        assert!(MIN_KI <= MAX_KI);
        assert!(MIN_KD <= MAX_KD);
        assert!(MAX_VELOCITY > 0.0);
        assert!(MAX_ACCELERATION > 0.0);
        assert!(MAX_POSITION_RANGE > 0.0);
        assert!(MAX_OUT_MAX > 0.0);
        assert!(MAX_LAG_ERROR > 0.0);
        assert!(MAX_HOMING_SPEED > 0.0);
        assert!(MAX_HOMING_TIMEOUT > 0.0);
        assert!(MAX_SAFE_DECEL > 0.0);
        assert!(MIN_CYCLE_TIME_US > 0);
        assert!(MIN_CYCLE_TIME_US <= MAX_CYCLE_TIME_US);
        assert!(MANUAL_TIMEOUT_MIN > 0.0);
        assert!(MANUAL_TIMEOUT_MIN <= MANUAL_TIMEOUT_MAX);
    }

    #[test]
    fn di_bank_fits_in_u64_array() {
        // DI bit-packing uses [u64; 16] = 1024 bits.
        assert!(MAX_DI <= 64 * 16);
    }

    #[test]
    fn do_bank_fits_in_u64_array() {
        assert!(MAX_DO <= 64 * 16);
    }
}
