//! Homing types for the Control Unit (FR-030–FR-035).
//!
//! Defines `HomingMethod`, `HomingDirection`, and `HomingConfig`.

use serde::{Deserialize, Serialize};

/// Homing method enumeration (FR-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum HomingMethod {
    /// Home by driving into a hard stop and detecting current threshold.
    HardStop = 0,
    /// Home by finding a home sensor trigger.
    HomeSensor = 1,
    /// Home by driving to a limit switch.
    LimitSwitch = 2,
    /// Two-phase homing: sensor + index pulse.
    IndexPulse = 3,
    /// Absolute encoder — apply zero offset, no motion needed.
    Absolute = 4,
    /// No homing — axis is always considered referenced.
    NoHoming = 5,
}

impl HomingMethod {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::HardStop),
            1 => Some(Self::HomeSensor),
            2 => Some(Self::LimitSwitch),
            3 => Some(Self::IndexPulse),
            4 => Some(Self::Absolute),
            5 => Some(Self::NoHoming),
            _ => None,
        }
    }

    /// Returns true if this method requires an `approach_direction` (FR-033a).
    #[inline]
    pub const fn requires_approach_direction(&self) -> bool {
        matches!(
            self,
            Self::HardStop | Self::HomeSensor | Self::LimitSwitch | Self::IndexPulse
        )
    }
}

impl Default for HomingMethod {
    fn default() -> Self {
        Self::NoHoming
    }
}

/// Homing approach direction (FR-033a).
///
/// Determines initial travel direction during homing approach phase.
/// Safety-critical: prevents wrong-direction homing into mechanical stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum HomingDirection {
    /// Approach in +direction.
    Positive = 0,
    /// Approach in -direction.
    Negative = 1,
}

impl HomingDirection {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Positive),
            1 => Some(Self::Negative),
            _ => None,
        }
    }

    /// Returns the sign multiplier for approach direction.
    #[inline]
    pub const fn sign(&self) -> f64 {
        match self {
            Self::Positive => 1.0,
            Self::Negative => -1.0,
        }
    }
}

impl Default for HomingDirection {
    fn default() -> Self {
        Self::Positive
    }
}

/// Homing configuration for a single axis (FR-033).
///
/// Method-specific fields use the universal parameter set.
/// Unused fields for a given method are ignored (zero/default).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomingConfig {
    /// Homing method to use.
    #[serde(default)]
    pub method: HomingMethod,
    /// Homing speed [mm/s].
    #[serde(default = "default_homing_speed")]
    pub speed: f64,
    /// Maximum torque during homing [%].
    #[serde(default = "default_torque_limit")]
    pub torque_limit: f64,
    /// Per-method timeout [s].
    #[serde(default = "default_homing_timeout")]
    pub timeout: f64,
    /// HARD_STOP only: current threshold for detecting hard stop.
    #[serde(default)]
    pub current_threshold: f64,
    /// Approach direction — mandatory for HardStop/HomeSensor/LimitSwitch/IndexPulse (FR-033a).
    #[serde(default)]
    pub approach_direction: Option<HomingDirection>,
    /// HOME_SENSOR / INDEX_PULSE: sensor role name (resolved from io.toml, FR-149).
    #[serde(default)]
    pub sensor_role: Option<String>,
    /// INDEX_PULSE only: index pulse role name (resolved from io.toml, FR-149).
    #[serde(default)]
    pub index_role: Option<String>,
    /// NC/NO config for sensor.
    #[serde(default)]
    pub sensor_nc: bool,
    /// LIMIT_SWITCH: +1 (high limit) or -1 (low limit).
    #[serde(default)]
    pub limit_direction: i8,
    /// ABSOLUTE only: zero offset [mm].
    #[serde(default)]
    pub zero_offset: f64,
}

fn default_homing_speed() -> f64 {
    10.0
}
fn default_torque_limit() -> f64 {
    50.0
}
fn default_homing_timeout() -> f64 {
    30.0
}

impl Default for HomingConfig {
    fn default() -> Self {
        Self {
            method: HomingMethod::NoHoming,
            speed: 10.0,
            torque_limit: 50.0,
            timeout: 30.0,
            current_threshold: 0.0,
            approach_direction: None,
            sensor_role: None,
            index_role: None,
            sensor_nc: false,
            limit_direction: 0,
            zero_offset: 0.0,
        }
    }
}

impl HomingConfig {
    /// Validates that approach_direction is set when required (FR-033a).
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.method.requires_approach_direction() && self.approach_direction.is_none() {
            return Err(
                "approach_direction is mandatory for HardStop, HomeSensor, LimitSwitch, and IndexPulse homing methods (FR-033a)",
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homing_method_roundtrip() {
        for v in 0..=5u8 {
            let method = HomingMethod::from_u8(v).unwrap();
            assert_eq!(method as u8, v);
        }
        assert!(HomingMethod::from_u8(6).is_none());
    }

    #[test]
    fn homing_method_requires_direction() {
        assert!(HomingMethod::HardStop.requires_approach_direction());
        assert!(HomingMethod::HomeSensor.requires_approach_direction());
        assert!(HomingMethod::LimitSwitch.requires_approach_direction());
        assert!(HomingMethod::IndexPulse.requires_approach_direction());
        assert!(!HomingMethod::Absolute.requires_approach_direction());
        assert!(!HomingMethod::NoHoming.requires_approach_direction());
    }

    #[test]
    fn homing_direction_roundtrip() {
        assert_eq!(HomingDirection::from_u8(0).unwrap(), HomingDirection::Positive);
        assert_eq!(HomingDirection::from_u8(1).unwrap(), HomingDirection::Negative);
        assert!(HomingDirection::from_u8(2).is_none());
    }

    #[test]
    fn homing_direction_sign() {
        assert_eq!(HomingDirection::Positive.sign(), 1.0);
        assert_eq!(HomingDirection::Negative.sign(), -1.0);
    }

    #[test]
    fn homing_config_validate_direction() {
        let mut config = HomingConfig {
            method: HomingMethod::HardStop,
            approach_direction: None,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        config.approach_direction = Some(HomingDirection::Positive);
        assert!(config.validate().is_ok());

        let nohoming = HomingConfig {
            method: HomingMethod::NoHoming,
            approach_direction: None,
            ..Default::default()
        };
        assert!(nohoming.validate().is_ok());
    }
}
