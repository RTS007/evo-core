//! Control accuracy validation tests (T094 / SC-004).
//!
//! Verifies the PID control loop converges on step inputs with
//! steady-state error < 0.1 mm for reference axis configurations.

use evo_common::control_unit::control::{ControlOutputVector, UniversalControlParameters};
use evo_control_unit::control::output::{AxisControlState, ControlInput, compute_control_output};
use evo_control_unit::control::lag::evaluate_lag;

/// Simulated plant: integrator-based position model.
///
/// `position += velocity * dt`
/// `velocity += (torque / inertia - damping * velocity) * dt`
///
/// Simple 2nd-order model sufficient for control loop convergence tests.
struct SimulatedAxis {
    position: f64,
    velocity: f64,
    inertia: f64,
    damping: f64,
}

impl SimulatedAxis {
    fn new(inertia: f64, damping: f64) -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
            inertia,
            damping,
        }
    }

    /// Apply torque for one cycle and return new position/velocity.
    fn step(&mut self, torque: f64, dt: f64) {
        let accel = torque / self.inertia - self.damping * self.velocity;
        self.velocity += accel * dt;
        self.position += self.velocity * dt;
    }
}

/// Reference axis parameters: well-tuned PID for a 10 kg linear axis.
///
/// Gains are chosen so that the closed-loop converges to < 0.1 mm
/// steady-state error on a simple integrator-based plant with
/// inertia = 10 kg, damping = 0.5 N·s/mm within ~5 s at 1 kHz.
fn reference_params() -> UniversalControlParameters {
    UniversalControlParameters {
        kp: 400.0,
        ki: 200.0,
        kd: 20.0,
        tf: 0.001,
        tt: 0.02,
        kvff: 0.0,
        kaff: 0.0,
        friction: 0.0,
        jn: 0.0,
        bn: 0.0,
        gdob: 0.0,
        f_notch: 0.0,
        bw_notch: 0.0,
        flp: 0.0,
        out_max: 500.0,
        lag_error_limit: 5.0,
        ..Default::default()
    }
}

/// Run a step response simulation and return the final steady-state error.
///
/// - `step_size`: target position [mm]
/// - `settle_cycles`: number of cycles to run
/// - `dt`: cycle period [s]
/// - `inertia`: simulated axis inertia [kg]
/// - `damping`: simulated axis viscous damping [N·s/mm]
fn run_step_response(
    params: &UniversalControlParameters,
    step_size: f64,
    settle_cycles: usize,
    dt: f64,
    inertia: f64,
    damping: f64,
) -> (f64, Vec<f64>) {
    let mut control_state = AxisControlState::default();
    let mut axis = SimulatedAxis::new(inertia, damping);
    let mut errors = Vec::with_capacity(settle_cycles);

    for _ in 0..settle_cycles {
        let input = ControlInput {
            target_position: step_size,
            actual_position: axis.position,
            target_velocity: 0.0,
            actual_velocity: axis.velocity,
            target_acceleration: 0.0,
            dt,
        };

        let output: ControlOutputVector = compute_control_output(
            &mut control_state,
            params,
            &input,
        );

        // Apply calculated torque to simulated plant
        axis.step(output.calculated_torque, dt);
        errors.push((step_size - axis.position).abs());
    }

    let final_error = (step_size - axis.position).abs();
    (final_error, errors)
}

// ─── SC-004: Steady-state error < 0.1 mm ───────────────────────────

#[test]
fn step_response_1mm_steady_state_error_below_threshold() {
    let params = reference_params();
    let dt = 0.001; // 1 kHz cycle
    let (final_error, _) = run_step_response(&params, 1.0, 10_000, dt, 10.0, 0.5);

    assert!(
        final_error < 0.1,
        "SC-004: Steady-state error {:.6} mm exceeds 0.1 mm threshold for 1 mm step",
        final_error,
    );
}

#[test]
fn step_response_10mm_steady_state_error_below_threshold() {
    let params = reference_params();
    let dt = 0.001;
    let (final_error, _) = run_step_response(&params, 10.0, 20_000, dt, 10.0, 0.5);

    assert!(
        final_error < 0.1,
        "SC-004: Steady-state error {:.6} mm exceeds 0.1 mm threshold for 10 mm step",
        final_error,
    );
}

#[test]
fn step_response_100mm_steady_state_error_below_threshold() {
    let params = reference_params();
    let dt = 0.001;
    let (final_error, _) = run_step_response(&params, 100.0, 50_000, dt, 10.0, 0.5);

    assert!(
        final_error < 0.1,
        "SC-004: Steady-state error {:.6} mm exceeds 0.1 mm threshold for 100 mm step",
        final_error,
    );
}

// ─── Overshoot and settling ─────────────────────────────────────────

#[test]
fn step_response_overshoot_bounded() {
    let params = reference_params();
    let dt = 0.001;
    let step_size = 10.0;
    let (_, _errors) = run_step_response(&params, step_size, 10_000, dt, 10.0, 0.5);

    // Find the minimum error (closest approach), then check for any overshoot
    // Overshoot means position exceeds target, so error would be negative in signed terms.
    // Since we track absolute error, we need to check position directly.
    // Re-run with position tracking.
    let mut control_state = AxisControlState::default();
    let mut axis = SimulatedAxis::new(10.0, 0.5);
    let mut max_overshoot = 0.0f64;

    for _ in 0..10_000 {
        let input = ControlInput {
            target_position: step_size,
            actual_position: axis.position,
            target_velocity: 0.0,
            actual_velocity: axis.velocity,
            target_acceleration: 0.0,
            dt,
        };

        let output = compute_control_output(&mut control_state, &params, &input);
        axis.step(output.calculated_torque, dt);

        if axis.position > step_size {
            let overshoot = axis.position - step_size;
            max_overshoot = max_overshoot.max(overshoot);
        }
    }

    // Allow up to 20% overshoot for underdamped response
    let overshoot_pct = max_overshoot / step_size * 100.0;
    assert!(
        overshoot_pct < 20.0,
        "Overshoot {:.2}% exceeds 20% limit for 10 mm step",
        overshoot_pct,
    );
}

