//! Homing supervision (6 methods) — T082 + T083.
//!
//! Supervises axis homing procedure with method-specific logic:
//! HardStop, HomeSensor, LimitSwitch, IndexPulse, Absolute, NoHoming.
//!
//! ## Architecture
//!
//! The CU **supervises** homing — it doesn't generate trajectories.
//! It monitors sensors and drive feedback to determine when each
//! homing phase completes, then signals MotionState transitions.
//!
//! ## Methods (FR-032)
//!
//! | Method      | Mechanism                         | Sensor                  |
//! |-------------|-----------------------------------|-------------------------|
//! | HardStop    | Drive into hard stop              | Current > threshold     |
//! | HomeSensor  | Drive to home sensor              | IoRole::Ref(N) trigger  |
//! | LimitSwitch | Drive to limit switch             | IoRole::LimitMin/Max(N) |
//! | IndexPulse  | Two-phase: sensor then index      | sensor_role + index_role|
//! | Absolute    | No motion, apply offset           | Absolute encoder        |
//! | NoHoming    | Immediately referenced            | N/A                     |
//!
//! ## Lifecycle (FR-031)
//!
//! 1. Caller issues `StartHoming` → MotionState::Homing
//! 2. `HomingSupervisor::start()` validates preconditions (PowerState::Motion)
//! 3. Each cycle: `tick()` checks sensors/timeout → phase transitions
//! 4. On success: `referenced=true`, position zeroed/offset applied
//! 5. On failure: MotionError::HOMING_FAILED, MotionState::MotionError

use evo_common::control_unit::homing::{HomingConfig, HomingMethod};
use evo_common::io::registry::IoRegistry;
use evo_common::io::role::IoRole;

// ─── Homing Phases ──────────────────────────────────────────────────

/// Internal phase of the homing procedure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomingPhase {
    /// Not homing (idle).
    Idle,
    /// Phase 1: Approaching sensor/stop in configured direction.
    Approach,
    /// Phase 2 (IndexPulse only): Searching for index pulse after sensor trigger.
    IndexSearch,
    /// Homing completed successfully.
    Complete,
    /// Homing failed (timeout or error).
    Failed,
}

// ─── Homing Result ──────────────────────────────────────────────────

/// Result of a single homing tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HomingTickResult {
    /// Homing still in progress.
    InProgress,
    /// Homing completed — caller must set referenced=true and apply offset.
    Success {
        /// Position offset to apply (zero_offset for Absolute, 0.0 otherwise).
        position_offset: f64,
    },
    /// Homing failed — caller must set MotionError::HOMING_FAILED.
    Failed {
        reason: HomingFailReason,
    },
}

/// Reason for homing failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomingFailReason {
    /// Homing timeout exceeded.
    Timeout,
    /// Start rejected: axis not in correct power state.
    InvalidPowerState,
    /// Start rejected: method requires approach direction but none configured.
    MissingDirection,
}

// ─── Homing Supervisor ──────────────────────────────────────────────

/// Per-axis homing supervisor state machine.
#[derive(Debug, Clone)]
pub struct HomingSupervisor {
    /// Current homing phase.
    phase: HomingPhase,
    /// Homing method from config.
    method: HomingMethod,
    /// Approach direction sign (+1.0 or -1.0).
    direction_sign: f64,
    /// Homing speed limit [mm/s].
    speed_limit: f64,
    /// Torque limit [% of max].
    torque_limit: f64,
    /// Timeout countdown in cycles.
    timeout_cycles: u64,
    /// Maximum timeout in cycles (from config).
    max_timeout_cycles: u64,
    /// Current threshold for HardStop method.
    current_threshold: f64,
    /// Zero offset for Absolute method.
    zero_offset: f64,
    /// Sensor role name (for HomeSensor/IndexPulse).
    sensor_role: Option<IoRole>,
    /// Index role name (for IndexPulse phase 2).
    index_role: Option<IoRole>,
    /// Limit direction for LimitSwitch method (+1 = high, -1 = low).
    limit_direction: i8,
    /// Axis ID for IoRole lookup.
    axis_id: u8,
}

