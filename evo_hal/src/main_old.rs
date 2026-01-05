//! # EVO HAL Core
//!
//! Hardware Abstraction Layer with shared memory integration for real-time
//! sensor data, actuator control, and I/O management.

use evo_shared_memory::{
    SegmentWriter, ShmResult,
    data::hal::{
        ActuatorMode, ActuatorState, ActuatorStatus, CommStatus, HardwareConfig, IOBankStatus,
        SensorReading, SensorStatus,
    },
    data::segments::{
        HAL_ACTUATOR_STATE, HAL_HARDWARE_CONFIG, HAL_IO_BANK_STATUS, HAL_SENSOR_DATA,
        STANDARD_SEGMENT_SIZE,
    },
};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

fn main() -> ShmResult<()> {
    println!("EVO HAL Core starting...");

    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("Initializing EVO HAL Core with centralized data structures");

    // Create writers for HAL data
    let mut sensor_writer = SegmentWriter::create(HAL_SENSOR_DATA, STANDARD_SEGMENT_SIZE)?;
    let mut actuator_writer = SegmentWriter::create(HAL_ACTUATOR_STATE, STANDARD_SEGMENT_SIZE)?;
    let mut io_writer = SegmentWriter::create(HAL_IO_BANK_STATUS, STANDARD_SEGMENT_SIZE)?;
    let mut config_writer = SegmentWriter::create(HAL_HARDWARE_CONFIG, STANDARD_SEGMENT_SIZE)?;

    info!("HAL Core shared memory segments initialized successfully");

    // Initialize hardware configuration
    let mut config = HardwareConfig {
        version: 1,
        sensor_calibrations: HashMap::new(),
        actuator_limits: HashMap::new(),
        io_configurations: HashMap::new(),
        safety_interlocks: Vec::new(),
        update_frequency: 100.0,
        timestamp_us: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64,
    };

    // Add some example calibrations
    config
        .sensor_calibrations
        .insert("temp_01".to_string(), vec![0.1, 25.0, 0.001]);
    config
        .sensor_calibrations
        .insert("pressure_01".to_string(), vec![1.0, 0.0, 0.01]);
    config
        .actuator_limits
        .insert("motor_01".to_string(), (0.0, 100.0));

    // Write initial configuration
    let config_data = serde_json::to_vec(&config)?;
    config_writer.write(&config_data)?;
    info!("Initial hardware configuration written");

    // Main HAL operation loop
    let mut cycle_count = 0u64;

    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        // Simulate sensor readings
        let sensor_data = SensorReading {
            sensor_id: "temp_sensor_01".to_string(),
            raw_value: 25.5 + (cycle_count as f64 * 0.1).sin(),
            calibrated_value: 25.5 + (cycle_count as f64 * 0.1).sin(),
            unit: "°C".to_string(),
            timestamp_us: now,
            quality_flags: 0,
            status: SensorStatus::Normal,
            uncertainty: 0.1,
        };

        // Write sensor data
        let sensor_json = serde_json::to_vec(&sensor_data)?;
        sensor_writer.write(&sensor_json)?;

        // Simulate actuator state
        let actuator_data = ActuatorState {
            actuator_id: "motor_01".to_string(),
            current_value: 50.0 + (cycle_count as f64 * 0.05).cos() * 10.0,
            target_value: 50.0,
            output_percent: 75.0,
            status: ActuatorStatus::Active,
            timestamp_us: now,
            error_code: 0,
            mode: ActuatorMode::Auto,
        };

        // Write actuator data
        let actuator_json = serde_json::to_vec(&actuator_data)?;
        actuator_writer.write(&actuator_json)?;

        // Simulate I/O bank status
        let io_data = IOBankStatus {
            bank_id: "io_bank_01".to_string(),
            digital_inputs: 0b10101010, // Example bit pattern
            digital_outputs: 0b01010101,
            analog_inputs: vec![3.3, 2.1, 4.8, 1.2],
            analog_outputs: vec![2.5, 3.0],
            comm_status: CommStatus::Good,
            timestamp_us: now,
            config_version: config.version,
        };

        // Write I/O data
        let io_json = serde_json::to_vec(&io_data)?;
        io_writer.write(&io_json)?;

        cycle_count += 1;

        if cycle_count % 100 == 0 {
            info!(
                "HAL Core cycle {}: Sensor={:.2}°C, Actuator={:.1}%, IO=OK",
                cycle_count, sensor_data.calibrated_value, actuator_data.output_percent
            );
        }

        thread::sleep(Duration::from_millis(10)); // 100 Hz update rate
    }
}
