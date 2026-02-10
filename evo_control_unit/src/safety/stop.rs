//! SAFETY_STOP detection and per-axis `SafeStopCategory` execution (T051, FR-121).
//!
//! STO: immediate disable + brake.
//! SS1: MaxDec → disable + brake.
//! SS2: MaxDec → hold torque.
//!
//! When SAFETY_STOP triggers, `MachineState` is forced to `SystemError`.

use evo_common::control_unit::safety::SafeStopConfig;
use evo_common::control_unit::state::SafeStopCategory;

/// Per-axis safe stop execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopPhase {
    /// Not in a safety stop.
    Idle,
    /// Decelerating to zero (SS1/SS2).
    Decelerating,
    /// Deceleration complete, waiting for brake delay (STO/SS1).
    WaitingBrake,
    /// Stop sequence complete — axis disabled and braked.
    Complete,
}

/// Per-axis SAFETY_STOP executor.
///
/// Drives the axis through the configured stop protocol
/// (STO, SS1, SS2) when a safety stop is triggered.
#[derive(Debug)]
pub struct SafeStopExecutor {
    /// Configured stop category.
    category: SafeStopCategory,
    /// Safe deceleration rate [user units/s²].
    max_decel_safe: f64,
    /// Delay before brake engagement after STO [cycles].
    sto_brake_delay_cycles: u64,
    /// Holding torque for SS2 [%].
    ss2_holding_torque: f64,
    /// Current phase of the stop sequence.
    phase: StopPhase,
    /// Cycles spent in current phase.
    phase_cycles: u64,
    /// Overall timeout for the stop sequence [cycles].
    timeout_cycles: u64,
    /// Total cycles elapsed since stop was triggered.
    elapsed_cycles: u64,
}

impl SafeStopExecutor {
    /// Create a new executor from config.
    pub fn new(
        config: &SafeStopConfig,
        cycle_time_us: u32,
        safety_stop_timeout: f64,
    ) -> Self {
        let cycle_s = cycle_time_us as f64 / 1_000_000.0;
        Self {
            category: config.category,
            max_decel_safe: config.max_decel_safe,
            sto_brake_delay_cycles: (config.sto_brake_delay / cycle_s).ceil() as u64,
            ss2_holding_torque: config.ss2_holding_torque,
            phase: StopPhase::Idle,
            phase_cycles: 0,
            timeout_cycles: (safety_stop_timeout / cycle_s).ceil() as u64,
            elapsed_cycles: 0,
        }
    }

    /// Current stop phase.
    #[inline]
    pub const fn phase(&self) -> StopPhase {
        self.phase
    }

    /// Whether the stop sequence is active (not Idle and not Complete).
    #[inline]
    pub const fn is_active(&self) -> bool {
        matches!(self.phase, StopPhase::Decelerating | StopPhase::WaitingBrake)
    }

    /// Whether the stop sequence has completed.
    #[inline]
    pub const fn is_complete(&self) -> bool {
        matches!(self.phase, StopPhase::Complete)
    }

    /// Configured stop category.
    #[inline]
    pub const fn category(&self) -> SafeStopCategory {
        self.category
    }

    /// SS2 holding torque percentage.
    #[inline]
    pub const fn ss2_holding_torque(&self) -> f64 {
        self.ss2_holding_torque
    }

    /// Trigger the safety stop sequence.
    ///
    /// Transitions from Idle to the first phase per category:
    /// - STO: immediate → WaitingBrake (no deceleration).
    /// - SS1/SS2: start Decelerating.
    pub fn trigger(&mut self) {
        if self.phase != StopPhase::Idle {
            return; // already active
        }

        self.elapsed_cycles = 0;
        self.phase_cycles = 0;

        self.phase = match self.category {
            SafeStopCategory::STO => StopPhase::WaitingBrake,
            SafeStopCategory::SS1 | SafeStopCategory::SS2 => StopPhase::Decelerating,
        };
    }

