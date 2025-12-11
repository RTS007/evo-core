# Implementation Plan: Shared Memory Lifecycle

**Branch**: `002-shm-lifecycle` | **Date**: 28 November 2025 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/002-shm-lifecycle/spec.md`

**Note**: This plan implements the shared memory lifecycle with single-writer segments and lock-free reads for the EVO real-time industrial control system.

## Summary

Implement the foundational shared memory lifecycle management system for EVO's real-time architecture. This feature enables single-writer, multi-reader segments with lock-free access patterns, filesystem-based discovery via SegmentDiscovery, optimistic versioning for conflict detection, and pidfd-based cleanup of orphaned segments. The implementation serves as the backbone for high-performance inter-process communication between EVO's RT modules (Control Unit, HAL Core, Recipe Executor, API Liaison) through SegmentWriter/SegmentReader interfaces while maintaining sub-microsecond latency and deterministic behavior.

## Technical Context

**Language/Version**: Rust 1.75+  
**Primary Dependencies**: libc, memmap2, serde, nix, tracing, thiserror  
**Storage**: Memory-mapped files in /dev/shm with filesystem-based discovery  
**Testing**: cargo test with criterion for performance benchmarks, proptest for property testing  
**Target Platform**: Linux x86_64/ARM64 with PREEMPT_RT kernel support
**Project Type**: Embedded real-time library (part of evo_shared_memory crate)  
**Constraints**: <0.01% deadline miss rate (Class A Critical), pidfd-based orphan detection (RT mode) with periodic fallback (simulation), page-aligned memory  
**Scale/Scope**: Support 10+ concurrent readers per segment (minimum), 100 readers (production target), up to 1000 readers (stress test maximum), 4KB-1GB segment sizes, filesystem-based discovery

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

✅ **Principle I (Soft Real-Time Performance)**: Timing contracts defined - Class A Critical deadline <1ms for write operations, Class B Important for read operations <100µs, explicit miss rate budgets documented

✅ **Principle II (Test-First)**: Full TDD approach with multi-layer testing strategy including timing validation, property testing for lock-free algorithms, and stress testing for deadline miss rates  

✅ **Principle V (Performance Bounds)**: Explicit resource budgets - memory mapped files with page alignment, CPU budget <5% overhead, deterministic allocation patterns

✅ **Principle XI (Specification-Driven)**: Formal specification with machine-readable contracts, traceable lineage from user requirements to implementation

✅ **Principle XVII (Modular Library-First)**: Standalone evo_shared_memory crate with minimal dependencies, clear isolation between RT and non-RT functionality

✅ **Principle XVIII (Deterministic Interface)**: Bounded execution time guarantees, programmatic interfaces with documented timing characteristics, non-blocking diagnostic access

✅ **Principle XIX (Non-RT Isolation)**: Clear separation - SHM Inspector runs in separate non-RT process, no shared resources with RT threads

✅ **Principle VI (Observability & Traceability)**: Structured logging with timing-safe tracepoints, <2% CPU overhead budget for RT threads

## Project Structure

### Documentation (this feature)

```text
specs/002-shm-lifecycle/
├── plan.md              # This file
├── research.md          # Phase 0 output - lock-free algorithms, memory mapping strategies  
├── data-model.md        # Phase 1 output - segment metadata, version counters, discovery format
├── quickstart.md        # Phase 1 output - developer guide for SHM usage
├── contracts/           # Phase 1 output - SHM segment schema, discovery protocol
└── tasks.md             # Phase 2 output (created by /speckit.tasks)
```

### Source Code (repository root)

```text
evo_shared_memory/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # Public API, re-exports
│   ├── segment.rs                # SharedMemorySegment core implementation  
│   ├── writer.rs                 # Single writer implementation with optimistic versioning
│   ├── reader.rs                 # Lock-free reader implementation
│   ├── discovery.rs              # Filesystem-based segment discovery (/dev/shm)
│   ├── lifecycle.rs              # Segment creation, cleanup, orphan detection
│   ├── version.rs                # Even/odd versioning conflict detection
│   ├── error.rs                  # Error types and handling
│   └── platform/
│       ├── linux.rs              # Linux-specific memory mapping, process detection
│       └── mod.rs                # Platform abstraction
├── tests/
│   ├── unit/
│   │   ├── segment_tests.rs      # Basic segment operations
│   │   ├── writer_tests.rs       # Single writer semantics  
│   │   ├── reader_tests.rs       # Lock-free reader validation
│   │   └── discovery_tests.rs    # Filesystem discovery
│   ├── integration/
│   │   ├── multi_process.rs      # Cross-process validation
│   │   ├── stress_test.rs        # Concurrent readers stress testing
│   │   └── cleanup_test.rs       # Orphan detection and cleanup
│   ├── timing/
│   │   ├── latency_bench.rs      # Sub-microsecond latency validation
│   │   ├── deadline_test.rs      # Class A/B deadline miss rate validation
│   │   └── overhead_bench.rs     # <5% overhead vs native pointers
│   └── property/
│       ├── lock_free_props.rs    # Property tests for lock-free algorithms
│       └── versioning_props.rs   # Optimistic versioning correctness
├── benches/
│   ├── read_write_perf.rs        # Performance benchmarks
│   └── concurrent_access.rs      # Scalability benchmarks  
└── examples/
    ├── simple_writer.rs          # Basic usage examples
    ├── simple_reader.rs
    └── discovery_example.rs

