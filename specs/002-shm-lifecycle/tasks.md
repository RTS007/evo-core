# Tasks: Shared Memory Lifecycle

**Input**: Design documents from `/specs/002-shm-lifecycle/`
**Prerequisites**: plan.md (required), spec.md (required for user stories)

**Feature**: Implement foundational shared memory lifecycle management system for EVO's real-time architecture with single-writer, multi-reader segments using lock-free access patterns.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [x] T001 Create evo_shared_memory crate structure with Cargo.toml in evo_shared_memory/
- [x] T002 Initialize Rust project dependencies (libc 0.2.150+, memmap2 0.9+, serde 1.0.190+, nix 0.27+, tracing 0.1.40+, thiserror 1.0.50+, parking_lot 0.12+) and dev-dependencies (criterion 0.5+, proptest 1.4+) in evo_shared_memory/Cargo.toml
- [x] T003 [P] Configure cargo clippy and rustfmt settings in evo_shared_memory/.rustfmt.toml
- [x] T004 [P] Setup initial project structure with src/lib.rs, tests/, benches/, examples/ directories in evo_shared_memory/
- [x] T005 [P] Create platform abstraction module structure in evo_shared_memory/src/platform/mod.rs and linux.rs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [x] T006 Create core error types and handling in evo_shared_memory/src/error.rs
- [x] T007 [P] Implement SegmentHeader struct with cache-line alignment in evo_shared_memory/src/segment.rs
- [x] T008 [P] Setup atomic version counter implementation in evo_shared_memory/src/version.rs
- [x] T009 [P] Implement platform-specific memory mapping primitives in evo_shared_memory/src/platform/linux.rs
- [x] T010 Create base SharedMemorySegment struct with memory layout in evo_shared_memory/src/segment.rs
- [x] T011 Setup tracing infrastructure with RT-safe logging in evo_shared_memory/src/lib.rs
- [x] T012 Implement basic memory alignment validation functions in evo_shared_memory/src/segment.rs
- [x] T013 Implement segment size validation (4KB-1GB range, page-aligned) in evo_shared_memory/src/segment.rs
- [x] T014 Create basic cleanup infrastructure and orphan detection in evo_shared_memory/src/lifecycle.rs
- [x] T015 Implement pidfd-based process death detection in evo_shared_memory/src/lifecycle.rs

**Checkpoint**: Foundation ready - user story implementation can now begin in parallel

---

## Phase 3: User Story 1 - Single Writer Creates Segment (Priority: P1) 🎯 MVP

**Goal**: Enable exclusive write access to shared memory segments with lock-free semantics

**Independent Test**: Create segment, write data, verify exclusive access and data persistence without readers

### Implementation for User Story 1

- [x] T016 [P] [US1] Create SegmentWriter struct with exclusive ownership in evo_shared_memory/src/writer.rs
- [x] T017 [P] [US1] Implement segment creation with O_CREAT|O_EXCL atomicity in evo_shared_memory/src/writer.rs
- [x] T018 [US1] Implement optimistic even/odd versioning in writer operations in evo_shared_memory/src/writer.rs
- [x] T019 [US1] Add memory barrier placement for write consistency in evo_shared_memory/src/writer.rs
- [x] T020 [US1] Implement SegmentWriter::write() with sub-microsecond latency in evo_shared_memory/src/writer.rs
- [x] T021 [US1] Add SegmentWriter::write_at() for offset-based writes in evo_shared_memory/src/writer.rs
- [x] T022 [US1] Implement SegmentWriter::flush() with memory barriers in evo_shared_memory/src/writer.rs
- [x] T023 [US1] Create segment naming scheme with collision prevention using `/dev/shm/evo_{module}_{name}_{pid}` and JSON metadata file `/dev/shm/evo_{name}.meta` (mode 0600) in evo_shared_memory/src/writer.rs
- [x] T024 [US1] Add writer process ID tracking and validation in evo_shared_memory/src/writer.rs
- [x] T025 [US1] Add segment size validation in SegmentWriter::create() with detailed error types in evo_shared_memory/src/writer.rs
- [x] T026 [US1] Integrate automatic cleanup on writer process exit in evo_shared_memory/src/writer.rs

**Checkpoint**: At this point, User Story 1 should be fully functional and testable independently

---

## Phase 4: User Story 2 - Multiple Readers Access Data Lock-Free (Priority: P2)

**Goal**: Enable concurrent lock-free read access from multiple processes

**Independent Test**: Multiple readers simultaneously access same segment without blocking or corruption

### Implementation for User Story 2

