//! Shared memory segment structures and operations

use crate::error::{ShmError, ShmResult};
use crate::version::VersionCounter;
use evo::shm::consts::{CACHE_LINE_SIZE, EVO_SHM_MAGIC, SHM_MAX_SIZE, SHM_MIN_SIZE};
use memmap2::MmapMut;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Segment header with cache-line alignment
#[repr(C, align(64))]
pub struct SegmentHeader {
    /// Magic number for validation
    pub magic: u64,
    /// Version counter for optimistic concurrency
    pub version: AtomicU64,
    /// Writer process ID
    pub writer_pid: AtomicU32,
    /// Active reader count
    pub reader_count: AtomicU32,
    /// Data section size
    pub size: u64,
    /// Creation timestamp (monotonic)
    pub created_ts: u64,
    /// Last write timestamp
    pub last_write_ts: AtomicU64,
    /// Header checksum
    pub checksum: AtomicU32,
    /// Cache line padding to ensure 128-byte header
    _padding: [u8; 64],
}

impl SegmentHeader {
    /// Create new segment header
    pub fn new(size: usize, writer_pid: u32) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        Self {
            magic: EVO_SHM_MAGIC,
            version: AtomicU64::new(0),
            writer_pid: AtomicU32::new(writer_pid),
            reader_count: AtomicU32::new(0),
            size: size as u64,
            created_ts: now,
            last_write_ts: AtomicU64::new(now),
            checksum: AtomicU32::new(0),
            _padding: [0; 64],
        }
    }

    /// Validate header magic and integrity
    pub fn validate(&self) -> ShmResult<()> {
        if self.magic != EVO_SHM_MAGIC {
            return Err(ShmError::NotFound {
                name: "invalid magic".to_string(),
            });
        }
        Ok(())
    }

    /// Get version counter wrapper
    pub fn version_counter(&self) -> VersionCounter {
        VersionCounter::from_raw(self.version.load(Ordering::Acquire))
    }

    /// Increment reader count
    pub fn add_reader(&self) -> u32 {
        self.reader_count.fetch_add(1, Ordering::AcqRel)
    }

    /// Decrement reader count
    pub fn remove_reader(&self) -> u32 {
        self.reader_count.fetch_sub(1, Ordering::AcqRel)
    }

    /// Get current reader count
    pub fn get_reader_count(&self) -> u32 {
        self.reader_count.load(Ordering::Acquire)
    }
}

/// Core shared memory segment representation
pub struct SharedMemorySegment {
    /// Segment name
    pub name: String,
    /// Total mapped size (including header)
    pub total_size: usize,
    /// Data section size
    pub data_size: usize,
    /// Memory mapping
    mmap: MmapMut,
}

impl SharedMemorySegment {
    /// Create new segment with validation
    pub fn new(name: String, data_size: usize, mmap: MmapMut) -> ShmResult<Self> {
        validate_segment_size(data_size)?;
        validate_memory_alignment(mmap.as_ptr() as usize)?;

        Ok(Self {
            name,
            total_size: data_size + std::mem::size_of::<SegmentHeader>(),
            data_size,
            mmap,
        })
    }

    /// Get header pointer
    pub fn header(&self) -> &SegmentHeader {
        unsafe { &*(self.mmap.as_ptr() as *const SegmentHeader) }
    }

    /// Get mutable header pointer (writer only)
    pub fn header_mut(&mut self) -> &mut SegmentHeader {
        unsafe { &mut *(self.mmap.as_mut_ptr() as *mut SegmentHeader) }
    }

    /// Get data section pointer
    pub fn data_ptr(&self) -> *const u8 {
        unsafe { self.mmap.as_ptr().add(std::mem::size_of::<SegmentHeader>()) }
    }

    /// Get mutable data section pointer (writer only)
    pub fn data_ptr_mut(&mut self) -> *mut u8 {
        unsafe {
            self.mmap
                .as_mut_ptr()
                .add(std::mem::size_of::<SegmentHeader>())
        }
    }

    /// Get data section as slice
    pub fn data(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.data_ptr(), self.data_size) }
    }

    /// Get mutable data section as slice (writer only)
    pub fn data_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.data_ptr_mut(), self.data_size) }
    }
}

/// Validate segment size constraints
pub fn validate_segment_size(size: usize) -> ShmResult<()> {
    if size < SHM_MIN_SIZE {
        return Err(ShmError::InvalidSize { size });
    }

    if size > SHM_MAX_SIZE {
        return Err(ShmError::InvalidSize { size });
    }

    // Must be page-aligned (4KB on most systems)
    if size % SHM_MIN_SIZE != 0 {
        return Err(ShmError::InvalidSize { size });
    }

    Ok(())
}

/// Validate memory alignment
pub fn validate_memory_alignment(address: usize) -> ShmResult<()> {
    if address % CACHE_LINE_SIZE != 0 {
        return Err(ShmError::AlignmentError {
            address,
            alignment: CACHE_LINE_SIZE,
        });
    }
    Ok(())
}

/// Memory prefetch strategies for hot paths
pub mod prefetch {
    use evo::shm::consts::CACHE_LINE_SIZE;
    use std::arch::x86_64::_MM_HINT_T0;
    use std::arch::x86_64::_mm_prefetch;

