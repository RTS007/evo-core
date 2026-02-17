# Implementation Plan: RT System Integration — SHM P2P, Watchdog, HAL↔CU Cooperation

**Branch**: `006-rt-shm-integration` | **Date**: 2026-02-10 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/006-rt-shm-integration/spec.md`

## Summary

Deliver the foundational runtime integration of the EVO system: a working pipeline where the watchdog (`evo`) spawns HAL and CU as child processes, both exchange real-time data through P2P shared memory, and all RT programs share a unified configuration model. The legacy `evo_shared_memory` crate is removed entirely. The P2P typed writer/reader pair migrates from CU to `evo_common`, becoming the single SHM transport for all crates. Per-axis configuration files replace the monolithic config. 15 segment type contracts are defined. All stub programs get P2P skeleton initialization.

## Technical Context

**Language/Version**: Rust 2024 edition (rustc 1.85+)
**Primary Dependencies**: `nix 0.30` (POSIX SHM, flock, signals, sched), `serde + toml 0.9` (config), `tracing 0.1` (logging), `heapless 0.9` (fixed-size collections), `clap` (CLI args), `libc 0.2`
**Storage**: POSIX shared memory (`/dev/shm/evo_*`), TOML config files on filesystem
**Testing**: `cargo test` (unit), integration tests with multi-process SHM, `criterion` benchmarks for latency
**Target Platform**: Linux x86_64 (POSIX SHM required)
**Project Type**: Workspace with 12 crates (after `evo_shared_memory` removal)
**Performance Goals**: P2P write ≤ 5µs, read ≤ 2µs for segments ≤ 8KB (all current segments are ≤ 4KB page-rounded)
**Constraints**: Zero heap allocation, zero mutex, zero syscalls in RT hot path. Lock-free seqlock protocol for read/write.
**Scale/Scope**: 15 SHM segment types, 12 workspace crates, ~69 audit items resolved, 8 reference axis configs

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Notes |
|---|-----------|--------|-------|
| I | Soft RT Performance | ✅ PASS | P2P hot path: zero alloc, zero mutex, zero syscall. Deadlines: write ≤5µs (Class A), read ≤2µs (Class A). Miss budget: <0.01%. |
| II | Test-First | ✅ PASS | Unit tests for P2P protocol, integration tests for multi-process SHM, timing benchmarks, config validation tests. TDD for each phase. |
| III | Code Quality | ✅ PASS | `#[deny(warnings)]`, `#[repr(C)]` for all segment types, static assertions on sizes, `serde(deny_unknown_fields)` for configs. |
| IV | Consistent Interface | ✅ PASS | Structured `ShmError`/`ConfigError` with stable codes. CLI `--config-dir` for all programs. |
| V | Performance Budgets | ✅ PASS | All segments ≤ 4KB (page-rounded). Benchmarks via criterion. Segment sizes verified by static_assert. |
| VI | Observability | ✅ PASS | `tracing` for all crates. Heartbeat counters per segment. `CuToMqtSegment` status snapshot. Cycle timing stats. |
| VII | Config Versioning | ✅ PASS | `serde(deny_unknown_fields)` rejects unknown params. `struct_version_hash<T>()` for binary segment compatibility. |
| VIII | Security | ✅ PASS | `shm_open` mode `0o600`. `flock` for writer/reader exclusion. No ambient root. |
| IX | Simplicity | ✅ PASS | Removing `evo_shared_memory` (net -1 crate). Moving P2P to `evo_common` (single source). No speculative layering. |
| X | Change Review | ✅ PASS | Constitution mapping in this section. |
| XI | Spec-Driven | ✅ PASS | All work derives from spec.md FRs. Audit Resolution Matrix maps 69 items. |
| XII | Error Handling | ✅ PASS | `ShmError` with 9 variants. Watchdog restart with backoff. HAL/CU graceful fallback on missing segments. |
| XIII | Lifecycle | ✅ PASS | All SHM allocation at startup before RT loop. `Drop` for cleanup. Ordered startup/shutdown. |
| XIV | Memory Management | ✅ PASS | Pre-allocated page-aligned buffers. `#[repr(C, align(64))]` for cache-line alignment. `[u64; 16]` bit-packed DI/DO. |
| XV | IPC | ✅ PASS | Zero-copy SHM via P2P seqlock. `flock` for exclusion. Bounded retry (max 3). Fixed segment names. |
| XVI | Architecture | ✅ PASS | ADR: P2P as sole RT IPC. Topology diagram. Protocol rationale table. |
| XVII | Library-First | ✅ PASS | P2P library in `evo_common`. Segments in `evo_common`. Conversions in `evo_common`. Apps are thin binaries. |
| XVIII | Deterministic Interface | ✅ PASS | Bounded P2P read/write. Non-blocking segment attach. Heartbeat-based health check. |
| XIX | Non-RT Isolation | ✅ PASS | MQTT, gRPC, API, Dashboard, Diagnostic run in separate processes. SHM P2P for RT↔RT only. Lock-free overwrite semantics. |
| XX | Simulation | ✅ PASS | HAL `SimulationDriver` produces valid `HalStatus`. CU has sim-mode loop (`cfg(not(feature = "rt"))`). |
| XXI | Fault Injection | ⏳ PARTIAL | Short RT stability test (FR-078) covers timing. Full fault injection deferred to future spec. |
| XXII | Supply Chain | ⏳ N/A | Build integrity not in scope for this feature. |
| XXIII | Perf Modeling | ✅ PASS | Latency budgets per segment (write ≤5µs, read ≤2µs). Criterion benchmarks. Static size assertions. |
| XXIV | Resource Isolation | ⏳ DEFERRED | CPU affinity, cgroups, DVFS deferred per A-007. Watchdog RT thread management in future spec. |
| XXV | Error Classification | ⏳ PARTIAL | ShmError (9 variants) and ConfigError defined with structured codes. Watchdog uses RECOVERABLE (restart+backoff). Full RECOVERABLE/DEGRADABLE/FATAL taxonomy not yet formalized across all modules — acceptable for integration scope. |
| XXVI | Implementation Phases | ✅ PASS | This spec covers Phase 1 (I,II,III,VII,IX) + Phase 2 (XI,XVII,XVIII) + Phase 3 (V,VI,XII,XIII) constitution principles. Phase 4 partially via VIII (shm_open 0o600, flock). |
| XXVII | Timing Test Methodology | ⏳ DEFERRED | FR-078 short RT stability test + criterion benchmarks exist. Full controlled environment (isolcpus, nohz_full, p99.9 reporting) deferred alongside XXIV (RT kernel setup) per A-007. |

