//! Control system data structures

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Control system state data shared across EVO components
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlState {
    /// Current system position (primary controlled variable)
    pub position: f64,
    /// Current velocity (derivative of position)
    pub velocity: f64,
    /// Current acceleration
    pub acceleration: f64,
    /// Target/setpoint position
    pub target_position: f64,
    /// Position error (target - current)
    pub position_error: f64,
    /// Control output signal
    pub control_output: f64,
    /// Current control mode
    pub control_mode: ControlMode,
    /// System operational status
    pub system_status: SystemStatus,
    /// Emergency stop state
    pub emergency_stop: bool,
    /// Safety interlock status
    pub safety_interlocks: u32,
    /// Control loop frequency in Hz
    pub loop_frequency: f64,
    /// Last update timestamp in microseconds
    pub timestamp_us: u64,
    /// Cycle counter for diagnostics
    pub cycle_count: u64,
    /// PID controller parameters
    pub pid_params: PIDParameters,
}

/// Control system operational modes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ControlMode {
    /// System idle/stopped
    Idle,
    /// Automatic control active
    Auto,
    /// Manual control mode
    Manual,
    /// Homing/calibration mode
    Homing,
    /// Diagnostic/test mode
    Diagnostic,
    /// Emergency/safe mode
    Emergency,
}

/// System operational status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SystemStatus {
    /// System running normally
    Running,
    /// System starting up
    Starting,
    /// System stopping
    Stopping,
    /// System in error state
    Error,
    /// System in maintenance mode
    Maintenance,
    /// System in safe state
    Safe,
}

/// PID controller parameters
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PIDParameters {
    /// Proportional gain
    pub kp: f64,
    /// Integral gain
    pub ki: f64,
    /// Derivative gain
    pub kd: f64,
    /// Integral windup limit
    pub integral_limit: f64,
    /// Output saturation minimum limit
    pub output_min: f64,
    /// Output saturation maximum limit
    pub output_max: f64,
    /// Sample time in seconds
    pub sample_time: f64,
}

/// Real-time control commands from supervisory systems
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCommand {
    /// Command identifier
    pub command_id: u64,
    /// Command type
    pub command_type: CommandType,
    /// Target position (for position commands)
    pub target_position: Option<f64>,
    /// Target velocity (for velocity commands)
    pub target_velocity: Option<f64>,
    /// Command priority (higher = more urgent)
    pub priority: u8,
    /// Command timestamp in microseconds
    pub timestamp_us: u64,
    /// Execution deadline in microseconds
    pub deadline_us: u64,
    /// Command source identifier
    pub source: String,
    /// Additional parameters
    pub parameters: Vec<(String, f64)>,
}

/// Control command types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommandType {
    /// Move to absolute position
    MoveAbsolute,
    /// Move relative distance
    MoveRelative,
    /// Set velocity
    SetVelocity,
    /// Stop motion
    Stop,
    /// Emergency stop
    EmergencyStop,
    /// Home/calibrate
    Home,
    /// Set control mode
    SetMode(ControlMode),
    /// Update PID parameters
    UpdatePID(PIDParameters),
    /// Reset system
    Reset,
}

/// Performance metrics for control system monitoring
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Average control loop execution time in nanoseconds
    pub avg_loop_time_ns: u64,
    /// Maximum control loop execution time in nanoseconds
    pub max_loop_time_ns: u64,
    /// Minimum control loop execution time in nanoseconds
    pub min_loop_time_ns: u64,
    /// Standard deviation of loop times
    pub loop_time_std_dev_ns: u64,
    /// Number of deadline misses
    pub deadline_misses: u64,
    /// Total control cycles executed
    pub total_cycles: u64,
    /// CPU utilization percentage
    pub cpu_utilization: f32,
    /// Memory usage in bytes
    pub memory_usage: u64,
    /// Last metrics update timestamp
    pub timestamp_us: u64,
}

// Default implementations
impl Default for ControlState {
    fn default() -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
            acceleration: 0.0,
            target_position: 0.0,
            position_error: 0.0,
            control_output: 0.0,
            control_mode: ControlMode::Idle,
            system_status: SystemStatus::Starting,
            emergency_stop: false,
            safety_interlocks: 0,
            loop_frequency: 1000.0,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            cycle_count: 0,
            pid_params: PIDParameters::default(),
        }
    }
}

impl Default for PIDParameters {
    fn default() -> Self {
        Self {
            kp: 1.0,
            ki: 0.1,
            kd: 0.01,
            integral_limit: 100.0,
            output_min: -100.0,
            output_max: 100.0,
            sample_time: 0.001, // 1ms
        }
    }
}

impl Default for ControlCommand {
    fn default() -> Self {
        Self {
            command_id: 0,
            command_type: CommandType::Stop,
            target_position: None,
            target_velocity: None,
            priority: 0,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            deadline_us: 0,
            source: String::new(),
            parameters: Vec::new(),
        }
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            avg_loop_time_ns: 0,
            max_loop_time_ns: 0,
            min_loop_time_ns: u64::MAX,
            loop_time_std_dev_ns: 0,
            deadline_misses: 0,
            total_cycles: 0,
            cpu_utilization: 0.0,
            memory_usage: 0,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
        }
    }
}
