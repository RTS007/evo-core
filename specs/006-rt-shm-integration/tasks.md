# Tasks: RT System Integration — SHM P2P, Watchdog, HAL↔CU Cooperation

**Input**: Design documents from `/specs/006-rt-shm-integration/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: Included — the plan's Constitution Check mandates "Test-First / TDD for each phase" and the spec includes FR-078 (RT stability test) and per-story independent test criteria.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story. Dependency order is respected — some P2 stories appear before P1 stories when they are prerequisites.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

Rust workspace with 12 crates. All paths relative to repository root (`evo-core/`).

- Shared library: `evo_common/src/`
- Binaries: `evo/src/`, `evo_hal/src/`, `evo_control_unit/src/`, `evo_grpc/src/`, etc.
- Config files: `config/`
- Tests: `evo_common/tests/`, per-crate `tests/`

---

## Phase 1: Setup (Project Initialization)

**Purpose**: Set Rust 2024 edition and centralize workspace dependencies.

- [X] T001 Set `edition = "2024"` in all workspace crate Cargo.toml files (evo_common, evo, evo_hal, evo_control_unit, evo_grpc, evo_mqtt, evo_recipe_executor, evo_api, evo_diagnostic, evo_dashboard — excluding evo_shared_memory, deleted in Phase 6)
- [X] T002 [P] Add `[workspace.dependencies]` section to root `Cargo.toml` with shared deps: serde, toml, tracing, tracing-subscriber, heapless 0.9, nix 0.30, libc, thiserror, clap, criterion
- [X] T003 [P] Update all crate Cargo.toml files to reference workspace deps via `{ workspace = true }` syntax

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: P2P core library, system-wide constants, shared helpers. MUST be complete before ANY user story work begins.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T004 Create `evo_common/src/consts.rs` with `pub const MAX_AXES: usize = 64`, `MAX_DI: usize = 1024`, `MAX_DO: usize = 1024`, `MAX_AI: usize = 1024`, `MAX_AO: usize = 1024`, `DEFAULT_CONFIG_PATH`, `DEFAULT_STATE_FILE`, `CYCLE_TIME_US` — move from `evo_common/src/hal/consts.rs`
- [X] T005 Update `evo_common/src/lib.rs` to declare `pub mod consts`; trim `evo_common/src/hal/consts.rs` to remove moved constants and delete `HAL_SERVICE_NAME`; update all imports across workspace
- [X] T006 Migrate `OutboundWriter<T>` from `evo_control_unit/src/shm/writer.rs` to `TypedP2pWriter<T>` in `evo_common/src/shm/p2p.rs` — add `flock(LOCK_EX|LOCK_NB)` on create, `shm_open` mode `0o600`, `shm_unlink` on Drop
- [X] T007 Migrate `InboundReader<T>` from `evo_control_unit/src/shm/reader.rs` to `TypedP2pReader<T>` in `evo_common/src/shm/p2p.rs` — add `flock(LOCK_SH|LOCK_NB)` on attach, destination + version hash validation
- [X] T008 Implement `ShmError` with 9 variants in `evo_common/src/shm/p2p.rs`: InvalidMagic, VersionMismatch, DestinationMismatch, WriterAlreadyExists, ReaderAlreadyConnected, ReadContention, SegmentNotFound, PermissionDenied, HeartbeatStale
- [X] T009 Fix Rust 2024 `unsafe_op_in_unsafe_fn` — add explicit `unsafe {}` blocks inside unsafe fn bodies in P2P raw pointer code in `evo_common/src/shm/p2p.rs`
- [X] T010 [P] Create `evo_common/src/shm/io_helpers.rs` with `get_di(bank: &[u64; 16], pin: usize) -> bool` and `set_do(bank: &mut [u64; 16], pin: usize, value: bool)` bit-packed helpers
- [X] T011 [P] Unify `AnalogCurve` — keep canonical definition in `evo_common/src/io/config.rs`, delete duplicate struct from `evo_common/src/hal/config.rs`, update all imports
- [X] T012 Remove `EVO_SHM_MAGIC` from `evo_common/src/shm/consts.rs` and delete empty `evo_common/src/shm/config.rs`
- [X] T013 Update `evo_common/src/shm/mod.rs` to export `pub mod segments`, `pub mod conversions`, `pub mod io_helpers`
- [X] T014 Update `evo_common/src/prelude.rs` with top-10 exports: MAX_AXES, CYCLE_TIME_US, IoRole, IoRegistry, ShmError, ModuleAbbrev, TypedP2pWriter, TypedP2pReader, P2pSegmentHeader, ConfigLoader

**Checkpoint**: Foundation ready — P2P core API, constants, and helpers available for all user stories.

---

## Phase 3: User Story 3 — P2P SHM Library in evo_common (Priority: P1) 🎯 MVP

**Goal**: Complete the P2P SHM library with SegmentDiscovery and full test coverage, providing the sole SHM transport API for the entire workspace.

**Independent Test**: Process A creates `TypedP2pWriter::<TestStruct>::create("evo_test_seg")`, writes data; process B attaches with `TypedP2pReader::<TestStruct>::attach("evo_test_seg")`, reads data. Verify data matches, heartbeat increments, version hash mismatch is caught, duplicate writer rejected, stale detection works.

### Implementation for User Story 3

- [X] T015 [US3] Implement `SegmentDiscovery` in `evo_common/src/shm/p2p.rs` — `list_segments() -> Vec<SegmentInfo>` enumerating `/dev/shm/evo_*`, `list_for(module: ModuleAbbrev) -> Vec<SegmentInfo>`, with flock probe for writer liveness
- [X] T016 [US3] Write P2P unit tests in `evo_common/src/shm/p2p.rs` (or `evo_common/tests/p2p_unit.rs`) — create, attach, read/write round-trip, heartbeat increment, version hash validation, destination enforcement, flock single-writer/single-reader, stale detection
- [X] T017 [US3] Write P2P multi-process integration test in `evo_common/tests/p2p_integration.rs` — writer process + reader process, verify data consistency, verify cleanup on writer drop

**Checkpoint**: P2P SHM library is fully functional and tested. Any program can add SHM with ~5 lines of code.

---

## Phase 4: User Story 2 — Unified Configuration: One Source of Truth (Priority: P1)

**Goal**: HAL and CU load the same config files, building identical views of axes and I/O. Per-axis auto-discovery eliminates monolithic config.

**Independent Test**: Define a machine with 8 axes and I/O points. Verify ConfigLoader auto-discovers all axis files, validates NN↔id, rejects duplicates, rejects unknown fields. Verify both HAL and CU agree on axis count, IDs, parameters.

### Implementation for User Story 2

- [X] T018 [US2] Create `SystemConfig` + `WatchdogConfig` structs with `#[serde(deny_unknown_fields)]` in `evo_common/src/config.rs`
- [X] T019 [US2] Implement `load_config_dir(path: &Path) -> Result<FullConfig, ConfigError>` in `evo_common/src/config.rs` — load config.toml → SystemConfig, machine.toml → MachineConfig, io.toml → IoConfig, glob `axis_*_*.toml` → sort by NN → validate NN↔id → check duplicates → Vec\<AxisConfig\>
- [X] T019a [US2] Define numeric bounds constants in `evo_common/src/config.rs` (`MIN_KP`, `MAX_KP`, `MAX_VELOCITY`, `MIN_CYCLE_TIME_US`, etc. per FR-054) and add validation in `load_config_dir()` returning `ConfigError::ValidationError` for out-of-range values
- [X] T020 [P] [US2] Create `config/config.toml` with self-documenting header and `[watchdog]`, `[hal]`, `[cu]`, `[re]`, `[mqtt]`, `[grpc]`, `[api]`, `[dashboard]`, `[diagnostic]` sections
- [X] T021 [P] [US2] Create `config/io.toml` by copying and adapting from `specs/005-control-unit/io.toml` reference with self-documenting header
- [X] T022 [P] [US2] Rewrite `config/machine.toml` to global-only format — `[machine]`, `[global_safety]`, `[service_bypass]`, no `[[axes]]`, no I/O, with self-documenting header
- [X] T023 [US2] Migrate `config/test_8axis.toml` into 8 per-axis files (`config/axis_01_x.toml` through `config/axis_08_tailstock.toml`) each with self-documenting header, then delete `config/test_8axis.toml`
- [X] T024 [US2] Write config auto-discovery tests in `evo_common/tests/config_tests.rs` — axis file discovery, NN↔id validation, duplicate detection, missing axes error, unknown fields rejection, legacy `[[axes]]` rejection, numeric bounds validation (FR-054)

