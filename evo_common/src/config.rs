//! Configuration loading traits and types.
//!
//! This module provides a standardized way to load TOML configuration files
//! across all EVO applications, including the unified `load_config_dir()` API
//! that auto-discovers axis files and validates consistency.
//!
//! # Usage
//!
//! ```rust,no_run
//! use evo_common::config::{load_config_dir, ConfigError};
//! use std::path::Path;
//!
//! let full = load_config_dir(Path::new("config")).expect("load all configs");
//! println!("Machine: {}", full.machine.machine.name);
//! println!("Axes: {}", full.axes.len());
//! ```

use serde::{Deserialize, Serialize};

/// Log level for configuration (replaces `log::Level`).
///
/// Serializes to lowercase strings: "trace", "debug", "info", "warn", "error".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Trace-level verbosity.
    Trace,
    /// Debug-level verbosity.
    Debug,
    /// Info-level verbosity (default).
    Info,
    /// Warning-level verbosity.
    Warn,
    /// Error-level verbosity.
    Error,
}
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Error type for configuration loading operations.
///
/// This enum represents all possible errors that can occur when loading
/// configuration files.
#[derive(Debug, Clone, Error)]
pub enum ConfigError {
    /// Configuration file not found at specified path.
    #[error("Configuration file not found")]
    FileNotFound,

    /// TOML parsing failed.
    #[error("Failed to parse configuration: {0}")]
    ParseError(String),

    /// Semantic validation failed.
    #[error("Configuration validation failed: {0}")]
    ValidationError(String),

    /// Unknown field in TOML (strict parsing with `deny_unknown_fields`).
    #[error("Unknown field: {0}")]
    UnknownField(String),

    /// Duplicate axis ID (two axis files have same NN prefix).
    #[error("Duplicate axis ID: {0}")]
    DuplicateAxisId(u8),

    /// Axis ID in file does not match NN prefix in filename.
    #[error("Axis ID mismatch in {file}: expected {expected}, found {found}")]
    AxisIdMismatch {
        /// Filename containing the mismatch.
        file: String,
        /// Expected ID from filename NN prefix.
        expected: u8,
        /// Actual ID read from `[axis].id` field.
        found: u8,
    },

    /// No axis configuration files found in the config directory.
    #[error("No axis files found in config directory")]
    NoAxesDefined,
}

/// Common configuration fields shared across all EVO applications.
///
/// This struct should be embedded in application-specific configuration
/// structs to provide consistent base configuration.
///
/// # TOML Example
///
/// ```toml
/// [shared]
/// log_level = "debug"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedConfig {
    #[serde(default = "default_log_level")]
    pub log_level: LogLevel,

    pub service_name: String,
}

fn default_log_level() -> LogLevel {
    LogLevel::Info
}

impl SharedConfig {
    /// Validate the configuration.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::ValidationError` if:
    /// - `service_name` is empty
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.service_name.is_empty() {
            return Err(ConfigError::ValidationError(
                "service_name cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Trait for loading configuration from TOML files.
///
/// This trait provides a default implementation that works with any type
/// implementing `serde::de::DeserializeOwned`.
///
/// # Contract
///
/// - Returns `ConfigError::FileNotFound` if the file does not exist
/// - Returns `ConfigError::ParseError` if TOML syntax is invalid
/// - Returns `ConfigError::ValidationError` if semantic validation fails
///
/// # Example
///
/// ```rust,no_run
/// use evo_common::config::{ConfigLoader, SharedConfig, ConfigError};
/// use serde::Deserialize;
/// use std::path::Path;
///
/// #[derive(Debug, Deserialize)]
/// struct AppConfig {
///     shared: SharedConfig,
/// }
///
/// fn main() -> Result<(), ConfigError> {
///     let config = AppConfig::load(Path::new("config.toml"))?;
///     Ok(())
/// }
/// ```
pub trait ConfigLoader: Sized + serde::de::DeserializeOwned {
    /// Load configuration from a TOML file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TOML configuration file
    ///
    /// # Returns
    ///
    /// * `Ok(Self)` - Successfully loaded and parsed configuration
    /// * `Err(ConfigError)` - Loading or parsing failed
    fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::FileNotFound
            } else {
                ConfigError::ParseError(e.to_string())
            }
        })?;

        toml::from_str(&content).map_err(|e| ConfigError::ParseError(e.to_string()))
    }
}

