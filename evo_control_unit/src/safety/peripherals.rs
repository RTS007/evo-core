//! Safety peripheral monitoring: tailstock, locking pin, brake, guard.
//!
//! Reads DI via `IoRegistry` role-based API, evaluates safety conditions,
//! and produces boolean flags + error flags on violations (FR-082–FR-085).
//!
//! NC/NO logic is handled transparently by `IoRegistry::read_di` —
//! callers always see logical `true` = signal active.

use evo_common::control_unit::error::PowerError;
use evo_common::control_unit::safety::{
    BrakeConfig, GuardConfig, IndexConfig, TailstockConfig, TailstockType,
};
use evo_common::io::registry::IoRegistry;
use evo_common::io::role::IoRole;

// ─── Result Types ───────────────────────────────────────────────────

/// Per-cycle evaluation result for a single peripheral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeripheralResult {
    /// Whether this peripheral allows motion.
    pub ok: bool,
    /// Accumulated error flags (empty if `ok == true` and no timeouts).
    pub errors: PowerError,
}

impl PeripheralResult {
    /// Peripheral is safe — no errors.
    pub const OK: Self = Self {
        ok: true,
        errors: PowerError::empty(),
    };

    /// Peripheral is unsafe with the given error.
    pub const fn fault(errors: PowerError) -> Self {
        Self { ok: false, errors }
    }
}

// ─── T045: Tailstock Monitor (FR-082) ───────────────────────────────

/// Tailstock peripheral monitor.
///
/// Evaluates tailstock safety per type 0–4:
/// - Type 0 (None): always ok.
/// - Type 1 (Standard): di_closed must be active.
/// - Type 2 (Sliding): di_closed AND di_clamp_locked must be active.
/// - Type 3 (Combined): di_closed OR di_clamp_locked must be active.
/// - Type 4 (Auto): di_closed AND di_clamp_locked must be active.
///
/// Detects `ERR_SENSOR_CONFLICT` when di_closed AND di_open are both active.
#[derive(Debug)]
pub struct TailstockMonitor {
    /// Tailstock type determines evaluation logic.
    tailstock_type: TailstockType,
    /// IoRole for the closed sensor.
    role_closed: IoRole,
    /// IoRole for the open sensor.
    role_open: IoRole,
    /// IoRole for the clamp locked sensor (types 2-4 only).
    role_clamp: Option<IoRole>,
}

impl TailstockMonitor {
    /// Create a new tailstock monitor from config and axis number.
    ///
    /// Returns `None` if the tailstock type is `None` (type 0).
    pub fn new(config: &TailstockConfig, axis_id: u8) -> Option<Self> {
        if config.tailstock_type == TailstockType::None {
            return None;
        }
        Some(Self {
            tailstock_type: config.tailstock_type,
            role_closed: IoRole::TailClosed(axis_id),
            role_open: IoRole::TailOpen(axis_id),
            role_clamp: config
                .di_clamp_locked
                .as_ref()
                .map(|_| IoRole::TailClamp(axis_id)),
        })
    }

    /// Evaluate tailstock safety from DI bank.
    ///
    /// Returns `PeripheralResult` with `ok` flag and any error flags.
    pub fn evaluate(
        &self,
        registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> PeripheralResult {
        let closed = registry.read_di(&self.role_closed, di_bank).unwrap_or(false);
        let open = registry.read_di(&self.role_open, di_bank).unwrap_or(false);

        // Sensor conflict: both closed AND open active simultaneously.
        if closed && open {
            return PeripheralResult::fault(PowerError::DRIVE_TAIL_OPEN);
        }

        let clamp_locked = self
            .role_clamp
            .as_ref()
            .and_then(|r| registry.read_di(r, di_bank))
            .unwrap_or(false);

        let ok = match self.tailstock_type {
            TailstockType::None => true, // unreachable — filtered in new()
            TailstockType::Standard => closed,
            TailstockType::Sliding => closed && clamp_locked,
            TailstockType::Combined => closed || clamp_locked,
            TailstockType::Auto => closed && clamp_locked,
        };

        if ok {
            PeripheralResult::OK
        } else {
            PeripheralResult::fault(PowerError::DRIVE_TAIL_OPEN)
        }
    }
}

// ─── T046: Locking Pin Monitor (FR-083) ─────────────────────────────

/// Locking pin state evaluation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinPosition {
    /// Pin is locked — NOT safe for motion.
    Locked,
    /// Pin is in middle position — NOT safe for motion.
    Middle,
    /// Pin is free — safe for motion.
    Free,
    /// No sensor confirms any position — sensor error.
    Unknown,
}

