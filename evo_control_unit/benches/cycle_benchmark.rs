//! Cycle benchmark (T087) — measure full control pipeline for N-axis configurations.
//!
//! Validates SC-001: full cycle completes in <1ms for up to 64 axes.
//! Benchmarks the compute-intensive portion: per-axis control output + lag + state
//! snapshot population (excludes SHM I/O which is measured separately).

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use evo_common::control_unit::control::UniversalControlParameters;
use evo_control_unit::control::output::{AxisControlState, ControlInput, compute_control_output};
use evo_control_unit::control::lag::evaluate_lag;
use evo_control_unit::cycle::AxisRuntimeState;

/// Reference control parameters for a typical linear axis.
fn reference_params() -> UniversalControlParameters {
    UniversalControlParameters {
        kp: 120.0,
        ki: 15.0,
        kd: 0.8,
        tf: 0.0002,
        tt: 0.01,
        kvff: 0.95,
        kaff: 0.001,
        friction: 0.5,
        jn: 0.01,
        bn: 0.005,
        gdob: 200.0,
        f_notch: 800.0,
        bw_notch: 50.0,
        flp: 500.0,
        out_max: 100.0,
        // lag_error_limit and lag_policy use defaults (1.0mm, Unwanted)
        ..Default::default()
    }
}

/// Simulate one full processing cycle for N axes.
///
/// This mirrors the compute-intensive portions of `cycle_body()`:
/// 1. Read feedback (simulated with in-memory copy)
/// 2. Compute control output per axis (PID + FF + DOB + filters)
/// 3. Evaluate lag per axis
/// 4. Populate diagnostic snapshot per axis
#[inline(never)]
fn simulate_cycle(
    n: usize,
    states: &mut [AxisControlState],
    params: &[UniversalControlParameters],
    runtime: &mut [AxisRuntimeState],
    cycle: u64,
) {
    let dt = 0.001; // 1 kHz

    // Simulate motion: sinusoidal trajectory for each axis.
    let t = cycle as f64 * dt;

    for i in 0..n {
        // Simulated feedback: actual lags target slightly.
        let target_pos = 10.0 * (t * (1.0 + 0.1 * i as f64)).sin();
        let target_vel = 10.0 * (1.0 + 0.1 * i as f64) * (t * (1.0 + 0.1 * i as f64)).cos();
        let actual_pos = target_pos - 0.05; // 50µm lag
        let actual_vel = target_vel * 0.99;

        runtime[i].target_position = target_pos;
        runtime[i].target_velocity = target_vel;
        runtime[i].actual_position = actual_pos;
        runtime[i].actual_velocity = actual_vel;

        // Compute control output (PID + FF + DOB + filters).
        let input = ControlInput {
            target_position: target_pos,
            actual_position: actual_pos,
            target_velocity: target_vel,
            actual_velocity: actual_vel,
            target_acceleration: 0.0,
            dt,
        };
        let output = compute_control_output(&mut states[i], &params[i], &input);

        runtime[i].control_output = [
            output.calculated_torque,
            output.target_velocity,
            output.target_position,
            output.torque_offset,
        ];

        // Evaluate lag.
        runtime[i].lag = target_pos - actual_pos;
        let _lag_result = evaluate_lag(
            target_pos,
            actual_pos,
            params[i].lag_error_limit,
            params[i].lag_policy,
        );

        // Populate snapshot fields (simulating MQT write).
        runtime[i].torque_estimate = output.calculated_torque;
    }
}

fn bench_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("cycle_full");
    group.significance_level(0.01);
    group.sample_size(500);

    for &n_axes in &[1, 4, 8, 16, 32, 64] {
        // Pre-allocate state arrays.
        let params: Vec<UniversalControlParameters> = (0..n_axes).map(|_| reference_params()).collect();
        let mut ctrl_states: Vec<AxisControlState> =
            (0..n_axes).map(|_| AxisControlState::default()).collect();
        let mut runtime: Vec<AxisRuntimeState> =
            (0..n_axes).map(|_| AxisRuntimeState::default()).collect();

        // Warm up filter initialization (happens once at startup).
        for i in 0..n_axes {
            let input = ControlInput {
                target_position: 0.0,
                actual_position: 0.0,
                target_velocity: 0.0,
                actual_velocity: 0.0,
                target_acceleration: 0.0,
                dt: 0.001,
            };
            compute_control_output(&mut ctrl_states[i], &params[i], &input);
        }

        let mut cycle_count = 0u64;

        group.bench_with_input(
            BenchmarkId::new("axes", n_axes),
            &n_axes,
            |b, &_n| {
                b.iter(|| {
                    cycle_count += 1;
                    simulate_cycle(
                        n_axes,
                        &mut ctrl_states,
                        &params,
                        &mut runtime,
                        cycle_count,
                    );
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_cycle);
criterion_main!(benches);
