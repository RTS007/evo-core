//! TOML configuration loader with validation (FR-141, FR-142, FR-148).
//!
//! Loads `ControlUnitConfig`, `CuMachineConfig`, and `IoConfig` from TOML files.
//! Validates: parameter bounds (FR-156), axis ID uniqueness, coupling graph
//! acyclicity, required peripheral I/O roles, and global role completeness.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use evo_common::control_unit::config::{ControlUnitConfig, CuAxisConfig, CuMachineConfig};
use evo_common::io::config::IoConfig;
use evo_common::io::registry::{IoConfigError, IoRegistry};

// ─── Error Type ─────────────────────────────────────────────────────

/// Configuration loading/validation error.
#[derive(Debug)]
pub enum ConfigError {
    /// File I/O error.
    IoError(String),
    /// TOML parse error.
    ParseError(String),
    /// Parameter validation error.
    ValidationError(String),
    /// I/O config validation error.
    IoConfigError(IoConfigError),
    /// Multiple I/O config errors.
    IoConfigErrors(Vec<IoConfigError>),
    /// Hot-reload denied (not in SAFETY_STOP state).
    ReloadDenied(String),
    /// Hot-reload validation failed (shadow config rejected).
    ReloadValidationFailed(String),
    /// Hot-reload scope violation (non-reloadable field changed).
    ReloadScopeViolation(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "config I/O error: {e}"),
            Self::ParseError(e) => write!(f, "config parse error: {e}"),
            Self::ValidationError(e) => write!(f, "config validation: {e}"),
            Self::IoConfigError(e) => write!(f, "I/O config: {e}"),
            Self::IoConfigErrors(errs) => {
                write!(f, "I/O config: {} errors:", errs.len())?;
                for e in errs {
                    write!(f, "\n  - {e}")?;
                }
                Ok(())
            }
            Self::ReloadDenied(reason) => write!(f, "ERR_RELOAD_DENIED: {reason}"),
            Self::ReloadValidationFailed(detail) => {
                write!(f, "ERR_RELOAD_VALIDATION_FAILED: {detail}")
            }
            Self::ReloadScopeViolation(detail) => {
                write!(f, "ERR_RELOAD_SCOPE_VIOLATION: {detail}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

// ─── Loaded Config Bundle ───────────────────────────────────────────

/// Complete validated configuration bundle, ready for runtime use.
#[derive(Debug)]
pub struct LoadedConfig {
    pub cu_config: ControlUnitConfig,
    pub machine: CuMachineConfig,
    pub io_registry: IoRegistry,
}

// ─── Loading Functions ──────────────────────────────────────────────

/// Load and validate the Control Unit configuration from TOML files.
///
/// 1. Parse `cu_config_path` → `ControlUnitConfig`
/// 2. Parse `machine_config_path` (from CU config) → `CuMachineConfig`
/// 3. Parse `io_config_path` (from CU config) → `IoConfig` → `IoRegistry`
/// 4. Run all validation rules.
pub fn load_config(cu_config_path: &Path) -> Result<LoadedConfig, ConfigError> {
    let cu_toml = std::fs::read_to_string(cu_config_path).map_err(|e| {
        ConfigError::IoError(format!(
            "failed to read {}: {e}",
            cu_config_path.display()
        ))
    })?;
    let cu_config: ControlUnitConfig =
        toml::from_str(&cu_toml).map_err(|e| ConfigError::ParseError(format!("CU config: {e}")))?;

    cu_config
        .validate()
        .map_err(ConfigError::ValidationError)?;

    let machine_path = Path::new(&cu_config.machine_config_path);
    let machine_toml = std::fs::read_to_string(machine_path).map_err(|e| {
        ConfigError::IoError(format!(
            "failed to read {}: {e}",
            machine_path.display()
        ))
    })?;
    let machine: CuMachineConfig = toml::from_str(&machine_toml)
        .map_err(|e| ConfigError::ParseError(format!("machine config: {e}")))?;

    let io_path = Path::new(&cu_config.io_config_path);
    let io_toml = std::fs::read_to_string(io_path).map_err(|e| {
        ConfigError::IoError(format!("failed to read {}: {e}", io_path.display()))
    })?;
    let io_config = IoConfig::from_toml(&io_toml)
        .map_err(|e| ConfigError::ParseError(format!("I/O config: {e}")))?;
    let io_registry =
        IoRegistry::from_config(&io_config).map_err(ConfigError::IoConfigError)?;

    validate_machine_config(&machine)?;
    validate_io_completeness(&machine, &io_registry)?;

    Ok(LoadedConfig {
        cu_config,
        machine,
        io_registry,
    })
}

/// Load config from TOML strings (for testing).
pub fn load_config_from_strings(
    cu_toml: &str,
    machine_toml: &str,
    io_toml: &str,
) -> Result<LoadedConfig, ConfigError> {
    let cu_config: ControlUnitConfig =
        toml::from_str(cu_toml).map_err(|e| ConfigError::ParseError(format!("CU config: {e}")))?;
    cu_config
        .validate()
        .map_err(ConfigError::ValidationError)?;

    let machine: CuMachineConfig = toml::from_str(machine_toml)
        .map_err(|e| ConfigError::ParseError(format!("machine config: {e}")))?;

    let io_config = IoConfig::from_toml(io_toml)
        .map_err(|e| ConfigError::ParseError(format!("I/O config: {e}")))?;
    let io_registry =
        IoRegistry::from_config(&io_config).map_err(ConfigError::IoConfigError)?;

    validate_machine_config(&machine)?;
    validate_io_completeness(&machine, &io_registry)?;

    Ok(LoadedConfig {
        cu_config,
        machine,
        io_registry,
    })
}

// ─── Machine Config Validation ──────────────────────────────────────

pub fn validate_machine_config(machine: &CuMachineConfig) -> Result<(), ConfigError> {
    validate_axis_id_uniqueness(&machine.axes)?;
    validate_coupling_graph(&machine.axes)?;
    Ok(())
}

/// Check that all axis IDs are unique and in range 1..=64.
fn validate_axis_id_uniqueness(axes: &[CuAxisConfig]) -> Result<(), ConfigError> {
    let mut seen = HashSet::new();
    for ax in axes {
        let id = ax.axis_id;
        if id == 0 || id > 64 {
            return Err(ConfigError::ValidationError(format!(
                "axis_id {} out of range [1, 64]",
                id
            )));
        }
        if !seen.insert(id) {
            return Err(ConfigError::ValidationError(format!(
                "duplicate axis_id {}",
                id
            )));
        }
    }
    Ok(())
}

/// Check that the coupling graph is acyclic (no circular master-slave chains).
fn validate_coupling_graph(axes: &[CuAxisConfig]) -> Result<(), ConfigError> {
    let mut master_to_slaves: HashMap<u8, Vec<u8>> = HashMap::new();
    let mut axis_set: HashSet<u8> = HashSet::new();

    for ax in axes {
        axis_set.insert(ax.axis_id);
        if let Some(ref coupling) = ax.coupling {
            if let Some(master_id) = coupling.master_axis {
                master_to_slaves
                    .entry(master_id)
                    .or_default()
                    .push(ax.axis_id);
            }
        }
    }

    let mut visited = HashSet::new();
    let mut on_stack = HashSet::new();

    for &ax_id in &axis_set {
        if !visited.contains(&ax_id) {
            if has_cycle(ax_id, &master_to_slaves, &mut visited, &mut on_stack) {
                return Err(ConfigError::ValidationError(
                    "coupling graph contains a cycle".to_string(),
                ));
            }
        }
    }

    for ax in axes {
        if let Some(ref coupling) = ax.coupling {
            if let Some(master_id) = coupling.master_axis {
                if !axis_set.contains(&master_id) {
                    return Err(ConfigError::ValidationError(format!(
                        "axis {} references non-existent master {}",
                        ax.axis_id, master_id
                    )));
                }
            }
        }
    }

    Ok(())
}

fn has_cycle(
    node: u8,
    graph: &HashMap<u8, Vec<u8>>,
    visited: &mut HashSet<u8>,
    on_stack: &mut HashSet<u8>,
) -> bool {
    visited.insert(node);
    on_stack.insert(node);

    if let Some(children) = graph.get(&node) {
        for &child in children {
            if !visited.contains(&child) {
                if has_cycle(child, graph, visited, on_stack) {
                    return true;
                }
            } else if on_stack.contains(&child) {
                return true;
            }
        }
    }

    on_stack.remove(&node);
    false
}

// ─── I/O Completeness Validation ────────────────────────────────────

pub fn validate_io_completeness(
    machine: &CuMachineConfig,
    io_registry: &IoRegistry,
) -> Result<(), ConfigError> {
    io_registry
        .validate_global_roles()
        .map_err(ConfigError::IoConfigError)?;

    for ax in &machine.axes {
        let id = ax.axis_id;
        let has_tailstock = ax.tailstock.is_some();
        let tailstock_type = ax
            .tailstock
            .as_ref()
            .map_or(0, |t| t.tailstock_type as u8);
        let has_index = ax.index.is_some();
        let has_brake = ax.brake.is_some();
        let has_guard = ax.guard.is_some();
        let has_motion_enable = ax.motion_enable_input.is_some();

        let homing_needs_ref = matches!(
            ax.homing.method,
            evo_common::control_unit::homing::HomingMethod::HomeSensor
                | evo_common::control_unit::homing::HomingMethod::IndexPulse
        );
        let homing_needs_limit = matches!(
            ax.homing.method,
            evo_common::control_unit::homing::HomingMethod::LimitSwitch
        );

        let result = io_registry.validate_roles_for_axis(
            id,
            has_tailstock,
            tailstock_type,
            has_index,
            has_brake,
            has_guard,
            has_motion_enable,
            homing_needs_ref,
            homing_needs_limit,
        );

        if let Err(errors) = result {
            return Err(ConfigError::IoConfigErrors(errors));
        }
    }

    Ok(())
}

// ─── Hot-Reload: Shadow Config (T098/T099, FR-144–FR-147) ───────────

/// Result of a successful hot-reload parse and validation.
#[derive(Debug)]
pub struct ShadowConfig {
    /// Validated new machine configuration.
    pub machine: CuMachineConfig,
    /// Validated new I/O registry.
    pub io_registry: IoRegistry,
}

/// Parse and validate a shadow configuration from TOML strings (T098 / FR-146).
///
/// This is called during SAFETY_STOP when the RT loop is halted.
///
/// Steps:
/// 1. Parse new machine config + I/O config
/// 2. Full validation (axis ID uniqueness, coupling graph, parameter bounds)
/// 3. Reloadable-scope check against the active config
///
/// If any step fails, the shadow config is discarded and an error is returned.
pub fn parse_shadow_config(
    machine_toml: &str,
    io_toml: &str,
    active: &LoadedConfig,
) -> Result<ShadowConfig, ConfigError> {
    // 1. Parse
    let shadow_machine: CuMachineConfig = toml::from_str(machine_toml)
        .map_err(|e| ConfigError::ReloadValidationFailed(format!("machine parse: {e}")))?;
    let io_config = IoConfig::from_toml(io_toml)
        .map_err(|e| ConfigError::ReloadValidationFailed(format!("I/O parse: {e}")))?;
    let shadow_registry = IoRegistry::from_config(&io_config)
        .map_err(|e| ConfigError::ReloadValidationFailed(format!("I/O registry: {e}")))?;

    // 2. Full validation (same rules as startup)
    validate_machine_config(&shadow_machine)
        .map_err(|e| ConfigError::ReloadValidationFailed(format!("validation: {e}")))?;
    validate_io_completeness(&shadow_machine, &shadow_registry)
        .map_err(|e| ConfigError::ReloadValidationFailed(format!("I/O completeness: {e}")))?;

    // 3. Reloadable-scope check (FR-145)
    validate_reload_scope(&active.machine, &shadow_machine)?;

    Ok(ShadowConfig {
        machine: shadow_machine,
        io_registry: shadow_registry,
    })
}

/// Validate that the shadow config only changes reloadable fields (FR-145).
///
/// **Reloadable**: PID gains, lag_error_limit, lag_policy, safe_stop timings,
/// peripheral timeouts, homing parameters, feedforward/DOB/filter gains,
/// guard secure_speed.
///
/// **NOT reloadable** (require full restart):
/// - Axis count
/// - Axis ID assignments
/// - Coupling topology (master/slave relationships)
pub fn validate_reload_scope(
    active: &CuMachineConfig,
    shadow: &CuMachineConfig,
) -> Result<(), ConfigError> {
    // Axis count must not change
    if active.axes.len() != shadow.axes.len() {
        return Err(ConfigError::ReloadScopeViolation(format!(
            "axis count changed: {} → {} (requires restart)",
            active.axes.len(),
            shadow.axes.len(),
        )));
    }

    // Axis IDs must be identical in same order
    for (i, (a, s)) in active.axes.iter().zip(shadow.axes.iter()).enumerate() {
        if a.axis_id != s.axis_id {
            return Err(ConfigError::ReloadScopeViolation(format!(
                "axis[{i}] ID changed: {} → {} (requires restart)",
                a.axis_id, s.axis_id,
            )));
        }

        // Coupling topology must not change
        let active_master = a.coupling.as_ref().and_then(|c| c.master_axis);
        let shadow_master = s.coupling.as_ref().and_then(|c| c.master_axis);
        if active_master != shadow_master {
            return Err(ConfigError::ReloadScopeViolation(format!(
                "axis {} coupling master changed: {:?} → {:?} (requires restart)",
                a.axis_id, active_master, shadow_master,
            )));
        }
        let active_slaves = a.coupling.as_ref().map(|c| &c.slave_axes);
        let shadow_slaves = s.coupling.as_ref().map(|c| &c.slave_axes);
        if active_slaves != shadow_slaves {
            return Err(ConfigError::ReloadScopeViolation(format!(
                "axis {} coupling slaves changed (requires restart)",
                a.axis_id,
            )));
        }
    }

    Ok(())
}

/// Atomic config swap result (T099 / FR-146).
#[derive(Debug, PartialEq, Eq)]
pub enum ReloadResult {
    /// Config swapped successfully.
    Success,
    /// Validation failed — active config unchanged.
    ValidationFailed(String),
    /// Reload denied (not in SAFETY_STOP).
    Denied(String),
}

/// Attempt to perform an atomic config swap (T099 / FR-146).
///
/// This function:
/// 1. Parses and validates the shadow config
/// 2. Checks reloadable scope
/// 3. Swaps `active.machine` and `active.io_registry` with shadow values
///
/// On failure, `active` is unchanged (rollback = no-op since swap hasn't happened).
///
/// Returns `ReloadResult::Success` on success, or details on failure.
pub fn atomic_config_swap(
    active: &mut LoadedConfig,
    machine_toml: &str,
    io_toml: &str,
) -> ReloadResult {
    let shadow = match parse_shadow_config(machine_toml, io_toml, active) {
        Ok(s) => s,
        Err(e) => {
            return ReloadResult::ValidationFailed(format!("{e}"));
        }
    };

    // Atomic swap: replace machine config and I/O registry
    active.machine = shadow.machine;
    active.io_registry = shadow.io_registry;

    ReloadResult::Success
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_cu_toml() -> &'static str {
        r#"
cycle_time_us = 1000
max_axes = 8
machine_config_path = "machine.toml"
io_config_path = "io.toml"
"#
    }

    fn minimal_machine_toml() -> &'static str {
        r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 500.0
"#
    }

    fn minimal_io_toml() -> &'static str {
        r#"
[Safety]
io = [
    { type = "di", role = "EStop", pin = 1, logic = "NC" },
]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
"#
    }