    /// Tick the executor one cycle.
    ///
    /// Returns a `StopAction` indicating what the cycle should do.
    pub fn tick(&mut self, current_speed: f64) -> StopAction {
        if self.phase == StopPhase::Idle || self.phase == StopPhase::Complete {
            return StopAction::None;
        }

        self.elapsed_cycles += 1;
        self.phase_cycles += 1;

        // Global timeout — force complete.
        if self.elapsed_cycles >= self.timeout_cycles {
            self.phase = StopPhase::Complete;
            return StopAction::DisableAndBrake;
        }

        match self.phase {
            StopPhase::Decelerating => {
                if current_speed.abs() < 0.01 {
                    // Axis has stopped.
                    match self.category {
                        SafeStopCategory::SS2 => {
                            // SS2: hold torque, sequence complete.
                            self.phase = StopPhase::Complete;
                            StopAction::HoldTorque(self.ss2_holding_torque)
                        }
                        SafeStopCategory::SS1 | SafeStopCategory::STO => {
                            // SS1: disable + brake.
                            self.phase = StopPhase::WaitingBrake;
                            self.phase_cycles = 0;
                            StopAction::DisableAndBrake
                        }
                    }
                } else {
                    // Continue decelerating.
                    StopAction::Decelerate(self.max_decel_safe)
                }
            }
            StopPhase::WaitingBrake => {
                if self.phase_cycles >= self.sto_brake_delay_cycles {
                    self.phase = StopPhase::Complete;
                    StopAction::DisableAndBrake
                } else {
                    StopAction::DisableAndBrake
                }
            }
            StopPhase::Idle | StopPhase::Complete => StopAction::None,
        }
    }

    /// Reset the executor back to Idle.
    pub fn reset(&mut self) {
        self.phase = StopPhase::Idle;
        self.phase_cycles = 0;
        self.elapsed_cycles = 0;
    }
}

/// Action to perform this cycle during a safety stop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StopAction {
    /// No action (idle or complete).
    None,
    /// Decelerate at the given rate [user units/s²].
    Decelerate(f64),
    /// Disable drive and engage brake.
    DisableAndBrake,
    /// Hold position with given torque percentage (SS2).
    HoldTorque(f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config(category: SafeStopCategory) -> SafeStopConfig {
        SafeStopConfig {
            category,
            max_decel_safe: 10000.0,
            sto_brake_delay: 0.002, // 2ms = 2 cycles at 1ms
            ss2_holding_torque: 20.0,
        }
    }

    #[test]
    fn sto_immediate_disable_and_brake() {
        let cfg = default_config(SafeStopCategory::STO);
        let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);
        assert_eq!(exec.phase(), StopPhase::Idle);

        exec.trigger();
        assert_eq!(exec.phase(), StopPhase::WaitingBrake);

        // Tick past brake delay.
        let a1 = exec.tick(0.0);
        assert_eq!(a1, StopAction::DisableAndBrake);
        let a2 = exec.tick(0.0);
        assert_eq!(a2, StopAction::DisableAndBrake);
        assert_eq!(exec.phase(), StopPhase::Complete);
    }

    #[test]
    fn ss1_decelerate_then_disable() {
        let cfg = default_config(SafeStopCategory::SS1);
        let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);
        exec.trigger();
        assert_eq!(exec.phase(), StopPhase::Decelerating);

        // Still moving → decelerate.
        let a1 = exec.tick(100.0);
        assert!(matches!(a1, StopAction::Decelerate(_)));

        // Speed reaches zero.
        let a2 = exec.tick(0.0);
        assert_eq!(a2, StopAction::DisableAndBrake);
        assert_eq!(exec.phase(), StopPhase::WaitingBrake);
    }

    #[test]
    fn ss2_decelerate_then_hold_torque() {
        let cfg = default_config(SafeStopCategory::SS2);
        let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);
        exec.trigger();

        // Decelerate.
        let _ = exec.tick(50.0);
        // Stop.
        let a = exec.tick(0.0);
        assert_eq!(a, StopAction::HoldTorque(20.0));
        assert_eq!(exec.phase(), StopPhase::Complete);
    }

    #[test]
    fn timeout_forces_complete() {
        let cfg = default_config(SafeStopCategory::SS1);
        // Very short timeout: 2ms.
        let mut exec = SafeStopExecutor::new(&cfg, 1000, 0.002);
        exec.trigger();

        // Speed never reaches zero but timeout expires.
        let _ = exec.tick(100.0);
        let a = exec.tick(100.0);
        assert_eq!(a, StopAction::DisableAndBrake);
        assert_eq!(exec.phase(), StopPhase::Complete);
    }

    #[test]
    fn double_trigger_is_noop() {
        let cfg = default_config(SafeStopCategory::SS1);
        let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);
        exec.trigger();
        assert_eq!(exec.phase(), StopPhase::Decelerating);
        exec.trigger(); // second trigger
        assert_eq!(exec.phase(), StopPhase::Decelerating);
    }

    #[test]
    fn reset_returns_to_idle() {
        let cfg = default_config(SafeStopCategory::STO);
        let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);
        exec.trigger();
        exec.reset();
        assert_eq!(exec.phase(), StopPhase::Idle);
    }
}