- [x] T027 [P] [US2] Create SegmentReader struct with lock-free design in evo_shared_memory/src/reader.rs
- [x] T028 [P] [US2] Implement version validation before/after reads in evo_shared_memory/src/reader.rs
- [x] T029 [US2] Implement SegmentReader::read() with conflict detection in evo_shared_memory/src/reader.rs
- [x] T030 [US2] Add SegmentReader::read_range() for offset-based reads in evo_shared_memory/src/reader.rs
- [x] T031 [US2] Implement copy-based read operations for consistency in evo_shared_memory/src/reader.rs
- [x] T032 [US2] Add reader count tracking and atomic updates in evo_shared_memory/src/reader.rs
- [x] T033 [US2] Implement SegmentReader::has_changed() version checking in evo_shared_memory/src/reader.rs
- [x] T034 [US2] Create reader attachment mechanism with error handling in evo_shared_memory/src/reader.rs
- [x] T035 [US2] Add concurrent reader scaling validation in evo_shared_memory/src/reader.rs
- [x] T036 [US2] Integrate automatic cleanup on reader process exit in evo_shared_memory/src/reader.rs

**Checkpoint**: At this point, User Stories 1 AND 2 should both work independently

---

## Phase 5: User Story 3 - Memory Lifecycle Management (Priority: P3)

**Goal**: Automatic lifecycle management with cleanup and discovery

**Independent Test**: Segment creation, usage tracking, and cleanup verification with orphan detection

### Implementation for User Story 3

- [x] T037 [P] [US3] Create discovery service with filesystem scanning in evo_shared_memory/src/discovery.rs
- [x] T038 [P] [US3] Implement segment metadata serialization in evo_shared_memory/src/discovery.rs
- [x] T039 [US3] Add inotify-based /dev/shm change detection in evo_shared_memory/src/discovery.rs
- [x] T040 [US3] Create SegmentInfo metadata structure in evo_shared_memory/src/discovery.rs
- [x] T041 [US3] Implement list_segments() discovery API in evo_shared_memory/src/discovery.rs
- [x] T042 [US3] Enhance cleanup with grace periods and advanced orphan detection in evo_shared_memory/src/lifecycle.rs
- [x] T043 [US3] Add cleanup_orphaned_segments() API with advanced filtering in evo_shared_memory/src/lifecycle.rs
- [x] T044 [US3] Integrate with EVO supervisor lifecycle management in evo_shared_memory/src/lifecycle.rs
- [x] T045 [US3] Add periodic cleanup scheduling and monitoring in evo_shared_memory/src/lifecycle.rs

**Checkpoint**: All user stories should now be independently functional

---

## Phase 6: EVO Integration & Real-Time Validation

**Purpose**: Integration with existing EVO modules and RT performance validation

- [x] T046 [P] Create Control Unit SHM integration in evo_control_unit/src/shm_integration.rs
- [x] T047 [P] Create HAL Core SHM integration in evo_hal_core/src/shm_integration.rs
- [x] T048 [P] Create Recipe Executor SHM integration in evo_recipe_executor/src/shm_integration.rs
- [ ] T049 [P] Create API Liaison SHM integration in evo_grpc/src/shm_integration.rs
- [ ] T050 Implement ShmLifecycleManager trait for EVO supervisor in evo/src/main.rs
- [x] T051 Add RT deadline validation tests in evo_shared_memory/tests/timing/deadline_test.rs
- [x] T052 Implement stress testing with 1000+ concurrent readers in evo_shared_memory/tests/integration/stress_test.rs
- [x] T053 Add memory alignment verification on target hardware in evo_shared_memory/tests/unit/alignment_tests.rs
- [x] T054 Create RT scheduling policy validation tests in evo_shared_memory/tests/timing/rt_compliance.rs

---

## Phase 7: Performance Optimization & Validation

**Purpose**: Performance optimization and comprehensive validation

- [x] T055 [P] Implement NUMA-aware memory allocation with mbind() in evo_shared_memory/src/platform/linux.rs
- [x] T056 [P] Add huge page support for large segments (>2MB) in evo_shared_memory/src/platform/linux.rs
- [x] T057 Create memory prefetch strategies for hot paths in evo_shared_memory/src/segment.rs
- [x] T058 Implement cache-friendly data structure layout optimization in evo_shared_memory/src/segment.rs
- [x] T059 Create comprehensive performance benchmarks with sub-microsecond validation in evo_shared_memory/benches/read_write_perf.rs and evo_shared_memory/benches/concurrent_access.rs
- [x] T060 Add 24-hour endurance testing with deadline miss rate validation in evo_shared_memory/tests/integration/endurance_test.rs
- [x] T061 Create statistical latency analysis (P95, P99, P99.9) in evo_shared_memory/tests/timing/latency_analysis.rs
- [x] T062 Implement jitter measurement with RT kernel isolation in evo_shared_memory/tests/timing/jitter_test.rs
- [x] T063 Add property testing for version counter overflow handling in evo_shared_memory/tests/property/versioning_props.rs
- [x] T064 Create memory corruption detection tests in evo_shared_memory/tests/property/corruption_tests.rs
- [x] T065 Implement segment size validation unit tests in evo_shared_memory/tests/unit/size_validation_tests.rs
- [x] T066 Add race condition exploration with timing patterns in evo_shared_memory/tests/property/race_condition_tests.rs

