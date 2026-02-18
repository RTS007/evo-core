//! Hierarchical error propagation rules (T035).
//!
//! Classifies each error flag as CRITICAL or non-critical and implements
//! propagation per FR-091 and FR-092.
//!
//! ## Propagation Model
//!
//! - **Non-critical errors** (FR-091): affect only the faulting axis.
//!   The axis enters a degraded state but other axes continue.
//!
//! - **Critical errors** (FR-092): trigger `SafetyState::SAFETY_STOP`
//!   for **all** axes. The machine enters `SYSTEM_ERROR`.
//!
//! ## Coupling Chain Propagation (FR-053)
//!
//! A critical error on a slave axis propagates `ERR_SLAVE_FAULT` to
//! the master axis. Error propagation walks the coupling graph upward.
//!
//! ## Zero-Allocation Guarantee
//!
//! All functions operate on pre-allocated arrays and return stack-local
//! results. No heap allocation occurs.

use evo_common::consts::MAX_AXES;
use evo_common::control_unit::error::{AxisErrorState, CouplingError};
use evo_common::control_unit::state::AxisId;

/// Maximum number of axes (compile-time bound for fixed arrays).
const MAX_AXES_USIZE: usize = MAX_AXES as usize;

// ─── Propagation Result ─────────────────────────────────────────────

/// Outcome of a per-cycle error evaluation pass.
#[derive(Debug, Clone)]
pub struct PropagationResult {
    /// If true, at least one CRITICAL error was found → trigger `SAFETY_STOP`.
    pub safety_stop_required: bool,
    /// Per-axis: true if the axis has any error (critical or not).
    pub axis_has_error: [bool; MAX_AXES_USIZE],
    /// Per-axis: true if the axis has a CRITICAL error.
    pub axis_has_critical: [bool; MAX_AXES_USIZE],
    /// Index of the first axis with a critical error (for diagnostics).
    pub first_critical_axis: Option<u8>,
}

impl PropagationResult {
    /// Create a clean result (no errors).
    pub const fn clean() -> Self {
        Self {
            safety_stop_required: false,
            axis_has_error: [false; MAX_AXES_USIZE],
            axis_has_critical: [false; MAX_AXES_USIZE],
            first_critical_axis: None,
        }
    }
}

// ─── Coupling Topology ──────────────────────────────────────────────

/// Pre-computed coupling graph for error propagation.
///
/// Built once at startup from configuration. Each axis stores its
/// master axis ID (or `None` if uncoupled or if it IS the master).
#[derive(Debug, Clone)]
pub struct CouplingTopology {
    /// `master_of[axis_id] = Some(master_axis_id)` if `axis_id` is a slave.
    master_of: [Option<u8>; MAX_AXES_USIZE],
}

impl CouplingTopology {
    /// Build coupling topology from config.
    ///
    /// # Arguments
    /// - `axis_master_pairs`: Iterator of `(axis_id, master_axis_id)` for
    ///   each axis that has a coupling.master_axis set.
    pub fn from_config(axis_master_pairs: impl Iterator<Item = (u8, u8)>) -> Self {
        let mut master_of = [None; MAX_AXES_USIZE];
        for (axis_id, master_id) in axis_master_pairs {
            if (axis_id as usize) < MAX_AXES_USIZE {
                master_of[axis_id as usize] = Some(master_id);
            }
        }
        Self { master_of }
    }

    /// Create an empty topology (no coupling).
    pub const fn empty() -> Self {
        Self {
            master_of: [None; MAX_AXES_USIZE],
        }
    }

    /// Get the master axis for a given axis (None if uncoupled).
    #[inline]
    pub fn master_of(&self, axis_id: AxisId) -> Option<u8> {
        self.master_of.get(axis_id as usize).copied().flatten()
    }
}

// ─── Error Evaluation ───────────────────────────────────────────────

