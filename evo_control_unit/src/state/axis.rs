//! LEVEL 3: AxisState container (T041).
//!
//! Holds all 6 orthogonal per-axis state machines plus safety flags,
//! error state, control state, and source lock.
//!
//! This is the per-axis orchestrator called once per cycle to evaluate
//! all sub-state machines and produce the axis state snapshot.

use evo_common::control_unit::config::MAX_AXES_LIMIT;
use evo_common::control_unit::error::AxisErrorState;
use evo_common::control_unit::shm::AxisStateSnapshot;
use evo_common::control_unit::state::{
    CouplingState, GearboxState, LoadingState,
};

use super::motion::MotionStateMachine;
use super::operational::OperationalModeMachine;
use super::power::PowerStateMachine;

/// Maximum number of axes (compile-time bound).
const MAX_AXES: usize = MAX_AXES_LIMIT as usize;

/// Per-axis state container aggregating all sub-state machines.
#[derive(Debug, Clone)]
pub struct AxisState {
    /// Axis ID (1-based).
    pub axis_id: u8,
    /// Power state machine.
    pub power: PowerStateMachine,
    /// Motion state machine.
    pub motion: MotionStateMachine,
    /// Operational mode manager.
    pub operational: OperationalModeMachine,
    /// Coupling state (simple state, full SM deferred to T057+).
    pub coupling: CouplingState,
    /// Gearbox state (simple state, full SM deferred to T057+).
    pub gearbox: GearboxState,
    /// Loading state.
    pub loading: LoadingState,
    /// Aggregate error state.
    pub errors: AxisErrorState,
    /// Whether axis is referenced (from HAL feedback).
    pub referenced: bool,
    /// Command source lock: who owns this axis (0 = nobody).
    pub locked_by: u8,
}

impl AxisState {
    /// Create a new axis state with the given configuration.
    pub fn new(axis_id: u8, has_brake: bool, has_lock_pin: bool, is_gravity_axis: bool) -> Self {
        Self {
            axis_id,
            power: PowerStateMachine::new(has_brake, has_lock_pin, is_gravity_axis),
            motion: MotionStateMachine::new(),
            operational: OperationalModeMachine::new(),
            coupling: CouplingState::Uncoupled,
            gearbox: GearboxState::NoGearbox,
            loading: LoadingState::Production,
            errors: AxisErrorState::default(),
            referenced: false,
            locked_by: 0,
        }
    }

    /// Produce a diagnostic snapshot of this axis for the MQT segment.
    pub fn snapshot(&self) -> AxisStateSnapshot {
        AxisStateSnapshot {
            axis_id: self.axis_id,
            power: self.power.state() as u8,
            motion: self.motion.state() as u8,
            operational: self.operational.mode() as u8,
            coupling: self.coupling as u8,
            gearbox: self.gearbox as u8,
            loading: self.loading as u8,
            locked_by: self.locked_by,
            safety_flags: 0xFF, // TODO: from AxisSafetyState (T042+)
            error_power: self.errors.power.bits(),
            error_motion: self.errors.motion.bits(),
            error_command: self.errors.command.bits(),
            error_gearbox: self.errors.gearbox.bits(),
            error_coupling: self.errors.coupling.bits(),
            _pad: [0u8; 5],
            position: 0.0,  // filled from HAL feedback in cycle_body
            velocity: 0.0,
            lag: 0.0,
            torque: 0.0,
        }
    }
}

/// Collection of all axis states, pre-allocated for zero-allocation cycle.
#[derive(Debug)]
pub struct AxisStates {
    /// Per-axis state (indexed by position, not axis_id).
    pub axes: [Option<AxisState>; MAX_AXES],
    /// Number of active axes.
    pub count: u8,
}

impl AxisStates {
    /// Create an empty axis states collection.
    pub const fn new() -> Self {
        Self {
            axes: [const { None }; MAX_AXES],
            count: 0,
        }
    }

    /// Initialize axes from config.
    pub fn init_from_config(
        configs: &[(u8, bool, bool, bool)], // (axis_id, has_brake, has_lock_pin, is_gravity)
    ) -> Self {
        let mut states = Self::new();
        for (i, &(axis_id, has_brake, has_lock_pin, is_gravity)) in configs.iter().enumerate() {
            if i >= MAX_AXES {
                break;
            }
            states.axes[i] = Some(AxisState::new(axis_id, has_brake, has_lock_pin, is_gravity));
            states.count += 1;
        }
        states
    }

    /// Get a reference to an axis state by index (0-based position).
    #[inline]
    pub fn get(&self, index: usize) -> Option<&AxisState> {
        self.axes.get(index).and_then(|a| a.as_ref())
    }

    /// Get a mutable reference to an axis state by index.
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut AxisState> {
        self.axes.get_mut(index).and_then(|a| a.as_mut())
    }

    /// Find an axis by its axis_id.
    pub fn find_by_id(&self, axis_id: u8) -> Option<(usize, &AxisState)> {
        for i in 0..self.count as usize {
            if let Some(ref ax) = self.axes[i] {
                if ax.axis_id == axis_id {
                    return Some((i, ax));
                }
            }
        }
        None
    }

    /// Find a mutable reference to an axis by its axis_id.
    pub fn find_by_id_mut(&mut self, axis_id: u8) -> Option<(usize, &mut AxisState)> {
        let n = self.count as usize;
        for i in 0..n {
            if self.axes[i].as_ref().is_some_and(|ax| ax.axis_id == axis_id) {
                return Some((i, self.axes[i].as_mut().unwrap()));
            }
        }
        None
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use evo_common::control_unit::state::{OperationalMode, PowerState};

    #[test]
    fn axis_state_initial_values() {
        let ax = AxisState::new(1, true, false, false);
        assert_eq!(ax.axis_id, 1);
        assert_eq!(ax.power.state(), PowerState::PowerOff);
        assert_eq!(ax.motion.state(), evo_common::control_unit::state::MotionState::Standstill);
        assert_eq!(ax.operational.mode(), OperationalMode::Position);
        assert_eq!(ax.coupling, CouplingState::Uncoupled);
        assert!(!ax.referenced);
        assert_eq!(ax.locked_by, 0);
    }

    #[test]
    fn axis_state_snapshot_fields() {
        let ax = AxisState::new(5, true, false, false);
        let snap = ax.snapshot();
        assert_eq!(snap.axis_id, 5);
        assert_eq!(snap.power, PowerState::PowerOff as u8);
        assert_eq!(snap.coupling, CouplingState::Uncoupled as u8);
    }

    #[test]
    fn axis_states_init_from_config() {
        let configs = vec![
            (1, true, false, false),
            (2, false, true, true),
            (3, true, true, false),
        ];
        let states = AxisStates::init_from_config(&configs);
        assert_eq!(states.count, 3);
        assert_eq!(states.get(0).unwrap().axis_id, 1);
        assert_eq!(states.get(2).unwrap().axis_id, 3);
        assert!(states.get(3).is_none());
    }

    #[test]
    fn axis_states_find_by_id() {
        let configs = vec![(1, false, false, false), (5, false, false, false)];
        let states = AxisStates::init_from_config(&configs);
        let (idx, ax) = states.find_by_id(5).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(ax.axis_id, 5);
        assert!(states.find_by_id(99).is_none());
    }
}