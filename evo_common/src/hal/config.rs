//! HAL configuration types.
//!
//! This module contains configuration types for the hardware abstraction layer:
//! - `MachineConfig` - Main configuration loaded from machine.toml
//! - `AxisConfig` - Per-axis configuration
//! - `DigitalIOConfig` / `AnalogIOConfig` - I/O configuration
//! - Various enums for axis types, referencing modes, etc.

use crate::hal::consts::{MAX_AI, MAX_AO, MAX_AXES, MAX_DI, MAX_DO};
use crate::hal::driver::HalError;
use crate::io::config::AnalogCurve;
use crate::prelude::DEFAULT_CYCLE_TIME_US;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Default function for cycle_time_us
fn default_cycle_time_us() -> u32 {
    DEFAULT_CYCLE_TIME_US
}

/// Default function for in_position_window
fn default_in_position_window() -> f64 {
    0.01
}

/// Default function for referencing speed
fn default_ref_speed() -> f64 {
    5.0
}

/// Default true helper
fn default_true() -> bool {
    true
}

/// Main configuration loaded from `machine.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineConfig {
    /// System cycle time in microseconds.
    /// Defaults to DEFAULT_CYCLE_TIME_US (1000μs) if omitted.
    #[serde(default = "default_cycle_time_us")]
    pub cycle_time_us: u32,

    /// Path to state persistence file (relative to config dir).
    /// Used by all drivers to persist axis positions across restarts.
    #[serde(default)]
    pub state_file: Option<PathBuf>,

    /// List of HAL drivers to load (e.g., ["ethercat", "canopen"]).
    /// Note: "simulation" cannot be mixed with other drivers.
    #[serde(default)]
    pub drivers: Vec<String>,

    /// Per-driver configuration sections.
    /// Key = driver name, Value = driver-specific TOML table.
    #[serde(default)]
    pub driver_config: HashMap<String, toml::Value>,

    /// Paths to axis configuration files (relative to config dir).
    #[serde(default)]
    pub axes: Vec<PathBuf>,

    /// Digital input configuration.
    #[serde(default)]
    pub digital_inputs: Vec<DigitalIOConfig>,

    /// Digital output configuration.
    #[serde(default)]
    pub digital_outputs: Vec<DigitalIOConfig>,

    /// Analog input configuration.
    #[serde(default)]
    pub analog_inputs: Vec<AnalogIOConfig>,

    /// Analog output configuration.
    #[serde(default)]
    pub analog_outputs: Vec<AnalogIOConfig>,
}

impl MachineConfig {
    /// Validate the machine configuration.
    ///
    /// # Validation Rules
    /// 1. `cycle_time_us` > 0
    /// 2. `axes.len()` <= MAX_AXES
    /// 3. `digital_inputs.len()` <= MAX_DI
    /// 4. `digital_outputs.len()` <= MAX_DO
    /// 5. `analog_inputs.len()` <= MAX_AI
    /// 6. `analog_outputs.len()` <= MAX_AO
    /// 7. All I/O names unique within category
    pub fn validate(&self) -> Result<(), HalError> {
        // Check cycle time
        if self.cycle_time_us == 0 {
            return Err(HalError::ConfigError(
                "cycle_time_us must be greater than 0".to_string(),
            ));
        }

        // Check axis count
        if self.axes.len() > MAX_AXES {
            return Err(HalError::ConfigError(format!(
                "Too many axes: {} (max {})",
                self.axes.len(),
                MAX_AXES
            )));
        }

        // Check digital input count
        if self.digital_inputs.len() > MAX_DI {
            return Err(HalError::ConfigError(format!(
                "Too many digital inputs: {} (max {})",
                self.digital_inputs.len(),
                MAX_DI
            )));
        }

        // Check digital output count
        if self.digital_outputs.len() > MAX_DO {
            return Err(HalError::ConfigError(format!(
                "Too many digital outputs: {} (max {})",
                self.digital_outputs.len(),
                MAX_DO
            )));
        }

        // Check analog input count
        if self.analog_inputs.len() > MAX_AI {
            return Err(HalError::ConfigError(format!(
                "Too many analog inputs: {} (max {})",
                self.analog_inputs.len(),
                MAX_AI
            )));
        }

        // Check analog output count
        if self.analog_outputs.len() > MAX_AO {
            return Err(HalError::ConfigError(format!(
                "Too many analog outputs: {} (max {})",
                self.analog_outputs.len(),
                MAX_AO
            )));
        }

        // Check for duplicate digital input names
        let mut di_names = std::collections::HashSet::new();
        for di in &self.digital_inputs {
            if !di_names.insert(&di.name) {
                return Err(HalError::ConfigError(format!(
                    "Duplicate digital input name: {}",
                    di.name
                )));
            }
        }

        // Check for duplicate digital output names
        let mut do_names = std::collections::HashSet::new();
        for do_cfg in &self.digital_outputs {
            if !do_names.insert(&do_cfg.name) {
                return Err(HalError::ConfigError(format!(
                    "Duplicate digital output name: {}",
                    do_cfg.name
                )));
            }
        }

        // Check for duplicate analog input names
        let mut ai_names = std::collections::HashSet::new();
        for ai in &self.analog_inputs {
            if !ai_names.insert(&ai.name) {
                return Err(HalError::ConfigError(format!(
                    "Duplicate analog input name: {}",
                    ai.name
                )));
            }
        }

        // Check for duplicate analog output names
        let mut ao_names = std::collections::HashSet::new();
        for ao in &self.analog_outputs {
            if !ao_names.insert(&ao.name) {
                return Err(HalError::ConfigError(format!(
                    "Duplicate analog output name: {}",
                    ao.name
                )));
            }
        }

        Ok(())
    }
}

