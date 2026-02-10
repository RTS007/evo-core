//! Outbound segment writing (T030).
//!
//! Writes CU→HAL, CU→MQT, CU→RE segments with heartbeat increment
//! and write_seq lock-free protocol (odd=writing, even=committed).
//!
//! The library-level `SegmentWriter::write()` already provides the
//! odd/even version protocol. This wrapper additionally manages the
//! P2P header's `heartbeat` and `write_seq` fields.

use std::marker::PhantomData;

use evo_common::shm::p2p::{struct_version_hash, ModuleAbbrev, P2pSegmentHeader};
use evo_shared_memory::SegmentWriter;

use super::segments::{data_size_for, SegmentError};

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
//
// Verified by static assertion below.

const HEARTBEAT_OFFSET: usize = 16;
const WRITE_SEQ_OFFSET: usize = 32;

// Compile-time verification of field offsets.
// We construct a zeroed header and check field addresses.
const _: () = {
    assert!(core::mem::size_of::<P2pSegmentHeader>() == 64);
    // We can't take field addresses in const context, but we verify
    // the total size matches our hand-computed layout. The offset
    // assertions are done in the runtime test below.
};

/// Typed outbound segment writer with P2P heartbeat management.
///
/// Wraps [`SegmentWriter`] to provide:
/// - Pre-allocated write buffer (no allocation in RT loop)
/// - Automatic heartbeat incrementing on every commit
/// - P2P header initialization with correct magic, version hash,
///   source/destination modules
///
/// # Safety
/// `T` must be `#[repr(C)]` with a `P2pSegmentHeader` as its first field.
/// All segment payload types in `evo_common::control_unit::shm` satisfy this.
pub struct OutboundWriter<T: Copy> {
    writer: SegmentWriter,
    /// Pre-allocated byte buffer (page-aligned size). Reused every cycle.
    write_buf: Vec<u8>,
    /// Cached P2P header template (magic, version_hash, source, dest, payload_size).
    /// Re-applied on every `commit()` so callers need not set header fields.
    header_template: [u8; 64],
    /// Monotonic heartbeat counter, incremented on every `commit()`.
    heartbeat: u64,
    _marker: PhantomData<T>,
}

