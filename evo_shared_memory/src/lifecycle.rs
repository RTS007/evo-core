//! Lifecycle management and cleanup operations

use crate::error::ShmResult;
use crate::platform::is_process_alive;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Segment lifecycle manager trait
pub trait ShmLifecycleManager {
    /// Initialize shared memory subsystem
    fn initialize_shm_subsystem() -> ShmResult<()>;

    /// Register cleanup handler
    fn register_cleanup_handler(&self, handler: Box<dyn Fn() + Send>);

    /// Periodic cleanup of orphaned segments
    fn periodic_cleanup(&self) -> ShmResult<usize>;

    /// Emergency cleanup of all segments
    fn emergency_cleanup(&self) -> ShmResult<()>;
}

/// Cleanup coordinator for orphaned segments
pub struct SegmentCleanup {
    /// Grace period before force cleanup
    grace_period: Duration,
    /// Known segments and their metadata
    tracked_segments: HashMap<String, SegmentMetadata>,
}

/// Metadata for tracking segment lifecycle
#[derive(Debug, Clone)]
pub struct SegmentMetadata {
    /// Segment name
    pub name: String,
    /// Writer process ID
    pub writer_pid: u32,
    /// Last known access time
    pub last_access: SystemTime,
    /// Reader process IDs
    pub reader_pids: Vec<u32>,
    /// Creation time
    pub created_at: SystemTime,
}

impl SegmentCleanup {
    /// Create new cleanup coordinator
    pub fn new(grace_period: Duration) -> Self {
        Self {
            grace_period,
            tracked_segments: HashMap::new(),
        }
    }

    /// Register a segment for lifecycle tracking
    pub fn register_segment(&mut self, metadata: SegmentMetadata) {
        self.tracked_segments
            .insert(metadata.name.clone(), metadata);
    }

    /// Unregister a segment
    pub fn unregister_segment(&mut self, name: &str) {
        self.tracked_segments.remove(name);
    }

    /// Detect and cleanup orphaned segments
    pub fn cleanup_orphaned_segments(&mut self) -> ShmResult<usize> {
        let mut cleaned_count = 0;
        let mut to_remove = Vec::new();

        for (name, metadata) in &self.tracked_segments {
            if self.is_orphaned(metadata)? {
                // Check if grace period has passed
                if let Ok(elapsed) = metadata.last_access.elapsed() {
                    if elapsed > self.grace_period {
                        tracing::info!("Cleaning up orphaned segment: {}", name);
                        if self.cleanup_segment(name).is_ok() {
                            to_remove.push(name.clone());
                            cleaned_count += 1;
                        }
                    }
                }
            }
        }

        // Remove cleaned segments from tracking
        for name in to_remove {
            self.tracked_segments.remove(&name);
        }

        Ok(cleaned_count)
    }

    /// Check if segment is orphaned (writer and all readers dead)
    fn is_orphaned(&self, metadata: &SegmentMetadata) -> ShmResult<bool> {
        // Check writer process
        if is_process_alive(metadata.writer_pid) {
            return Ok(false);
        }

        // Check all reader processes
        for &reader_pid in &metadata.reader_pids {
            if is_process_alive(reader_pid) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Clean up a specific segment
    fn cleanup_segment(&self, name: &str) -> ShmResult<()> {
        // Remove from filesystem
        let shm_path = format!("/dev/shm/{}", name);
        if std::path::Path::new(&shm_path).exists() {
            std::fs::remove_file(&shm_path)?;
        }

        // Remove metadata file if exists
        let meta_path = format!("/dev/shm/{}.meta", name);
        if std::path::Path::new(&meta_path).exists() {
            std::fs::remove_file(&meta_path)?;
        }

        Ok(())
    }

    /// Update segment access time
    pub fn update_access_time(&mut self, name: &str) {
        if let Some(metadata) = self.tracked_segments.get_mut(name) {
            metadata.last_access = SystemTime::now();
        }
    }

    /// Add reader to segment
    pub fn add_reader(&mut self, name: &str, reader_pid: u32) {
        if let Some(metadata) = self.tracked_segments.get_mut(name) {
            if !metadata.reader_pids.contains(&reader_pid) {
                metadata.reader_pids.push(reader_pid);
            }
        }
    }

    /// Remove reader from segment
    pub fn remove_reader(&mut self, name: &str, reader_pid: u32) {
        if let Some(metadata) = self.tracked_segments.get_mut(name) {
            metadata.reader_pids.retain(|&pid| pid != reader_pid);
        }
    }
}

impl Default for SegmentCleanup {
    fn default() -> Self {
        Self::new(Duration::from_secs(10)) // 10-second grace period
    }
}

/// Global cleanup instance using thread-safe singleton pattern
static GLOBAL_CLEANUP: std::sync::LazyLock<std::sync::Mutex<SegmentCleanup>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(SegmentCleanup::default()));

/// Get global cleanup instance (returns a guard for thread safety)
pub fn get_global_cleanup() -> std::sync::MutexGuard<'static, SegmentCleanup> {
    GLOBAL_CLEANUP.lock().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_cleanup_creation() {
        let cleanup = SegmentCleanup::new(Duration::from_secs(5));
        assert_eq!(cleanup.grace_period, Duration::from_secs(5));
        assert!(cleanup.tracked_segments.is_empty());
    }

    #[test]
    fn test_segment_registration() {
        let mut cleanup = SegmentCleanup::default();
        let metadata = SegmentMetadata {
            name: "test_segment".to_string(),
            writer_pid: 12345,
            last_access: SystemTime::now(),
            reader_pids: vec![],
            created_at: SystemTime::now(),
        };

        cleanup.register_segment(metadata.clone());
        assert!(cleanup.tracked_segments.contains_key("test_segment"));

        cleanup.unregister_segment("test_segment");
        assert!(!cleanup.tracked_segments.contains_key("test_segment"));
    }
}
