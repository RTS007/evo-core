//! API and gRPC data structures

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// API request tracking and performance metrics
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequestMetrics {
    /// Request unique identifier
    pub request_id: String,
    /// Timestamp in microseconds since UNIX epoch
    pub timestamp_us: u64,
    /// gRPC method name
    pub method_name: String,
    /// Client identification
    pub client_id: String,
    /// Request processing status
    pub status: RequestStatus,
    /// Request start time
    pub start_time_us: u64,
    /// Request completion time (0 if not completed)
    pub completion_time_us: u64,
    /// Request processing duration in microseconds
    pub duration_us: u64,
    /// Response size in bytes
    pub response_size_bytes: u32,
    /// Request size in bytes
    pub request_size_bytes: u32,
    /// Error code if request failed
    pub error_code: u32,
    /// Error message if request failed
    pub error_message: String,
    /// Authentication status
    pub auth_status: AuthStatus,
}

/// Request processing status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RequestStatus {
    /// Request received and queued
    Received,
    /// Request authentication in progress
    Authenticating,
    /// Request being processed
    Processing,
    /// Request completed successfully
    Completed,
    /// Request failed
    Failed,
    /// Request cancelled
    Cancelled,
    /// Request timed out
    Timeout,
}

/// Authentication status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthStatus {
    /// Not authenticated
    None,
    /// Authentication in progress
    Pending,
    /// Authentication successful
    Authenticated,
    /// Authentication failed
    Failed,
    /// Authentication expired
    Expired,
}

/// Client session information
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSession {
    /// Session unique identifier
    pub session_id: String,
    /// Client IP address
    pub client_ip: String,
    /// Client user agent
    pub user_agent: String,
    /// Session start timestamp
    pub start_timestamp_us: u64,
    /// Last activity timestamp
    pub last_activity_us: u64,
    /// Authentication status
    pub auth_status: AuthStatus,
    /// User identifier (if authenticated)
    pub user_id: Option<String>,
    /// Session permissions
    pub permissions: Vec<String>,
    /// Request count for this session
    pub request_count: u64,
    /// Session timeout in microseconds
    pub timeout_us: u64,
}

/// Aggregated system state for API responses
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStateSnapshot {
    /// Snapshot timestamp in microseconds
    pub timestamp_us: u64,
    /// Overall system health status
    pub system_health: SystemHealth,
    /// Control system summary
    pub control_summary: ControlSystemSummary,
    /// Hardware summary
    pub hardware_summary: HardwareSummary,
    /// Recipe execution summary
    pub recipe_summary: RecipeSummary,
    /// Performance metrics summary
    pub performance_summary: PerformanceSummary,
    /// Active alarms and warnings
    pub active_alarms: Vec<SystemAlarm>,
    /// System uptime in microseconds
    pub uptime_us: u64,
}

/// Overall system health enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SystemHealth {
    /// All systems operating normally
    Healthy,
    /// Minor issues detected
    Warning,
    /// Significant issues detected
    Degraded,
    /// Critical errors present
    Critical,
    /// System offline or unreachable
    Offline,
}

/// Control system summary for API
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlSystemSummary {
    /// Current control mode
    pub control_mode: String,
    /// Current position
    pub current_position: f64,
    /// Target position
    pub target_position: f64,
    /// Position error
    pub position_error: f64,
    /// Emergency stop status
    pub emergency_stop: bool,
    /// Control loop frequency
    pub loop_frequency: f64,
    /// Number of active safety interlocks
    pub active_interlocks: u32,
}

/// Hardware summary for API
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSummary {
    /// Number of active sensors
    pub active_sensors: u32,
    /// Number of active actuators
    pub active_actuators: u32,
    /// Number of sensors in error state
    pub sensors_in_error: u32,
    /// Number of actuators in error state
    pub actuators_in_error: u32,
    /// Average sensor update frequency
    pub sensor_update_freq: f32,
    /// Communication status
    pub comm_status: String,
}

