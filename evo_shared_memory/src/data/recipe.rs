//! Recipe execution data structures

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Recipe execution state and progress information
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeExecutionState {
    /// Currently active recipe identifier
    pub recipe_id: String,
    /// Recipe name/description
    pub recipe_name: String,
    /// Current execution status
    pub status: RecipeStatus,
    /// Current step number (0-based)
    pub current_step: u32,
    /// Total number of steps
    pub total_steps: u32,
    /// Overall progress percentage (0.0 - 100.0)
    pub progress_percent: f64,
    /// Recipe start timestamp
    pub start_timestamp_us: u64,
    /// Expected completion timestamp
    pub estimated_completion_us: u64,
    /// Current step start timestamp
    pub step_start_timestamp_us: u64,
    /// Recipe execution variables
    pub variables: HashMap<String, f64>,
    /// Error information if execution failed
    pub error_message: String,
    /// Last update timestamp
    pub timestamp_us: u64,
}

/// Recipe execution status enumeration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RecipeStatus {
    /// Recipe is idle/not running
    Idle,
    /// Recipe is loading/preparing
    Loading,
    /// Recipe is executing normally
    Running,
    /// Recipe execution is paused
    Paused,
    /// Recipe completed successfully
    Completed,
    /// Recipe execution failed
    Failed,
    /// Recipe was aborted by user
    Aborted,
    /// Recipe is in cleanup phase
    Cleanup,
}

/// Individual recipe step execution state
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecutionState {
    /// Step number (0-based)
    pub step_number: u32,
    /// Step name/description
    pub step_name: String,
    /// Step type
    pub step_type: StepType,
    /// Current step status
    pub status: StepStatus,
    /// Step progress percentage (0.0 - 100.0)
    pub progress_percent: f64,
    /// Step start timestamp
    pub start_timestamp_us: u64,
    /// Expected step duration in microseconds
    pub expected_duration_us: u64,
    /// Actual step duration (when completed)
    pub actual_duration_us: Option<u64>,
    /// Step parameters
    pub parameters: HashMap<String, f64>,
    /// Step results/outputs
    pub results: HashMap<String, f64>,
    /// Error message if step failed
    pub error_message: String,
    /// Retry count for this step
    pub retry_count: u32,
    /// Maximum allowed retries
    pub max_retries: u32,
    /// Last update timestamp
    pub timestamp_us: u64,
}

/// Recipe step types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepType {
    /// Move to position
    Move,
    /// Wait for duration
    Wait,
    /// Set parameter value
    SetParameter,
    /// Read sensor value
    ReadSensor,
    /// Execute control action
    Control,
    /// Conditional logic
    Condition,
    /// Loop/iteration
    Loop,
    /// Call subroutine
    Subroutine,
    /// Custom action
    Custom(String),
}

/// Step execution status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    /// Step is pending execution
    Pending,
    /// Step is currently executing
    Executing,
    /// Step completed successfully
    Completed,
    /// Step failed
    Failed,
    /// Step was skipped
    Skipped,
    /// Step is waiting for condition
    Waiting,
}

/// Recipe command structure for controlling execution
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeCommand {
    /// Command identifier
    pub command_id: u64,
    /// Command type
    pub command_type: RecipeCommandType,
    /// Target recipe identifier (for start commands)
    pub recipe_id: Option<String>,
    /// Command parameters
    pub parameters: HashMap<String, f64>,
    /// Command priority
    pub priority: u8,
    /// Command timestamp
    pub timestamp_us: u64,
    /// Command source
    pub source: String,
}

/// Recipe command types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RecipeCommandType {
    /// Start recipe execution
    Start,
    /// Pause recipe execution
    Pause,
    /// Resume paused recipe
    Resume,
    /// Stop recipe execution
    Stop,
    /// Abort recipe execution
    Abort,
    /// Skip current step
    SkipStep,
    /// Retry current step
    RetryStep,
    /// Set recipe variable
    /// Set a variable to a specific value
    SetVariable {
        /// Variable name to set
        name: String,
        /// Value to set the variable to
        value: f64,
    },
    /// Jump to specific step
    JumpToStep(u32),
}

// Default implementations
impl Default for RecipeExecutionState {
    fn default() -> Self {
        Self {
            recipe_id: String::new(),
            recipe_name: String::new(),
            status: RecipeStatus::Idle,
            current_step: 0,
            total_steps: 0,
            progress_percent: 0.0,
            start_timestamp_us: 0,
            estimated_completion_us: 0,
            step_start_timestamp_us: 0,
            variables: HashMap::new(),
            error_message: String::new(),
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
        }
    }
}

impl Default for StepExecutionState {
    fn default() -> Self {
        Self {
            step_number: 0,
            step_name: String::new(),
            step_type: StepType::Wait,
            status: StepStatus::Pending,
            progress_percent: 0.0,
            start_timestamp_us: 0,
            expected_duration_us: 0,
            actual_duration_us: None,
            parameters: HashMap::new(),
            results: HashMap::new(),
            error_message: String::new(),
            retry_count: 0,
            max_retries: 3,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
        }
    }
}

impl Default for RecipeCommand {
    fn default() -> Self {
        Self {
            command_id: 0,
            command_type: RecipeCommandType::Stop,
            recipe_id: None,
            parameters: HashMap::new(),
            priority: 0,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
            source: String::new(),
        }
    }
}