/// Evaluate all axis errors and determine propagation (called every cycle).
///
/// This is the main entry point for the error subsystem. It:
/// 1. Scans each active axis for CRITICAL errors.
/// 2. If any CRITICAL error is found, sets `safety_stop_required`.
/// 3. Propagates slave errors to master axes via the coupling topology.
///
/// # Arguments
/// - `errors`: Per-axis error state array (indexed by axis_id).
/// - `axis_count`: Number of active axes.
/// - `topology`: Pre-computed coupling graph.
///
/// # Returns
/// `PropagationResult` indicating whether a SAFETY_STOP is needed.
///
/// # RT Safety
/// Zero-allocation. O(axis_count) scan with O(depth) coupling walk.
pub fn evaluate_errors(
    errors: &[AxisErrorState; MAX_AXES_USIZE],
    axis_count: u8,
    topology: &CouplingTopology,
) -> PropagationResult {
    let mut result = PropagationResult::clean();
    let n = axis_count as usize;

    // Phase 1: Scan for local errors and critical classification.
    for i in 0..n {
        let err = &errors[i];
        if err.has_any_error() {
            result.axis_has_error[i] = true;
        }
        if err.has_critical() {
            result.axis_has_critical[i] = true;
            result.safety_stop_required = true;
            if result.first_critical_axis.is_none() {
                result.first_critical_axis = Some(i as u8);
            }
        }
    }

    // Phase 2: Coupling chain propagation.
    // If a slave axis has ANY error, propagate ERR_SLAVE_FAULT to its master.
    // Walk upward through the coupling chain (slave → master → grandmaster...).
    //
    // Note: We don't modify the input `errors` array (it's immutable).
    // The propagation result marks masters as having errors too.
    for i in 0..n {
        if !result.axis_has_error[i] {
            continue;
        }
        // Walk the coupling chain upward.
        let mut current = i as u8;
        let mut depth = 0u8;
        while let Some(master) = topology.master_of(current) {
            let m = master as usize;
            if m >= n {
                break;
            }
            result.axis_has_error[m] = true;

            // If the slave error is critical, propagate criticality upward.
            if result.axis_has_critical[i] {
                result.axis_has_critical[m] = true;
            }

            current = master;
            depth += 1;
            // Safety: max coupling chain depth is 8 (I-CP-3).
            if depth >= 8 {
                break;
            }
        }
    }

    result
}

