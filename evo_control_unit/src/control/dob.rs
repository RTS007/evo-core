//! Disturbance Observer (DOB).
//!
//! Estimates disturbance from nominal model (Jn, Bn) and actual response,
//! filtered with gDOB bandwidth. Zero gDOB disables entirely.
//!
//! The DOB estimates external disturbances by comparing the actual system
//! response against a nominal model. The estimated disturbance is then
//! subtracted from the control output to reject it.
//!
//! Algorithm (discrete, first-order low-pass filter):
//! ```text
//! nominal_torque = Jn × (v[k] - v[k-1]) / dt + Bn × v[k]
//! disturbance_raw = applied_torque - nominal_torque
//! alpha = gDOB × dt / (1 + gDOB × dt)
//! disturbance_filtered += alpha × (disturbance_raw - disturbance_filtered)
//! dob_output = -disturbance_filtered   (reject the disturbance)
//! ```

/// DOB gains — extracted from `UniversalControlParameters`.
#[derive(Debug, Clone, Copy)]
pub struct DobGains {
    /// Nominal inertia [kg·m²] (or equivalent linear mass).
    pub jn: f64,
    /// Nominal damping [N·s/m].
    pub bn: f64,
    /// Observer bandwidth [rad/s] (0 = disabled).
    pub gdob: f64,
}

/// Internal state of the Disturbance Observer.
///
/// Must be reset on axis disable / mode change (I-PW-4 / I-OM-4).
#[derive(Debug, Clone, Copy)]
pub struct DobState {
    /// Previous actual velocity [mm/s].
    prev_velocity: f64,
    /// Filtered disturbance estimate.
    disturbance_filtered: f64,
}

impl Default for DobState {
    fn default() -> Self {
        Self {
            prev_velocity: 0.0,
            disturbance_filtered: 0.0,
        }
    }
}

impl DobState {
    /// Reset all internal state to zero.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Compute one DOB cycle.
///
/// # Arguments
/// - `state`: Mutable DOB internal state.
/// - `gains`: DOB gains for this axis.
/// - `actual_velocity`: Current actual velocity from encoder [mm/s].
/// - `applied_torque`: Total torque that was applied last cycle [Nm].
/// - `dt`: Cycle period [s].
///
/// # Returns
/// DOB correction output [Nm] — add to control signal to reject disturbance.
/// Returns 0.0 when `gDOB == 0.0` (disabled).
#[inline]
pub fn dob_compute(
    state: &mut DobState,
    gains: &DobGains,
    actual_velocity: f64,
    applied_torque: f64,
    dt: f64,
) -> f64 {
    if gains.gdob == 0.0 || dt <= 0.0 {
        return 0.0;
    }

    // Nominal model: torque that the ideal system would need
    let acceleration_estimate = (actual_velocity - state.prev_velocity) / dt;
    let nominal_torque = gains.jn * acceleration_estimate + gains.bn * actual_velocity;

    // Raw disturbance = what was applied minus what nominal model expects
    let disturbance_raw = applied_torque - nominal_torque;

    // First-order low-pass filter on disturbance estimate
    let alpha = gains.gdob * dt / (1.0 + gains.gdob * dt);
    state.disturbance_filtered +=
        alpha * (disturbance_raw - state.disturbance_filtered);

    state.prev_velocity = actual_velocity;

    // Negate: compensate (reject) the disturbance
    -state.disturbance_filtered
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 0.001;

    fn disabled_gains() -> DobGains {
        DobGains {
            jn: 1.0,
            bn: 0.5,
            gdob: 0.0, // disabled
        }
    }

    #[test]
    fn disabled_when_gdob_zero() {
        let mut s = DobState::default();
        let g = disabled_gains();
        let out = dob_compute(&mut s, &g, 100.0, 50.0, DT);
        assert_eq!(out, 0.0);
    }

    #[test]
    fn zero_dt_returns_zero() {
        let mut s = DobState::default();
        let g = DobGains {
            jn: 1.0,
            bn: 0.5,
            gdob: 100.0,
        };
        let out = dob_compute(&mut s, &g, 100.0, 50.0, 0.0);
        assert_eq!(out, 0.0);
    }

    #[test]
    fn no_disturbance_converges_to_zero() {
        let mut s = DobState::default();
        let g = DobGains {
            jn: 1.0,
            bn: 0.1,
            gdob: 50.0,
        };
        // Simulate constant velocity (no accel), with applied torque = Bn * v
        // → nominal_torque matches applied → no disturbance
        let v = 100.0;
        let applied = g.bn * v; // exactly what nominal model expects at steady state
        // First cycle: acceleration estimate = (100 - 0) / dt → large but transient
        dob_compute(&mut s, &g, v, applied, DT);
        // After many cycles at same velocity, disturbance should converge to ~0
        for _ in 0..10000 {
            dob_compute(&mut s, &g, v, applied, DT);
        }
        assert!(
            s.disturbance_filtered.abs() < 0.1,
            "should converge to zero disturbance: {}",
            s.disturbance_filtered
        );
    }

    #[test]
    fn step_disturbance_detected() {
        let mut s = DobState::default();
        let g = DobGains {
            jn: 0.0, // no inertia for simplicity
            bn: 0.0, // no damping
            gdob: 100.0,
        };
        // Nominal model expects 0 torque (Jn=0, Bn=0). Applied=10 → disturbance=10.
        // After enough cycles, filtered disturbance → 10, output → -10.
        for _ in 0..5000 {
            dob_compute(&mut s, &g, 0.0, 10.0, DT);
        }
        let out = dob_compute(&mut s, &g, 0.0, 10.0, DT);
        // Should approach -10 (rejecting the 10 Nm disturbance)
        assert!(
            (out - (-10.0)).abs() < 0.5,
            "DOB output should reject disturbance: {}",
            out
        );
    }

    #[test]
    fn reset_clears_state() {
        let mut s = DobState::default();
        let g = DobGains {
            jn: 1.0,
            bn: 0.5,
            gdob: 100.0,
        };
        for _ in 0..100 {
            dob_compute(&mut s, &g, 50.0, 20.0, DT);
        }
        assert!(s.disturbance_filtered.abs() > 0.0);
        s.reset();
        assert_eq!(s.prev_velocity, 0.0);
        assert_eq!(s.disturbance_filtered, 0.0);
    }
}