impl Default for MachineConfig {
    fn default() -> Self {
        Self {
            cycle_time_us: DEFAULT_CYCLE_TIME_US,
            state_file: None,
            drivers: Vec::new(),
            driver_config: HashMap::new(),
            axes: Vec::new(),
            digital_inputs: Vec::new(),
            digital_outputs: Vec::new(),
            analog_inputs: Vec::new(),
            analog_outputs: Vec::new(),
        }
    }
}

/// Per-axis configuration loaded from individual axis TOML files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisConfig {
    /// Axis name (unique identifier)
    pub name: String,

    /// Axis type
    pub axis_type: AxisType,

    /// Encoder resolution (increments per user unit) - required for types 1,2,3
    #[serde(default)]
    pub encoder_resolution: Option<f64>,

    /// Maximum velocity in user units per second - required for type 1
    #[serde(default)]
    pub max_velocity: Option<f64>,

    /// Maximum acceleration in user units per second² - required for type 1
    #[serde(default)]
    pub max_acceleration: Option<f64>,

    /// Lag error limit in user units - required for type 1
    #[serde(default)]
    pub lag_error_limit: Option<f64>,

    /// Master axis index (0-based) - required for type 2 (Slave)
    #[serde(default)]
    pub master_axis: Option<usize>,

    /// Coupling offset for Slave axes - captured at coupling time.
    /// slave_position = master_position + coupling_offset
    #[serde(default)]
    pub coupling_offset: Option<f64>,

    /// In-position window in user units (e.g., 0.1 mm).
    /// Axis is "in position" when |actual - target| <= in_position_window.
    /// Default: 0.01 user units
    #[serde(default = "default_in_position_window")]
    pub in_position_window: f64,

    /// Referencing configuration
    #[serde(default)]
    pub referencing: ReferencingConfig,

    /// Software limits
    #[serde(default)]
    pub soft_limit_positive: Option<f64>,
    #[serde(default)]
    pub soft_limit_negative: Option<f64>,
}