    #[test]
    fn load_valid_config() {
        let loaded = load_config_from_strings(
            minimal_cu_toml(),
            minimal_machine_toml(),
            minimal_io_toml(),
        )
        .unwrap();
        assert_eq!(loaded.cu_config.cycle_time_us, 1000);
        assert_eq!(loaded.machine.axes.len(), 1);
        assert!(loaded.io_registry.has_role(&evo_common::io::role::IoRole::EStop));
    }

    #[test]
    fn reject_duplicate_axis_id() {
        let machine_toml = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 500.0

[[axes]]
axis_id = 1
name = "Y-Axis"
max_velocity = 500.0
"#;
        let err = load_config_from_strings(minimal_cu_toml(), machine_toml, minimal_io_toml());
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("duplicate axis_id"), "got: {msg}");
    }

    #[test]
    fn reject_axis_id_out_of_range() {
        let machine_toml = r#"
[[axes]]
axis_id = 0
name = "Bad"
max_velocity = 500.0
"#;
        let err = load_config_from_strings(minimal_cu_toml(), machine_toml, minimal_io_toml());
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("out of range"), "got: {msg}");
    }

    #[test]
    fn reject_cyclic_coupling() {
        let machine_toml = r#"
[[axes]]
axis_id = 1
name = "A"
max_velocity = 500.0
[axes.coupling]
master_axis = 2
slave_axes = []
coupling_ratio = 1.0

[[axes]]
axis_id = 2
name = "B"
max_velocity = 500.0
[axes.coupling]
master_axis = 1
slave_axes = []
coupling_ratio = 1.0
"#;
        let io_toml = r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
[Axis2]
io = [
    { type = "di", role = "LimitMin2", pin = 32, logic = "NC" },
    { type = "di", role = "LimitMax2", pin = 33, logic = "NC" },
]
"#;
        let err = load_config_from_strings(minimal_cu_toml(), machine_toml, io_toml);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("cycle"), "got: {msg}");
    }

    #[test]
    fn reject_missing_io_roles() {
        let machine_toml = r#"
[[axes]]
axis_id = 1
name = "X"
max_velocity = 500.0

[axes.brake]
do_brake = "BrakeOut1"
di_released = "BrakeIn1"
release_timeout = 2.0
engage_timeout = 2.0
"#;
        let err = load_config_from_strings(minimal_cu_toml(), machine_toml, minimal_io_toml());
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("BrakeOut1") || msg.contains("BrakeIn1"), "got: {msg}");
    }

    #[test]
    fn reject_missing_estop() {
        let io_toml = r#"
[Axes]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
"#;
        let err = load_config_from_strings(minimal_cu_toml(), minimal_machine_toml(), io_toml);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("EStop"), "got: {msg}");
    }

    #[test]
    fn reject_invalid_cu_params() {
        let cu_toml = r#"
cycle_time_us = 50
max_axes = 8
machine_config_path = "machine.toml"
io_config_path = "io.toml"
"#;
        let err = load_config_from_strings(cu_toml, minimal_machine_toml(), minimal_io_toml());
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("cycle_time_us"), "got: {msg}");
    }

    #[test]
    fn valid_coupling_chain() {
        let machine_toml = r#"
[[axes]]
axis_id = 1
name = "Master"
max_velocity = 500.0

[[axes]]
axis_id = 2
name = "Slave"
max_velocity = 500.0
[axes.coupling]
master_axis = 1
slave_axes = []
coupling_ratio = 2.0
"#;
        let io_toml = r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
[Axis2]
io = [
    { type = "di", role = "LimitMin2", pin = 32, logic = "NC" },
    { type = "di", role = "LimitMax2", pin = 33, logic = "NC" },
]
"#;
        let loaded =
            load_config_from_strings(minimal_cu_toml(), machine_toml, io_toml).unwrap();
        assert_eq!(loaded.machine.axes.len(), 2);
    }

    // ── T035c: Additional config edge-case tests ──

    #[test]
    fn reject_max_axes_zero() {
        let cu_toml = r#"
cycle_time_us = 1000
max_axes = 0
machine_config_path = "machine.toml"
io_config_path = "io.toml"
"#;
        let err = load_config_from_strings(cu_toml, minimal_machine_toml(), minimal_io_toml());
        assert!(err.is_err());
    }

    #[test]
    fn reject_max_axes_over_limit() {
        let cu_toml = r#"
cycle_time_us = 1000
max_axes = 255
machine_config_path = "machine.toml"
io_config_path = "io.toml"
"#;
        let err = load_config_from_strings(cu_toml, minimal_machine_toml(), minimal_io_toml());
        assert!(err.is_err());
    }

    #[test]
    fn reject_axis_id_exceeds_max_axes() {
        // axis_id=5 with max_axes=2 — the axis_id is valid (1..=64) but
        // the IO config doesn't declare roles for axis 5 (V-IO-4 violation).
        let cu_toml = r#"
cycle_time_us = 1000
max_axes = 2
machine_config_path = "machine.toml"
io_config_path = "io.toml"
"#;
        let machine_toml = r#"
[[axes]]
axis_id = 5
name = "Too-High"
max_velocity = 500.0
"#;
        let err = load_config_from_strings(cu_toml, machine_toml, minimal_io_toml());
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("LimitMin5") || msg.contains("V-IO-4"), "got: {msg}");
    }

    #[test]
    fn reject_malformed_cu_toml() {
        let bad_toml = "this is not valid toml @@@@";
        let err = load_config_from_strings(bad_toml, minimal_machine_toml(), minimal_io_toml());
        assert!(err.is_err());
    }

    #[test]
    fn reject_malformed_machine_toml() {
        let bad_toml = "this is not valid toml @@@@";
        let err = load_config_from_strings(minimal_cu_toml(), bad_toml, minimal_io_toml());
        assert!(err.is_err());
    }

    #[test]
    fn reject_malformed_io_toml() {
        let bad_toml = "this is not valid toml @@@@";
        let err = load_config_from_strings(minimal_cu_toml(), minimal_machine_toml(), bad_toml);
        assert!(err.is_err());
    }

    #[test]
    fn multi_axis_valid() {
        let machine_toml = r#"
[[axes]]
axis_id = 1
name = "X"
max_velocity = 500.0

[[axes]]
axis_id = 2
name = "Y"
max_velocity = 500.0

[[axes]]
axis_id = 3
name = "Z"
max_velocity = 500.0
"#;
        let io_toml = r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
[Axis2]
io = [
    { type = "di", role = "LimitMin2", pin = 32, logic = "NC" },
    { type = "di", role = "LimitMax2", pin = 33, logic = "NC" },
]
[Axis3]
io = [
    { type = "di", role = "LimitMin3", pin = 34, logic = "NC" },
    { type = "di", role = "LimitMax3", pin = 35, logic = "NC" },
]
"#;
        let loaded = load_config_from_strings(minimal_cu_toml(), machine_toml, io_toml).unwrap();
        assert_eq!(loaded.machine.axes.len(), 3);
    }

    #[test]
    fn config_error_display() {
        let err = ConfigError::ValidationError("bad value".to_string());
        assert!(err.to_string().contains("bad value"));
    }

    #[test]
    fn reject_coupling_to_nonexistent_axis() {
        let machine_toml = r#"
[[axes]]
axis_id = 1
name = "A"
max_velocity = 500.0
[axes.coupling]
master_axis = 99
slave_axes = []
coupling_ratio = 1.0
"#;
        let err = load_config_from_strings(minimal_cu_toml(), machine_toml, minimal_io_toml());
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("non-existent") || msg.contains("99"), "got: {msg}");
    }

    // ── T098/T099: Hot-reload tests ──

    #[test]
    fn shadow_config_valid_reload() {
        let loaded = load_config_from_strings(
            minimal_cu_toml(),
            minimal_machine_toml(),
            minimal_io_toml(),
        )
        .unwrap();

        // Change PID gains (reloadable)
        let updated_machine = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 600.0
[axes.control]
kp = 200.0
"#;
        let shadow = parse_shadow_config(updated_machine, minimal_io_toml(), &loaded);
        assert!(shadow.is_ok(), "valid reload should succeed: {:?}", shadow.err());
        let s = shadow.unwrap();
        assert_eq!(s.machine.axes[0].max_velocity, 600.0);
        assert_eq!(s.machine.axes[0].control.kp, 200.0);
    }

    #[test]
    fn shadow_config_reject_axis_count_change() {
        let loaded = load_config_from_strings(
            minimal_cu_toml(),
            minimal_machine_toml(),
            minimal_io_toml(),
        )
        .unwrap();

        // Try to add an axis (non-reloadable)
        let two_axes = r#"
[[axes]]
axis_id = 1
name = "X"
max_velocity = 500.0

[[axes]]
axis_id = 2
name = "Y"
max_velocity = 500.0
"#;
        let io2 = r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
[Axis2]
io = [
    { type = "di", role = "LimitMin2", pin = 32, logic = "NC" },
    { type = "di", role = "LimitMax2", pin = 33, logic = "NC" },
]
"#;
        let err = parse_shadow_config(two_axes, io2, &loaded);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("axis count changed"), "got: {msg}");
    }

    #[test]
    fn shadow_config_reject_axis_id_change() {
        let loaded = load_config_from_strings(
            minimal_cu_toml(),
            minimal_machine_toml(),
            minimal_io_toml(),
        )
        .unwrap();

        // Change axis_id (non-reloadable)
        let changed_id = r#"
[[axes]]
axis_id = 2
name = "X-Axis"
max_velocity = 500.0
"#;
        let io2 = r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis2]
