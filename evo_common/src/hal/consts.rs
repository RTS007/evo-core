//! HAL-specific constants.
//!
//! System-wide constants (MAX_AXES, MAX_DI, etc.) have moved to
//! [`crate::consts`]. This module re-exports them for backward
//! compatibility and retains HAL-only constants.

/// Canonical HAL service name (used for SHM segment naming and logging).
pub const HAL_SERVICE_NAME: &str = "hal";

// Re-export system-wide constants for backward compatibility.
pub use crate::consts::{
    DEFAULT_CONFIG_PATH, DEFAULT_STATE_FILE, MAX_AI, MAX_AO, MAX_AXES, MAX_DI, MAX_DO,
};
