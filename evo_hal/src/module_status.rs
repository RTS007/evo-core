//! Module status publisher for EVO supervisor integration.
//!
//! This module handles publishing HAL status to the EVO supervisor
//! via shared memory, enabling centralized monitoring and coordination.

use evo_shared_memory::{
    SegmentWriter, ShmResult,
    data::system::{EvoModuleStatus, ModuleHealth, ModuleState, ModuleType},
};
use std::collections::HashMap;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Default heartbeat interval in microseconds (100ms)
const DEFAULT_HEARTBEAT_INTERVAL_US: u64 = 100_000;

/// SHM segment size for module status (~4KB)
const MODULE_STATUS_SHM_SIZE: usize = 4096;

/// Module status publisher for HAL.
pub struct ModuleStatusPublisher {
    /// Status segment writer
    writer: Option<SegmentWriter>,
    /// Module status data
    status: EvoModuleStatus,
    /// Last publish timestamp
    last_publish_us: u64,
    /// Publish interval
    publish_interval_us: u64,
}

impl ModuleStatusPublisher {
    /// Create a new module status publisher.
    ///
    /// # Arguments
    /// * `module_id` - Unique identifier for this HAL instance
    pub fn new(module_id: &str) -> Self {
        let now_us = current_timestamp_us();
        
        Self {
            writer: None,
            status: EvoModuleStatus {
                module_id: module_id.to_string(),
                module_type: ModuleType::HalCore,
                process_id: process::id(),
                state: ModuleState::Starting,
                health: ModuleHealth::Healthy,
                startup_timestamp_us: now_us,
                last_heartbeat_us: now_us,
                heartbeat_interval_us: DEFAULT_HEARTBEAT_INTERVAL_US,
                version: env!("CARGO_PKG_VERSION").to_string(),
                managed_segments: vec![],
                cpu_usage: 0.0,
                memory_usage: 0,
                active_connections: 0,
                custom_metrics: HashMap::new(),
                error_info: None,
            },
            last_publish_us: 0,
            publish_interval_us: DEFAULT_HEARTBEAT_INTERVAL_US,
        }
    }

    /// Initialize the status publisher - creates SHM segment.
    ///
    /// # Arguments
    /// * `hal_segment_name` - Name of the main HAL SHM segment (added to managed_segments)
    pub fn init(&mut self, hal_segment_name: &str) -> ShmResult<()> {
        // Create status segment with module-specific base name (SegmentWriter prefixes evo_ and PID)
        let segment_name = format!("module_{}", self.status.module_id);
        info!("Creating module status segment: {}", segment_name);
        
        self.writer = Some(SegmentWriter::create(&segment_name, MODULE_STATUS_SHM_SIZE)?);
        
        // Add managed segments (base names; actual files get evo_ prefix + PID)
        self.status.managed_segments.push(hal_segment_name.to_string());
        self.status.managed_segments.push(segment_name);
        
        // Update state to running
        self.status.state = ModuleState::Running;
        
        // Publish initial status
        self.publish()?;
        
        info!("Module status publisher initialized");
        Ok(())
    }

    /// Update and publish status.
    ///
    /// This should be called periodically from the RT loop.
    pub fn update(&mut self) -> ShmResult<()> {
        let now_us = current_timestamp_us();
        
        // Only publish at configured interval
        if now_us - self.last_publish_us < self.publish_interval_us {
            return Ok(());
        }
        
        self.status.last_heartbeat_us = now_us;
        self.publish()
    }

    /// Publish current status to SHM.
    fn publish(&mut self) -> ShmResult<()> {
        if let Some(ref mut writer) = self.writer {
            let serialized = serde_json::to_vec(&self.status).map_err(|e| {
                evo_shared_memory::ShmError::Io {
                    source: std::io::Error::new(std::io::ErrorKind::Other, e),
                }
            })?;
            writer.write(&serialized)?;
            self.last_publish_us = current_timestamp_us();
            debug!("Published module status (state={:?})", self.status.state);
        }
        Ok(())
    }