impl AxisConfig {
    /// Validate the axis configuration.
    ///
    /// # Validation Rules
    /// 1. `name` not empty
    /// 2. For `Positioning`: `encoder_resolution`, `max_velocity`, `max_acceleration`, `lag_error_limit` required and > 0
    /// 3. For `Slave`: `master_axis` required
    /// 4. For `Measurement`: `encoder_resolution` required and > 0
    /// 5. `soft_limit_negative` < `soft_limit_positive` (if both set)
    /// 6. `in_position_window` >= 0
    pub fn validate(&self, axis_index: usize, all_axes: &[AxisConfig]) -> Result<(), HalError> {
        // Check name
        if self.name.is_empty() {
            return Err(HalError::ConfigError(format!(
                "Axis {} has empty name",
                axis_index
            )));
        }

        // Validate based on axis type
        match self.axis_type {
            AxisType::Positioning => {
                if self.encoder_resolution.map_or(true, |v| v <= 0.0) {
                    return Err(HalError::ConfigError(format!(
                        "Axis '{}': encoder_resolution required and must be > 0 for Positioning type",
                        self.name
                    )));
                }
                if self.max_velocity.map_or(true, |v| v <= 0.0) {
                    return Err(HalError::ConfigError(format!(
                        "Axis '{}': max_velocity required and must be > 0 for Positioning type",
                        self.name
                    )));
                }
                if self.max_acceleration.map_or(true, |v| v <= 0.0) {
                    return Err(HalError::ConfigError(format!(
                        "Axis '{}': max_acceleration required and must be > 0 for Positioning type",
                        self.name
                    )));
                }
                if self.lag_error_limit.map_or(true, |v| v <= 0.0) {
                    return Err(HalError::ConfigError(format!(
                        "Axis '{}': lag_error_limit required and must be > 0 for Positioning type",
                        self.name
                    )));
                }
            }
            AxisType::Slave => {
                if self.master_axis.is_none() {
                    return Err(HalError::ConfigError(format!(
                        "Axis '{}': master_axis required for Slave type",
                        self.name
                    )));
                }
                let master_idx = self.master_axis.unwrap();
                if master_idx >= axis_index {
                    return Err(HalError::ConfigError(format!(
                        "Axis '{}': master_axis must reference an earlier axis (got {}, self is {})",
                        self.name, master_idx, axis_index
                    )));
                }
                if master_idx < all_axes.len() && all_axes[master_idx].axis_type == AxisType::Slave
                {
                    return Err(HalError::ConfigError(format!(
                        "Axis '{}': master axis '{}' cannot be a Slave type",
                        self.name, all_axes[master_idx].name
                    )));
                }
            }
            AxisType::Measurement => {
                if self.encoder_resolution.map_or(true, |v| v <= 0.0) {
                    return Err(HalError::ConfigError(format!(
                        "Axis '{}': encoder_resolution required and must be > 0 for Measurement type",
                        self.name
                    )));
                }
            }
            AxisType::Simple => {
                // No special requirements for Simple type
            }
        }

        // Check software limits
        if let (Some(neg), Some(pos)) = (self.soft_limit_negative, self.soft_limit_positive) {
            if neg >= pos {
                return Err(HalError::ConfigError(format!(
                    "Axis '{}': soft_limit_negative ({}) must be < soft_limit_positive ({})",
                    self.name, neg, pos
                )));
            }
        }

        // Check in_position_window
        if self.in_position_window < 0.0 {
            return Err(HalError::ConfigError(format!(
                "Axis '{}': in_position_window must be >= 0",
                self.name
            )));
        }

        Ok(())
    }
}

/// Axis type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum AxisType {
    /// On/off axis without position feedback
    #[default]
    Simple = 0,
    /// Full servo axis with encoder and kinematics
    Positioning = 1,
    /// Axis coupled to master axis
    Slave = 2,
    /// Encoder-only axis without drive
    Measurement = 3,
}

/// Referencing configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReferencingConfig {
    /// Whether referencing is required: "yes", "perhaps", "no"
    #[serde(default)]
    pub required: ReferencingRequired,

    /// Referencing mode (0-5)
    #[serde(default)]
    pub mode: ReferencingMode,

    /// Digital input index for reference switch
    #[serde(default)]
    pub reference_switch: Option<usize>,

    /// True if reference switch is normally closed
    #[serde(default)]
    pub normally_closed: bool,

    /// True if referencing moves in negative direction first
    #[serde(default = "default_true")]
    pub negative_direction: bool,

    /// Referencing speed in user units per second
    #[serde(default = "default_ref_speed")]
    pub speed: f64,

    /// Show error if K0 distance is too small
    #[serde(default)]
    pub show_k0_distance_error: bool,

    //=== Simulation-specific fields ===
    /// Position where virtual reference switch activates (simulation only).
    /// Default: 0.0 user units
    #[serde(default)]
    pub reference_switch_position: f64,

    /// Position where virtual K0 index pulse triggers (simulation only).
    /// Default: 0.0 user units
    #[serde(default)]
    pub index_pulse_position: f64,
}

