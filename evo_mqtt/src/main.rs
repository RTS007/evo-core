//! # EVO MQTT Bridge
//!
//! Reads status snapshots from HAL, CU, and RE via P2P SHM segments
//! and publishes them over MQTT. This module is read-only — it never
//! writes SHM segments.
//!
//! # SHM Segments (all readers)
//!
//! | Segment       | Type            | Source |
//! |---------------|-----------------|--------|
//! | `evo_cu_mqt`  | CuToMqtSegment  | CU     |
//! | `evo_hal_mqt` | HalToMqtSegment | HAL    |
//! | `evo_re_mqt`  | ReToMqtSegment  | RE     |

use evo_common::shm::p2p::TypedP2pReader;
use evo_common::shm::segments::{
    CuToMqtSegment, HalToMqtSegment, ReToMqtSegment,
    SEG_CU_MQT, SEG_HAL_MQT, SEG_RE_MQT,
};
use tracing::{debug, info};

fn main() {
    tracing_subscriber::fmt().compact().init();
    info!("EVO MQTT Bridge starting...");

    // Attach readers — non-fatal if segments don't exist yet.
    let stale_threshold: u32 = 100; // cycles before marking stale

    let reader_cu_mqt = try_attach::<CuToMqtSegment>(SEG_CU_MQT, stale_threshold);
    let reader_hal_mqt = try_attach::<HalToMqtSegment>(SEG_HAL_MQT, stale_threshold);
    let reader_re_mqt = try_attach::<ReToMqtSegment>(SEG_RE_MQT, stale_threshold);

    info!(
        "MQTT readers: cu_mqt={}, hal_mqt={}, re_mqt={}",
        if reader_cu_mqt.is_some() { "attached" } else { "pending" },
        if reader_hal_mqt.is_some() { "attached" } else { "pending" },
        if reader_re_mqt.is_some() { "attached" } else { "pending" },
    );

    // Placeholder: in full implementation this would enter a publish loop.
    info!("MQTT Bridge initialized — placeholder read loop (not yet implemented)");
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
