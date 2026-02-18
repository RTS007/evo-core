//! CouplingState transitions, synchronization, and error propagation (T054–T058).
//!
//! Master-slave coupling with bottom-up synchronization, same-cycle
//! SYNCHRONIZED transition, and fault cascade through coupling chains.
//!
//! ## State Machine Transitions (FR-050)
//!
//! ```text
//! Uncoupled → Coupling → Master | WaitingSync
//! WaitingSync → SlaveCoupled | SlaveModulated | SyncLost
//! any_coupled → Decoupling → Uncoupled
//! Master → Decoupling (on master fault → cascade all slaves)
//! Synchronized → SyncLost (lag diff exceeded)
//! SyncLost → WaitingSync (re-sync command)
//! ```

use evo_common::consts::MAX_AXES;
use evo_common::control_unit::error::CouplingError;
use evo_common::control_unit::state::{CouplingConfig, CouplingState, MotionState, PowerState};

// ─── Coupling Events ────────────────────────────────────────────────

/// Events that drive the CouplingState machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CouplingEvent {
    /// Couple command received (from RPC/RE).
    CoupleAsmaster,
    /// Couple command: this axis becomes a slave.
    CoupleAsSlave,
    /// Synchronization condition met (position/velocity within tolerance).
    SyncAchieved,
    /// Synchronization timeout expired.
    SyncTimeout,
    /// Synchronization lost (lag diff exceeded).
    SyncLost,
    /// Decouple command received.
    Decouple,
    /// All slaves acknowledged decoupling.
    DecoupleComplete,
    /// Re-sync command after SyncLost.
    Resync,
    /// Master fault: force decouple all slaves.
    MasterFault,
}

/// Transition result for coupling state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CouplingTransition {
    Ok(CouplingState),
    Rejected(&'static str),
}

// ─── T054: Coupling State Machine ───────────────────────────────────

/// Per-axis coupling state machine.
///
/// Manages the coupling lifecycle including coupling, synchronization,
/// and decoupling transitions.
#[derive(Debug, Clone)]
pub struct CouplingStateMachine {
    state: CouplingState,
    /// Sync timeout tracking [cycles].
    sync_wait_cycles: u64,
    /// Configured sync timeout [cycles].
    sync_timeout_cycles: u64,
    /// Whether this is a modulated coupling.
    is_modulated: bool,
}

impl CouplingStateMachine {
    /// Create a new coupling state machine with optional config.
    pub fn new(config: Option<&CouplingConfig>, cycle_time_us: u32) -> Self {
        let cycle_s = cycle_time_us as f64 / 1_000_000.0;
        let (timeout_cycles, is_modulated) = match config {
            Some(c) => (
                (c.sync_timeout / cycle_s).ceil() as u64,
                c.modulation_offset.abs() > f64::EPSILON,
            ),
            None => (5000, false),
        };
        Self {
            state: CouplingState::Uncoupled,
            sync_wait_cycles: 0,
            sync_timeout_cycles: timeout_cycles,
            is_modulated,
        }
    }

    #[inline]
    pub const fn state(&self) -> CouplingState {
        self.state
    }

    /// Handle a coupling event.
    ///
    /// Guards (I-CP-4): coupling blocked if axis is in MotionError or PowerError.
    pub fn handle_event(
        &mut self,
        event: CouplingEvent,
        power_state: PowerState,
        motion_state: MotionState,
    ) -> CouplingTransition {
        use CouplingEvent as E;
        use CouplingState as C;

        // Guard: coupling/sync blocked during error states (I-CP-4).
        if matches!(event, E::CoupleAsmaster | E::CoupleAsSlave | E::Resync)
            && (power_state == PowerState::PowerError
                || motion_state == MotionState::MotionError)
        {
            return CouplingTransition::Rejected("coupling blocked during error state (I-CP-4)");
        }

        // Guard: must be standstill for coupling (I-CP-4).
        if matches!(event, E::CoupleAsmaster | E::CoupleAsSlave)
            && motion_state != MotionState::Standstill
        {
            return CouplingTransition::Rejected("coupling requires standstill (I-CP-4)");
        }

        let next = match (self.state, event) {
            // Uncoupled → Coupling
            (C::Uncoupled, E::CoupleAsmaster) => {
                C::Master
            }
            (C::Uncoupled, E::CoupleAsSlave) => {
                self.sync_wait_cycles = 0;
                C::WaitingSync
            }

            // WaitingSync → Slave states
            (C::WaitingSync, E::SyncAchieved) => {
                if self.is_modulated {
                    C::SlaveModulated
                } else {
                    C::SlaveCoupled
                }
            }
            (C::WaitingSync, E::SyncTimeout) => C::SyncLost,

            // Synchronized → SyncLost
            (C::SlaveCoupled | C::SlaveModulated, E::SyncLost) => C::SyncLost,

            // SyncLost → WaitingSync (re-sync)
            (C::SyncLost, E::Resync) => {
                self.sync_wait_cycles = 0;
                C::WaitingSync
            }

            // any_coupled → Decoupling
            (s, E::Decouple) if s.is_coupled() => C::Decoupling,

            // Master fault → Decoupling
            (C::Master, E::MasterFault) => C::Decoupling,

            // Decoupling → Uncoupled
            (C::Decoupling, E::DecoupleComplete) => C::Uncoupled,

            _ => {
                return CouplingTransition::Rejected("invalid coupling transition");
            }
        };

        self.state = next;
        CouplingTransition::Ok(next)
    }

