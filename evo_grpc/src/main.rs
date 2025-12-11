//! # EVO gRPC API Liaison Service
//!
//! Primary API gateway providing gRPC endpoints for external system integration.
//! Aggregates real-time data from EVO modules via shared memory for optimal API performance.

use evo_shared_memory::{
    SegmentReader, SegmentWriter, ShmResult,
    data::api::{
        ApiRequestMetrics, AuthStatus, ClientSession, ControlSystemSummary, HardwareSummary,
        PerformanceSummary, RecipeSummary, RequestStatus, SystemHealth, SystemStateSnapshot,
    },
    data::control::ControlState,
    data::hal::SensorReading,
    data::recipe::RecipeExecutionState,
    data::segments::*,
};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

fn main() -> ShmResult<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let instance_id = "grpc_main".to_string();
    info!("Starting EVO gRPC API Liaison Service: {}", instance_id);

    // Create writers for API data
    let mut metrics_writer = SegmentWriter::create(API_REQUEST_METRICS, STANDARD_SEGMENT_SIZE)?;
    let mut state_writer = SegmentWriter::create(API_SYSTEM_STATE, LARGE_SEGMENT_SIZE)?;
    let mut sessions_writer = SegmentWriter::create(API_CLIENT_SESSIONS, STANDARD_SEGMENT_SIZE)?;

    // Create readers for other module data
    let mut control_reader = SegmentReader::attach(CONTROL_STATE).ok();
    let mut hal_sensor_reader = SegmentReader::attach(HAL_SENSOR_DATA).ok();
    let mut recipe_reader = SegmentReader::attach(RECIPE_STATE).ok();

    info!("API Liaison shared memory segments initialized successfully");

    // Simulate API request processing and system state aggregation
    demo_api_operations(
        &mut metrics_writer,
        &mut state_writer,
        &mut sessions_writer,
        &mut control_reader,
        &mut hal_sensor_reader,
        &mut recipe_reader,
    )?;

    Ok(())
}

/// Demonstrate API operations with shared memory integration
fn demo_api_operations(
    metrics_writer: &mut SegmentWriter,
    state_writer: &mut SegmentWriter,
    sessions_writer: &mut SegmentWriter,
    control_reader: &mut Option<SegmentReader>,
    hal_reader: &mut Option<SegmentReader>,
    recipe_reader: &mut Option<SegmentReader>,
) -> ShmResult<()> {
    let mut cycle_count = 0u64;

    info!("Starting API aggregation loop");

    loop {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        // Create a sample client session
        let client_session = ClientSession {
            session_id: "session_001".to_string(),
            client_ip: "192.168.1.100".to_string(),
            user_agent: "EVO-Dashboard/1.0".to_string(),
            start_timestamp_us: current_time,
            last_activity_us: current_time,
            auth_status: AuthStatus::Authenticated,
            user_id: Some("operator_01".to_string()),
            permissions: vec!["read".to_string(), "control".to_string()],
            request_count: cycle_count,
            timeout_us: 3600_000_000,
        };

        // Write session data
        let session_data = serde_json::to_vec(&client_session)?;
        sessions_writer.write(&session_data)?;

        // Simulate API request metrics
        let request_metrics = ApiRequestMetrics {
            request_id: format!("req_{:06}", cycle_count),
            timestamp_us: current_time,
            method_name: "GetSystemStatus".to_string(),
            client_id: "dashboard_client".to_string(),
            status: RequestStatus::Completed,
            start_time_us: current_time - 1500, // 1.5ms processing time
            completion_time_us: current_time,
            duration_us: 1500,
            response_size_bytes: 2048,
            request_size_bytes: 128,
            error_code: 0,
            error_message: String::new(),
            auth_status: AuthStatus::Authenticated,
        };

        // Write metrics
        let metrics_data = serde_json::to_vec(&request_metrics)?;
        metrics_writer.write(&metrics_data)?;

        // Aggregate system state from other modules
        let mut system_snapshot = SystemStateSnapshot {
            timestamp_us: current_time,
            system_health: SystemHealth::Healthy,
            control_summary: ControlSystemSummary::default(),
            hardware_summary: HardwareSummary::default(),
            recipe_summary: RecipeSummary::default(),
            performance_summary: PerformanceSummary {
                cpu_usage: 45.5,
                memory_usage: 67.2,
                avg_response_time_us: 1500.0,
                requests_processed: cycle_count,
                failed_requests: 0,
                load_average: 1.2,
            },
            active_alarms: Vec::new(),
            uptime_us: current_time,
        };

        // Read control data if available
        if let Some(reader) = control_reader.as_mut() {
            if let Ok(control_data) = reader.read() {
                if let Ok(control_state) = serde_json::from_slice::<ControlState>(&control_data) {
                    system_snapshot.control_summary = ControlSystemSummary {
                        control_mode: format!("{:?}", control_state.control_mode),
                        current_position: control_state.position,
                        target_position: control_state.target_position,
                        position_error: control_state.position_error,
                        emergency_stop: control_state.emergency_stop,
                        loop_frequency: control_state.loop_frequency,
                        active_interlocks: control_state.safety_interlocks,
                    };
                    debug!("Updated control summary from shared memory");
                }
            }
        }

        // Read HAL data if available
        if let Some(reader) = hal_reader.as_mut() {
            if let Ok(hal_data) = reader.read() {
                if let Ok(_sensor_reading) = serde_json::from_slice::<SensorReading>(&hal_data) {
                    system_snapshot.hardware_summary = HardwareSummary {
                        active_sensors: 5,
                        active_actuators: 3,
                        sensors_in_error: 0,
                        actuators_in_error: 0,
                        sensor_update_freq: 100.0,
                        comm_status: "Good".to_string(),
                    };
                    debug!("Updated hardware summary from shared memory");
                }
            }
        }

        // Read recipe data if available
        if let Some(reader) = recipe_reader.as_mut() {
            if let Ok(recipe_data) = reader.read() {
                if let Ok(recipe_state) =
                    serde_json::from_slice::<RecipeExecutionState>(&recipe_data)
                {
                    system_snapshot.recipe_summary = RecipeSummary {
                        active_recipes: 1,
                        current_recipe: recipe_state.recipe_name.clone(),
                        current_progress: recipe_state.progress_percent,
                        execution_status: format!("{:?}", recipe_state.status),
                        steps_completed: recipe_state.current_step,
                        total_steps: recipe_state.total_steps,
                    };
                    debug!("Updated recipe summary from shared memory");
                }
            }
        }

        // Write aggregated system state
        let state_data = serde_json::to_vec(&system_snapshot)?;
        state_writer.write(&state_data)?;

        cycle_count += 1;

        if cycle_count % 50 == 0 {
            info!(
                "API Liaison cycle {}: Processed {} requests, System Health: {:?}",
                cycle_count, cycle_count, system_snapshot.system_health
            );
        }

        thread::sleep(Duration::from_millis(200)); // 5 Hz aggregation rate

        if cycle_count > 500 {
            info!("Demo completed after {} cycles", cycle_count);
            break;
        }
    }

    Ok(())
}