// Blanket implementation for all types that implement DeserializeOwned.
// This allows any serde-deserializable struct to use ConfigLoader.
impl<T: serde::de::DeserializeOwned> ConfigLoader for T {}

// ─── Numeric Bounds Constants (FR-054) ─────────────────────────────

/// Minimum Kp gain value.
pub const MIN_KP: f64 = 0.0;
/// Maximum Kp gain value.
pub const MAX_KP: f64 = 10_000.0;
/// Minimum Ki gain value.
pub const MIN_KI: f64 = 0.0;
/// Maximum Ki gain value.
pub const MAX_KI: f64 = 10_000.0;
/// Minimum Kd gain value.
pub const MIN_KD: f64 = 0.0;
/// Maximum Kd gain value.
pub const MAX_KD: f64 = 1_000.0;
/// Maximum velocity (mm/s or deg/s).
pub const MAX_VELOCITY: f64 = 100_000.0;
/// Maximum acceleration (mm/s² or deg/s²).
pub const MAX_ACCELERATION: f64 = 1_000_000.0;
/// Minimum cycle time in microseconds.
pub const MIN_CYCLE_TIME_US: u32 = 100;
/// Maximum cycle time in microseconds.
pub const MAX_CYCLE_TIME_US: u32 = 100_000;
/// Maximum axis count.
pub const MAX_AXIS_COUNT: usize = crate::consts::MAX_AXES;
/// Maximum position range (absolute value).
pub const MAX_POSITION_RANGE: f64 = 1_000_000.0;
/// Maximum out_max control output.
pub const MAX_OUT_MAX: f64 = 1_000.0;
/// Maximum lag error limit.
pub const MAX_LAG_ERROR: f64 = 100.0;
/// Maximum homing speed.
pub const MAX_HOMING_SPEED: f64 = 10_000.0;
/// Maximum homing timeout.
pub const MAX_HOMING_TIMEOUT: f64 = 300.0;
/// Maximum safe deceleration.
pub const MAX_SAFE_DECEL: f64 = 1_000_000.0;

// ─── WatchdogConfig ────────────────────────────────────────────────

fn default_max_restarts() -> u32 {
    5
}
fn default_initial_backoff_ms() -> u64 {
    100
}
fn default_max_backoff_s() -> u64 {
    30
}
fn default_stable_run_s() -> u64 {
    60
}
fn default_sigterm_timeout_s() -> f64 {
    2.0
}
fn default_hal_ready_timeout_s() -> f64 {
    5.0
}

/// Watchdog configuration — how `evo` binary manages child processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchdogConfig {
    /// Maximum consecutive restarts before degraded state (1..=100).
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    /// Initial restart delay in milliseconds (10..=10_000).
    #[serde(default = "default_initial_backoff_ms")]
    pub initial_backoff_ms: u64,
    /// Maximum restart delay in seconds (1..=300).
    #[serde(default = "default_max_backoff_s")]
    pub max_backoff_s: u64,
    /// Successful run duration to reset backoff counter in seconds (10..=3600).
    #[serde(default = "default_stable_run_s")]
    pub stable_run_s: u64,
    /// Timeout before escalating to SIGKILL in seconds (0.5..=30.0).
    #[serde(default = "default_sigterm_timeout_s")]
    pub sigterm_timeout_s: f64,
    /// Timeout waiting for `evo_hal_cu` segment in seconds (1.0..=60.0).
    #[serde(default = "default_hal_ready_timeout_s")]
    pub hal_ready_timeout_s: f64,
}