    /// Force decouple (called by error propagation).
    pub fn force_decouple(&mut self) {
        self.state = CouplingState::Uncoupled;
        self.sync_wait_cycles = 0;
    }

    /// Force to SyncLost (called by lag diff exceed).
    pub fn force_sync_lost(&mut self) {
        if self.state.is_coupled() {
            self.state = CouplingState::SyncLost;
        }
    }

    /// Tick sync timeout for WaitingSync state.
    ///
    /// Returns true if sync timeout has expired.
    pub fn tick_sync_timeout(&mut self) -> bool {
        if self.state == CouplingState::WaitingSync {
            self.sync_wait_cycles += 1;
            self.sync_wait_cycles >= self.sync_timeout_cycles
        } else {
            false
        }
    }
}

// ─── T055: Bottom-Up Synchronization (FR-052) ──────────────────────

/// Per-axis sync readiness flags.
const MAX_AXES_USIZE: usize = MAX_AXES as usize;

/// Check if all direct slaves of a master have reached sync.
///
/// A slave is "sync-ready" if its CouplingState is SlaveCoupled or SlaveModulated.
pub fn all_slaves_synced(
    master_id: u8,
    axis_configs: &[(u8, Option<&CouplingConfig>)],
    coupling_states: &[CouplingState],
) -> bool {
    for (axis_id, config) in axis_configs {
        if let Some(cfg) = config {
            if cfg.master_axis == Some(master_id) {
                let state = coupling_states.get(*axis_id as usize).copied().unwrap_or(CouplingState::Uncoupled);
                if !matches!(state, CouplingState::SlaveCoupled | CouplingState::SlaveModulated) {
                    return false;
                }
            }
        }
    }
    true
}

/// Execute bottom-up synchronization for one cycle.
///
/// For each axis in WaitingSync, checks if position/velocity is within
/// sync tolerance. Returns a list of axes that achieved sync this cycle.
///
/// `sync_ready[axis_id]` should be set by the caller based on position/velocity check.
pub fn process_bottom_up_sync(
    sync_ready: &[bool; MAX_AXES_USIZE],
    coupling_machines: &mut [CouplingStateMachine],
    axis_count: usize,
) -> heapless::Vec<u8, 64> {
    let mut newly_synced = heapless::Vec::<u8, 64>::new();

    for i in 0..axis_count {
        if coupling_machines[i].state() == CouplingState::WaitingSync && sync_ready[i] {
            // This axis meets sync conditions — transition to coupled.
            let ps = PowerState::Standby; // sync happens while powered
            let ms = MotionState::Standstill;
            let _ = coupling_machines[i].handle_event(CouplingEvent::SyncAchieved, ps, ms);
            let _ = newly_synced.push(i as u8);
        }
    }

    newly_synced
}

// ─── T056: Slave Position Calculation (FR-051) ──────────────────────

/// Calculate slave target position from master position.
///
/// `SLAVE_COUPLED`: `target = master_pos × ratio`
/// `SLAVE_MODULATED`: `target = master_pos × ratio + offset`
#[inline]
pub fn calculate_slave_position(
    master_position: f64,
    coupling_ratio: f64,
    modulation_offset: f64,
    is_modulated: bool,
) -> f64 {
    let base = master_position * coupling_ratio;
    if is_modulated {
        base + modulation_offset
    } else {
        base
    }
}

// ─── T058: Lag Difference Monitoring (FR-104) ───────────────────────

/// Check master-slave lag difference.
///
/// Returns `true` if `|master_lag - slave_lag| > max_lag_diff`.
/// This is a CRITICAL condition → triggers SAFETY_STOP for all coupled axes.
#[inline]
pub fn check_lag_difference(
    master_lag: f64,
    slave_lag: f64,
    max_lag_diff: f64,
) -> bool {
    (master_lag - slave_lag).abs() > max_lag_diff
}