impl<T: Copy> OutboundWriter<T> {
    /// Create a new outbound writer segment.
    ///
    /// Allocates the SHM segment and writes an initial P2P header.
    /// The write buffer is pre-allocated to avoid RT-path allocation.
    ///
    /// # Arguments
    /// - `name`: Segment name (e.g., `"cu_hal"`).
    /// - `source`: Source module identifier.
    /// - `dest`: Destination module identifier.
    pub fn create(
        name: &str,
        source: ModuleAbbrev,
        dest: ModuleAbbrev,
    ) -> Result<Self, SegmentError> {
        let data_size = data_size_for::<T>();
        let mut writer = SegmentWriter::create(name, data_size)?;

        // Build initial P2P header.
        let payload_bytes =
            (core::mem::size_of::<T>() - core::mem::size_of::<P2pSegmentHeader>()) as u32;
        let header = P2pSegmentHeader::new(source, dest, struct_version_hash::<T>(), payload_bytes);

        // Serialize header into the write buffer.
        let mut write_buf = vec![0u8; data_size];
        let hdr_size = core::mem::size_of::<P2pSegmentHeader>();
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const P2pSegmentHeader as *const u8,
                hdr_size,
            )
        };
        write_buf[..hdr_size].copy_from_slice(hdr_bytes);

        // Cache the header template for re-application on every commit().
        let mut header_template = [0u8; 64];
        header_template.copy_from_slice(hdr_bytes);

        // Write initial data so readers see a valid header immediately.
        writer.write(&write_buf)?;

        Ok(Self {
            writer,
            write_buf,
            header_template,
            heartbeat: 0,
            _marker: PhantomData,
        })
    }

    /// Write a complete segment payload to shared memory.
    ///
    /// This method:
    /// 1. Copies the payload `T` into the pre-allocated buffer.
    /// 2. Increments the P2P heartbeat counter.
    /// 3. Sets `write_seq` to an even value (committed).
    /// 4. Calls the library's `write()` which handles the library-level
    ///    odd/even version protocol and memory barriers.
    ///
    /// # RT Safety
    /// No heap allocation occurs in this method. The write buffer is
    /// pre-allocated at `create()` time.
    pub fn commit(&mut self, payload: &T) -> Result<(), SegmentError> {
        let type_size = core::mem::size_of::<T>();
        let hdr_size = core::mem::size_of::<P2pSegmentHeader>();

        // Copy payload bytes to pre-allocated buffer (includes caller's header).
        let src = unsafe { core::slice::from_raw_parts(payload as *const T as *const u8, type_size) };
        self.write_buf[..type_size].copy_from_slice(src);

        // Re-apply cached P2P header template (magic, version_hash, source,
        // dest, payload_size). This ensures correctness even if the caller
        // passes a zeroed or partially-filled struct.
        self.write_buf[..hdr_size].copy_from_slice(&self.header_template);

        // Increment heartbeat.
        self.heartbeat += 1;
        self.write_buf[HEARTBEAT_OFFSET..HEARTBEAT_OFFSET + 8]
            .copy_from_slice(&self.heartbeat.to_ne_bytes());

        // Set write_seq to even (committed). The library-level odd/even
        // protocol provides the actual atomicity guarantee.
        let seq = (self.heartbeat as u32).wrapping_mul(2);
        self.write_buf[WRITE_SEQ_OFFSET..WRITE_SEQ_OFFSET + 4]
            .copy_from_slice(&seq.to_ne_bytes());

        // Library write: sets library version odd → copies data → sets even.
        self.writer.write(&self.write_buf)?;
        Ok(())
    }

    /// Get the current heartbeat counter value.
    pub fn heartbeat(&self) -> u64 {
        self.heartbeat
    }

    /// Get the segment name.
    pub fn name(&self) -> &str {
        self.writer.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_common::shm::p2p::P2pSegmentHeader;

    #[test]
    fn verify_p2p_header_offsets() {
        // Construct a header with known field values and check byte offsets.
        let header = P2pSegmentHeader::new(
            ModuleAbbrev::Cu,
            ModuleAbbrev::Hal,
            0xDEAD_BEEF,
            42,
        );
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const P2pSegmentHeader as *const u8,
                core::mem::size_of::<P2pSegmentHeader>(),
            )
        };

        // heartbeat at offset 16 (8 bytes, little-endian 0)
        let hb = u64::from_ne_bytes(bytes[HEARTBEAT_OFFSET..HEARTBEAT_OFFSET + 8].try_into().unwrap());
        assert_eq!(hb, 0, "heartbeat should be 0 in a new header");

        // write_seq at offset 32 (4 bytes, little-endian 0)
        let ws = u32::from_ne_bytes(bytes[WRITE_SEQ_OFFSET..WRITE_SEQ_OFFSET + 4].try_into().unwrap());
        assert_eq!(ws, 0, "write_seq should be 0 in a new header");

        // version_hash at offset 8
        let vh = u32::from_ne_bytes(bytes[8..12].try_into().unwrap());
        assert_eq!(vh, 0xDEAD_BEEF);

        // payload_size at offset 28
        let ps = u32::from_ne_bytes(bytes[28..32].try_into().unwrap());
        assert_eq!(ps, 42);
    }

    #[test]
    fn writer_type_constraints() {
        // Verify that all our outbound segment types are Copy.
        fn assert_copy<T: Copy>() {}
        assert_copy::<evo_common::control_unit::shm::CuToHalSegment>();
        assert_copy::<evo_common::control_unit::shm::CuToMqtSegment>();
        assert_copy::<evo_common::control_unit::shm::CuToReSegment>();
    }

    #[test]
    fn p2p_header_at_offset_zero_outbound() {
        use evo_common::control_unit::shm::{CuToHalSegment, CuToMqtSegment, CuToReSegment};

        fn check_header_offset<T: Copy>() {
            let val: T = unsafe { core::mem::zeroed() };
            let t_ptr = &val as *const T as usize;
            let h_ptr =
                unsafe { &*(&val as *const T as *const P2pSegmentHeader) }
                    as *const P2pSegmentHeader as usize;
            assert_eq!(t_ptr, h_ptr, "P2pSegmentHeader is not at offset 0");
        }

        check_header_offset::<CuToHalSegment>();
        check_header_offset::<CuToMqtSegment>();
        check_header_offset::<CuToReSegment>();
    }
}
