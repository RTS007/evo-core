//! Single writer implementation with exclusive ownership

use crate::error::{ShmError, ShmResult};
use crate::lifecycle::{SegmentMetadata, get_global_cleanup};
use crate::platform::{LinuxMemoryConfig, create_segment_mmap, get_current_pid};
use crate::segment::{SegmentHeader, SharedMemorySegment, validate_segment_size};
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{Ordering, fence};
use std::time::{SystemTime, UNIX_EPOCH};

/// Single writer with exclusive segment ownership
pub struct SegmentWriter {
    segment: SharedMemorySegment,
    current_version: u64,
    writer_pid: u32,
}

impl SegmentWriter {
    /// Create new writer with exclusive segment ownership
    pub fn create(name: &str, size: usize) -> ShmResult<Self> {
        // Validate segment size
        validate_segment_size(size)?;

        // Get current process ID
        let writer_pid = get_current_pid();

        // Create segment path with collision prevention
        let segment_path = format!("/dev/shm/evo_{}_{}", name, writer_pid);

        // Check if segment already exists
        if std::path::Path::new(&segment_path).exists() {
            return Err(ShmError::AlreadyExists {
                name: name.to_string(),
            });
        }

        // Calculate total size including header
        let header_size = std::mem::size_of::<SegmentHeader>();
        let total_size = size + header_size;

        // Create memory mapping with Linux optimizations
        let config = LinuxMemoryConfig::default();
        let mut mmap = create_segment_mmap(&segment_path, total_size, &config)?;

        // Initialize segment header
        {
            let header = unsafe { &mut *(mmap.as_mut_ptr() as *mut SegmentHeader) };
            *header = SegmentHeader::new(size, writer_pid);
        }

        // Memory barrier to ensure header is written
        fence(Ordering::Release);

        // Create segment wrapper
        let segment = SharedMemorySegment::new(name.to_string(), size, mmap)?;

        // Create metadata file for discovery
        Self::create_metadata_file(name, size, writer_pid)?;

        // Register with cleanup coordinator
        let metadata = SegmentMetadata {
            name: name.to_string(),
            writer_pid,
            last_access: SystemTime::now(),
            reader_pids: vec![],
            created_at: SystemTime::now(),
        };
        get_global_cleanup().register_segment(metadata);

        Ok(Self {
            segment,
            current_version: 0,
            writer_pid,
        })
    }

    /// Write data to segment with sub-microsecond latency
    pub fn write(&mut self, data: &[u8]) -> ShmResult<()> {
        self.write_at(0, data)
    }

