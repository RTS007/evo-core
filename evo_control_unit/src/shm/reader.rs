//! Inbound segment reading (T029).
//!
//! Reads HAL→CU, RE→CU, RPC→CU segments with heartbeat staleness
//! detection (N=3 for RT, configurable for non-RT) and one-time
//! P2P version hash validation.

use std::marker::PhantomData;

use evo_common::shm::p2p::{struct_version_hash, P2pSegmentHeader};
use evo_shared_memory::SegmentReader;

use super::segments::SegmentError;

/// Typed inbound segment reader with heartbeat staleness detection.
///
/// Wraps [`SegmentReader`] to provide:
/// - Zero-copy typed access to segment payloads
/// - One-time P2P magic + version hash validation on first read
/// - Per-read heartbeat staleness monitoring
///
/// # Safety
/// `T` must be `#[repr(C)]` with a `P2pSegmentHeader` as its first field.
/// All segment payload types in `evo_common::control_unit::shm` satisfy this.
pub struct InboundReader<T: Copy> {
    reader: SegmentReader,
    /// Name stored for error messages.
    name: String,
    /// Pre-allocated aligned buffer for payload deserialization.
    /// Avoids alignment issues from the library's byte-aligned read buffer.
    payload: T,
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

impl<T: Copy> InboundReader<T> {
    /// Attach to an existing P2P segment.
    ///
    /// # Arguments
    /// - `name`: Segment name (e.g., `"hal_cu"`).
    /// - `stale_threshold`: Max consecutive reads without heartbeat change.
    ///
    /// # Errors
    /// - `SegmentError::Shm(NotFound)` if the segment does not exist.
    /// - `SegmentError::PayloadTooSmall` if the segment data is smaller than `T`.
    pub fn attach(name: &str, stale_threshold: u32) -> Result<Self, SegmentError> {
        let reader = SegmentReader::attach(name)?;

        // Validate that the segment is large enough for our type.
        let data_size = reader.data_size();
        let type_size = core::mem::size_of::<T>();
        if data_size < type_size {
            return Err(SegmentError::PayloadTooSmall {
                segment: name.to_string(),
                expected: type_size,
                actual: data_size,
            });
        }

        // Zero-initialize the payload buffer.
        // SAFETY: All P2P segment types are repr(C) with only numeric fields;
        // all-zeros is a valid bit pattern for every field.
        let payload: T = unsafe { core::mem::zeroed() };

        Ok(Self {
            reader,
            name: name.to_string(),
            payload,
            verified: false,
            expected_hash: struct_version_hash::<T>(),
            last_heartbeat: 0,
            stale_count: 0,
            stale_threshold,
            _marker: PhantomData,
        })
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
    /// - `SegmentError::InvalidMagic` on first read if magic is wrong.
    /// - `SegmentError::VersionMismatch` on first read if hash differs.
    /// - `SegmentError::Stale` if heartbeat unchanged for `stale_threshold` reads.
    /// - `SegmentError::Shm(VersionConflict)` if too many read retries.
    pub fn read(&mut self) -> Result<&T, SegmentError> {
        let data = self.reader.read()?;
        let type_size = core::mem::size_of::<T>();

        // Copy to aligned buffer (library read buffer is byte-aligned).
        // SAFETY: We validated data_size >= type_size at attach time,
        // and SegmentReader::read() returns data_size bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                &mut self.payload as *mut T as *mut u8,
                type_size,
            );
        }

        // ── One-time P2P header validation ──
        if !self.verified {
            // SAFETY: T has P2pSegmentHeader at offset 0.
            let header =
                unsafe { &*(&self.payload as *const T as *const P2pSegmentHeader) };

            if !header.is_magic_valid() {
                return Err(SegmentError::InvalidMagic {
                    segment: self.name.clone(),
                });
            }
            if header.version_hash != self.expected_hash {
                return Err(SegmentError::VersionMismatch {
                    segment: self.name.clone(),
                    expected: self.expected_hash,
                    actual: header.version_hash,
                });
            }
            self.verified = true;
        }

        // ── Heartbeat staleness check ──
        let header =
            unsafe { &*(&self.payload as *const T as *const P2pSegmentHeader) };
        let heartbeat = header.heartbeat;

        if heartbeat == self.last_heartbeat && self.last_heartbeat != 0 {
            self.stale_count += 1;
            if self.stale_count >= self.stale_threshold {
                return Err(SegmentError::Stale {
                    segment: self.name.clone(),
                    missed_beats: self.stale_count,
                });
            }
        } else {
            self.last_heartbeat = heartbeat;
            self.stale_count = 0;
        }

        Ok(&self.payload)
    }

    /// Check if the segment has new data since the last read.
    ///
    /// This is a cheap check (no data copy) using the library-level
    /// version counter, not the P2P heartbeat.
    pub fn has_changed(&self) -> bool {
        self.reader.has_changed()
    }

    /// Get the segment name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the current stale count (consecutive reads without heartbeat change).
    pub fn stale_count(&self) -> u32 {
        self.stale_count
    }

    /// Get the last observed heartbeat value.
    pub fn last_heartbeat(&self) -> u64 {
        self.last_heartbeat
    }

    /// Reset staleness counter (e.g., after a recovery action).
    pub fn reset_stale(&mut self) {
        self.stale_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use evo_common::shm::p2p::P2pSegmentHeader;

    #[test]
    fn reader_type_constraints() {
        // Verify that all our segment types are Copy (required by InboundReader).
        fn assert_copy<T: Copy>() {}
        assert_copy::<evo_common::control_unit::shm::HalToCuSegment>();
        assert_copy::<evo_common::control_unit::shm::ReToCuSegment>();
        assert_copy::<evo_common::control_unit::shm::RpcToCuSegment>();
    }

    #[test]
    fn p2p_header_at_offset_zero() {
        // Verify that P2pSegmentHeader is at offset 0 of each inbound segment type.
        use evo_common::control_unit::shm::{HalToCuSegment, ReToCuSegment, RpcToCuSegment};

        fn check_header_offset<T: Copy>() {
            let val: T = unsafe { core::mem::zeroed() };
            let t_ptr = &val as *const T as usize;
            let h_ptr = unsafe { &*(&val as *const T as *const P2pSegmentHeader) } as *const P2pSegmentHeader as usize;
            assert_eq!(t_ptr, h_ptr, "P2pSegmentHeader is not at offset 0");
        }

        check_header_offset::<HalToCuSegment>();
        check_header_offset::<ReToCuSegment>();
        check_header_offset::<RpcToCuSegment>();
    }
}