# Integration with existing EVO crates
evo/src/main.rs                   # EVO supervisor - SHM lifecycle management
evo_control_unit/src/
├── shm_integration.rs            # Control unit SHM usage (EXCLUSIVE writer for control_state)
evo_hal/src/ 
├── shm_integration.rs            # HAL SHM usage (EXCLUSIVE writer for phys input-IO)
evo_recipe_executor/src/
├── shm_integration.rs            # Recipe executor SHM usage (EXCLUSIVE writer for phys output-IO)
evo_grpc/src/
├── shm_integration.rs            # API Liaison SHM usage (EXCLUSIVE writer for commands/config)
```

**Structure Decision**: Standalone library crate (evo_shared_memory) following EVO's modular architecture with clear RT/non-RT separation. Integration points in existing EVO modules maintain single-writer ownership model as defined in the architecture documentation.

## Complexity Tracking

*No constitutional violations - implementation follows established patterns*

## Phase 0: Research & Dependencies

### Research Topics
1. **Lock-Free Algorithm Validation**
   - Memory ordering semantics for optimistic even/odd versioning (Acquire-Release ordering)
   - Sequential consistency guarantees: readers validate version before/after reads, retry on mismatch
   - Cache coherency considerations for x86_64/ARM64 (64-byte cache line alignment)
   - ABA problem prevention: 64-bit version counter with epoch bits
   - Atomic operations: `AtomicU64::load(Ordering::Acquire)`, `AtomicU64::store(Ordering::Release)`

2. **Memory Layout & Data Structures**
   - **Segment Header** (128 bytes, cache-line aligned):
     ```rust
     struct SegmentHeader {
         magic: u64,           // 0x45564F5F53484D00 ("EVO_SHM\0")
         version: AtomicU64,   // Even/odd version counter
         writer_pid: AtomicU32,// Writer process ID
         reader_count: AtomicU32, // Active reader count
         size: u64,            // Data section size
         created_ts: u64,      // Creation timestamp (monotonic)
         last_write_ts: AtomicU64, // Last write timestamp
         checksum: AtomicU32,  // Header checksum
         _padding: [u8; 64],   // Cache line padding
     }
     ```
   - **Data Section**: Immediately follows header, size specified in header
   - **Memory Alignment**: 4KB page-aligned segments, 64-byte cache-line aligned header
   - **Size Validation**: 
     - Minimum: 4KB (4,096 bytes) - one memory page
     - Maximum: 1GB (1,073,741,824 bytes) - practical limit
     - Alignment: Must be multiple of page size (4KB on most systems)
     - Validation logic: `(size >= 4096) && (size <= 1024*1024*1024) && (size % 4096 == 0)`

3. **Process Detection & Cleanup**
   - **RT Mode - Linux pidfd mechanism**: Use `pidfd_open()` + `epoll` for process death notification
   - **Simulation Mode - Periodic validation**: Use `kill(pid, 0)` checks every 5 seconds for cross-platform compatibility
   - **Heartbeat Integration**: 5-second timeout with EVO supervisor coordination
   - **Cleanup Triggers**: 
     - RT Mode: Immediate pidfd death notification
     - Simulation Mode: Periodic 5-second PID validation scan
     - Periodic: 30-second orphan scan by EVO supervisor
     - Manual: explicit cleanup API call
   - **Grace Periods**: 10-second grace period before force cleanup

4. **Discovery Protocol Specifics**
   - **Naming Convention**: `/dev/shm/evo_{module}_{name}_{pid}` (collision-free)
   - **Metadata Files**: `/dev/shm/evo_{name}.meta` contains JSON metadata
   - **Atomic Operations**: 
     - Create: `O_CREAT | O_EXCL` for atomicity
     - Delete: Unlink metadata first, then segment file
   - **Permissions**: 0600 (owner read/write only) for security

5. **Memory Mapping Best Practices**
   - Use `MAP_SHARED | MAP_LOCKED` for RT performance
   - `madvise(MADV_DONTFORK)` to prevent copy-on-write in child processes
   - NUMA-aware allocation: bind to local NUMA node using `mbind()`
   - Huge pages for segments >2MB: use `MAP_HUGETLB` flag

### Dependency Analysis
- **libc**: 0.2.150+ for POSIX memory mapping primitives and pidfd support
- **memmap2**: 0.9+ for safe Rust memory mapping with MAP_LOCKED support
- **serde**: 1.0.190+ with derive feature for segment metadata serialization
- **nix**: 0.27+ for Unix-specific system calls (pidfd_open, madvise, mbind)
- **tracing**: 0.1.40+ for RT-safe logging with lock-free ring buffers
- **thiserror**: 1.0.50+ for zero-cost error handling abstractions
- **criterion**: 0.5+ for performance benchmarks (dev-dependency)
- **proptest**: 1.4+ for property-based testing (dev-dependency)
- **parking_lot**: 0.12+ for RT-friendly synchronization primitives

## Phase 1: Design & Contracts

### Core Data Model
```rust
// Core segment representation
pub struct SharedMemorySegment {
    name: String,
    size: usize,
    header: *mut SegmentHeader,
    data: *mut u8,
    mmap: MmapMut,
}