/// Locking pin peripheral monitor.
///
/// Valid state for motion: NOT locked AND free (FR-083).
/// Tracks retract/insert timeout via cycle counting.
#[derive(Debug)]
pub struct LockPinMonitor {
    role_locked: IoRole,
    role_middle: Option<IoRole>,
    role_free: IoRole,
    /// Retract timeout in cycles.
    retract_timeout_cycles: u64,
    /// Insert timeout in cycles.
    insert_timeout_cycles: u64,
    /// Cycles spent waiting for retraction (locked→free).
    retract_wait_cycles: u64,
    /// Cycles spent waiting for insertion (free→locked).
    insert_wait_cycles: u64,
    /// Whether we are currently waiting for retraction.
    awaiting_retract: bool,
    /// Whether we are currently waiting for insertion.
    awaiting_insert: bool,
}

impl LockPinMonitor {
    /// Create a new locking pin monitor.
    pub fn new(config: &IndexConfig, axis_id: u8, cycle_time_us: u32) -> Self {
        let cycle_s = cycle_time_us as f64 / 1_000_000.0;
        Self {
            role_locked: IoRole::IndexLocked(axis_id),
            role_middle: config.di_middle.as_ref().map(|_| IoRole::IndexMiddle(axis_id)),
            role_free: IoRole::IndexFree(axis_id),
            retract_timeout_cycles: (config.retract_timeout / cycle_s).ceil() as u64,
            insert_timeout_cycles: (config.insert_timeout / cycle_s).ceil() as u64,
            retract_wait_cycles: 0,
            insert_wait_cycles: 0,
            awaiting_retract: false,
            awaiting_insert: false,
        }
    }

