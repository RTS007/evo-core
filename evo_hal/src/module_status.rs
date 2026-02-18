//! Module status publisher for EVO supervisor integration (stub).
//!
//! No-op implementation — will use P2P segments in Phase 8 (T058–T065).

use tracing::debug;

/// Module status publisher for HAL (no-op stub).
pub struct ModuleStatusPublisher {
    module_id: String,
}

impl ModuleStatusPublisher {
    /// Create a new (no-op) module status publisher.
    pub fn new(module_id: &str) -> Self {
        Self {
            module_id: module_id.to_string(),
        }
    }

    /// Initialize (no-op). Returns Ok always.
    pub fn init(&mut self, _segment_name: &str) -> Result<(), String> {
        debug!("ModuleStatusPublisher::init stub for {}", self.module_id);
        Ok(())
    }

    /// Update timing metrics (no-op).
    pub fn update_timing_metrics(
        &mut self,
        _cycle_count: u64,
        _avg_cycle_us: u64,
        _max_cycle_us: u64,
        _timing_violations: u64,
    ) {
    }

    /// Publish status (no-op). Returns Ok always.
    pub fn update(&mut self) -> Result<(), String> {
        Ok(())
    }

    /// Shutdown (no-op). Returns Ok always.
    pub fn shutdown(&mut self) -> Result<(), String> {
        debug!("ModuleStatusPublisher::shutdown stub for {}", self.module_id);
        Ok(())
    }
}