/// Recipe execution summary for API
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeSummary {
    /// Number of active recipes
    pub active_recipes: u32,
    /// Current recipe name
    pub current_recipe: String,
    /// Overall execution progress
    pub current_progress: f64,
    /// Execution status
    pub execution_status: String,
    /// Steps completed
    pub steps_completed: u32,
    /// Total steps
    pub total_steps: u32,
}

/// Performance metrics summary for API
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSummary {
    /// CPU usage percentage
    pub cpu_usage: f64,
    /// Memory usage percentage
    pub memory_usage: f64,
    /// Average API response time in microseconds
    pub avg_response_time_us: f64,
    /// Number of requests processed
    pub requests_processed: u64,
    /// Number of failed requests
    pub failed_requests: u64,
    /// System load average
    pub load_average: f64,
}

/// System alarm information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemAlarm {
    /// Alarm unique identifier
    pub alarm_id: String,
    /// Alarm severity level
    pub severity: AlarmSeverity,
    /// Alarm source component
    pub source: String,
    /// Alarm message
    pub message: String,
    /// Alarm timestamp
    pub timestamp_us: u64,
    /// Alarm acknowledgment status
    pub acknowledged: bool,
}

/// Alarm severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum AlarmSeverity {
    /// Informational message
    Info,
    /// Warning condition
    Warning,
    /// Error condition
    Error,
    /// Critical alarm
    Critical,
}

// Default implementations
impl Default for ApiRequestMetrics {
    fn default() -> Self {
        Self {
            request_id: String::new(),
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            method_name: String::new(),
            client_id: String::new(),
            status: RequestStatus::Received,
            start_time_us: 0,
            completion_time_us: 0,
            duration_us: 0,
            response_size_bytes: 0,
            request_size_bytes: 0,
            error_code: 0,
            error_message: String::new(),
            auth_status: AuthStatus::None,
        }
    }
}

impl Default for ClientSession {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        Self {
            session_id: String::new(),
            client_ip: String::new(),
            user_agent: String::new(),
            start_timestamp_us: now,
            last_activity_us: now,
            auth_status: AuthStatus::None,
            user_id: None,
            permissions: Vec::new(),
            request_count: 0,
            timeout_us: 3600_000_000, // 1 hour
        }
    }
}

impl Default for SystemStateSnapshot {
    fn default() -> Self {
        Self {
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            system_health: SystemHealth::Healthy,
            control_summary: ControlSystemSummary::default(),
            hardware_summary: HardwareSummary::default(),
            recipe_summary: RecipeSummary::default(),
            performance_summary: PerformanceSummary::default(),
            active_alarms: Vec::new(),
            uptime_us: 0,
        }
    }
}

impl Default for ControlSystemSummary {
    fn default() -> Self {
        Self {
            control_mode: "Idle".to_string(),
            current_position: 0.0,
            target_position: 0.0,
            position_error: 0.0,
            emergency_stop: false,
            loop_frequency: 1000.0,
            active_interlocks: 0,
        }
    }
}

impl Default for HardwareSummary {
    fn default() -> Self {
        Self {
            active_sensors: 0,
            active_actuators: 0,
            sensors_in_error: 0,
            actuators_in_error: 0,
            sensor_update_freq: 100.0,
            comm_status: "Unknown".to_string(),
        }
    }
}

impl Default for RecipeSummary {
    fn default() -> Self {
        Self {
            active_recipes: 0,
            current_recipe: String::new(),
            current_progress: 0.0,
            execution_status: "Idle".to_string(),
            steps_completed: 0,
            total_steps: 0,
        }
    }
}

impl Default for PerformanceSummary {
    fn default() -> Self {
        Self {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            avg_response_time_us: 0.0,
            requests_processed: 0,
            failed_requests: 0,
            load_average: 0.0,
        }
    }
}
