# Feature Specification: Shared Memory Lifecycle

**Feature Branch**: `002-shm-lifecycle`  
**Created**: 28 listopada 2025  
**Status**: Draft  
**Input**: User description: "Shared Memory lifecycle (single-writer segments, lock-free reads)"

## Clarifications

### Session 2025-11-28

- Q: Data consistency model when writer updates while readers access → A: Sequential consistency model achieved through optimistic even/odd versioning where readers see updates in order and detect concurrent writes via version validation
- Q: Orphaned segment detection mechanism and timing → A: Process death detection using Linux pidfd mechanism with epoll for RT systems, fallback to periodic PID validation for simulation/non-RT environments
- Q: Segment size constraints and typical ranges → A: Configurable range from 4KB to 1GB with page-aligned boundaries
- Q: Segment identifier format and discovery mechanism → A: Named string identifiers with filesystem-based discovery (e.g., /dev/shm/evo_segment_name)
- Q: How readers handle concurrent write scenarios → A: Optimistic even/odd versioning

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Single Writer Creates Segment (Priority: P1)

A system component needs to create and manage a shared memory segment where it has exclusive write access, allowing other components to read data without blocking the writer's operations.

**Why this priority**: This is the foundational capability - without the ability to create and manage single-writer segments, no other functionality can work. It establishes the basic contract for shared memory ownership.

**Independent Test**: Can be fully tested by having one component create a segment, write data to it, and verify the segment exists and contains the written data without any readers present.

**Acceptance Scenarios**:

1. **Given** a system component needs to store data, **When** it creates a single-writer shared memory segment, **Then** the segment is allocated with the specified size and the component has exclusive write access
2. **Given** a single-writer segment exists, **When** the writer updates data in the segment, **Then** the changes are immediately visible in memory without requiring locks
3. **Given** a writer wants to modify data, **When** it performs write operations, **Then** no blocking occurs regardless of concurrent readers

---

### User Story 2 - Multiple Readers Access Data Lock-Free (Priority: P2)

Multiple system components need to read data from a shared memory segment simultaneously without blocking each other or the writer, ensuring high-performance concurrent access.

**Why this priority**: This enables the core benefit of the lock-free design - allowing multiple consumers to access data concurrently without performance degradation. Essential for scalable system architecture.

**Independent Test**: Can be tested by having multiple reader components simultaneously access the same segment and verify they can all read data concurrently without blocking or data corruption.

**Acceptance Scenarios**:

1. **Given** a shared memory segment with data, **When** multiple readers access it simultaneously, **Then** all readers can access the data without waiting for locks
2. **Given** concurrent readers are accessing a segment, **When** a new reader joins, **Then** it can immediately access current data without disrupting existing readers
3. **Given** readers are accessing data, **When** the writer updates the segment, **Then** readers continue operating without interruption

---

### User Story 3 - Memory Lifecycle Management (Priority: P3)

The system needs to properly manage the lifecycle of shared memory segments, ensuring clean creation, usage tracking, and safe cleanup when segments are no longer needed.

**Why this priority**: While not blocking basic functionality, proper lifecycle management prevents memory leaks and ensures system stability over time. Critical for production reliability.

**Independent Test**: Can be tested by creating segments, tracking their usage, and verifying proper cleanup when all consumers disconnect, confirming no memory leaks occur.

**Acceptance Scenarios**:

1. **Given** a shared memory segment is no longer needed, **When** the last consumer disconnects, **Then** the segment is properly cleaned up and memory is freed
2. **Given** system startup, **When** components initialize, **Then** they can discover existing segments or create new ones as needed
3. **Given** unexpected component shutdown, **When** cleanup occurs, **Then** orphaned segments are detected and properly handled

---

### Edge Cases

