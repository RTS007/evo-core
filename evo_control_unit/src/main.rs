//! # EVO Control Unit
//!
//! Real-time control system with shared memory integration for sub-microsecond
//! latency data sharing and inter-process coordination.

use evo_shared_memory::{
    SegmentReader, SegmentWriter, ShmResult,
    data::control::{
        CommandType, ControlCommand, ControlMode, ControlState, PIDParameters, PerformanceMetrics,
        SystemStatus,
    },
    data::segments::{CONTROL_COMMANDS, CONTROL_PERFORMANCE, CONTROL_STATE, STANDARD_SEGMENT_SIZE},
};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

fn main() -> ShmResult<()> {
    println!("EVO Control Unit starting...");

    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("Initializing EVO Control Unit with centralized data structures");

    // Create writers for control data
    let mut state_writer = SegmentWriter::create(CONTROL_STATE, STANDARD_SEGMENT_SIZE)?;
    let mut performance_writer = SegmentWriter::create(CONTROL_PERFORMANCE, STANDARD_SEGMENT_SIZE)?;

    // Create reader for commands (if any external commands come in)
    let command_reader_result = SegmentReader::attach(CONTROL_COMMANDS);
    let mut command_reader = match command_reader_result {
        Ok(reader) => Some(reader),
        Err(_) => {
            debug!("No command segment found, will operate without external commands");
            None
        }
    };

    info!("Control Unit shared memory segments initialized successfully");

    // Initialize control state
    let mut control_state = ControlState {
        position: 0.0,
        velocity: 0.0,
        acceleration: 0.0,
        target_position: 0.0,
        position_error: 0.0,
        control_output: 0.0,
        control_mode: ControlMode::Auto,
        system_status: SystemStatus::Running,
        emergency_stop: false,
        safety_interlocks: 0,
        loop_frequency: 1000.0,
        timestamp_us: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64,
        cycle_count: 0,
        pid_params: PIDParameters::default(),
    };

    let mut performance_metrics = PerformanceMetrics::default();
    let mut cycle_count = 0u64;

    info!("Starting control loop at 1000 Hz");

    // Main control loop
    loop {
        let loop_start = std::time::Instant::now();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        // Check for external commands
        if let Some(ref mut reader) = command_reader {
            if reader.has_changed() {
                if let Ok(command_data) = reader.read() {
                    if let Ok(command) = serde_json::from_slice::<ControlCommand>(&command_data) {
                        info!("Received command: {:?}", command.command_type);

                        // Process command
                        match command.command_type {
                            CommandType::MoveAbsolute => {
                                if let Some(target) = command.target_position {
                                    control_state.target_position = target;
                                    info!("Moving to position: {}", target);
                                }
                            }
                            CommandType::Stop => {
                                control_state.control_mode = ControlMode::Idle;
                                control_state.target_position = control_state.position;
                                info!("Stop command received");
                            }
                            CommandType::EmergencyStop => {
                                control_state.emergency_stop = true;
                                control_state.control_mode = ControlMode::Emergency;
                                control_state.control_output = 0.0;
                                info!("EMERGENCY STOP activated!");
                            }
                            _ => {
                                debug!("Unhandled command type: {:?}", command.command_type);
                            }
                        }
                    }
                }
            }
        }

        // Simulate control logic
        if !control_state.emergency_stop && control_state.control_mode == ControlMode::Auto {
            // Simple position control simulation
            let position_error = control_state.target_position - control_state.position;
            control_state.position_error = position_error;

            // Simple proportional controller
            control_state.control_output = position_error * control_state.pid_params.kp;

            // Simulate system response
            control_state.velocity = control_state.control_output * 0.1;
            control_state.position += control_state.velocity * 0.001; // dt = 1ms
            control_state.acceleration = control_state.velocity * 100.0; // rough estimate
        }

        // Update timestamps and counters
        control_state.timestamp_us = now;
        control_state.cycle_count = cycle_count;

        // Write control state
        let state_data = serde_json::to_vec(&control_state)?;
        state_writer.write(&state_data)?;

        // Update performance metrics
        let loop_duration = loop_start.elapsed();
        let loop_time_ns = loop_duration.as_nanos() as u64;

        performance_metrics.total_cycles += 1;
        if loop_time_ns > performance_metrics.max_loop_time_ns {
            performance_metrics.max_loop_time_ns = loop_time_ns;
        }
        if loop_time_ns < performance_metrics.min_loop_time_ns {
            performance_metrics.min_loop_time_ns = loop_time_ns;
        }

        // Update average (simple moving average)
        performance_metrics.avg_loop_time_ns =
            (performance_metrics.avg_loop_time_ns * (cycle_count - 1) + loop_time_ns)
                / cycle_count.max(1);

        performance_metrics.timestamp_us = now;

        // Write performance metrics every 100 cycles
        if cycle_count % 100 == 0 {
            let perf_data = serde_json::to_vec(&performance_metrics)?;
            performance_writer.write(&perf_data)?;

            info!(
                "Control cycle {}: pos={:.3}, target={:.3}, error={:.3}, output={:.3}",
                cycle_count,
                control_state.position,
                control_state.target_position,
                control_state.position_error,
                control_state.control_output
            );
        }

        cycle_count += 1;

        // Sleep to maintain 1000 Hz (1ms cycle time)
        thread::sleep(Duration::from_millis(1));
    }
}