**Checkpoint**: Unified config infrastructure complete. Any program can load all configs with a single `load_config_dir()` call.

---

## Phase 5: User Story 7 — Unified SHM Data Types (Priority: P2)

**Goal**: All 15 segment types defined in `evo_common` with `#[repr(C)]`, static size assertions, and conversion functions for HAL↔SHM data translation.

**Independent Test**: Verify `size_of::<HalToCuSegment>()` and `align_of::<HalToCuSegment>()` produce identical values across all binaries. Verify `struct_version_hash::<HalToCuSegment>()` matches. Verify conversion round-trip preserves data.

### Implementation for User Story 7

- [X] T025 [P] [US7] Define 9 additional segment structs in `evo_common/src/shm/segments.rs` — HalToMqtSegment, HalToRpcSegment, HalToReSegment, RpcToHalSegment, RpcToReSegment, ReToHalSegment, ReToMqtSegment, ReToRpcSegment, CuToRpcSegment — all `#[repr(C, align(64))]` with P2pSegmentHeader as first field
- [X] T026 [US7] Implement conversion functions in `evo_common/src/shm/conversions.rs` — `HalStatus → HalToCuSegment` (bool→bitfield packing, DI bool→u64 bit-packing, AI scaled extraction) and `CuToHalSegment → HalCommands` (reverse unpacking)
- [X] T027 [US7] Add `static_assert!` for size and alignment of all 15 segment types in `evo_common/src/shm/segments.rs`
- [X] T028 [US7] Write conversion round-trip tests and version hash stability tests in `evo_common/src/shm/conversions.rs` or `evo_common/tests/conversion_tests.rs`

