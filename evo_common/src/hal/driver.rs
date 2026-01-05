//! HAL driver trait and error types.
//!
//! This module defines:
//! - `HalDriver` trait - Interface for pluggable HAL drivers
//! - `HalError` enum - Error types for HAL operations
//! - `DriverFactory` type alias - Factory function type
//! - `DriverDiagnostics` struct - Optional driver diagnostics

use crate::hal::config::{AxisConfig, MachineConfig};
use crate::hal::types::{HalCommands, HalStatus};
use std::time::Duration;
use thiserror::Error;

/// Error types for HAL operations.
#[derive(Debug, Clone, Error)]
pub enum HalError {
    /// Driver initialization failed
    #[error("Initialization failed: {0}")]
    InitFailed(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Hardware communication error
    #[error("Hardware communication error: {0}")]
    CommunicationError(String),

    /// Driver not found
    #[error("Driver not found: {0}")]
    DriverNotFound(String),

    /// State persistence error
    #[error("State persistence error: {0}")]
    PersistenceError(String),

    /// SHM error
    #[error("Shared memory error: {0}")]
    ShmError(String),
}

/// Factory function type for creating driver instances.
pub type DriverFactory = fn() -> Box<dyn HalDriver>;

/// Optional driver diagnostics.
#[derive(Debug, Clone, Default)]
pub struct DriverDiagnostics {
    /// Number of cycles executed
    pub cycle_count: u64,
    /// Average cycle time in microseconds
    pub avg_cycle_time_us: f64,
    /// Maximum cycle time in microseconds
    pub max_cycle_time_us: f64,
    /// Number of timing violations
    pub timing_violations: u64,
    /// Driver-specific diagnostics (JSON string)
    pub custom: Option<String>,
}

/// Trait defining the interface for HAL drivers.
///
/// HAL Core manages drivers through this trait, enabling pluggable
/// hardware backends (simulation, EtherCAT, CANopen, etc.).
///
/// # Lifecycle
///
/// 1. `init()` - Called once before RT loop starts
/// 2. `cycle()` - Called every cycle_time_us from RT loop
/// 3. `shutdown()` - Called when HAL Core is stopping
///
/// # Timing Contracts
///
/// | Operation | Max Duration | RT Constraint |
/// |-----------|--------------|---------------|
/// | `init()` | 30 seconds | None (pre-RT) |
/// | `cycle()` | cycle_time_us | **HARD** |
/// | `shutdown()` | 1 second | None (post-RT) |
pub trait HalDriver: Send + Sync {
    /// Returns the driver's unique identifier (e.g., "simulation", "ethercat").
    fn name(&self) -> &'static str;

    /// Returns the driver's semantic version.
    fn version(&self) -> &'static str;

    /// Initialize the driver with machine configuration.
    ///
    /// Called once by HAL Core before entering the RT loop.
    /// Driver should:
    /// - Parse driver-specific config from `config.driver_config`
    /// - Load axis configurations from files
    /// - Initialize hardware connections (or simulation state)
    /// - Restore persisted state if applicable
    ///
    /// # Timing
    /// - No RT constraints (runs before RT loop)
    /// - May block for hardware initialization
    ///
    /// # Errors
    /// Return `HalError::InitFailed` if initialization cannot complete.
    fn init(&mut self, config: &MachineConfig) -> Result<(), HalError>;

    /// Execute one cycle of the driver.
    ///
    /// Called every `cycle_time_us` microseconds by HAL Core's RT loop.
    /// Driver should:
    /// - Read hardware inputs (or simulate)
    /// - Process commands from `HalCommands`
    /// - Update internal state
    /// - Return status in `HalStatus`
    ///
    /// # Timing
    /// - MUST complete within `cycle_time_us`
    /// - Should be deterministic (no allocations, no blocking I/O)
    ///
    /// # Arguments
    /// * `commands` - Commands from Control Unit (extracted from SHM by HAL Core)
    /// * `dt` - Actual elapsed time since last cycle (for physics/interpolation)
    ///
    /// # Returns
    /// `HalStatus` containing current state of all axes and I/O.
    fn cycle(&mut self, commands: &HalCommands, dt: Duration) -> HalStatus;

    /// Graceful shutdown of the driver.
    ///
    /// Called by HAL Core when shutting down.
    /// Driver should:
    /// - Persist state if applicable
    /// - Close hardware connections
    /// - Release resources
    ///
    /// # Timing
    /// - No strict RT constraints
    /// - Should complete within 1 second
    fn shutdown(&mut self) -> Result<(), HalError>;

    /// Set axis configurations after loading.
    ///
    /// Called by HAL Core after `init()` but before `run()`.
    /// Provides the loaded axis configurations to the driver.
    ///
    /// Default implementation does nothing (for drivers that don't need axis configs).
    fn set_axis_configs(&mut self, _configs: &[AxisConfig]) {
        // Default: no-op
    }

    /// Check if driver supports hot-swap (runtime replacement).
    /// Default: false
    fn supports_hot_swap(&self) -> bool {
        false
    }

    /// Get driver-specific diagnostics.
    /// Default: None
    fn diagnostics(&self) -> Option<DriverDiagnostics> {
        None
    }

    /// Handle driver-specific commands (extensibility point).
    /// Default: No-op, returns None
    fn handle_custom_command(&mut self, _cmd: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(dead_code)]
    struct TestDriver {
        initialized: bool,
    }

    impl HalDriver for TestDriver {
        fn name(&self) -> &'static str {
            "test"
        }

        fn version(&self) -> &'static str {
            "0.1.0"
        }

        fn init(&mut self, _config: &MachineConfig) -> Result<(), HalError> {
            self.initialized = true;
            Ok(())
        }

        fn cycle(&mut self, _commands: &HalCommands, _dt: Duration) -> HalStatus {
            HalStatus::default()
        }

        fn shutdown(&mut self) -> Result<(), HalError> {
            self.initialized = false;
            Ok(())
        }
    }

    #[test]
    fn test_hal_error_display() {
        let err = HalError::InitFailed("test error".to_string());
        assert!(err.to_string().contains("test error"));

        let err = HalError::DriverNotFound("simulation".to_string());
        assert!(err.to_string().contains("simulation"));
    }

    #[test]
    fn test_driver_diagnostics_default() {
        let diag = DriverDiagnostics::default();
        assert_eq!(diag.cycle_count, 0);
        assert_eq!(diag.avg_cycle_time_us, 0.0);
        assert_eq!(diag.timing_violations, 0);
        assert!(diag.custom.is_none());
    }
}
