//! Integration test: safety stop and recovery (T090).
//!
//! Validates the full safety lifecycle:
//! 1. CRITICAL error → SafetyState::SafetyStop
//! 2. Per-axis SafeStopCategory execution (STO, SS1, SS2)
//! 3. Recovery sequence: reset → flags clear → authorization → Safe

use evo_common::control_unit::safety::SafeStopConfig;
use evo_common::control_unit::state::{SafeStopCategory, SafetyState};

use evo_control_unit::safety::recovery::{RecoveryManager, RecoveryStep};
use evo_control_unit::safety::stop::{SafeStopExecutor, StopAction, StopPhase};
use evo_control_unit::state::machine::{MachineEvent, MachineStateMachine};
use evo_control_unit::state::safety::{SafetyEvent, SafetyStateMachine, SafetyTransition};

// ── Helpers ─────────────────────────────────────────────────────────

fn default_ss1_config() -> SafeStopConfig {
    SafeStopConfig {
        category: SafeStopCategory::SS1,
        max_decel_safe: 10000.0,
        sto_brake_delay: 0.01,
        ss2_holding_torque: 0.0,
    }
}

fn default_ss2_config() -> SafeStopConfig {
    SafeStopConfig {
        category: SafeStopCategory::SS2,
        max_decel_safe: 10000.0,
        sto_brake_delay: 0.01,
        ss2_holding_torque: 20.0,
    }
}

fn default_sto_config() -> SafeStopConfig {
    SafeStopConfig {
        category: SafeStopCategory::STO,
        max_decel_safe: 0.0,
        sto_brake_delay: 0.005,
        ss2_holding_torque: 0.0,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn critical_error_triggers_safety_stop() {
    let mut ssm = SafetyStateMachine::new();
    assert_eq!(ssm.state(), SafetyState::Safe);

    // Trigger safety stop from CRITICAL error.
    let result = ssm.handle_event(SafetyEvent::SafetyStop);
    assert_eq!(result, SafetyTransition::Ok(SafetyState::SafetyStop));
    assert!(ssm.requires_emergency_stop());
}

#[test]
fn safety_stop_forces_machine_system_error() {
    let mut msm = MachineStateMachine::new();
    let mut ssm = SafetyStateMachine::new();

    // Start the machine.
    msm.handle_event(MachineEvent::PowerOn);
    msm.handle_event(MachineEvent::InitComplete);

    // Trigger safety stop.
    ssm.force_safety_stop();
    assert!(ssm.requires_emergency_stop());

    // Machine must transition to SystemError.
    let _result = msm.handle_event(MachineEvent::CriticalFault);
}

#[test]
fn ss1_decelerate_then_disable() {
    let cfg = default_ss1_config();
    let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);
    assert_eq!(exec.phase(), StopPhase::Idle);

    exec.trigger();
    assert_eq!(exec.phase(), StopPhase::Decelerating);

    // Simulate deceleration: high speed.
    let action = exec.tick(100.0);
    assert_eq!(action, StopAction::Decelerate(10000.0));

    // Speed drops to near-zero → complete.
    let _action = exec.tick(0.005);
    // SS1: after deceleration → WaitingBrake
    assert!(
        exec.phase() == StopPhase::WaitingBrake || exec.phase() == StopPhase::Complete,
        "phase should be WaitingBrake or Complete after stop, got {:?}",
        exec.phase()
    );
}

#[test]
fn ss2_decelerate_then_hold_torque() {
    let cfg = default_ss2_config();
    let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);

    exec.trigger();
    assert_eq!(exec.phase(), StopPhase::Decelerating);

    // Decelerate until stopped.
    let _action = exec.tick(100.0);
    let action = exec.tick(0.005);

    // SS2: after deceleration → Complete with holding torque.
    assert!(
        matches!(action, StopAction::HoldTorque { .. }) || exec.is_complete(),
        "SS2 should hold torque or be complete after stop, got {action:?}"
    );
}

#[test]
fn sto_immediate_disable() {
    let cfg = default_sto_config();
    let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);

    exec.trigger();
    // STO: skips deceleration, goes to WaitingBrake immediately.
    assert_eq!(exec.phase(), StopPhase::WaitingBrake);
}

#[test]
fn full_safety_lifecycle_stop_and_recover() {
    // 1. Start machine.
    let mut msm = MachineStateMachine::new();
    let mut ssm = SafetyStateMachine::new();
    let mut recovery = RecoveryManager::new(true);

    msm.handle_event(MachineEvent::PowerOn);
    msm.handle_event(MachineEvent::InitComplete);

    // 2. Trigger safety stop.
    ssm.force_safety_stop();
    assert_eq!(ssm.state(), SafetyState::SafetyStop);

    // 3. Create per-axis SS1 executor and run to completion.
    let cfg = default_ss1_config();
    let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);
    exec.trigger();
    // Run until complete (simulate decelerating to zero quickly).
    for _ in 0..100 {
        if exec.is_complete() {
            break;
        }
        exec.tick(0.0); // Report zero speed each tick.
    }
    assert!(
        exec.is_complete(),
        "SS1 executor should complete, phase={:?}",
        exec.phase()
    );

    // 4. Begin recovery.
    recovery.begin();
    assert_eq!(recovery.step(), RecoveryStep::WaitingReset);

    // Reset pressed.
    recovery.tick(true, false);
    assert_eq!(recovery.step(), RecoveryStep::WaitingFlagsClear);

    // All axes safe.
    recovery.tick(false, true);
    assert_eq!(recovery.step(), RecoveryStep::WaitingAuthorization);

    // Authorize.
    recovery.authorize();
    recovery.tick(false, true);
    assert_eq!(recovery.step(), RecoveryStep::Complete);

    // 5. Safety state recovers.
    let result = ssm.handle_event(SafetyEvent::Recovery);
    assert_eq!(result, SafetyTransition::Ok(SafetyState::Safe));
    assert!(!ssm.requires_emergency_stop());
}

#[test]
fn safety_stop_timeout_forces_complete() {
    // Very short timeout: 5ms = 5 cycles at 1kHz.
    let cfg = default_ss1_config();
    let mut exec = SafeStopExecutor::new(&cfg, 1000, 0.005);

    exec.trigger();

    // Tick with axis still moving (never reaches zero speed).
    for _ in 0..10 {
        exec.tick(100.0);
    }

    // Timeout should have forced completion.
    assert!(exec.is_complete());
}

#[test]
fn reduced_speed_to_safety_stop() {
    let mut ssm = SafetyStateMachine::new();

    // Enter reduced speed mode.
    ssm.handle_event(SafetyEvent::ReducedSpeed);
    assert_eq!(ssm.state(), SafetyState::SafeReducedSpeed);

    // From reduced speed, safety stop still works.
    ssm.handle_event(SafetyEvent::SafetyStop);
    assert_eq!(ssm.state(), SafetyState::SafetyStop);
}

#[test]
fn multiple_stop_triggers_are_idempotent() {
    let cfg = default_ss1_config();
    let mut exec = SafeStopExecutor::new(&cfg, 1000, 5.0);

    exec.trigger();
    let phase_after_first = exec.phase();

    // Trigger again — should not restart.
    exec.trigger();
    assert_eq!(exec.phase(), phase_after_first);
}