**Checkpoint**: All 15 SHM segment types are defined with static guarantees. HAL↔SHM conversion is tested.

---

## Phase 6: User Story 4 — Remove evo_shared_memory Crate (Priority: P1)

**Goal**: Delete entire `evo_shared_memory` crate. All dependents migrated to `evo_common`'s P2P API. Zero references remain.

**Independent Test**: `cargo build --workspace` succeeds. `grep -r "evo_shared_memory" --include="*.toml" --include="*.rs"` returns zero matches. No files exist under `evo_shared_memory/`.

### Implementation for User Story 4

- [X] T029 [US4] Update CU's `ShmBundle` in `evo_control_unit/src/shm/mod.rs` to import `TypedP2pWriter`/`TypedP2pReader` from `evo_common::shm::p2p`; delete `evo_control_unit/src/shm/writer.rs` and `evo_control_unit/src/shm/reader.rs`
- [X] T030 [US4] Remove `evo_shared_memory` dependency from all crate Cargo.toml files: `evo/Cargo.toml`, `evo_hal/Cargo.toml`, `evo_grpc/Cargo.toml`, `evo_recipe_executor/Cargo.toml`, `evo_control_unit/Cargo.toml`
- [X] T031 [US4] Delete `evo_shared_memory/` directory entirely and remove from workspace `members` in root `Cargo.toml`
- [X] T032 [P] [US4] Delete dead HAL files: `evo_hal/src/main_old.rs`, `evo_hal/src/shm.rs`, `evo_hal/src/module_status.rs`
- [X] T033 [US4] Verify: `cargo build --workspace` succeeds and `grep -r "evo_shared_memory"` returns zero matches across entire workspace

