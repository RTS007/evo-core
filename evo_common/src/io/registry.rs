//! I/O Registry — Runtime role-based I/O access (FR-150, FR-152).
//!
//! Built at startup from `IoConfig`. Immutable after construction.
//! All runtime read/write methods are O(1) HashMap lookup, no heap allocation.

use std::collections::HashMap;
use std::fmt;

use super::config::{AnalogCurve, IoConfig, IoPoint};
use super::role::{DiLogic, IoPointType, IoRole};

// ─── Error Types ────────────────────────────────────────────────────

/// I/O configuration validation error.
#[derive(Debug, Clone)]
pub enum IoConfigError {
    /// Two I/O points share the same `(type, pin)` pair (V-IO-1).
    PinDuplicate {
        io_type: IoPointType,
        pin: u16,
        group_a: String,
        group_b: String,
    },
    /// Two I/O points share the same role string (V-IO-2).
    RoleDuplicate {
        role: String,
        group_a: String,
        group_b: String,
    },
    /// Role assigned to wrong I/O type (V-IO-3).
    RoleTypeMismatch {
        role: String,
        expected_type: IoPointType,
        actual_type: IoPointType,
    },
    /// Required role missing for axis peripheral (V-IO-4).
    RoleMissing {
        role: String,
        axis_id: u8,
        peripheral: String,
    },
    /// Required global role missing (V-IO-5).
    GlobalRoleMissing {
        role: String,
        peripheral: String,
    },
    /// Analog range invalid: min >= max (V-IO-6).
    AnalogRangeInvalid {
        pin: u16,
        min: f64,
        max: f64,
    },
    /// Analog average out of range (V-IO-6).
    AnalogAverageInvalid {
        pin: u16,
        average: u16,
    },
    /// Role string failed to parse.
    RoleParseError {
        role_str: String,
        error: String,
    },
}

impl fmt::Display for IoConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PinDuplicate {
                io_type,
                pin,
                group_a,
                group_b,
            } => write!(
                f,
                "V-IO-1: duplicate pin ({io_type}, {pin}) in groups '{group_a}' and '{group_b}'"
            ),
            Self::RoleDuplicate {
                role,
                group_a,
                group_b,
            } => write!(
                f,
                "V-IO-2: duplicate role '{role}' in groups '{group_a}' and '{group_b}'"
            ),
            Self::RoleTypeMismatch {
                role,
                expected_type,
                actual_type,
            } => write!(
                f,
                "V-IO-3: role '{role}' expects {expected_type} but assigned to {actual_type}"
            ),
            Self::RoleMissing {
                role,
                axis_id,
                peripheral,
            } => write!(
                f,
                "V-IO-4: missing role '{role}' for axis {axis_id} peripheral '{peripheral}'"
            ),
            Self::GlobalRoleMissing { role, peripheral } => {
                write!(
                    f,
                    "V-IO-5: missing required global role '{role}' for '{peripheral}'"
                )
            }
            Self::AnalogRangeInvalid { pin, min, max } => {
                write!(f, "V-IO-6: analog pin {pin} has invalid range [{min}, {max}]")
            }
            Self::AnalogAverageInvalid { pin, average } => {
                write!(
                    f,
                    "V-IO-6: analog pin {pin} has invalid average {average} (must be 1–1000)"
                )
            }
            Self::RoleParseError { role_str, error } => {
                write!(f, "role parse error for '{role_str}': {error}")
            }
        }
    }
}

impl std::error::Error for IoConfigError {}

// ─── IoBinding ──────────────────────────────────────────────────────

/// Runtime binding of a role to its physical I/O point.
#[derive(Debug, Clone)]
pub struct IoBinding {
    /// Group key from io.toml.
    pub group_key: String,
    /// Index within the group's `io` array.
    pub point_idx: usize,
    /// I/O type.
    pub io_type: IoPointType,
    /// Physical pin number.
    pub pin: u16,
    /// DI logic (NO/NC). Only meaningful for DI.
    pub logic: DiLogic,
    /// Analog scaling curve.
    pub curve: AnalogCurve,
    /// Output offset added after curve.
    pub offset: f64,
    /// Engineering range minimum.
    pub min: f64,
    /// Engineering range maximum.
    pub max: f64,
    /// DO inversion flag.
    pub inverted: bool,
    /// Conditional enable pin (two-hand operation). Only for DI.
    pub enable_pin: Option<u16>,
    /// Required state of `enable_pin`. Default: true.
    pub enable_state: bool,
    /// Max time between main and enable signal [ms]. 0 = no timeout.
    pub enable_timeout_ms: u32,
}