impl WatchdogConfig {
    /// Validate all fields against allowed bounds.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(1..=100).contains(&self.max_restarts) {
            return Err(ConfigError::ValidationError(format!(
                "watchdog.max_restarts={} out of range [1, 100]",
                self.max_restarts
            )));
        }
        if !(10..=10_000).contains(&self.initial_backoff_ms) {
            return Err(ConfigError::ValidationError(format!(
                "watchdog.initial_backoff_ms={} out of range [10, 10000]",
                self.initial_backoff_ms
            )));
        }
        if !(1..=300).contains(&self.max_backoff_s) {
            return Err(ConfigError::ValidationError(format!(
                "watchdog.max_backoff_s={} out of range [1, 300]",
                self.max_backoff_s
            )));
        }
        if !(10..=3600).contains(&self.stable_run_s) {
            return Err(ConfigError::ValidationError(format!(
                "watchdog.stable_run_s={} out of range [10, 3600]",
                self.stable_run_s
            )));
        }
        if !(0.5..=30.0).contains(&self.sigterm_timeout_s) {
            return Err(ConfigError::ValidationError(format!(
                "watchdog.sigterm_timeout_s={} out of range [0.5, 30.0]",
                self.sigterm_timeout_s
            )));
        }
        if !(1.0..=60.0).contains(&self.hal_ready_timeout_s) {
            return Err(ConfigError::ValidationError(format!(
                "watchdog.hal_ready_timeout_s={} out of range [1.0, 60.0]",
                self.hal_ready_timeout_s
            )));
        }
        Ok(())
    }
}

// ─── SystemConfig ──────────────────────────────────────────────────

/// Top-level system configuration — loaded from `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemConfig {
    /// Watchdog process management configuration.
    pub watchdog: WatchdogConfig,
    /// HAL program configuration (placeholder).
    #[serde(default)]
    pub hal: Option<toml::Value>,
    /// CU program configuration (placeholder).
    #[serde(default)]
    pub cu: Option<toml::Value>,
    /// Recipe executor configuration (placeholder).
    #[serde(default)]
    pub re: Option<toml::Value>,
    /// MQTT bridge configuration (placeholder).
    #[serde(default)]
    pub mqtt: Option<toml::Value>,
    /// gRPC API configuration (placeholder).
    #[serde(default)]
    pub grpc: Option<toml::Value>,
    /// HTTP API configuration (placeholder).
    #[serde(default)]
    pub api: Option<toml::Value>,
    /// Dashboard configuration (placeholder).
    #[serde(default)]
    pub dashboard: Option<toml::Value>,
    /// Diagnostic configuration (placeholder).
    #[serde(default)]
    pub diagnostic: Option<toml::Value>,
}

// ─── MachineConfig (unified) ───────────────────────────────────────

/// Machine identity section from `machine.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineIdentity {
    /// Machine display name.
    pub name: String,
}

/// Global safety configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalSafetyConfig {
    /// Default safe stop category: `"SS1"`, `"SS2"`, or `"STO"`.
    pub default_safe_stop: String,
    /// Safety stop timeout in seconds.
    pub safety_stop_timeout: f64,
    /// Whether operator authorization is required for recovery.
    pub recovery_authorization_required: bool,
}

/// Service bypass configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceBypassConfig {
    /// Axis IDs that can be bypassed in service mode.
    pub bypass_axes: Vec<u8>,
    /// Maximum velocity in service mode.
    pub max_service_velocity: f64,
}

/// Machine configuration — loaded from `machine.toml`.
///
/// Contains only machine-specific parameters. No axes, no I/O.
/// Axis configs are auto-discovered from `axis_NN_*.toml` files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewMachineConfig {
    /// Machine identity.
    pub machine: MachineIdentity,
    /// Global safety parameters.
    pub global_safety: GlobalSafetyConfig,
    /// Service bypass parameters.
    pub service_bypass: ServiceBypassConfig,
}

// ─── Per-Axis Config ───────────────────────────────────────────────

