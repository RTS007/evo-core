//! HAL (Hardware Abstraction Layer) data structures

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Hardware sensor reading with timestamp and quality flags
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReading {
    /// Unique sensor identifier
    pub sensor_id: String,
    /// Raw sensor value
    pub raw_value: f64,
    /// Calibrated/processed value
    pub calibrated_value: f64,
    /// Measurement unit (e.g., "°C", "bar", "rpm")
    pub unit: String,
    /// Measurement timestamp in microseconds since UNIX epoch
    pub timestamp_us: u64,
    /// Quality flags (bit field)
    pub quality_flags: u32,
    /// Sensor status code
    pub status: SensorStatus,
    /// Measurement uncertainty/tolerance
    pub uncertainty: f32,
}

/// Sensor operational status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SensorStatus {
    /// Operating normally
    Normal,
    /// Warning condition detected
    Warning,
    /// Error condition detected
    Error,
    /// Sensor offline/disconnected
    Offline,
    /// Calibration required
    NeedsCalibration,
}

/// Actuator state and control information
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActuatorState {
    /// Unique actuator identifier
    pub actuator_id: String,
    /// Current position/value
    pub current_value: f64,
    /// Target/setpoint value
    pub target_value: f64,
    /// Control output percentage (0-100)
    pub output_percent: f32,
    /// Actuator operational status
    pub status: ActuatorStatus,
    /// Last update timestamp in microseconds
    pub timestamp_us: u64,
    /// Error code (0 = no error)
    pub error_code: u32,
    /// Operating mode
    pub mode: ActuatorMode,
}

/// Actuator operational status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActuatorStatus {
    /// Ready for operation
    Ready,
    /// Currently moving/acting
    Active,
    /// Holding position
    Holding,
    /// Error condition
    Error,
    /// Emergency stop engaged
    EmergencyStop,
    /// Manual override active
    ManualOverride,
}

/// Actuator operating mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActuatorMode {
    /// Automatic control
    Auto,
    /// Manual control
    Manual,
    /// Maintenance mode
    Maintenance,
    /// Safety mode
    Safety,
}

/// I/O bank status for digital inputs/outputs
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IOBankStatus {
    /// Bank identifier
    pub bank_id: String,
    /// Digital input states (bit field)
    pub digital_inputs: u32,
    /// Digital output states (bit field)
    pub digital_outputs: u32,
    /// Analog input values
    pub analog_inputs: Vec<f32>,
    /// Analog output values
    pub analog_outputs: Vec<f32>,
    /// Bank communication status
    pub comm_status: CommStatus,
    /// Last update timestamp
    pub timestamp_us: u64,
    /// Configuration version
    pub config_version: u32,
}

/// Communication status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommStatus {
    /// Communication OK
    Good,
    /// Intermittent communication
    Degraded,
    /// Communication lost
    Lost,
    /// Communication error
    Error,
}

/// Hardware configuration data
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareConfig {
    /// Configuration version
    pub version: u32,
    /// Sensor calibration data (sensor_id -> calibration coefficients)
    pub sensor_calibrations: HashMap<String, Vec<f64>>,
    /// Actuator limits (actuator_id -> (min, max))
    pub actuator_limits: HashMap<String, (f64, f64)>,
    /// I/O bank configurations
    pub io_configurations: HashMap<String, IOBankConfig>,
    /// Safety interlock configurations
    pub safety_interlocks: Vec<SafetyInterlock>,
    /// Update frequency in Hz
    pub update_frequency: f32,
    /// Last configuration update timestamp
    pub timestamp_us: u64,
}

/// I/O bank configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IOBankConfig {
    /// Number of digital inputs
    pub digital_input_count: u8,
    /// Number of digital outputs
    pub digital_output_count: u8,
    /// Number of analog inputs
    pub analog_input_count: u8,
    /// Number of analog outputs
    pub analog_output_count: u8,
    /// Scan rate in Hz
    pub scan_rate: f32,
}

/// Safety interlock configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyInterlock {
    /// Interlock name/identifier
    pub name: String,
    /// Input signal that triggers interlock
    pub trigger_signal: String,
    /// Actions to take when triggered
    pub actions: Vec<SafetyAction>,
    /// Reset conditions
    pub reset_conditions: Vec<String>,
    /// Priority level (higher = more critical)
    pub priority: u8,
}

/// Safety action to take during interlock
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SafetyAction {
    /// Stop specific actuator
    StopActuator(String),
    /// Set output to safe state
    /// Set output signal to a specific value
    SetOutput {
        /// Signal name to set
        signal: String,
        /// Value to set the signal to
        value: f64,
    },
    /// Trigger emergency stop
    EmergencyStop,
    /// Send alarm notification
    SendAlarm(String),
}

// Default implementations
impl Default for SensorReading {
    fn default() -> Self {
        Self {
            sensor_id: String::new(),
            raw_value: 0.0,
            calibrated_value: 0.0,
            unit: String::new(),
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            quality_flags: 0,
            status: SensorStatus::Normal,
            uncertainty: 0.0,
        }
    }
}

impl Default for ActuatorState {
    fn default() -> Self {
        Self {
            actuator_id: String::new(),
            current_value: 0.0,
            target_value: 0.0,
            output_percent: 0.0,
            status: ActuatorStatus::Ready,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            error_code: 0,
            mode: ActuatorMode::Auto,
        }
    }
}

impl Default for IOBankStatus {
    fn default() -> Self {
        Self {
            bank_id: String::new(),
            digital_inputs: 0,
            digital_outputs: 0,
            analog_inputs: Vec::new(),
            analog_outputs: Vec::new(),
            comm_status: CommStatus::Good,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            config_version: 1,
        }
    }
}

impl Default for HardwareConfig {
    fn default() -> Self {
        Self {
            version: 1,
            sensor_calibrations: HashMap::new(),
            actuator_limits: HashMap::new(),
            io_configurations: HashMap::new(),
            safety_interlocks: Vec::new(),
            update_frequency: 100.0,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
        }
    }
}
