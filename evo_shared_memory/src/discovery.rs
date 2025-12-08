//! Segment discovery and metadata management

use crate::error::{ShmError, ShmResult};
use crate::platform::is_process_alive;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Segment discovery service with filesystem monitoring
pub struct SegmentDiscovery {
    known_segments: HashMap<String, SegmentInfo>,
}

/// Segment metadata information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SegmentInfo {
    /// Segment name
    pub name: String,
    /// Data section size in bytes
    pub size: usize,
    /// Writer process ID
    pub writer_pid: u32,
    /// Creation timestamp
    pub created_at: SystemTime,
    /// Last access timestamp
    pub last_accessed: SystemTime,
    /// Active reader count
    pub reader_count: u32,
}

impl SegmentDiscovery {
    /// Create new discovery service with optional filesystem monitoring
    pub fn new() -> Self {
        Self {
            known_segments: HashMap::new(),
        }
    }

    /// List all available segments by scanning /dev/shm
    pub fn list_segments(&self) -> ShmResult<Vec<SegmentInfo>> {
        let mut segments = Vec::new();

        let shm_dir = std::path::Path::new("/dev/shm");
        if !shm_dir.exists() {
            return Ok(segments); // Return empty list if /dev/shm doesn't exist
        }

        let entries = std::fs::read_dir(shm_dir).map_err(|e| ShmError::Io { source: e })?;

        for entry in entries {
            if let Ok(entry) = entry {
                if let Ok(file_name) = entry.file_name().into_string() {
                    // Look for EVO segment files
                    if file_name.starts_with("evo_") && !file_name.ends_with(".meta") {
                        if let Ok(segment_info) = self.parse_segment_info(&file_name) {
                            segments.push(segment_info);
                        }
                    }
                }
            }
        }

        // Sort by creation time (newest first)
        segments.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(segments)
    }

    /// Find segment by name
    pub fn find_segment(&self, name: &str) -> ShmResult<Option<SegmentInfo>> {
        // Check cache first
        if let Some(info) = self.known_segments.get(name) {
            // Verify the segment still exists and writer is alive
            if self.validate_segment_info(info) {
                return Ok(Some(info.clone()));
            }
        }

        // Scan filesystem for the segment
        let segments = self.list_segments()?;
        for segment in segments {
            if segment.name == name {
                return Ok(Some(segment));
            }
        }

        Ok(None)
    }

    /// Cleanup orphaned segments
    pub fn cleanup_orphaned_segments(&mut self) -> ShmResult<usize> {
        let segments = self.list_segments()?;
        let mut cleaned_count = 0;

        for segment in segments {
            if self.is_segment_orphaned(&segment)? {
                if self.cleanup_segment(&segment.name).is_ok() {
                    cleaned_count += 1;
                    self.known_segments.remove(&segment.name);
                }
            }
        }

        Ok(cleaned_count)
    }

    /// Update cached segment information
    pub fn update_segment_cache(&mut self, info: SegmentInfo) {
        self.known_segments.insert(info.name.clone(), info);
    }

