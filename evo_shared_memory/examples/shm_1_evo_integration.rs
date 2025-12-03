//! EVO module integration patterns example
//!
//! Demonstrates how to integrate the shared memory system with various EVO modules
//! including Control Unit, HAL Core, Recipe Executor, and GRPC API integration.

use evo_shared_memory::{SegmentReader, SegmentWriter, ShmError, ShmResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

/// Recipe execution state for Recipe Executor integration
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RecipeState {
    recipe_id: u32,
    step_index: u32,
    state: ExecutionState,
    progress: f32,
    start_time: u64,
    estimated_completion: u64,
    error_code: u32,
}

#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum ExecutionState {
    Idle = 0,
    Running = 1,
    Paused = 2,
    Completed = 3,
    Error = 4,
}

/// Control command for Control Unit integration
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ControlCommand {
    target_module: u32,
    command_type: u32,
    parameters: [f64; 8],
    timestamp: u64,
    priority: u8,
}

/// Hardware status for HAL Core integration
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct HardwareStatus {
    device_id: u32,
    status_flags: u32,
    temperature: f32,
    voltage: f32,
    current: f32,
    fault_count: u32,
    last_maintenance: u64,
}

fn serialize_data<T: Serialize>(data: &T) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(serde_json::to_vec(data)?)
}

fn deserialize_data<'a, T: Deserialize<'a>>(
    data: &'a [u8],
) -> Result<T, Box<dyn std::error::Error>> {
    // Create streaming deserializer.
    // It will read one valid JSON object and stop,
    // ignoring anything (zeros, garbage, old data) that comes after.
    let mut stream = serde_json::Deserializer::from_slice(data).into_iter::<T>();

    match stream.next() {
        Some(result) => Ok(result?),
        None => Err("Nie znaleziono poprawnego JSON-a w buforze".into()),
    }
}

/// EVO Integration Manager
struct EVOIntegrationManager {
    segments: HashMap<String, String>, // segment_name -> description
}

impl EVOIntegrationManager {
    fn new() -> ShmResult<Self> {
        Ok(Self {
            segments: HashMap::new(),
        })
    }

    fn register_segment(&mut self, name: &str, description: &str) {
        self.segments
            .insert(name.to_string(), description.to_string());
        println!("Registered segment '{}': {}", name, description);
    }

