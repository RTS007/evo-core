//! P2P Shared Memory segment header and module identifiers.
//!
//! Defines the `P2pSegmentHeader` struct (64 bytes, cache-line aligned)
//! used as the header for all P2P shared memory segments, and the
//! `ModuleAbbrev` enum identifying source/destination modules.

use static_assertions::const_assert_eq;

/// Magic bytes identifying a valid P2P segment: `"EVO_P2P\0"`.
pub const EVO_P2P_MAGIC: [u8; 8] = *b"EVO_P2P\0";

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
    pub const fn new(source: ModuleAbbrev, dest: ModuleAbbrev, version_hash: u32, payload_size: u32) -> Self {
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
        m[0] == b'E' && m[1] == b'V' && m[2] == b'O' && m[3] == b'_'
            && m[4] == b'P' && m[5] == b'2' && m[6] == b'P' && m[7] == 0
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
}
