//! # EVO Recipe Executor
//!
//! Recipe execution engine with shared memory integration for real-time
//! execution status, step monitoring, and command processing.

use evo_shared_memory::{
    SegmentReader, SegmentWriter, ShmResult,
    data::recipe::{
        RecipeCommand, RecipeCommandType, RecipeExecutionState, RecipeStatus, StepExecutionState,
        StepStatus, StepType,
    },
    data::segments::{RECIPE_COMMANDS, RECIPE_STATE, RECIPE_STEPS, STANDARD_SEGMENT_SIZE},
};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

fn main() -> ShmResult<()> {
    println!("EVO Recipe Executor starting...");

    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("Initializing EVO Recipe Executor with centralized data structures");

    // Create writers for recipe data
    let mut state_writer = SegmentWriter::create(RECIPE_STATE, STANDARD_SEGMENT_SIZE)?;
    let mut steps_writer = SegmentWriter::create(RECIPE_STEPS, STANDARD_SEGMENT_SIZE)?;

    // Create reader for commands (if any external commands come in)
    let command_reader_result = SegmentReader::attach(RECIPE_COMMANDS);
    let mut command_reader = match command_reader_result {
        Ok(reader) => Some(reader),
        Err(_) => {
            debug!("No command segment found, will operate without external commands");
            None
        }
    };

    info!("Recipe Executor shared memory segments initialized successfully");

    // Initialize recipe state
    let mut recipe_state = RecipeExecutionState {
        recipe_id: "demo_recipe_001".to_string(),
        recipe_name: "Demo Production Recipe".to_string(),
        status: RecipeStatus::Running,
        current_step: 0,
        total_steps: 5,
        progress_percent: 0.0,
        start_timestamp_us: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64,
        estimated_completion_us: 0,
        step_start_timestamp_us: 0,
        variables: HashMap::new(),
        error_message: String::new(),
        timestamp_us: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64,
    };

    // Initialize recipe variables
    recipe_state
        .variables
        .insert("temperature_setpoint".to_string(), 75.0);
    recipe_state
        .variables
        .insert("pressure_setpoint".to_string(), 2.5);
    recipe_state.variables.insert("flow_rate".to_string(), 1.2);

    // Initialize step states
    let mut step_states = Vec::new();
    for step_num in 0..5 {
        let step = StepExecutionState {
            step_number: step_num,
            step_name: format!("Step {}", step_num + 1),
            step_type: match step_num {
                0 => StepType::SetParameter,
                1 => StepType::Move,
                2 => StepType::Wait,
                3 => StepType::Control,
                4 => StepType::ReadSensor,
                _ => StepType::Wait,
            },
            status: if step_num == 0 {
                StepStatus::Executing
            } else {
                StepStatus::Pending
            },
            progress_percent: 0.0,
            start_timestamp_us: if step_num == 0 {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_micros() as u64
            } else {
                0
            },
            expected_duration_us: 30_000_000, // 30 seconds per step
            actual_duration_us: None,
            parameters: HashMap::new(),
            results: HashMap::new(),
            error_message: String::new(),
            retry_count: 0,
            max_retries: 3,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_micros() as u64,
        };
        step_states.push(step);
    }

    // Start first step
    recipe_state.step_start_timestamp_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;

    let mut cycle_count = 0u64;

    info!("Starting recipe execution: '{}'", recipe_state.recipe_name);

    // Main recipe execution loop
    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        // Check for external commands
        if let Some(ref mut reader) = command_reader {
            if reader.has_changed() {
                if let Ok(command_data) = reader.read() {
                    if let Ok(command) = serde_json::from_slice::<RecipeCommand>(&command_data) {
                        info!("Received recipe command: {:?}", command.command_type);

                        // Process command
                        match command.command_type {
                            RecipeCommandType::Pause => {
                                recipe_state.status = RecipeStatus::Paused;
                                info!("Recipe paused");
                            }
                            RecipeCommandType::Resume => {
                                recipe_state.status = RecipeStatus::Running;
                                info!("Recipe resumed");
                            }
                            RecipeCommandType::Stop => {
                                recipe_state.status = RecipeStatus::Aborted;
                                info!("Recipe stopped");
                            }
                            RecipeCommandType::SkipStep => {
                                if recipe_state.current_step < recipe_state.total_steps - 1 {
                                    step_states[recipe_state.current_step as usize].status =
                                        StepStatus::Skipped;
                                    recipe_state.current_step += 1;
                                    step_states[recipe_state.current_step as usize].status =
                                        StepStatus::Executing;
                                    recipe_state.step_start_timestamp_us = now;
                                    info!("Skipped to step {}", recipe_state.current_step + 1);
                                }
                            }
                            RecipeCommandType::SetVariable { name, value } => {
                                recipe_state.variables.insert(name.clone(), value);
                                info!("Set variable '{}' to {}", name, value);
                            }
                            _ => {
                                debug!("Unhandled command type: {:?}", command.command_type);
                            }
                        }
                    }
                }
            }
        }

        // Process current step if recipe is running
        if recipe_state.status == RecipeStatus::Running {
            let current_step_idx = recipe_state.current_step as usize;

            if current_step_idx < step_states.len() {
                let step = &mut step_states[current_step_idx];

                // Simulate step progress
                let step_elapsed = now - recipe_state.step_start_timestamp_us;
                let step_progress =
                    (step_elapsed as f64 / step.expected_duration_us as f64).min(1.0);

                step.progress_percent = step_progress * 100.0;
                step.timestamp_us = now;

                // Check if step is complete
                if step_progress >= 1.0 {
                    step.status = StepStatus::Completed;
                    step.actual_duration_us = Some(step_elapsed);

                    info!(
                        "Step {} completed: '{}'",
                        step.step_number + 1,
                        step.step_name
                    );

                    // Move to next step
                    recipe_state.current_step += 1;

                    if recipe_state.current_step >= recipe_state.total_steps {
                        // Recipe complete
                        recipe_state.status = RecipeStatus::Completed;
                        recipe_state.progress_percent = 100.0;
                        info!(
                            "Recipe '{}' completed successfully!",
                            recipe_state.recipe_name
                        );
                    } else {
                        // Start next step
                        step_states[recipe_state.current_step as usize].status =
                            StepStatus::Executing;
                        recipe_state.step_start_timestamp_us = now;
                        info!(
                            "Starting step {}: '{}'",
                            recipe_state.current_step + 1,
                            step_states[recipe_state.current_step as usize].step_name
                        );
                    }
                }

                // Update overall recipe progress
                let completed_steps = step_states
                    .iter()
                    .filter(|s| s.status == StepStatus::Completed)
                    .count();
                let current_step_progress = if recipe_state.current_step < recipe_state.total_steps
                {
                    step_progress
                } else {
                    1.0
                };

                recipe_state.progress_percent = ((completed_steps as f64 + current_step_progress)
                    / recipe_state.total_steps as f64)
                    * 100.0;
            }
        }

        // Update timestamps
        recipe_state.timestamp_us = now;

        // Write recipe state
        let state_data = serde_json::to_vec(&recipe_state)?;
        state_writer.write(&state_data)?;

        // Write current step state
        if (recipe_state.current_step as usize) < step_states.len() {
            let current_step = &step_states[recipe_state.current_step as usize];
            let step_data = serde_json::to_vec(current_step)?;
            steps_writer.write(&step_data)?;
        }

        // Log progress every 100 cycles
        if cycle_count % 100 == 0 {
            info!(
                "Recipe '{}': {:.1}% complete, Step {}/{} ({})",
                recipe_state.recipe_name,
                recipe_state.progress_percent,
                recipe_state.current_step + 1,
                recipe_state.total_steps,
                recipe_state.status as u8
            );
        }

        cycle_count += 1;

        // Exit if recipe is completed or aborted
        if matches!(
            recipe_state.status,
            RecipeStatus::Completed | RecipeStatus::Aborted | RecipeStatus::Failed
        ) {
            info!(
                "Recipe execution finished with status: {:?}",
                recipe_state.status
            );
            break;
        }

        thread::sleep(Duration::from_millis(100)); // 10 Hz update rate
    }

    Ok(())
}