---

## Phase 8: Production Readiness & Documentation

**Purpose**: Final production preparation and comprehensive documentation

- [x] T067 Create comprehensive API documentation with timing guarantees in evo_shared_memory/src/lib.rs
- [x] T068 Implement usage examples for each integration pattern in evo_shared_memory/examples/
- [x] T069 Create troubleshooting guide for common issues in docs/troubleshooting.md
- [x] T070 Add automated performance regression detection in CI pipeline
- [x] T071 Create memory usage tracking and alerting mechanisms in evo_shared_memory/src/monitoring.rs
- [x] T072 Implement error handling validation under fault injection in evo_shared_memory/tests/integration/fault_injection.rs

**Final Checkpoint**: Feature is production-ready with full documentation, monitoring, and regression testing

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion - BLOCKS all user stories
- **User Stories (Phase 3-5)**: All depend on Foundational phase completion
  - User stories can proceed in parallel (if staffed)
  - Or sequentially in priority order (P1 → P2 → P3)
- **EVO Integration (Phase 6)**: Depends on User Stories 1-3 completion
- **Testing (Phase 7)**: Can run in parallel with implementation phases
- **Polish (Phase 8)**: Depends on all core functionality being complete

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational (Phase 2) - No dependencies on other stories
- **User Story 2 (P2)**: Depends on User Story 1 foundation but independently testable
- **User Story 3 (P3)**: Can start after Foundational, integrates with US1/US2 but independently testable

### Within Each User Story

- Core data structures before operations
- Writer/Reader implementations before integration
- Basic functionality before optimization
- Error handling throughout implementation
- Story complete before moving to next priority

### Parallel Opportunities

- All Setup tasks marked [P] can run in parallel
- All Foundational tasks marked [P] can run in parallel (within Phase 2)
- Once Foundational phase completes, User Story 1 can start
- User Story 2 can start after User Story 1 core is ready
- All EVO integration tasks marked [P] can run in parallel
- All testing tasks marked [P] can run in parallel
- All documentation tasks marked [P] can run in parallel

---

## Parallel Example: User Story 1

```bash
# Launch foundational tasks together:
Task: "Implement SegmentHeader struct with cache-line alignment"
Task: "Setup atomic version counter implementation" 
Task: "Implement platform-specific memory mapping primitives"

# Launch User Story 1 models together:
Task: "Create SegmentWriter struct with exclusive ownership"
Task: "Implement segment creation with O_CREAT|O_EXCL atomicity"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL - blocks all stories)
3. Complete Phase 3: User Story 1 - Single Writer Creates Segment
4. **STOP and VALIDATE**: Test exclusive write access independently
5. Deploy/demo basic shared memory functionality

### Incremental Delivery

1. Complete Setup + Foundational → Foundation ready
2. Add User Story 1 → Test independently → Single-writer segments working
3. Add User Story 2 → Test independently → Multi-reader access working  
4. Add User Story 3 → Test independently → Full lifecycle management working
5. Add EVO Integration → Full system integration
6. Each story adds value without breaking previous stories

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 (Single Writer)
   - Developer B: User Story 2 (Multi-Reader) 
   - Developer C: User Story 3 (Lifecycle Management)
   - Developer D: Testing infrastructure in parallel
3. Stories complete and integrate independently

### Real-Time Constraints

- All timing-critical tasks (T019, T027, T047, T048, T051, T059) require RT hardware validation
- Comprehensive performance validation in Phase 7 with P95 < 1μs write latency (SC-002)
- Stress testing must confirm <0.01% deadline miss rate under production load (Class A Critical)
- Memory alignment verification mandatory on both x86_64 and ARM64 architectures
- Cleanup tasks (T014, T015, T026, T036) ensure proper resource management in RT environment
- Observability overhead must not exceed 2% of RT thread CPU budget per constitution

---

## Notes

- [P] tasks = different files, no dependencies on concurrent tasks
- [Story] label maps task to specific user story for traceability  
- Each user story should be independently completable and testable
- Comprehensive performance validation concentrated in Phase 7
- EVO constitution compliance verified through timing validation
- Stop at any checkpoint to validate story independently
- Commit after each task or logical group