**Checkpoint**: evo_shared_memory is gone. Exactly one SHM implementation exists (P2P in evo_common).

---

## Phase 7: User Story 9 — Dependency Cleanup and Workspace Hygiene (Priority: P2)

**Goal**: All dependency conflicts resolved, unused deps removed, aliases fixed, edition 2024 compatibility confirmed, zero build warnings.

**Independent Test**: `cargo build --workspace 2>&1 | grep -c "warning"` = 0 for dep-related warnings. `cargo tree -d` shows no duplicates for heapless, nix, serde, tracing. Dead files deleted.

### Implementation for User Story 9

- [X] T034 [US9] Fix nix 0.30 API break in `evo_control_unit/src/engine/runner.rs`: `MlockallFlags` → `MlockAllFlags`, verify `sched_setscheduler`/`sched_param` API compatibility
- [X] T035 [P] [US9] Remove unused dependencies: `parking_lot` from `evo/Cargo.toml`, `evo_grpc/Cargo.toml`, `evo_recipe_executor/Cargo.toml`; `tokio` from `evo/Cargo.toml`; `bitflags` from `evo_control_unit/Cargo.toml`; `static_assertions` from `evo_control_unit/Cargo.toml`
- [X] T036 [US9] Fix alias in `evo/Cargo.toml`: rename `evo = { package = "evo_common" }` → `evo_common = { path = "../evo_common" }` and update all `use evo::` → `use evo_common::` in `evo/src/main.rs`
- [X] T037 [US9] Migrate `evo_common` from `log` to `tracing` — find-and-replace `log::*` macros with `tracing::*` across all files under `evo_common/src/`
- [X] T038 [US9] Resolve `rt` feature flag in `evo_control_unit/Cargo.toml` — populate with `nix` feature gates (`nix/sched`, `nix/time`, `nix/resource`) for RT-specific code, or remove if always compiled
- [X] T039 [P] [US9] Delete stale config files: `config/test_cu.toml`, `config/test_io.toml`
- [X] T040 [US9] Verify: `cargo build --workspace` zero warnings, `cargo tree -d` zero duplicates for key deps (heapless, nix, serde, tracing)

**Checkpoint**: Workspace is clean — unified dependency versions, no dead code, no aliases, no stale configs.

---

## Phase 8: User Story 5 — HAL Writes Feedback to SHM, Reads Commands from SHM (Priority: P1)

**Goal**: HAL's RT loop writes full `HalToCuSegment` to SHM after every driver cycle, reads `CuToHalSegment` before every cycle. Loads io.toml, uses IoRegistry, enforces I/O role ownership.

**Independent Test**: Start HAL in simulation mode. Attach external reader to `evo_hal_cu`. Verify axis positions update every cycle. Write `ControlOutputVector` to `evo_cu_hal`. Verify HAL simulation driver receives commands.

### Implementation for User Story 5

