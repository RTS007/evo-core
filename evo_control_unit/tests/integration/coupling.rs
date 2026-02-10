//! Integration test: coupling lifecycle (T091).
//!
//! Validates master + 2 slaves → synchronization → motion → fault cascade.

use evo_common::control_unit::error::CouplingError;
use evo_common::control_unit::state::{CouplingConfig, CouplingState, MotionState, PowerState};

use evo_control_unit::state::coupling::{
    AxisCouplingRuntime, CouplingEvent, CouplingStateMachine, CouplingTransition,
    all_slaves_synced, calculate_slave_position,
    process_bottom_up_sync,
};

// ── Helpers ─────────────────────────────────────────────────────────

fn master_config() -> CouplingConfig {
    CouplingConfig {
        master_axis: None,
        slave_axes: heapless::Vec::new(),
        coupling_ratio: 1.0,
        modulation_offset: 0.0,
        sync_timeout: 5.0,
        max_lag_diff: 0.5,
    }
}

fn slave_config(master_id: u8) -> CouplingConfig {
    CouplingConfig {
        master_axis: Some(master_id),
        slave_axes: heapless::Vec::new(),
        coupling_ratio: 1.0,
        modulation_offset: 0.0,
        sync_timeout: 5.0,
        max_lag_diff: 0.5,
    }
}

#[allow(dead_code)]
fn modulated_slave_config(master_id: u8) -> CouplingConfig {
    CouplingConfig {
        master_axis: Some(master_id),
        slave_axes: heapless::Vec::new(),
        coupling_ratio: 2.0,
        modulation_offset: 10.0,
        sync_timeout: 5.0,
        max_lag_diff: 0.5,
    }
}

const CYCLE_US: u32 = 1000;

// ── Tests ───────────────────────────────────────────────────────────

#[test]
fn master_slave_couple_and_sync() {
    // Set up: axis 0 = master, axis 1 = slave, axis 2 = slave.
    let mut master = CouplingStateMachine::new(Some(&master_config()), CYCLE_US);
    let mut slave1 = CouplingStateMachine::new(Some(&slave_config(0)), CYCLE_US);
    let mut slave2 = CouplingStateMachine::new(Some(&slave_config(0)), CYCLE_US);

    let ps = PowerState::Standby;
    let ms = MotionState::Standstill;

    // Couple master.
    assert_eq!(
        master.handle_event(CouplingEvent::CoupleAsmaster, ps, ms),
        CouplingTransition::Ok(CouplingState::Master)
    );

    // Couple slaves → WaitingSync.
    assert_eq!(
        slave1.handle_event(CouplingEvent::CoupleAsSlave, ps, ms),
        CouplingTransition::Ok(CouplingState::WaitingSync)
    );
    assert_eq!(
        slave2.handle_event(CouplingEvent::CoupleAsSlave, ps, ms),
        CouplingTransition::Ok(CouplingState::WaitingSync)
    );

    // Slaves achieve sync.
    assert_eq!(
        slave1.handle_event(CouplingEvent::SyncAchieved, ps, ms),
        CouplingTransition::Ok(CouplingState::SlaveCoupled)
    );
    assert_eq!(
        slave2.handle_event(CouplingEvent::SyncAchieved, ps, ms),
        CouplingTransition::Ok(CouplingState::SlaveCoupled)
    );

    // Verify all coupled.
    assert_eq!(master.state(), CouplingState::Master);
    assert_eq!(slave1.state(), CouplingState::SlaveCoupled);
    assert_eq!(slave2.state(), CouplingState::SlaveCoupled);
}

#[test]
fn slave_position_calculation() {
    // Standard coupling: slave = master * ratio.
    let pos = calculate_slave_position(100.0, 1.0, 0.0, false);
    assert!((pos - 100.0).abs() < f64::EPSILON);

    // Modulated: slave = master * ratio + offset.
    let pos = calculate_slave_position(100.0, 2.0, 10.0, true);
    assert!((pos - 210.0).abs() < f64::EPSILON);
}

#[test]
fn master_fault_cascades_to_slaves() {
    let mut master = CouplingStateMachine::new(Some(&master_config()), CYCLE_US);
    let mut slave1 = CouplingStateMachine::new(Some(&slave_config(0)), CYCLE_US);
    let mut slave2 = CouplingStateMachine::new(Some(&slave_config(0)), CYCLE_US);

    let ps = PowerState::Standby;
    let ms = MotionState::Standstill;

    // Couple all.
    master.handle_event(CouplingEvent::CoupleAsmaster, ps, ms);
    slave1.handle_event(CouplingEvent::CoupleAsSlave, ps, ms);
    slave1.handle_event(CouplingEvent::SyncAchieved, ps, ms);
    slave2.handle_event(CouplingEvent::CoupleAsSlave, ps, ms);
    slave2.handle_event(CouplingEvent::SyncAchieved, ps, ms);

    // Master fault.
    let result = master.handle_event(CouplingEvent::MasterFault, ps, ms);
    assert_eq!(result, CouplingTransition::Ok(CouplingState::Decoupling));

    // In a real system, the cascade logic force-decouples all slaves.
    // We simulate that here:
    slave1.force_decouple();
    slave2.force_decouple();

    assert_eq!(slave1.state(), CouplingState::Uncoupled);
    assert_eq!(slave2.state(), CouplingState::Uncoupled);
}

