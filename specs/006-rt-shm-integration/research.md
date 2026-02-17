# Research: 006-rt-shm-integration

**Date**: 2026-02-10 | **Branch**: `006-rt-shm-integration`

---

## R1: P2P Writer/Reader Migration — CU → evo_common

**Decision**: Move `OutboundWriter<T>` and `InboundReader<T>` from `evo_control_unit/src/shm/` to `evo_common::shm::p2p`, renaming to `TypedP2pWriter<T>` and `TypedP2pReader<T>`.

**Rationale**: The existing implementations in CU are already fully generic over `T: Copy`. They have no CU-specific logic — only dependencies on `P2pSegmentHeader`, `ModuleAbbrev`, and `struct_version_hash<T>()`, all already in `evo_common::shm::p2p`. The migration is a move + rename.

**Alternatives considered**:
- Re-implement from scratch in `evo_common` → rejected: existing code is correct and tested
- Keep in CU and have HAL depend on CU → rejected: creates wrong dependency direction

**Key findings**:
- `OutboundWriter::commit()` copies payload into a pre-allocated page-aligned buffer, applies header, increments heartbeat, and delegates to the library-level writer's version protocol
- `InboundReader::read()` does one-time P2P validation (magic + version hash), then per-read staleness detection via heartbeat comparison
- **No flock is used** despite spec 005 FR-130f requiring it. `flock(LOCK_EX|LOCK_NB)` for single-writer and `flock(LOCK_SH|LOCK_NB)` for single-reader must be added during migration
- `SegmentError` type also migrates — currently CU-local
- `ShmBundle` (creates all 6 CU segments) stays in CU as CU-specific initialization

---

## R2: flock Enforcement — Currently Missing

**Decision**: Add `flock` enforcement to `TypedP2pWriter` (LOCK_EX|LOCK_NB on create) and `TypedP2pReader` (LOCK_SH|LOCK_NB on attach) during migration.

**Rationale**: The current implementation relies on file-existence scanning and PID-based naming for collision detection. This is race-prone and does not prevent two writers on the same segment across restarts. `flock` provides atomic kernel-enforced exclusion.

**Alternatives considered**:
- PID-based `.meta` files → rejected: stale after crash, requires cleanup, the exact problem `evo_shared_memory` had (audit §4.6)
- Atomic CAS on header field → rejected: doesn't survive writer crash

**Implementation**:
- Writer: after `shm_open` + `mmap`, call `flock(fd, LOCK_EX | LOCK_NB)`. If `EWOULDBLOCK` → `ShmError::WriterAlreadyExists`
- Reader: after `shm_open` + `mmap`, call `flock(fd, LOCK_SH | LOCK_NB)`. If `EWOULDBLOCK` → `ShmError::ReaderAlreadyConnected`
- `Drop`: flock is automatically released when fd is closed
- Uses `nix::fcntl::flock` (available in nix 0.30)

---

## R3: nix 0.29 → 0.30 Migration

**Decision**: Unify all crates to `nix = "0.30"` via `[workspace.dependencies]`.

**Rationale**: Two crates use different nix versions (CU: 0.29, evo_shared_memory: 0.30.1). Since evo_shared_memory is being deleted, the only migration is CU from 0.29 to 0.30.

**Known breaking changes**:
- `MlockallFlags` → `MlockAllFlags` (capital A) — confirmed in CU's runner.rs L293
- Verify: `sched_setscheduler`, `sched_getscheduler`, `sched_param` — check 0.30 API
- Verify: `Pid::from_raw()` — may have changed error handling

**Scope**: Only `evo_control_unit/src/engine/runner.rs` is affected (all nix usage is concentrated there).

---

## R4: Rust 2024 Edition Considerations

**Decision**: Use `edition = "2024"` for all crates in the workspace.

**Rationale**: User explicitly requested Rust 2024+. Key changes relevant to this project:
- `unsafe_op_in_unsafe_fn`: unsafe operations inside `unsafe fn` now require explicit `unsafe {}` blocks — affects P2P raw pointer code in writer/reader
- Lifetime capture rules (RFC 3498): `impl Trait` in return position captures all in-scope lifetimes — verify P2P reader lifetime handling
- `gen` keyword reserved — no conflicts found in codebase
- `#[must_use]` improvements — aligns well with our `Result` returns

**Impact**: The P2P writer/reader use `unsafe` for raw pointer operations (`ptr::copy_nonoverlapping`, `ptr::write`). These must be wrapped in explicit `unsafe {}` blocks inside `unsafe fn` when migrating to edition 2024.

---

## R5: Segment Sizing — All Under 8KB

**Decision**: Keep existing segment sizes. All meet the ≤8KB latency benchmark requirement.

**Rationale**: Static assertions confirm:
| Segment | Size | Page-rounded |
|---------|------|-------------|
| HalToCuSegment | 2304 B | 4096 B |
| CuToHalSegment | 3328 B | 4096 B |
| CuToMqtSegment | 3712 B | 4096 B |
| ReToCuSegment | 2688 B | 4096 B |
| RpcToCuSegment | 128 B | 4096 B |
| CuToReSegment | 128 B | 4096 B |

