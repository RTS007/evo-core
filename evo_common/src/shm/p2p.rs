//! P2P Shared Memory segment header, module identifiers, typed writer/reader.
//!
//! Defines the `P2pSegmentHeader` struct (64 bytes, cache-line aligned),
//! the `ModuleAbbrev` enum, `ShmError`, and the lock-free `TypedP2pWriter<T>`
//! / `TypedP2pReader<T>` for inter-process communication.
//!
//! ## Lock-Free Protocol
//!
//! The writer uses an odd/even `write_seq` protocol:
//! - Odd = write in progress (reader must retry)
//! - Even = committed (safe to read)
//!
//! There is **no flock** on data segments. The write_seq protocol provides
//! consistent reads without any kernel-level locking. Duplicate-writer
//! prevention is handled by an exclusive flock on a separate `.lock` file
//! under `/dev/shm/`.
//!
//! ## Segment Naming
//!
//! All segments are created under `/dev/shm/` with the name `evo_<name>`.
//! The name follows the convention `<source>_<dest>`, e.g. `hal_cu`.

use std::marker::PhantomData;
use std::os::unix::io::OwnedFd;
use std::ptr::NonNull;

use nix::fcntl::{Flock, FlockArg, OFlag};
use nix::sys::mman::{self, MapFlags, MmapAdvise, ProtFlags};
use nix::sys::stat::Mode;
use nix::unistd;
use static_assertions::const_assert_eq;
use thiserror::Error;

// ─── Constants ──────────────────────────────────────────────────────

/// Magic bytes identifying a valid P2P segment: `"EVO_P2P\0"`.
pub const EVO_P2P_MAGIC: [u8; 8] = *b"EVO_P2P\0";

/// Page size for segment data allocation.
pub const PAGE_SIZE: usize = 4096;

/// SHM name prefix for all EVO segments.
const SHM_PREFIX: &str = "/evo_";

// ─── P2P Header Field Offsets (repr(C) layout) ─────────────────────
//
// P2pSegmentHeader layout (64 bytes, align 64):
//   [0..8]   magic:         [u8; 8]
//   [8..12]  version_hash:  u32
//   [12..16] _pad:          (implicit padding for u64 align)
//   [16..24] heartbeat:     u64
//   [24]     source_module: u8
//   [25]     dest_module:   u8
//   [26..28] _pad:          (implicit padding for u32 align)
//   [28..32] payload_size:  u32
//   [32..36] write_seq:     u32
//   [36..64] _padding:      [u8; 28]

const HEARTBEAT_OFFSET: usize = 16;
const WRITE_SEQ_OFFSET: usize = 32;

// ─── Error Type ─────────────────────────────────────────────────────

/// Errors that can occur during P2P SHM operations.
#[derive(Debug, Error)]
pub enum ShmError {
    /// Invalid P2P magic bytes in segment header.
    #[error("invalid P2P magic on '{segment}'")]
    InvalidMagic {
        /// Segment name.
        segment: String,
    },

    /// P2P version hash mismatch (struct layout incompatibility).
    #[error("version hash mismatch on '{segment}': expected 0x{expected:08X}, got 0x{actual:08X}")]
    VersionMismatch {
        /// Segment name.
        segment: String,
        /// Expected hash (compiled-in).
        expected: u32,
        /// Actual hash read from SHM.
        actual: u32,
    },

    /// Destination module in header doesn't match expected reader module.
    #[error("destination mismatch on '{segment}': expected {expected:?}, got {actual:?}")]
    DestinationMismatch {
        /// Segment name.
        segment: String,
        /// Expected destination module.
        expected: ModuleAbbrev,
        /// Actual destination module read from header.
        actual: ModuleAbbrev,
    },

    /// Another writer already holds an exclusive lock on this segment.
    #[error("writer already exists for segment '{segment}'")]
    WriterAlreadyExists {
        /// Segment name.
        segment: String,
    },

    /// Too many read retries due to write contention.
    #[error("read contention on '{segment}': writer updating too frequently")]
    ReadContention {
        /// Segment name.
        segment: String,
    },

    /// Segment does not exist in `/dev/shm/`.
    #[error("segment not found: '{segment}'")]
    SegmentNotFound {
        /// Segment name.
        segment: String,
    },

    /// Permission denied when opening SHM segment.
    #[error("permission denied for segment '{segment}': {reason}")]
    PermissionDenied {
        /// Segment name.
        segment: String,
        /// Additional context.
        reason: String,
    },

    /// Heartbeat staleness detected (writer stopped updating).
    #[error("heartbeat stale on '{segment}': {missed_beats} consecutive misses")]
    HeartbeatStale {
        /// Segment name.
        segment: String,
        /// Number of consecutive reads without heartbeat change.
        missed_beats: u32,
    },

    /// Segment data too small for the expected payload type.
    #[error("payload too small on '{segment}': need {expected} bytes, got {actual}")]
    PayloadTooSmall {
        /// Segment name.
        segment: String,
        /// Expected minimum size in bytes.
        expected: usize,
        /// Actual data size in bytes.
        actual: usize,
    },

    /// OS-level error from nix/libc calls.
    #[error("OS error on '{segment}': {source}")]
    Os {
        /// Segment name.
        segment: String,
        /// Underlying errno.
        source: nix::errno::Errno,
    },
}

// ─── Module Abbreviation ────────────────────────────────────────────

/// Module abbreviation identifying source/destination of a P2P segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModuleAbbrev {
    /// Control Unit
    Cu = 0,
    /// Hardware Abstraction Layer
    Hal = 1,
    /// Recipe Executor
    Re = 2,
    /// MQTT Bridge
    Mqt = 3,
    /// gRPC API / RPC Bridge
    Rpc = 4,
}

impl ModuleAbbrev {
    /// Convert from raw `u8` value. Returns `None` for invalid values.
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Cu),
            1 => Some(Self::Hal),
            2 => Some(Self::Re),
            3 => Some(Self::Mqt),
            4 => Some(Self::Rpc),
            _ => None,
        }
    }
}

// ─── P2P Segment Header ────────────────────────────────────────────