**Gate result**: ✅ PASS — No violations. Six principles are partially addressed or deferred to downstream specs with explicit tracking (XXI, XXII, XXIV, XXV, XXVI, XXVII).

## Project Structure

### Documentation (this feature)

```text
specs/006-rt-shm-integration/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── segments.md      # All 15 SHM segment contracts
│   └── config.md        # Config file contracts (config.toml, machine.toml, io.toml, axis)
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
evo_common/src/
├── consts.rs                    # NEW: MAX_AXES, MAX_DI/DO/AI/AO, DEFAULT_CONFIG_PATH, CYCLE_TIME_US
├── config.rs                    # EXTEND: ConfigLoader + load_config_dir() + SystemConfig
├── lib.rs                       # EXTEND: pub mod consts
├── prelude.rs                   # EXTEND: re-export top-10 symbols
├── hal/
│   ├── consts.rs                # TRIM: remove moved constants, remove HAL_SERVICE_NAME
│   ├── config.rs                # TRIM: remove duplicate AnalogCurve, keep HAL-specific types
│   ├── driver.rs                # KEEP
│   └── types.rs                 # KEEP: HalCommands, HalStatus
├── io/
│   ├── config.rs                # KEEP: IoConfig, IoGroup, IoPoint, AnalogCurve (canonical)
│   ├── registry.rs              # KEEP: IoRegistry
│   └── role.rs                  # KEEP: IoRole
├── control_unit/
│   ├── shm.rs                   # KEEP: 6 existing CU-internal payload structs (ShmBundle, ShmReaders, etc. — NOT P2P segment types). All 15 P2P segment types live in shm/segments.rs per SC-013.
│   └── ...                      # KEEP: all CU shared types
└── shm/
    ├── mod.rs                   # EXTEND: pub mod p2p, segments, conversions, io_helpers
    ├── p2p.rs                   # EXTEND: + TypedP2pWriter<T>, TypedP2pReader<T>, SegmentDiscovery, ShmError
    ├── segments.rs              # NEW: 9 additional segment types (skeleton/placeholder)
    ├── conversions.rs           # NEW: HalStatus↔HalToCuSegment, CuToHalSegment↔HalCommands
    ├── io_helpers.rs            # NEW: get_di(), set_do() bit-packed helpers
    ├── config.rs                # DELETE (empty placeholder)
    └── consts.rs                # TRIM: remove EVO_SHM_MAGIC, keep P2P constants

evo/src/
└── main.rs                      # REWRITE: sync watchdog with spawn, waitpid, restart, SHM cleanup

evo_hal/src/
├── main.rs                      # EXTEND: --config-dir, P2P writer/reader setup
├── core.rs                      # EXTEND: fill TODO SHM read/write with P2P
├── lib.rs                       # KEEP
├── shm.rs                       # DELETE (dead HalShmData)
├── main_old.rs                  # DELETE (legacy)
├── module_status.rs             # DELETE/REPLACE with P2P-based reporting
└── drivers/simulation/          # KEEP

evo_control_unit/src/
├── main.rs                      # REWRITE: instantiate CycleRunner, enter RT loop
├── shm/
│   ├── writer.rs                # MIGRATE → evo_common::shm::p2p (delete local)
│   ├── reader.rs                # MIGRATE → evo_common::shm::p2p (delete local)
│   └── mod.rs                   # TRIM: ShmBundle stays, imports from evo_common
└── engine/
    ├── runner.rs                # EXTEND: nix 0.30 API fixes
    └── config_loader.rs         # EXTEND: use evo_common ConfigLoader

evo_shared_memory/               # DELETE ENTIRE CRATE

evo_grpc/src/main.rs             # REWRITE: P2P skeleton (6 segments)
evo_mqtt/src/main.rs             # REWRITE: P2P skeleton (3 readers)
evo_recipe_executor/src/main.rs  # REWRITE: P2P skeleton (7 segments)
evo_api/src/main.rs              # EXTEND: no SHM, placeholder for gRPC+MQTT client
evo_diagnostic/src/main.rs       # EXTEND: no SHM, placeholder
evo_dashboard/src/main.rs        # EXTEND: no SHM, placeholder

config/
├── config.toml                  # NEW: system/program configuration
├── machine.toml                 # REWRITE: global only, no axes, no I/O
├── io.toml                      # NEW: copy from specs/005-control-unit/io.toml
├── axis_01_x.toml               # NEW: migrated from test_8axis.toml
├── axis_02_y.toml               # NEW
├── axis_03_z.toml               # NEW
├── axis_04_a.toml               # NEW
├── axis_05_b.toml               # NEW
├── axis_06_c.toml               # NEW
├── axis_07_spindle.toml         # NEW
├── axis_08_tailstock.toml       # NEW
├── test_8axis.toml              # DELETE (migrated)
├── test_cu.toml                 # DELETE or UPDATE
└── test_io.toml                 # DELETE (replaced by io.toml)

Cargo.toml                       # EXTEND: [workspace.dependencies], remove evo_shared_memory
```

