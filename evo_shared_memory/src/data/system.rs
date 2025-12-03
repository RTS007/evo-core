//! System-wide data structures and module coordination

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// EVO module status information for system coordination
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvoModuleStatus {
    /// Module unique identifier
    pub module_id: String,
    /// Module type
    pub module_type: ModuleType,
    /// Process ID of the module
    pub process_id: u32,
    /// Current module state
    pub state: ModuleState,
    /// Module health status
    pub health: ModuleHealth,
    /// Module startup timestamp
    pub startup_timestamp_us: u64,
    /// Last heartbeat timestamp
    pub last_heartbeat_us: u64,
    /// Heartbeat interval in microseconds
    pub heartbeat_interval_us: u64,
    /// Module version information
    pub version: String,
    /// Shared memory segments managed by this module
    pub managed_segments: Vec<String>,
    /// Current CPU usage percentage
    pub cpu_usage: f32,
    /// Current memory usage in bytes
    pub memory_usage: u64,
    /// Number of active connections/clients
    pub active_connections: u32,
    /// Module-specific metrics
    pub custom_metrics: HashMap<String, f64>,
    /// Error information if module is in error state
    pub error_info: Option<ModuleError>,
}

/// EVO module types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModuleType {
    /// System supervisor
    Supervisor,
    /// Control unit
    ControlUnit,
    /// Hardware abstraction layer
    HalCore,
    /// Recipe executor
    RecipeExecutor,
    /// gRPC API service
    ApiLiaison,
    /// Dashboard service
    Dashboard,
    /// Diagnostic service
    Diagnostic,
    /// MQTT service
    Mqtt,
    /// Custom module type
    Custom(String),
}

/// Module operational state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModuleState {
    /// Module is starting up
    Starting,
    /// Module is running normally
    Running,
    /// Module is stopping
    Stopping,
    /// Module has stopped
    Stopped,
    /// Module is in error state
    Error,
    /// Module is in maintenance mode
    Maintenance,
    /// Module is restarting
    Restarting,
}

/// Module health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum ModuleHealth {
    /// Module is healthy
    Healthy,
    /// Module has minor issues
    Warning,
    /// Module has significant issues
    Degraded,
    /// Module is in critical state
    Critical,
    /// Module is unresponsive
    Unresponsive,
}

/// Module error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleError {
    /// Error code
    pub error_code: u32,
    /// Error message
    pub message: String,
    /// Error timestamp
    pub timestamp_us: u64,
    /// Error source/location
    pub source: String,
    /// Error stack trace (if available)
    pub stack_trace: Option<String>,
    /// Recovery suggestions
    pub recovery_suggestions: Vec<String>,
}

/// System-wide health and status information
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemState {
    /// System startup timestamp
    pub startup_timestamp_us: u64,
    /// Current system timestamp
    pub current_timestamp_us: u64,
    /// Overall system health
    pub overall_health: SystemHealth,
    /// Number of running modules
    pub running_modules: u32,
    /// Number of modules in error state
    pub error_modules: u32,
    /// Total CPU usage across all modules
    pub total_cpu_usage: f32,
    /// Total memory usage across all modules
    pub total_memory_usage: u64,
    /// System load average
    pub load_average: f64,
    /// Number of active shared memory segments
    pub active_segments: u32,
    /// Total shared memory usage in bytes
    pub total_shm_usage: u64,
    /// Number of active API connections
    pub active_api_connections: u32,
    /// System configuration version
    pub config_version: u32,
    /// Emergency stop status
    pub emergency_stop_active: bool,
    /// Maintenance mode status
    pub maintenance_mode: bool,
}

/// Overall system health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum SystemHealth {
    /// All systems operational
    Healthy,
    /// Minor issues detected
    Warning,
    /// Significant issues affecting performance
    Degraded,
    /// Critical issues requiring attention
    Critical,
    /// System is offline or unreachable
    Offline,
}

/// System command for coordinating module actions
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCommand {
    /// Command unique identifier
    pub command_id: u64,
    /// Command type
    pub command_type: SystemCommandType,
    /// Target module (None = all modules)
    pub target_module: Option<String>,
    /// Command parameters
    pub parameters: HashMap<String, String>,
    /// Command priority (higher = more urgent)
    pub priority: u8,
    /// Command timestamp
    pub timestamp_us: u64,
    /// Command source module
    pub source: String,
    /// Expected completion timeout
    pub timeout_us: u64,
}

/// System command types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SystemCommandType {
    /// Start module
    StartModule,
    /// Stop module
    StopModule,
    /// Restart module
    RestartModule,
    /// Update module configuration
    UpdateConfig,
    /// Trigger emergency stop
    EmergencyStop,
    /// Clear emergency stop
    ClearEmergencyStop,
    /// Enter maintenance mode
    EnterMaintenance,
    /// Exit maintenance mode
    ExitMaintenance,
    /// Request status update
    StatusUpdate,
    /// Shutdown system
    Shutdown,
}

// Default implementations
impl Default for EvoModuleStatus {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        Self {
            module_id: String::new(),
            module_type: ModuleType::Custom("Unknown".to_string()),
            process_id: 0,
            state: ModuleState::Starting,
            health: ModuleHealth::Healthy,
            startup_timestamp_us: now,
            last_heartbeat_us: now,
            heartbeat_interval_us: 1_000_000, // 1 second
            version: String::new(),
            managed_segments: Vec::new(),
            cpu_usage: 0.0,
            memory_usage: 0,
            active_connections: 0,
            custom_metrics: HashMap::new(),
            error_info: None,
        }
    }
}

impl Default for SystemState {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        Self {
            startup_timestamp_us: now,
            current_timestamp_us: now,
            overall_health: SystemHealth::Healthy,
            running_modules: 0,
            error_modules: 0,
            total_cpu_usage: 0.0,
            total_memory_usage: 0,
            load_average: 0.0,
            active_segments: 0,
            total_shm_usage: 0,
            active_api_connections: 0,
            config_version: 1,
            emergency_stop_active: false,
            maintenance_mode: false,
        }
    }
}

impl Default for SystemCommand {
    fn default() -> Self {
        Self {
            command_id: 0,
            command_type: SystemCommandType::StatusUpdate,
            target_module: None,
            parameters: HashMap::new(),
            priority: 0,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            source: String::new(),
            timeout_us: 30_000_000, // 30 seconds
        }
    }
}
