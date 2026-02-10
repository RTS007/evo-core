//! Error bitflag types for the Control Unit (FR-090).
//!
//! All error types use the `bitflags` crate for compact bitflag representation.
//! Error flags marked CRITICAL trigger global SAFETY_STOP (FR-091, FR-092).

use bitflags::bitflags;

bitflags! {
    /// Power-related error flags (FR-090).
    ///
    /// CRITICAL flags (→ SAFETY_STOP): DRIVE_TAIL_OPEN, DRIVE_LOCK_PIN_LOCKED, DRIVE_BRAKE_LOCKED.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PowerError: u16 {
        /// Brake release/engage timeout.
        const BRAKE_TIMEOUT         = 0x0001;
        /// Locking pin retract/insert timeout.
        const LOCK_PIN_TIMEOUT      = 0x0002;
        /// Drive-reported fault.
        const DRIVE_FAULT           = 0x0004;
        /// Drive not ready within timeout.
        const DRIVE_NOT_READY       = 0x0008;
        /// Motion enable signal lost.
        const MOTION_ENABLE_LOST    = 0x0010;
        /// Tailstock open during motion. **CRITICAL → SAFETY_STOP**.
        const DRIVE_TAIL_OPEN       = 0x0020;
        /// Locking pin locked during motion. **CRITICAL → SAFETY_STOP**.
        const DRIVE_LOCK_PIN_LOCKED = 0x0040;
        /// Brake engaged during motion. **CRITICAL → SAFETY_STOP**.
        const DRIVE_BRAKE_LOCKED    = 0x0080;
    }
}

impl PowerError {
    /// Mask of all CRITICAL flags that trigger SAFETY_STOP.
    pub const CRITICAL_MASK: Self = Self::from_bits_truncate(
        Self::DRIVE_TAIL_OPEN.bits()
            | Self::DRIVE_LOCK_PIN_LOCKED.bits()
            | Self::DRIVE_BRAKE_LOCKED.bits(),
    );

    /// Returns true if any CRITICAL flag is set.
    #[inline]
    pub const fn has_critical(&self) -> bool {
        self.intersects(Self::CRITICAL_MASK)
    }
}

impl Default for PowerError {
    fn default() -> Self {
        Self::empty()
    }
}

bitflags! {
    /// Motion-related error flags (FR-090).
    ///
    /// CRITICAL flags (→ SAFETY_STOP): LAG_CRITICAL, DRIVE_ZEROSPEED, CYCLE_OVERRUN.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MotionError: u16 {
        /// Lag error exceeded limit (per lag_policy).
        const LAG_EXCEED           = 0x0001;
        /// Lag critical — SAFETY_STOP for ALL axes. **CRITICAL → SAFETY_STOP**.
        const LAG_CRITICAL         = 0x0002;
        /// Hardware limit switch triggered.
        const HARD_LIMIT           = 0x0004;
        /// Software position limit exceeded.
        const SOFT_LIMIT           = 0x0008;
        /// Overspeed detected.
        const OVERSPEED            = 0x0010;
        /// Acceleration limit exceeded.
        const ACCELERATION_LIMIT   = 0x0020;
        /// Homing procedure failed.
        const HOMING_FAILED        = 0x0040;
        /// Collision detected.
        const COLLISION_DETECTED   = 0x0080;
        /// Encoder fault.
        const ENCODER_FAULT        = 0x0100;
        /// Drive zero-speed signal during motion. **CRITICAL → SAFETY_STOP**.
        const DRIVE_ZEROSPEED      = 0x0200;
        /// Cycle time exceeded budget. **CRITICAL → SAFETY_STOP**.
        const CYCLE_OVERRUN        = 0x0400;
        /// Axis not referenced (informational).
        const NOT_REFERENCED       = 0x0800;
    }
}

impl MotionError {
    /// Mask of all CRITICAL flags that trigger SAFETY_STOP.
    pub const CRITICAL_MASK: Self = Self::from_bits_truncate(
        Self::LAG_CRITICAL.bits() | Self::DRIVE_ZEROSPEED.bits() | Self::CYCLE_OVERRUN.bits(),
    );

    /// Returns true if any CRITICAL flag is set.
    #[inline]
    pub const fn has_critical(&self) -> bool {
        self.intersects(Self::CRITICAL_MASK)
    }
}

impl Default for MotionError {
    fn default() -> Self {
        Self::empty()
    }
}