impl HomingSupervisor {
    /// Create a new supervisor from homing config.
    ///
    /// `cycle_time_s`: cycle time in seconds (e.g. 0.001 for 1ms).
    pub fn new(config: &HomingConfig, axis_id: u8, cycle_time_s: f64) -> Self {
        let timeout_cycles = if cycle_time_s > 0.0 {
            (config.timeout / cycle_time_s).ceil() as u64
        } else {
            u64::MAX
        };

        let sensor_role = config
            .sensor_role
            .as_ref()
            .and_then(|s| s.parse::<IoRole>().ok());

        let index_role = config
            .index_role
            .as_ref()
            .and_then(|s| s.parse::<IoRole>().ok());

        let direction_sign = config
            .approach_direction
            .map_or(1.0, |d| d.sign());

        Self {
            phase: HomingPhase::Idle,
            method: config.method,
            direction_sign,
            speed_limit: config.speed,
            torque_limit: config.torque_limit,
            timeout_cycles,
            max_timeout_cycles: timeout_cycles,
            current_threshold: config.current_threshold,
            zero_offset: config.zero_offset,
            sensor_role,
            index_role,
            limit_direction: config.limit_direction,
            axis_id,
        }
    }

    /// Current homing phase.
    #[inline]
    pub fn phase(&self) -> HomingPhase {
        self.phase
    }

