//! Lock-free reader implementation

use crate::error::{ShmError, ShmResult};
use crate::lifecycle::get_global_cleanup;
use crate::platform::{attach_segment_mmap, get_current_pid};
use crate::segment::{SegmentHeader, SharedMemorySegment};
use crate::version::VersionCounter;
use std::sync::atomic::{Ordering, fence};

/// Lock-free reader with conflict detection
pub struct SegmentReader {
    segment: SharedMemorySegment,
    last_seen_version: u64,
    read_buffer: Vec<u8>,
    reader_pid: u32,
}

impl SegmentReader {
    /// Attach to existing segment
    pub fn attach(name: &str) -> ShmResult<Self> {
        let reader_pid = get_current_pid();

        // Try to find segment by checking common naming patterns
        let possible_paths = Self::find_segment_paths(name)?;

        let mut segment_path = None;
        for path in possible_paths {
            if std::path::Path::new(&path).exists() {
                segment_path = Some(path);
                break;
            }
        }

        let segment_path = segment_path.ok_or_else(|| ShmError::NotFound {
            name: name.to_string(),
        })?;

        // Attach to memory mapping
        let mmap = attach_segment_mmap(&segment_path)?;

        // Validate segment header
        let header = unsafe { &*(mmap.as_ptr() as *const SegmentHeader) };
        header.validate()?;

        // Get data size from header
        let data_size = header.size as usize;

        // Create segment wrapper
        let segment = SharedMemorySegment::new(name.to_string(), data_size, mmap)?;

        // Increment reader count
        segment.header().add_reader();

        // Register with cleanup coordinator
        get_global_cleanup().add_reader(name, reader_pid);

        // Get initial version
        let initial_version = segment.header().version.load(Ordering::Acquire);

        Ok(Self {
            segment,
            last_seen_version: initial_version,
            read_buffer: Vec::with_capacity(data_size),
            reader_pid,
        })
    }

    /// Read data from segment with conflict detection
    pub fn read(&mut self) -> ShmResult<&[u8]> {
        self.read_range(0, self.segment.data_size)
    }

    /// Read data range with offset and length
    pub fn read_range(&mut self, offset: usize, len: usize) -> ShmResult<&[u8]> {
        // Validate bounds
        if offset + len > self.segment.data_size {
            return Err(ShmError::InvalidSize { size: offset + len });
        }

        let header = self.segment.header();
        let max_retries = 10;

        for _attempt in 0..max_retries {
            // Read version before data access
            let version_before = header.version.load(Ordering::Acquire);

            // Skip if write is in progress (odd version)
            if !VersionCounter::is_stable(version_before) {
                std::thread::yield_now();
                continue;
            }

            // Memory barrier before reading data
            fence(Ordering::Acquire);

            // Ensure buffer is large enough
            if self.read_buffer.len() < len {
                self.read_buffer.resize(len, 0);
            }

            // Copy data to buffer for consistency
            unsafe {
                let src_ptr = self.segment.data_ptr().add(offset);
                std::ptr::copy_nonoverlapping(src_ptr, self.read_buffer.as_mut_ptr(), len);
            }

            // Memory barrier after reading data
            fence(Ordering::Acquire);

            // Read version after data access
            let version_after = header.version.load(Ordering::Acquire);

            // Check if version changed during read (conflict detection)
            if version_before == version_after {
                // Successful read - update last seen version
                self.last_seen_version = version_after;

                // Update access time
                get_global_cleanup().update_access_time(&self.segment.name);

                return Ok(&self.read_buffer[..len]);
            }

            // Version mismatch - retry
            std::thread::yield_now();
        }

        // Too many retries - likely high write contention
        Err(ShmError::VersionConflict)
    }

    /// Get current version
    pub fn version(&self) -> u64 {
        self.last_seen_version
    }

    /// Check if data has changed since last read
    pub fn has_changed(&self) -> bool {
        let current_version = self.segment.header().version.load(Ordering::Acquire);
        current_version != self.last_seen_version && VersionCounter::is_stable(current_version)
    }

    /// Get reader process ID
    pub fn reader_pid(&self) -> u32 {
        self.reader_pid
    }

    /// Get segment name
    pub fn name(&self) -> &str {
        &self.segment.name
    }

    /// Get data size
    pub fn data_size(&self) -> usize {
        self.segment.data_size
    }

    /// Get current reader count
    pub fn reader_count(&self) -> u32 {
        self.segment.header().get_reader_count()
    }