- [X] T041 [US5] Add `--config-dir` CLI arg to `evo_hal/src/main.rs`; load `config.toml`, `machine.toml`, `io.toml`, and axis files via `evo_common::config::load_config_dir()`; build `IoRegistry` from loaded IoConfig
- [X] T042 [US5] Create 4 `TypedP2pWriter`s at startup in `evo_hal/src/core.rs`: `evo_hal_cu` (HalToCuSegment), `evo_hal_mqt` (HalToMqtSegment), `evo_hal_rpc` (HalToRpcSegment), `evo_hal_re` (HalToReSegment)
- [X] T043 [US5] Attempt attach of 3 `TypedP2pReader`s in `evo_hal/src/core.rs`: `evo_cu_hal` (CuToHalSegment), `evo_rpc_hal` (RpcToHalSegment), `evo_re_hal` (ReToHalSegment) — non-blocking, retry periodically
- [X] T044 [US5] Fill `TODO: Write status to SHM` in `evo_hal/src/core.rs` run loop — convert `HalStatus → HalToCuSegment` via `evo_common::shm::conversions`, call `writer.commit()`
- [X] T045 [US5] Fill `TODO: Read commands from SHM` in `evo_hal/src/core.rs` run loop — read `CuToHalSegment → HalCommands` via conversions if segment exists, else default zero commands
- [X] T046 [P] [US5] Implement DI bit-packing in `evo_hal/src/core.rs` — read `[bool; 1024]` from driver → pack to `[u64; 16]` using `evo_common::shm::io_helpers::set_do()`
- [X] T047 [P] [US5] Implement AI scaling in `evo_hal/src/core.rs` — read `[AnalogValue; 1024]` from driver → extract `.scaled` to `[f64; 64]` for HalToCuSegment
- [X] T048 [US5] Implement I/O role ownership enforcement in `evo_hal/src/core.rs` — HAL ignores RE commands for IoRole-assigned pins, logs using existing error schema
- [X] T049 [P] [US5] Clean up unused public methods in HAL simulation (`evo_hal/src/drivers/simulation/`); delete or fix `evo_hal/config/machine.toml` (FR-059, FR-064)
- [X] T050 [US5] Refactor `DriverRegistry` global `LazyLock<RwLock<HashMap>>` to constructor-injection in `evo_hal/src/driver_registry.rs` — remove `#[ignore]` from tests
- [X] T051 [US5] Write HAL SHM tests: writer creates segment and external reader verifies data, HAL reads known values from segment, DI packing round-trip, AI scaling verification, role ownership enforcement test

**Checkpoint**: HAL writes sensor data to SHM and reads control commands every RT cycle. Simulation pipeline is functional.

---

## Phase 9: User Story 6 — CU Binary Runs the RT Loop (Priority: P1)

**Goal**: CU binary instantiates `CycleRunner` and enters the RT loop, reading HAL feedback and writing commands every cycle.

**Independent Test**: Start CU binary with `--config-dir config/`. Verify process enters `CycleRunner::run()`. Verify `evo_cu_hal` segment created with incrementing heartbeat. Verify `evo_cu_mqt` has live status.

### Implementation for User Story 6

- [X] T052 [US6] Rewrite `evo_control_unit/src/main.rs` — parse `--config-dir`, load all configs via `load_config_dir()`, build IoRegistry, create/attach SHM segments (evo_cu_hal, evo_cu_mqt, evo_cu_re, evo_cu_rpc writers; evo_hal_cu reader), call `CycleRunner::run()`
- [X] T053 [US6] Extend `CycleRunner` with `IoRegistry` and `AxisControlState[MAX_AXES]` in `evo_control_unit/src/engine/runner.rs`
- [X] T054 [US6] Ensure cycle body reads `di_bank` and `ai_values` from `HalToCuSegment` and exposes to state machine logic in `evo_control_unit/src/engine/runner.rs`
- [X] T055 [US6] Fix MQT `error_flags` truncation — write as `u32` not `u16`/`u8` in `evo_control_unit/src/engine/runner.rs`
- [X] T056 [US6] Add periodic `try_attach_re()` and `try_attach_rpc()` — once per second, not every RT cycle — in `evo_control_unit/src/engine/runner.rs`
- [X] T057 [US6] Write CU binary tests: CU starts and creates segments, CU reads from evo_hal_cu, CU writes to evo_cu_hal, MQT status has full-width error_flags

**Checkpoint**: CU binary enters RT loop, reads HAL feedback, writes commands. Control pipeline is connected.

---

## Phase 10: User Story 1 — Watchdog Starts HAL and CU, End-to-End Data Flow (Priority: P1)

