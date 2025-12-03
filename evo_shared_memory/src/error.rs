//! Error types for shared memory operations

use thiserror::Error;

/// Errors that can occur during shared memory operations
#[derive(Error, Debug)]
pub enum ShmError {
    /// Segment already exists
    #[error("Segment already exists: {name}")]
    AlreadyExists {
        /// Segment name
        name: String,
    },

    /// Segment not found
    #[error("Segment not found: {name}")]
    NotFound {
        /// Segment name
        name: String,
    },

    /// Invalid segment size
    #[error("Invalid segment size: {size} bytes (must be 4KB-1GB, page-aligned)")]
    InvalidSize {
        /// Attempted size in bytes
        size: usize,
    },

    /// Version conflict detected during read
    #[error("Version conflict detected - retry recommended")]
    VersionConflict,

    /// Permission denied
    #[error("Permission denied accessing segment: {name}")]
    PermissionDenied {
        /// Segment name
        name: String,
    },

    /// System resources exhausted
    #[error("System resource exhausted - cleanup required")]
    ResourceExhausted,

    /// Real-time deadline violated
    #[error("Real-time deadline violated: {operation}")]
    DeadlineViolation {
        /// Operation that violated deadline
        operation: String,
    },

    /// Memory alignment error
    #[error("Memory alignment error: address {address:#x} not aligned to {alignment}")]
    AlignmentError {
        /// Memory address
        address: usize,
        /// Required alignment
        alignment: usize,
    },

    /// Process not found or already dead
    #[error("Process not found: {pid}")]
    ProcessNotFound {
        /// Process ID
        pid: u32,
    },

    /// IO error
    #[error("IO error: {source}")]
    Io {
        /// Source IO error
        #[from]
        source: std::io::Error,
    },

    /// Nix system call error
    #[error("System call error: {source}")]
    Nix {
        /// Source nix error
        #[from]
        source: nix::Error,
    },

    /// JSON serialization/deserialization error
    #[error("JSON error: {source}")]
    Json {
        /// Source JSON error
        #[from]
        source: serde_json::Error,
    },
}

/// Result type for shared memory operations
pub type ShmResult<T> = Result<T, ShmError>;