bitflags! {
    /// Command-related error flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CommandError: u8 {
        /// Axis is locked by a different command source.
        const SOURCE_LOCKED         = 0x01;
        /// Command source not authorized for this operation.
        const SOURCE_NOT_AUTHORIZED = 0x02;
        /// Source heartbeat stale (FR-130c).
        const SOURCE_TIMEOUT        = 0x04;
    }
}

impl Default for CommandError {
    fn default() -> Self {
        Self::empty()
    }
}

bitflags! {
    /// Gearbox-related error flags (FR-060).
    ///
    /// CRITICAL: NO_GEARSTEP → SAFETY_STOP (I-GB-2).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct GearboxError: u8 {
        /// Gear change timeout.
        const GEAR_TIMEOUT         = 0x01;
        /// Conflicting gear sensor readings.
        const GEAR_SENSOR_CONFLICT = 0x02;
        /// Unexpected gear loss during motion. **CRITICAL → SAFETY_STOP**.
        const NO_GEARSTEP          = 0x04;
        /// Gear change command denied.
        const GEAR_CHANGE_DENIED   = 0x08;
    }
}

impl GearboxError {
    /// Mask of all CRITICAL flags that trigger SAFETY_STOP.
    pub const CRITICAL_MASK: Self = Self::from_bits_truncate(Self::NO_GEARSTEP.bits());

    /// Returns true if any CRITICAL flag is set.
    #[inline]
    pub const fn has_critical(&self) -> bool {
        self.intersects(Self::CRITICAL_MASK)
    }
}

impl Default for GearboxError {
    fn default() -> Self {
        Self::empty()
    }
}

bitflags! {
    /// Coupling-related error flags (FR-050).
    ///
    /// CRITICAL: LAG_DIFF_EXCEED → SAFETY_STOP.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CouplingError: u8 {
        /// Synchronization timeout.
        const SYNC_TIMEOUT     = 0x01;
        /// Slave axis has a fault.
        const SLAVE_FAULT      = 0x02;
        /// Master axis lost/disconnected.
        const MASTER_LOST      = 0x04;
        /// Master-slave lag difference exceeded. **CRITICAL → SAFETY_STOP**.
        const LAG_DIFF_EXCEED  = 0x08;
    }
}

impl CouplingError {
    /// Mask of all CRITICAL flags that trigger SAFETY_STOP.
    pub const CRITICAL_MASK: Self = Self::from_bits_truncate(Self::LAG_DIFF_EXCEED.bits());

    /// Returns true if any CRITICAL flag is set.
    #[inline]
    pub const fn has_critical(&self) -> bool {
        self.intersects(Self::CRITICAL_MASK)
    }
}

impl Default for CouplingError {
    fn default() -> Self {
        Self::empty()
    }
}

/// Container for all per-axis error flags (FR-090).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AxisErrorState {
    /// Power-related errors.
    pub power: PowerError,
    /// Motion-related errors.
    pub motion: MotionError,
    /// Command-related errors.
    pub command: CommandError,
    /// Gearbox-related errors.
    pub gearbox: GearboxError,
    /// Coupling-related errors.
    pub coupling: CouplingError,
}

impl AxisErrorState {
    /// Returns true if any CRITICAL error flag is set across all categories.
    #[inline]
    pub const fn has_critical(&self) -> bool {
        self.power.has_critical()
            || self.motion.has_critical()
            || self.gearbox.has_critical()
            || self.coupling.has_critical()
    }

    /// Returns true if any error flag is set.
    #[inline]
    pub const fn has_any_error(&self) -> bool {
        !self.power.is_empty()
            || !self.motion.is_empty()
            || !self.command.is_empty()
            || !self.gearbox.is_empty()
            || !self.coupling.is_empty()
    }