/// Per-axis coupling runtime state.
///
/// Combines the coupling state machine with runtime coupling data.
#[derive(Debug)]
pub struct AxisCouplingRuntime {
    /// State machine.
    pub machine: CouplingStateMachine,
    /// Coupling config (from loaded config).
    pub config: CouplingConfig,
    /// Accumulated coupling errors.
    pub errors: CouplingError,
}

impl AxisCouplingRuntime {
    /// Create from config.
    pub fn new(config: CouplingConfig, cycle_time_us: u32) -> Self {
        let machine = CouplingStateMachine::new(Some(&config), cycle_time_us);
        Self {
            machine,
            config,
            errors: CouplingError::empty(),
        }
    }

    /// Evaluate coupling for this axis in one cycle.
    ///
    /// Checks sync timeout and lag difference.
    pub fn evaluate_cycle(
        &mut self,
        master_lag: Option<f64>,
        slave_lag: f64,
    ) {
        // Check sync timeout.
        if self.machine.tick_sync_timeout() {
            let ps = PowerState::Standby;
            let ms = MotionState::Standstill;
            let _ = self.machine.handle_event(CouplingEvent::SyncTimeout, ps, ms);
            self.errors |= CouplingError::SYNC_TIMEOUT;
        }

        // Check lag difference (FR-104) — only for active slave.
        if let Some(m_lag) = master_lag {
            if self.machine.state().is_slave() {
                if check_lag_difference(m_lag, slave_lag, self.config.max_lag_diff) {
                    self.machine.force_sync_lost();
                    self.errors |= CouplingError::LAG_DIFF_EXCEED;
                }
            }
        }
    }

