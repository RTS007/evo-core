//! P2P segment connection and lifecycle management.
//!
//! Creates outbound writer segments (CU→HAL, CU→MQT, CU→RE) and
//! attaches inbound reader segments (HAL→CU, RE→CU, RPC→CU) using
//! `TypedP2pWriter` / `TypedP2pReader` from `evo_common::shm::p2p`.

use evo_common::shm::p2p::{ModuleAbbrev, ShmError, TypedP2pReader, TypedP2pWriter};
use evo_common::shm::segments::{
    CuToHalSegment, CuToMqtSegment, CuToReSegment, HalToCuSegment, ReToCuSegment,
    RpcToCuSegment, SEG_CU_HAL, SEG_CU_MQT, SEG_CU_RE, SEG_HAL_CU, SEG_RE_CU, SEG_RPC_CU,
};

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
    pub cu_to_hal: TypedP2pWriter<CuToHalSegment>,
    /// CU → MQT: diagnostic state snapshot.
    pub cu_to_mqt: TypedP2pWriter<CuToMqtSegment>,
    /// CU → RE: recipe acknowledgements.
    pub cu_to_re: TypedP2pWriter<CuToReSegment>,

    // ── Inbound (CU reads) ──
    /// HAL → CU: axis feedback, digital/analog inputs. **Required.**
    pub hal_to_cu: TypedP2pReader<HalToCuSegment>,
    /// RE → CU: recipe commands. **Optional** (RE may start later).
    pub re_to_cu: Option<TypedP2pReader<ReToCuSegment>>,
    /// RPC → CU: API commands. **Optional** (API may start later).
    pub rpc_to_cu: Option<TypedP2pReader<RpcToCuSegment>>,
}

impl CuSegments {
    /// Create all outbound segments and attach all inbound segments.
    ///
    /// # Errors
    /// - HAL→CU is required; failure to attach is a fatal error.
    /// - RE→CU and RPC→CU are optional; `SegmentNotFound` is not an error.
    pub fn init(thresholds: &SegmentThresholds) -> Result<Self, ShmError> {
        // ── Create outbound writer segments ──
        let cu_to_hal = TypedP2pWriter::<CuToHalSegment>::create(
            SEG_CU_HAL,
            ModuleAbbrev::Cu,
            ModuleAbbrev::Hal,
        )?;
        let cu_to_mqt = TypedP2pWriter::<CuToMqtSegment>::create(
            SEG_CU_MQT,
            ModuleAbbrev::Cu,
            ModuleAbbrev::Mqt,
        )?;
        let cu_to_re = TypedP2pWriter::<CuToReSegment>::create(
            SEG_CU_RE,
            ModuleAbbrev::Cu,
            ModuleAbbrev::Re,
        )?;

        // ── Attach inbound reader segments ──
        let hal_to_cu =
            TypedP2pReader::<HalToCuSegment>::attach(SEG_HAL_CU, thresholds.hal_stale)?;

        // RE and RPC are optional — they may not be running yet.
        let re_to_cu =
            match TypedP2pReader::<ReToCuSegment>::attach(SEG_RE_CU, thresholds.re_stale) {
                Ok(r) => Some(r),
                Err(ShmError::SegmentNotFound { .. }) => None,
                Err(e) => return Err(e),
            };

        let rpc_to_cu =
            match TypedP2pReader::<RpcToCuSegment>::attach(SEG_RPC_CU, thresholds.rpc_stale) {
                Ok(r) => Some(r),
                Err(ShmError::SegmentNotFound { .. }) => None,
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
    pub fn try_attach_re(&mut self, stale_threshold: u32) -> Result<bool, ShmError> {
        if self.re_to_cu.is_some() {
            return Ok(true);
        }
        match TypedP2pReader::<ReToCuSegment>::attach(SEG_RE_CU, stale_threshold) {
            Ok(r) => {
                self.re_to_cu = Some(r);
                Ok(true)
            }
            Err(ShmError::SegmentNotFound { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Attempt to late-attach the RPC→CU segment (if API started after CU).
    pub fn try_attach_rpc(&mut self, stale_threshold: u32) -> Result<bool, ShmError> {
        if self.rpc_to_cu.is_some() {
            return Ok(true);
        }
        match TypedP2pReader::<RpcToCuSegment>::attach(SEG_RPC_CU, stale_threshold) {
            Ok(r) => {
                self.rpc_to_cu = Some(r);
                Ok(true)
            }
            Err(ShmError::SegmentNotFound { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_thresholds_defaults() {
        let t = SegmentThresholds::default();
        assert_eq!(t.hal_stale, 3);
        assert_eq!(t.re_stale, 1000);
        assert_eq!(t.rpc_stale, 1000);
    }
}