io = [
    { type = "di", role = "LimitMin2", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax2", pin = 31, logic = "NC" },
]
"#;
        let err = parse_shadow_config(changed_id, io2, &loaded);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("ID changed"), "got: {msg}");
    }

    #[test]
    fn shadow_config_reject_coupling_topology_change() {
        let machine1 = r#"
[[axes]]
axis_id = 1
name = "Master"
max_velocity = 500.0

[[axes]]
axis_id = 2
name = "Slave"
max_velocity = 500.0
[axes.coupling]
master_axis = 1
slave_axes = []
coupling_ratio = 2.0
"#;
        let io2 = r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
[Axis2]
io = [
    { type = "di", role = "LimitMin2", pin = 32, logic = "NC" },
    { type = "di", role = "LimitMax2", pin = 33, logic = "NC" },
]
"#;
        let loaded = load_config_from_strings(minimal_cu_toml(), machine1, io2).unwrap();

        // Remove coupling (topology change)
        let no_coupling = r#"
[[axes]]
axis_id = 1
name = "Master"
max_velocity = 500.0

[[axes]]
axis_id = 2
name = "Slave"
max_velocity = 500.0
"#;
        let err = parse_shadow_config(no_coupling, io2, &loaded);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("coupling") && msg.contains("changed"), "got: {msg}");
    }

    #[test]
    fn shadow_config_reject_invalid_toml() {
        let loaded = load_config_from_strings(
            minimal_cu_toml(),
            minimal_machine_toml(),
            minimal_io_toml(),
        )
        .unwrap();

        let err = parse_shadow_config("invalid toml @@@", minimal_io_toml(), &loaded);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("RELOAD_VALIDATION_FAILED"), "got: {msg}");
    }

    #[test]
    fn atomic_swap_success() {
        let mut loaded = load_config_from_strings(
            minimal_cu_toml(),
            minimal_machine_toml(),
            minimal_io_toml(),
        )
        .unwrap();

        assert_eq!(loaded.machine.axes[0].max_velocity, 500.0);

        let updated = r#"
[[axes]]
axis_id = 1
name = "X-Axis"
max_velocity = 750.0
"#;
        let result = atomic_config_swap(&mut loaded, updated, minimal_io_toml());
        assert_eq!(result, ReloadResult::Success);
        assert_eq!(loaded.machine.axes[0].max_velocity, 750.0);
    }

    #[test]
    fn atomic_swap_rollback_on_failure() {
        let mut loaded = load_config_from_strings(
            minimal_cu_toml(),
            minimal_machine_toml(),
            minimal_io_toml(),
        )
        .unwrap();

        let original_velocity = loaded.machine.axes[0].max_velocity;

        // Invalid config (axis count change)
        let bad = r#"
[[axes]]
axis_id = 1
name = "X"
max_velocity = 500.0
[[axes]]
axis_id = 2
name = "Y"
max_velocity = 500.0
"#;
        let io2 = r#"
[Safety]
io = [{ type = "di", role = "EStop", pin = 1, logic = "NC" }]
[Axis1]
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC" },
]
[Axis2]
io = [
    { type = "di", role = "LimitMin2", pin = 32, logic = "NC" },
    { type = "di", role = "LimitMax2", pin = 33, logic = "NC" },
]
"#;
        let result = atomic_config_swap(&mut loaded, bad, io2);
        assert!(matches!(result, ReloadResult::ValidationFailed(_)));
        // Active config must be unchanged
        assert_eq!(loaded.machine.axes[0].max_velocity, original_velocity);
        assert_eq!(loaded.machine.axes.len(), 1);
    }

    #[test]
    fn reload_result_display() {
        let err = ConfigError::ReloadDenied("not in E-STOP".to_string());
        assert!(err.to_string().contains("RELOAD_DENIED"));
        let err = ConfigError::ReloadValidationFailed("bad param".to_string());
        assert!(err.to_string().contains("RELOAD_VALIDATION_FAILED"));
        let err = ConfigError::ReloadScopeViolation("axis count".to_string());
        assert!(err.to_string().contains("RELOAD_SCOPE_VIOLATION"));
    }
}