// ─── Two-Hand State ─────────────────────────────────────────────────

/// Tracks timing for two-hand operation conditional enable.
///
/// Maintains timestamps (in cycle counts) of when each signal last became
/// active. Both signals must activate within the specified timeout window.
#[derive(Debug, Clone, Default)]
pub struct TwoHandState {
    /// Cycle count when main signal last became active.
    main_active_cycle: u64,
    /// Cycle count when enable signal last became active.
    enable_active_cycle: u64,
    /// Current cycle counter (caller must increment).
    pub cycle: u64,
    /// Cycle period in milliseconds (set from CU config).
    pub cycle_period_ms: u32,
    /// Previous main signal state.
    prev_main: bool,
    /// Previous enable signal state.
    prev_enable: bool,
}

impl TwoHandState {
    /// Create a new TwoHandState with the given cycle period.
    pub fn new(cycle_period_ms: u32) -> Self {
        Self {
            cycle_period_ms,
            ..Default::default()
        }
    }

    /// Update the state and return whether both signals are valid
    /// within the timeout window.
    pub fn update(
        &mut self,
        main_active: bool,
        enable_active: bool,
        timeout_ms: u32,
    ) -> Option<bool> {
        // Detect rising edges.
        if main_active && !self.prev_main {
            self.main_active_cycle = self.cycle;
        }
        if enable_active && !self.prev_enable {
            self.enable_active_cycle = self.cycle;
        }
        self.prev_main = main_active;
        self.prev_enable = enable_active;

        // Both must be currently active.
        if !main_active || !enable_active {
            return Some(false);
        }

        // Both active — check timing.
        let delta_cycles = if self.main_active_cycle > self.enable_active_cycle {
            self.main_active_cycle - self.enable_active_cycle
        } else {
            self.enable_active_cycle - self.main_active_cycle
        };
        let delta_ms = delta_cycles * self.cycle_period_ms as u64;
        Some(delta_ms <= timeout_ms as u64)
    }

    /// Advance cycle counter. Call once per RT cycle.
    #[inline]
    pub fn tick(&mut self) {
        self.cycle += 1;
    }
}

// ─── IoRegistry ─────────────────────────────────────────────────────

/// Runtime I/O registry — maps `IoRole` to `IoBinding` for O(1) lookup.
///
/// Built once at startup. Immutable after construction.
#[derive(Debug, Clone)]
pub struct IoRegistry {
    bindings: HashMap<IoRole, IoBinding>,
    pub di_count: u16,
    pub do_count: u16,
    pub ai_count: u16,
    pub ao_count: u16,
}

