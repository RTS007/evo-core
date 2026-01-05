//! # EVO System Supervisor
//!
//! Central coordinator for the EVO system.
//! Provides comprehensive lifecycle management, health monitoring, and
//! coordination of all EVO subsystems and modules.

use evo::shm::consts::SHM_MIN_SIZE;
use evo_shared_memory::{
    SegmentDiscovery, SegmentWriter, ShmResult,
    data::system::{EvoModuleStatus, ModuleState, SystemState},
};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::signal;
use tokio::time::interval;
use tracing::{error, info};

pub struct EvoSupervisor {
    supervisor_id: String,
    system_state_writer: Option<SegmentWriter>,
    discovery: SegmentDiscovery,
    module_statuses: HashMap<String, EvoModuleStatus>,
    system_state: SystemState,
}

impl EvoSupervisor {
    pub fn new(supervisor_id: String) -> ShmResult<Self> {
        Ok(Self {
            supervisor_id,
            system_state_writer: None,
            discovery: SegmentDiscovery::new(),
            module_statuses: HashMap::new(),
            system_state: SystemState::default(),
        })
    }

    pub async fn initialize(&mut self) -> ShmResult<()> {
        info!("🔧 Initializing EVO Supervisor: {}", self.supervisor_id);

        // Create system state segment
        // Base names; SegmentWriter adds evo_ prefix and PID to avoid collisions
        self.system_state_writer = Some(SegmentWriter::create("status", SHM_MIN_SIZE)?);

        // Initialize system state
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        self.system_state.startup_timestamp_us = now;
        self.system_state.current_timestamp_us = now;

        info!("✅ EVO Supervisor initialized successfully");
        Ok(())
    }

    pub async fn run_supervisor_loop(&mut self) -> ShmResult<()> {
        let mut heartbeat = interval(Duration::from_secs(1));

        loop {
            heartbeat.tick().await;

            // Update system state
            self.update_system_state().await?;

            // Discover and monitor modules
            self.monitor_modules().await?;

            // Publish system state
            self.publish_system_state().await?;
        }
    }

    async fn update_system_state(&mut self) -> ShmResult<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        self.system_state.current_timestamp_us = now;
        Ok(())
    }

    async fn monitor_modules(&mut self) -> ShmResult<()> {
        let segments = self.discovery.list_segments()?;

        // Count different segment types
        let mut module_segments = 0;

        for segment in &segments {
            // Skip evo (watchdog) segments
            if segment.name == "status" {
                continue;
            }

            if segment.name.starts_with("module_") {
                module_segments += 1;
                // Try to read module status
                match evo_shared_memory::SegmentReader::attach(&segment.name) {
                    Ok(mut reader) => {
                        match reader.read() {
                            Ok(data) => {
                                // Find end of JSON (last '}' character) - SHM may have trailing zeros
                                let json_end = data.iter().rposition(|&b| b == b'}');
                                let json_data = match json_end {
                                    Some(pos) => &data[..=pos],
                                    None => data,
                                };

                                match serde_json::from_slice::<EvoModuleStatus>(json_data) {
                                    Ok(status) => {
                                        info!(
                                            "  📦 Module '{}': {:?} ({:?}) -> {} = {:.2}",
                                            status.module_id,
                                            status.state,
                                            status.health,
                                            "max_cycle_us",
                                            status
                                                .custom_metrics
                                                .get("max_cycle_us")
                                                .unwrap_or(&0.0)
                                        );

                                        self.module_statuses
                                            .insert(status.module_id.clone(), status);
                                    }
                                    Err(e) => {
                                        info!(
                                            "    ⚠️ Failed to parse module status for '{}': {}",
                                            segment.name, e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                info!("    ⚠️ Failed to read segment '{}': {:?}", segment.name, e);
                            }
                        }
                    }
                    Err(e) => {
                        info!(
                            "    ⚠️ Failed to attach to segment '{}': {:?}",
                            segment.name, e
                        );
                    }
                }
            }
        }

        info!("📡 Monitoring: {} modules", module_segments);

        // Update system state
        self.system_state.active_segments = segments.len() as u32;
        self.system_state.running_modules = self.module_statuses.len() as u32;

        // Count modules in error state
        self.system_state.error_modules = self
            .module_statuses
            .values()
            .filter(|s| s.state == ModuleState::Error)
            .count() as u32;

        Ok(())
    }

    async fn publish_system_state(&mut self) -> ShmResult<()> {
        if let Some(ref mut writer) = self.system_state_writer {
            let serialized = serde_json::to_vec(&self.system_state).map_err(|e| {
                evo_shared_memory::ShmError::Io {
                    source: std::io::Error::new(std::io::ErrorKind::Other, e),
                }
            })?;
            writer.write(&serialized)?;
        }
        Ok(())
    }

    pub async fn graceful_shutdown(&mut self) -> ShmResult<()> {
        info!("🛑 EVO Supervisor shutting down gracefully...");
        self.system_state.emergency_stop_active = true;
        self.publish_system_state().await?;
        Ok(())
    }

    pub fn get_system_state(&self) -> &SystemState {
        &self.system_state
    }

    pub fn get_all_module_statuses(&self) -> &HashMap<String, EvoModuleStatus> {
        &self.module_statuses
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with structured output
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();

    info!("🚀 Starting EVO System Supervisor");

    // Create supervisor instance
    let supervisor_id = "evo_supervisor_main".to_string();
    let mut supervisor = EvoSupervisor::new(supervisor_id)?;

    // Initialize supervisor and shared memory
    supervisor.initialize().await?;

    // Setup graceful shutdown handler
    let shutdown_future = async {
        match signal::ctrl_c().await {
            Ok(()) => {
                info!("🛑 Received shutdown signal (Ctrl+C)");
            }
            Err(err) => {
                error!("Unable to listen for shutdown signal: {}", err);
            }
        }
    };

    // Main supervisor loop with shutdown handling
    let supervisor_future = supervisor.run_supervisor_loop();

    // Run supervisor until shutdown signal
    tokio::select! {
        result = supervisor_future => {
            match result {
                Ok(()) => info!("Supervisor loop completed normally"),
                Err(e) => error!("Supervisor loop error: {:?}", e),
            }
        }
        _ = shutdown_future => {
            info!("Initiating graceful shutdown...");
        }
    }

    // Perform graceful shutdown
    supervisor.graceful_shutdown().await?;

    // Wait a moment for cleanup
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Display final system state
    let final_state = supervisor.get_system_state();
    info!("📊 Final System State:");
    info!("  - System Health: {:?}", final_state.overall_health);
    info!("  - Running Modules: {}", final_state.running_modules);
    info!("  - Error Modules: {}", final_state.error_modules);
    info!("  - Active Segments: {}", final_state.active_segments);
    info!("  - Emergency Stop: {}", final_state.emergency_stop_active);

    // Display module summary
    info!("📋 Module Summary:");
    for (_module_id, status) in supervisor.get_all_module_statuses() {
        info!(
            "  - {}: {:?} ({:?})",
            status.module_id, status.state, status.health
        );
    }

    info!("🏁 EVO System Supervisor shutdown complete");
    Ok(())
}