// Exclusive writer with version management
pub struct SegmentWriter {
    segment: SharedMemorySegment,
    current_version: u64,
    write_buffer: Vec<u8>, // Optional double-buffering
}

// Lock-free reader with conflict detection
pub struct SegmentReader {
    segment: SharedMemorySegment,
    last_seen_version: u64,
    read_buffer: Vec<u8>, // Copy buffer for consistency
}

// Discovery service
pub struct SegmentDiscovery {
    watch_fd: RawFd, // inotify fd for /dev/shm
    known_segments: HashMap<String, SegmentInfo>,
}

// Segment metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SegmentInfo {
    pub name: String,
    pub size: usize,
    pub writer_pid: u32,
    pub created_at: SystemTime,
    pub last_accessed: SystemTime,
    pub reader_count: u32,
}
```

### API Contracts
```rust
// Segment lifecycle
pub fn create_segment(name: &str, size: usize) -> Result<SegmentWriter, ShmError>;
pub fn attach_reader(name: &str) -> Result<SegmentReader, ShmError>;
pub fn list_segments() -> Result<Vec<SegmentInfo>, ShmError>;
pub fn cleanup_orphaned_segments() -> Result<usize, ShmError>;

// Writer operations
impl SegmentWriter {
    pub fn write(&mut self, data: &[u8]) -> Result<(), ShmError>; // <1ms deadline
    pub fn write_at(&mut self, offset: usize, data: &[u8]) -> Result<(), ShmError>;
    pub fn flush(&mut self) -> Result<(), ShmError>; // Memory barrier
    pub fn current_version(&self) -> u64;
}

// Reader operations  
impl SegmentReader {
    pub fn read(&mut self) -> Result<&[u8], ShmError>; // <100µs deadline
    pub fn read_range(&mut self, offset: usize, len: usize) -> Result<&[u8], ShmError>;
    pub fn version(&self) -> u64;
    pub fn has_changed(&self) -> bool;
}