/// Axis identity section (`[axis]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisIdentity {
    /// Axis number (must match NN in filename).
    pub id: u8,
    /// Axis display name.
    pub name: String,
    /// Axis type: `"linear"` or `"rotary"`.
    #[serde(rename = "type")]
    pub axis_type: String,
}

/// Kinematics section (`[kinematics]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KinematicsConfig {
    /// Maximum velocity (mm/s or deg/s).
    pub max_velocity: f64,
    /// Maximum acceleration (optional).
    pub max_acceleration: Option<f64>,
    /// Safe reduced speed limit for safety mode.
    pub safe_reduced_speed_limit: f64,
    /// Minimum position.
    pub min_pos: f64,
    /// Maximum position.
    pub max_pos: f64,
    /// In-position window (tolerance).
    #[serde(default = "default_in_position_window")]
    pub in_position_window: f64,
}

fn default_in_position_window() -> f64 {
    0.05
}

/// Control (PID) section (`[control]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlConfig {
    /// Proportional gain.
    pub kp: f64,
    /// Integral gain.
    pub ki: f64,
    /// Derivative gain.
    pub kd: f64,
    /// Filter time constant.
    #[serde(default = "default_tf")]
    pub tf: f64,
    /// Tracking time constant.
    #[serde(default = "default_tt")]
    pub tt: f64,
    /// Velocity feedforward gain.
    #[serde(default)]
    pub kvff: f64,
    /// Acceleration feedforward gain.
    #[serde(default)]
    pub kaff: f64,
    /// Friction compensation.
    #[serde(default)]
    pub friction: f64,
    /// Jerk normalization.
    #[serde(default = "default_jn")]
    pub jn: f64,
    /// Bandwidth normalization.
    #[serde(default = "default_bn")]
    pub bn: f64,
    /// Disturbance observer gain.
    #[serde(default = "default_gdob")]
    pub gdob: f64,
    /// Notch filter frequency (0 = disabled).
    #[serde(default)]
    pub f_notch: f64,
    /// Notch filter bandwidth (0 = disabled).
    #[serde(default)]
    pub bw_notch: f64,
    /// Low-pass filter frequency (0 = disabled).
    #[serde(default)]
    pub flp: f64,
    /// Maximum control output.
    #[serde(default = "default_out_max")]
    pub out_max: f64,
    /// Lag error limit.
    pub lag_error_limit: f64,
    /// Lag policy: `"Unwanted"`, `"Warning"`, or `"Error"`.
    #[serde(default = "default_lag_policy")]
    pub lag_policy: String,
}

fn default_tf() -> f64 {
    0.001
}
fn default_tt() -> f64 {
    0.01
}
fn default_jn() -> f64 {
    0.01
}
fn default_bn() -> f64 {
    0.001
}
fn default_gdob() -> f64 {
    200.0
}
fn default_out_max() -> f64 {
    100.0
}
fn default_lag_policy() -> String {
    "Error".to_string()
}

/// Safe stop section (`[safe_stop]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafeStopConfig {
    /// Category: `"SS1"`, `"SS2"`, or `"STO"`.
    pub category: String,
    /// Maximum safe deceleration.
    pub max_decel_safe: f64,
    /// STO brake engage delay in seconds.
    #[serde(default = "default_sto_brake_delay")]
    pub sto_brake_delay: f64,
    /// SS2 holding torque.
    #[serde(default)]
    pub ss2_holding_torque: f64,
}

fn default_sto_brake_delay() -> f64 {
    0.1
}

/// Homing section (`[homing]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomingConfig {
    /// Method: `"HomeSensor"`, `"TorqueLimit"`, or `"IndexPulse"`.
    pub method: String,
    /// Homing speed.
    pub speed: f64,
    /// Torque limit for torque-based homing.
    #[serde(default = "default_torque_limit")]
    pub torque_limit: f64,
    /// Homing timeout in seconds.
    #[serde(default = "default_homing_timeout")]
    pub timeout: f64,
    /// Approach direction: `"Positive"` or `"Negative"`.
    #[serde(default = "default_approach_direction")]
    pub approach_direction: String,
}

