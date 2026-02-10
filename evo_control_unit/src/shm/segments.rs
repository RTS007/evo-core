//! P2P segment connection and lifecycle management (T028).
//!
//! Creates outbound writer segments (CU→HAL, CU→MQT, CU→RE) and
//! attaches inbound reader segments (HAL→CU, RE→CU, RPC→CU) with
//! P2P version hash validation.

use evo_common::control_unit::shm::{
    CuToHalSegment, CuToMqtSegment, CuToReSegment, HalToCuSegment, ReToCuSegment,
    RpcToCuSegment,
};
use evo_common::shm::p2p::ModuleAbbrev;
use evo_shared_memory::ShmError;

use super::reader::InboundReader;
use super::writer::OutboundWriter;

// ─── Segment Names (P2P convention: evo_[SOURCE]_[DEST]) ───────────

/// HAL → CU inbound segment name.
pub const SEG_HAL_CU: &str = "hal_cu";
/// CU → HAL outbound segment name.
pub const SEG_CU_HAL: &str = "cu_hal";
/// RE → CU inbound segment name.
pub const SEG_RE_CU: &str = "re_cu";
/// CU → MQT outbound segment name.
pub const SEG_CU_MQT: &str = "cu_mqt";
/// RPC → CU inbound segment name.
pub const SEG_RPC_CU: &str = "rpc_cu";
/// CU → RE outbound segment name.
pub const SEG_CU_RE: &str = "cu_re";

// ─── Error Type ─────────────────────────────────────────────────────

/// Errors that can occur during segment setup or runtime I/O.
#[derive(Debug)]
pub enum SegmentError {
    /// Shared memory library error.
    Shm(ShmError),
    /// P2P version hash mismatch (struct layout incompatibility).
    VersionMismatch {
        /// Segment name.
        segment: String,
        /// Expected hash (compiled-in).
        expected: u32,
        /// Actual hash read from SHM.
        actual: u32,
    },
    /// Invalid P2P magic bytes.
    InvalidMagic {
        /// Segment name.
        segment: String,
    },
    /// Heartbeat staleness detected (writer stopped updating).
    Stale {
        /// Segment name.
        segment: String,
        /// Number of consecutive reads without heartbeat change.
        missed_beats: u32,
    },
    /// Segment data too small for the expected payload type.
    PayloadTooSmall {
        /// Segment name.
        segment: String,
        /// Expected minimum size.
        expected: usize,
        /// Actual data size.
        actual: usize,
    },
}

impl std::fmt::Display for SegmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shm(e) => write!(f, "SHM error: {e}"),
            Self::VersionMismatch {
                segment,
                expected,
                actual,
            } => write!(
                f,
                "version hash mismatch on '{segment}': expected 0x{expected:08X}, got 0x{actual:08X}"
            ),
            Self::InvalidMagic { segment } => {
                write!(f, "invalid P2P magic on '{segment}'")
            }
            Self::Stale {
                segment,
                missed_beats,
            } => write!(
                f,
                "heartbeat stale on '{segment}': {missed_beats} consecutive misses"
            ),
            Self::PayloadTooSmall {
                segment,
                expected,
                actual,
            } => write!(
                f,
                "payload too small on '{segment}': need {expected} bytes, got {actual}"
            ),
        }
    }
}

impl std::error::Error for SegmentError {}

impl From<ShmError> for SegmentError {
    fn from(e: ShmError) -> Self {
        Self::Shm(e)
    }
}

// ─── Staleness Thresholds ───────────────────────────────────────────

/// Staleness thresholds for inbound segment readers.
///
/// These control how many consecutive reads without a heartbeat change
/// trigger a staleness error.
#[derive(Debug, Clone, Copy)]
pub struct SegmentThresholds {
    /// HAL heartbeat staleness threshold [cycles] (FR-130c: RT, default 3).
    pub hal_stale: u32,
    /// RE heartbeat staleness threshold [cycles] (configurable, default 1000).
    pub re_stale: u32,
    /// RPC heartbeat staleness threshold [cycles] (configurable, default 1000).
    pub rpc_stale: u32,
}