    fn get_segment_info(&self) -> Vec<(String, String)> {
        self.segments
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn cleanup_all(&mut self) -> ShmResult<()> {
        println!("Cleaning up all segments...");
        // In a real app, we might trigger cleanup via the lifecycle manager
        // For this example, we'll just clear our list
        self.segments.clear();
        Ok(())
    }
}

fn main() -> ShmResult<()> {
    println!("EVO Shared Memory - Module Integration Example");
    println!("=============================================");

    let mut integration_manager = EVOIntegrationManager::new()?;

    // Start EVO modules
    let control_unit_handle = thread::spawn(|| {
        control_unit_integration().unwrap_or_else(|e| {
            eprintln!("Control Unit error: {}", e);
        });
    });

    let hal_core_handle = thread::spawn(|| {
        hal_core_integration().unwrap_or_else(|e| {
            eprintln!("HAL Core error: {}", e);
        });
    });

    let recipe_executor_handle = thread::spawn(|| {
        recipe_executor_integration().unwrap_or_else(|e| {
            eprintln!("Recipe Executor error: {}", e);
        });
    });

    let api_gateway_handle = thread::spawn(|| {
        api_gateway_integration().unwrap_or_else(|e| {
            eprintln!("API Gateway error: {}", e);
        });
    });

    // Register all segments
    integration_manager.register_segment("control_commands", "Control Unit command distribution");
    integration_manager.register_segment("hardware_status", "HAL Core hardware monitoring");
    integration_manager.register_segment("recipe_state", "Recipe Executor state tracking");
    integration_manager.register_segment("api_responses", "GRPC API response caching");

    // Monitor integration for 15 seconds
    thread::sleep(Duration::from_secs(15));

    // Print integration summary
    println!("\nEVO Integration Summary:");
    for (name, desc) in integration_manager.get_segment_info() {
        println!("  {}: {}", name, desc);
    }

    // Wait for modules to complete
    control_unit_handle.join().unwrap();
    hal_core_handle.join().unwrap();
    recipe_executor_handle.join().unwrap();
    api_gateway_handle.join().unwrap();

    // Cleanup
    integration_manager.cleanup_all()?;

    println!("EVO integration example completed!");
    Ok(())
}

/// Control Unit integration - manages system-wide control commands
fn control_unit_integration() -> ShmResult<()> {
    println!("Starting Control Unit integration...");

    let mut command_writer = SegmentWriter::create("control_commands", 1000)?;
    let mut command_counter = 0;

    for cycle in 0..50 {
        // 15 second test at ~3Hz
        let command = ControlCommand {
            target_module: cycle % 4,                          // Rotate between modules
            command_type: if cycle % 10 == 0 { 2 } else { 1 }, // Periodic config updates
            parameters: [
                cycle as f64,
                (cycle as f64 * 0.1).sin() * 100.0,
                50.0 + (cycle as f64 * 0.2).cos() * 25.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            priority: if cycle % 10 == 0 { 255 } else { 100 }, // High priority config updates
        };

        let data = serialize_data(&command).unwrap();
        command_writer.write(&data)?;
        command_counter += 1;

        if command_counter % 10 == 0 {
            println!(
                "Control Unit: Sent {} commands (target module: {})",
                command_counter, command.target_module
            );
        }

        thread::sleep(Duration::from_millis(300));
    }

    println!("Control Unit: Completed {} commands", command_counter);
    Ok(())
}

/// HAL Core integration - hardware monitoring and status reporting
fn hal_core_integration() -> ShmResult<()> {
    println!("Starting HAL Core integration...");

    thread::sleep(Duration::from_millis(50)); // Slight startup delay

    let mut status_writer = SegmentWriter::create("hardware_status", 1000)?;
    let devices = [0x1001, 0x1002, 0x1003, 0x1004]; // Device IDs
    let mut update_counter = 0;

    for cycle in 0..75 {
        // 15 second test at 5Hz
        for &device_id in &devices {
            // Simulate device status
            let temp = 45.0 + (cycle as f32 * 0.1).sin() * 5.0;
            let voltage = 24.0 + (cycle as f32 * 0.05).cos() * 0.5;
            let current = 2.5 + (cycle as f32 * 0.03).sin() * 0.2;

            let status = HardwareStatus {
                device_id,
                status_flags: if temp > 50.0 { 0x2 } else { 0x1 }, // Overtemp warning
                temperature: temp,
                voltage,
                current,
                fault_count: if cycle > 40 && device_id == 0x1003 {
                    1
                } else {
                    0
                },
                last_maintenance: 1700000000, // Fixed timestamp
            };

            let data = serialize_data(&status).unwrap();
            status_writer.write(&data)?;
            update_counter += 1;
        }

        if update_counter % 20 == 0 {
            println!("HAL Core: {} status updates sent", update_counter);
        }

        thread::sleep(Duration::from_millis(200));
    }

    println!("HAL Core: Completed {} status updates", update_counter);
    Ok(())
}

/// Recipe Executor integration - recipe state tracking
fn recipe_executor_integration() -> ShmResult<()> {
    println!("Starting Recipe Executor integration...");

    thread::sleep(Duration::from_millis(100)); // Startup delay

    let mut state_writer = SegmentWriter::create("recipe_state", 1000)?;
    let recipe_id = 12345;
    let total_steps = 10;

    // Initial state
    let mut current_state = RecipeState {
        recipe_id,
        step_index: 0,
        state: ExecutionState::Idle,
        progress: 0.0,
        start_time: 0,
        estimated_completion: 0,
        error_code: 0,
    };

    let data = serialize_data(&current_state).unwrap();
    state_writer.write(&data)?;
    println!("Recipe Executor: Recipe {} initialized", recipe_id);

    thread::sleep(Duration::from_secs(1));

    // Start execution
    current_state.state = ExecutionState::Running;
    current_state.start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    current_state.estimated_completion = current_state.start_time + 10; // 10 second recipe

    for step in 0..total_steps {
        current_state.step_index = step;
        current_state.progress = (step as f32) / (total_steps as f32) * 100.0;

        // Simulate step execution
        let data = serialize_data(&current_state).unwrap();
        state_writer.write(&data)?;
        println!(
            "Recipe Executor: Step {}/{} ({:.1}%)",
            step + 1,
            total_steps,
            current_state.progress
        );

        // Simulate step processing time
        thread::sleep(Duration::from_millis(1200));
    }

    // Complete recipe
    current_state.state = ExecutionState::Completed;
    current_state.progress = 100.0;
    let data = serialize_data(&current_state).unwrap();
    state_writer.write(&data)?;

    println!(
        "Recipe Executor: Recipe {} completed successfully",
        recipe_id
    );
    Ok(())
}

/// API Gateway integration - GRPC response caching
fn api_gateway_integration() -> ShmResult<()> {
    println!("Starting API Gateway integration...");

    thread::sleep(Duration::from_millis(200)); // Allow other modules to start

    // Monitor various segments and cache responses
    let mut command_reader = loop {
        match SegmentReader::attach("control_commands") {
            Ok(r) => break r,
            Err(ShmError::NotFound { .. }) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e),
        }
    };

    let mut status_reader = loop {
        match SegmentReader::attach("hardware_status") {
            Ok(r) => break r,
            Err(ShmError::NotFound { .. }) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e),
        }
    };