    /// Read the current pin position from DI bank.
    pub fn read_position(
        &self,
        registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> PinPosition {
        let locked = registry.read_di(&self.role_locked, di_bank).unwrap_or(false);
        let free = registry.read_di(&self.role_free, di_bank).unwrap_or(false);
        let middle = self
            .role_middle
            .as_ref()
            .and_then(|r| registry.read_di(r, di_bank))
            .unwrap_or(false);

        if locked && !free && !middle {
            PinPosition::Locked
        } else if free && !locked && !middle {
            PinPosition::Free
        } else if middle && !locked && !free {
            PinPosition::Middle
        } else if !locked && !free && !middle {
            PinPosition::Unknown
        } else {
            // Multiple sensors active — conflict → treat as unknown/unsafe.
            PinPosition::Unknown
        }
    }

    /// Evaluate locking pin safety.
    ///
    /// Motion requires pin to be Free. If awaiting retraction and timeout
    /// expires, sets `LOCK_PIN_TIMEOUT`. If pin is locked during motion,
    /// sets `DRIVE_LOCK_PIN_LOCKED`.
    pub fn evaluate(
        &mut self,
        registry: &IoRegistry,
        di_bank: &[u64; 16],
        is_powered: bool,
    ) -> PeripheralResult {
        let position = self.read_position(registry, di_bank);

        match position {
            PinPosition::Free => {
                // Pin retracted — safe for motion.
                self.awaiting_retract = false;
                self.retract_wait_cycles = 0;
                PeripheralResult::OK
            }
            PinPosition::Locked => {
                // Pin is locked — not safe.
                self.awaiting_insert = false;
                self.insert_wait_cycles = 0;

                if is_powered {
                    // Axis is powered and pin is locked — critical error.
                    return PeripheralResult::fault(PowerError::DRIVE_LOCK_PIN_LOCKED);
                }

                // Track retraction wait.
                if self.awaiting_retract {
                    self.retract_wait_cycles += 1;
                    if self.retract_wait_cycles >= self.retract_timeout_cycles {
                        return PeripheralResult::fault(PowerError::LOCK_PIN_TIMEOUT);
                    }
                }
                PeripheralResult::fault(PowerError::empty())
            }
            PinPosition::Middle | PinPosition::Unknown => {
                // Transitional or unknown — not safe, track timeout.
                if self.awaiting_retract {
                    self.retract_wait_cycles += 1;
                    if self.retract_wait_cycles >= self.retract_timeout_cycles {
                        return PeripheralResult::fault(PowerError::LOCK_PIN_TIMEOUT);
                    }
                }
                if self.awaiting_insert {
                    self.insert_wait_cycles += 1;
                    if self.insert_wait_cycles >= self.insert_timeout_cycles {
                        return PeripheralResult::fault(PowerError::LOCK_PIN_TIMEOUT);
                    }
                }
                PeripheralResult::fault(PowerError::empty())
            }
        }
    }

    /// Start awaiting pin retraction (called when power-on sequence begins).
    pub fn start_retract(&mut self) {
        self.awaiting_retract = true;
        self.retract_wait_cycles = 0;
    }

    /// Start awaiting pin insertion (called when power-off sequence begins).
    pub fn start_insert(&mut self) {
        self.awaiting_insert = true;
        self.insert_wait_cycles = 0;
    }

    /// Reset all timeout tracking.
    pub fn reset(&mut self) {
        self.awaiting_retract = false;
        self.awaiting_insert = false;
        self.retract_wait_cycles = 0;
        self.insert_wait_cycles = 0;
    }
}

// ─── T047: Brake Monitor (FR-084) ──────────────────────────────────

/// Brake command state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrakeCommand {
    /// Brake should be engaged (holding).
    Engage,
    /// Brake should be released (free to move).
    Release,
}

/// Axis brake peripheral monitor.
///
/// DO: brake command (TRUE = release, unless inverted).
/// DI: brake release confirmation.
/// Timeout tracking for both release and engage operations.
#[derive(Debug)]
pub struct BrakeMonitor {
    role_do: IoRole,
    role_di: IoRole,
    /// Brake release timeout in cycles.
    release_timeout_cycles: u64,
    /// Brake engage timeout in cycles.
    engage_timeout_cycles: u64,
    /// Whether this brake can always be free (no position holding needed).
    always_free: bool,
    /// Whether the DO output polarity is inverted.
    inverted: bool,
    /// Current commanded state.
    command: BrakeCommand,
    /// Cycles spent waiting for confirmation.
    wait_cycles: u64,
}

impl BrakeMonitor {
    /// Create a new brake monitor.
    pub fn new(config: &BrakeConfig, axis_id: u8, cycle_time_us: u32) -> Self {
        let cycle_s = cycle_time_us as f64 / 1_000_000.0;
        Self {
            role_do: IoRole::BrakeOut(axis_id),
            role_di: IoRole::BrakeIn(axis_id),
            release_timeout_cycles: (config.release_timeout / cycle_s).ceil() as u64,
            engage_timeout_cycles: (config.engage_timeout / cycle_s).ceil() as u64,
            always_free: config.always_free,
            inverted: config.inverted,
            command: BrakeCommand::Engage,
            wait_cycles: 0,
        }
    }

    /// Command the brake to release (during power-on).
    pub fn command_release(&mut self) {
        if self.command != BrakeCommand::Release {
            self.command = BrakeCommand::Release;
            self.wait_cycles = 0;
        }
    }

    /// Command the brake to engage (during power-off or safety stop).
    pub fn command_engage(&mut self) {
        if self.command != BrakeCommand::Engage {
            self.command = BrakeCommand::Engage;
            self.wait_cycles = 0;
        }
    }