    /// Prefetch locality hints
    pub enum PrefetchHint {
        /// Temporal locality (T0) - data will be used again soon
        Temporal,
        /// Non-temporal (NTA) - data will be used once
        NonTemporal,
        /// T1 cache hint - moderate locality
        Moderate,
        /// T2 cache hint - low locality
        Low,
    }

    /// Prefetch memory region for reading
    #[cfg(target_arch = "x86_64")]
    #[allow(dead_code)]
    pub fn prefetch_read(addr: *const u8, size: usize, hint: PrefetchHint) {
        unsafe {
            let mut ptr = addr;
            let end = addr.add(size);

            while ptr < end {
                match hint {
                    PrefetchHint::Temporal => {
                        _mm_prefetch(ptr as *const i8, _MM_HINT_T0);
                    }
                    PrefetchHint::NonTemporal => {
                        _mm_prefetch(ptr as *const i8, 0); // _MM_HINT_NTA
                    }
                    PrefetchHint::Moderate => {
                        _mm_prefetch(ptr as *const i8, 1); // _MM_HINT_T1
                    }
                    PrefetchHint::Low => {
                        _mm_prefetch(ptr as *const i8, 2); // _MM_HINT_T2
                    }
                }
                ptr = ptr.add(CACHE_LINE_SIZE);
            }
        }
    }

    /// Prefetch for non-x86_64 architectures (no-op)
    #[cfg(not(target_arch = "x86_64"))]
    #[allow(dead_code)]
    pub fn prefetch_read(_addr: *const u8, _size: usize, _hint: PrefetchHint) {
        // No-op for non-x86_64 architectures
    }

    /// Prefetch segment header for immediate access
    #[allow(dead_code)]
    pub fn prefetch_header(header: *const super::SegmentHeader) {
        prefetch_read(
            header as *const u8,
            std::mem::size_of::<super::SegmentHeader>(),
            PrefetchHint::Temporal,
        );
    }

    /// Prefetch data region with streaming pattern
    #[allow(dead_code)]
    pub fn prefetch_data_streaming(data_ptr: *const u8, size: usize) {
        prefetch_read(data_ptr, size, PrefetchHint::NonTemporal);
    }

    /// Prefetch data region for repeated access
    #[allow(dead_code)]
    pub fn prefetch_data_cached(data_ptr: *const u8, size: usize) {
        prefetch_read(data_ptr, size, PrefetchHint::Temporal);
    }
}

/// Cache-friendly data structure layout optimizations
pub mod cache {
    use evo::shm::consts::CACHE_LINE_SIZE;

    /// Align pointer to cache line boundary
    #[allow(dead_code)]
    pub fn align_to_cache_line(ptr: *const u8) -> *const u8 {
        let addr = ptr as usize;
        let aligned_addr = (addr + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1);
        aligned_addr as *const u8
    }

    /// Calculate cache line aligned size
    #[allow(dead_code)]
    pub fn cache_aligned_size(size: usize) -> usize {
        (size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1)
    }

    /// Check if pointer is cache line aligned
    #[allow(dead_code)]
    pub fn is_cache_aligned(ptr: *const u8) -> bool {
        (ptr as usize) % CACHE_LINE_SIZE == 0
    }

    /// Memory layout optimization for data structures
    #[derive(Debug, Clone, Copy)]
    pub struct LayoutOptimizer {
        /// Current offset in layout
        pub offset: usize,
        /// Total size with padding
        pub total_size: usize,
    }

    impl LayoutOptimizer {
        /// Create new layout optimizer
        #[allow(dead_code)]
        pub fn new() -> Self {
            Self {
                offset: 0,
                total_size: 0,
            }
        }

        /// Add field with automatic alignment
        #[allow(dead_code)]
        pub fn add_field(&mut self, size: usize, align: usize) -> usize {
            // Align offset to field alignment
            self.offset = (self.offset + align - 1) & !(align - 1);
            let field_offset = self.offset;
            self.offset += size;
            self.total_size = self.offset;
            field_offset
        }

        /// Finalize layout with cache line alignment
        #[allow(dead_code)]
        pub fn finalize(&mut self) -> usize {
            self.total_size = cache_aligned_size(self.total_size);
            self.total_size
        }
    }

    impl Default for LayoutOptimizer {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_size_validation() {
        // Valid sizes
        assert!(validate_segment_size(SHM_MIN_SIZE).is_ok());
        assert!(validate_segment_size(8192).is_ok());
        assert!(validate_segment_size(1024 * 1024).is_ok());

        // Invalid sizes
        assert!(validate_segment_size(1024).is_err()); // Too small
        assert!(validate_segment_size(4097).is_err()); // Not page-aligned
        assert!(validate_segment_size(2 * 1024 * 1024 * 1024).is_err()); // Too large
    }

    #[test]
    fn test_header_creation() {
        let header = SegmentHeader::new(SHM_MIN_SIZE, 12345);
        assert_eq!(header.magic, EVO_SHM_MAGIC);
        assert_eq!(header.size, SHM_MIN_SIZE as u64);
        assert_eq!(header.writer_pid.load(Ordering::Relaxed), 12345);
        assert_eq!(header.reader_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_header_validation() {
        let header = SegmentHeader::new(SHM_MIN_SIZE, 12345);
        assert!(header.validate().is_ok());

        // Test invalid magic
        let mut invalid_header = header;
        invalid_header.magic = 0;
        assert!(invalid_header.validate().is_err());
    }
}