/// P2P Segment Header — 64 bytes, cache-line aligned.
///
/// Every P2P shared memory segment starts with this header. The writer
/// populates it on every write cycle. The reader validates `magic`,
/// `version_hash`, and monitors `heartbeat` for staleness.
///
/// ## Lock-Free Protocol
///
/// `write_seq` uses odd/even protocol:
/// - Odd = write in progress (reader must retry)
/// - Even = committed (reader can safely read payload)
///
/// `write_seq` must be accessed atomically (`AtomicU32`) in runtime code.
/// The struct uses `u32` for FFI/serialization compatibility.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct P2pSegmentHeader {
    /// Magic bytes: must be `EVO_P2P_MAGIC` (`"EVO_P2P\0"`).
    pub magic: [u8; 8],

    /// Compile-time hash of the payload struct layout.
    /// Computed via `struct_version_hash::<T>()`.
    /// Reader refuses to connect if mismatch.
    pub version_hash: u32,

    /// Monotonically increasing cycle counter.
    /// Writer increments by 1 on every write.
    /// Reader triggers staleness if unchanged for N consecutive reads.
    pub heartbeat: u64,

    /// Source module identifier.
    pub source_module: u8,

    /// Destination module identifier.
    pub dest_module: u8,

    /// Size of payload bytes following this header.
    pub payload_size: u32,

    /// Lock-free write sequence number.
    /// Odd = write in progress, even = committed.
    /// Must be accessed as `AtomicU32` at runtime.
    pub write_seq: u32,

    /// Padding to fill 64 bytes total.
    pub _padding: [u8; 28],
}

const_assert_eq!(core::mem::size_of::<P2pSegmentHeader>(), 64);
const_assert_eq!(core::mem::align_of::<P2pSegmentHeader>(), 64);

impl P2pSegmentHeader {
    /// Create a new header with default values.
    pub const fn new(
        source: ModuleAbbrev,
        dest: ModuleAbbrev,
        version_hash: u32,
        payload_size: u32,
    ) -> Self {
        Self {
            magic: EVO_P2P_MAGIC,
            version_hash,
            heartbeat: 0,
            source_module: source as u8,
            dest_module: dest as u8,
            payload_size,
            write_seq: 0,
            _padding: [0u8; 28],
        }
    }

    /// Validate the magic bytes.
    #[inline]
    pub const fn is_magic_valid(&self) -> bool {
        let m = &self.magic;
        m[0] == b'E'
            && m[1] == b'V'
            && m[2] == b'O'
            && m[3] == b'_'
            && m[4] == b'P'
            && m[5] == b'2'
            && m[6] == b'P'
            && m[7] == 0
    }
}