    /// Write the brake DO command to the DO bank.
    pub fn write_command(
        &self,
        registry: &IoRegistry,
        do_bank: &mut [u64; 16],
    ) {
        // TRUE = release, FALSE = engage (before inversion).
        // IoRegistry::write_do applies its own binding-level inversion.
        // BrakeConfig.inverted is an additional axis-level inversion.
        let logical = match self.command {
            BrakeCommand::Release => true,
            BrakeCommand::Engage => false,
        };
        let output = if self.inverted { !logical } else { logical };
        let _ = registry.write_do(&self.role_do, output, do_bank);
    }

    /// Evaluate brake safety from DI bank.
    ///
    /// When brake is always_free, evaluation always returns OK.
    /// Otherwise, checks DI confirmation against commanded state with timeout.
    pub fn evaluate(
        &mut self,
        registry: &IoRegistry,
        di_bank: &[u64; 16],
        _is_powered: bool,
    ) -> PeripheralResult {
        if self.always_free {
            return PeripheralResult::OK;
        }

        let released = registry.read_di(&self.role_di, di_bank).unwrap_or(false);

        match self.command {
            BrakeCommand::Release => {
                if released {
                    // Confirmation received — brake is released.
                    self.wait_cycles = 0;
                    PeripheralResult::OK
                } else {
                    // Waiting for release confirmation.
                    self.wait_cycles += 1;
                    if self.wait_cycles >= self.release_timeout_cycles {
                        PeripheralResult::fault(PowerError::BRAKE_TIMEOUT)
                    } else {
                        // Still waiting — not yet ok for motion, but no error yet.
                        PeripheralResult::fault(PowerError::empty())
                    }
                }
            }
            BrakeCommand::Engage => {
                if !released {
                    // Brake is engaged as expected.
                    self.wait_cycles = 0;
                    PeripheralResult::OK
                } else {
                    // Brake released while commanded engage — track timeout.
                    self.wait_cycles += 1;
                    if self.wait_cycles >= self.engage_timeout_cycles {
                        PeripheralResult::fault(PowerError::BRAKE_TIMEOUT)
                    } else {
                        PeripheralResult::OK
                    }
                }
            }
        }
    }

    /// Check if brake is confirmed released.
    pub fn is_released(
        &self,
        registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> bool {
        if self.always_free {
            return true;
        }
        registry.read_di(&self.role_di, di_bank).unwrap_or(false)
    }

    /// Reset the monitor state.
    pub fn reset(&mut self) {
        self.command = BrakeCommand::Engage;
        self.wait_cycles = 0;
    }
}

// ─── T048: Guard Monitor (FR-085) ──────────────────────────────────

/// Safety guard peripheral monitor.
///
/// When speed > `secure_speed`, guard MUST be closed AND locked.
/// Guard can only open when speed < `secure_speed` for at least
/// `open_delay` seconds (FR-085).
#[derive(Debug)]
pub struct GuardMonitor {
    role_closed: IoRole,
    role_locked: IoRole,
    /// Speed below which guard can open [user units/s].
    secure_speed: f64,
    /// Number of consecutive cycles speed must be below secure_speed.
    open_delay_cycles: u64,
    /// Cycles since speed dropped below secure_speed.
    low_speed_cycles: u64,
}

impl GuardMonitor {
    /// Create a new guard monitor.
    pub fn new(config: &GuardConfig, axis_id: u8, cycle_time_us: u32) -> Self {
        let cycle_s = cycle_time_us as f64 / 1_000_000.0;
        Self {
            role_closed: IoRole::GuardClosed(axis_id),
            role_locked: IoRole::GuardLocked(axis_id),
            secure_speed: config.secure_speed,
            open_delay_cycles: (config.open_delay / cycle_s).ceil() as u64,
            low_speed_cycles: 0,
        }
    }