fn default_torque_limit() -> f64 {
    30.0
}
fn default_homing_timeout() -> f64 {
    30.0
}
fn default_approach_direction() -> String {
    "Positive".to_string()
}

/// Brake section (optional, `[brake]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrakeConfig {
    /// Digital output role for brake engage.
    pub do_brake: String,
    /// Digital input role for brake released sensor.
    pub di_released: String,
    /// Release timeout in seconds.
    #[serde(default = "default_release_timeout")]
    pub release_timeout: f64,
    /// Engage timeout in seconds.
    #[serde(default = "default_engage_timeout")]
    pub engage_timeout: f64,
}

fn default_release_timeout() -> f64 {
    2.0
}
fn default_engage_timeout() -> f64 {
    1.0
}

/// Tailstock section (optional, `[tailstock]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TailstockConfig {
    /// Coupled axis ID.
    pub coupled_axis: u8,
    /// Clamp output role.
    pub clamp_role: String,
    /// Clamped sensor role (optional).
    pub clamped_role: Option<String>,
    /// Maximum force (optional).
    pub max_force: Option<f64>,
}

/// Guard section (optional, `[guard]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuardConfig {
    /// Digital input role for guard sensor.
    pub di_guard: String,
    /// Safe stop on guard open: `"SS1"`, `"SS2"`, or `"STO"`.
    #[serde(default = "default_stop_on_open")]
    pub stop_on_open: String,
}

fn default_stop_on_open() -> String {
    "SS1".to_string()
}

/// Coupling section (optional, `[coupling]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CouplingConfig {
    /// Master axis ID.
    pub master_axis: u8,
    /// Coupling ratio (slave_pos = master_pos * ratio).
    #[serde(default = "default_ratio")]
    pub ratio: f64,
    /// Maximum synchronization error.
    pub max_sync_error: f64,
}

fn default_ratio() -> f64 {
    1.0
}

/// Complete per-axis configuration — loaded from `axis_NN_name.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAxisConfig {
    /// Axis identity.
    pub axis: AxisIdentity,
    /// Kinematics.
    pub kinematics: KinematicsConfig,
    /// Control (PID).
    pub control: ControlConfig,
    /// Safe stop.
    pub safe_stop: SafeStopConfig,
    /// Homing.
    pub homing: HomingConfig,
    /// Brake (optional).
    pub brake: Option<BrakeConfig>,
    /// Tailstock (optional).
    pub tailstock: Option<TailstockConfig>,
    /// Guard (optional).
    pub guard: Option<GuardConfig>,
    /// Coupling (optional).
    pub coupling: Option<CouplingConfig>,
}

// ─── FullConfig ────────────────────────────────────────────────────

/// Aggregated configuration loaded by `load_config_dir()`.
#[derive(Debug, Clone)]
pub struct FullConfig {
    /// System/program configuration (from `config.toml`).
    pub system: SystemConfig,
    /// Machine configuration (from `machine.toml`).
    pub machine: NewMachineConfig,
    /// Per-axis configurations (auto-discovered from `axis_NN_*.toml`).
    pub axes: Vec<NewAxisConfig>,
    // Note: IoConfig and IoRegistry are loaded separately through io module.
}

// ─── load_config_dir ───────────────────────────────────────────────

/// Load all configuration files from a directory.
///
/// 1. Loads `config.toml` → `SystemConfig`
/// 2. Loads `machine.toml` → `NewMachineConfig`
/// 3. Auto-discovers `axis_NN_*.toml` → sorted `Vec<NewAxisConfig>`
/// 4. Validates axis ID ↔ filename consistency, duplicate detection, bounds.
///
/// # Errors
///
/// Returns `ConfigError` for missing files, parse errors, validation failures.
pub fn load_config_dir(path: &Path) -> Result<FullConfig, ConfigError> {
    // 1. Load config.toml.
    let system_path = path.join("config.toml");
    let system: SystemConfig = load_toml_file(&system_path)?;
    system.watchdog.validate()?;

    // 2. Load machine.toml.
    let machine_path = path.join("machine.toml");
    let machine: NewMachineConfig = load_toml_file(&machine_path)?;
    validate_machine_config(&machine)?;

    // 3. Auto-discover axis files.
    let axes = discover_axis_files(path)?;

    // 4. Validate global constraints.
    if axes.len() > MAX_AXIS_COUNT {
        return Err(ConfigError::ValidationError(format!(
            "too many axes: {} > {}",
            axes.len(),
            MAX_AXIS_COUNT
        )));
    }

    Ok(FullConfig {
        system,
        machine,
        axes,
    })
}