/// Apply coupling error propagation to mutable error state arrays.
///
/// This modifies the error states in-place, setting `ERR_SLAVE_FAULT`
/// on master axes when their slaves have errors. Called after
/// `evaluate_errors()` when the decision to act has been made.
///
/// # Arguments
/// - `errors`: Mutable per-axis error state array.
/// - `axis_count`: Number of active axes.
/// - `topology`: Pre-computed coupling graph.
pub fn propagate_coupling_errors(
    errors: &mut [AxisErrorState; MAX_AXES_USIZE],
    axis_count: u8,
    topology: &CouplingTopology,
) {
    let n = axis_count as usize;

    for i in 0..n {
        if !errors[i].has_any_error() {
            continue;
        }
        // Walk the coupling chain upward, setting SLAVE_FAULT on masters.
        let mut current = i as u8;
        let mut depth = 0u8;
        while let Some(master) = topology.master_of(current) {
            let m = master as usize;
            if m >= n {
                break;
            }
            errors[m].coupling.insert(CouplingError::SLAVE_FAULT);
            current = master;
            depth += 1;
            if depth >= 8 {
                break;
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use evo_common::control_unit::error::{
        CommandError, CouplingError, GearboxError, MotionError, PowerError,
    };

    fn make_errors() -> [AxisErrorState; MAX_AXES_USIZE] {
        [AxisErrorState::default(); MAX_AXES_USIZE]
    }

    #[test]
    fn clean_state_no_errors() {
        let errors = make_errors();
        let topo = CouplingTopology::empty();
        let result = evaluate_errors(&errors, 4, &topo);

        assert!(!result.safety_stop_required);
        assert!(result.first_critical_axis.is_none());
        for i in 0..4 {
            assert!(!result.axis_has_error[i]);
            assert!(!result.axis_has_critical[i]);
        }
    }

    #[test]
    fn non_critical_error_axis_local() {
        let mut errors = make_errors();
        // Axis 1 has a non-critical lag exceed error.
        errors[1].motion = MotionError::LAG_EXCEED;

        let topo = CouplingTopology::empty();
        let result = evaluate_errors(&errors, 4, &topo);

        assert!(!result.safety_stop_required);
        assert!(!result.axis_has_error[0]); // Axis 0 unaffected.
        assert!(result.axis_has_error[1]); // Axis 1 has error.
        assert!(!result.axis_has_critical[1]); // But not critical.
    }

    #[test]
    fn critical_error_triggers_safety_stop() {
        let mut errors = make_errors();
        // Axis 2 has a CRITICAL tailstock error.
        errors[2].power = PowerError::DRIVE_TAIL_OPEN;

        let topo = CouplingTopology::empty();
        let result = evaluate_errors(&errors, 4, &topo);

        assert!(result.safety_stop_required);
        assert!(result.axis_has_critical[2]);
        assert_eq!(result.first_critical_axis, Some(2));
    }

    #[test]
    fn critical_cycle_overrun() {
        let mut errors = make_errors();
        errors[0].motion = MotionError::CYCLE_OVERRUN;

        let topo = CouplingTopology::empty();
        let result = evaluate_errors(&errors, 4, &topo);

        assert!(result.safety_stop_required);
        assert!(result.axis_has_critical[0]);
        assert_eq!(result.first_critical_axis, Some(0));
    }

    #[test]
    fn coupling_chain_propagation() {
        // Axis 2 is slave of axis 1 (master=1).
        // Axis 1 is slave of axis 0 (master=0).
        let topo = CouplingTopology::from_config([(2, 1), (1, 0)].into_iter());

        let mut errors = make_errors();
        // Slave axis 2 has a non-critical error.
        errors[2].motion = MotionError::LAG_EXCEED;

        let result = evaluate_errors(&errors, 3, &topo);

        // Axis 2 has the error.
        assert!(result.axis_has_error[2]);
        // Propagated upward to axis 1 (master) and axis 0 (grandmaster).
        assert!(result.axis_has_error[1]);
        assert!(result.axis_has_error[0]);
        // But not critical (LAG_EXCEED is non-critical).
        assert!(!result.safety_stop_required);
    }

    #[test]
    fn coupling_critical_propagation() {
        // Axis 1 is slave of axis 0.
        let topo = CouplingTopology::from_config([(1, 0)].into_iter());

        let mut errors = make_errors();
        // Slave axis 1 has a CRITICAL error.
        errors[1].power = PowerError::DRIVE_TAIL_OPEN;

        let result = evaluate_errors(&errors, 2, &topo);

        assert!(result.safety_stop_required);
        assert!(result.axis_has_critical[1]);
        // Critical propagates to master.
        assert!(result.axis_has_critical[0]);
    }

    #[test]
    fn propagate_sets_slave_fault_on_master() {
        let topo = CouplingTopology::from_config([(1, 0)].into_iter());
        let mut errors = make_errors();
        errors[1].motion = MotionError::LAG_EXCEED;

        propagate_coupling_errors(&mut errors, 2, &topo);

        assert!(errors[0].coupling.contains(CouplingError::SLAVE_FAULT));
        // Axis 1's original error is preserved.
        assert!(errors[1].motion.contains(MotionError::LAG_EXCEED));
    }

    #[test]
    fn propagation_result_clean() {
        let result = PropagationResult::clean();
        assert!(!result.safety_stop_required);
        assert!(result.first_critical_axis.is_none());
    }

    #[test]
    fn coupling_topology_from_config() {
        let topo = CouplingTopology::from_config([(3, 1), (5, 3)].into_iter());
        assert_eq!(topo.master_of(3), Some(1));
        assert_eq!(topo.master_of(5), Some(3));
        assert_eq!(topo.master_of(0), None);
        assert_eq!(topo.master_of(1), None);
    }

    // ── T035d: Additional edge-case tests ──

    #[test]
    fn zero_axes_no_errors() {
        let errors = make_errors();
        let topo = CouplingTopology::empty();
        let result = evaluate_errors(&errors, 0, &topo);
        assert!(!result.safety_stop_required);
        assert!(result.first_critical_axis.is_none());
    }

    #[test]
    fn mixed_critical_and_non_critical_across_axes() {
        let mut errors = make_errors();
        // Axis 0: non-critical only.
        errors[0].motion = MotionError::LAG_EXCEED;
        // Axis 1: critical.
        errors[1].power = PowerError::DRIVE_TAIL_OPEN;
        // Axis 2: no error.
        // Axis 3: non-critical only.
        errors[3].command = CommandError::SOURCE_LOCKED;

        let topo = CouplingTopology::empty();
        let result = evaluate_errors(&errors, 4, &topo);

        assert!(result.safety_stop_required);
        assert_eq!(result.first_critical_axis, Some(1));
        assert!(result.axis_has_error[0]);
        assert!(!result.axis_has_critical[0]);
        assert!(result.axis_has_error[1]);
        assert!(result.axis_has_critical[1]);
        assert!(!result.axis_has_error[2]);
        assert!(result.axis_has_error[3]);
        assert!(!result.axis_has_critical[3]);
    }

    #[test]
    fn diamond_coupling_topology() {
        // Diamond: axis 2 and 3 both slave of axis 1.
        // Axis 1 is slave of axis 0.
        //     0
        //     |
        //     1
        //    / \
        //   2   3
        let topo = CouplingTopology::from_config([(1, 0), (2, 1), (3, 1)].into_iter());

        let mut errors = make_errors();
        errors[2].motion = MotionError::LAG_EXCEED;

        let result = evaluate_errors(&errors, 4, &topo);

        assert!(result.axis_has_error[2]);
        assert!(result.axis_has_error[1]); // propagated from 2
        assert!(result.axis_has_error[0]); // propagated from 1→0
        assert!(!result.axis_has_error[3]); // sibling not affected
    }

    #[test]
    fn max_depth_coupling_chain() {
        // Chain of 8 axes: 7→6→5→4→3→2→1→0 (depth = 7, within limit).
        let pairs: Vec<(u8, u8)> = (1..8).map(|i| (i, i - 1)).collect();
        let topo = CouplingTopology::from_config(pairs.into_iter());

        let mut errors = make_errors();
        errors[7].motion = MotionError::LAG_EXCEED;

        let result = evaluate_errors(&errors, 8, &topo);

        // Error should propagate all the way to axis 0.
        for i in 0..8 {
            assert!(result.axis_has_error[i], "axis {i} should have error");
        }
    }

    #[test]
    fn multiple_critical_errors_first_wins() {
        let mut errors = make_errors();
        errors[3].power = PowerError::DRIVE_TAIL_OPEN;
        errors[1].motion = MotionError::CYCLE_OVERRUN;

        let topo = CouplingTopology::empty();
        let result = evaluate_errors(&errors, 4, &topo);

        assert!(result.safety_stop_required);
        // Axis 1 is scanned before axis 3.
        assert_eq!(result.first_critical_axis, Some(1));
    }

    #[test]
    fn propagate_coupling_preserves_existing_errors() {
        let topo = CouplingTopology::from_config([(1, 0)].into_iter());
        let mut errors = make_errors();
        // Master already has its own error.
        errors[0].command = CommandError::SOURCE_TIMEOUT;
        // Slave has error.
        errors[1].motion = MotionError::LAG_EXCEED;

        propagate_coupling_errors(&mut errors, 2, &topo);

        // Master's original error is preserved.
        assert!(errors[0].command.contains(CommandError::SOURCE_TIMEOUT));
        // And now also has SLAVE_FAULT.
        assert!(errors[0].coupling.contains(CouplingError::SLAVE_FAULT));
    }

    #[test]
    fn all_error_categories_detected() {
        for (i, err_fn) in [
            |e: &mut AxisErrorState| e.power = PowerError::BRAKE_TIMEOUT,
            |e: &mut AxisErrorState| e.motion = MotionError::LAG_EXCEED,
            |e: &mut AxisErrorState| e.command = CommandError::SOURCE_LOCKED,
            |e: &mut AxisErrorState| e.gearbox = GearboxError::GEAR_TIMEOUT,
            |e: &mut AxisErrorState| e.coupling = CouplingError::SYNC_TIMEOUT,
        ]
        .iter()
        .enumerate()
        {
            let mut errors = make_errors();
            err_fn(&mut errors[0]);
            let topo = CouplingTopology::empty();
            let result = evaluate_errors(&errors, 1, &topo);
            assert!(
                result.axis_has_error[0],
                "error category {i} not detected"
            );
        }
    }
}