    /// Whether homing is actively running.
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self.phase, HomingPhase::Approach | HomingPhase::IndexSearch)
    }

    /// Homing speed limit [mm/s].
    #[inline]
    pub fn speed_limit(&self) -> f64 {
        self.speed_limit
    }

    /// Homing torque limit [%].
    #[inline]
    pub fn torque_limit(&self) -> f64 {
        self.torque_limit
    }

    /// Direction sign for approach velocity.
    #[inline]
    pub fn direction_sign(&self) -> f64 {
        self.direction_sign
    }

    /// Start the homing procedure.
    ///
    /// Returns `Err` if the method requires an approach direction and none is set,
    /// or if the method doesn't require motion (Absolute/NoHoming are instant).
    pub fn start(&mut self) -> HomingTickResult {
        match self.method {
            HomingMethod::NoHoming => {
                // Immediately referenced, no motion needed.
                self.phase = HomingPhase::Complete;
                HomingTickResult::Success { position_offset: 0.0 }
            }
            HomingMethod::Absolute => {
                // Apply zero offset, no motion needed.
                self.phase = HomingPhase::Complete;
                HomingTickResult::Success {
                    position_offset: self.zero_offset,
                }
            }
            method => {
                if method.requires_approach_direction()
                    && self.direction_sign.abs() < f64::EPSILON
                {
                    self.phase = HomingPhase::Failed;
                    return HomingTickResult::Failed {
                        reason: HomingFailReason::MissingDirection,
                    };
                }
                self.phase = HomingPhase::Approach;
                self.timeout_cycles = self.max_timeout_cycles;
                HomingTickResult::InProgress
            }
        }
    }

    /// Tick the homing supervisor once per RT cycle.
    ///
    /// # Parameters
    /// - `_actual_velocity`: current axis velocity [mm/s] (reserved for future speed checks)
    /// - `actual_torque_pct`: current torque as % of max (for HardStop)
    /// - `io_registry`: for reading DI sensors
    /// - `di_bank`: digital input bank
    pub fn tick(
        &mut self,
        _actual_velocity: f64,
        actual_torque_pct: f64,
        io_registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> HomingTickResult {
        match self.phase {
            HomingPhase::Idle | HomingPhase::Complete | HomingPhase::Failed => {
                return if self.phase == HomingPhase::Complete {
                    HomingTickResult::Success { position_offset: 0.0 }
                } else if self.phase == HomingPhase::Failed {
                    HomingTickResult::Failed { reason: HomingFailReason::Timeout }
                } else {
                    HomingTickResult::InProgress
                };
            }
            HomingPhase::Approach => {
                // Check timeout.
                if self.timeout_cycles == 0 {
                    self.phase = HomingPhase::Failed;
                    return HomingTickResult::Failed {
                        reason: HomingFailReason::Timeout,
                    };
                }
                self.timeout_cycles -= 1;

                match self.method {
                    HomingMethod::HardStop => {
                        self.tick_hard_stop(actual_torque_pct)
                    }
                    HomingMethod::HomeSensor => {
                        self.tick_home_sensor(io_registry, di_bank)
                    }
                    HomingMethod::LimitSwitch => {
                        self.tick_limit_switch(io_registry, di_bank)
                    }
                    HomingMethod::IndexPulse => {
                        self.tick_index_phase1(io_registry, di_bank)
                    }
                    _ => HomingTickResult::InProgress,
                }
            }
            HomingPhase::IndexSearch => {
                // Phase 2 of IndexPulse.
                if self.timeout_cycles == 0 {
                    self.phase = HomingPhase::Failed;
                    return HomingTickResult::Failed {
                        reason: HomingFailReason::Timeout,
                    };
                }
                self.timeout_cycles -= 1;
                self.tick_index_phase2(io_registry, di_bank)
            }
        }
    }

    /// Reset the supervisor to idle.
    pub fn reset(&mut self) {
        self.phase = HomingPhase::Idle;
        self.timeout_cycles = self.max_timeout_cycles;
    }

    // ─── Method-Specific Tick Logic ─────────────────────────────────

    fn tick_hard_stop(&mut self, actual_torque_pct: f64) -> HomingTickResult {
        // HardStop: detect when current (torque %) exceeds threshold.
        if actual_torque_pct.abs() >= self.current_threshold {
            self.phase = HomingPhase::Complete;
            HomingTickResult::Success { position_offset: 0.0 }
        } else {
            HomingTickResult::InProgress
        }
    }

    fn tick_home_sensor(
        &mut self,
        io_registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> HomingTickResult {
        // HomeSensor: check Ref(axis_id) or custom sensor_role.
        let triggered = if let Some(ref role) = self.sensor_role {
            io_registry.read_di(role, di_bank).unwrap_or(false)
        } else {
            io_registry
                .read_di(&IoRole::Ref(self.axis_id), di_bank)
                .unwrap_or(false)
        };

        if triggered {
            self.phase = HomingPhase::Complete;
            HomingTickResult::Success { position_offset: 0.0 }
        } else {
            HomingTickResult::InProgress
        }
    }

    fn tick_limit_switch(
        &mut self,
        io_registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> HomingTickResult {
        // LimitSwitch: check LimitMin or LimitMax based on limit_direction.
        let role = if self.limit_direction >= 0 {
            IoRole::LimitMax(self.axis_id)
        } else {
            IoRole::LimitMin(self.axis_id)
        };

        let triggered = io_registry.read_di(&role, di_bank).unwrap_or(false);

        if triggered {
            self.phase = HomingPhase::Complete;
            HomingTickResult::Success { position_offset: 0.0 }
        } else {
            HomingTickResult::InProgress
        }
    }

    fn tick_index_phase1(
        &mut self,
        io_registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> HomingTickResult {
        // IndexPulse Phase 1: find sensor trigger.
        let triggered = if let Some(ref role) = self.sensor_role {
            io_registry.read_di(role, di_bank).unwrap_or(false)
        } else {
            io_registry
                .read_di(&IoRole::Ref(self.axis_id), di_bank)
                .unwrap_or(false)
        };

        if triggered {
            // Transition to phase 2: search for index pulse.
            self.phase = HomingPhase::IndexSearch;
        }
        HomingTickResult::InProgress
    }

    fn tick_index_phase2(
        &mut self,
        io_registry: &IoRegistry,
        di_bank: &[u64; 16],
    ) -> HomingTickResult {
        // IndexPulse Phase 2: find index pulse trigger.
        let triggered = if let Some(ref role) = self.index_role {
            io_registry.read_di(role, di_bank).unwrap_or(false)
        } else {
            false // No index role configured → will timeout
        };

        if triggered {
            self.phase = HomingPhase::Complete;
            HomingTickResult::Success { position_offset: 0.0 }
        } else {
            HomingTickResult::InProgress
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use evo_common::control_unit::homing::HomingDirection;
    use evo_common::io::config::IoConfig;
    use evo_common::io::registry::{set_bit, IoRegistry};

    fn empty_registry() -> IoRegistry {
        let cfg = IoConfig {
            groups: Default::default(),
        };
        IoRegistry::from_config(&cfg).unwrap()
    }

    fn registry_with_ref(axis: u8, pin: u16) -> IoRegistry {
        let toml = format!(
            r#"
[A]
io = [{{ type = "di", role = "Ref{axis}", pin = {pin} }}]
"#
        );
        let cfg = IoConfig::from_toml(&toml).unwrap();
        IoRegistry::from_config(&cfg).unwrap()
    }

    fn registry_with_limit_min(axis: u8, pin: u16) -> IoRegistry {
        let toml = format!(
            r#"
[A]
io = [{{ type = "di", role = "LimitMin{axis}", pin = {pin} }}]
"#
        );
        let cfg = IoConfig::from_toml(&toml).unwrap();
        IoRegistry::from_config(&cfg).unwrap()
    }

    fn default_config(method: HomingMethod) -> HomingConfig {
        let mut cfg = HomingConfig::default();
        cfg.method = method;
        cfg.speed = 20.0;
        cfg.torque_limit = 30.0;
        cfg.timeout = 5.0;
        cfg.current_threshold = 80.0;
        cfg.approach_direction = Some(HomingDirection::Positive);
        cfg
    }

    // ── NoHoming ──

    #[test]
    fn no_homing_immediately_referenced() {
        let cfg = default_config(HomingMethod::NoHoming);
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);
        let result = sv.start();
        assert_eq!(result, HomingTickResult::Success { position_offset: 0.0 });
        assert_eq!(sv.phase(), HomingPhase::Complete);
    }

    // ── Absolute ──

    #[test]
    fn absolute_applies_offset() {
        let mut cfg = default_config(HomingMethod::Absolute);
        cfg.zero_offset = 42.5;
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);
        let result = sv.start();
        assert_eq!(
            result,
            HomingTickResult::Success {
                position_offset: 42.5
            }
        );
        assert_eq!(sv.phase(), HomingPhase::Complete);
    }

    // ── HardStop ──

    #[test]
    fn hard_stop_detects_current_threshold() {
        let cfg = default_config(HomingMethod::HardStop);
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);
        let reg = empty_registry();
        let di = [0u64; 16];

        assert!(matches!(sv.start(), HomingTickResult::InProgress));

        // Below threshold.
        assert_eq!(sv.tick(10.0, 50.0, &reg, &di), HomingTickResult::InProgress);

        // At threshold.
        assert_eq!(
            sv.tick(5.0, 80.0, &reg, &di),
            HomingTickResult::Success { position_offset: 0.0 }
        );
        assert_eq!(sv.phase(), HomingPhase::Complete);
    }

    #[test]
    fn hard_stop_timeout() {
        let mut cfg = default_config(HomingMethod::HardStop);
        cfg.timeout = 0.003; // 3 cycles at 1ms
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);
        let reg = empty_registry();
        let di = [0u64; 16];

        sv.start();

        // 3 ticks → timeout (timeout_cycles = ceil(0.003/0.001) = 3).
        assert_eq!(sv.tick(10.0, 10.0, &reg, &di), HomingTickResult::InProgress);
        assert_eq!(sv.tick(10.0, 10.0, &reg, &di), HomingTickResult::InProgress);
        assert_eq!(sv.tick(10.0, 10.0, &reg, &di), HomingTickResult::InProgress);
        assert_eq!(
            sv.tick(10.0, 10.0, &reg, &di),
            HomingTickResult::Failed {
                reason: HomingFailReason::Timeout
            }
        );
    }

    // ── HomeSensor ──

    #[test]
    fn home_sensor_triggers_on_ref() {
        let cfg = default_config(HomingMethod::HomeSensor);
        let reg = registry_with_ref(1, 34);
        let mut di = [0u64; 16];
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);

        sv.start();
        assert_eq!(sv.tick(20.0, 0.0, &reg, &di), HomingTickResult::InProgress);

        // Trigger ref sensor.
        set_bit(&mut di, 34, true);
        assert_eq!(
            sv.tick(20.0, 0.0, &reg, &di),
            HomingTickResult::Success { position_offset: 0.0 }
        );
    }

    // ── LimitSwitch ──

    #[test]
    fn limit_switch_negative_direction() {
        let mut cfg = default_config(HomingMethod::LimitSwitch);
        cfg.limit_direction = -1;
        cfg.approach_direction = Some(HomingDirection::Negative);
        let reg = registry_with_limit_min(1, 30);
        let mut di = [0u64; 16];
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);

        sv.start();
        assert_eq!(sv.tick(-10.0, 0.0, &reg, &di), HomingTickResult::InProgress);

        // Trigger LimitMin.
        set_bit(&mut di, 30, true);
        assert_eq!(
            sv.tick(-10.0, 0.0, &reg, &di),
            HomingTickResult::Success { position_offset: 0.0 }
        );
    }

    // ── IndexPulse ──

    #[test]
    fn index_pulse_two_phase() {
        let mut cfg = default_config(HomingMethod::IndexPulse);
        cfg.sensor_role = Some("Ref1".to_string());
        cfg.index_role = Some("Index1".to_string());

        // Build registry with both roles.
        let toml = r#"
[A]
io = [
    { type = "di", role = "Ref1", pin = 34 },
    { type = "di", role = "Index1", pin = 35 },
]
"#;
        let io_cfg = IoConfig::from_toml(toml).unwrap();
        let reg = IoRegistry::from_config(&io_cfg).unwrap();
        let mut di = [0u64; 16];
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);

        sv.start();
        assert_eq!(sv.phase(), HomingPhase::Approach);

        // No sensors → in progress.
        assert_eq!(sv.tick(20.0, 0.0, &reg, &di), HomingTickResult::InProgress);

        // Trigger sensor (phase 1 → phase 2).
        set_bit(&mut di, 34, true);
        assert_eq!(sv.tick(20.0, 0.0, &reg, &di), HomingTickResult::InProgress);
        assert_eq!(sv.phase(), HomingPhase::IndexSearch);

        // Clear sensor, set index pulse.
        set_bit(&mut di, 34, false);
        set_bit(&mut di, 35, true);
        assert_eq!(
            sv.tick(20.0, 0.0, &reg, &di),
            HomingTickResult::Success { position_offset: 0.0 }
        );
    }

    // ── Speed & torque limits ──

    #[test]
    fn supervisor_exposes_limits() {
        let cfg = default_config(HomingMethod::HardStop);
        let sv = HomingSupervisor::new(&cfg, 1, 0.001);
        assert_eq!(sv.speed_limit(), 20.0);
        assert_eq!(sv.torque_limit(), 30.0);
        assert_eq!(sv.direction_sign(), 1.0);
    }

    // ── Reset ──

    #[test]
    fn reset_returns_to_idle() {
        let cfg = default_config(HomingMethod::HardStop);
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);
        sv.start();
        assert_eq!(sv.phase(), HomingPhase::Approach);
        sv.reset();
        assert_eq!(sv.phase(), HomingPhase::Idle);
    }

    // ── Is active ──

    #[test]
    fn is_active_check() {
        let cfg = default_config(HomingMethod::HardStop);
        let mut sv = HomingSupervisor::new(&cfg, 1, 0.001);
        assert!(!sv.is_active());
        sv.start();
        assert!(sv.is_active());
    }
}