/// Load and parse a single TOML file with strict (deny_unknown_fields) parsing.
fn load_toml_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ConfigError::FileNotFound
        } else {
            ConfigError::ParseError(format!("{}: {}", path.display(), e))
        }
    })?;

    toml::from_str(&content).map_err(|e| {
        let msg = e.to_string();
        // Detect "unknown field" errors from serde(deny_unknown_fields).
        if msg.contains("unknown field") {
            ConfigError::UnknownField(format!("{}: {}", path.display(), msg))
        } else {
            ConfigError::ParseError(format!("{}: {}", path.display(), msg))
        }
    })
}

/// Auto-discover and load axis files matching `axis_NN_*.toml`.
///
/// Glob pattern: any file starting with `axis_` and ending with `.toml`,
/// where the two characters after `axis_` are digits (NN).
///
/// Returns a sorted vector of axis configs (sorted by NN).
pub fn discover_axis_files(dir: &Path) -> Result<Vec<NewAxisConfig>, ConfigError> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        ConfigError::ParseError(format!("cannot read config directory {}: {}", dir.display(), e))
    })?;

    let mut axis_files: Vec<(u8, PathBuf, String)> = Vec::new(); // (NN, path, filename)

    for entry in entries.flatten() {
        let fname = entry.file_name();
        let fname_str = fname.to_string_lossy();

        // Match pattern: axis_NN_*.toml
        if !fname_str.starts_with("axis_") || !fname_str.ends_with(".toml") {
            continue;
        }

        let rest = &fname_str[5..]; // After "axis_"
        if rest.len() < 4 {
            // Need at least NN_X.toml → 2 digits + '_' + something + ".toml"
            continue;
        }

        // Parse NN (first two characters).
        let nn_str = &rest[..2];
        let nn: u8 = match nn_str.parse() {
            Ok(n) => n,
            Err(_) => continue, // Skip files with non-numeric NN
        };

        // Must have underscore after NN.
        if rest.as_bytes().get(2) != Some(&b'_') {
            continue;
        }

        axis_files.push((nn, entry.path(), fname_str.to_string()));
    }

    if axis_files.is_empty() {
        return Err(ConfigError::NoAxesDefined);
    }

    // Sort by NN.
    axis_files.sort_by_key(|(nn, _, _)| *nn);

    // Check for duplicate NNs.
    for w in axis_files.windows(2) {
        if w[0].0 == w[1].0 {
            return Err(ConfigError::DuplicateAxisId(w[0].0));
        }
    }

    let mut axes = Vec::with_capacity(axis_files.len());
    for (nn, path, fname) in &axis_files {
        let axis: NewAxisConfig = load_toml_file(path)?;

        // Validate NN ↔ [axis].id consistency.
        if axis.axis.id != *nn {
            return Err(ConfigError::AxisIdMismatch {
                file: fname.clone(),
                expected: *nn,
                found: axis.axis.id,
            });
        }

        // Validate numeric bounds (FR-054).
        validate_axis_bounds(&axis, &fname)?;

        axes.push(axis);
    }

    Ok(axes)
}