**Goal**: `evo` binary spawns HAL→CU, monitors via waitpid, restarts with backoff, graceful shutdown with SHM cleanup.

**Independent Test**: Run `evo --config-dir config/`. Verify HAL starts first, creates `evo_hal_cu`. CU starts after. Heartbeats increment. Kill HAL with SIGKILL — watchdog restarts within timeout. Send SIGTERM to watchdog — clean exit within 5 seconds.

### Implementation for User Story 1

- [X] T058 [US1] Rewrite `evo/src/main.rs` — synchronous main loop, load `config.toml [watchdog]` via `evo_common::config`, parse `--config-dir` CLI arg
- [X] T059 [US1] Implement `spawn_module()` in `evo/src/main.rs` — `std::process::Command` for `evo_hal` and `evo_control_unit` with `--config-dir` forwarding
- [X] T060 [US1] Implement ordered startup in `evo/src/main.rs` — spawn HAL first, poll `/dev/shm/evo_hal_cu` until exists + heartbeat > 0 (configurable timeout from config.toml), then spawn CU
- [X] T061 [US1] Implement process monitoring in `evo/src/main.rs` — `waitpid(WNOHANG)` poll loop, detect child exit/crash
- [X] T062 [US1] Implement restart logic in `evo/src/main.rs` — exponential backoff (100ms → 30s configurable), max 5 restarts configurable, stable-run reset after 60s, single CRITICAL log on exhaustion
- [X] T063 [US1] Implement graceful shutdown in `evo/src/main.rs` — SIGTERM/SIGINT handler, CU→HAL shutdown order (reverse of startup), SIGKILL fallback after timeout, shm_unlink all `evo_*` segments
- [X] T064 [US1] Implement orphan SHM cleanup on startup in `evo/src/main.rs` — enumerate `/dev/shm/evo_*`, flock probe for writer liveness, `shm_unlink` dead segments
- [X] T065 [US1] Create `WatchdogTrait` in `evo_common` (new module or in `evo_common/src/lib.rs`) — `spawn_module()`, `health_check()`, `restart_module()`, `shutdown_all()`; implement in `evo/src/main.rs`
- [X] T066 [US1] Implement optional P2P header heartbeat monitoring in `evo/src/main.rs` — read first 64 bytes of mapped segment for hang detection (supplementary to waitpid)
- [X] T067 [US1] Write watchdog tests: spawn and verify segments created, crash child and verify restart with backoff, graceful shutdown with SHM cleanup, orphan cleanup on startup

**Checkpoint**: Full end-to-end pipeline operational — watchdog spawns HAL→CU, data flows through SHM, crash recovery works, clean shutdown.

---

## Phase 11: User Story 8 — Skeleton P2P Contracts for All Programs (Priority: P2)

**Goal**: Every EVO program has P2P initialization code. Future integration requires only filling in application logic.

**Independent Test**: For each stub, verify segment struct types exist in evo_common, stub's main.rs has P2P init code, segment names follow `evo_[SRC]_[DST]` convention. `cargo build --workspace` succeeds.

### Implementation for User Story 8

- [X] T068 [P] [US8] Rewrite `evo_mqtt/src/main.rs` — attach 3 `TypedP2pReader`s (evo_cu_mqt, evo_hal_mqt, evo_re_mqt), placeholder read loop with tracing output
- [X] T069 [P] [US8] Rewrite `evo_grpc/src/main.rs` — create 3 `TypedP2pWriter`s (evo_rpc_cu, evo_rpc_hal, evo_rpc_re), attach 3 `TypedP2pReader`s (evo_cu_rpc, evo_hal_rpc, evo_re_rpc), placeholder gRPC server
- [X] T070 [P] [US8] Rewrite `evo_recipe_executor/src/main.rs` — create 4 `TypedP2pWriter`s (evo_re_cu, evo_re_hal, evo_re_mqt, evo_re_rpc), attach 3 `TypedP2pReader`s (evo_cu_re, evo_hal_re, evo_rpc_re)
- [X] T071 [P] [US8] Update `evo_api/src/main.rs` — no SHM, placeholder for gRPC client (connects to evo_grpc) + MQTT subscriber (connects to evo_mqtt)
- [X] T072 [P] [US8] Update `evo_diagnostic/src/main.rs` — no SHM, placeholder (communicates via gRPC + MQTT)
- [X] T073 [P] [US8] Update `evo_dashboard/src/main.rs` — no SHM, placeholder (communicates via gRPC + MQTT)
- [X] T074 [US8] Verify all stub programs compile: `cargo build --workspace` succeeds; all stubs depend only on `evo_common` for SHM types

