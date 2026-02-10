//! PID controller with backward Euler integration, derivative filter (Tf),
//! and anti-windup via back-calculation (Tt).
//!
//! Zero Ki disables integral; zero Kd disables derivative.

/// Internal state of the PID controller.
///
/// Preserves integral accumulator and filtered derivative across cycles.
/// Must be reset (via [`PidState::reset`]) on axis disable or mode change
/// per invariant I-PW-4 / I-OM-4.
#[derive(Debug, Clone, Copy)]
pub struct PidState {
    /// Integral accumulator.
    integral: f64,
    /// Previous position error (for derivative).
    prev_error: f64,
    /// Filtered derivative term (low-pass via Tf).
    derivative_filtered: f64,
    /// Previous raw (unsaturated) PID output — for anti-windup.
    prev_raw_output: f64,
}

impl Default for PidState {
    fn default() -> Self {
        Self {
            integral: 0.0,
            prev_error: 0.0,
            derivative_filtered: 0.0,
            prev_raw_output: 0.0,
        }
    }
}

impl PidState {
    /// Reset all internal state to zero.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// PID gains — extracted from `UniversalControlParameters` for clarity.
#[derive(Debug, Clone, Copy)]
pub struct PidGains {
    /// Proportional gain.
    pub kp: f64,
    /// Integral gain (0 = disabled).
    pub ki: f64,
    /// Derivative gain (0 = disabled).
    pub kd: f64,
    /// Derivative filter time constant [s] (0 = unfiltered).
    pub tf: f64,
    /// Anti-windup tracking time constant [s] (0 = disabled).
    pub tt: f64,
    /// Output saturation limit [Nm] — needed for anti-windup.
    pub out_max: f64,
}

/// Compute one PID cycle using backward Euler integration.
///
/// # Arguments
/// - `state`: Mutable PID internal state (integral, derivative, etc.).
/// - `gains`: PID gains for this axis.
/// - `error`: Current position error (target − actual) [mm].
/// - `dt`: Cycle period [s].
///
/// # Returns
/// PID output [Nm] (unsaturated — clamping is done in the output stage).
#[inline]
pub fn pid_compute(state: &mut PidState, gains: &PidGains, error: f64, dt: f64) -> f64 {
    if dt <= 0.0 {
        return 0.0;
    }

    // ── P term ──────────────────────────────────────────────
    let p_term = gains.kp * error;

    // ── I term (backward Euler) ─────────────────────────────
    let i_term = if gains.ki != 0.0 {
        // Anti-windup: back-calculation correction.
        // When the previous raw output was saturated, the difference between
        // saturated and raw is fed back to reduce the integral.
        let anti_windup = if gains.tt > 0.0 && gains.out_max > 0.0 {
            let saturated = state.prev_raw_output.clamp(-gains.out_max, gains.out_max);
            (saturated - state.prev_raw_output) / gains.tt
        } else {
            0.0
        };

        // Backward Euler: integral += (Ki * error + anti_windup) * dt
        state.integral += (gains.ki * error + anti_windup) * dt;
        state.integral
    } else {
        // Ki == 0 → integral disabled, accumulator stays at 0.
        state.integral = 0.0;
        0.0
    };

    // ── D term (with first-order filter) ────────────────────
    let d_term = if gains.kd != 0.0 {
        let raw_derivative = (error - state.prev_error) / dt;

        if gains.tf > 0.0 {
            // First-order low-pass on derivative:
            // alpha = dt / (tf + dt)
            let alpha = dt / (gains.tf + dt);
            state.derivative_filtered += alpha * (raw_derivative - state.derivative_filtered);
            gains.kd * state.derivative_filtered
        } else {
            // No filter — use raw derivative.
            gains.kd * raw_derivative
        }
    } else {
        state.derivative_filtered = 0.0;
        0.0
    };

    state.prev_error = error;

    let raw_output = p_term + i_term + d_term;
    state.prev_raw_output = raw_output;

    raw_output
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 0.001; // 1 kHz cycle

    fn gains_p_only(kp: f64) -> PidGains {
        PidGains {
            kp,
            ki: 0.0,
            kd: 0.0,
            tf: 0.0,
            tt: 0.0,
            out_max: 100.0,
        }
    }

    #[test]
    fn pure_proportional() {
        let mut s = PidState::default();
        let g = gains_p_only(10.0);
        let out = pid_compute(&mut s, &g, 1.0, DT);
        assert!((out - 10.0).abs() < 1e-12);
    }

    #[test]
    fn zero_gains_produce_zero() {
        let mut s = PidState::default();
        let g = gains_p_only(0.0);
        let out = pid_compute(&mut s, &g, 5.0, DT);
        assert!((out).abs() < 1e-12);
    }

    #[test]
    fn integral_accumulates() {
        let mut s = PidState::default();
        let g = PidGains {
            kp: 0.0,
            ki: 100.0,
            kd: 0.0,
            tf: 0.0,
            tt: 0.0,
            out_max: 100.0,
        };
        // 10 cycles with constant error = 1.0
        for _ in 0..10 {
            pid_compute(&mut s, &g, 1.0, DT);
        }
        // integral = Ki * error * dt * n_cycles = 100 * 1.0 * 0.001 * 10 = 1.0
        assert!((s.integral - 1.0).abs() < 1e-10);
    }

    #[test]
    fn derivative_responds_to_error_change() {
        let mut s = PidState::default();
        let g = PidGains {
            kp: 0.0,
            ki: 0.0,
            kd: 1.0,
            tf: 0.0, // no filter
            tt: 0.0,
            out_max: 100.0,
        };
        // First cycle: error=0 → derivative = 0
        let out1 = pid_compute(&mut s, &g, 0.0, DT);
        assert!((out1).abs() < 1e-12);
        // Second cycle: error=1.0 → derivative = (1-0)/0.001 = 1000
        // D output = Kd * 1000 = 1000
        let out2 = pid_compute(&mut s, &g, 1.0, DT);
        assert!((out2 - 1000.0).abs() < 1e-8);
    }

    #[test]
    fn derivative_filter_smooths() {
        let mut s = PidState::default();
        let g = PidGains {
            kp: 0.0,
            ki: 0.0,
            kd: 1.0,
            tf: 0.01, // 10ms filter
            tt: 0.0,
            out_max: 100.0,
        };
        // First cycle: error=0
        pid_compute(&mut s, &g, 0.0, DT);
        // Second cycle: step to error=1.0 → raw derivative = 1000
        // alpha = 0.001 / (0.01 + 0.001) ≈ 0.0909
        // filtered = 0 + 0.0909 * (1000 - 0) ≈ 90.9
        let out = pid_compute(&mut s, &g, 1.0, DT);
        let expected_alpha = DT / (0.01 + DT);
        let expected = 1.0 * expected_alpha * 1000.0;
        assert!((out - expected).abs() < 1e-6);
    }

    #[test]
    fn anti_windup_limits_integral_growth() {
        let mut s = PidState::default();
        let g = PidGains {
            kp: 1.0,
            ki: 1000.0,
            kd: 0.0,
            tf: 0.0,
            tt: 0.01, // aggressive anti-windup
            out_max: 10.0,
        };
        // Run many cycles with large error — integral should not blow up
        for _ in 0..10000 {
            pid_compute(&mut s, &g, 100.0, DT);
        }
        // With anti-windup the integral should settle near a bounded value
        // Without it: integral = 1000 * 100 * 0.001 * 10000 = 1_000_000
        // With Tt=0.01, it should be much smaller
        assert!(s.integral.abs() < 1_000_000.0);
        // The integral + P should push raw output toward out_max region
        let final_out = pid_compute(&mut s, &g, 100.0, DT);
        // With strong anti-windup, raw output should be bounded near out_max
        // (P alone = 100, so final_out ≈ P + I, well below unconstrained ~1M)
        assert!(final_out.abs() < 100_000.0, "anti-windup failed: {}", final_out);
    }

    #[test]
    fn reset_clears_state() {
        let mut s = PidState::default();
        let g = PidGains {
            kp: 1.0,
            ki: 100.0,
            kd: 1.0,
            tf: 0.01,
            tt: 0.01,
            out_max: 100.0,
        };
        for _ in 0..100 {
            pid_compute(&mut s, &g, 5.0, DT);
        }
        assert!(s.integral.abs() > 0.0);
        s.reset();
        assert_eq!(s.integral, 0.0);
        assert_eq!(s.prev_error, 0.0);
        assert_eq!(s.derivative_filtered, 0.0);
        assert_eq!(s.prev_raw_output, 0.0);
    }

    #[test]
    fn zero_dt_returns_zero() {
        let mut s = PidState::default();
        let g = gains_p_only(10.0);
        let out = pid_compute(&mut s, &g, 5.0, 0.0);
        assert_eq!(out, 0.0);
    }
}
