//! Configuration structures for the Control Unit (FR-141, FR-142).
//!
//! All config types use `serde::Deserialize` for TOML loading.
//! Numeric parameters have const `MIN`/`MAX` bounds (FR-156).
//! Optional fields use `#[serde(default)]` for forward-compatible deserialization (FR-157).

use serde::{Deserialize, Serialize};

use crate::consts::{
    CYCLE_TIME_US, CYCLE_TIME_US_MAX, CYCLE_TIME_US_MIN, HAL_STALE_THRESHOLD_DEFAULT,
    MANUAL_TIMEOUT_DEFAULT, MANUAL_TIMEOUT_MAX, MANUAL_TIMEOUT_MIN, MAX_AXES,
    MQT_UPDATE_INTERVAL_DEFAULT, NON_RT_STALE_THRESHOLD_DEFAULT,
};

use super::command::ServiceBypassConfig;
use super::control::UniversalControlParameters;
use super::homing::HomingConfig;
use super::safety::{
    BrakeConfig, GearAssistConfig, GuardConfig, IndexConfig, SafeStopConfig, TailstockConfig,
};
use super::state::{AxisId, CouplingConfig, SafeStopCategory};

// ─── Top-Level Config ───────────────────────────────────────────────

/// Top-level Control Unit configuration (FR-141).
///
/// Loaded from TOML at startup. Immutable after `MachineState::Starting` (FR-138a).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlUnitConfig {
    /// Target cycle time in microseconds (default: 1000 = 1ms).
    #[serde(default = "default_cycle_time_us")]
    pub cycle_time_us: u32,

    /// Maximum axis count (default: 64).
    #[serde(default = "default_max_axes")]
    pub max_axes: u8,

    /// Path to machine configuration TOML.
    pub machine_config_path: String,

    /// Path to I/O configuration TOML (FR-148).
    pub io_config_path: String,

    /// Manual → Idle timeout [s] (default: 30.0).
    #[serde(default = "default_manual_timeout")]
    pub manual_timeout: f64,

    /// HAL heartbeat staleness threshold [cycles] (default: 3).
    #[serde(default = "default_hal_stale")]
    pub hal_stale_threshold: u32,

    /// RE heartbeat staleness threshold [cycles] (default: 1000).
    #[serde(default = "default_non_rt_stale")]
    pub re_stale_threshold: u32,

    /// RPC heartbeat staleness threshold [cycles] (default: 1000).
    #[serde(default = "default_non_rt_stale")]
    pub rpc_stale_threshold: u32,

    /// Diagnostic write interval [cycles] (default: 10 = 10ms).
    #[serde(default = "default_mqt_update")]
    pub mqt_update_interval: u32,
}

fn default_cycle_time_us() -> u32 {
    CYCLE_TIME_US as u32
}
fn default_max_axes() -> u8 {
    MAX_AXES
}
fn default_manual_timeout() -> f64 {
    MANUAL_TIMEOUT_DEFAULT
}
fn default_hal_stale() -> u32 {
    HAL_STALE_THRESHOLD_DEFAULT
}
fn default_non_rt_stale() -> u32 {
    NON_RT_STALE_THRESHOLD_DEFAULT
}
fn default_mqt_update() -> u32 {
    MQT_UPDATE_INTERVAL_DEFAULT
}

impl ControlUnitConfig {
    /// Validate parameter bounds (FR-156).
    pub fn validate(&self) -> Result<(), String> {
        if self.cycle_time_us < CYCLE_TIME_US_MIN || self.cycle_time_us > CYCLE_TIME_US_MAX {
            return Err(format!(
                "cycle_time_us {} out of range [{}, {}]",
                self.cycle_time_us, CYCLE_TIME_US_MIN, CYCLE_TIME_US_MAX
            ));
        }
        if self.max_axes == 0 || self.max_axes > MAX_AXES {
            return Err(format!(
                "max_axes {} out of range [1, {}]",
                self.max_axes, MAX_AXES
            ));
        }
        if self.manual_timeout < MANUAL_TIMEOUT_MIN || self.manual_timeout > MANUAL_TIMEOUT_MAX {
            return Err(format!(
                "manual_timeout {} out of range [{}, {}]",
                self.manual_timeout, MANUAL_TIMEOUT_MIN, MANUAL_TIMEOUT_MAX
            ));
        }
        Ok(())
    }
}

// ─── Machine Config ─────────────────────────────────────────────────

/// Machine-level configuration loaded from TOML (FR-142).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuMachineConfig {
    /// Per-axis configurations.
    pub axes: Vec<CuAxisConfig>,
    /// Global safety configuration.
    #[serde(default)]
    pub global_safety: GlobalSafetyConfig,
    /// Service mode bypass configuration (FR-001a).
    #[serde(default)]
    pub service_bypass: ServiceBypassConfig,
}

impl Default for CuMachineConfig {
    fn default() -> Self {
        Self {
            axes: Vec::new(),
            global_safety: GlobalSafetyConfig::default(),
            service_bypass: ServiceBypassConfig::default(),
        }
    }
}