impl Default for SegmentThresholds {
    fn default() -> Self {
        Self {
            hal_stale: 3,
            re_stale: 1000,
            rpc_stale: 1000,
        }
    }
}

// ─── CU Segment Bundle ─────────────────────────────────────────────

/// All P2P segments owned or observed by the Control Unit.
///
/// The CU creates (writes) 3 outbound segments and attaches (reads)
/// 3 inbound segments. RE→CU and RPC→CU are optional because those
/// processes may not be running at CU startup.
pub struct CuSegments {
    // ── Outbound (CU writes) ──
    /// CU → HAL: axis commands, digital/analog outputs.
    pub cu_to_hal: OutboundWriter<CuToHalSegment>,
    /// CU → MQT: diagnostic state snapshot.
    pub cu_to_mqt: OutboundWriter<CuToMqtSegment>,
    /// CU → RE: recipe acknowledgements.
    pub cu_to_re: OutboundWriter<CuToReSegment>,

    // ── Inbound (CU reads) ──
    /// HAL → CU: axis feedback, digital/analog inputs. **Required.**
    pub hal_to_cu: InboundReader<HalToCuSegment>,
    /// RE → CU: recipe commands. **Optional** (RE may start later).
    pub re_to_cu: Option<InboundReader<ReToCuSegment>>,
    /// RPC → CU: API commands. **Optional** (API may start later).
    pub rpc_to_cu: Option<InboundReader<RpcToCuSegment>>,
}

impl CuSegments {
    /// Create all outbound segments and attach all inbound segments.
    ///
    /// # Errors
    /// - Returns `SegmentError::Shm` if segment creation/attachment fails.
    /// - Returns `SegmentError::VersionMismatch` if an inbound segment has
    ///   a different struct layout hash than compiled into this binary.
    /// - HAL→CU is required; failure to attach is a fatal error.
    /// - RE→CU and RPC→CU are optional; `NotFound` is not an error.
    pub fn init(thresholds: &SegmentThresholds) -> Result<Self, SegmentError> {
        // ── Create outbound writer segments ──
        let cu_to_hal = OutboundWriter::<CuToHalSegment>::create(
            SEG_CU_HAL,
            ModuleAbbrev::Cu,
            ModuleAbbrev::Hal,
        )?;
        let cu_to_mqt = OutboundWriter::<CuToMqtSegment>::create(
            SEG_CU_MQT,
            ModuleAbbrev::Cu,
            ModuleAbbrev::Mqt,
        )?;
        let cu_to_re = OutboundWriter::<CuToReSegment>::create(
            SEG_CU_RE,
            ModuleAbbrev::Cu,
            ModuleAbbrev::Re,
        )?;

        // ── Attach inbound reader segments ──
        let hal_to_cu =
            InboundReader::<HalToCuSegment>::attach(SEG_HAL_CU, thresholds.hal_stale)?;

        // RE and RPC are optional — they may not be running yet.
        let re_to_cu = match InboundReader::<ReToCuSegment>::attach(SEG_RE_CU, thresholds.re_stale)
        {
            Ok(r) => Some(r),
            Err(SegmentError::Shm(ShmError::NotFound { .. })) => None,
            Err(e) => return Err(e),
        };

        let rpc_to_cu =
            match InboundReader::<RpcToCuSegment>::attach(SEG_RPC_CU, thresholds.rpc_stale) {
                Ok(r) => Some(r),
                Err(SegmentError::Shm(ShmError::NotFound { .. })) => None,
                Err(e) => return Err(e),
            };

        Ok(Self {
            cu_to_hal,
            cu_to_mqt,
            cu_to_re,
            hal_to_cu,
            re_to_cu,
            rpc_to_cu,
        })
    }