**Checkpoint**: All 12 workspace programs have appropriate SHM initialization (or documented non-SHM rationale).

---

## Phase 12: Polish & Cross-Cutting Concerns

**Purpose**: Integration testing, benchmarks, deduplication verification, success criteria validation.

- [X] T075 [P] Integration test: run `evo --config-dir config/`, verify HAL and CU start, `evo_hal_cu` and `evo_cu_hal` segments active, heartbeats incrementing at cycle rate
- [X] T076 [P] Data round-trip test: HAL writes known axis positions to `evo_hal_cu`, verify CU reads matching values within 2 cycles (≤ 2ms at 1kHz)
- [X] T077 [P] Crash recovery test: SIGKILL HAL, verify watchdog detects death, restarts both HAL and CU, system recovers within 10 seconds
- [X] T078 Graceful shutdown test: SIGTERM watchdog, verify CU stops first, HAL stops second, all `evo_*` SHM segments cleaned, exit within 5 seconds with code 0
- [X] T079 Config agreement test: both HAL and CU load same configs, compare IoRegistry outputs for 10 roles — all match
- [X] T080 Short RT stability test (FR-078): `CycleRunner` 10,000 cycles, verify zero deadline misses (within ±10% of configured cycle time)
- [X] T081 P2P latency benchmarks via criterion in `evo_common/benches/p2p_bench.rs`: write ≤ 5µs, read ≤ 2µs for segments ≤ 8KB
- [X] T082 [US10] Verify constant deduplication: `grep -rn "pub const MAX_AXES" --include="*.rs"` returns exactly 1 result in `evo_common/src/consts.rs`; `grep -rn "struct AnalogCurve" --include="*.rs"` returns exactly 1 definition; `grep -rn "EVO_SHM_MAGIC" --include="*.rs"` returns 0 matches
- [X] T083 Verify all success criteria SC-001 through SC-014 from spec.md are met
- [X] T084 Run quickstart.md validation — follow all steps in `specs/006-rt-shm-integration/quickstart.md` and verify they work

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories
- **US3 (Phase 3)**: Depends on Foundational — P2P testing requires Writer/Reader from Phase 2
- **US2 (Phase 4)**: Depends on Foundational — config structs use constants from Phase 2
- **US7 (Phase 5)**: Depends on Foundational — segment structs reference P2pSegmentHeader from Phase 2
- **US4 (Phase 6)**: Depends on US3 + US7 — needs P2P library + segment types to replace evo_shared_memory deps
- **US9 (Phase 7)**: Depends on US4 — cleanup after crate removal; can partially parallel with US4
- **US5 (Phase 8)**: Depends on US4 + US7 + US2 — needs P2P, segments, config, evo_shared_memory gone
- **US6 (Phase 9)**: Depends on US4 + US7 + US2 + US9 (nix 0.30 fix) — can run in parallel with US5
- **US1 (Phase 10)**: Depends on US5 + US6 — needs both HAL and CU functional
- **US8 (Phase 11)**: Depends on US4 + US7 — needs P2P lib + segment types; can run in parallel with US5/US6
- **Polish (Phase 12)**: Depends on all user stories being complete

### Dependency Diagram