/// Per-axis configuration (FR-142).
///
/// Peripheral I/O fields use string role names resolved via `IoRegistry` at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuAxisConfig {
    /// Axis ID (1-based, 1..=64).
    pub axis_id: AxisId,
    /// Human-readable name (e.g., "Spindle", "X-Axis").
    pub name: String,

    /// Maximum axis velocity [user units/s].
    /// Used for 5% unreferenced limit (FR-035).
    pub max_velocity: f64,

    /// Velocity limit during SAFE_REDUCED_SPEED [user units/s] (FR-011).
    #[serde(default = "default_safe_reduced_speed")]
    pub safe_reduced_speed_limit: f64,

    /// Control engine parameters.
    #[serde(default)]
    pub control: UniversalControlParameters,

    /// Safe stop configuration.
    #[serde(default)]
    pub safe_stop: SafeStopConfig,

    /// Homing configuration.
    #[serde(default)]
    pub homing: HomingConfig,

    /// Tailstock configuration (optional).
    #[serde(default)]
    pub tailstock: Option<TailstockConfig>,

    /// Locking pin configuration (optional).
    #[serde(default)]
    pub index: Option<IndexConfig>,

    /// Brake configuration (optional).
    #[serde(default)]
    pub brake: Option<BrakeConfig>,

    /// Safety guard configuration (optional).
    #[serde(default)]
    pub guard: Option<GuardConfig>,

    /// Coupling configuration (optional).
    #[serde(default)]
    pub coupling: Option<CouplingConfig>,

    /// Gear assist configuration for gear shifting (optional, FR-062).
    #[serde(default)]
    pub gear_assist: Option<GearAssistConfig>,

    /// DI role for motion enable signal (FR-021).
    #[serde(default)]
    pub motion_enable_input: Option<String>,

    /// Software position limits.
    #[serde(default)]
    pub min_pos: f64,
    #[serde(default = "default_max_pos")]
    pub max_pos: f64,

    /// In-position window for soft limit tolerance [mm] (FR-035).
    #[serde(default = "default_in_position_window")]
    pub in_position_window: f64,

    /// Per-axis loading config flags (FR-073).
    #[serde(default)]
    pub loading_blocked: bool,
    #[serde(default)]
    pub loading_manual: bool,
}

fn default_safe_reduced_speed() -> f64 {
    50.0
}
fn default_max_pos() -> f64 {
    f64::MAX
}
fn default_in_position_window() -> f64 {
    0.1
}

impl CuAxisConfig {
    /// Convert from the new unified `NewAxisConfig` (from `load_config_dir`).
    ///
    /// Maps the per-axis identity, kinematics, and control fields into CU-specific
    /// configuration. Optional peripherals (tailstock, brake, guard) are not mapped
    /// because they use a different config schema in the unified layout.
    pub fn from_new_axis_config(ax: &crate::config::NewAxisConfig) -> Self {
        Self {
            axis_id: ax.axis.id,
            name: ax.axis.name.clone(),
            max_velocity: ax.kinematics.max_velocity,
            safe_reduced_speed_limit: ax.kinematics.safe_reduced_speed_limit,
            control: UniversalControlParameters {
                kp: ax.control.kp,
                ki: ax.control.ki,
                kd: ax.control.kd,
                ..Default::default()
            },
            safe_stop: SafeStopConfig::default(),
            homing: HomingConfig::default(),
            tailstock: None,
            index: None,
            brake: None,
            guard: None,
            coupling: None,
            gear_assist: None,
            motion_enable_input: None,
            min_pos: ax.kinematics.min_pos,
            max_pos: ax.kinematics.max_pos,
            in_position_window: ax.kinematics.in_position_window,
            loading_blocked: false,
            loading_manual: false,
        }
    }
}

/// Global safety configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GlobalSafetyConfig {
    /// Fallback safe stop category if axis has no explicit config (default: SS1).
    #[serde(default)]
    pub default_safe_stop: SafeStopCategory,
    /// Maximum time for all axes to complete safe stop [s] (default: 5.0).
    #[serde(default = "default_safety_stop_timeout")]
    pub safety_stop_timeout: f64,
    /// Require manual authorization after reset (FR-122, default: true).
    #[serde(default = "default_recovery_auth")]
    pub recovery_authorization_required: bool,
}

fn default_safety_stop_timeout() -> f64 {
    5.0
}
fn default_recovery_auth() -> bool {
    true
}

impl Default for GlobalSafetyConfig {
    fn default() -> Self {
        Self {
            default_safe_stop: SafeStopCategory::SS1,
            safety_stop_timeout: 5.0,
            recovery_authorization_required: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_unit_config_validate() {
        let valid = ControlUnitConfig {
            cycle_time_us: 1000,
            max_axes: 8,
            machine_config_path: "machine.toml".to_string(),
            io_config_path: "io.toml".to_string(),
            manual_timeout: 30.0,
            hal_stale_threshold: 3,
            re_stale_threshold: 1000,
            rpc_stale_threshold: 1000,
            mqt_update_interval: 10,
        };
        assert!(valid.validate().is_ok());

        let bad_cycle = ControlUnitConfig {
            cycle_time_us: 50,
            ..valid.clone()
        };
        assert!(bad_cycle.validate().is_err());

        let bad_axes = ControlUnitConfig {
            max_axes: 0,
            ..valid.clone()
        };
        assert!(bad_axes.validate().is_err());

        let bad_timeout = ControlUnitConfig {
            manual_timeout: 0.1,
            ..valid.clone()
        };
        assert!(bad_timeout.validate().is_err());
    }
}