    /// Write data at specific offset
    pub fn write_at(&mut self, offset: usize, data: &[u8]) -> ShmResult<()> {
        // Validate bounds
        if offset + data.len() > self.segment.data_size {
            return Err(ShmError::InvalidSize {
                size: offset + data.len(),
            });
        }

        // Get header and data pointers separately to avoid borrowing conflicts
        let header_ptr = self.segment.header() as *const SegmentHeader;
        let data_ptr = unsafe { self.segment.data_ptr_mut().add(offset) };

        // Begin write operation (increment to odd version)
        let current_version = unsafe { (*header_ptr).version.load(Ordering::Acquire) };
        let write_version = current_version + 1;
        unsafe {
            (*header_ptr)
                .version
                .store(write_version, Ordering::Release)
        };
        self.current_version = write_version;

        // Memory barrier before data write
        fence(Ordering::Release);

        // Copy data to segment
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), data_ptr, data.len());
        }

        // Memory barrier after data write
        fence(Ordering::Release);

        // Complete write operation (increment to even version)
        let final_version = write_version + 1;
        unsafe {
            (*header_ptr)
                .version
                .store(final_version, Ordering::Release)
        };
        self.current_version = final_version;

        // Update last write timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        unsafe { (*header_ptr).last_write_ts.store(now, Ordering::Release) };

        // Update access time in cleanup coordinator
        get_global_cleanup().update_access_time(&self.segment.name);

        Ok(())
    }

    /// Flush writes with memory barriers
    pub fn flush(&mut self) -> ShmResult<()> {
        // Full memory barrier to ensure all writes are visible
        fence(Ordering::SeqCst);
        Ok(())
    }

    /// Get current version
    pub fn current_version(&self) -> u64 {
        self.current_version
    }

    /// Get writer process ID
    pub fn writer_pid(&self) -> u32 {
        self.writer_pid
    }

    /// Get segment name
    pub fn name(&self) -> &str {
        &self.segment.name
    }

    /// Get data size
    pub fn data_size(&self) -> usize {
        self.segment.data_size
    }

    /// Create JSON metadata file for discovery
    fn create_metadata_file(name: &str, size: usize, writer_pid: u32) -> ShmResult<()> {
        use crate::discovery::SegmentInfo;

        let metadata = SegmentInfo {
            name: name.to_string(),
            size,
            writer_pid,
            created_at: SystemTime::now(),
            last_accessed: SystemTime::now(),
            reader_count: 0,
        };

        let metadata_path = format!("/dev/shm/evo_{}.meta", name);
        let metadata_json = serde_json::to_string_pretty(&metadata).map_err(|e| ShmError::Io {
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;

        // Create metadata file with exclusive access
        let mut file = OpenOptions::new()
            .create_new(true) // Fail if already exists
            .write(true)
            .mode(0o600) // Owner read/write only
            .open(&metadata_path)?;

        std::io::Write::write_all(&mut file, metadata_json.as_bytes())?;

        Ok(())
    }
}

impl Drop for SegmentWriter {
    fn drop(&mut self) {
        // Cleanup on writer process exit
        let segment_path = format!("/dev/shm/evo_{}_{}", self.segment.name, self.writer_pid);
        let metadata_path = format!("/dev/shm/evo_{}.meta", self.segment.name);

        // Remove files
        let _ = std::fs::remove_file(segment_path);
        let _ = std::fs::remove_file(metadata_path);

        // Unregister from cleanup coordinator
        get_global_cleanup().unregister_segment(&self.segment.name);
    }
}

#[cfg(test)]
mod tests {
    use evo::shm::consts::SHM_MIN_SIZE;

    use super::*;

    #[test]
    fn test_writer_creation() {
        let writer = SegmentWriter::create("test_segment", SHM_MIN_SIZE);
        assert!(writer.is_ok());

        let writer = writer.unwrap();
        assert_eq!(writer.data_size(), SHM_MIN_SIZE);
        assert_eq!(writer.current_version(), 0);
        assert!(writer.writer_pid() > 0);
    }

    #[test]
    fn test_exclusive_creation() {
        let _writer1 = SegmentWriter::create("exclusive_test", SHM_MIN_SIZE).unwrap();

        // Second writer should fail
        let writer2 = SegmentWriter::create("exclusive_test", SHM_MIN_SIZE);
        assert!(matches!(writer2, Err(ShmError::AlreadyExists { .. })));
    }

    #[test]
    fn test_write_operations() {
        let mut writer = SegmentWriter::create("write_test", SHM_MIN_SIZE).unwrap();

        let data = b"Hello, World!";
        assert!(writer.write(data).is_ok());
        assert!(writer.current_version() > 0);

        // Test offset write
        assert!(writer.write_at(100, data).is_ok());

        // Test bounds checking
        let large_data = vec![0u8; 5000];
        assert!(matches!(
            writer.write(&large_data),
            Err(ShmError::InvalidSize { .. })
        ));
    }

    #[test]
    fn test_version_management() {
        let mut writer = SegmentWriter::create("version_test", SHM_MIN_SIZE).unwrap();

        let initial_version = writer.current_version();
        writer.write(b"test data").unwrap();

        // Version should have incremented (twice: begin_write + end_write)
        assert!(writer.current_version() > initial_version);

        // Version should be even (stable) - starts at 0, goes to 1 (begin), then 2 (end)
        assert_eq!(writer.current_version() % 2, 0);
        assert_eq!(writer.current_version(), 2);
    }
}
