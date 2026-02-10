//! I/O Role types (FR-149, FR-151).
//!
//! `IoRole` maps a string like `"LimitMin1"` to a typed enum variant
//! with axis number extraction. Used by both HAL and CU to resolve
//! I/O points by functional role rather than pin number.

use core::fmt;
use core::str::FromStr;
use serde::{Deserialize, Serialize};

// ─── IoPointType ────────────────────────────────────────────────────

/// I/O point type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum IoPointType {
    Di = 0,
    Do = 1,
    Ai = 2,
    Ao = 3,
}

impl fmt::Display for IoPointType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Di => write!(f, "di"),
            Self::Do => write!(f, "do"),
            Self::Ai => write!(f, "ai"),
            Self::Ao => write!(f, "ao"),
        }
    }
}

impl FromStr for IoPointType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "di" => Ok(Self::Di),
            "do" => Ok(Self::Do),
            "ai" => Ok(Self::Ai),
            "ao" => Ok(Self::Ao),
            _ => Err(format!("unknown IoPointType: {s:?}")),
        }
    }
}

// ─── DiLogic ────────────────────────────────────────────────────────

/// Digital input logic interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum DiLogic {
    /// Normally Open — true when signal present.
    #[serde(rename = "NO")]
    NO = 0,
    /// Normally Closed — inverted (wire break = active).
    #[serde(rename = "NC")]
    NC = 1,
}

impl Default for DiLogic {
    fn default() -> Self {
        Self::NO
    }
}

impl FromStr for DiLogic {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "NO" => Ok(Self::NO),
            "NC" => Ok(Self::NC),
            _ => Err(format!("unknown DiLogic: {s:?}, expected \"NO\" or \"NC\"")),
        }
    }
}

// ─── IoRole ─────────────────────────────────────────────────────────

/// Functional I/O role following **FunctionAxisNumber** convention.
///
/// Global roles have no axis parameter. Per-axis roles carry a `u8` axis
/// number (1-based). Unknown strings become `Custom`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IoRole {
    // ── Safety (global) ─────────────
    EStop,
    SafetyGate,
    EStopReset,

    // ── Control (global) ────────────
    Start,
    Stop,
    Reset,
    Pause,

    // ── Pneumatics / general (global) ──
    PressureOk,
    VacuumOk,

    // ── Per-axis DI ─────────────────
    LimitMin(u8),
    LimitMax(u8),
    Ref(u8),
    Enable(u8),

    // ── Per-axis peripherals ────────
    TailClosed(u8),
    TailOpen(u8),
    TailClamp(u8),
    IndexLocked(u8),
    IndexMiddle(u8),
    IndexFree(u8),
    BrakeIn(u8),
    BrakeOut(u8),
    GuardClosed(u8),
    GuardLocked(u8),

    // ── Project-specific extension ──
    Custom(String),
}

impl IoRole {
    /// Return the axis number if this is a per-axis role, else `None`.
    pub fn axis(&self) -> Option<u8> {
        match self {
            Self::LimitMin(n)
            | Self::LimitMax(n)
            | Self::Ref(n)
            | Self::Enable(n)
            | Self::TailClosed(n)
            | Self::TailOpen(n)
            | Self::TailClamp(n)
            | Self::IndexLocked(n)
            | Self::IndexMiddle(n)
            | Self::IndexFree(n)
            | Self::BrakeIn(n)
            | Self::BrakeOut(n)
            | Self::GuardClosed(n)
            | Self::GuardLocked(n) => Some(*n),
            _ => None,
        }
    }

    /// Expected I/O type for known roles (V-IO-3).
    pub fn expected_io_type(&self) -> Option<IoPointType> {
        match self {
            // DI roles
            Self::EStop
            | Self::SafetyGate
            | Self::EStopReset
            | Self::Start
            | Self::Stop
            | Self::Reset
            | Self::Pause
            | Self::PressureOk
            | Self::VacuumOk
            | Self::LimitMin(_)
            | Self::LimitMax(_)
            | Self::Ref(_)
            | Self::Enable(_)
            | Self::TailClosed(_)
            | Self::TailOpen(_)
            | Self::TailClamp(_)
            | Self::IndexLocked(_)
            | Self::IndexMiddle(_)
            | Self::IndexFree(_)
            | Self::BrakeIn(_)
            | Self::GuardClosed(_)
            | Self::GuardLocked(_) => Some(IoPointType::Di),

            // DO roles
            Self::BrakeOut(_) => Some(IoPointType::Do),

            // Custom — any type allowed
            Self::Custom(_) => None,
        }
    }
}