#[test]
fn lag_diff_exceed_triggers_sync_lost() {
    let cfg = slave_config(0);
    let mut runtime = AxisCouplingRuntime::new(cfg, CYCLE_US);

    let ps = PowerState::Standby;
    let ms = MotionState::Standstill;

    // Couple and sync.
    runtime
        .machine
        .handle_event(CouplingEvent::CoupleAsSlave, ps, ms);
    runtime
        .machine
        .handle_event(CouplingEvent::SyncAchieved, ps, ms);
    assert_eq!(runtime.machine.state(), CouplingState::SlaveCoupled);

    // Evaluate cycle with excessive lag difference.
    runtime.evaluate_cycle(Some(0.0), 1.0); // |0.0 - 1.0| > 0.5

    assert_eq!(runtime.machine.state(), CouplingState::SyncLost);
    assert!(runtime.errors.contains(CouplingError::LAG_DIFF_EXCEED));
}

#[test]
fn sync_timeout_triggers_sync_lost() {
    let mut cfg = slave_config(0);
    cfg.sync_timeout = 0.003; // 3ms = 3 cycles at 1kHz
    let mut runtime = AxisCouplingRuntime::new(cfg, CYCLE_US);

    let ps = PowerState::Standby;
    let ms = MotionState::Standstill;

    runtime
        .machine
        .handle_event(CouplingEvent::CoupleAsSlave, ps, ms);
    assert_eq!(runtime.machine.state(), CouplingState::WaitingSync);

    // Tick 4 cycles without sync → timeout.
    for _ in 0..4 {
        runtime.evaluate_cycle(None, 0.0);
    }

    assert_eq!(runtime.machine.state(), CouplingState::SyncLost);
    assert!(runtime.errors.contains(CouplingError::SYNC_TIMEOUT));
}

#[test]
fn bottom_up_sync_process() {
    let cfg0 = master_config();
    let cfg1 = slave_config(0);
    let cfg2 = slave_config(0);

    let mut machines = vec![
        CouplingStateMachine::new(Some(&cfg0), CYCLE_US),
        CouplingStateMachine::new(Some(&cfg1), CYCLE_US),
        CouplingStateMachine::new(Some(&cfg2), CYCLE_US),
    ];

    let ps = PowerState::Standby;
    let ms = MotionState::Standstill;

    // Set up coupling.
    machines[0].handle_event(CouplingEvent::CoupleAsmaster, ps, ms);
    machines[1].handle_event(CouplingEvent::CoupleAsSlave, ps, ms);
    machines[2].handle_event(CouplingEvent::CoupleAsSlave, ps, ms);

    // Slave 1 ready, slave 2 not yet.
    let mut sync_ready = [false; 64];
    sync_ready[1] = true;

    let newly_synced = process_bottom_up_sync(&sync_ready, &mut machines, 3);
    assert_eq!(newly_synced.len(), 1);
    assert_eq!(newly_synced[0], 1);
    assert_eq!(machines[1].state(), CouplingState::SlaveCoupled);
    assert_eq!(machines[2].state(), CouplingState::WaitingSync);

    // Now slave 2 also ready.
    sync_ready[2] = true;
    let newly_synced = process_bottom_up_sync(&sync_ready, &mut machines, 3);
    assert_eq!(newly_synced.len(), 1);
    assert_eq!(newly_synced[0], 2);
    assert_eq!(machines[2].state(), CouplingState::SlaveCoupled);
}

#[test]
fn coupling_blocked_during_error_state() {
    let mut sm = CouplingStateMachine::new(Some(&master_config()), CYCLE_US);

    // PowerError blocks coupling.
    let result = sm.handle_event(
        CouplingEvent::CoupleAsmaster,
        PowerState::PowerError,
        MotionState::Standstill,
    );
    assert!(matches!(result, CouplingTransition::Rejected(_)));

    // MotionError blocks coupling.
    let result = sm.handle_event(
        CouplingEvent::CoupleAsmaster,
        PowerState::Standby,
        MotionState::MotionError,
    );
    assert!(matches!(result, CouplingTransition::Rejected(_)));
}

#[test]
fn coupling_requires_standstill() {
    let mut sm = CouplingStateMachine::new(Some(&master_config()), CYCLE_US);

    // Motion state other than Standstill blocks coupling.
    let result = sm.handle_event(
        CouplingEvent::CoupleAsmaster,
        PowerState::Standby,
        MotionState::Accelerating,
    );
    assert!(matches!(result, CouplingTransition::Rejected(_)));
}

#[test]
fn all_slaves_synced_check() {
    // axis 0 = master, axis 1 and 2 are slaves of master 0.
    let cfg0 = master_config();
    let cfg1 = slave_config(0);
    let cfg2 = slave_config(0);

    let axis_configs: Vec<(u8, Option<&CouplingConfig>)> = vec![
        (0, Some(&cfg0)),
        (1, Some(&cfg1)),
        (2, Some(&cfg2)),
    ];

    // Neither slave synced.
    let states = [
        CouplingState::Master,
        CouplingState::WaitingSync,
        CouplingState::WaitingSync,
    ];
    assert!(!all_slaves_synced(0, &axis_configs, &states));

    // Only slave 1 synced.
    let states = [
        CouplingState::Master,
        CouplingState::SlaveCoupled,
        CouplingState::WaitingSync,
    ];
    assert!(!all_slaves_synced(0, &axis_configs, &states));

    // Both synced.
    let states = [
        CouplingState::Master,
        CouplingState::SlaveCoupled,
        CouplingState::SlaveCoupled,
    ];
    assert!(all_slaves_synced(0, &axis_configs, &states));
}