**Structure Decision**: Existing Rust workspace structure preserved. No new crates added. Net -1 crate (evo_shared_memory removed). All new code lands in `evo_common` (library-first) with thin binary wiring in each crate.

## Implementation Phases

### Phase A: Foundation (P2P Library + Constants + Config) — FR-001 through FR-009, FR-080 through FR-083, FR-050 through FR-059a

**Goal**: Single P2P transport API in `evo_common`, unified constants, per-axis config loading with auto-discovery.

1. **A.1** — Create `evo_common/src/consts.rs`: Move MAX_AXES, MAX_DI/DO/AI/AO, DEFAULT_CONFIG_PATH, CYCLE_TIME_US from hal/consts.rs. Remove HAL_SERVICE_NAME. Update all imports.
2. **A.2** — Migrate `OutboundWriter<T>` → `TypedP2pWriter<T>` in `evo_common::shm::p2p`: Move from CU, add `flock(LOCK_EX|LOCK_NB)` on create, `shm_open` mode `0o600`, `shm_unlink` on Drop.
3. **A.3** — Migrate `InboundReader<T>` → `TypedP2pReader<T>` in `evo_common::shm::p2p`: Move from CU, add `flock(LOCK_SH|LOCK_NB)` on attach, destination + version validation.
4. **A.4** — Migrate `SegmentError` → `ShmError` in `evo_common::shm::p2p`: All 9 variants per FR-004.
5. **A.5** — Implement `SegmentDiscovery` in `evo_common::shm::p2p` (FR-007): enumerate `/dev/shm/evo_*`, flock probe for writer liveness.
6. **A.6** — Create `evo_common/src/shm/io_helpers.rs` (FR-015): `get_di()`, `set_do()` with bit-packing.
7. **A.7** — Remove `EVO_SHM_MAGIC` from `evo_common::shm::consts` (FR-082). Remove empty `evo_common/src/shm/config.rs` (FR-060).
8. **A.8** — Unify `AnalogCurve` — keep in `evo_common::io::config`, delete duplicate from `evo_common::hal::config` (FR-081).
9. **A.9** — Create `config.toml` schema: `SystemConfig` struct with `WatchdogConfig` and per-program stubs (FR-059a).
10. **A.10** — Implement per-axis auto-discovery in `ConfigLoader`: glob `axis_*_*.toml`, validate NN↔id, duplicate detection (FR-055, FR-055a, FR-056).
11. **A.11** — Migrate reference configs: split `test_8axis.toml` into 8 axis files, update `machine.toml`, create `io.toml` in `config/`, create `config.toml` (FR-058).
12. **A.12** — Update `evo_common::prelude` with top-10 exports (FR-076).
13. **A.13** — Tests: P2P unit tests (create, attach, read, write, heartbeat, version hash, destination, flock, stale). Config auto-discovery tests. io_helpers tests.

