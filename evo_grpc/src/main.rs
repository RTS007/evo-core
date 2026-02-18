//! # EVO gRPC API Liaison Service
//!
//! Bridges external gRPC clients to the real-time system via P2P SHM.
//! Creates writer segments for commands flowing into the RT domain and
//! attaches reader segments for status flowing out.
//!
//! # SHM Segments
//!
//! **Writers** (gRPC → RT):
//!
//! | Segment       | Type            | Destination |
//! |---------------|-----------------|-------------|
//! | `evo_rpc_cu`  | RpcToCuSegment  | CU          |
//! | `evo_rpc_hal` | RpcToHalSegment | HAL         |
//! | `evo_rpc_re`  | RpcToReSegment  | RE          |
//!
//! **Readers** (RT → gRPC):
//!
//! | Segment       | Type            | Source |
//! |---------------|-----------------|--------|
//! | `evo_cu_rpc`  | CuToRpcSegment  | CU     |
//! | `evo_hal_rpc` | HalToRpcSegment | HAL    |
//! | `evo_re_rpc`  | ReToRpcSegment  | RE     |

use evo_common::shm::p2p::{ModuleAbbrev, TypedP2pReader, TypedP2pWriter};
use evo_common::shm::segments::{
    CuToRpcSegment, HalToRpcSegment, ReToRpcSegment,
    RpcToCuSegment, RpcToHalSegment, RpcToReSegment,
    SEG_CU_RPC, SEG_HAL_RPC, SEG_RE_RPC,
    SEG_RPC_CU, SEG_RPC_HAL, SEG_RPC_RE,
};
use tracing::{debug, info};

fn main() {
    tracing_subscriber::fmt().compact().init();
    info!("EVO gRPC Liaison starting...");

    // ── Writers: gRPC → RT ──────────────────────────────────────────
    let writer_rpc_cu = try_create_writer::<RpcToCuSegment>(
        SEG_RPC_CU, ModuleAbbrev::Rpc, ModuleAbbrev::Cu,
    );
    let writer_rpc_hal = try_create_writer::<RpcToHalSegment>(
        SEG_RPC_HAL, ModuleAbbrev::Rpc, ModuleAbbrev::Hal,
    );
    let writer_rpc_re = try_create_writer::<RpcToReSegment>(
        SEG_RPC_RE, ModuleAbbrev::Rpc, ModuleAbbrev::Re,
    );

    info!(
        "gRPC writers: rpc_cu={}, rpc_hal={}, rpc_re={}",
        status(&writer_rpc_cu), status(&writer_rpc_hal), status(&writer_rpc_re),
    );

    // ── Readers: RT → gRPC ──────────────────────────────────────────
    let stale_threshold: u32 = 1000;
    let reader_cu_rpc = try_attach::<CuToRpcSegment>(SEG_CU_RPC, stale_threshold);
    let reader_hal_rpc = try_attach::<HalToRpcSegment>(SEG_HAL_RPC, stale_threshold);
    let reader_re_rpc = try_attach::<ReToRpcSegment>(SEG_RE_RPC, stale_threshold);

    info!(
        "gRPC readers: cu_rpc={}, hal_rpc={}, re_rpc={}",
        status(&reader_cu_rpc), status(&reader_hal_rpc), status(&reader_re_rpc),
    );

    // Placeholder: in full implementation this would start a tonic gRPC server.
    info!("gRPC Liaison initialized — placeholder (not yet implemented)");
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