/// Compile-time version hash for struct compatibility detection.
///
/// Computes a hash from `size_of::<T>()` and `align_of::<T>()`.
/// If the struct layout changes, the hash changes, and reader/writer
/// refuse to connect.
///
/// **Known limitation**: Does not detect field reordering within the
/// same total size/alignment. This is acceptable because `#[repr(C)]`
/// structs with explicit padding have deterministic field order.
pub const fn struct_version_hash<T>() -> u32 {
    let size = core::mem::size_of::<T>() as u32;
    let align = core::mem::align_of::<T>() as u32;
    size.wrapping_mul(0x9E3779B9) ^ align.wrapping_mul(0x517CC1B7)
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Compute the minimum page-aligned data size for a segment type.
///
/// Returns `size` rounded up to the nearest multiple of `PAGE_SIZE` (4096).
pub const fn data_size_for<T>() -> usize {
    let raw = core::mem::size_of::<T>();
    let pages = (raw + PAGE_SIZE - 1) / PAGE_SIZE;
    pages * PAGE_SIZE
}

/// Compute the page-aligned mmap size for a P2P segment: header (64 B) + payload T.
///
/// The SHM region layout is `[P2pSegmentHeader (64 B)][T payload]`, rounded
/// up to the nearest page boundary.
const fn segment_mmap_size<T>() -> usize {
    let raw = core::mem::size_of::<P2pSegmentHeader>() + core::mem::size_of::<T>();
    let pages = (raw + PAGE_SIZE - 1) / PAGE_SIZE;
    pages * PAGE_SIZE
}

/// Build the POSIX SHM path for a segment name (e.g., `"hal_cu"` → `"/evo_hal_cu"`).
fn shm_path(name: &str) -> String {
    format!("{SHM_PREFIX}{name}")
}

/// Build the lock-file path for writer-exclusivity enforcement.
fn lock_path(name: &str) -> String {
    format!("{SHM_PREFIX}{name}.lock")
}

// ─── TypedP2pWriter ─────────────────────────────────────────────────

/// Typed outbound segment writer with P2P heartbeat management.
///
/// Creates a POSIX shared memory segment, acquires an exclusive lock on a
/// separate `.lock` file (so readers are not blocked), and provides
/// zero-allocation writes with automatic heartbeat increment.
///
/// # SHM Layout
///
/// The mapped region is `[P2pSegmentHeader (64 B)][T payload (size_of::<T>() B)]`,
/// rounded up to the nearest page boundary. `T` is the **payload-only** type
/// — it does NOT need to embed `P2pSegmentHeader` as its first field.
///
/// # Safety Requirements
///
/// `T` must be `#[repr(C)]` with all-zeroes being a valid bit pattern.
///
/// # Lifecycle
///
/// - **Create**: `shm_open(O_CREAT | O_RDWR)` + `ftruncate` + `mmap`
///   + `flock(LOCK_EX)` on separate `.lock` shm segment
/// - **Write**: Copy payload to pre-allocated buffer, apply header, increment heartbeat
/// - **Drop**: `munmap` + `shm_unlink` + lock file auto-released
pub struct TypedP2pWriter<T: Copy> {
    /// Exclusive flock on the `.lock` SHM segment — prevents duplicate writers.
    /// Held for the lifetime of the writer; readers don't touch this file.
    _lock: Flock<OwnedFd>,
    /// POSIX SHM file descriptor for the data segment.
    /// Kept alive for cleanup (shm_unlink on drop). Not flock'd.
    _data_fd: OwnedFd,
    /// Memory-mapped pointer to the data segment.
    map_ptr: NonNull<libc::c_void>,
    /// Total mapped size (page-aligned).
    map_len: usize,
    /// Segment name (without `/evo_` prefix).
    name: String,
    /// Pre-allocated byte buffer (page-aligned size). Reused every cycle.
    write_buf: Vec<u8>,
    /// Cached P2P header template (magic, version_hash, source, dest, payload_size).
    header_template: [u8; 64],
    /// Monotonic heartbeat counter, incremented on every `commit()`.
    heartbeat: u64,
    _marker: PhantomData<T>,
}

// SAFETY: The mmap pointer is only accessed by the single owning writer.
// The segment is protected by the lock-free write_seq protocol plus the
// exclusive .lock file that prevents duplicate writers.
unsafe impl<T: Copy> Send for TypedP2pWriter<T> {}

impl<T: Copy> TypedP2pWriter<T> {
    /// Create a new P2P shared memory segment and acquire exclusive writer lock.
    ///
    /// # Arguments
    /// - `name`: Segment name (e.g., `"hal_cu"`). Will be prefixed with `evo_`.
    /// - `source`: Source module identifier.
    /// - `dest`: Destination module identifier.
    ///
    /// # Errors
    /// - `ShmError::WriterAlreadyExists` if another writer holds the segment.
    /// - `ShmError::Os` for system-level errors.
    pub fn create(
        name: &str,
        source: ModuleAbbrev,
        dest: ModuleAbbrev,
    ) -> Result<Self, ShmError> {
        let data_size = segment_mmap_size::<T>();

        // === Writer-exclusivity: flock on a separate .lock segment ===
        let lock_name = lock_path(name);
        let lock_fd = mman::shm_open(
            lock_name.as_str(),
            OFlag::O_CREAT | OFlag::O_RDWR,
            Mode::S_IRUSR | Mode::S_IWUSR,
        )
        .map_err(|e| ShmError::Os {
            segment: name.to_string(),
            source: e,
        })?;

        let lock = Flock::lock(lock_fd, FlockArg::LockExclusiveNonblock).map_err(
            |(_, errno)| {
                if errno == nix::errno::Errno::EWOULDBLOCK {
                    ShmError::WriterAlreadyExists {
                        segment: name.to_string(),
                    }
                } else {
                    ShmError::Os {
                        segment: name.to_string(),
                        source: errno,
                    }
                }
            },
        )?;

        // === Open/create the data segment (no flock) ===
        let shm_name = shm_path(name);
        let data_fd = mman::shm_open(
            shm_name.as_str(),
            OFlag::O_CREAT | OFlag::O_RDWR,
            Mode::S_IRUSR | Mode::S_IWUSR, // 0o600
        )
        .map_err(|e| ShmError::Os {
            segment: name.to_string(),
            source: e,
        })?;

        // Set the segment size.
        unistd::ftruncate(&data_fd, data_size as libc::off_t).map_err(|e| ShmError::Os {
            segment: name.to_string(),
            source: e,
        })?;

        // Memory-map the segment.
        let map_ptr = unsafe {
            mman::mmap(
                None,
                std::num::NonZeroUsize::new(data_size).unwrap(),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                &data_fd,
                0,
            )
            .map_err(|e| ShmError::Os {
                segment: name.to_string(),
                source: e,
            })?
        };

        // Advise sequential access for RT performance.
        let _ = unsafe { mman::madvise(map_ptr, data_size, MmapAdvise::MADV_SEQUENTIAL) };

        // Build initial P2P header.
        let payload_bytes = core::mem::size_of::<T>() as u32;
        let header = P2pSegmentHeader::new(source, dest, struct_version_hash::<T>(), payload_bytes);

        // Serialize header into the write buffer.
        let mut write_buf = vec![0u8; data_size];
        let hdr_size = core::mem::size_of::<P2pSegmentHeader>();
        let hdr_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                &header as *const P2pSegmentHeader as *const u8,
                hdr_size,
            )
        };
        write_buf[..hdr_size].copy_from_slice(hdr_bytes);

        // Cache the header template for re-application on every commit().
        let mut header_template = [0u8; 64];
        header_template.copy_from_slice(hdr_bytes);

        // Write initial data to mapped memory so readers see a valid header immediately.
        unsafe {
            core::ptr::copy_nonoverlapping(
                write_buf.as_ptr(),
                map_ptr.as_ptr() as *mut u8,
                data_size,
            );
        }

        // Full memory barrier to ensure initial write is visible.
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);

        Ok(Self {
            _lock: lock,
            _data_fd: data_fd,
            map_ptr,
            map_len: data_size,
            name: name.to_string(),
            write_buf,
            header_template,
            heartbeat: 0,
            _marker: PhantomData,
        })
    }

    /// Write a complete segment payload to shared memory.
    ///
    /// This method:
    /// 1. Sets `write_seq` to odd (write in progress) in mapped memory.
    /// 2. Copies the payload `T` into the pre-allocated buffer.
    /// 3. Re-applies the cached P2P header template.
    /// 4. Increments the heartbeat counter.
    /// 5. Copies buffer to mapped memory with committed `write_seq`.
    /// 6. Issues a release fence.
    ///
    /// # RT Safety
    /// No heap allocation occurs in this method. The write buffer is
    /// pre-allocated at `create()` time.
    pub fn commit(&mut self, payload: &T) -> Result<(), ShmError> {
        let type_size = core::mem::size_of::<T>();
        let hdr_size = core::mem::size_of::<P2pSegmentHeader>();
        let map = self.map_ptr.as_ptr() as *mut u8;

        // Increment heartbeat.
        self.heartbeat += 1;

        // Calculate write_seq values.
        let seq_odd = self.heartbeat.wrapping_mul(2).wrapping_sub(1) as u32;
        let seq_even = self.heartbeat.wrapping_mul(2) as u32;

        // === STEP 1: Signal write-in-progress (odd write_seq) in mapped memory ===
        unsafe {
            let ws_ptr = map.add(WRITE_SEQ_OFFSET) as *mut u32;
            core::ptr::write_volatile(ws_ptr, seq_odd);
        }
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);

        // === STEP 2: Build payload in pre-allocated buffer ===

        // Copy payload bytes to pre-allocated buffer at offset after header.
        let src: &[u8] =
            unsafe { core::slice::from_raw_parts(payload as *const T as *const u8, type_size) };
        self.write_buf[hdr_size..hdr_size + type_size].copy_from_slice(src);

        // Re-apply cached P2P header template (magic, version_hash, source,
        // dest, payload_size). Ensures correctness even if the caller passes
        // a zeroed or partially-filled struct.
        self.write_buf[..hdr_size].copy_from_slice(&self.header_template);

        // Write heartbeat into buffer.
        self.write_buf[HEARTBEAT_OFFSET..HEARTBEAT_OFFSET + 8]
            .copy_from_slice(&self.heartbeat.to_ne_bytes());

        // Write committed write_seq into buffer.
        self.write_buf[WRITE_SEQ_OFFSET..WRITE_SEQ_OFFSET + 4]
            .copy_from_slice(&seq_even.to_ne_bytes());

        // === STEP 3: Copy buffer to mapped memory ===
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.write_buf.as_ptr(),
                map,
                self.map_len.min(self.write_buf.len()),
            );
        }

        // === STEP 4: Final barrier — committed write_seq is now visible ===
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);

        Ok(())
    }

    /// Get the current heartbeat counter value.
    #[inline]
    pub fn heartbeat(&self) -> u64 {
        self.heartbeat
    }

    /// Get the segment name (without prefix).
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the mapped data size.
    #[inline]
    pub fn data_size(&self) -> usize {
        self.map_len
    }
}