### Phase B: Segment Types + Conversions — FR-010 through FR-014f, FR-035

**Goal**: All 15 segment types defined. Conversion functions for HAL↔SHM. All structs `#[repr(C)]` with static size assertions.

1. **B.1** — Define 9 new segment structs in `evo_common::shm::segments` (FR-014 through FR-014f): HalToMqtSegment, HalToRpcSegment, HalToReSegment, RpcToHalSegment, RpcToReSegment, ReToHalSegment, ReToMqtSegment, ReToRpcSegment, CuToRpcSegment. All `#[repr(C, align(64))]`, P2pSegmentHeader as first field, static_assert on sizes.
2. **B.2** — Implement `evo_common::shm::conversions` (FR-035): `HalStatus → HalToCuSegment` (bool→bitfield packing, DI bool→u64 bit-packing, AI scaled extraction). `CuToHalSegment → HalCommands` (reverse).
3. **B.3** — Tests: size/alignment assertions for all 15 types. Conversion round-trip tests. Version hash stability test.

### Phase C: Remove evo_shared_memory — FR-060 through FR-063

**Goal**: Delete entire crate. All dependents migrated. Zero references remain.

1. **C.1** — Update CU's `ShmBundle` to import `TypedP2pWriter`/`TypedP2pReader` from `evo_common` instead of local `shm/writer.rs`/`shm/reader.rs`. Delete CU's local writer.rs and reader.rs.
2. **C.2** — Remove `evo_shared_memory` dependency from: `evo/Cargo.toml`, `evo_hal/Cargo.toml`, `evo_grpc/Cargo.toml`, `evo_recipe_executor/Cargo.toml`, `evo_control_unit/Cargo.toml`.
3. **C.3** — Delete `evo_shared_memory/` directory. Remove from workspace members in root `Cargo.toml`.
4. **C.4** — Delete dead files: `evo_hal/src/main_old.rs`, `evo_hal/src/shm.rs`, `evo_hal/src/module_status.rs` (FR-060, FR-063).
5. **C.5** — Verify: `cargo build --workspace` succeeds. `grep -r "evo_shared_memory"` returns zero. No files in `evo_shared_memory/`.

### Phase D: Dependency Cleanup — FR-070 through FR-076

**Goal**: `[workspace.dependencies]`, version unification, unused dep removal, alias fix, edition 2024.

1. **D.1** — Add `[workspace.dependencies]` to root `Cargo.toml`: serde, toml, tracing, tracing-subscriber, heapless 0.9, nix 0.30, libc, thiserror, clap, criterion. Update all crate Cargo.toml to use `{ workspace = true }`.
2. **D.2** — Fix nix 0.30 API break in CU runner.rs: `MlockallFlags` → `MlockAllFlags`.
3. **D.3** — Remove unused deps: `parking_lot` from evo/evo_grpc/evo_recipe_executor, `tokio` from evo, `bitflags` from evo_control_unit (keep in evo_common), `static_assertions` from evo_control_unit (keep in evo_common).
4. **D.4** — Fix alias: rename `evo = { package = "evo_common" }` → `evo_common = { path = "../evo_common" }` in evo/Cargo.toml. Update all `use evo::` → `use evo_common::` (FR-074).
5. **D.5** — Migrate `evo_common` from `log` to `tracing` (FR-073).
6. **D.6** — Resolve `rt` feature flag in evo_control_unit: populate with `nix` feature gates or remove (FR-075).
7. **D.7** — Set `edition = "2024"` in all crate Cargo.toml files. Fix `unsafe_op_in_unsafe_fn` warnings — add explicit `unsafe {}` blocks inside unsafe fn bodies in P2P code.
8. **D.8** — Verify: `cargo build --workspace` zero warnings. `cargo tree -d` zero duplicates for key deps.