- What happens when a writer process crashes while holding exclusive access to a segment?
- How does system handle memory exhaustion when creating new segments?
- What occurs when readers try to access a segment that's being destroyed?
- How are race conditions handled between segment creation and first reader access?
- How does optimistic versioning handle high-frequency writer updates that cause frequent reader retry scenarios?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST provide exclusive write access to a single component per shared memory segment
- **FR-002**: System MUST allow multiple readers to access shared memory segments concurrently without locks
- **FR-003**: System MUST ensure sequential consistency through optimistic even/odd versioning where readers validate version counters before/after reads to detect concurrent writes and retry if necessary
- **FR-004**: System MUST provide memory-mapped access to shared memory segments for optimal performance
- **FR-005**: System MUST track segment lifecycle including creation, usage, and cleanup
- **FR-006**: System MUST handle segment discovery using named string identifiers with filesystem-based discovery (e.g., /dev/shm/evo_segment_name)
- **FR-007**: System MUST prevent data races between single writer and multiple readers through lock-free mechanisms
- **FR-008**: System MUST provide automatic cleanup of unused segments using pidfd-based process death detection for RT systems, with fallback to periodic PID validation for simulation/non-RT environments
- **FR-009**: System MUST support configurable segment sizes from 4KB to 1GB with page-aligned boundaries
- **FR-010**: System MUST handle process cleanup ensuring segments are properly released when writers disconnect

### Key Entities

- **SharedMemorySegment**: Core segment representation with memory layout, header pointer, and memory mapping (internal implementation detail)
- **SegmentWriter**: Component with exclusive write access to a segment, responsible for data updates, version management, and segment lifecycle
- **SegmentReader**: Component with read-only access to segment data, operates lock-free with conflict detection and does not block other readers or the writer
- **SegmentDiscovery**: Service for filesystem-based segment enumeration and change detection
- **SegmentInfo**: Metadata structure containing segment properties (name, size, writer PID, timestamps, reader count)

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Multiple readers can access shared memory data without blocking each other or the writer with tiered scaling: minimum 10 concurrent readers (baseline), target 100 readers (production), maximum 1000 readers (stress test limit)
- **SC-002**: Write operations complete without waiting for reader locks, with P95 < 1μs latency (Class A Critical <1ms deadline)
- **SC-003**: System supports at least 100 concurrent shared memory segments without performance degradation
- **SC-004**: Memory access overhead <5% vs native pointer dereferencing
- **SC-005**: Reader count scales linearly up to 1000 concurrent readers per segment
- **SC-006**: Segment cleanup occurs within 1 second of last consumer disconnection
- **SC-007**: Zero memory leaks detected during 24-hour stress testing with segment creation/destruction cycles

## Assumptions *(mandatory)*

- **A-001**: Target platform provides memory-mapped file support (Linux/Unix mmap or Windows equivalent)
- **A-002**: System runs on architectures with cache-coherent memory (x86_64, ARM64)
- **A-003**: Components using shared memory operate as separate processes, not threads
- **A-004**: Maximum segment size is bounded by available system virtual memory
- **A-005**: Rust's memory safety guarantees apply to prevent unsafe memory access patterns
- **A-006**: Operating system provides adequate virtual memory management for concurrent segment access

## Dependencies *(mandatory)*

- **D-001**: Operating system memory-mapped file support
- **D-002**: Rust standard library or equivalent shared memory primitives
- **D-003**: Process identification and lifecycle monitoring capabilities
- **D-004**: Atomic memory operations for lock-free coordination

## Scope & Boundaries *(mandatory)*

### In Scope
- Single-writer, multi-reader shared memory segments with SegmentWriter/SegmentReader pattern
- Lock-free read access patterns with optimistic versioning
- Automatic lifecycle management and cleanup via SegmentDiscovery
- Cross-process memory sharing with SharedMemorySegment abstraction
- Memory-mapped segment access with page alignment

### Out of Scope
- Multi-writer scenarios (requires separate locking mechanisms)
- Network-distributed shared memory
- Persistent storage beyond process lifecycle
- Encryption or security mechanisms for memory content
- Dynamic segment resizing after creation