// Error taxonomy with constitution-compliant structured error handling
#[derive(thiserror::Error, Debug)]
pub enum ShmError {
    #[error("Segment already exists: {name}")]
    AlreadyExists { name: String },
    #[error("Segment not found: {name}")]
    NotFound { name: String },
    #[error("Invalid segment size: {size} bytes (must be 4KB-1GB, page-aligned)")]
    InvalidSize { size: usize },
    #[error("Version conflict detected - retry recommended")]
    VersionConflict,
    #[error("Permission denied accessing segment: {name}")]
    PermissionDenied { name: String },
    #[error("System resource exhausted - cleanup required")]
    ResourceExhausted,
    #[error("Real-time deadline violated: {operation}")]
    DeadlineViolation { operation: String },
    #[error("IO error: {source}")]
    Io { #[from] source: std::io::Error },
}
```

### Performance Contracts
- **Write Operations**: 
  - <1ms deadline (Class A Critical) for 4KB writes with P95 < 1μs typical latency
  - <500ns typical latency for cache-resident data (sub-microsecond target)
  - Memory barriers: `std::sync::atomic::fence(Ordering::Release)`
- **Read Operations**: 
  - <100µs deadline (Class B Important) for 4KB reads  
  - <50µs typical latency for version validation
  - Lock-free: No blocking synchronization primitives
- **Memory Overhead**: <5% vs native pointer access
  - Header: 128 bytes fixed overhead per segment
  - Metadata: <1KB per segment in discovery cache
- **Scalability**: Tiered reader scaling without performance degradation
  - Baseline: 10 concurrent readers (development minimum)
  - Production: 100-1000 concurrent readers (linear scaling target)
  - Stress: Up to 10,000 concurrent readers (maximum capacity test)
  - Linear read performance scaling up to CPU core count
  - No reader registration overhead

### Integration Contracts
```rust
// EVO supervisor interface
pub trait ShmLifecycleManager {
    fn initialize_shm_subsystem() -> Result<(), ShmError>;
    fn register_cleanup_handler(handler: Box<dyn Fn() + Send>);
    fn periodic_cleanup(&self) -> Result<usize, ShmError>;
    fn emergency_cleanup(&self) -> Result<(), ShmError>;
}

// RT module integration pattern
pub trait ShmUser {
    fn segment_name(&self) -> &str;
    fn segment_size(&self) -> usize;
    fn initialize_shm(&mut self) -> Result<(), ShmError>;
    fn shutdown_shm(&mut self) -> Result<(), ShmError>;
}

// Module-specific segment ownership
const CONTROL_UNIT_SEGMENT: &str = "control_state";  // Exclusive writer
const HAL_INPUT_SEGMENT: &str = "phys_input_io";     // Exclusive writer
const RECIPE_OUTPUT_SEGMENT: &str = "phys_output_io"; // Exclusive writer
const API_COMMAND_SEGMENT: &str = "commands_config";  // Exclusive writer
```

- **Process Supervision**: EVO supervisor manages SHM lifecycle across system restarts
- **Ownership Model**: Each RT module maintains exclusive writer ownership per segment type
- **Non-RT Isolation**: Non-RT modules access SHM through dedicated reader processes only
- **Cleanup Integration**: Automatic cleanup integrated with EVO's process supervision architecture

## Phase 2: Implementation Strategy

Implementation follows TDD with timing validation at each step:

### Sprint 1: Foundation (Week 1-2)
1. **Memory Layout Implementation**
   - SegmentHeader struct with cache-line alignment
   - Page-aligned memory mapping with MAP_LOCKED
   - Version counter with atomic operations
2. **Single Writer Core**
   - Exclusive segment creation with O_CREAT|O_EXCL
   - Optimistic versioning with even/odd counters
   - Memory barrier placement for consistency
3. **Lock-Free Reader**
   - Version validation before/after reads
   - Copy-based read operations for consistency
   - Conflict detection and retry logic
4. **Unit Tests with Timing**
   - <1ms write deadline validation (10,000 iterations)
   - <100µs read deadline validation (100,000 iterations)
   - Memory alignment verification tests

### Sprint 2: Discovery & Lifecycle (Week 3-4)
1. **Filesystem Discovery Protocol**
   - `/dev/shm/evo_{module}_{name}_{pid}` naming scheme
   - JSON metadata files with atomic create/delete
   - inotify-based change detection
2. **Process Detection & Cleanup**
   - pidfd-based death notification integration
   - 10-second grace period implementation
   - Orphan segment detection algorithm
3. **EVO Supervisor Integration**
   - ShmLifecycleManager trait implementation
   - Periodic cleanup (30-second intervals)
   - Emergency cleanup procedures
4. **Multi-Process Integration Tests**
   - Cross-process read/write validation
   - Process death simulation and cleanup verification
   - Concurrent segment creation/deletion tests

### Sprint 3: Performance & Validation (Week 5-6)
1. **Performance Optimization**
   - NUMA-aware memory allocation (mbind)
   - Cache-friendly data structure layout
   - Memory prefetch strategies for hot paths
2. **Comprehensive Validation**
   - 10+ concurrent readers per segment (linear scaling validation)
   - 100+ segments system-wide (resource exhaustion testing)
   - 24-hour endurance testing with deadline miss rate <0.01%
3. **Statistical Analysis**
   - p95, p99, p99.9 latencies
   - Jitter measurement with RT kernel isolation
   - Deadline miss rate validation under load
4. **Property Testing**
   - Version counter overflow handling
   - Memory corruption detection
   - Race condition exploration with different timing patterns

### Sprint 4: Integration & Documentation (Week 7-8)
1. **EVO RT Module Integration**
   - Control Unit: control_state segment integration
   - HAL Core: phys_input_io segment integration
   - Recipe Executor: phys_output_io segment integration
   - API Liaison: commands_config segment integration
2. **Documentation & Examples**
   - API documentation with timing guarantees
   - Usage examples for each integration pattern
   - Troubleshooting guide for common issues
3. **Performance Regression Suite**
   - Baseline establishment for CI/CD
   - Automated performance regression detection
   - Memory usage tracking and alerting
4. **Production Readiness**
   - Error handling validation under fault injection
   - Recovery procedure documentation
   - Monitoring and observability integration

## Risk Mitigation

**Technical Risks:**
- **Memory alignment issues** → 
  - Early prototype validation on target hardware (x86_64/ARM64)
  - Compile-time alignment verification with `#[repr(align(64))]`
  - Runtime alignment checks in debug builds
  - Cache line padding validation with performance counters

- **Version counter overflow** → 
  - 64-bit counters with 32-bit epoch + 32-bit sequence
  - Overflow detection with wrapping arithmetic checks
  - Automatic cleanup when approaching overflow (2^32 writes)
  - Epoch increment with global coordination

- **Cleanup race conditions** → 
  - Extensive property testing with timing validation
  - State machine verification with model checking
  - Atomic cleanup operations with compare-and-swap
  - Grace period implementation with timeout guarantees

**Performance Risks:**
- **Cache miss penalties** → 
  - NUMA-aware allocation with `mbind(MPOL_BIND)`
  - 64-byte cache line alignment for all hot data structures
  - Memory prefetch strategies: `_mm_prefetch` for read-ahead
  - CPU affinity binding for RT threads to avoid migration

- **System call overhead** → 
  - Minimize system calls in hot paths: use memory-mapped operations
  - Batch operations where possible (multiple segment operations)
  - Use vDSO calls for time operations (`CLOCK_MONOTONIC_COARSE`)
  - Pre-allocate file descriptors to avoid open/close overhead

- **Memory fragmentation** → 
  - Pre-allocated segment pools with fixed sizes (4KB, 64KB, 1MB, 16MB)
  - Huge page usage for large segments (>2MB) with `MAP_HUGETLB`
  - Memory pool recycling with generation-based cleanup
  - Virtual memory reservation to prevent address space fragmentation

**Integration Risks:**
- **EVO module dependencies** → 
  - Phased integration: start with Control Unit (lowest risk)
  - Backward compatibility shims during transition period
  - Feature flags for gradual rollout (`--enable-shm-integration`)
  - Rollback procedures with state preservation

- **Process lifecycle coupling** → 
  - Clear separation: SHM library independent of EVO supervisor
  - Well-defined initialization/shutdown sequences
  - Timeout-based operations to prevent blocking
  - Dead letter queues for orphaned state recovery

- **Real-time guarantee violations** → 
  - Continuous timing validation in CI/CD pipeline
  - Real-time scheduling policy verification (`SCHED_FIFO`)
  - CPU isolation validation with `isolcpus` kernel parameter
  - Memory locking verification with `mlockall(MCL_CURRENT|MCL_FUTURE)`

**Operational Risks:**
- **Resource exhaustion** → 
  - Configurable limits: max segments per process (default: 16)
  - System-wide limits: max total segments (default: 1024)
  - Memory usage monitoring with alerts at 80% capacity
  - Automatic cleanup of stale segments after 24 hours

- **Security vulnerabilities** → 
  - Restricted permissions: 0600 (owner only) for all SHM files
  - Input validation: segment names, sizes, offsets
  - Memory poisoning detection in debug builds
  - Process credential verification before cleanup operations

- **Debugging complexity** → 
  - Comprehensive tracing with bounded overhead (<2% CPU)
  - SHM inspector tool for runtime state visualization
  - Crash dump integration with segment state capture
  - Performance regression detection with historical baselines

## Monitoring & Observability

**Performance Metrics:**
- **Latency measurements:**
  - Writer operations: P95 < 1μs, P99 < 5μs (SC-002 compliance)
  - Reader operations: P95 < 500ns, P99 < 1μs (Class B <100μs deadline)
  - Discovery operations: P95 < 1ms, P99 < 5ms
  - Cleanup operations: P95 < 10ms, P99 < 50ms

- **Throughput tracking:**
  - Write operations per second (target: >1M ops/sec per core)
  - Read operations per second (target: >10M ops/sec per core)
  - Concurrent reader scaling (linear up to CPU count)
  - Memory bandwidth utilization (<80% of theoretical max)

- **Resource utilization:**
  - Memory overhead per segment (<256 bytes metadata)
  - File descriptor usage (bounded by `ulimit -n`)
  - CPU utilization in RT threads (<50% average)
  - Memory fragmentation ratio (<10% overhead)

**Health Indicators:**
- **Correctness validation:**
  - Version mismatch detection rate (target: 0 per hour)
  - Data corruption incidents (target: 0 per year)
  - Orphan segment count (trending toward 0)
  - Reader count accuracy (±0 tolerance)

- **System stability:**
  - Process crash correlation with SHM usage
  - Memory leak detection (stable RSS over 24h)
  - File descriptor leak monitoring
  - RT scheduling violations count (target: <0.01% of operations)

**Operational Dashboards:**
- **Real-time view:**
  - Active segments count and sizes
  - Reader/writer distribution across processes
  - Current latency percentiles (rolling 1-minute window)
  - Memory pool utilization by size class

- **Historical trends:**
  - Performance regression detection (weekly comparison)
  - Resource usage growth patterns (monthly)
  - Error rate trending (daily aggregation)
  - Capacity planning metrics (segment count, memory usage)

**Alerting Rules:**
- **Critical (immediate response):**
  - RT deadline miss rate > 0.01%
  - Data corruption detected
  - System memory usage > 95%
  - Process crash with SHM correlation

- **Warning (within 1 hour):**
  - Latency P99 exceeds SLA by 20%
  - Orphan segment count > 10
  - Memory fragmentation > 15%
  - Discovery operation failures > 1%

- **Info (daily review):**
  - Performance trend degradation
  - Unusual access patterns
  - Resource usage growth
  - Configuration drift detection

**Debugging Tools:**
- **State inspection:**
  ```bash
  # Runtime segment analysis
  evo-shm inspect --all-segments
  evo-shm inspect --process-id 1234
  evo-shm inspect --segment-name "control_data"
  
  # Performance profiling
  evo-shm profile --duration 60s --focus latency
  evo-shm profile --memory-access-patterns
  
  # Health validation
  evo-shm validate --consistency-check
  evo-shm validate --performance-baseline
  ```

- **Trace collection:**
  - Structured logging with correlation IDs
  - Performance trace spans with timing data
  - Error context preservation for debugging
  - Integration with EVO central observability

## Acceptance Criteria

### Functional Validation

**Single-Writer Enforcement:**
- ✅ Multiple writers attempting to claim same segment: second writer receives `SegmentBusy` error within 100μs
- ✅ Writer process crash: automatic cleanup releases exclusive lock within 1 second
- ✅ Writer process exits gracefully: immediate lock release with zero data loss
- ✅ Concurrent writer attempts: deterministic error handling with clear ownership semantics

**Multi-Reader Support:**
- ✅ 10+ concurrent readers: baseline performance maintained (development minimum)
- ✅ 100 concurrent readers: linear performance scaling (production target)
- ✅ Up to 1000 concurrent readers: stress test validation without system failure
- ✅ Reader attachment during active writes: consistent data view with version validation
- ✅ Reader process crash: automatic cleanup removes reader count within 1 second
- ✅ Dynamic reader join/leave: zero impact on existing readers or writer performance

**Data Consistency:**
- ✅ Read-while-write scenarios: readers see either old or new data, never partial updates
- ✅ Version counter validation: 10^9 write operations without version conflicts
- ✅ Memory ordering: all reads observe causally ordered writes on x86_64 and ARM64
- ✅ Cross-process consistency: data written by process A visible to process B within 10μs

**Discovery & Lifecycle:**
- ✅ Segment enumeration: discovery completes within 1ms for <1000 segments
- ✅ Orphan detection: cleanup of crashed process segments within 5 seconds
- ✅ Metadata persistence: segment recovery after system restart preserves correct state
- ✅ Concurrent discovery: multiple processes discovering simultaneously without conflicts

### Performance Validation

**Latency Requirements:**
```rust
// Consolidated validation framework
#[test]
fn validate_performance_requirements() {
    // SC-002: Write latency P95 < 1μs (Class A Critical <1ms deadline)
    let write_samples = benchmark_writes(10_000);
    assert!(write_samples.percentile(95.0) < Duration::from_nanos(1000));
    assert!(write_samples.percentile(99.0) < Duration::from_nanos(5000));
    
    // Read latency: P95 < 500ns, P99 < 1μs (Class B <100μs deadline)
    let read_samples = benchmark_reads(100_000);
    assert!(read_samples.percentile(95.0) < Duration::from_nanos(500));
    
    // SC-003: Throughput requirements
    assert!(write_throughput > 1_000_000.0); // >1M writes/sec per core
    assert!(read_throughput > 10_000_000.0); // >10M reads/sec per core
}
```

**Scalability Validation:**
- ✅ Memory usage scaling: O(segments + readers) with <256 bytes overhead per segment
- ✅ CPU usage scaling: <2% per 1000 operations in steady state
- ✅ Reader count scaling: linear performance up to 1000 readers per segment
- ✅ Segment count scaling: constant lookup time up to 10,000 segments

### Integration Validation

**EVO Module Compatibility:**
```rust
// Integration test framework
#[tokio::test]
async fn validate_control_unit_integration() {
    let control_unit = EvoControlUnit::new().await;
    let shm_handle = control_unit.get_shared_data_handle("sensor_data");
    
    // Validate RT constraints maintained
    let latency_samples = measure_rt_performance(&control_unit, Duration::from_secs(10)).await;
    assert!(latency_samples.deadline_misses == 0);
    assert!(latency_samples.max_latency < Duration::from_micros(100));
}

#[test]
fn validate_hal_core_integration() {
    let hal = EvoHalCore::initialize();
    let sensor_segment = hal.create_sensor_data_segment(1024 * 1024).unwrap();
    
    // Validate high-frequency updates
    let update_rate = measure_sensor_update_rate(&hal, Duration::from_secs(5));
    assert!(update_rate > 10_000.0); // >10kHz sensor updates
}
```

**Real-Time Compliance:**
- ✅ RT thread scheduling: zero inversions during 1-hour stress test
- ✅ Memory locking: all SHM memory locked in physical RAM
- ✅ CPU isolation: RT operations confined to isolated CPU cores
- ✅ Interrupt handling: <10μs worst-case interrupt latency impact

### Security Validation

**Access Control:**
- ✅ Process isolation: processes can only access segments they created or were granted
- ✅ File permissions: all SHM files created with mode 0600 (owner-only)
- ✅ Memory protection: segments mapped with appropriate read/write permissions
- ✅ Privilege escalation: no elevated privileges required for normal operations

**Resource Protection:**
- ✅ Memory bounds: all accesses validated against segment boundaries
- ✅ Resource limits: configurable limits enforced (segments per process, total memory)
- ✅ DoS prevention: rate limiting on segment creation/destruction
- ✅ Input validation: all segment names, sizes, and offsets validated

### Operational Validation

**Reliability Testing:**
```bash
# 24-hour soak test
./tests/soak_test.sh --duration 24h --load high --validation continuous

# Chaos engineering 
./tests/chaos_test.sh --kill-random-processes --memory-pressure --cpu-stress

# Performance regression
./tests/regression_test.sh --baseline v1.0.0 --threshold 5%
```

**Maintenance Operations:**
- ✅ Zero-downtime updates: rolling deployment without service interruption
- ✅ Configuration changes: dynamic reconfiguration without restart
- ✅ Monitoring integration: all metrics exported to EVO central monitoring
- ✅ Backup/restore: segment state preservation across system maintenance