impl<T: Copy> Drop for TypedP2pWriter<T> {
    fn drop(&mut self) {
        // Unmap the data segment.
        unsafe {
            let _ = mman::munmap(self.map_ptr, self.map_len);
        }
        // Unlink the SHM data segment (removes from /dev/shm).
        let shm_name = shm_path(&self.name);
        let _ = mman::shm_unlink(shm_name.as_str());
        // Unlink the lock file.
        let lk = lock_path(&self.name);
        let _ = mman::shm_unlink(lk.as_str());
        // _lock (Flock<OwnedFd>) and _data_fd (OwnedFd) are dropped automatically,
        // releasing the flock and closing file descriptors.
    }
}

// ─── TypedP2pReader ─────────────────────────────────────────────────

/// Typed inbound segment reader with heartbeat staleness detection.
///
/// Attaches to an existing POSIX shared memory segment (without any lock on
/// the data segment) and provides zero-copy typed access with one-time P2P
/// header validation and per-read heartbeat monitoring.
///
/// # SHM Layout
///
/// The mapped region is `[P2pSegmentHeader (64 B)][T payload]`. `T` is the
/// **payload-only** type — it does NOT need to embed `P2pSegmentHeader`.
///
/// # Safety Requirements
///
/// `T` must be `#[repr(C)]` with all-zeroes being a valid bit pattern.
///
/// # Lifecycle
///
/// - **Attach**: `shm_open(O_RDONLY)` + `mmap(PROT_READ)`
/// - **Read**: Copy from mmap to aligned buffer, validate header, check heartbeat
/// - **Drop**: `munmap` (no shm_unlink — writer owns segment lifetime)
pub struct TypedP2pReader<T: Copy> {
    /// POSIX SHM file descriptor for the data segment (read-only, no flock).
    _data_fd: OwnedFd,
    /// Memory-mapped pointer to the data segment (PROT_READ).
    map_ptr: NonNull<libc::c_void>,
    /// Total mapped size.
    map_len: usize,
    /// Segment name (without `/evo_` prefix).
    name: String,
    /// Pre-allocated aligned buffer for payload deserialization.
    payload: T,
    /// Pre-allocated buffer for reading the P2P header separately from payload.
    header_buf: P2pSegmentHeader,
    /// Whether the one-time P2P header validation has been done.
    verified: bool,
    /// Expected version hash (compiled-in for type T).
    expected_hash: u32,
    /// Last observed heartbeat value.
    last_heartbeat: u64,
    /// Consecutive reads without heartbeat change.
    stale_count: u32,
    /// Staleness threshold (number of unchanged reads before error).
    stale_threshold: u32,
    _marker: PhantomData<T>,
}

// SAFETY: The mmap pointer is read-only. The lock-free write_seq protocol
// ensures consistent reads without any kernel-level locking.
unsafe impl<T: Copy> Send for TypedP2pReader<T> {}

impl<T: Copy> TypedP2pReader<T> {
    /// Attach to an existing P2P segment.
    ///
    /// # Arguments
    /// - `name`: Segment name (e.g., `"hal_cu"`). Will be prefixed with `evo_`.
    /// - `stale_threshold`: Max consecutive reads without heartbeat change.
    ///
    /// # Errors
    /// - `ShmError::SegmentNotFound` if the segment does not exist.
    /// - `ShmError::PayloadTooSmall` if the segment is smaller than `T`.
    /// - `ShmError::Os` for system-level errors.
    pub fn attach(name: &str, stale_threshold: u32) -> Result<Self, ShmError> {
        let shm_name = shm_path(name);

        // Open existing SHM segment (read-only — no flock needed).
        let data_fd = mman::shm_open(shm_name.as_str(), OFlag::O_RDONLY, Mode::empty()).map_err(
            |e| {
                if e == nix::errno::Errno::ENOENT {
                    ShmError::SegmentNotFound {
                        segment: name.to_string(),
                    }
                } else if e == nix::errno::Errno::EACCES {
                    ShmError::PermissionDenied {
                        segment: name.to_string(),
                        reason: "insufficient permissions to open SHM segment".to_string(),
                    }
                } else {
                    ShmError::Os {
                        segment: name.to_string(),
                        source: e,
                    }
                }
            },
        )?;

        // Get segment size from fd.
        let stat = nix::sys::stat::fstat(&data_fd).map_err(|e| ShmError::Os {
            segment: name.to_string(),
            source: e,
        })?;
        let file_size = stat.st_size as usize;

        // Validate size — segment must hold at least header + payload.
        let type_size = core::mem::size_of::<T>();
        let hdr_size = core::mem::size_of::<P2pSegmentHeader>();
        let min_size = hdr_size + type_size;
        if file_size < min_size {
            return Err(ShmError::PayloadTooSmall {
                segment: name.to_string(),
                expected: min_size,
                actual: file_size,
            });
        }

        // Compute map length (at least one page).
        let map_len = segment_mmap_size::<T>().max(file_size);

        // Memory-map the segment (read-only).
        let map_ptr = unsafe {
            mman::mmap(
                None,
                std::num::NonZeroUsize::new(map_len).unwrap(),
                ProtFlags::PROT_READ,
                MapFlags::MAP_SHARED,
                &data_fd,
                0,
            )
            .map_err(|e| ShmError::Os {
                segment: name.to_string(),
                source: e,
            })?
        };

        // Zero-initialize the payload buffer.
        // SAFETY: All P2P segment types are repr(C) with only numeric fields;
        // all-zeros is a valid bit pattern for every field.
        let payload: T = unsafe { core::mem::zeroed() };
        let header_buf: P2pSegmentHeader = unsafe { core::mem::zeroed() };

        Ok(Self {
            _data_fd: data_fd,
            map_ptr,
            map_len,
            name: name.to_string(),
            payload,
            header_buf,
            verified: false,
            expected_hash: struct_version_hash::<T>(),
            last_heartbeat: 0,
            stale_count: 0,
            stale_threshold,
            _marker: PhantomData,
        })
    }

