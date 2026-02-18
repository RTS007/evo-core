//! System-wide constants for the EVO workspace.
//!
//! Single source of truth for all numeric limits and default paths.
//! Imported by all crates — no duplication permitted.

/// Maximum number of axes.
pub const MAX_AXES: usize = 64;

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

/// Default configuration directory path.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/evo/config";

/// Default state file name (HAL persistent state).
pub const DEFAULT_STATE_FILE: &str = "hal_state";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_consistent() {
        assert!(MAX_AXES > 0 && MAX_AXES <= 256);
        assert!(MAX_DI > 0);
        assert!(MAX_DO > 0);
        assert!(MAX_AI > 0);
        assert!(MAX_AO > 0);
        assert!(CYCLE_TIME_US > 0);
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
