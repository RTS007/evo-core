//! System-wide constants for the EVO workspace.
//!
//! Single source of truth for all numeric limits and default paths.
//! Imported by all crates — no duplication permitted.

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

/// Default system cycle time in microseconds (1 kHz = 1000 µs).
pub const CYCLE_TIME_US: u64 = 1000;

/// Minimum allowed cycle time [µs] for runtime config.
pub const CYCLE_TIME_US_MIN: u32 = 100;

/// Maximum allowed cycle time [µs] for runtime config.
pub const CYCLE_TIME_US_MAX: u32 = 10_000;

/// Default manual mode timeout [s].
pub const MANUAL_TIMEOUT_DEFAULT: f64 = 30.0;

/// Minimum manual mode timeout [s].
pub const MANUAL_TIMEOUT_MIN: f64 = 1.0;

/// Maximum manual mode timeout [s].
pub const MANUAL_TIMEOUT_MAX: f64 = 300.0;

/// Default RT HAL staleness threshold [cycles].
pub const HAL_STALE_THRESHOLD_DEFAULT: u32 = 3;

/// Default RE/RPC staleness threshold [cycles].
pub const NON_RT_STALE_THRESHOLD_DEFAULT: u32 = 1000;

/// Default diagnostic update interval [cycles].
pub const MQT_UPDATE_INTERVAL_DEFAULT: u32 = 10;

/// Default configuration directory path.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/evo/config";

/// Default state file name (HAL persistent state).
pub const DEFAULT_STATE_FILE: &str = "hal_state";

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
        assert!(CYCLE_TIME_US > 0);
        assert!(CYCLE_TIME_US as u32 >= CYCLE_TIME_US_MIN);
        assert!(CYCLE_TIME_US as u32 <= CYCLE_TIME_US_MAX);
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
