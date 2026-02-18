//! # EVO Recipe Executor
//!
//! Executes CNC/motion recipes by writing commands to CU, HAL, and gRPC,
//! and publishing status to MQTT. Reads acknowledgments from CU, HAL,
//! and gRPC.
//!
//! # SHM Segments
//!
//! **Writers** (RE → others):
//!
//! | Segment       | Type            | Destination |
//! |---------------|-----------------|-------------|
//! | `evo_re_cu`   | ReToCuSegment   | CU          |
//! | `evo_re_hal`  | ReToHalSegment  | HAL         |
//! | `evo_re_mqt`  | ReToMqtSegment  | MQT         |
//! | `evo_re_rpc`  | ReToRpcSegment  | gRPC        |
//!
//! **Readers** (others → RE):
//!
//! | Segment       | Type            | Source |
//! |---------------|-----------------|--------|
//! | `evo_cu_re`   | CuToReSegment   | CU     |
//! | `evo_hal_re`  | HalToReSegment  | HAL    |
//! | `evo_rpc_re`  | RpcToReSegment  | gRPC   |

use evo_common::shm::p2p::{ModuleAbbrev, TypedP2pReader, TypedP2pWriter};
use evo_common::shm::segments::{
    CuToReSegment, HalToReSegment, RpcToReSegment,
    ReToCuSegment, ReToHalSegment, ReToMqtSegment, ReToRpcSegment,
    SEG_CU_RE, SEG_HAL_RE, SEG_RPC_RE,
    SEG_RE_CU, SEG_RE_HAL, SEG_RE_MQT, SEG_RE_RPC,
};
use tracing::{debug, info};

fn main() {
    tracing_subscriber::fmt().compact().init();
    info!("EVO Recipe Executor starting...");

    // ── Writers: RE → others ────────────────────────────────────────
    let writer_re_cu = try_create_writer::<ReToCuSegment>(
        SEG_RE_CU, ModuleAbbrev::Re, ModuleAbbrev::Cu,
    );
    let writer_re_hal = try_create_writer::<ReToHalSegment>(
        SEG_RE_HAL, ModuleAbbrev::Re, ModuleAbbrev::Hal,
    );
    let writer_re_mqt = try_create_writer::<ReToMqtSegment>(
        SEG_RE_MQT, ModuleAbbrev::Re, ModuleAbbrev::Mqt,
    );
    let writer_re_rpc = try_create_writer::<ReToRpcSegment>(
        SEG_RE_RPC, ModuleAbbrev::Re, ModuleAbbrev::Rpc,
    );

    info!(
        "RE writers: re_cu={}, re_hal={}, re_mqt={}, re_rpc={}",
        status(&writer_re_cu), status(&writer_re_hal),
        status(&writer_re_mqt), status(&writer_re_rpc),
    );

    // ── Readers: others → RE ────────────────────────────────────────
    let stale_threshold: u32 = 1000;
    let reader_cu_re = try_attach::<CuToReSegment>(SEG_CU_RE, stale_threshold);
    let reader_hal_re = try_attach::<HalToReSegment>(SEG_HAL_RE, stale_threshold);
    let reader_rpc_re = try_attach::<RpcToReSegment>(SEG_RPC_RE, stale_threshold);

    info!(
        "RE readers: cu_re={}, hal_re={}, rpc_re={}",
        status(&reader_cu_re), status(&reader_hal_re), status(&reader_rpc_re),
    );

    // Placeholder: in full implementation this would enter a recipe execution loop.
    info!("Recipe Executor initialized — placeholder (not yet implemented)");
}

fn try_create_writer<T: Default + Copy>(
    seg_name: &str,
    src: ModuleAbbrev,
    dst: ModuleAbbrev,
) -> Option<TypedP2pWriter<T>> {
    match TypedP2pWriter::<T>::create(seg_name, src, dst) {
        Ok(w) => {
            info!("Created writer: evo_{seg_name}");
            Some(w)
        }
        Err(e) => {
            debug!("Could not create evo_{seg_name}: {e}");
            None
        }
    }
}

fn try_attach<T: Default + Copy>(
    seg_name: &str,
    stale_threshold: u32,
) -> Option<TypedP2pReader<T>> {
    match TypedP2pReader::<T>::attach(seg_name, stale_threshold) {
        Ok(r) => {
            info!("Attached reader: evo_{seg_name}");
            Some(r)
        }
        Err(e) => {
            debug!("Could not attach evo_{seg_name} (will retry later): {e}");
            None
        }
    }
}

fn status<T>(opt: &Option<T>) -> &'static str {
    if opt.is_some() { "ok" } else { "pending" }
}
