//! SHM (Shared Memory) constants.
//!
//! These constants define the fundamental parameters for the EVO shared memory system.
//! They are the single source of truth - all other crates should import from here.
//!
//! Note: The P2P magic constant lives in `evo_common::shm::p2p::EVO_P2P_MAGIC`.
//! The P2P protocol is the sole SHM transport.

/// Minimum shared memory segment size in bytes.
///
/// Set to 4KB (one memory page) as the smallest practical segment size.
/// Segments smaller than this would have excessive overhead.
pub const SHM_MIN_SIZE: usize = 4096;

/// Maximum shared memory segment size in bytes.
///
/// Set to 1GB as a reasonable upper limit to prevent excessive memory usage.
/// This limit can be increased if needed for specific use cases.
pub const SHM_MAX_SIZE: usize = 1_073_741_824; // 1GB

/// CPU cache line size in bytes.
///
/// Used for memory alignment to prevent false sharing between threads.
/// 64 bytes is the standard cache line size on most modern x86-64 processors.
pub const CACHE_LINE_SIZE: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shm_min_size_is_page_size() {
        assert_eq!(SHM_MIN_SIZE, 4096);
    }

    #[test]
    fn test_shm_max_size_is_1gb() {
        assert_eq!(SHM_MAX_SIZE, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_cache_line_size() {
        assert_eq!(CACHE_LINE_SIZE, 64);
    }

    #[test]
    fn test_size_constraints() {
        assert!(SHM_MIN_SIZE < SHM_MAX_SIZE);
        assert!(SHM_MIN_SIZE > 0);
    }
}
