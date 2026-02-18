//! HAL command and status types.
//!
//! This module defines the data structures for HAL communication:
//! - `HalCommands` - Commands from Control Unit to HAL
//! - `HalStatus` - Status from HAL to Control Unit
//! - `AxisCommand` / `AxisStatus` - Per-axis data
//! - `AnalogValue` - Dual representation for analog I/O

use crate::hal::consts::{MAX_AI, MAX_AO, MAX_AXES, MAX_DI, MAX_DO};

/// Commands read from SHM, passed to driver.
#[derive(Debug, Clone)]
pub struct HalCommands {
    /// Per-axis commands
    pub axes: [AxisCommand; MAX_AXES as usize],
    /// Digital output states (from Control Unit)
    pub digital_outputs: [bool; MAX_DO],
    /// Analog output values (normalized 0.0-1.0)
    pub analog_outputs: [f64; MAX_AO],
}

impl Default for HalCommands {
    fn default() -> Self {
        Self {
            axes: [AxisCommand::default(); MAX_AXES as usize],
            digital_outputs: [false; MAX_DO],
            analog_outputs: [0.0; MAX_AO],
        }
    }
}

/// Per-axis command structure.
#[derive(Debug, Clone, Copy, Default)]
pub struct AxisCommand {
    /// Target position in user units
    pub target_position: f64,
    /// Enable axis
    pub enable: bool,
    /// Reset error
    pub reset: bool,
    /// Start referencing
    pub reference: bool,
}

/// Status returned by driver, written to SHM.
#[derive(Debug, Clone)]
pub struct HalStatus {
    /// Per-axis status
    pub axes: [AxisStatus; MAX_AXES as usize],
    /// Digital input states (from hardware/simulation)
    pub digital_inputs: [bool; MAX_DI],
    /// Analog input values (normalized 0.0-1.0, scaled)
    pub analog_inputs: [AnalogValue; MAX_AI],
}

impl Default for HalStatus {
    fn default() -> Self {
        Self {
            axes: [AxisStatus::default(); MAX_AXES as usize],
            digital_inputs: [false; MAX_DI],
            analog_inputs: [AnalogValue::default(); MAX_AI],
        }
    }
}

/// Per-axis status structure.
#[derive(Debug, Clone, Copy, Default)]
pub struct AxisStatus {
    /// Actual position in user units
    pub actual_position: f64,
    /// Actual velocity in user units/sec
    pub actual_velocity: f64,
    /// Current lag error
    pub lag_error: f64,
    /// Axis ready for motion
    pub ready: bool,
    /// Axis in error state
    pub error: bool,
    /// Axis is referenced
    pub referenced: bool,
    /// Referencing in progress
    pub referencing: bool,
    /// Axis is moving
    pub moving: bool,
    /// At target position (within in_position_window)
    pub in_position: bool,
    /// Error code (0 = no error)
    pub error_code: u16,
}

/// Analog value with dual representation.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnalogValue {
    /// Normalized value (0.0 - 1.0)
    pub normalized: f64,
    /// Scaled value in engineering units
    pub scaled: f64,
}

impl AnalogValue {
    /// Create a new analog value from normalized input.
    pub fn from_normalized(normalized: f64) -> Self {
        Self {
            normalized,
            scaled: normalized,
        }
    }

    /// Create a new analog value with both representations.
    pub fn new(normalized: f64, scaled: f64) -> Self {
        Self { normalized, scaled }
    }
}

// Error codes for axis status
/// No error
pub const ERROR_NONE: u16 = 0x0000;
/// Lag error limit exceeded
pub const ERROR_LAG: u16 = 0x0001;
/// Positive software limit reached
pub const ERROR_SOFT_LIMIT_POS: u16 = 0x0002;
/// Negative software limit reached
pub const ERROR_SOFT_LIMIT_NEG: u16 = 0x0003;
/// Reference switch not found during referencing
pub const ERROR_REF_SWITCH_NOT_FOUND: u16 = 0x0010;
/// Index pulse not found during referencing
pub const ERROR_REF_INDEX_NOT_FOUND: u16 = 0x0011;
/// Referencing timeout
pub const ERROR_REF_TIMEOUT: u16 = 0x0012;
/// Internal simulation error
pub const ERROR_INTERNAL: u16 = 0x00FF;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axis_command_default() {
        let cmd = AxisCommand::default();
        assert_eq!(cmd.target_position, 0.0);
        assert!(!cmd.enable);
        assert!(!cmd.reset);
        assert!(!cmd.reference);
    }

    #[test]
    fn test_axis_status_default() {
        let status = AxisStatus::default();
        assert_eq!(status.actual_position, 0.0);
        assert_eq!(status.actual_velocity, 0.0);
        assert_eq!(status.lag_error, 0.0);
        assert!(!status.ready);
        assert!(!status.error);
        assert!(!status.referenced);
        assert!(!status.referencing);
        assert!(!status.moving);
        assert!(!status.in_position);
        assert_eq!(status.error_code, ERROR_NONE);
    }

    #[test]
    fn test_analog_value() {
        let v = AnalogValue::from_normalized(0.5);
        assert_eq!(v.normalized, 0.5);
        assert_eq!(v.scaled, 0.5);

        let v2 = AnalogValue::new(0.5, 50.0);
        assert_eq!(v2.normalized, 0.5);
        assert_eq!(v2.scaled, 50.0);
    }

    #[test]
    fn test_hal_commands_default() {
        let cmds = HalCommands::default();
        assert_eq!(cmds.axes.len(), MAX_AXES as usize);
        assert_eq!(cmds.digital_outputs.len(), MAX_DO);
        assert_eq!(cmds.analog_outputs.len(), MAX_AO);
    }

    #[test]
    fn test_hal_status_default() {
        let status = HalStatus::default();
        assert_eq!(status.axes.len(), MAX_AXES as usize);
        assert_eq!(status.digital_inputs.len(), MAX_DI);
        assert_eq!(status.analog_inputs.len(), MAX_AI);
    }
}