    /// Find possible segment paths for a given name
    fn find_segment_paths(name: &str) -> ShmResult<Vec<String>> {
        let mut paths = Vec::new();

        // Try to find segments with the naming pattern /dev/shm/evo_{name}_{pid}
        let shm_dir = std::path::Path::new("/dev/shm");
        if !shm_dir.exists() {
            return Err(ShmError::NotFound {
                name: "shm directory not found".to_string(),
            });
        }

        let entries = std::fs::read_dir(shm_dir).map_err(|e| ShmError::Io { source: e })?;

        let pattern = format!("evo_{}_", name);
        for entry in entries {
            if let Ok(entry) = entry {
                if let Ok(file_name) = entry.file_name().into_string() {
                    if file_name.starts_with(&pattern) && !file_name.ends_with(".meta") {
                        paths.push(format!("/dev/shm/{}", file_name));
                    }
                }
            }
        }

        if paths.is_empty() {
            return Err(ShmError::NotFound {
                name: name.to_string(),
            });
        }

        // Sort by modification time (most recent first) to prefer active segments
        paths.sort_by(|a, b| {
            let meta_a = std::fs::metadata(a).ok();
            let meta_b = std::fs::metadata(b).ok();

            match (meta_a, meta_b) {
                (Some(a), Some(b)) => b
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                    .cmp(&a.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        Ok(paths)
    }
}

impl Drop for SegmentReader {
    fn drop(&mut self) {
        // Decrement reader count
        self.segment.header().remove_reader();

        // Remove from cleanup coordinator
        get_global_cleanup().remove_reader(&self.segment.name, self.reader_pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SHM_MIN_SIZE, writer::SegmentWriter};

    #[test]
    fn test_reader_attachment() {
        // Create a segment first
        let test_name = format!("reader_test_{}", std::process::id());
        let mut writer = SegmentWriter::create(&test_name, SHM_MIN_SIZE).unwrap();
        writer.write(b"Hello, Reader!").unwrap();

        // Attach reader
        let reader = SegmentReader::attach(&test_name);
        assert!(reader.is_ok());

        let reader = reader.unwrap();
        assert_eq!(reader.data_size(), SHM_MIN_SIZE);
        assert_eq!(reader.reader_count(), 1);
    }

    #[test]
    fn test_reader_data_access() {
        // Create segment and write data
        let test_name = format!("data_test_{}", std::process::id());
        let mut writer = SegmentWriter::create(&test_name, SHM_MIN_SIZE).unwrap();
        let test_data = b"Test data for reading";
        writer.write(test_data).unwrap();

        // Read data
        let mut reader = SegmentReader::attach(&test_name).unwrap();
        let read_data = reader.read().unwrap();

        // Verify data matches (first bytes should match)
        assert_eq!(&read_data[..test_data.len()], test_data);
        assert!(reader.version() > 0);
    }

    #[test]
    fn test_multiple_readers() {
        let pid = std::process::id();
        let name = format!("multi_reader_test_{}", pid);
        // Create segment
        let mut writer = SegmentWriter::create(&name, SHM_MIN_SIZE).unwrap();
        writer.write(b"Shared data").unwrap();

        // Attach multiple readers
        let reader1 = SegmentReader::attach(&name).unwrap();
        let reader2 = SegmentReader::attach(&name).unwrap();
        let reader3 = SegmentReader::attach(&name).unwrap();

        // Check reader count
        assert_eq!(reader1.reader_count(), 3);
        assert_eq!(reader2.reader_count(), 3);
        assert_eq!(reader3.reader_count(), 3);
    }

    #[test]
    fn test_version_tracking() {
        // Create segment and reader
        let test_name = format!("version_track_test_{}", std::process::id());
        let mut writer = SegmentWriter::create(&test_name, SHM_MIN_SIZE).unwrap();
        let mut reader = SegmentReader::attach(&test_name).unwrap();

        let initial_version = reader.version();

        // Write data
        writer.write(b"New data").unwrap();

        // Reader should detect change
        assert!(reader.has_changed());

        // Read to update version
        reader.read().unwrap();
        assert!(reader.version() > initial_version);
        assert!(!reader.has_changed());
    }

    #[test]
    fn test_read_range() {
        let pid = std::process::id();
        let name = format!("range_test_{}", pid);
        // Create segment with data
        let mut writer = SegmentWriter::create(&name, SHM_MIN_SIZE).unwrap();
        writer
            .write(b"Hello, World! This is a longer message.")
            .unwrap();

        // Read specific range
        let mut reader = SegmentReader::attach(&name).unwrap();
        let range_data = reader.read_range(7, 6).unwrap(); // "World!"

        assert_eq!(std::str::from_utf8(range_data).unwrap(), "World!");
    }

    #[test]
    fn test_nonexistent_segment() {
        let reader = SegmentReader::attach("nonexistent_segment");
        assert!(matches!(reader, Err(ShmError::NotFound { .. })));
    }
}