    /// Evaluate guard safety.
    ///
    /// `current_speed`: absolute value of axis velocity [user units/s].
    ///
    /// Rules:
    /// - If speed > secure_speed: guard MUST be closed AND locked.
    /// - If speed ≤ secure_speed for < open_delay: guard MUST be closed AND locked.
    /// - If speed ≤ secure_speed for ≥ open_delay: guard may be open (ok=true).
    pub fn evaluate(
        &mut self,
        registry: &IoRegistry,
        di_bank: &[u64; 16],
        current_speed: f64,
    ) -> PeripheralResult {
        let closed = registry.read_di(&self.role_closed, di_bank).unwrap_or(false);
        let locked = registry.read_di(&self.role_locked, di_bank).unwrap_or(false);

        // Track low-speed duration.
        if current_speed.abs() <= self.secure_speed {
            self.low_speed_cycles = self.low_speed_cycles.saturating_add(1);
        } else {
            self.low_speed_cycles = 0;
        }

        // Determine if guard opening is permitted.
        let open_allowed = self.low_speed_cycles >= self.open_delay_cycles;

        if closed && locked {
            // Guard fully closed and locked — always ok.
            PeripheralResult::OK
        } else if open_allowed {
            // Speed has been below secure_speed long enough — guard may open.
            PeripheralResult::OK
        } else {
            // Guard is not closed/locked and speed is too high or delay not met.
            PeripheralResult::fault(PowerError::empty())
        }
    }

    /// Reset the low-speed timer.
    pub fn reset(&mut self) {
        self.low_speed_cycles = 0;
    }
}

// ─── Aggregate Peripheral Evaluator ─────────────────────────────────

/// Per-axis collection of all safety peripheral monitors.
///
/// Created at startup from axis config. Each optional peripheral
/// is `None` if not configured.
#[derive(Debug)]
pub struct AxisPeripherals {
    pub tailstock: Option<TailstockMonitor>,
    pub lock_pin: Option<LockPinMonitor>,
    pub brake: Option<BrakeMonitor>,
    pub guard: Option<GuardMonitor>,
}

/// Aggregate evaluation result for all peripherals on one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeripheralsEvaluation {
    pub tailstock_ok: bool,
    pub lock_pin_ok: bool,
    pub brake_ok: bool,
    pub guard_ok: bool,
    /// Accumulated power error flags from all peripherals.
    pub errors: PowerError,
}

impl PeripheralsEvaluation {
    /// All peripherals are safe.
    pub const fn all_ok(&self) -> bool {
        self.tailstock_ok && self.lock_pin_ok && self.brake_ok && self.guard_ok
    }
}

impl AxisPeripherals {
    /// Create peripheral monitors from axis configuration.
    pub fn from_config(
        tailstock: Option<&TailstockConfig>,
        index: Option<&IndexConfig>,
        brake: Option<&BrakeConfig>,
        guard: Option<&GuardConfig>,
        axis_id: u8,
        cycle_time_us: u32,
    ) -> Self {
        Self {
            tailstock: tailstock.and_then(|c| TailstockMonitor::new(c, axis_id)),
            lock_pin: index.map(|c| LockPinMonitor::new(c, axis_id, cycle_time_us)),
            brake: brake.map(|c| BrakeMonitor::new(c, axis_id, cycle_time_us)),
            guard: guard.map(|c| GuardMonitor::new(c, axis_id, cycle_time_us)),
        }
    }

    /// Evaluate all peripherals for this axis.
    ///
    /// `is_powered`: axis has power (Standby or Motion state).
    /// `current_speed`: absolute axis velocity for guard check.
    pub fn evaluate(
        &mut self,
        registry: &IoRegistry,
        di_bank: &[u64; 16],
        is_powered: bool,
        current_speed: f64,
    ) -> PeripheralsEvaluation {
        let mut errors = PowerError::empty();

        let tailstock_ok = match &self.tailstock {
            Some(monitor) => {
                let r = monitor.evaluate(registry, di_bank);
                errors |= r.errors;
                r.ok
            }
            None => true,
        };

        let lock_pin_ok = match &mut self.lock_pin {
            Some(monitor) => {
                let r = monitor.evaluate(registry, di_bank, is_powered);
                errors |= r.errors;
                r.ok
            }
            None => true,
        };

        let brake_ok = match &mut self.brake {
            Some(monitor) => {
                let r = monitor.evaluate(registry, di_bank, is_powered);
                errors |= r.errors;
                r.ok
            }
            None => true,
        };

        let guard_ok = match &mut self.guard {
            Some(monitor) => {
                let r = monitor.evaluate(registry, di_bank, current_speed);
                errors |= r.errors;
                r.ok
            }
            None => true,
        };

        PeripheralsEvaluation {
            tailstock_ok,
            lock_pin_ok,
            brake_ok,
            guard_ok,
            errors,
        }
    }