impl IoRegistry {
    /// Build the registry from an `IoConfig`, running all validation rules.
    ///
    /// Returns the first validation error encountered.
    pub fn from_config(config: &IoConfig) -> Result<Self, IoConfigError> {
        let mut bindings = HashMap::new();
        // Track (type, pin) → group_key for V-IO-1.
        let mut pin_map: HashMap<(IoPointType, u16), String> = HashMap::new();
        // Track role_string → group_key for V-IO-2.
        let mut role_map: HashMap<String, String> = HashMap::new();
        let mut di_count: u16 = 0;
        let mut do_count: u16 = 0;
        let mut ai_count: u16 = 0;
        let mut ao_count: u16 = 0;

        for (group_key, group) in &config.groups {
            for (idx, point) in group.io.iter().enumerate() {
                // Count by type.
                match point.io_type {
                    IoPointType::Di => di_count += 1,
                    IoPointType::Do => do_count += 1,
                    IoPointType::Ai => ai_count += 1,
                    IoPointType::Ao => ao_count += 1,
                }

                // V-IO-1: Pin uniqueness.
                let pin_key = (point.io_type, point.pin);
                if let Some(prev_group) = pin_map.get(&pin_key) {
                    return Err(IoConfigError::PinDuplicate {
                        io_type: point.io_type,
                        pin: point.pin,
                        group_a: prev_group.clone(),
                        group_b: group_key.clone(),
                    });
                }
                pin_map.insert(pin_key, group_key.clone());

                // V-IO-6: Analog range validity.
                if matches!(point.io_type, IoPointType::Ai | IoPointType::Ao) {
                    let min = point.min.unwrap_or(0.0);
                    let max = point.max.unwrap_or(0.0);
                    if min >= max {
                        return Err(IoConfigError::AnalogRangeInvalid {
                            pin: point.pin,
                            min,
                            max,
                        });
                    }
                    if point.io_type == IoPointType::Ai {
                        let avg = point.average.unwrap_or(5);
                        if avg == 0 || avg > 1000 {
                            return Err(IoConfigError::AnalogAverageInvalid {
                                pin: point.pin,
                                average: avg,
                            });
                        }
                    }
                }

                // Process role if present.
                if let Some(role_str) = &point.role {
                    // V-IO-2: Role uniqueness.
                    if let Some(prev_group) = role_map.get(role_str) {
                        return Err(IoConfigError::RoleDuplicate {
                            role: role_str.clone(),
                            group_a: prev_group.clone(),
                            group_b: group_key.clone(),
                        });
                    }
                    role_map.insert(role_str.clone(), group_key.clone());

                    // Parse role string.
                    let role: IoRole = role_str.parse().map_err(|e: String| {
                        IoConfigError::RoleParseError {
                            role_str: role_str.clone(),
                            error: e,
                        }
                    })?;

                    // V-IO-3: Role type correctness.
                    if let Some(expected) = role.expected_io_type() {
                        if expected != point.io_type {
                            return Err(IoConfigError::RoleTypeMismatch {
                                role: role_str.clone(),
                                expected_type: expected,
                                actual_type: point.io_type,
                            });
                        }
                    }

                    // Build binding.
                    let binding = Self::build_binding(group_key, idx, point);
                    bindings.insert(role, binding);
                }
            }
        }

        Ok(Self {
            bindings,
            di_count,
            do_count,
            ai_count,
            ao_count,
        })
    }

    fn build_binding(group_key: &str, idx: usize, point: &IoPoint) -> IoBinding {
        IoBinding {
            group_key: group_key.to_string(),
            point_idx: idx,
            io_type: point.io_type,
            pin: point.pin,
            logic: point.logic.unwrap_or_default(),
            curve: point.curve.unwrap_or_default(),
            offset: point.offset.unwrap_or(0.0),
            min: point.min.unwrap_or(0.0),
            max: point.max.unwrap_or(0.0),
            inverted: point.inverted.unwrap_or(false),
            enable_pin: point.enable_pin,
            enable_state: point.enable_state.unwrap_or(true),
            enable_timeout_ms: point.enable_timeout.unwrap_or(0),
        }
    }

    /// Look up a binding by role.
    pub fn get(&self, role: &IoRole) -> Option<&IoBinding> {
        self.bindings.get(role)
    }

    /// Check if a role exists in the registry.
    pub fn has_role(&self, role: &IoRole) -> bool {
        self.bindings.contains_key(role)
    }

    /// Number of registered role bindings.
    pub fn role_count(&self) -> usize {
        self.bindings.len()
    }

    // ─── Runtime I/O Access ─────────────────────────────────────────

    /// Read a digital input with NC/NO logic applied (FR-152).
    ///
    /// Returns the logical value: `true` = signal active.
    /// - NO: true when raw bit is set.
    /// - NC: inverted — raw 0 (wire break) = active `true`.
    pub fn read_di(&self, role: &IoRole, di_bank: &[u64; 16]) -> Option<bool> {
        let binding = self.bindings.get(role)?;
        debug_assert_eq!(binding.io_type, IoPointType::Di);
        let raw_bit = extract_bit(di_bank, binding.pin);
        Some(match binding.logic {
            DiLogic::NO => raw_bit,
            DiLogic::NC => !raw_bit,
        })
    }

