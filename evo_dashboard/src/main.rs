//! # EVO Dashboard Service
//!
//! Serves the operator dashboard UI (web-based). This module does NOT use
//! SHM directly — it communicates via:
//!
//! - **gRPC client** → `evo_grpc` for real-time status and control
//! - **MQTT subscriber** → `evo_mqtt` for live data streams
//!
//! # Architecture
//!
//! ```text
//! Operator Browser ──WebSocket──► evo_dashboard ──gRPC──► evo_grpc
//!                                               └─MQTT─► evo_mqtt
//! ```

use tracing::info;

fn main() {
    tracing_subscriber::fmt().compact().init();
    info!("EVO Dashboard Service starting...");

    // No SHM segments — communicates via gRPC + MQTT.
    // Placeholder: in full implementation this would start a web server
    // serving static assets and WebSocket endpoints for live data.

    info!("Dashboard Service initialized — placeholder (not yet implemented)");
}
