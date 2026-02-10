//! PID / control engine micro-benchmark (T088).
//!
//! Measures throughput of individual control pipeline stages:
//! - PID compute alone
//! - Feedforward compute alone
//! - DOB compute alone
//! - Filter chain (notch + lowpass)
//! - Full compute_control_output() — validates ~0.4µs per axis (research.md Topic 11)

use criterion::{Criterion, criterion_group, criterion_main};

use evo_common::control_unit::control::UniversalControlParameters;
use evo_control_unit::control::dob::{DobGains, DobState, dob_compute};
use evo_control_unit::control::feedforward::{FeedforwardGains, feedforward_compute};
use evo_control_unit::control::filters::FilterChainState;
use evo_control_unit::control::output::{AxisControlState, ControlInput, compute_control_output};
use evo_control_unit::control::pid::{PidGains, PidState, pid_compute};

const DT: f64 = 0.001; // 1 kHz

fn reference_pid_gains() -> PidGains {
    PidGains {
        kp: 120.0,
        ki: 15.0,
        kd: 0.8,
        tf: 0.0002,
        tt: 0.01,
        out_max: 100.0,
    }
}

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
        ..Default::default()
    }
}

fn bench_pid_only(c: &mut Criterion) {
    let gains = reference_pid_gains();
    let mut state = PidState::default();
    let mut cycle = 0u64;

    c.bench_function("pid_compute", |b| {
        b.iter(|| {
            cycle += 1;
            let t = cycle as f64 * DT;
            let error = 0.05 * t.sin(); // oscillating error
            pid_compute(&mut state, &gains, error, DT)
        });
    });
}

fn bench_feedforward_only(c: &mut Criterion) {
    let gains = FeedforwardGains {
        kvff: 0.95,
        kaff: 0.001,
        friction: 0.5,
    };
    let mut cycle = 0u64;

    c.bench_function("feedforward_compute", |b| {
        b.iter(|| {
            cycle += 1;
            let t = cycle as f64 * DT;
            let vel = 100.0 * t.cos();
            let accel = -100.0 * t.sin();
            feedforward_compute(&gains, vel, accel)
        });
    });
}

fn bench_dob_only(c: &mut Criterion) {
    let gains = DobGains {
        jn: 0.01,
        bn: 0.005,
        gdob: 200.0,
    };
    let mut state = DobState::default();
    let mut cycle = 0u64;

    c.bench_function("dob_compute", |b| {
        b.iter(|| {
            cycle += 1;
            let t = cycle as f64 * DT;
            let vel = 50.0 * t.cos();
            let torque = 5.0 * t.sin();
            dob_compute(&mut state, &gains, vel, torque, DT)
        });
    });
}

fn bench_filter_chain(c: &mut Criterion) {
    let mut filters = FilterChainState::default();
    let params = reference_params();
    // Initialize filter coefficients.
    filters.init(params.f_notch, params.bw_notch, params.flp, 1.0 / DT);
    let mut cycle = 0u64;

    c.bench_function("filter_chain_apply", |b| {
        b.iter(|| {
            cycle += 1;
            let t = cycle as f64 * DT;
            let input = 10.0 * t.sin() + 0.5 * (800.0 * t).sin(); // signal with resonance
            filters.apply(input, params.flp, DT)
        });
    });
}

fn bench_full_control_output(c: &mut Criterion) {
    let params = reference_params();
    let mut state = AxisControlState::default();

    // Warm up to initialize filters.
    let warmup_input = ControlInput {
        target_position: 0.0,
        actual_position: 0.0,
        target_velocity: 0.0,
        actual_velocity: 0.0,
        target_acceleration: 0.0,
        dt: DT,
    };
    compute_control_output(&mut state, &params, &warmup_input);

    let mut cycle = 0u64;

    c.bench_function("compute_control_output", |b| {
        b.iter(|| {
            cycle += 1;
            let t = cycle as f64 * DT;
            let target_pos = 10.0 * t.sin();
            let target_vel = 10.0 * t.cos();
            let actual_pos = target_pos - 0.05;
            let actual_vel = target_vel * 0.99;

            let input = ControlInput {
                target_position: target_pos,
                actual_position: actual_pos,
                target_velocity: target_vel,
                actual_velocity: actual_vel,
                target_acceleration: -10.0 * t.sin(),
                dt: DT,
            };
            compute_control_output(&mut state, &params, &input)
        });
    });
}

criterion_group!(
    benches,
    bench_pid_only,
    bench_feedforward_only,
    bench_dob_only,
    bench_filter_chain,
    bench_full_control_output,
);
criterion_main!(benches);