#[test]
fn step_response_settles_within_10_seconds() {
    let params = reference_params();
    let dt = 0.001;
    let step_size = 10.0;
    let total_cycles = 15_000; // 15 seconds max

    let mut control_state = AxisControlState::default();
    let mut axis = SimulatedAxis::new(10.0, 0.5);
    let mut settled_cycle = None;
    let threshold = 0.1; // 0.1 mm

    for cycle in 0..total_cycles {
        let input = ControlInput {
            target_position: step_size,
            actual_position: axis.position,
            target_velocity: 0.0,
            actual_velocity: axis.velocity,
            target_acceleration: 0.0,
            dt,
        };

        let output = compute_control_output(&mut control_state, &params, &input);
        axis.step(output.calculated_torque, dt);

        let error = (step_size - axis.position).abs();
        if error < threshold && settled_cycle.is_none() {
            settled_cycle = Some(cycle);
        }
        // If we drift back out of tolerance, reset
        if error >= threshold {
            settled_cycle = None;
        }
    }

    let settle_time_ms = settled_cycle.map(|c| c as f64 * dt * 1000.0);
    assert!(
        settle_time_ms.is_some(),
        "Axis did not settle within {:.0} ms for {:.0} mm step",
        total_cycles as f64 * dt * 1000.0,
        step_size,
    );
    let settle_ms = settle_time_ms.unwrap();
    assert!(
        settle_ms < 10_000.0,
        "Settling time {:.1} ms exceeds 10000 ms limit",
        settle_ms,
    );
}

// ─── Lag monitoring integration ─────────────────────────────────────

#[test]
fn lag_within_limits_during_step_response() {
    let params = reference_params();
    let dt = 0.001;
    let step_size = 10.0;
    let total_cycles = 10_000;

    let mut control_state = AxisControlState::default();
    let mut axis = SimulatedAxis::new(10.0, 0.5);
    let mut safety_stop_triggered = false;

    for _ in 0..total_cycles {
        let input = ControlInput {
            target_position: step_size,
            actual_position: axis.position,
            target_velocity: 0.0,
            actual_velocity: axis.velocity,
            target_acceleration: 0.0,
            dt,
        };

        let output = compute_control_output(&mut control_state, &params, &input);
        axis.step(output.calculated_torque, dt);

        // Evaluate lag with "Unwanted" policy (default from reference params)
        let lag = evaluate_lag(
            step_size,
            axis.position,
            params.lag_error_limit,
            evo_common::control_unit::state::LagPolicy::Unwanted,
        );

        if lag.trigger_safety_stop {
            safety_stop_triggered = true;
            break;
        }
    }

    assert!(
        !safety_stop_triggered,
        "SAFETY_STOP triggered during normal step response — lag exceeded limit",
    );
}

// ─── Heavy-load axis (worst case) ───────────────────────────────────

#[test]
fn heavy_load_axis_converges() {
    // 50 kg inertia, higher damping — harder to control
    let params = UniversalControlParameters {
        kp: 500.0,
        ki: 200.0,
        kd: 25.0,
        tf: 0.001,
        tt: 0.02,
        out_max: 1000.0,
        lag_error_limit: 5.0,
        ..Default::default()
    };
    let dt = 0.001;
    let (final_error, _) = run_step_response(&params, 10.0, 30_000, dt, 50.0, 2.0);

    assert!(
        final_error < 0.1,
        "SC-004: Heavy axis steady-state error {:.6} mm exceeds 0.1 mm for 10 mm step",
        final_error,
    );
}

// ─── Multi-axis concurrent accuracy ─────────────────────────────────

#[test]
fn eight_axis_concurrent_accuracy() {
    let dt = 0.001;
    let settle_cycles = 50_000;
    let step_sizes = [1.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 2.0];
    let params = reference_params();

    let mut states: Vec<AxisControlState> = (0..8).map(|_| AxisControlState::default()).collect();
    let mut axes: Vec<SimulatedAxis> = (0..8).map(|_| SimulatedAxis::new(10.0, 0.5)).collect();

    for _ in 0..settle_cycles {
        for (i, (state, axis)) in states.iter_mut().zip(axes.iter_mut()).enumerate() {
            let input = ControlInput {
                target_position: step_sizes[i],
                actual_position: axis.position,
                target_velocity: 0.0,
                actual_velocity: axis.velocity,
                target_acceleration: 0.0,
                dt,
            };

            let output = compute_control_output(state, &params, &input);
            axis.step(output.calculated_torque, dt);
        }
    }

    for (i, axis) in axes.iter().enumerate() {
        let error = (step_sizes[i] - axis.position).abs();
        assert!(
            error < 0.1,
            "SC-004: Axis {} steady-state error {:.6} mm exceeds 0.1 mm for {:.0} mm step",
            i + 1,
            error,
            step_sizes[i],
        );
    }
}