```
Phase 1 (Setup) ──► Phase 2 (Foundational) ─┬─► Phase 3 (US3) ──┐
                                              ├─► Phase 4 (US2)   │
                                              └─► Phase 5 (US7) ──┼─► Phase 6 (US4) ──► Phase 7 (US9)
                                                                   │          │                │
                                                                   │          ├──► Phase 8 (US5) ◄─┘
                                                                   │          │           │
                                                                   │          ├──► Phase 9 (US6) ◄─┘
                                                                   │          │           │
                                                                   │          │    Phase 10 (US1) ◄─── US5+US6
                                                                   │          │
                                                                   │          └──► Phase 11 (US8)
                                                                   │
                                                                   └──► Phase 12 (Polish) ◄─── all
```

### User Story Dependencies

- **US3 (P1)**: After Foundational — no dependencies on other stories
- **US2 (P1)**: After Foundational — no dependencies on other stories
- **US7 (P2)**: After Foundational — no dependencies on other stories
- **US3, US2, US7 can proceed in parallel** after Foundational
- **US4 (P1)**: After US3 + US7 (needs P2P + segment types)
- **US9 (P2)**: After US4 (can partially overlap)
- **US5 (P1)**: After US4 + US7 + US2 + US9(nix fix)
- **US6 (P1)**: After US4 + US7 + US2 + US9(nix fix) — **parallel with US5**
- **US1 (P1)**: After US5 + US6
- **US8 (P2)**: After US4 + US7 — **parallel with US5/US6**
- **US10 (P3)**: Covered by Foundational (T004, T005, T011, T012) and verified in Polish (T082)

### Within Each User Story

- Models/types before services
- Services before binary integration
- Core implementation before tests
- Story complete before moving to dependent stories

### Parallel Opportunities

**After Phase 2 (Foundational)**:
- T015 (SegmentDiscovery) ‖ T018 (SystemConfig) ‖ T025 (segment structs)
- T020, T021, T022 (config files) — all different files
- T016, T017 (P2P tests) ‖ T024 (config tests) ‖ T028 (conversion tests)

**After Phase 6 (US4)**:
- Phase 8 (US5) ‖ Phase 9 (US6) ‖ Phase 11 (US8) — all different binaries
- T042–T043 (HAL P2P) ‖ T052 (CU main.rs) ‖ T068–T073 (stubs)

**Within Phase 11 (US8)**:
- T068 ‖ T069 ‖ T070 ‖ T071 ‖ T072 ‖ T073 — all different binaries, all [P]

---

## Implementation Strategy

### MVP First (US3 + US2 + US7 → US4 → US5 + US6 → US1)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phases 3–5 in parallel: US3 (P2P tests), US2 (config), US7 (segments)
4. Complete Phase 6: US4 (remove evo_shared_memory)
5. Complete Phase 7: US9 (dependency cleanup)
6. Complete Phases 8–9 in parallel: US5 (HAL SHM) + US6 (CU binary)
7. Complete Phase 10: US1 (watchdog E2E)
8. **STOP and VALIDATE**: Run integration tests — full pipeline operational
9. Complete Phase 11: US8 (stubs)
10. Complete Phase 12: Polish & benchmarks

### Incremental Delivery

1. Foundational → P2P library usable by all crates
2. US3 + US2 + US7 → All types, config, and tests ready
3. US4 → Clean workspace (evo_shared_memory gone)
4. US5 + US6 → HAL↔CU pipeline functional
5. US1 → Full system operational (watchdog + restart + shutdown)
6. US8 → All programs have SHM skeleton
7. Each increment is independently testable and deployable

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks within same phase
- [Story] label maps task to specific user story for traceability
- All SHM segment structs must be `#[repr(C, align(64))]` with static size assertions
- Rust 2024 edition requires explicit `unsafe {}` blocks inside `unsafe fn` bodies
- nix 0.30 breaks `MlockallFlags` → `MlockAllFlags` — fix before CU can compile
- Every TOML config file must include self-documenting header comment block
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