    /// Attach to an existing P2P segment with destination module validation.
    ///
    /// Same as `attach()`, but additionally validates that the segment's
    /// destination module matches `expected_dest`.
    pub fn attach_validated(
        name: &str,
        stale_threshold: u32,
        expected_dest: ModuleAbbrev,
    ) -> Result<Self, ShmError> {
        let reader = Self::attach(name, stale_threshold)?;

        // Read the dest_module byte directly from mapped memory.
        // P2pSegmentHeader layout: dest_module is at byte offset 25.
        let map = reader.map_ptr.as_ptr() as *const u8;
        let dest_byte = unsafe { core::ptr::read(map.add(25)) };

        if let Some(actual_dest) = ModuleAbbrev::from_u8(dest_byte) {
            if actual_dest != expected_dest {
                return Err(ShmError::DestinationMismatch {
                    segment: name.to_string(),
                    expected: expected_dest,
                    actual: actual_dest,
                });
            }
        }

        Ok(reader)
    }

    /// Read the current segment payload.
    ///
    /// On the first successful read, validates P2P magic and version hash.
    /// On every read, checks the heartbeat for staleness.
    ///
    /// Returns a reference to the internal aligned buffer containing the
    /// latest payload. The reference is valid until the next call to `read()`.
    ///
    /// # Errors
    /// - `ShmError::InvalidMagic` on first read if magic is wrong.
    /// - `ShmError::VersionMismatch` on first read if hash differs.
    /// - `ShmError::HeartbeatStale` if heartbeat unchanged for `stale_threshold` reads.
    /// - `ShmError::ReadContention` if too many read retries.
    pub fn read(&mut self) -> Result<&T, ShmError> {
        let type_size = core::mem::size_of::<T>();
        let hdr_size = core::mem::size_of::<P2pSegmentHeader>();
        let max_retries = 10u32;
        let map = self.map_ptr.as_ptr() as *const u8;

        for _attempt in 0..max_retries {
            // Read write_seq before data.
            let seq_before = unsafe {
                let ws_ptr = map.add(WRITE_SEQ_OFFSET) as *const u32;
                core::ptr::read_volatile(ws_ptr)
            };

            // Skip if write is in progress (odd write_seq).
            if seq_before & 1 != 0 {
                std::thread::yield_now();
                continue;
            }

            // Acquire barrier before reading data.
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);

            // Copy header from mmap[0..hdr_size] to header_buf.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    map,
                    &mut self.header_buf as *mut P2pSegmentHeader as *mut u8,
                    hdr_size,
                );
            }

            // Copy payload from mmap[hdr_size..hdr_size+type_size] to payload buf.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    map.add(hdr_size),
                    &mut self.payload as *mut T as *mut u8,
                    type_size,
                );
            }

            // Acquire barrier after reading data.
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);

            // Read write_seq after data.
            let seq_after = unsafe {
                let ws_ptr = map.add(WRITE_SEQ_OFFSET) as *const u32;
                core::ptr::read_volatile(ws_ptr)
            };

            // If write_seq changed during read, retry.
            if seq_before != seq_after {
                std::thread::yield_now();
                continue;
            }

            // === Successful consistent read ===

            // One-time P2P header validation.
            if !self.verified {
                let header = &self.header_buf;

                if !header.is_magic_valid() {
                    return Err(ShmError::InvalidMagic {
                        segment: self.name.clone(),
                    });
                }
                if header.version_hash != self.expected_hash {
                    return Err(ShmError::VersionMismatch {
                        segment: self.name.clone(),
                        expected: self.expected_hash,
                        actual: header.version_hash,
                    });
                }
                self.verified = true;
            }

            // Heartbeat staleness check.
            let heartbeat = self.header_buf.heartbeat;

            if heartbeat == self.last_heartbeat && self.last_heartbeat != 0 {
                self.stale_count += 1;
                if self.stale_count >= self.stale_threshold {
                    return Err(ShmError::HeartbeatStale {
                        segment: self.name.clone(),
                        missed_beats: self.stale_count,
                    });
                }
            } else {
                self.last_heartbeat = heartbeat;
                self.stale_count = 0;
            }

            return Ok(&self.payload);
        }

        // Too many retries.
        Err(ShmError::ReadContention {
            segment: self.name.clone(),
        })
    }

    /// Check if the segment has new data since the last read.
    ///
    /// This is a cheap check using the P2P heartbeat field (no data copy).
    pub fn has_changed(&self) -> bool {
        let map = self.map_ptr.as_ptr() as *const u8;
        let heartbeat = unsafe {
            let hb_ptr = map.add(HEARTBEAT_OFFSET) as *const u64;
            core::ptr::read_volatile(hb_ptr)
        };
        heartbeat != self.last_heartbeat
    }

    /// Get the segment name (without prefix).
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the current stale count (consecutive reads without heartbeat change).
    #[inline]
    pub fn stale_count(&self) -> u32 {
        self.stale_count
    }

    /// Get the last observed heartbeat value.
    #[inline]
    pub fn last_heartbeat(&self) -> u64 {
        self.last_heartbeat
    }

    /// Reset staleness counter (e.g., after a recovery action).
    pub fn reset_stale(&mut self) {
        self.stale_count = 0;
    }
}

impl<T: Copy> Drop for TypedP2pReader<T> {
    fn drop(&mut self) {
        // Unmap the data segment.
        unsafe {
            let _ = mman::munmap(self.map_ptr, self.map_len);
        }
        // _data_fd (OwnedFd) is dropped automatically, closing the fd.
        // Note: Reader does NOT shm_unlink — only the writer owns the segment lifetime.
    }
}

// ─── Segment Discovery ─────────────────────────────────────────────

/// Information about a discovered SHM segment.
#[derive(Debug, Clone)]
pub struct SegmentInfo {
    /// Segment name (without `/evo_` prefix), e.g. `"hal_cu"`.
    pub name: String,
    /// Full filesystem path, e.g. `"/dev/shm/evo_hal_cu"`.
    pub path: std::path::PathBuf,
    /// Segment file size in bytes.
    pub size: u64,
    /// Whether the segment has a valid P2P magic header.
    pub valid_magic: bool,
    /// Whether a writer currently holds the exclusive `.lock` file.
    pub writer_alive: bool,
    /// Source module (from header), if magic is valid.
    pub source: Option<ModuleAbbrev>,
    /// Destination module (from header), if magic is valid.
    pub dest: Option<ModuleAbbrev>,
    /// Current heartbeat value (from header), if magic is valid.
    pub heartbeat: Option<u64>,
}