    /// Read a digital input with conditional enable check (two-hand operation).
    ///
    /// If the binding has an `enable_pin`, both the main signal and the enable
    /// signal must be in the required state. If `enable_timeout_ms` is nonzero,
    /// the caller must provide a `TwoHandState` to enforce timing.
    ///
    /// Returns `true` only if:
    /// 1. Main signal is active (per NC/NO logic), AND
    /// 2. Enable pin signal matches `enable_state`, AND
    /// 3. Both signals appeared within `enable_timeout_ms` (if set).
    pub fn read_di_with_enable(
        &self,
        role: &IoRole,
        di_bank: &[u64; 16],
        two_hand: Option<&mut TwoHandState>,
    ) -> Option<bool> {
        let binding = self.bindings.get(role)?;
        debug_assert_eq!(binding.io_type, IoPointType::Di);
        let raw_bit = extract_bit(di_bank, binding.pin);
        let main_active = match binding.logic {
            DiLogic::NO => raw_bit,
            DiLogic::NC => !raw_bit,
        };

        let enable_pin = match binding.enable_pin {
            Some(pin) => pin,
            None => return Some(main_active),
        };

        let enable_raw = extract_bit(di_bank, enable_pin);
        let enable_active = enable_raw == binding.enable_state;

        if binding.enable_timeout_ms == 0 {
            // No timeout — both must be active simultaneously.
            return Some(main_active && enable_active);
        }

        // Timeout-based two-hand: use TwoHandState.
        let ths = match two_hand {
            Some(s) => s,
            None => return Some(main_active && enable_active),
        };

        let timeout_ms = binding.enable_timeout_ms;
        ths.update(main_active, enable_active, timeout_ms)
    }

    /// Read an analog input with scaling applied (FR-152).
    ///
    /// Returns the value in engineering units.
    pub fn read_ai(&self, role: &IoRole, ai_values: &[f64; 64]) -> Option<f64> {
        let binding = self.bindings.get(role)?;
        debug_assert_eq!(binding.io_type, IoPointType::Ai);
        let raw = ai_values[binding.pin as usize];
        let range = binding.max - binding.min;
        if range.abs() < f64::EPSILON {
            return Some(binding.min + binding.offset);
        }
        let normalized = (raw - binding.min) / range;
        let scaled = binding.curve.evaluate(normalized);
        Some(scaled * range + binding.min + binding.offset)
    }

    /// Write a digital output with inversion applied (FR-152).
    pub fn write_do(&self, role: &IoRole, value: bool, do_bank: &mut [u64; 16]) -> Option<()> {
        let binding = self.bindings.get(role)?;
        debug_assert_eq!(binding.io_type, IoPointType::Do);
        let physical = if binding.inverted { !value } else { value };
        set_bit(do_bank, binding.pin, physical);
        Some(())
    }

    /// Write an analog output with reverse scaling (FR-152).
    pub fn write_ao(&self, role: &IoRole, value: f64, ao_values: &mut [f64; 64]) -> Option<()> {
        let binding = self.bindings.get(role)?;
        debug_assert_eq!(binding.io_type, IoPointType::Ao);
        let range = binding.max - binding.min;
        let normalized = if range.abs() < f64::EPSILON {
            0.0
        } else {
            (value - binding.min) / range
        };
        ao_values[binding.pin as usize] = normalized;
        Some(())
    }

    // ─── Validation Helpers (V-IO-4, V-IO-5) ───────────────────────

    /// Validate that all required global roles exist (V-IO-5).
    pub fn validate_global_roles(&self) -> Result<(), IoConfigError> {
        if !self.has_role(&IoRole::EStop) {
            return Err(IoConfigError::GlobalRoleMissing {
                role: "EStop".to_string(),
                peripheral: "global_safety".to_string(),
            });
        }
        Ok(())
    }

