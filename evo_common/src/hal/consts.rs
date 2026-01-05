//! HAL (Hardware Abstraction Layer) constants.
//!
//! This module contains constants for the hardware abstraction layer,
//! including maximum counts for axes and I/O points.

/// Canonical HAL service name (used for SHM naming and logging).
pub const HAL_SERVICE_NAME: &str = "hal";

/// Maximum number of axes
pub const MAX_AXES: usize = 64;

/// Maximum number of digital inputs
pub const MAX_DI: usize = 1024;

/// Maximum number of digital outputs
pub const MAX_DO: usize = 1024;

/// Maximum number of analog inputs
pub const MAX_AI: usize = 1024;

/// Maximum number of analog outputs
pub const MAX_AO: usize = 1024;

/// Default configuration file path
pub const DEFAULT_CONFIG_PATH: &str = "/etc/evo/machine.toml";

/// Default state file name
pub const DEFAULT_STATE_FILE: &str = "hal_state";
