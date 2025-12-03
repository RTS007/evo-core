//! Central data structure definitions for the EVO shared memory system.
//!
//! This module provides centralized data structure definitions for all EVO modules,
//! ensuring a single source of truth for shared memory communication.

// Declare submodules
pub mod api;
pub mod control;
pub mod hal;
pub mod recipe;
pub mod system;

/// Shared memory segment names and size constants.
pub mod segments {
    use crate::SHM_MIN_SIZE;

    /// Segment for HAL sensor readings
    pub const HAL_SENSOR_DATA: &str = "evo_hal_sensors";
    /// Segment for HAL actuator states
    pub const HAL_ACTUATOR_STATE: &str = "evo_hal_actuators";
    /// Segment for HAL I/O bank status
    pub const HAL_IO_BANK_STATUS: &str = "evo_hal_io_banks";
    /// Segment for HAL hardware configuration
    pub const HAL_HARDWARE_CONFIG: &str = "evo_hal_config";

    /// Segment for control system state
    pub const CONTROL_STATE: &str = "evo_control_state";
    /// Segment for control system commands
    pub const CONTROL_COMMANDS: &str = "evo_control_commands";
    /// Segment for control system performance metrics
    pub const CONTROL_PERFORMANCE: &str = "evo_control_performance";

    /// Segment for recipe execution state
    pub const RECIPE_STATE: &str = "evo_recipe_state";
    /// Segment for recipe step definitions
    pub const RECIPE_STEPS: &str = "evo_recipe_steps";
    /// Segment for recipe execution commands
    pub const RECIPE_COMMANDS: &str = "evo_recipe_commands";

    /// Segment for API request metrics
    pub const API_REQUEST_METRICS: &str = "evo_api_metrics";
    /// Segment for aggregated system state
    pub const API_SYSTEM_STATE: &str = "evo_api_system_state";
    /// Segment for API client session data
    pub const API_CLIENT_SESSIONS: &str = "evo_api_sessions";

    /// Segment for system module status
    pub const SYSTEM_MODULE_STATUS: &str = "evo_system_modules";
    /// Segment for system health monitoring
    pub const SYSTEM_HEALTH: &str = "evo_system_health";

    /// Standard shared memory segment size (4KB)
    pub const STANDARD_SEGMENT_SIZE: usize = SHM_MIN_SIZE;
    /// Large shared memory segment size (8KB)
    pub const LARGE_SEGMENT_SIZE: usize = SHM_MIN_SIZE * 2;
    /// Huge shared memory segment size (16KB)
    pub const HUGE_SEGMENT_SIZE: usize = SHM_MIN_SIZE * 4;
}
