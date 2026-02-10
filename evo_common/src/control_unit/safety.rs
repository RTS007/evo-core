//! Safety types for the Control Unit (FR-080, FR-082-FR-085).
//!
//! Defines `AxisSafetyState` (8 boolean flags), `SafeStopConfig`,
//! and all safety peripheral configuration types.

use serde::{Deserialize, Serialize};

use super::state::SafeStopCategory;

/// Per-axis safety flags (FR-080).
///
/// Motion is blocked when ANY flag is `false` (FR-081).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisSafetyState {
    /// Tailstock is in safe position.
    pub tailstock_ok: bool,
    /// Locking pin is in safe position.
    pub lock_pin_ok: bool,
    /// Brake is released (or not configured).
    pub brake_ok: bool,
    /// Safety guard is closed and locked.
    pub guard_ok: bool,
    /// Hardware limit switches are not triggered.
    pub limit_switch_ok: bool,
    /// Position is within software limits.
    pub soft_limit_ok: bool,
    /// Motion enable input is active.
    pub motion_enable_ok: bool,
    /// Gearbox is in a valid state.
    pub gearbox_ok: bool,
}

impl Default for AxisSafetyState {
    fn default() -> Self {
        Self {
            tailstock_ok: true,
            lock_pin_ok: true,
            brake_ok: true,
            guard_ok: true,
            limit_switch_ok: true,
            soft_limit_ok: true,
            motion_enable_ok: true,
            gearbox_ok: true,
        }
    }
}

impl AxisSafetyState {
    /// Returns true if ALL safety flags are OK (motion is allowed).
    #[inline]
    pub const fn all_ok(&self) -> bool {
        self.tailstock_ok
            && self.lock_pin_ok
            && self.brake_ok
            && self.guard_ok
            && self.limit_switch_ok
            && self.soft_limit_ok
            && self.motion_enable_ok
            && self.gearbox_ok
    }

    /// Pack 8 boolean flags into a single `u8` for SHM transport.
    #[inline]
    pub const fn pack(&self) -> u8 {
        (self.tailstock_ok as u8)
            | ((self.lock_pin_ok as u8) << 1)
            | ((self.brake_ok as u8) << 2)
            | ((self.guard_ok as u8) << 3)
            | ((self.limit_switch_ok as u8) << 4)
            | ((self.soft_limit_ok as u8) << 5)
            | ((self.motion_enable_ok as u8) << 6)
            | ((self.gearbox_ok as u8) << 7)
    }

    /// Unpack from a single `u8`.
    #[inline]
    pub const fn unpack(v: u8) -> Self {
        Self {
            tailstock_ok: (v & 0x01) != 0,
            lock_pin_ok: (v & 0x02) != 0,
            brake_ok: (v & 0x04) != 0,
            guard_ok: (v & 0x08) != 0,
            limit_switch_ok: (v & 0x10) != 0,
            soft_limit_ok: (v & 0x20) != 0,
            motion_enable_ok: (v & 0x40) != 0,
            gearbox_ok: (v & 0x80) != 0,
        }
    }
}

/// Per-axis safe stop configuration (FR-015).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SafeStopConfig {
    /// Safe stop category (default: SS1).
    #[serde(default)]
    pub category: SafeStopCategory,
    /// Safe deceleration rate [mm/s²].
    #[serde(default = "default_max_decel_safe")]
    pub max_decel_safe: f64,
    /// Delay before brake engagement after STO [s].
    #[serde(default = "default_sto_brake_delay")]
    pub sto_brake_delay: f64,
    /// Holding torque for SS2 [%] (default: 20.0).
    #[serde(default = "default_ss2_holding_torque")]
    pub ss2_holding_torque: f64,
}

fn default_max_decel_safe() -> f64 {
    10000.0
}
fn default_sto_brake_delay() -> f64 {
    0.1
}
fn default_ss2_holding_torque() -> f64 {
    20.0
}

impl Default for SafeStopConfig {
    fn default() -> Self {
        Self {
            category: SafeStopCategory::SS1,
            max_decel_safe: 10000.0,
            sto_brake_delay: 0.1,
            ss2_holding_torque: 20.0,
        }
    }
}

// ─── Safety Peripheral Configs ──────────────────────────────────────

/// Tailstock type enumeration (FR-082).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TailstockType {
    /// Type 0: no tailstock.
    None = 0,
    /// Type 1: standard with sensors.
    Standard = 1,
    /// Type 2: with clamp.
    Sliding = 2,
    /// Type 3: type 1+2 combined.
    Combined = 3,
    /// Type 4: automatic clamp.
    Auto = 4,
}

impl TailstockType {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Standard),
            2 => Some(Self::Sliding),
            3 => Some(Self::Combined),
            4 => Some(Self::Auto),
            _ => None,
        }
    }
}

impl Default for TailstockType {
    fn default() -> Self {
        Self::None
    }
}