    /// Attempt to late-attach the RE→CU segment (if RE started after CU).
    pub fn try_attach_re(&mut self, stale_threshold: u32) -> Result<bool, SegmentError> {
        if self.re_to_cu.is_some() {
            return Ok(true);
        }
        match InboundReader::<ReToCuSegment>::attach(SEG_RE_CU, stale_threshold) {
            Ok(r) => {
                self.re_to_cu = Some(r);
                Ok(true)
            }
            Err(SegmentError::Shm(ShmError::NotFound { .. })) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Attempt to late-attach the RPC→CU segment (if API started after CU).
    pub fn try_attach_rpc(&mut self, stale_threshold: u32) -> Result<bool, SegmentError> {
        if self.rpc_to_cu.is_some() {
            return Ok(true);
        }
        match InboundReader::<RpcToCuSegment>::attach(SEG_RPC_CU, stale_threshold) {
            Ok(r) => {
                self.rpc_to_cu = Some(r);
                Ok(true)
            }
            Err(SegmentError::Shm(ShmError::NotFound { .. })) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Page size for segment data allocation.
pub(crate) const PAGE_SIZE: usize = 4096;

/// Compute the minimum page-aligned data size for a segment type.
///
/// The evo_shared_memory library requires `size >= 4096 && size % 4096 == 0`.
pub(crate) fn data_size_for<T>() -> usize {
    let raw = core::mem::size_of::<T>();
    let pages = (raw + PAGE_SIZE - 1) / PAGE_SIZE;
    pages * PAGE_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_common::shm::p2p::{struct_version_hash, P2pSegmentHeader};

    #[test]
    fn data_size_rounds_up_to_page() {
        // All our segment types fit within one page.
        assert_eq!(data_size_for::<HalToCuSegment>(), PAGE_SIZE);
        assert_eq!(data_size_for::<CuToHalSegment>(), PAGE_SIZE);
        assert_eq!(data_size_for::<ReToCuSegment>(), PAGE_SIZE);
        assert_eq!(data_size_for::<CuToMqtSegment>(), PAGE_SIZE);
        assert_eq!(data_size_for::<RpcToCuSegment>(), PAGE_SIZE);
        assert_eq!(data_size_for::<CuToReSegment>(), PAGE_SIZE);
    }

    #[test]
    fn version_hashes_are_unique_for_different_sizes() {
        // struct_version_hash uses only size+alignment, so types with
        // identical layout (RpcToCuSegment ≈ CuToReSegment, both 88B/64-align)
        // will collide. The P2P header's source_module/dest_module fields
        // provide additional disambiguation.
        let hashes = [
            ("HalToCu", struct_version_hash::<HalToCuSegment>()),
            ("CuToHal", struct_version_hash::<CuToHalSegment>()),
            ("ReToCu", struct_version_hash::<ReToCuSegment>()),
            ("CuToMqt", struct_version_hash::<CuToMqtSegment>()),
        ];
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(
                    hashes[i].1, hashes[j].1,
                    "hash collision between {} and {}",
                    hashes[i].0, hashes[j].0
                );
            }
        }
    }

    #[test]
    fn segment_thresholds_defaults() {
        let t = SegmentThresholds::default();
        assert_eq!(t.hal_stale, 3);
        assert_eq!(t.re_stale, 1000);
        assert_eq!(t.rpc_stale, 1000);
    }

    #[test]
    fn p2p_header_fits_in_all_segments() {
        let hdr_size = core::mem::size_of::<P2pSegmentHeader>();
        assert!(hdr_size <= core::mem::size_of::<HalToCuSegment>());
        assert!(hdr_size <= core::mem::size_of::<CuToHalSegment>());
        assert!(hdr_size <= core::mem::size_of::<ReToCuSegment>());
        assert!(hdr_size <= core::mem::size_of::<CuToMqtSegment>());
        assert!(hdr_size <= core::mem::size_of::<RpcToCuSegment>());
        assert!(hdr_size <= core::mem::size_of::<CuToReSegment>());
    }
}
