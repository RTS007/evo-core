//! # EVO REST API Gateway
//!
//! Exposes a REST/HTTP API for external clients and web UIs. This module
//! does NOT use SHM directly — it communicates via:
//!
//! - **gRPC client** → `evo_grpc` for real-time commands and status
//! - **MQTT subscriber** → `evo_mqtt` for event streams
//!
//! # Architecture
//!
//! ```text
//! External clients ──HTTP──► evo_api ──gRPC──► evo_grpc ──SHM──► RT domain
//!                                    └─MQTT─► evo_mqtt ◄─SHM─── RT domain
//! ```

use tracing::info;

fn main() {
    tracing_subscriber::fmt().compact().init();
    info!("EVO REST API Gateway starting...");

    // No SHM segments — communicates via gRPC + MQTT.
    // Placeholder: in full implementation this would start an HTTP server
    // (e.g., axum or actix-web) with routes for status, commands, config.

    info!("API Gateway initialized — placeholder (not yet implemented)");
}