    /// Write brake DO command if brake is configured.
    pub fn write_brake_command(
        &self,
        registry: &IoRegistry,
        do_bank: &mut [u64; 16],
    ) {
        if let Some(ref brake) = self.brake {
            brake.write_command(registry, do_bank);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use evo_common::io::config::IoConfig;

    // Helper: create an empty IoRegistry for testing.
    fn empty_registry() -> IoRegistry {
        let cfg = IoConfig {
            groups: Default::default(),
        };
        IoRegistry::from_config(&cfg).unwrap()
    }

    fn empty_di_bank() -> [u64; 16] {
        [0u64; 16]
    }

    // ── Tailstock Tests ─────────────────────────────────────────────

    #[test]
    fn tailstock_type_none_returns_none() {
        let cfg = TailstockConfig {
            tailstock_type: TailstockType::None,
            di_closed: "TailClosed1".to_string(),
            closed_nc: false,
            di_open: "TailOpen1".to_string(),
            di_clamp_locked: None,
        };
        assert!(TailstockMonitor::new(&cfg, 1).is_none());
    }

    #[test]
    fn tailstock_standard_no_registry_binding_returns_fault() {
        let cfg = TailstockConfig {
            tailstock_type: TailstockType::Standard,
            di_closed: "TailClosed1".to_string(),
            closed_nc: false,
            di_open: "TailOpen1".to_string(),
            di_clamp_locked: None,
        };
        let monitor = TailstockMonitor::new(&cfg, 1).unwrap();
        let reg = empty_registry();
        let di = empty_di_bank();
        // No bindings registered → read_di returns None → defaults to false → not ok.
        let result = monitor.evaluate(&reg, &di);
        assert!(!result.ok);
        assert!(result.errors.contains(PowerError::DRIVE_TAIL_OPEN));
    }

    #[test]
    fn tailstock_combined_type_or_logic() {
        // Type 3: closed OR clamp_locked.
        let cfg = TailstockConfig {
            tailstock_type: TailstockType::Combined,
            di_closed: "TailClosed1".to_string(),
            closed_nc: false,
            di_open: "TailOpen1".to_string(),
            di_clamp_locked: Some("TailClamp1".to_string()),
        };
        let monitor = TailstockMonitor::new(&cfg, 1).unwrap();
        let reg = empty_registry();
        let di = empty_di_bank();
        // No bindings → both false → fault.
        let result = monitor.evaluate(&reg, &di);
        assert!(!result.ok);
    }

    // ── Locking Pin Tests ───────────────────────────────────────────

    #[test]
    fn lock_pin_unknown_returns_fault() {
        let cfg = IndexConfig {
            di_locked: "IndexLocked1".to_string(),
            di_middle: None,
            di_free: "IndexFree1".to_string(),
            retract_timeout: 3.0,
            insert_timeout: 3.0,
        };
        // With an empty registry, all DIs default to false → Unknown → fault.
        let mut monitor = LockPinMonitor::new(&cfg, 1, 1000);
        let reg = empty_registry();
        let di = empty_di_bank();
        let result = monitor.evaluate(&reg, &di, false);
        assert!(!result.ok);
    }

    #[test]
    fn lock_pin_retract_timeout() {
        let cfg = IndexConfig {
            di_locked: "IndexLocked1".to_string(),
            di_middle: None,
            di_free: "IndexFree1".to_string(),
            retract_timeout: 0.003, // 3ms = 3 cycles at 1ms
            insert_timeout: 3.0,
        };
        let mut monitor = LockPinMonitor::new(&cfg, 1, 1000);
        let reg = empty_registry();
        let di = empty_di_bank();
        monitor.start_retract();
        // Tick 3+ times → timeout.
        for _ in 0..3 {
            let _ = monitor.evaluate(&reg, &di, false);
        }
        let result = monitor.evaluate(&reg, &di, false);
        assert!(result.errors.contains(PowerError::LOCK_PIN_TIMEOUT));
    }

    // ── Brake Tests ─────────────────────────────────────────────────

    #[test]
    fn brake_always_free_returns_ok() {
        let cfg = BrakeConfig {
            do_brake: "BrakeOut1".to_string(),
            di_released: "BrakeIn1".to_string(),
            release_timeout: 2.0,
            engage_timeout: 1.0,
            always_free: true,
            inverted: false,
        };
        let mut monitor = BrakeMonitor::new(&cfg, 1, 1000);
        let reg = empty_registry();
        let di = empty_di_bank();
        let result = monitor.evaluate(&reg, &di, false);
        assert!(result.ok);
    }

    #[test]
    fn brake_release_timeout() {
        let cfg = BrakeConfig {
            do_brake: "BrakeOut1".to_string(),
            di_released: "BrakeIn1".to_string(),
            release_timeout: 0.002, // 2ms = 2 cycles at 1ms
            engage_timeout: 1.0,
            always_free: false,
            inverted: false,
        };
        let mut monitor = BrakeMonitor::new(&cfg, 1, 1000);
        let reg = empty_registry();
        let di = empty_di_bank();
        monitor.command_release();
        // No DI binding → read_di returns None → defaults to false (not released).
        for _ in 0..2 {
            let _ = monitor.evaluate(&reg, &di, false);
        }
        let result = monitor.evaluate(&reg, &di, false);
        assert!(result.errors.contains(PowerError::BRAKE_TIMEOUT));
    }

    // ── Guard Tests ─────────────────────────────────────────────────

    #[test]
    fn guard_low_speed_after_delay_allows_open() {
        let cfg = GuardConfig {
            di_closed: "GuardClosed1".to_string(),
            di_locked: "GuardLocked1".to_string(),
            secure_speed: 10.0,
            open_delay: 0.002, // 2ms = 2 cycles at 1ms
        };
        let mut monitor = GuardMonitor::new(&cfg, 1, 1000);
        let reg = empty_registry();
        let di = empty_di_bank();
        // Guard not closed (no binding), speed is 0 → must wait open_delay.
        let r1 = monitor.evaluate(&reg, &di, 0.0);
        assert!(!r1.ok); // delay not met yet
        let r2 = monitor.evaluate(&reg, &di, 0.0);
        assert!(r2.ok); // delay met → guard may open
    }

    #[test]
    fn guard_high_speed_requires_closed_locked() {
        let cfg = GuardConfig {
            di_closed: "GuardClosed1".to_string(),
            di_locked: "GuardLocked1".to_string(),
            secure_speed: 10.0,
            open_delay: 2.0,
        };
        let mut monitor = GuardMonitor::new(&cfg, 1, 1000);
        let reg = empty_registry();
        let di = empty_di_bank();
        // High speed, guard not registered → not closed → fault.
        let result = monitor.evaluate(&reg, &di, 50.0);
        assert!(!result.ok);
    }

    // ── Aggregate Tests ─────────────────────────────────────────────

    #[test]
    fn axis_peripherals_none_all_ok() {
        let mut periph = AxisPeripherals::from_config(None, None, None, None, 1, 1000);
        let reg = empty_registry();
        let di = empty_di_bank();
        let eval = periph.evaluate(&reg, &di, false, 0.0);
        assert!(eval.all_ok());
        assert!(eval.errors.is_empty());
    }

    #[test]
    fn peripheral_result_ok_constant() {
        let r = PeripheralResult::OK;
        assert!(r.ok);
        assert!(r.errors.is_empty());
    }
}
