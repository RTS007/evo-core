//! Recovery timing benchmark (T095 / SC-009).
//!
//! Measures the latency of the SYSTEM_ERROR → Idle recovery path:
//! - Safety stop trigger + complete
//! - Recovery sequence (reset → flags clear → authorize → complete)
//! - Machine state transition SystemError → Idle
//!
//! SC-009: Recovery latency < 100 ms (our logic is data-structure ops,
//! so we measure the pure computation time — I/O and SHM latency are
//! not included).

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use evo_common::control_unit::safety::{AxisSafetyState, SafeStopConfig};
use evo_common::control_unit::state::{MachineState, SafeStopCategory};
use evo_control_unit::safety::recovery::{RecoveryManager, RecoveryStep};
use evo_control_unit::safety::stop::{SafeStopExecutor, StopAction};
use evo_control_unit::state::machine::{
    MachineEvent, MachineStateMachine, TransitionResult,
};

fn sto_config() -> SafeStopConfig {
    SafeStopConfig {
        category: SafeStopCategory::STO,
        max_decel_safe: 10_000.0,
        sto_brake_delay: 0.1,
        ss2_holding_torque: 20.0,
    }
}

fn ss1_config() -> SafeStopConfig {
    SafeStopConfig {
        category: SafeStopCategory::SS1,
        max_decel_safe: 10_000.0,
        sto_brake_delay: 0.1,
        ss2_holding_torque: 20.0,
    }
}

/// Benchmark: trigger safety stop and run to completion for one axis (STO).
fn bench_safety_stop_execute(c: &mut Criterion) {
    let cfg = sto_config();
    c.bench_function("safety_stop_sto_complete", |b| {
        b.iter(|| {
            let mut executor = SafeStopExecutor::new(&cfg, 1000, 5.0);
            executor.trigger();

            let mut speed = 100.0;
            for _ in 0..10_000 {
                let action = executor.tick(black_box(speed));
                if executor.is_complete() {
                    return black_box(action);
                }
                match action {
                    StopAction::DisableAndBrake => speed = 0.0,
                    StopAction::Decelerate(decel) => {
                        speed = (speed - decel * 0.001).max(0.0);
                    }
                    StopAction::HoldTorque(_) => {}
                    StopAction::None => {}
                }
            }
            black_box(StopAction::None)
        })
    });
}

/// Benchmark: trigger safety stop and run to completion (SS1 — decel+disable).
fn bench_safety_stop_ss1(c: &mut Criterion) {
    let cfg = ss1_config();
    c.bench_function("safety_stop_ss1_complete", |b| {
        b.iter(|| {
            let mut executor = SafeStopExecutor::new(&cfg, 1000, 5.0);
            executor.trigger();

            let mut speed = 100.0;
            for _ in 0..10_000 {
                let action = executor.tick(black_box(speed));
                if executor.is_complete() {
                    return black_box(action);
                }
                match action {
                    StopAction::DisableAndBrake => speed = 0.0,
                    StopAction::Decelerate(decel) => {
                        speed = (speed - decel * 0.001).max(0.0);
                    }
                    StopAction::HoldTorque(_) => {}
                    StopAction::None => {}
                }
            }
            black_box(StopAction::None)
        })
    });
}

/// Benchmark: full recovery sequence (no I/O — pure state machine ops).
fn bench_recovery_sequence(c: &mut Criterion) {
    c.bench_function("recovery_full_sequence", |b| {
        b.iter(|| {
            let mut rm = RecoveryManager::new(true);

            // Begin recovery
            rm.begin();
            assert_eq!(rm.step(), RecoveryStep::WaitingReset);

            // Reset pressed
            rm.tick(true, false);
            assert_eq!(rm.step(), RecoveryStep::WaitingFlagsClear);

            // All axes safe
            rm.tick(false, true);
            assert_eq!(rm.step(), RecoveryStep::WaitingAuthorization);

            // Authorize
            rm.authorize();
            rm.tick(false, true);
            assert_eq!(rm.step(), RecoveryStep::Complete);

            black_box(rm.step())
        })
    });
}

/// Benchmark: recovery without authorization (faster path).
fn bench_recovery_no_auth(c: &mut Criterion) {
    c.bench_function("recovery_no_auth", |b| {
        b.iter(|| {
            let mut rm = RecoveryManager::new(false);
            rm.begin();
            rm.tick(true, false); // reset pressed
            rm.tick(false, true); // all axes safe → Complete (no auth needed)
            assert_eq!(rm.step(), RecoveryStep::Complete);
            black_box(rm.step())
        })
    });
}

/// Benchmark: machine state transition SystemError → Idle via ErrorRecovery.
fn bench_machine_state_recovery(c: &mut Criterion) {
    c.bench_function("machine_state_error_to_idle", |b| {
        b.iter(|| {
            let mut sm = MachineStateMachine::new();

            // Drive to SystemError
            sm.handle_event(MachineEvent::PowerOn);
            sm.handle_event(MachineEvent::InitComplete);
            sm.handle_event(MachineEvent::CriticalFault);
            assert_eq!(sm.state(), MachineState::SystemError);

            // Recover
            let result = sm.handle_event(MachineEvent::ErrorRecovery);
            assert_eq!(result, TransitionResult::Ok(MachineState::Idle));

            black_box(result)
        })
    });
}

/// Benchmark: complete recovery pipeline (stop + recovery + state machine).
///
/// This is the full SYSTEM_ERROR→Idle path measured end-to-end.
fn bench_full_recovery_pipeline(c: &mut Criterion) {
    let cfg = sto_config();
    c.bench_function("full_recovery_pipeline_8axis", |b| {
        b.iter(|| {
            // 1. Safety stop for 8 axes (STO = fastest)
            let mut executors: Vec<SafeStopExecutor> =
                (0..8).map(|_| SafeStopExecutor::new(&cfg, 1000, 5.0)).collect();

            for ex in &mut executors {
                ex.trigger();
            }

            // Run all executors to completion
            for _ in 0..10_000 {
                let all_done = executors.iter().all(|e| e.is_complete());
                if all_done {
                    break;
                }
                for ex in &mut executors {
                    ex.tick(black_box(0.0));
                }
            }

            // 2. Recovery sequence
            let mut rm = RecoveryManager::new(true);
            rm.begin();
            rm.tick(true, false); // reset
            rm.tick(false, true); // flags clear
            rm.authorize();
            rm.tick(false, true); // authorized → complete

            // 3. Machine state transition
            let mut sm = MachineStateMachine::new();
            sm.handle_event(MachineEvent::PowerOn);
            sm.handle_event(MachineEvent::InitComplete);
            sm.handle_event(MachineEvent::CriticalFault);
            let result = sm.handle_event(MachineEvent::ErrorRecovery);

            black_box(result)
        })
    });
}

/// Benchmark: 8-axis safety flag evaluation (all_axes_safe check).
fn bench_safety_flag_evaluation(c: &mut Criterion) {
    c.bench_function("safety_flags_8axis_check", |b| {
        let states: Vec<AxisSafetyState> = (0..8).map(|_| AxisSafetyState::default()).collect();
        b.iter(|| {
            let result = RecoveryManager::all_axes_safe(black_box(&states));
            black_box(result)
        })
    });
}

criterion_group!(
    benches,
    bench_safety_stop_execute,
    bench_safety_stop_ss1,
    bench_recovery_sequence,
    bench_recovery_no_auth,
    bench_machine_state_recovery,
    bench_full_recovery_pipeline,
    bench_safety_flag_evaluation,
);
criterion_main!(benches);
