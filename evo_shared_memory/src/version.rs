//! Atomic version counter for optimistic concurrency control

use std::sync::atomic::{AtomicU64, Ordering};

/// Version counter using even/odd optimistic versioning
///
/// Writers increment the version on each update, readers validate
/// version before and after reads to detect concurrent modifications.
#[derive(Debug)]
pub struct VersionCounter {
    counter: AtomicU64,
}

impl VersionCounter {
    /// Create a new version counter starting at 0 (even)
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }

    /// Create version counter from existing value
    pub fn from_raw(value: u64) -> Self {
        Self {
            counter: AtomicU64::new(value),
        }
    }

    /// Get current version with acquire ordering
    pub fn load(&self) -> u64 {
        self.counter.load(Ordering::Acquire)
    }

    /// Set version with release ordering (used by writers)
    pub fn store(&self, value: u64) {
        self.counter.store(value, Ordering::Release);
    }

    /// Begin write operation - increment to odd version
    pub fn begin_write(&self) -> u64 {
        let current = self.counter.load(Ordering::Acquire);
        let next = current + 1;
        self.counter.store(next, Ordering::Release);
        next
    }

    /// Complete write operation - increment to even version
    pub fn end_write(&self) -> u64 {
        let current = self.counter.load(Ordering::Acquire);
        let next = current + 1;
        self.counter.store(next, Ordering::Release);
        next
    }

    /// Check if version is stable (even)
    pub fn is_stable(version: u64) -> bool {
        version % 2 == 0
    }

    /// Check if version indicates write in progress (odd)
    pub fn is_writing(version: u64) -> bool {
        version % 2 == 1
    }
}

impl Default for VersionCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_counter_creation() {
        let counter = VersionCounter::new();
        assert_eq!(counter.load(), 0);
        assert!(VersionCounter::is_stable(counter.load()));
    }

    #[test]
    fn test_write_cycle() {
        let counter = VersionCounter::new();

        // Begin write - should be odd
        let v1 = counter.begin_write();
        assert_eq!(v1, 1);
        assert!(VersionCounter::is_writing(v1));

        // End write - should be even
        let v2 = counter.end_write();
        assert_eq!(v2, 2);
        assert!(VersionCounter::is_stable(v2));
    }

    #[test]
    fn test_version_validation() {
        assert!(VersionCounter::is_stable(0));
        assert!(VersionCounter::is_stable(2));
        assert!(VersionCounter::is_stable(100));

        assert!(VersionCounter::is_writing(1));
        assert!(VersionCounter::is_writing(3));
        assert!(VersionCounter::is_writing(99));
    }
}