/// Enumerate and inspect live EVO P2P segments under `/dev/shm/`.
///
/// ## Usage
///
/// ```no_run
/// use evo_common::shm::p2p::SegmentDiscovery;
///
/// let all = SegmentDiscovery::list_segments();
/// for seg in &all {
///     println!("{}: alive={}, hb={:?}", seg.name, seg.writer_alive, seg.heartbeat);
/// }
/// ```
pub struct SegmentDiscovery;

impl SegmentDiscovery {
    /// SHM directory path.
    const SHM_DIR: &'static str = "/dev/shm";
    /// Filename prefix for EVO segments (matches SHM_PREFIX without leading `/`).
    const FILE_PREFIX: &'static str = "evo_";

    /// List all EVO P2P segments found in `/dev/shm/`.
    ///
    /// Returns a sorted `Vec<SegmentInfo>` for every file matching `evo_*`
    /// (excluding `.lock` files). Each entry probes the P2P header for magic
    /// validation and the `.lock` SHM file for writer liveness.
    pub fn list_segments() -> Vec<SegmentInfo> {
        let dir = match std::fs::read_dir(Self::SHM_DIR) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };

        let mut segments = Vec::new();
        for entry in dir.flatten() {
            let fname = entry.file_name();
            let fname_str = fname.to_string_lossy();

            // Skip non-evo files and .lock files.
            if !fname_str.starts_with(Self::FILE_PREFIX) || fname_str.ends_with(".lock") {
                continue;
            }

            let name = fname_str[Self::FILE_PREFIX.len()..].to_string();
            let path = entry.path();

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mut info = SegmentInfo {
                name: name.clone(),
                path,
                size: meta.len(),
                valid_magic: false,
                writer_alive: false,
                source: None,
                dest: None,
                heartbeat: None,
            };

            // Read the first 64 bytes to extract P2P header fields.
            if meta.len() >= core::mem::size_of::<P2pSegmentHeader>() as u64 {
                if let Ok(data) = std::fs::read(&info.path) {
                    if data.len() >= core::mem::size_of::<P2pSegmentHeader>() {
                        Self::parse_header(&data, &mut info);
                    }
                }
            }

            // Probe writer liveness via flock on the `.lock` SHM file.
            info.writer_alive = Self::probe_writer(&name);

            segments.push(info);
        }