### Phase E: HAL SHM Integration — FR-030 through FR-036

**Goal**: HAL writes feedback to SHM, reads commands from SHM, every RT cycle. Loads io.toml. Uses IoRegistry.

1. **E.1** — HAL `main.rs`: add `--config-dir` CLI arg, load `config.toml` + `machine.toml` + `io.toml` + axis files via `evo_common::config`. Build `IoRegistry`.
2. **E.2** — HAL `core.rs`: create 4 `TypedP2pWriter`s at startup (evo_hal_cu, evo_hal_mqt, evo_hal_rpc, evo_hal_re). Attempt attach of 3 `TypedP2pReader`s (evo_cu_hal, evo_rpc_hal, evo_re_hal — non-blocking, retry periodically).
3. **E.3** — Fill `TODO: Write status to SHM` in core.rs run_loop: convert `HalStatus → HalToCuSegment`, call `writer.commit()`.
4. **E.4** — Fill `TODO: Read commands from SHM` in core.rs run_loop: read `CuToHalSegment → HalCommands` if segment exists, else default zero commands.
5. **E.5** — Implement DI bit-packing (FR-032): read `[bool; 1024]` → pack to `[u64; 16]` using `set_do()` helper.
6. **E.6** — Implement AI scaling (FR-033): read `[AnalogValue; 1024]` → extract `.scaled` to `[f64; 64]`.
7. **E.7** — Implement I/O role ownership enforcement (FR-036): HAL ignores RE commands for role-assigned pins.
8. **E.8** — Clean up dead code: remove unused public methods from HAL simulation (FR-064).
9. **E.9** — Refactor `DriverRegistry` global state → constructor-injection or per-test instances (FR-077).
10. **E.10** — Tests: HAL writes segment and external reader verifies data. HAL reads segment with known values. DI packing round-trip. AI scaling verification. Role ownership enforcement test.

### Phase F: CU Binary Integration — FR-040 through FR-044

**Goal**: CU binary instantiates CycleRunner and enters RT loop. Reads HAL feedback, writes commands.

1. **F.1** — Rewrite `evo_control_unit/src/main.rs`: parse `--config-dir`, load all configs, build IoRegistry, instantiate `CycleRunner::new()`, call `runner.run()`.
2. **F.2** — Extend `CycleRunner` with `IoRegistry` and `AxisControlState[MAX_AXES]` (FR-041).
3. **F.3** — Ensure cycle body reads `di_bank` and `ai_values` from `HalToCuSegment` and exposes to state machine logic (FR-042).
4. **F.4** — Fix MQT error_flags truncation: write as u32, not u16/u8 (FR-043).
5. **F.5** — Add periodic `try_attach_re()` and `try_attach_rpc()` — once per second (FR-044).
6. **F.6** — Tests: CU starts and creates segments. CU reads from evo_hal_cu. CU writes to evo_cu_hal. MQT status has full-width error_flags.

### Phase G: Watchdog — FR-020 through FR-028

**Goal**: `evo` binary spawns HAL→CU, monitors via waitpid, restarts with backoff, graceful shutdown.

1. **G.1** — Rewrite `evo/src/main.rs`: synchronous main loop, load `config.toml [watchdog]`, parse `--config-dir`.
2. **G.2** — Implement `spawn_module()`: `std::process::Command` for evo_hal and evo_control_unit with `--config-dir`.
3. **G.3** — Implement ordered startup: spawn HAL, poll `/dev/shm/evo_hal_cu` until exists + heartbeat > 0 (timeout from config), then spawn CU (FR-020).
4. **G.4** — Implement process monitoring: `waitpid(WNOHANG)` poll loop, detect child exit/crash (FR-021).
5. **G.5** — Implement restart logic: exponential backoff (100ms → 30s), max 5 restarts, stable-run reset timer, single CRITICAL log on exhaustion (FR-022).
6. **G.6** — Implement graceful shutdown: SIGTERM/SIGINT handler, CU→HAL shutdown order, SIGKILL fallback, SHM cleanup (FR-023, FR-026).
7. **G.7** — Implement orphan cleanup on startup: enumerate `/dev/shm/evo_*`, flock probe, `shm_unlink` dead segments (FR-024).
8. **G.8** — Create `WatchdogTrait` in `evo_common` if not exists. Implement in `evo` binary (FR-027).
9. **G.9** — Optional: P2P header heartbeat monitoring for hang detection (FR-028).
10. **G.10** — Tests: spawn and verify segments. Crash child and verify restart. Graceful shutdown with SHM cleanup. Orphan cleanup test.