// ─── FunctionAxisNumber Parser ──────────────────────────────────────

/// Split a role string into (prefix, optional_axis_number).
///
/// `"LimitMin1"` → `("LimitMin", Some(1))`
/// `"EStop"`     → `("EStop", None)`
fn split_role_str(s: &str) -> (&str, Option<u8>) {
    // Find the position where trailing digits start.
    let digit_start = s
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, _)| i);

    match digit_start {
        Some(i) if i > 0 => {
            let prefix = &s[..i];
            let num_str = &s[i..];
            match num_str.parse::<u8>() {
                Ok(n) => (prefix, Some(n)),
                Err(_) => (s, None), // overflow → treat as custom
            }
        }
        _ => (s, None),
    }
}

impl FromStr for IoRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (prefix, axis) = split_role_str(s);

        // Match known global roles first (no axis number expected).
        match prefix {
            "EStop" if axis.is_none() => return Ok(Self::EStop),
            "SafetyGate" if axis.is_none() => return Ok(Self::SafetyGate),
            "EStopReset" if axis.is_none() => return Ok(Self::EStopReset),
            "Start" if axis.is_none() => return Ok(Self::Start),
            "Stop" if axis.is_none() => return Ok(Self::Stop),
            "Reset" if axis.is_none() => return Ok(Self::Reset),
            "Pause" if axis.is_none() => return Ok(Self::Pause),
            "PressureOk" if axis.is_none() => return Ok(Self::PressureOk),
            "VacuumOk" if axis.is_none() => return Ok(Self::VacuumOk),
            _ => {}
        }

        // Match known per-axis roles (axis number required).
        if let Some(n) = axis {
            match prefix {
                "LimitMin" => return Ok(Self::LimitMin(n)),
                "LimitMax" => return Ok(Self::LimitMax(n)),
                "Ref" => return Ok(Self::Ref(n)),
                "Enable" => return Ok(Self::Enable(n)),
                "TailClosed" => return Ok(Self::TailClosed(n)),
                "TailOpen" => return Ok(Self::TailOpen(n)),
                "TailClamp" => return Ok(Self::TailClamp(n)),
                "IndexLocked" => return Ok(Self::IndexLocked(n)),
                "IndexMiddle" => return Ok(Self::IndexMiddle(n)),
                "IndexFree" => return Ok(Self::IndexFree(n)),
                "BrakeIn" => return Ok(Self::BrakeIn(n)),
                "BrakeOut" => return Ok(Self::BrakeOut(n)),
                "GuardClosed" => return Ok(Self::GuardClosed(n)),
                "GuardLocked" => return Ok(Self::GuardLocked(n)),
                _ => {}
            }
        }

        // Fallback: Custom role.
        Ok(Self::Custom(s.to_string()))
    }
}