        segments.sort_by(|a, b| a.name.cmp(&b.name));
        segments
    }

    /// List segments where the source or destination matches `module`.
    pub fn list_for(module: ModuleAbbrev) -> Vec<SegmentInfo> {
        Self::list_segments()
            .into_iter()
            .filter(|s| s.source == Some(module) || s.dest == Some(module))
            .collect()
    }

    /// Remove orphan (dead-writer) segments from `/dev/shm/`.
    ///
    /// A segment is considered dead if no writer holds the `.lock` file.
    /// Returns the number of cleaned-up segments.
    pub fn cleanup_dead() -> usize {
        let segments = Self::list_segments();
        let mut cleaned = 0;
        for seg in &segments {
            if !seg.writer_alive {
                let data_name = format!("{}{}", SHM_PREFIX, seg.name);
                let lock_name = format!("{}{}.lock", SHM_PREFIX, seg.name);
                let _ = mman::shm_unlink(data_name.as_str());
                let _ = mman::shm_unlink(lock_name.as_str());
                cleaned += 1;
            }
        }
        cleaned
    }

    /// Parse P2P header fields from the first 64 bytes of raw SHM data.
    fn parse_header(data: &[u8], info: &mut SegmentInfo) {
        // Check magic.
        if data[..8] == EVO_P2P_MAGIC {
            info.valid_magic = true;
            // source_module at offset 24, dest_module at offset 25.
            info.source = ModuleAbbrev::from_u8(data[24]);
            info.dest = ModuleAbbrev::from_u8(data[25]);
            // heartbeat at offset 16.
            info.heartbeat = Some(u64::from_ne_bytes(
                data[HEARTBEAT_OFFSET..HEARTBEAT_OFFSET + 8]
                    .try_into()
                    .unwrap_or([0; 8]),
            ));
        }
    }

    /// Probe whether a writer is alive by attempting a non-blocking exclusive flock
    /// on the `.lock` SHM file.
    ///
    /// - If `flock(LOCK_EX | LOCK_NB)` **succeeds**, no writer holds it → writer is dead.
    /// - If it fails with `EWOULDBLOCK`, a writer is alive.
    /// - If the `.lock` file doesn't exist, the writer is dead.
    fn probe_writer(segment_name: &str) -> bool {
        let lock_shm_name = format!("{}{}.lock", SHM_PREFIX, segment_name);

        let fd = match mman::shm_open(
            lock_shm_name.as_str(),
            OFlag::O_RDWR,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(_) => return false,
        };

        match Flock::lock(fd, FlockArg::LockExclusiveNonblock) {
            Ok(_flock) => {
                // Lock succeeded → no writer currently holds it → writer is dead.
                // Flock is released on drop of the returned Flock guard.
                false
            }
            Err((_, errno)) => {
                // EWOULDBLOCK means someone already holds the lock → writer alive.
                errno == nix::errno::Errno::EWOULDBLOCK
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_size_and_alignment() {
        assert_eq!(core::mem::size_of::<P2pSegmentHeader>(), 64);
        assert_eq!(core::mem::align_of::<P2pSegmentHeader>(), 64);
    }

    #[test]
    fn magic_validation() {
        let header = P2pSegmentHeader::new(ModuleAbbrev::Cu, ModuleAbbrev::Hal, 0, 0);
        assert!(header.is_magic_valid());

        let mut bad_header = header;
        bad_header.magic[0] = b'X';
        assert!(!bad_header.is_magic_valid());
    }

    #[test]
    fn version_hash_determinism() {
        let h1 = struct_version_hash::<P2pSegmentHeader>();
        let h2 = struct_version_hash::<P2pSegmentHeader>();
        assert_eq!(h1, h2);
    }

    #[test]
    fn version_hash_differs_for_different_types() {
        let h1 = struct_version_hash::<P2pSegmentHeader>();
        let h2 = struct_version_hash::<u8>();
        assert_ne!(h1, h2);
    }

    #[test]
    fn module_abbrev_roundtrip() {
        for val in 0..=4u8 {
            let abbrev = ModuleAbbrev::from_u8(val).unwrap();
            assert_eq!(abbrev as u8, val);
        }
        assert!(ModuleAbbrev::from_u8(5).is_none());
        assert!(ModuleAbbrev::from_u8(255).is_none());
    }

    #[test]
    fn data_size_rounds_up_to_page() {
        assert_eq!(data_size_for::<P2pSegmentHeader>(), PAGE_SIZE);
        assert_eq!(data_size_for::<u8>(), PAGE_SIZE);
    }

    #[test]
    fn shm_error_display() {
        let e = ShmError::WriterAlreadyExists {
            segment: "test".to_string(),
        };
        assert!(e.to_string().contains("writer already exists"));

        let e = ShmError::HeartbeatStale {
            segment: "test".to_string(),
            missed_beats: 5,
        };
        assert!(e.to_string().contains("5 consecutive misses"));
    }

    #[test]
    fn verify_p2p_header_offsets() {
        let header = P2pSegmentHeader::new(ModuleAbbrev::Cu, ModuleAbbrev::Hal, 0xDEAD_BEEF, 42);
        let bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                &header as *const P2pSegmentHeader as *const u8,
                core::mem::size_of::<P2pSegmentHeader>(),
            )
        };

        let hb =
            u64::from_ne_bytes(bytes[HEARTBEAT_OFFSET..HEARTBEAT_OFFSET + 8].try_into().unwrap());
        assert_eq!(hb, 0);

        let ws =
            u32::from_ne_bytes(bytes[WRITE_SEQ_OFFSET..WRITE_SEQ_OFFSET + 4].try_into().unwrap());
        assert_eq!(ws, 0);

        let vh = u32::from_ne_bytes(bytes[8..12].try_into().unwrap());
        assert_eq!(vh, 0xDEAD_BEEF);

        let ps = u32::from_ne_bytes(bytes[28..32].try_into().unwrap());
        assert_eq!(ps, 42);
    }

    /// Test: create a writer, write data, attach a reader, read data.
    #[test]
    fn writer_reader_roundtrip() {
        let name = format!("test_rt_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct TestSegment {
            header: P2pSegmentHeader,
            value: u64,
            _pad: [u8; 56],
        }

        let mut writer =
            TypedP2pWriter::<TestSegment>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
                .expect("create writer");

        let mut payload: TestSegment = unsafe { core::mem::zeroed() };
        payload.value = 0xCAFE_BABE;
        writer.commit(&payload).expect("commit");
        assert_eq!(writer.heartbeat(), 1);

        let mut reader = TypedP2pReader::<TestSegment>::attach(&name, 10).expect("attach reader");

        let data = reader.read().expect("read");
        assert_eq!(data.value, 0xCAFE_BABE);

        payload.value = 0xDEAD_BEEF;
        writer.commit(&payload).expect("commit 2");
        assert_eq!(writer.heartbeat(), 2);

        let data = reader.read().expect("read 2");
        assert_eq!(data.value, 0xDEAD_BEEF);
    }

    /// Test: duplicate writer is rejected.
    #[test]
    fn duplicate_writer_rejected() {
        let name = format!("test_dup_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            _pad: [u8; 64],
        }

        let _writer =
            TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
                .expect("first writer");

        let result = TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal);
        assert!(matches!(result, Err(ShmError::WriterAlreadyExists { .. })));
    }

    /// Test: segment not found returns appropriate error.
    #[test]
    fn reader_not_found() {
        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            _pad: [u8; 64],
        }

        let result = TypedP2pReader::<Seg>::attach("nonexistent_seg_12345", 10);
        assert!(matches!(result, Err(ShmError::SegmentNotFound { .. })));
    }

    /// Test: heartbeat staleness detection.
    #[test]
    fn heartbeat_staleness() {
        let name = format!("test_stale_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            _pad: [u8; 64],
        }

        let mut writer =
            TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
                .expect("create writer");

        let payload: Seg = unsafe { core::mem::zeroed() };
        writer.commit(&payload).expect("commit");

        let mut reader = TypedP2pReader::<Seg>::attach(&name, 3).expect("attach");

        // First read succeeds (heartbeat changes from 0 to 1).
        reader.read().expect("read 1");
        // Second read: heartbeat unchanged → stale_count = 1.
        reader.read().expect("read 2");
        // Third read: stale_count = 2.
        reader.read().expect("read 3");
        // Fourth read: stale_count = 3 → should error.
        let result = reader.read();
        assert!(matches!(result, Err(ShmError::HeartbeatStale { .. })));
    }

    /// Test: version hash mismatch detection.
    #[test]
    fn version_hash_mismatch() {
        let name = format!("test_vmm_{}", std::process::id());

        // SegA is 128 bytes (1 cache line header + 64 bytes payload).
        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct SegA {
            header: P2pSegmentHeader,
            value: u64,
            _pad: [u8; 56],
        }

        // SegB is 256 bytes (different size → different version hash).
        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct SegB {
            header: P2pSegmentHeader,
            values: [u64; 24],
        }

        // Sanity: hashes must differ.
        assert_ne!(
            struct_version_hash::<SegA>(),
            struct_version_hash::<SegB>()
        );

        let mut writer =
            TypedP2pWriter::<SegA>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
                .expect("create writer");

        let payload: SegA = unsafe { core::mem::zeroed() };
        writer.commit(&payload).expect("commit");

        let mut reader = TypedP2pReader::<SegB>::attach(&name, 10).expect("attach");
        let result = reader.read();
        assert!(matches!(result, Err(ShmError::VersionMismatch { .. })));
    }

    /// Test: writer drop cleans up SHM segment.
    #[test]
    fn writer_drop_cleanup() {
        let name = format!("test_drop_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            _pad: [u8; 64],
        }

        {
            let _writer =
                TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
                    .expect("create writer");
        }

        let shm_file = format!("/dev/shm/evo_{name}");
        assert!(
            !std::path::Path::new(&shm_file).exists(),
            "SHM file should be removed after writer drop"
        );
    }

    /// Test: destination validation.
    #[test]
    fn destination_validation() {
        let name = format!("test_dest_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            _pad: [u8; 64],
        }

        let mut writer =
            TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
                .expect("create writer");

        let payload: Seg = unsafe { core::mem::zeroed() };
        writer.commit(&payload).expect("commit");

        let _reader = TypedP2pReader::<Seg>::attach_validated(&name, 10, ModuleAbbrev::Hal)
            .expect("attach with correct dest");

        let result = TypedP2pReader::<Seg>::attach_validated(&name, 10, ModuleAbbrev::Re);
        assert!(matches!(result, Err(ShmError::DestinationMismatch { .. })));
    }

    /// Test: multiple readers can attach simultaneously.
    #[test]
    fn multiple_readers() {
        let name = format!("test_multi_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            value: u64,
            _pad: [u8; 56],
        }

        let mut writer =
            TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
                .expect("create writer");

        let mut payload: Seg = unsafe { core::mem::zeroed() };
        payload.value = 42;
        writer.commit(&payload).expect("commit");

        // Attach multiple readers simultaneously — this must not fail.
        let mut reader1 = TypedP2pReader::<Seg>::attach(&name, 10).expect("reader 1");
        let mut reader2 = TypedP2pReader::<Seg>::attach(&name, 10).expect("reader 2");

        let d1 = reader1.read().expect("read 1");
        let d2 = reader2.read().expect("read 2");
        assert_eq!(d1.value, 42);
        assert_eq!(d2.value, 42);
    }

    // ─── Discovery Tests ────────────────────────────────────────────

    /// Test: SegmentDiscovery finds a created segment with valid header info.
    #[test]
    fn discovery_finds_segment() {
        let name = format!("test_disc_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            values: [u64; 8],
        }

        let mut writer =
            TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Hal, ModuleAbbrev::Cu)
                .expect("create writer");

        let payload: Seg = unsafe { core::mem::zeroed() };
        writer.commit(&payload).expect("commit");

        let all = SegmentDiscovery::list_segments();
        let found = all.iter().find(|s| s.name == name);
        assert!(found.is_some(), "discovery should find the segment");

        let info = found.unwrap();
        assert!(info.valid_magic, "magic should be valid");
        assert!(info.writer_alive, "writer should be alive");
        assert_eq!(info.source, Some(ModuleAbbrev::Hal));
        assert_eq!(info.dest, Some(ModuleAbbrev::Cu));
        assert!(info.heartbeat.is_some());
    }

    /// Test: writer dead after drop is detected by discovery.
    #[test]
    fn discovery_writer_dead_after_drop() {
        let name = format!("test_disc_dead_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            val: u64,
            _pad: [u8; 56],
        }

        // Create and immediately drop the writer.
        {
            let mut writer =
                TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Mqt)
                    .expect("create writer");
            let payload: Seg = unsafe { core::mem::zeroed() };
            writer.commit(&payload).expect("commit");
        }
        // Writer is dropped — shm_unlink removes the segment.
        // Discovery should NOT find it.
        let all = SegmentDiscovery::list_segments();
        let found = all.iter().find(|s| s.name == name);
        assert!(found.is_none(), "segment should be removed after writer drop");
    }

    /// Test: list_for filters by module.
    #[test]
    fn discovery_list_for_module() {
        let pid = std::process::id();
        let name_a = format!("test_disc_a_{pid}");
        let name_b = format!("test_disc_b_{pid}");

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            _data: [u8; 64],
        }

        let mut wa =
            TypedP2pWriter::<Seg>::create(&name_a, ModuleAbbrev::Hal, ModuleAbbrev::Cu)
                .expect("create A");
        let mut wb =
            TypedP2pWriter::<Seg>::create(&name_b, ModuleAbbrev::Re, ModuleAbbrev::Mqt)
                .expect("create B");

        let payload: Seg = unsafe { core::mem::zeroed() };
        wa.commit(&payload).expect("commit A");
        wb.commit(&payload).expect("commit B");

        let hal_segs = SegmentDiscovery::list_for(ModuleAbbrev::Hal);
        assert!(
            hal_segs.iter().any(|s| s.name == name_a),
            "HAL should find name_a"
        );
        assert!(
            !hal_segs.iter().any(|s| s.name == name_b),
            "HAL should NOT find name_b"
        );

        let mqt_segs = SegmentDiscovery::list_for(ModuleAbbrev::Mqt);
        assert!(
            mqt_segs.iter().any(|s| s.name == name_b),
            "MQT should find name_b as dest"
        );
    }

    /// Test: cleanup_dead removes orphan segments.
    #[test]
    fn discovery_cleanup_dead() {
        let name = format!("test_disc_cleanup_{}", std::process::id());

        #[derive(Debug, Clone, Copy)]
        #[repr(C, align(64))]
        struct Seg {
            header: P2pSegmentHeader,
            _data: [u8; 64],
        }

        // Create a segment, commit data, then manually unlink the lock file
        // to simulate a crashed writer (data remains but lock file is gone).
        let mut writer =
            TypedP2pWriter::<Seg>::create(&name, ModuleAbbrev::Cu, ModuleAbbrev::Hal)
                .expect("create writer");
        let payload: Seg = unsafe { core::mem::zeroed() };
        writer.commit(&payload).expect("commit");

        // Manually create an orphan: unlink the lock, keep the data.
        // We do this by creating a second segment, dropping it, then
        // re-creating just the data file without the lock.
        // Simpler approach: create a raw data segment without any lock file.
        let orphan_name = format!("test_disc_orphan_{}", std::process::id());
        let shm_name = format!("/evo_{orphan_name}");
        {
            // Create data segment manually (no lock file).
            let fd = mman::shm_open(
                shm_name.as_str(),
                OFlag::O_CREAT | OFlag::O_RDWR,
                Mode::from_bits_truncate(0o600),
            )
            .expect("shm_open orphan");
            unistd::ftruncate(&fd, 4096).expect("ftruncate orphan");

            // Write valid magic so parse_header works.
            use std::io::Write;
            use std::os::fd::{AsRawFd, FromRawFd};
            let mut file = unsafe { std::fs::File::from_raw_fd(fd.as_raw_fd()) };
            file.write_all(&EVO_P2P_MAGIC).ok();
            // Don't close — OwnedFd already owns it. Forget the File.
            std::mem::forget(file);
        }

        // The orphan should be discovered but writer_alive=false.
        let all = SegmentDiscovery::list_segments();
        let orphan_info = all.iter().find(|s| s.name == orphan_name);
        assert!(orphan_info.is_some(), "orphan should be discovered");
        assert!(!orphan_info.unwrap().writer_alive, "orphan has no writer");

        // Live writer should still be alive.
        let live = all.iter().find(|s| s.name == name);
        assert!(live.is_some());
        assert!(live.unwrap().writer_alive);

        // Cleanup should remove the orphan.
        let cleaned = SegmentDiscovery::cleanup_dead();
        assert!(cleaned >= 1, "at least the orphan should be cleaned");

        // Verify orphan is gone.
        let after = SegmentDiscovery::list_segments();
        assert!(
            !after.iter().any(|s| s.name == orphan_name),
            "orphan should be gone after cleanup"
        );

        // Live writer should still be present.
        assert!(after.iter().any(|s| s.name == name), "live segment survives cleanup");
    }
}