/// Referencing requirement level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReferencingRequired {
    /// Always require referencing on startup
    Yes,
    /// Use persisted position if available, else require referencing
    Perhaps,
    /// Never require referencing
    #[default]
    No,
}

/// Referencing mode enumeration (6 modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum ReferencingMode {
    /// No referencing needed
    #[default]
    None = 0,
    /// Reference switch + K0 index pulse
    SwitchThenIndex = 1,
    /// Reference switch only
    SwitchOnly = 2,
    /// K0 index pulse only
    IndexOnly = 3,
    /// Limit switch + K0 index pulse
    LimitThenIndex = 4,
    /// Limit switch only
    LimitOnly = 5,
}

/// Digital I/O configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigitalIOConfig {
    /// I/O point name
    pub name: String,

    /// Optional description
    #[serde(default)]
    pub description: Option<String>,

    /// Initial value for simulation (inputs only).
    /// Default: false (off)
    #[serde(default)]
    pub initial_value: bool,

    /// Linked DI reactions for simulation (outputs only).
    /// Format: (trigger, delay_s, di_index, result)
    #[serde(default)]
    pub linked_inputs: Vec<LinkedDigitalInput>,
}

/// Linked digital input reaction.
/// When a DO changes to `trigger` state, after `delay_s` seconds,
/// set DI at `di_index` to `result` state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedDigitalInput {
    /// DO state that triggers this reaction (true = ON, false = OFF)
    pub trigger: bool,
    /// Delay in seconds before DI changes
    pub delay_s: f64,
    /// Index of DI to affect
    pub di_index: usize,
    /// State to set DI to
    pub result: bool,
}

/// Analog I/O configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalogIOConfig {
    /// I/O point name
    pub name: String,

    /// Minimum scaled value (engineering units)
    #[serde(default)]
    pub min_value: f64,

    /// Maximum scaled value (engineering units)
    #[serde(default = "default_max_value")]
    pub max_value: f64,

    /// Scaling curve configuration
    #[serde(default)]
    pub curve: AnalogCurve,

    /// Engineering unit name (e.g., "bar", "°C", "V")
    #[serde(default)]
    pub unit: Option<String>,

    /// Initial value for simulation (inputs only, in engineering units).
    /// Default: min_value
    #[serde(default)]
    pub initial_value: Option<f64>,
}