    /// Set module state.
    pub fn set_state(&mut self, state: ModuleState) {
        self.status.state = state;
    }

    /// Set module health.
    pub fn set_health(&mut self, health: ModuleHealth) {
        self.status.health = health;
    }

    /// Set a custom metric.
    pub fn set_metric(&mut self, name: &str, value: f64) {
        self.status.custom_metrics.insert(name.to_string(), value);
    }

    /// Update timing metrics from RT loop statistics.
    pub fn update_timing_metrics(&mut self, cycle_count: u64, avg_cycle_us: u64, max_cycle_us: u64, violations: u64) {
        self.status.custom_metrics.insert("cycle_count".to_string(), cycle_count as f64);
        self.status.custom_metrics.insert("avg_cycle_us".to_string(), avg_cycle_us as f64);
        self.status.custom_metrics.insert("max_cycle_us".to_string(), max_cycle_us as f64);
        self.status.custom_metrics.insert("timing_violations".to_string(), violations as f64);
        
        // Update health based on timing violations
        let violation_rate = if cycle_count > 0 {
            violations as f64 / cycle_count as f64
        } else {
            0.0
        };
        
        if violation_rate > 0.1 {
            self.status.health = ModuleHealth::Critical;
        } else if violation_rate > 0.01 {
            self.status.health = ModuleHealth::Degraded;
        } else if violation_rate > 0.001 {
            self.status.health = ModuleHealth::Warning;
        } else {
            self.status.health = ModuleHealth::Healthy;
        }
    }

    /// Set error state with message.
    pub fn set_error(&mut self, error_msg: &str) {
        self.status.state = ModuleState::Error;
        self.status.health = ModuleHealth::Critical;
        self.status.error_info = Some(evo_shared_memory::data::system::ModuleError {
            error_code: 1,
            message: error_msg.to_string(),
            timestamp_us: current_timestamp_us(),
            source: "evo_hal".to_string(),
            stack_trace: None,
            recovery_suggestions: vec!["Check logs for details".to_string()],
        });
        warn!("Module error set: {}", error_msg);
    }

    /// Clear error state.
    pub fn clear_error(&mut self) {
        self.status.error_info = None;
        if self.status.state == ModuleState::Error {
            self.status.state = ModuleState::Running;
        }
        self.status.health = ModuleHealth::Healthy;
    }

    /// Shutdown - set stopping state and publish final status.
    pub fn shutdown(&mut self) -> ShmResult<()> {
        info!("Module status publisher shutting down");
        self.status.state = ModuleState::Stopping;
        self.publish()?;
        
        // Final state
        self.status.state = ModuleState::Stopped;
        self.publish()?;
        
        Ok(())
    }
}

/// Get current timestamp in microseconds since UNIX epoch.
fn current_timestamp_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_status_publisher_new() {
        let publisher = ModuleStatusPublisher::new("hal_test");
        assert_eq!(publisher.status.module_id, "hal_test");
        assert_eq!(publisher.status.module_type, ModuleType::HalCore);
        assert_eq!(publisher.status.state, ModuleState::Starting);
    }

    #[test]
    fn test_health_update_from_timing() {
        let mut publisher = ModuleStatusPublisher::new("hal_test");
        
        // No violations - healthy
        publisher.update_timing_metrics(1000, 500, 800, 0);
        assert_eq!(publisher.status.health, ModuleHealth::Healthy);
        
        // Few violations - warning
        publisher.update_timing_metrics(1000, 500, 1200, 5);
        assert_eq!(publisher.status.health, ModuleHealth::Warning);
        
        // Many violations - degraded
        publisher.update_timing_metrics(1000, 500, 1500, 20);
        assert_eq!(publisher.status.health, ModuleHealth::Degraded);
        
        // Critical violations
        publisher.update_timing_metrics(1000, 500, 2000, 150);
        assert_eq!(publisher.status.health, ModuleHealth::Critical);
    }
}
