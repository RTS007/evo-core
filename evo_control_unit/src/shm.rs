//! SHM integration root.
//!
//! P2P segment connection, lifecycle management, inbound reading,
//! and outbound writing with heartbeat and lock-free protocol.

pub mod reader;
pub mod segments;
pub mod writer;