fn default_max_value() -> f64 {
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_machine_config_default() {
        let config = MachineConfig::default();
        assert_eq!(config.cycle_time_us, DEFAULT_CYCLE_TIME_US);
        assert!(config.axes.is_empty());
        assert!(config.digital_inputs.is_empty());
    }

    #[test]
    fn test_machine_config_validate_empty() {
        let config = MachineConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_machine_config_validate_cycle_time_zero() {
        let mut config = MachineConfig::default();
        config.cycle_time_us = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_machine_config_validate_duplicate_di_names() {
        let mut config = MachineConfig::default();
        config.digital_inputs.push(DigitalIOConfig {
            name: "di_test".to_string(),
            description: None,
            initial_value: false,
            linked_inputs: vec![],
        });
        config.digital_inputs.push(DigitalIOConfig {
            name: "di_test".to_string(),
            description: None,
            initial_value: false,
            linked_inputs: vec![],
        });
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate"));
    }

    #[test]
    fn test_axis_type_default() {
        assert_eq!(AxisType::default(), AxisType::Simple);
    }

    #[test]
    fn test_referencing_mode_default() {
        assert_eq!(ReferencingMode::default(), ReferencingMode::None);
    }

    #[test]
    fn test_referencing_required_default() {
        assert_eq!(ReferencingRequired::default(), ReferencingRequired::No);
    }

    #[test]
    fn test_analog_curve_linear() {
        let curve = AnalogCurve::LINEAR;
        assert_eq!(curve.eval(0.0), 0.0);
        assert_eq!(curve.eval(0.5), 0.5);
        assert_eq!(curve.eval(1.0), 1.0);
    }

    #[test]
    fn test_analog_curve_quadratic() {
        let curve = AnalogCurve::QUADRATIC;
        assert_eq!(curve.eval(0.0), 0.0);
        assert_eq!(curve.eval(0.5), 0.25);
        assert_eq!(curve.eval(1.0), 1.0);
    }

    #[test]
    fn test_analog_curve_to_scaled() {
        let curve = AnalogCurve::LINEAR;
        assert_eq!(curve.to_scaled(0.0, 0.0, 100.0), 0.0);
        assert_eq!(curve.to_scaled(0.5, 0.0, 100.0), 50.0);
        assert_eq!(curve.to_scaled(1.0, 0.0, 100.0), 100.0);
    }

    #[test]
    fn test_analog_curve_to_normalized() {
        let curve = AnalogCurve::LINEAR;
        assert!((curve.to_normalized(0.0, 0.0, 100.0) - 0.0).abs() < 0.001);
        assert!((curve.to_normalized(50.0, 0.0, 100.0) - 0.5).abs() < 0.001);
        assert!((curve.to_normalized(100.0, 0.0, 100.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_analog_curve_validate() {
        let curve = AnalogCurve::LINEAR;
        assert!(curve.validate().is_ok());

        let invalid = AnalogCurve::new(0.0, 0.0, 0.5, 0.0);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_axis_config_validate_positioning() {
        let axis = AxisConfig {
            name: "test".to_string(),
            axis_type: AxisType::Positioning,
            encoder_resolution: Some(1000.0),
            max_velocity: Some(100.0),
            max_acceleration: Some(500.0),
            lag_error_limit: Some(1.0),
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.01,
            referencing: ReferencingConfig::default(),
            soft_limit_positive: Some(1000.0),
            soft_limit_negative: Some(-1000.0),
        };
        assert!(axis.validate(0, &[]).is_ok());
    }

    #[test]
    fn test_axis_config_validate_positioning_missing_fields() {
        let axis = AxisConfig {
            name: "test".to_string(),
            axis_type: AxisType::Positioning,
            encoder_resolution: None,
            max_velocity: None,
            max_acceleration: None,
            lag_error_limit: None,
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.01,
            referencing: ReferencingConfig::default(),
            soft_limit_positive: None,
            soft_limit_negative: None,
        };
        assert!(axis.validate(0, &[]).is_err());
    }

    #[test]
    fn test_axis_config_validate_slave() {
        let master = AxisConfig {
            name: "master".to_string(),
            axis_type: AxisType::Positioning,
            encoder_resolution: Some(1000.0),
            max_velocity: Some(100.0),
            max_acceleration: Some(500.0),
            lag_error_limit: Some(1.0),
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.01,
            referencing: ReferencingConfig::default(),
            soft_limit_positive: None,
            soft_limit_negative: None,
        };

        let slave = AxisConfig {
            name: "slave".to_string(),
            axis_type: AxisType::Slave,
            encoder_resolution: None,
            max_velocity: None,
            max_acceleration: None,
            lag_error_limit: None,
            master_axis: Some(0),
            coupling_offset: None,
            in_position_window: 0.01,
            referencing: ReferencingConfig::default(),
            soft_limit_positive: None,
            soft_limit_negative: None,
        };

        assert!(slave.validate(1, &[master]).is_ok());
    }

    #[test]
    fn test_axis_config_validate_soft_limits() {
        let axis = AxisConfig {
            name: "test".to_string(),
            axis_type: AxisType::Simple,
            encoder_resolution: None,
            max_velocity: None,
            max_acceleration: None,
            lag_error_limit: None,
            master_axis: None,
            coupling_offset: None,
            in_position_window: 0.01,
            referencing: ReferencingConfig::default(),
            soft_limit_positive: Some(100.0),
            soft_limit_negative: Some(200.0), // Invalid: neg > pos
        };
        assert!(axis.validate(0, &[]).is_err());
    }
}