    let mut recipe_reader = loop {
        match SegmentReader::attach("recipe_state") {
            Ok(r) => break r,
            Err(ShmError::NotFound { .. }) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e),
        }
    };

    let mut api_request_count = 0;
    let start_time = Instant::now();

    while start_time.elapsed() < Duration::from_secs(14) {
        api_request_count += 1;

        // Simulate API request processing by reading latest data
        let mut response_data = Vec::new();

        // Get latest control command
        if command_reader.has_changed() {
            if let Ok(bytes) = command_reader.read() {
                if let Ok(command) = deserialize_data::<ControlCommand>(bytes) {
                    response_data.push(format!(
                        "Command: module={}, type={}, priority={}",
                        command.target_module, command.command_type, command.priority
                    ));
                }
            }
        }

        // Get latest hardware status
        if status_reader.has_changed() {
            if let Ok(bytes) = status_reader.read() {
                if let Ok(status) = deserialize_data::<HardwareStatus>(bytes) {
                    response_data.push(format!(
                        "HW Status: device=0x{:X}, temp={:.1}°C, status=0x{:X}",
                        status.device_id, status.temperature, status.status_flags
                    ));
                }
            }
        }

        // Get latest recipe state
        if recipe_reader.has_changed() {
            if let Ok(bytes) = recipe_reader.read() {
                if let Ok(recipe) = deserialize_data::<RecipeState>(bytes) {
                    response_data.push(format!(
                        "Recipe: id={}, step={}, progress={:.1}%, state={:?}",
                        recipe.recipe_id, recipe.step_index, recipe.progress, recipe.state
                    ));
                }
            }
        }

        if !response_data.is_empty() && api_request_count % 50 == 0 {
            println!(
                "API Gateway: Processed {} requests, latest data:",
                api_request_count
            );
            for data in &response_data {
                println!("  {}", data);
            }
        }

        // Simulate API processing delay
        thread::sleep(Duration::from_millis(100));
    }

    println!(
        "API Gateway: Processed {} total requests",
        api_request_count
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evo_integration_manager() -> ShmResult<()> {
        let mut manager = EVOIntegrationManager::new()?;

        manager.register_segment("test_segment", "Test description");
        let segments = manager.get_segment_info();

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].0, "test_segment");
        assert_eq!(segments[0].1, "Test description");

        Ok(())
    }

    #[test]
    fn test_control_command_creation() {
        let command = ControlCommand {
            target_module: 1,
            command_type: 100,
            parameters: [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            timestamp: 123456789,
            priority: 200,
        };

        assert_eq!(command.target_module, 1);
        assert_eq!(command.command_type, 100);
        assert_eq!(command.priority, 200);
    }

    #[test]
    fn test_recipe_state_transitions() {
        let mut state = RecipeState {
            recipe_id: 1,
            step_index: 0,
            state: ExecutionState::Idle,
            progress: 0.0,
            start_time: 0,
            estimated_completion: 0,
            error_code: 0,
        };

        assert_eq!(state.state, ExecutionState::Idle);

        state.state = ExecutionState::Running;
        assert_eq!(state.state, ExecutionState::Running);

        state.state = ExecutionState::Completed;
        assert_eq!(state.state, ExecutionState::Completed);
    }

    #[test]
    fn test_hardware_status_flags() {
        let normal_status = HardwareStatus {
            device_id: 0x1001,
            status_flags: 0x1, // Normal operation
            temperature: 25.0,
            voltage: 24.0,
            current: 2.0,
            fault_count: 0,
            last_maintenance: 0,
        };

        assert_eq!(normal_status.status_flags & 0x1, 0x1); // Normal flag set
        assert_eq!(normal_status.fault_count, 0);

        let warning_status = HardwareStatus {
            device_id: 0x1002,
            status_flags: 0x3, // Normal + Warning
            temperature: 55.0, // High temp
            voltage: 24.0,
            current: 2.0,
            fault_count: 1,
            last_maintenance: 0,
        };

        assert_eq!(warning_status.status_flags & 0x2, 0x2); // Warning flag set
        assert_eq!(warning_status.fault_count, 1);
    }
}