### Phase H: Stub Programs P2P Skeleton — FR-090 through FR-092

**Goal**: All stub programs have P2P initialization. Compiles and runs.

1. **H.1** — `evo_mqtt/src/main.rs`: attach 3 readers (evo_cu_mqt, evo_hal_mqt, evo_re_mqt). Placeholder read loop.
2. **H.2** — `evo_grpc/src/main.rs`: create 3 writers (evo_rpc_cu, evo_rpc_hal, evo_rpc_re), attach 3 readers (evo_cu_rpc, evo_hal_rpc, evo_re_rpc). Placeholder gRPC server.
3. **H.3** — `evo_recipe_executor/src/main.rs`: create 4 writers (evo_re_cu, evo_re_hal, evo_re_mqt, evo_re_rpc), attach 3 readers (evo_cu_re, evo_hal_re, evo_rpc_re).
4. **H.4** — `evo_api/src/main.rs`: no SHM, placeholder for gRPC client + MQTT subscriber.
5. **H.5** — `evo_diagnostic/src/main.rs` and `evo_dashboard/src/main.rs`: no SHM, placeholder.
6. **H.6** — All stubs depend only on `evo_common`. Error handling: log + graceful exit on failure.
7. **H.7** — Tests: all stubs compile. Cargo build workspace succeeds.

### Phase I: Integration Testing + CI — FR-078

**Goal**: End-to-end pipeline test. Short RT stability test. Benchmarks.

1. **I.1** — Integration test: run `evo --config-dir config/`, verify HAL and CU start, segments active, heartbeats incrementing.
2. **I.2** — Data round-trip test: HAL writes known positions, verify CU reads matching values within 2 cycles.
3. **I.3** — Crash recovery test: SIGKILL HAL, verify watchdog restarts both, system recovers.
4. **I.4** — Graceful shutdown test: SIGTERM watchdog, verify clean exit within 5 seconds.
5. **I.5** — Config agreement test: both HAL and CU load same configs, compare IoRegistry outputs.
6. **I.6** — Short RT stability test (FR-078): CycleRunner 10,000 cycles, zero deadline misses.
7. **I.7** — P2P latency benchmarks: criterion for write ≤5µs, read ≤2µs on ≤8KB segments.
8. **I.8** — Final verification: all SC-001 through SC-014 success criteria met.

## Phase Dependencies

```
Phase A (Foundation) ──┐
                       ├── Phase B (Segments) ──┐
                       │                        ├── Phase C (Remove evo_shared_memory)
                       │                        │           │
Phase D (Deps) ────────┘                        │           │
                                                ├── Phase E (HAL SHM)
                                                │           │
                                                ├── Phase F (CU Binary)
                                                │           │
                                                │           ├── Phase G (Watchdog)
                                                │           │
                                                ├── Phase H (Stubs)
                                                │
                                                └── Phase I (Integration)
```

- **A** and **D** can proceed in parallel (A is code, D is Cargo.toml)
- **B** depends on A (needs TypedP2pWriter/Reader and consts)
- **C** depends on A+B (needs P2P lib + segment types to replace evo_shared_memory deps)
- **E** depends on A+B+C+D.2 (needs P2P lib, segments, evo_shared_memory gone, config infra from A.9–A.11, nix 0.30 fix from D.2)
- **F** depends on A+B+C+D.2 (same as E — nix 0.30 MlockAllFlags fix required for CU compilation)
- **E** and **F** can proceed in parallel
- **G** depends on E+F (needs both HAL and CU functional)
- **H** depends on A+B+C (needs P2P lib + segment types)
- **H** can proceed in parallel with E/F
- **I** depends on all phases

## Complexity Tracking

No constitution violations. No complexity justifications needed.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (none)    | —          | —                                   |