/// Tailstock configuration (FR-082).
///
/// I/O points referenced by `IoRole` and resolved from `io.toml` at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailstockConfig {
    /// Tailstock type (0-4).
    pub tailstock_type: TailstockType,
    /// DI role for tailstock closed confirmation.
    pub di_closed: String,
    /// NC/NO logic for closed sensor.
    #[serde(default)]
    pub closed_nc: bool,
    /// DI role for tailstock open confirmation.
    pub di_open: String,
    /// DI role for tailstock clamp locked (Type 2-4 only).
    #[serde(default)]
    pub di_clamp_locked: Option<String>,
}

/// Locking pin configuration (FR-083).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// DI role for pin locked position.
    pub di_locked: String,
    /// DI role for pin middle position (optional).
    #[serde(default)]
    pub di_middle: Option<String>,
    /// DI role for pin free position.
    pub di_free: String,
    /// Retract timeout [s].
    #[serde(default = "default_pin_timeout")]
    pub retract_timeout: f64,
    /// Insert timeout [s].
    #[serde(default = "default_pin_timeout")]
    pub insert_timeout: f64,
}

fn default_pin_timeout() -> f64 {
    3.0
}

/// Brake configuration (FR-084).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrakeConfig {
    /// DO role for brake command output.
    pub do_brake: String,
    /// DI role for brake release confirmation.
    pub di_released: String,
    /// Brake release timeout [s].
    #[serde(default = "default_brake_release_timeout")]
    pub release_timeout: f64,
    /// Brake engage timeout [s].
    #[serde(default = "default_brake_engage_timeout")]
    pub engage_timeout: f64,
    /// Some axes don't need position holding.
    #[serde(default)]
    pub always_free: bool,
    /// Output polarity inversion (also configurable per-point in io.toml).
    #[serde(default)]
    pub inverted: bool,
}

fn default_brake_release_timeout() -> f64 {
    2.0
}
fn default_brake_engage_timeout() -> f64 {
    1.0
}

/// Safety guard configuration (FR-085).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardConfig {
    /// DI role for guard closed sensor.
    pub di_closed: String,
    /// DI role for guard locked sensor.
    pub di_locked: String,
    /// Speed below which guard can open [mm/s].
    #[serde(default = "default_secure_speed")]
    pub secure_speed: f64,
    /// Speed must be below secure_speed for this duration before guard opens [s].
    #[serde(default = "default_open_delay")]
    pub open_delay: f64,
}

fn default_secure_speed() -> f64 {
    10.0
}
fn default_open_delay() -> f64 {
    2.0
}

/// Gear assist (oscillation) configuration for gear shifting (FR-062).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GearAssistConfig {
    /// Oscillation amplitude during gear shift [mm].
    pub assist_amplitude: f64,
    /// Oscillation frequency [Hz].
    pub assist_frequency: f64,
    /// Maximum time for gear assist motion [s].
    #[serde(default = "default_assist_timeout")]
    pub assist_timeout: f64,
    /// Maximum oscillation attempts before GearboxError.
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u8,
}

fn default_assist_timeout() -> f64 {
    5.0
}
fn default_max_attempts() -> u8 {
    3
}

impl Default for GearAssistConfig {
    fn default() -> Self {
        Self {
            assist_amplitude: 1.0,
            assist_frequency: 5.0,
            assist_timeout: 5.0,
            max_attempts: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_safety_state_all_ok() {
        let s = AxisSafetyState::default();
        assert!(s.all_ok());

        let s2 = AxisSafetyState {
            brake_ok: false,
            ..Default::default()
        };
        assert!(!s2.all_ok());
    }

    #[test]
    fn axis_safety_state_pack_unpack_roundtrip() {
        let original = AxisSafetyState::default();
        let packed = original.pack();
        let unpacked = AxisSafetyState::unpack(packed);
        assert_eq!(original, unpacked);

        // All false
        let all_false = AxisSafetyState {
            tailstock_ok: false,
            lock_pin_ok: false,
            brake_ok: false,
            guard_ok: false,
            limit_switch_ok: false,
            soft_limit_ok: false,
            motion_enable_ok: false,
            gearbox_ok: false,
        };
        let packed = all_false.pack();
        assert_eq!(packed, 0);
        let unpacked = AxisSafetyState::unpack(packed);
        assert_eq!(all_false, unpacked);

        // Mixed
        let mixed = AxisSafetyState {
            tailstock_ok: true,
            lock_pin_ok: false,
            brake_ok: true,
            guard_ok: false,
            limit_switch_ok: true,
            soft_limit_ok: false,
            motion_enable_ok: true,
            gearbox_ok: false,
        };
        let packed = mixed.pack();
        let unpacked = AxisSafetyState::unpack(packed);
        assert_eq!(mixed, unpacked);
    }

    #[test]
    fn tailstock_type_roundtrip() {
        for v in 0..=4u8 {
            let t = TailstockType::from_u8(v).unwrap();
            assert_eq!(t as u8, v);
        }
        assert!(TailstockType::from_u8(5).is_none());
    }
}