    /// Clear all error flags.
    #[inline]
    pub fn clear(&mut self) {
        self.power = PowerError::empty();
        self.motion = MotionError::empty();
        self.command = CommandError::empty();
        self.gearbox = GearboxError::empty();
        self.coupling = CouplingError::empty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_error_critical() {
        let non_critical = PowerError::BRAKE_TIMEOUT | PowerError::DRIVE_FAULT;
        assert!(!non_critical.has_critical());

        let critical = PowerError::DRIVE_TAIL_OPEN;
        assert!(critical.has_critical());

        let mixed = PowerError::BRAKE_TIMEOUT | PowerError::DRIVE_BRAKE_LOCKED;
        assert!(mixed.has_critical());
    }

    #[test]
    fn motion_error_critical() {
        let non_critical = MotionError::LAG_EXCEED | MotionError::HARD_LIMIT;
        assert!(!non_critical.has_critical());

        let critical = MotionError::CYCLE_OVERRUN;
        assert!(critical.has_critical());
    }

    #[test]
    fn gearbox_error_critical() {
        let non_critical = GearboxError::GEAR_TIMEOUT;
        assert!(!non_critical.has_critical());

        let critical = GearboxError::NO_GEARSTEP;
        assert!(critical.has_critical());
    }

    #[test]
    fn coupling_error_critical() {
        let non_critical = CouplingError::SYNC_TIMEOUT | CouplingError::SLAVE_FAULT;
        assert!(!non_critical.has_critical());

        let critical = CouplingError::LAG_DIFF_EXCEED;
        assert!(critical.has_critical());
    }

    #[test]
    fn axis_error_state_has_critical() {
        let mut e = AxisErrorState::default();
        assert!(!e.has_critical());
        assert!(!e.has_any_error());

        e.power = PowerError::BRAKE_TIMEOUT;
        assert!(!e.has_critical());
        assert!(e.has_any_error());

        e.motion = MotionError::CYCLE_OVERRUN;
        assert!(e.has_critical());

        e.clear();
        assert!(!e.has_critical());
        assert!(!e.has_any_error());
    }

    #[test]
    fn bitflag_operations() {
        let mut e = PowerError::empty();
        e.insert(PowerError::BRAKE_TIMEOUT);
        e.insert(PowerError::DRIVE_FAULT);
        assert!(e.contains(PowerError::BRAKE_TIMEOUT));
        assert!(e.contains(PowerError::DRIVE_FAULT));
        assert!(!e.contains(PowerError::DRIVE_TAIL_OPEN));

        e.remove(PowerError::BRAKE_TIMEOUT);
        assert!(!e.contains(PowerError::BRAKE_TIMEOUT));
        assert!(e.contains(PowerError::DRIVE_FAULT));
    }

    // ── T035a: bitflags round-trip tests (bits → from_bits → bits) ──

    #[test]
    fn power_error_bits_roundtrip() {
        // Each individual flag round-trips through bits.
        for flag in [
            PowerError::BRAKE_TIMEOUT,
            PowerError::LOCK_PIN_TIMEOUT,
            PowerError::DRIVE_FAULT,
            PowerError::DRIVE_NOT_READY,
            PowerError::MOTION_ENABLE_LOST,
            PowerError::DRIVE_TAIL_OPEN,
            PowerError::DRIVE_LOCK_PIN_LOCKED,
            PowerError::DRIVE_BRAKE_LOCKED,
        ] {
            let bits = flag.bits();
            let back = PowerError::from_bits(bits).unwrap();
            assert_eq!(back, flag, "round-trip failed for PowerError 0x{bits:04x}");
        }
        // Combined flags round-trip.
        let combo = PowerError::BRAKE_TIMEOUT | PowerError::DRIVE_TAIL_OPEN;
        assert_eq!(PowerError::from_bits(combo.bits()).unwrap(), combo);
    }

    #[test]
    fn motion_error_bits_roundtrip() {
        for flag in [
            MotionError::LAG_EXCEED,
            MotionError::LAG_CRITICAL,
            MotionError::HARD_LIMIT,
            MotionError::SOFT_LIMIT,
            MotionError::OVERSPEED,
            MotionError::ACCELERATION_LIMIT,
            MotionError::HOMING_FAILED,
            MotionError::COLLISION_DETECTED,
            MotionError::ENCODER_FAULT,
            MotionError::DRIVE_ZEROSPEED,
            MotionError::CYCLE_OVERRUN,
            MotionError::NOT_REFERENCED,
        ] {
            let bits = flag.bits();
            let back = MotionError::from_bits(bits).unwrap();
            assert_eq!(back, flag, "round-trip failed for MotionError 0x{bits:04x}");
        }
        let combo = MotionError::LAG_EXCEED | MotionError::CYCLE_OVERRUN | MotionError::NOT_REFERENCED;
        assert_eq!(MotionError::from_bits(combo.bits()).unwrap(), combo);
    }

    #[test]
    fn command_error_bits_roundtrip() {
        for flag in [
            CommandError::SOURCE_LOCKED,
            CommandError::SOURCE_NOT_AUTHORIZED,
            CommandError::SOURCE_TIMEOUT,
        ] {
            let bits = flag.bits();
            let back = CommandError::from_bits(bits).unwrap();
            assert_eq!(back, flag, "round-trip failed for CommandError 0x{bits:02x}");
        }
        let combo = CommandError::SOURCE_LOCKED | CommandError::SOURCE_TIMEOUT;
        assert_eq!(CommandError::from_bits(combo.bits()).unwrap(), combo);
    }

    #[test]
    fn gearbox_error_bits_roundtrip() {
        for flag in [
            GearboxError::GEAR_TIMEOUT,
            GearboxError::GEAR_SENSOR_CONFLICT,
            GearboxError::NO_GEARSTEP,
            GearboxError::GEAR_CHANGE_DENIED,
        ] {
            let bits = flag.bits();
            let back = GearboxError::from_bits(bits).unwrap();
            assert_eq!(back, flag, "round-trip failed for GearboxError 0x{bits:02x}");
        }
        let combo = GearboxError::GEAR_TIMEOUT | GearboxError::NO_GEARSTEP;
        assert_eq!(GearboxError::from_bits(combo.bits()).unwrap(), combo);
    }

    #[test]
    fn coupling_error_bits_roundtrip() {
        for flag in [
            CouplingError::SYNC_TIMEOUT,
            CouplingError::SLAVE_FAULT,
            CouplingError::MASTER_LOST,
            CouplingError::LAG_DIFF_EXCEED,
        ] {
            let bits = flag.bits();
            let back = CouplingError::from_bits(bits).unwrap();
            assert_eq!(back, flag, "round-trip failed for CouplingError 0x{bits:02x}");
        }
        let combo = CouplingError::all();
        assert_eq!(CouplingError::from_bits(combo.bits()).unwrap(), combo);
    }

    #[test]
    fn all_bitflags_empty_and_all() {
        // empty() has no bits set; all() has all bits set.
        assert_eq!(PowerError::empty().bits(), 0);
        assert_eq!(MotionError::empty().bits(), 0);
        assert_eq!(CommandError::empty().bits(), 0);
        assert_eq!(GearboxError::empty().bits(), 0);
        assert_eq!(CouplingError::empty().bits(), 0);

        assert_ne!(PowerError::all().bits(), 0);
        assert_ne!(MotionError::all().bits(), 0);
        assert_ne!(CommandError::all().bits(), 0);
        assert_ne!(GearboxError::all().bits(), 0);
        assert_ne!(CouplingError::all().bits(), 0);
    }

    #[test]
    fn bitflags_insert_remove_toggle() {
        let mut m = MotionError::empty();
        m.insert(MotionError::LAG_EXCEED);
        assert!(m.contains(MotionError::LAG_EXCEED));

        m.toggle(MotionError::LAG_EXCEED);
        assert!(!m.contains(MotionError::LAG_EXCEED));

        m.toggle(MotionError::HARD_LIMIT);
        assert!(m.contains(MotionError::HARD_LIMIT));
        m.remove(MotionError::HARD_LIMIT);
        assert!(m.is_empty());
    }

    #[test]
    fn critical_mask_covers_all_critical_flags() {
        // Verify that CRITICAL_MASK correctly identifies all critical flags.
        // PowerError critical: DRIVE_TAIL_OPEN, DRIVE_LOCK_PIN_LOCKED, DRIVE_BRAKE_LOCKED.
        for flag in [PowerError::DRIVE_TAIL_OPEN, PowerError::DRIVE_LOCK_PIN_LOCKED, PowerError::DRIVE_BRAKE_LOCKED] {
            assert!(flag.has_critical(), "PowerError {flag:?} should be critical");
        }
        for flag in [PowerError::BRAKE_TIMEOUT, PowerError::DRIVE_FAULT, PowerError::DRIVE_NOT_READY] {
            assert!(!flag.has_critical(), "PowerError {flag:?} should NOT be critical");
        }

        // MotionError critical: LAG_CRITICAL, DRIVE_ZEROSPEED, CYCLE_OVERRUN.
        for flag in [MotionError::LAG_CRITICAL, MotionError::DRIVE_ZEROSPEED, MotionError::CYCLE_OVERRUN] {
            assert!(flag.has_critical(), "MotionError {flag:?} should be critical");
        }
        for flag in [MotionError::LAG_EXCEED, MotionError::HARD_LIMIT, MotionError::SOFT_LIMIT] {
            assert!(!flag.has_critical(), "MotionError {flag:?} should NOT be critical");
        }

        // GearboxError critical: NO_GEARSTEP.
        assert!(GearboxError::NO_GEARSTEP.has_critical());
        assert!(!GearboxError::GEAR_TIMEOUT.has_critical());

        // CouplingError critical: LAG_DIFF_EXCEED.
        assert!(CouplingError::LAG_DIFF_EXCEED.has_critical());
        assert!(!CouplingError::SYNC_TIMEOUT.has_critical());
        assert!(!CouplingError::SLAVE_FAULT.has_critical());
    }
}