impl fmt::Display for IoRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EStop => write!(f, "EStop"),
            Self::SafetyGate => write!(f, "SafetyGate"),
            Self::EStopReset => write!(f, "EStopReset"),
            Self::Start => write!(f, "Start"),
            Self::Stop => write!(f, "Stop"),
            Self::Reset => write!(f, "Reset"),
            Self::Pause => write!(f, "Pause"),
            Self::PressureOk => write!(f, "PressureOk"),
            Self::VacuumOk => write!(f, "VacuumOk"),
            Self::LimitMin(n) => write!(f, "LimitMin{n}"),
            Self::LimitMax(n) => write!(f, "LimitMax{n}"),
            Self::Ref(n) => write!(f, "Ref{n}"),
            Self::Enable(n) => write!(f, "Enable{n}"),
            Self::TailClosed(n) => write!(f, "TailClosed{n}"),
            Self::TailOpen(n) => write!(f, "TailOpen{n}"),
            Self::TailClamp(n) => write!(f, "TailClamp{n}"),
            Self::IndexLocked(n) => write!(f, "IndexLocked{n}"),
            Self::IndexMiddle(n) => write!(f, "IndexMiddle{n}"),
            Self::IndexFree(n) => write!(f, "IndexFree{n}"),
            Self::BrakeIn(n) => write!(f, "BrakeIn{n}"),
            Self::BrakeOut(n) => write!(f, "BrakeOut{n}"),
            Self::GuardClosed(n) => write!(f, "GuardClosed{n}"),
            Self::GuardLocked(n) => write!(f, "GuardLocked{n}"),
            Self::Custom(s) => write!(f, "{s}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_global_roles() {
        assert_eq!("EStop".parse::<IoRole>().unwrap(), IoRole::EStop);
        assert_eq!("SafetyGate".parse::<IoRole>().unwrap(), IoRole::SafetyGate);
        assert_eq!("PressureOk".parse::<IoRole>().unwrap(), IoRole::PressureOk);
        assert_eq!("VacuumOk".parse::<IoRole>().unwrap(), IoRole::VacuumOk);
        assert_eq!("Start".parse::<IoRole>().unwrap(), IoRole::Start);
        assert_eq!("Stop".parse::<IoRole>().unwrap(), IoRole::Stop);
        assert_eq!("Reset".parse::<IoRole>().unwrap(), IoRole::Reset);
        assert_eq!("Pause".parse::<IoRole>().unwrap(), IoRole::Pause);
    }

    #[test]
    fn parse_per_axis_roles() {
        assert_eq!("LimitMin1".parse::<IoRole>().unwrap(), IoRole::LimitMin(1));
        assert_eq!("LimitMax3".parse::<IoRole>().unwrap(), IoRole::LimitMax(3));
        assert_eq!("Ref2".parse::<IoRole>().unwrap(), IoRole::Ref(2));
        assert_eq!("Enable4".parse::<IoRole>().unwrap(), IoRole::Enable(4));
        assert_eq!(
            "TailClosed1".parse::<IoRole>().unwrap(),
            IoRole::TailClosed(1)
        );
        assert_eq!("TailOpen1".parse::<IoRole>().unwrap(), IoRole::TailOpen(1));
        assert_eq!(
            "TailClamp1".parse::<IoRole>().unwrap(),
            IoRole::TailClamp(1)
        );
        assert_eq!(
            "IndexLocked2".parse::<IoRole>().unwrap(),
            IoRole::IndexLocked(2)
        );
        assert_eq!(
            "IndexMiddle2".parse::<IoRole>().unwrap(),
            IoRole::IndexMiddle(2)
        );
        assert_eq!(
            "IndexFree2".parse::<IoRole>().unwrap(),
            IoRole::IndexFree(2)
        );
        assert_eq!("BrakeIn3".parse::<IoRole>().unwrap(), IoRole::BrakeIn(3));
        assert_eq!("BrakeOut3".parse::<IoRole>().unwrap(), IoRole::BrakeOut(3));
        assert_eq!(
            "GuardClosed1".parse::<IoRole>().unwrap(),
            IoRole::GuardClosed(1)
        );
        assert_eq!(
            "GuardLocked1".parse::<IoRole>().unwrap(),
            IoRole::GuardLocked(1)
        );
    }

    #[test]
    fn roundtrip_display_parse() {
        let roles = [
            IoRole::EStop,
            IoRole::LimitMin(1),
            IoRole::LimitMax(64),
            IoRole::BrakeOut(3),
            IoRole::Ref(2),
            IoRole::Custom("MyCustom".to_string()),
        ];
        for role in &roles {
            let s = role.to_string();
            let parsed: IoRole = s.parse().unwrap();
            assert_eq!(&parsed, role, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn custom_fallback() {
        let role: IoRole = "MyCustomSensor".parse().unwrap();
        assert_eq!(role, IoRole::Custom("MyCustomSensor".to_string()));
    }

    #[test]
    fn expected_io_types() {
        assert_eq!(
            IoRole::EStop.expected_io_type(),
            Some(IoPointType::Di)
        );
        assert_eq!(
            IoRole::LimitMin(1).expected_io_type(),
            Some(IoPointType::Di)
        );
        assert_eq!(
            IoRole::BrakeOut(3).expected_io_type(),
            Some(IoPointType::Do)
        );
        assert_eq!(
            IoRole::Custom("x".to_string()).expected_io_type(),
            None
        );
    }

    #[test]
    fn axis_number() {
        assert_eq!(IoRole::EStop.axis(), None);
        assert_eq!(IoRole::LimitMin(5).axis(), Some(5));
        assert_eq!(IoRole::BrakeOut(2).axis(), Some(2));
        assert_eq!(IoRole::Custom("x".to_string()).axis(), None);
    }

    #[test]
    fn io_point_type_roundtrip() {
        assert_eq!("di".parse::<IoPointType>().unwrap(), IoPointType::Di);
        assert_eq!("do".parse::<IoPointType>().unwrap(), IoPointType::Do);
        assert_eq!("ai".parse::<IoPointType>().unwrap(), IoPointType::Ai);
        assert_eq!("ao".parse::<IoPointType>().unwrap(), IoPointType::Ao);
        assert!("xx".parse::<IoPointType>().is_err());
    }

    #[test]
    fn di_logic_default_is_no() {
        assert_eq!(DiLogic::default(), DiLogic::NO);
    }
}