/// Validate machine config constraints.
fn validate_machine_config(cfg: &NewMachineConfig) -> Result<(), ConfigError> {
    let ss = &cfg.global_safety.default_safe_stop;
    if ss != "SS1" && ss != "SS2" && ss != "STO" {
        return Err(ConfigError::ValidationError(format!(
            "global_safety.default_safe_stop must be SS1, SS2, or STO; got '{ss}'"
        )));
    }
    if cfg.global_safety.safety_stop_timeout <= 0.0 {
        return Err(ConfigError::ValidationError(
            "global_safety.safety_stop_timeout must be > 0".to_string(),
        ));
    }
    if cfg.service_bypass.max_service_velocity <= 0.0 || cfg.service_bypass.max_service_velocity > MAX_VELOCITY {
        return Err(ConfigError::ValidationError(format!(
            "service_bypass.max_service_velocity={} out of range (0, {}]",
            cfg.service_bypass.max_service_velocity,
            MAX_VELOCITY
        )));
    }
    Ok(())
}

/// Validate numeric bounds for a single axis config (FR-054).
fn validate_axis_bounds(axis: &NewAxisConfig, fname: &str) -> Result<(), ConfigError> {
    let ctx = |field: &str, val: f64, min: f64, max: f64| {
        ConfigError::ValidationError(format!(
            "{fname}: {field}={val} out of range [{min}, {max}]"
        ))
    };

    let k = &axis.kinematics;
    if k.max_velocity <= 0.0 || k.max_velocity > MAX_VELOCITY {
        return Err(ctx("kinematics.max_velocity", k.max_velocity, 0.0, MAX_VELOCITY));
    }
    if let Some(ma) = k.max_acceleration {
        if ma <= 0.0 || ma > MAX_ACCELERATION {
            return Err(ctx("kinematics.max_acceleration", ma, 0.0, MAX_ACCELERATION));
        }
    }
    if k.min_pos >= k.max_pos {
        return Err(ConfigError::ValidationError(format!(
            "{fname}: kinematics.min_pos ({}) must be < max_pos ({})",
            k.min_pos, k.max_pos
        )));
    }
    if k.min_pos.abs() > MAX_POSITION_RANGE || k.max_pos.abs() > MAX_POSITION_RANGE {
        return Err(ConfigError::ValidationError(format!(
            "{fname}: position range exceeds {MAX_POSITION_RANGE}"
        )));
    }

    let c = &axis.control;
    if c.kp < MIN_KP || c.kp > MAX_KP {
        return Err(ctx("control.kp", c.kp, MIN_KP, MAX_KP));
    }
    if c.ki < MIN_KI || c.ki > MAX_KI {
        return Err(ctx("control.ki", c.ki, MIN_KI, MAX_KI));
    }
    if c.kd < MIN_KD || c.kd > MAX_KD {
        return Err(ctx("control.kd", c.kd, MIN_KD, MAX_KD));
    }
    if c.out_max <= 0.0 || c.out_max > MAX_OUT_MAX {
        return Err(ctx("control.out_max", c.out_max, 0.0, MAX_OUT_MAX));
    }
    if c.lag_error_limit <= 0.0 || c.lag_error_limit > MAX_LAG_ERROR {
        return Err(ctx("control.lag_error_limit", c.lag_error_limit, 0.0, MAX_LAG_ERROR));
    }
    let valid_policies = ["Unwanted", "Warning", "Error"];
    if !valid_policies.contains(&c.lag_policy.as_str()) {
        return Err(ConfigError::ValidationError(format!(
            "{fname}: control.lag_policy='{}' must be one of {:?}",
            c.lag_policy, valid_policies
        )));
    }

    let ss = &axis.safe_stop;
    let valid_categories = ["SS1", "SS2", "STO"];
    if !valid_categories.contains(&ss.category.as_str()) {
        return Err(ConfigError::ValidationError(format!(
            "{fname}: safe_stop.category='{}' must be one of {:?}",
            ss.category, valid_categories
        )));
    }
    if ss.max_decel_safe <= 0.0 || ss.max_decel_safe > MAX_SAFE_DECEL {
        return Err(ctx("safe_stop.max_decel_safe", ss.max_decel_safe, 0.0, MAX_SAFE_DECEL));
    }

    let h = &axis.homing;
    let valid_methods = ["HomeSensor", "TorqueLimit", "IndexPulse"];
    if !valid_methods.contains(&h.method.as_str()) {
        return Err(ConfigError::ValidationError(format!(
            "{fname}: homing.method='{}' must be one of {:?}",
            h.method, valid_methods
        )));
    }
    if h.speed <= 0.0 || h.speed > MAX_HOMING_SPEED {
        return Err(ctx("homing.speed", h.speed, 0.0, MAX_HOMING_SPEED));
    }
    if h.timeout <= 0.0 || h.timeout > MAX_HOMING_TIMEOUT {
        return Err(ctx("homing.timeout", h.timeout, 0.0, MAX_HOMING_TIMEOUT));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_log_level_default() {
        assert_eq!(default_log_level(), LogLevel::Info);
    }

    #[test]
    fn test_log_level_serialization() {
        // Test serialization within a struct (TOML requires a table)
        #[derive(Serialize)]
        struct TestWrapper {
            level: LogLevel,
        }

        let wrapper = TestWrapper {
            level: LogLevel::Trace,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("trace"));

        let wrapper = TestWrapper {
            level: LogLevel::Debug,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("debug"));

        let wrapper = TestWrapper {
            level: LogLevel::Info,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("info"));

        let wrapper = TestWrapper {
            level: LogLevel::Warn,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("warn"));

        let wrapper = TestWrapper {
            level: LogLevel::Error,
        };
        assert!(toml::to_string(&wrapper).unwrap().contains("error"));
    }

    #[test]
    fn test_log_level_deserialization() {
        // Test deserialization within a struct (TOML requires a table)
        #[derive(Debug, Deserialize, PartialEq)]
        struct TestWrapper {
            level: LogLevel,
        }

        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"trace\"")
                .unwrap()
                .level,
            LogLevel::Trace
        );
        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"debug\"")
                .unwrap()
                .level,
            LogLevel::Debug
        );
        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"info\"")
                .unwrap()
                .level,
            LogLevel::Info
        );
        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"warn\"")
                .unwrap()
                .level,
            LogLevel::Warn
        );
        assert_eq!(
            toml::from_str::<TestWrapper>("level = \"error\"")
                .unwrap()
                .level,
            LogLevel::Error
        );
    }

    #[test]
    fn test_shared_config_validation_success() {
        let config = SharedConfig {
            log_level: LogLevel::Info,
            service_name: "test-service".to_string(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_shared_config_validation_empty_service_name() {
        let config = SharedConfig {
            log_level: LogLevel::Info,
            service_name: "".to_string(),
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result, Err(ConfigError::ValidationError(_))));
    }

    #[test]
    fn test_config_loader_file_not_found() {
        #[allow(dead_code)]
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            value: String,
        }

        let result = TestConfig::load(Path::new("/nonexistent/path/config.toml"));
        assert!(matches!(result, Err(ConfigError::FileNotFound)));
    }

    #[test]
    fn test_config_loader_parse_error() {
        #[allow(dead_code)]
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            value: String,
        }

        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "invalid toml {{{{").unwrap();

        let result = TestConfig::load(file.path());
        assert!(matches!(result, Err(ConfigError::ParseError(_))));
    }

    #[test]
    fn test_config_loader_success() {
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            shared: SharedConfig,
            port: u16,
        }

        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"port = 8080

[shared]
log_level = "debug"
service_name = "test-service"
"#
        )
        .unwrap();
        file.flush().unwrap();

        let config = TestConfig::load(file.path()).unwrap();
        assert_eq!(config.shared.log_level, LogLevel::Debug);
        assert_eq!(config.shared.service_name, "test-service");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_shared_config_default_log_level() {
        #[derive(Debug, Deserialize)]
        struct TestConfig {
            shared: SharedConfig,
        }

        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"[shared]
service_name = "test-service"
"#
        )
        .unwrap();
        file.flush().unwrap();

        let config = TestConfig::load(file.path()).unwrap();
        assert_eq!(config.shared.log_level, LogLevel::Info); // Default
    }
}