    /// Clear coupling errors (after recovery).
    pub fn clear_errors(&mut self) {
        self.errors = CouplingError::empty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn default_config() -> CouplingConfig {
        CouplingConfig::default()
    }

    fn slave_config(master: u8) -> CouplingConfig {
        CouplingConfig {
            master_axis: Some(master),
            ..Default::default()
        }
    }

    fn modulated_slave_config(master: u8) -> CouplingConfig {
        CouplingConfig {
            master_axis: Some(master),
            modulation_offset: 5.0,
            ..Default::default()
        }
    }

    // ── T054: State Machine Tests ───────────────────────────────────

    #[test]
    fn initial_state_is_uncoupled() {
        let sm = CouplingStateMachine::new(None, 1000);
        assert_eq!(sm.state(), CouplingState::Uncoupled);
    }

    #[test]
    fn couple_as_master() {
        let mut sm = CouplingStateMachine::new(None, 1000);
        let r = sm.handle_event(
            CouplingEvent::CoupleAsmaster,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(r, CouplingTransition::Ok(CouplingState::Master));
    }

    #[test]
    fn couple_as_slave_goes_to_waiting_sync() {
        let cfg = slave_config(1);
        let mut sm = CouplingStateMachine::new(Some(&cfg), 1000);
        let r = sm.handle_event(
            CouplingEvent::CoupleAsSlave,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(r, CouplingTransition::Ok(CouplingState::WaitingSync));
    }

    #[test]
    fn sync_achieved_becomes_slave_coupled() {
        let cfg = slave_config(1);
        let mut sm = CouplingStateMachine::new(Some(&cfg), 1000);
        sm.handle_event(
            CouplingEvent::CoupleAsSlave,
            PowerState::Standby,
            MotionState::Standstill,
        );
        let r = sm.handle_event(
            CouplingEvent::SyncAchieved,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(r, CouplingTransition::Ok(CouplingState::SlaveCoupled));
    }

    #[test]
    fn sync_achieved_modulated() {
        let cfg = modulated_slave_config(1);
        let mut sm = CouplingStateMachine::new(Some(&cfg), 1000);
        sm.handle_event(
            CouplingEvent::CoupleAsSlave,
            PowerState::Standby,
            MotionState::Standstill,
        );
        let r = sm.handle_event(
            CouplingEvent::SyncAchieved,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(r, CouplingTransition::Ok(CouplingState::SlaveModulated));
    }

    #[test]
    fn sync_timeout_goes_to_sync_lost() {
        let cfg = slave_config(1);
        let mut sm = CouplingStateMachine::new(Some(&cfg), 1000);
        sm.handle_event(
            CouplingEvent::CoupleAsSlave,
            PowerState::Standby,
            MotionState::Standstill,
        );
        let r = sm.handle_event(
            CouplingEvent::SyncTimeout,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(r, CouplingTransition::Ok(CouplingState::SyncLost));
    }

    #[test]
    fn decouple_from_master() {
        let mut sm = CouplingStateMachine::new(None, 1000);
        sm.handle_event(
            CouplingEvent::CoupleAsmaster,
            PowerState::Standby,
            MotionState::Standstill,
        );
        let r = sm.handle_event(
            CouplingEvent::Decouple,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(r, CouplingTransition::Ok(CouplingState::Decoupling));
    }

    #[test]
    fn decouple_complete_returns_to_uncoupled() {
        let mut sm = CouplingStateMachine::new(None, 1000);
        sm.handle_event(
            CouplingEvent::CoupleAsmaster,
            PowerState::Standby,
            MotionState::Standstill,
        );
        sm.handle_event(
            CouplingEvent::Decouple,
            PowerState::Standby,
            MotionState::Standstill,
        );
        let r = sm.handle_event(
            CouplingEvent::DecoupleComplete,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(r, CouplingTransition::Ok(CouplingState::Uncoupled));
    }

    #[test]
    fn coupling_blocked_during_error() {
        let mut sm = CouplingStateMachine::new(None, 1000);
        let r = sm.handle_event(
            CouplingEvent::CoupleAsmaster,
            PowerState::PowerError,
            MotionState::Standstill,
        );
        assert!(matches!(r, CouplingTransition::Rejected(_)));
    }

    #[test]
    fn coupling_blocked_when_not_standstill() {
        let mut sm = CouplingStateMachine::new(None, 1000);
        let r = sm.handle_event(
            CouplingEvent::CoupleAsmaster,
            PowerState::Standby,
            MotionState::ConstantVelocity,
        );
        assert!(matches!(r, CouplingTransition::Rejected(_)));
    }

    #[test]
    fn resync_from_sync_lost() {
        let cfg = slave_config(1);
        let mut sm = CouplingStateMachine::new(Some(&cfg), 1000);
        sm.handle_event(
            CouplingEvent::CoupleAsSlave,
            PowerState::Standby,
            MotionState::Standstill,
        );
        sm.handle_event(
            CouplingEvent::SyncTimeout,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(sm.state(), CouplingState::SyncLost);
        let r = sm.handle_event(
            CouplingEvent::Resync,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(r, CouplingTransition::Ok(CouplingState::WaitingSync));
    }

    #[test]
    fn force_decouple() {
        let mut sm = CouplingStateMachine::new(None, 1000);
        sm.handle_event(
            CouplingEvent::CoupleAsmaster,
            PowerState::Standby,
            MotionState::Standstill,
        );
        sm.force_decouple();
        assert_eq!(sm.state(), CouplingState::Uncoupled);
    }

    // ── T055: Sync Timeout Tick ─────────────────────────────────────

    #[test]
    fn sync_timeout_ticks() {
        let mut cfg = slave_config(1);
        cfg.sync_timeout = 0.003; // 3ms = 3 cycles at 1ms
        let mut sm = CouplingStateMachine::new(Some(&cfg), 1000);
        sm.handle_event(
            CouplingEvent::CoupleAsSlave,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert!(!sm.tick_sync_timeout());
        assert!(!sm.tick_sync_timeout());
        assert!(sm.tick_sync_timeout()); // 3rd tick → timeout
    }

    // ── T056: Slave Position Calculation ────────────────────────────

    #[test]
    fn slave_coupled_position() {
        let pos = calculate_slave_position(100.0, 2.0, 0.0, false);
        assert_eq!(pos, 200.0);
    }

    #[test]
    fn slave_modulated_position() {
        let pos = calculate_slave_position(100.0, 2.0, 5.0, true);
        assert_eq!(pos, 205.0);
    }

    // ── T058: Lag Difference Check ──────────────────────────────────

    #[test]
    fn lag_diff_within_limit() {
        assert!(!check_lag_difference(0.5, 0.3, 1.0));
    }

    #[test]
    fn lag_diff_exceeds_limit() {
        assert!(check_lag_difference(1.5, 0.0, 1.0));
    }

    // ── AxisCouplingRuntime Tests ────────────────────────────────────

    #[test]
    fn runtime_lag_diff_triggers_sync_lost() {
        let cfg = slave_config(1);
        let mut rt = AxisCouplingRuntime::new(cfg, 1000);
        // Manually set to SlaveCoupled.
        rt.machine.handle_event(
            CouplingEvent::CoupleAsSlave,
            PowerState::Standby,
            MotionState::Standstill,
        );
        rt.machine.handle_event(
            CouplingEvent::SyncAchieved,
            PowerState::Standby,
            MotionState::Standstill,
        );
        assert_eq!(rt.machine.state(), CouplingState::SlaveCoupled);

        // Lag diff exceeds limit.
        rt.evaluate_cycle(Some(5.0), 0.0);
        assert_eq!(rt.machine.state(), CouplingState::SyncLost);
        assert!(rt.errors.contains(CouplingError::LAG_DIFF_EXCEED));
    }
}