    /// Parse segment information from filesystem
    fn parse_segment_info(&self, filename: &str) -> ShmResult<SegmentInfo> {
        // Parse filename: evo_{name}_{pid}
        let parts: Vec<&str> = filename.split('_').collect();
        if parts.len() < 3 || parts[0] != "evo" {
            return Err(ShmError::NotFound {
                name: "invalid filename format".to_string(),
            });
        }

        let pid: u32 = parts[parts.len() - 1]
            .parse()
            .map_err(|_| ShmError::NotFound {
                name: "invalid PID in filename".to_string(),
            })?;

        let name = parts[1..parts.len() - 1].join("_");

        // Try to load metadata from .meta file
        let meta_path = format!("/dev/shm/evo_{}.meta", name);
        if let Ok(meta_content) = std::fs::read_to_string(&meta_path) {
            if let Ok(mut info) = serde_json::from_str::<SegmentInfo>(&meta_content) {
                // Update with current reader count from segment header if available
                if let Ok(reader_count) = self.get_current_reader_count(&filename) {
                    info.reader_count = reader_count;
                }
                return Ok(info);
            }
        }

        // Fallback: create basic info from filesystem metadata
        let segment_path = format!("/dev/shm/{}", filename);
        let file_meta = std::fs::metadata(&segment_path).map_err(|e| ShmError::Io { source: e })?;

        let created_at = file_meta
            .created()
            .or_else(|_| file_meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        Ok(SegmentInfo {
            name,
            size: file_meta.len() as usize,
            writer_pid: pid,
            created_at,
            last_accessed: created_at,
            reader_count: 0,
        })
    }

    /// Get current reader count from segment header
    fn get_current_reader_count(&self, filename: &str) -> ShmResult<u32> {
        use crate::platform::attach_segment_mmap;
        use crate::segment::SegmentHeader;

        let segment_path = format!("/dev/shm/{}", filename);
        let mmap = attach_segment_mmap(&segment_path)?;

        let header = unsafe { &*(mmap.as_ptr() as *const SegmentHeader) };
        header.validate()?;

        Ok(header.get_reader_count())
    }

    /// Check if segment is orphaned (writer and all readers dead)
    fn is_segment_orphaned(&self, info: &SegmentInfo) -> ShmResult<bool> {
        // Check if writer process is still alive
        if is_process_alive(info.writer_pid) {
            return Ok(false);
        }

        // For a more thorough check, we could track reader PIDs,
        // but for now we assume if the writer is dead and enough time
        // has passed, the segment is likely orphaned
        if let Ok(elapsed) = info.created_at.elapsed() {
            // Consider orphaned if writer dead and segment is older than 1 minute
            Ok(elapsed.as_secs() > 60)
        } else {
            Ok(true) // If we can't determine age, consider orphaned
        }
    }

    /// Validate that segment info is still accurate
    fn validate_segment_info(&self, info: &SegmentInfo) -> bool {
        // Check if writer is still alive
        is_process_alive(info.writer_pid)
    }

    /// Clean up a specific segment by name
    fn cleanup_segment(&self, name: &str) -> ShmResult<()> {
        // Find and remove segment files
        let shm_dir = std::path::Path::new("/dev/shm");
        if !shm_dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(shm_dir).map_err(|e| ShmError::Io { source: e })?;

        for entry in entries {
            if let Ok(entry) = entry {
                if let Ok(file_name) = entry.file_name().into_string() {
                    // Remove segment file and metadata file
                    if file_name.starts_with(&format!("evo_{}_", name))
                        || file_name == format!("evo_{}.meta", name)
                    {
                        let file_path = entry.path();
                        let _ = std::fs::remove_file(file_path);
                    }
                }
            }
        }

        Ok(())
    }

    /// Get discovery statistics
    pub fn get_statistics(&self) -> DiscoveryStats {
        let segments = self.list_segments().unwrap_or_default();
        let total_segments = segments.len();
        let active_writers = segments
            .iter()
            .filter(|s| is_process_alive(s.writer_pid))
            .count();
        let total_readers = segments.iter().map(|s| s.reader_count).sum();

        DiscoveryStats {
            total_segments,
            active_writers,
            total_readers,
            orphaned_segments: total_segments - active_writers,
        }
    }
}

/// Discovery statistics
#[derive(Debug, Clone)]
pub struct DiscoveryStats {
    /// Total number of segments found
    pub total_segments: usize,
    /// Number of segments with active writers
    pub active_writers: usize,
    /// Total number of active readers across all segments
    pub total_readers: u32,
    /// Number of potentially orphaned segments
    pub orphaned_segments: usize,
}

impl Default for SegmentDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::SegmentReader;
    use crate::writer::SegmentWriter;
    use evo::shm::consts::SHM_MIN_SIZE;

    #[test]
    fn test_discovery_creation() {
        let discovery = SegmentDiscovery::new();
        let stats = discovery.get_statistics();

        // Should be able to get stats without error
        println!("Found {} segments", stats.total_segments);
    }

    #[test]
    fn test_segment_discovery() {
        // Create a segment
        let test_name = format!("discovery_test_{}", std::process::id());
        let _writer = SegmentWriter::create(&test_name, SHM_MIN_SIZE).unwrap();

        let discovery = SegmentDiscovery::new();
        let segments = discovery.list_segments().unwrap();

        // Should find our segment
        let found = segments.iter().any(|s| s.name == test_name);
        assert!(found, "Should find created segment");
    }

    #[test]
    fn test_find_segment() {
        // Create a segment
        let test_name = format!("findme_test_{}", std::process::id());
        let _writer = SegmentWriter::create(&test_name, SHM_MIN_SIZE).unwrap();

        let discovery = SegmentDiscovery::new();
        let found = discovery.find_segment(&test_name).unwrap();

        assert!(found.is_some());
        let segment_info = found.unwrap();
        assert_eq!(segment_info.name, test_name);
        assert!(segment_info.writer_pid > 0);
    }

    #[test]
    fn test_statistics() {
        // Create a segment with reader using a unique name
        let test_name = format!("stats_test_{}", std::process::id());
        let _writer = SegmentWriter::create(&test_name, SHM_MIN_SIZE).unwrap();
        let _reader = SegmentReader::attach(&test_name).unwrap();

        let discovery = SegmentDiscovery::new();
        let stats = discovery.get_statistics();

        assert!(stats.total_segments > 0);
        assert!(stats.active_writers > 0);
    }

    #[test]
    fn test_cleanup_functionality() {
        let mut discovery = SegmentDiscovery::new();

        // Should complete without error (even if no orphaned segments)
        let cleaned = discovery.cleanup_orphaned_segments().unwrap();
        println!("Cleaned {} segments", cleaned);
    }
}