**Note**: AI/AO arrays in segments use `[f64; 64]` (64 channels), not `[f64; 1024]` despite `MAX_AI = 1024`. This is because the segment struct was designed for the P2P hot path with fixed-size arrays. The full 1024-channel banks exist in the DI/DO bit-packed representation (`[u64; 16]` = 1024 bits). For AI/AO, only the first 64 channels are transmitted per cycle — sufficient for typical CNC machines.

**Alternatives considered**:
- Expand to `[f64; 1024]` for AI/AO → rejected: adds 15KB per segment (2× page faults), most machines use <16 analog channels

---

## R6: HalStatus → HalToCuSegment Conversion

**Decision**: Implement conversion in `evo_common::shm::conversions` as `impl From<&HalStatus> for HalToCuSegment` (or standalone fn).

**Rationale**: The mapping requires non-trivial translations:
| HalStatus field | HalToCuSegment field | Translation |
|----------------|---------------------|-------------|
| `axes[i].position` | `axes[i].position` | Direct f64 copy |
| `axes[i].velocity` | `axes[i].velocity` | Direct f64 copy |
| `axes[i].{ready,error,enabled,referenced,moving,in_position}` | `axes[i].drive_status` | Pack 6 bools → u8 bitfield |
| `axes[i].error_code` | `axes[i].fault_code` | Direct u32 copy |
| `digital_inputs: [bool; 1024]` | `di_bank: [u64; 16]` | Pack 1024 bools → 128 bytes bit-packed |
| `analog_inputs: [AnalogValue; 1024]` | `ai_values: [f64; 64]` | Extract `.scaled` from first 64 |

**Performance**: Conversion runs every cycle (~1kHz). Must be zero-alloc. The bool→bitfield packing is a tight loop of 1024 iterations — ~1µs on modern x86.

---

## R7: ConfigLoader — Per-Axis Auto-Discovery

**Decision**: Extend `evo_common::config::ConfigLoader` with a `load_config_dir(path: &Path) -> Result<FullConfig, ConfigError>` function that auto-discovers axis files.

**Rationale**: HAL already has partial support (loads axis files from explicit paths). CU has inline axis configs. The new approach:
1. Read `config.toml` → `SystemConfig`
2. Read `machine.toml` → `MachineConfig` (global only, no axes)
3. Read `io.toml` → `IoConfig` → build `IoRegistry`
4. Glob `axis_*_*.toml` in same directory → sort by NN prefix → parse each → validate id matches NN → check no duplicates → `Vec<AxisConfig>`
5. Return `FullConfig { system, machine, io, axes }`

**Alternatives considered**:
- Explicit axis file list in `machine.toml` → rejected: violates auto-discovery principle
- Subdirectory for axis files → rejected: violates flat-directory principle

---

## R8: Watchdog Rewrite Strategy

**Decision**: Rewrite `evo/src/main.rs` from scratch. The current implementation uses the old JSON-over-SHM paradigm and tokio async runtime — both are removed.

**Rationale**: The current watchdog (265 lines):
- Uses `evo_shared_memory` JSON API (being deleted)
- Uses `tokio` async runtime (spec says watchdog is sync — FR-072 removes tokio)
- Monitors SHM segments via JSON polling (wrong paradigm)
- Does NOT spawn child processes (the core requirement)

The new watchdog is synchronous, uses `std::process::Command` for spawning, `waitpid` for monitoring, and optionally reads P2P heartbeat headers for hang detection. All parameters come from `config.toml [watchdog]`.

---

## R9: Skeleton Segment Types — 9 Additional Structs

**Decision**: Define 9 additional `#[repr(C, align(64))]` segment types in `evo_common::shm::segments` as mostly-empty placeholders with `P2pSegmentHeader` as first field.

**Rationale**: The spec requires all 15 segment types. 6 already exist. The remaining 9 are:
1. `HalToMqtSegment` — superset of HalToCuSegment + cycle_time_ns + driver_state + DO/AO banks
2. `HalToRpcSegment` — request_id + result_code + error_msg
3. `HalToReSegment` — DI/DO/AI/AO banks + per-axis position/velocity/drive_ready
4. `RpcToHalSegment` — command_type + target_pin/axis + value + request_id
5. `RpcToReSegment` — empty placeholder (header only)
6. `ReToHalSegment` — set_do/set_ao commands + request_id
7. `ReToMqtSegment` — current_step + program_name + cycle_count + re_state
8. `ReToRpcSegment` — execution_progress + step_result + error_msg + request_id
9. `CuToRpcSegment` — superset of CuToMqtSegment + PID internals + cycle timing

All use fixed-size types only. Fixed-size string fields use `[u8; N]` arrays.

---

## R10: io.toml Format — Established Pattern

**Decision**: Use the existing `io.toml` format from spec 005 without modification.

**Rationale**: The format is well-defined in `specs/005-control-unit/io.toml` (125 lines) with comprehensive coverage: Safety, Pneumatics, Axes, Operator_Panel, Diagnostics groups. The `IoConfig`, `IoGroup`, `IoPoint`, `IoRole`, `IoRegistry` types already exist in `evo_common::io/`.

**Key observation**: The `io.toml` in `specs/005-control-unit/` is the only reference implementation. It needs to be copied to the `config/` directory alongside `machine.toml` for integration testing. Currently no `io.toml` exists in `config/`.