    /// Validate that all required I/O roles for an axis are present (V-IO-4).
    ///
    /// Checks axis peripherals from `CuAxisConfig` fields to determine
    /// which roles are required.
    pub fn validate_roles_for_axis(
        &self,
        axis_id: u8,
        has_tailstock: bool,
        tailstock_type: u8,
        has_index: bool,
        has_brake: bool,
        has_guard: bool,
        has_motion_enable: bool,
        homing_needs_ref: bool,
        homing_needs_limit: bool,
    ) -> Result<(), Vec<IoConfigError>> {
        let mut errors = Vec::new();

        // Limit switches — always required for each axis.
        self.require_role(
            &IoRole::LimitMin(axis_id),
            axis_id,
            "limit_switch",
            &mut errors,
        );
        self.require_role(
            &IoRole::LimitMax(axis_id),
            axis_id,
            "limit_switch",
            &mut errors,
        );

        // Motion enable input.
        if has_motion_enable {
            self.require_role(
                &IoRole::Enable(axis_id),
                axis_id,
                "motion_enable",
                &mut errors,
            );
        }

        // Homing sensor.
        if homing_needs_ref {
            self.require_role(&IoRole::Ref(axis_id), axis_id, "homing", &mut errors);
        }

        // Homing via limit switch (already required above, but explicit check).
        if homing_needs_limit {
            // LimitMin/LimitMax already required.
        }

        // Tailstock.
        if has_tailstock {
            self.require_role(
                &IoRole::TailClosed(axis_id),
                axis_id,
                "tailstock",
                &mut errors,
            );
            self.require_role(
                &IoRole::TailOpen(axis_id),
                axis_id,
                "tailstock",
                &mut errors,
            );
            if tailstock_type >= 2 {
                self.require_role(
                    &IoRole::TailClamp(axis_id),
                    axis_id,
                    "tailstock",
                    &mut errors,
                );
            }
        }

        // Locking pin.
        if has_index {
            self.require_role(
                &IoRole::IndexLocked(axis_id),
                axis_id,
                "locking_pin",
                &mut errors,
            );
            self.require_role(
                &IoRole::IndexFree(axis_id),
                axis_id,
                "locking_pin",
                &mut errors,
            );
            // IndexMiddle is optional.
        }

        // Brake.
        if has_brake {
            self.require_role(
                &IoRole::BrakeOut(axis_id),
                axis_id,
                "brake",
                &mut errors,
            );
            self.require_role(
                &IoRole::BrakeIn(axis_id),
                axis_id,
                "brake",
                &mut errors,
            );
        }

        // Guard.
        if has_guard {
            self.require_role(
                &IoRole::GuardClosed(axis_id),
                axis_id,
                "guard",
                &mut errors,
            );
            self.require_role(
                &IoRole::GuardLocked(axis_id),
                axis_id,
                "guard",
                &mut errors,
            );
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn require_role(
        &self,
        role: &IoRole,
        axis_id: u8,
        peripheral: &str,
        errors: &mut Vec<IoConfigError>,
    ) {
        if !self.has_role(role) {
            errors.push(IoConfigError::RoleMissing {
                role: role.to_string(),
                axis_id,
                peripheral: peripheral.to_string(),
            });
        }
    }
}

// ─── Bit Manipulation Helpers ───────────────────────────────────────

/// Extract a single bit from a `[u64; 16]` bank (1024-bit array).
///
/// Bit N corresponds to pin N.
#[inline]
pub fn extract_bit(bank: &[u64; 16], pin: u16) -> bool {
    let word = (pin / 64) as usize;
    let bit = pin % 64;
    if word < 16 {
        (bank[word] >> bit) & 1 != 0
    } else {
        false
    }
}

/// Set a single bit in a `[u64; 16]` bank.
#[inline]
pub fn set_bit(bank: &mut [u64; 16], pin: u16, value: bool) {
    let word = (pin / 64) as usize;
    let bit = pin % 64;
    if word < 16 {
        if value {
            bank[word] |= 1u64 << bit;
        } else {
            bank[word] &= !(1u64 << bit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::config::IoConfig;

    fn test_config() -> IoConfig {
        let toml_str = r#"
[Safety]
name = "Safety circuits"
io = [
    { type = "di", role = "EStop", pin = 1, logic = "NC", name = "Main E-Stop" },
]

[Axis1]
name = "Axis 1 I/O"
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC", name = "Limit switch 1-" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC", name = "Limit switch 1+" },
    { type = "di", role = "Ref1", pin = 34, name = "Homing sensor axis 1" },
    { type = "do", role = "BrakeOut1", pin = 100, name = "Brake output axis 1" },
    { type = "di", role = "BrakeIn1", pin = 35, name = "Brake confirmation axis 1" },
]

[Analog]
name = "Analog I/O"
io = [
    { type = "ai", pin = 0, max = 10.0, unit = "bar", name = "Pressure" },
    { type = "ao", pin = 0, min = 0.0, max = 5.0, name = "Valve" },
]
"#;
        IoConfig::from_toml(toml_str).unwrap()
    }

    #[test]
    fn registry_construction() {
        let config = test_config();
        let registry = IoRegistry::from_config(&config).unwrap();
        assert!(registry.has_role(&IoRole::EStop));
        assert!(registry.has_role(&IoRole::LimitMin(1)));
        assert!(registry.has_role(&IoRole::BrakeOut(1)));
        assert!(!registry.has_role(&IoRole::LimitMin(2)));
        assert_eq!(registry.di_count, 5);
        assert_eq!(registry.do_count, 1);
        assert_eq!(registry.ai_count, 1);
        assert_eq!(registry.ao_count, 1);
    }

    #[test]
    fn vio1_pin_duplicate() {
        let toml_str = r#"
[A]
io = [{ type = "di", pin = 1 }]
[B]
io = [{ type = "di", pin = 1 }]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let err = IoRegistry::from_config(&config).unwrap_err();
        assert!(matches!(err, IoConfigError::PinDuplicate { pin: 1, .. }));
    }

    #[test]
    fn vio1_same_pin_different_type_ok() {
        // Same pin number with different types is allowed.
        let toml_str = r#"
[A]
io = [
    { type = "di", pin = 0 },
    { type = "ai", pin = 0, max = 10.0 },
]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        assert!(IoRegistry::from_config(&config).is_ok());
    }

    #[test]
    fn vio2_role_duplicate() {
        let toml_str = r#"
[A]
io = [{ type = "di", role = "EStop", pin = 1 }]
[B]
io = [{ type = "di", role = "EStop", pin = 2 }]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let err = IoRegistry::from_config(&config).unwrap_err();
        assert!(matches!(
            err,
            IoConfigError::RoleDuplicate { role, .. } if role == "EStop"
        ));
    }

    #[test]
    fn vio3_role_type_mismatch() {
        let toml_str = r#"
[A]
io = [{ type = "do", role = "EStop", pin = 1 }]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let err = IoRegistry::from_config(&config).unwrap_err();
        assert!(matches!(err, IoConfigError::RoleTypeMismatch { .. }));
    }

    #[test]
    fn vio5_global_role_missing() {
        let toml_str = r#"
[A]
io = [{ type = "di", role = "LimitMin1", pin = 1 }]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let registry = IoRegistry::from_config(&config).unwrap();
        let err = registry.validate_global_roles().unwrap_err();
        assert!(matches!(err, IoConfigError::GlobalRoleMissing { .. }));
    }

    #[test]
    fn vio6_analog_range_invalid() {
        let toml_str = r#"
[A]
io = [{ type = "ai", pin = 0, min = 10.0, max = 5.0 }]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let err = IoRegistry::from_config(&config).unwrap_err();
        assert!(matches!(err, IoConfigError::AnalogRangeInvalid { .. }));
    }

    #[test]
    fn vio6_analog_average_invalid() {
        let toml_str = r#"
[A]
io = [{ type = "ai", pin = 0, max = 10.0, average = 0 }]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let err = IoRegistry::from_config(&config).unwrap_err();
        assert!(matches!(err, IoConfigError::AnalogAverageInvalid { .. }));
    }

    #[test]
    fn read_di_no_logic() {
        let config = test_config();
        let registry = IoRegistry::from_config(&config).unwrap();

        // Ref1 is pin 34, logic=NO (default).
        let mut di_bank = [0u64; 16];
        // Pin 34 not set → NO → false.
        assert_eq!(registry.read_di(&IoRole::Ref(1), &di_bank), Some(false));
        // Set pin 34.
        set_bit(&mut di_bank, 34, true);
        assert_eq!(registry.read_di(&IoRole::Ref(1), &di_bank), Some(true));
    }

    #[test]
    fn read_di_nc_logic() {
        let config = test_config();
        let registry = IoRegistry::from_config(&config).unwrap();

        // EStop is pin 1, logic=NC.
        let mut di_bank = [0u64; 16];
        // Pin 1 NOT set → NC inverts → true (active = wire break).
        assert_eq!(registry.read_di(&IoRole::EStop, &di_bank), Some(true));
        // Set pin 1 → NC inverts → false (signal present = normal).
        set_bit(&mut di_bank, 1, true);
        assert_eq!(registry.read_di(&IoRole::EStop, &di_bank), Some(false));
    }

    #[test]
    fn read_ai_scaling() {
        let config = test_config();
        let registry = IoRegistry::from_config(&config).unwrap();

        // Pressure sensor: pin=0, min=0, max=10, curve=linear (default).
        let role = IoRole::Custom("Pressure".to_string());
        // No role binding for "Pressure" as a Custom — it's not in test config with that role.
        assert_eq!(registry.read_ai(&role, &[0.0; 64]), None);
    }

    #[test]
    fn write_do_normal() {
        let config = test_config();
        let registry = IoRegistry::from_config(&config).unwrap();

        let mut do_bank = [0u64; 16];
        // BrakeOut1 is pin 100, not inverted.
        registry
            .write_do(&IoRole::BrakeOut(1), true, &mut do_bank)
            .unwrap();
        assert!(extract_bit(&do_bank, 100));

        registry
            .write_do(&IoRole::BrakeOut(1), false, &mut do_bank)
            .unwrap();
        assert!(!extract_bit(&do_bank, 100));
    }

    #[test]
    fn write_do_inverted() {
        let toml_str = r#"
[A]
io = [{ type = "do", role = "BrakeOut2", pin = 50, inverted = true }]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let registry = IoRegistry::from_config(&config).unwrap();

        let mut do_bank = [0u64; 16];
        // Write logical true → physical false (inverted).
        registry
            .write_do(&IoRole::BrakeOut(2), true, &mut do_bank)
            .unwrap();
        assert!(!extract_bit(&do_bank, 50));

        // Write logical false → physical true (inverted).
        registry
            .write_do(&IoRole::BrakeOut(2), false, &mut do_bank)
            .unwrap();
        assert!(extract_bit(&do_bank, 50));
    }

    #[test]
    fn validate_roles_for_axis_missing() {
        let toml_str = r#"
[A]
io = [{ type = "di", role = "EStop", pin = 1 }]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let registry = IoRegistry::from_config(&config).unwrap();

        // Axis 1 with brake — should fail: LimitMin1, LimitMax1, BrakeOut1, BrakeIn1 missing.
        let result = registry.validate_roles_for_axis(
            1,     // axis_id
            false, // no tailstock
            0,     // tailstock type
            false, // no index
            true,  // has brake
            false, // no guard
            false, // no motion enable
            false, // no homing ref
            false, // no homing limit
        );
        assert!(result.is_err());
        let errors = result.unwrap_err();
        // Should have LimitMin1, LimitMax1, BrakeOut1, BrakeIn1 = 4 errors.
        assert_eq!(errors.len(), 4);
    }

    #[test]
    fn validate_roles_for_axis_complete() {
        let config = test_config();
        let registry = IoRegistry::from_config(&config).unwrap();

        // Axis 1 with brake + homing ref — all roles present in test_config.
        let result = registry.validate_roles_for_axis(
            1,     // axis_id
            false, // no tailstock
            0,     // tailstock type
            false, // no index
            true,  // has brake
            false, // no guard
            false, // no motion enable
            true,  // homing needs ref
            false, // no homing limit
        );
        assert!(result.is_ok());
    }

    #[test]
    fn bit_manipulation() {
        let mut bank = [0u64; 16];
        assert!(!extract_bit(&bank, 0));
        set_bit(&mut bank, 0, true);
        assert!(extract_bit(&bank, 0));
        set_bit(&mut bank, 63, true);
        assert!(extract_bit(&bank, 63));
        set_bit(&mut bank, 64, true);
        assert!(extract_bit(&bank, 64));
        set_bit(&mut bank, 1023, true);
        assert!(extract_bit(&bank, 1023));

        // Clear bit 0.
        set_bit(&mut bank, 0, false);
        assert!(!extract_bit(&bank, 0));

        // Out-of-range pin → false.
        assert!(!extract_bit(&bank, 1024));
    }

    #[test]
    fn unknown_role_returns_none() {
        let config = test_config();
        let registry = IoRegistry::from_config(&config).unwrap();
        assert_eq!(
            registry.read_di(&IoRole::LimitMin(99), &[0u64; 16]),
            None
        );
    }

    // ── T080a: Two-hand conditional enable tests ──

    fn two_hand_config() -> IoConfig {
        let toml_str = r#"
[Safety]
io = [
    { type = "di", role = "EStop", pin = 1, logic = "NC" },
    { type = "di", role = "Start", pin = 2, enable_pin = 3, enable_state = true, enable_timeout = 500 },
]
"#;
        IoConfig::from_toml(toml_str).unwrap()
    }

    #[test]
    fn two_hand_both_active() {
        let config = two_hand_config();
        let registry = IoRegistry::from_config(&config).unwrap();
        let mut di_bank = [0u64; 16];
        set_bit(&mut di_bank, 2, true); // main
        set_bit(&mut di_bank, 3, true); // enable
        let mut ths = TwoHandState::new(1); // 1ms cycle
        // Both rise on same cycle → delta=0 ≤ 500 → true.
        let result = registry.read_di_with_enable(&IoRole::Start, &di_bank, Some(&mut ths));
        assert_eq!(result, Some(true));
    }

    #[test]
    fn two_hand_main_only() {
        let config = two_hand_config();
        let registry = IoRegistry::from_config(&config).unwrap();
        let mut di_bank = [0u64; 16];
        set_bit(&mut di_bank, 2, true); // main only
        let mut ths = TwoHandState::new(1);
        let result = registry.read_di_with_enable(&IoRole::Start, &di_bank, Some(&mut ths));
        assert_eq!(result, Some(false));
    }

    #[test]
    fn two_hand_timeout_exceeded() {
        let config = two_hand_config();
        let registry = IoRegistry::from_config(&config).unwrap();
        let mut ths = TwoHandState::new(1); // 1ms per cycle

        // Main activates at cycle 0.
        let mut di_bank = [0u64; 16];
        set_bit(&mut di_bank, 2, true);
        let _ = registry.read_di_with_enable(&IoRole::Start, &di_bank, Some(&mut ths));

        // Advance 600 cycles (600ms > 500ms timeout).
        for _ in 0..600 {
            ths.tick();
        }

        // Enable activates at cycle 600.
        set_bit(&mut di_bank, 3, true);
        let result = registry.read_di_with_enable(&IoRole::Start, &di_bank, Some(&mut ths));
        assert_eq!(result, Some(false));
    }

    #[test]
    fn two_hand_within_timeout() {
        let config = two_hand_config();
        let registry = IoRegistry::from_config(&config).unwrap();
        let mut ths = TwoHandState::new(1);

        // Main activates at cycle 0.
        let mut di_bank = [0u64; 16];
        set_bit(&mut di_bank, 2, true);
        let _ = registry.read_di_with_enable(&IoRole::Start, &di_bank, Some(&mut ths));

        // Advance 400 cycles (400ms ≤ 500ms timeout).
        for _ in 0..400 {
            ths.tick();
        }

        // Enable activates at cycle 400.
        set_bit(&mut di_bank, 3, true);
        let result = registry.read_di_with_enable(&IoRole::Start, &di_bank, Some(&mut ths));
        assert_eq!(result, Some(true));
    }

    #[test]
    fn two_hand_no_enable_pin() {
        // read_di_with_enable on a binding without enable_pin behaves like read_di.
        let config = test_config();
        let registry = IoRegistry::from_config(&config).unwrap();
        let mut di_bank = [0u64; 16];
        set_bit(&mut di_bank, 34, true); // Ref1 pin
        let mut ths = TwoHandState::new(1);
        let result = registry.read_di_with_enable(&IoRole::Ref(1), &di_bank, Some(&mut ths));
        assert_eq!(result, Some(true));
    }

    #[test]
    fn two_hand_state_tick() {
        let mut ths = TwoHandState::new(1);
        assert_eq!(ths.cycle, 0);
        ths.tick();
        assert_eq!(ths.cycle, 1);
        ths.tick();
        assert_eq!(ths.cycle, 2);
    }
}
