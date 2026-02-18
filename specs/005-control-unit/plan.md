# Implementation Plan: Control Unit — Axis Control Brain

**Branch**: `005-control-unit` | **Date**: 2026-02-08 | **Spec**: [spec.md](spec.md)  
**Input**: Feature specification from `/specs/005-control-unit/spec.md`

## Summary

Implement the Control Unit — the real-time axis control brain for the EVO industrial motion control system. The CU runs a deterministic <1ms cycle that reads sensor feedback from HAL, executes hierarchical state machines (5 levels: MachineState → SafetyState → 6 orthogonal per-axis states → safety flags → error flags), runs a universal position control engine (PID + feedforward + DOB + filters), produces per-axis `ControlOutputVector` for HAL, monitors safety peripherals, and reports telemetry via P2P shared memory segments. Zero dynamic allocation in the RT loop; SHM-only diagnostics; hard deadline enforcement.

## Technical Context

**Language/Version**: Rust 2025 (edition 2024)  
**Primary Dependencies**: evo_common (shared types), evo_shared_memory (P2P SHM — requires migration), serde + toml (config), nix (mlock/RT scheduling), libc (shm_open/mmap)  
**Storage**: TOML config files (read at startup); no persistent storage in RT loop  
**Testing**: cargo test (unit), integration tests with simulated HAL SHM, criterion (benchmarks), timing tests on PREEMPT_RT  
**Development Platform**: Linux x86_64 (standard kernel) — simulation mode with OSAL logical time  
**Target Platform**: Linux x86_64 with PREEMPT_RT kernel — production RT scheduling  
**Project Type**: Workspace member crate (`evo_control_unit`) within existing Cargo workspace  
**Performance Goals**: <1ms cycle time for 64 axes (Class A Critical, <0.01% miss rate); SAFETY_STOP reaction within 1 cycle  
**Constraints**: Zero heap allocation in RT loop; pre-allocated fixed-size arrays; mlock'd memory; dedicated CPU core (cgroup isolation)  
**Scale/Scope**: Up to 64 axes, 6 P2P SHM segments, 6 orthogonal state machines per axis, ~143 functional requirements

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. RT Performance Guarantees | ✅ PASS | Cycle <1ms documented as Class A Critical (<0.01% miss rate). FR-138 single overrun → SAFETY_STOP. Deadline ID: `DL-CU-CYCLE`. |
| II. Test-First & Verification | ✅ PASS | 7 user stories with acceptance scenarios. Unit + contract + timing + integration + system tests planned. Coverage target: ≥90% state machine logic, ≥85% control engine. |
| III. Code Quality & Static Analysis | ✅ PASS | Clippy + cargo deny in CI. All public interfaces have error contracts (FR-090). Complexity managed by orthogonal state machine decomposition. |
| IV. Consistent Interface Experience | ✅ PASS | Error messages: stable codes + structured payload (FR-090). Config parameters centrally declared (FR-141, FR-142). SHM diagnostics via evo_cu_mqt (FR-134). |
| V. Performance & Resource Bounds | ✅ PASS | SC-001 through SC-009 define measurable budgets. Per-module CPU budget to be documented in research.md. |
| VI. Observability & Traceability | ✅ PASS | SHM-only diagnostics (FR-134). Telemetry snapshot via evo_cu_mqt. Trace points: cycle start/end, state transitions, deadline violations, errors. Overhead <2% per constitution. |
| VII. Config & Versioning | ✅ PASS | Config immutable after STARTING during normal operation (FR-138a). Hot-reload supported exclusively in SAFETY_STOP state via atomic shadow-config swap (FR-144–FR-147). TOML config with schema validation (FR-142). Struct version hash in SHM headers (FR-130d). All numeric parameters have min/max bounds as const in evo_common (FR-156). Forward-compatible loading: serde(default) + ignore unknown fields (FR-157). |
| VIII. Security & Safety | ✅ PASS | All SHM inputs validated (segment address, struct hash, heartbeat). No network endpoints in CU. Least privilege: only SHM + clock access needed. |
| IX. Simplicity & Minimal Dependencies | ✅ PASS | Pure functions on RT path (control engine). Data-oriented: fixed-size arrays, no dynamic dispatch. Dependencies: evo_common, evo_shared_memory, serde, toml, nix, libc — all deterministic. |
| X. Change Review | ⏳ N/A | Applies at PR time. Plan: dual reviewer for timing-sensitive changes. |
| XI. Spec-Driven Development | ✅ PASS | This plan derives directly from spec.md with FR traceability. |
| XII. Error Handling & Degradation | ✅ PASS | FR-091/FR-092 define non-critical (axis-local) vs critical (global SAFETY_STOP). FR-130c staleness detection. FR-139 tiered startup. All error paths bounded. |
| XIII. Lifecycle & State Management | ✅ PASS | Explicit state transitions (FR-001/FR-002). All allocation in STARTING (FR-138a). Deterministic shutdown via POWERING_OFF (FR-022). Hot-reload: permitted only in SAFETY_STOP state (FR-144–FR-147) — RT loop is already suspended, so timing guarantees are trivially preserved. Atomic swap with rollback. ≤120ms worst-case. |
| XIV. Memory Management | ✅ PASS | Pre-allocated cache-aligned arrays (FR-138a). Fixed-size `[AxisState; 64]`. No pointer chasing on hot path. mlock/hugetlbfs. |
| XV. IPC & Synchronization | ✅ PASS | Zero-copy P2P SHM (FR-130). Lock-free even/odd versioning from evo_shared_memory. No mutexes in RT loop. |
| XVI. Architectural Governance | ✅ PASS | Architecture levels documented in spec. This plan serves as ADR. |
| XVII. Modular Library-First | ⚠️ JUSTIFIED | CU is an application crate, not a library. However: all shared types live in evo_common (FR-140), control engine can be extracted to a library crate in future. See Complexity Tracking. |
| XVIII. Deterministic Interfaces | ✅ PASS | All SHM contracts have bounded execution time. Diagnostic access via non-blocking SHM read. |
| XIX. Non-RT Isolation | ✅ PASS | CU is pure RT — no logging, no file I/O, no dashboard. All non-RT consumers read evo_cu_mqt from separate processes. |
| XX. Simulation Support | ✅ PASS | CU uses an OS Abstraction Layer (OSAL) that replaces wall-clock timing (`clock_nanosleep`, `sched_setscheduler`) with logical time steps on non-RT platforms. HAL simulation mode provides simulated SHM data. Mode selected at initialization (compile-time feature flag `rt` or runtime `--simulation`), zero overhead in production. See Simulation Mode section below. |
| XXI. Fault Injection | ⏳ DEFERRED | Phase 4 per constitution. Fault catalog entries for: cycle overrun, SHM corruption, stale heartbeat, sensor conflict. |
| XXII. Supply Chain | ⏳ DEFERRED | Phase 4 per constitution. Cargo.lock pinned. No new non-deterministic deps. |
| XXIII. Performance Modeling | ✅ PASS | Cycle budget decomposition in research.md. CI regression tests for cycle time. |
| XXIV. Resource Isolation | ✅ PASS | Dedicated CPU core, cgroup isolation, IRQ affinity. DVFS locked. FR-138a mlock. |
| XXV. Error Classification | ✅ PASS | FR-090 taxonomy: PowerError (RECOVERABLE/FATAL), MotionError (RECOVERABLE/FATAL), CommandError (RECOVERABLE), GearboxError, CouplingError. Response policies per FR-091/FR-092. |
| XXVI. Implementation Phases | ✅ PASS | Phase 1-2 principles (I,II,III,VII,IX,XI,XVII,XVIII) addressed in this plan. Phase 3-4 deferred items tracked. |
| XXVII. Timing Test Methodology | ✅ PASS | Isolated hardware, locked CPU freq, isolcpus. Statistical: mean/p95/p99/p99.9 over ≥10,000 cycles. Separate CI runner for timing. |

**GATE RESULT: PASS** — All Phase 1-3 principles satisfied. Phase 4 items (XXI, XXII) deferred per constitution phasing (XXVI).

## Project Structure

### Documentation (this feature)

```text
specs/005-control-unit/
├── plan.md              # This file
├── research.md          # Phase 0: technology decisions & cycle budget
├── data-model.md        # Phase 1: entity definitions & relationships
├── quickstart.md        # Phase 1: build/run/test instructions
├── contracts/           # Phase 1: SHM struct definitions & API contracts
│   ├── shm_segments.rs  # P2P segment struct definitions (Rust pseudo-code)
│   └── state_machines.md # State transition contracts
└── tasks.md             # Phase 2: implementation tasks (NOT created here)
```

### Source Code (repository root)

```text
evo_control_unit/
├── Cargo.toml                  # Crate manifest (existing, to be updated)
└── src/
    ├── main.rs                 # Entry point: RT setup, mlock, cycle loop
    ├── lib.rs                  # Public API for testing
    ├── config.rs               # TOML config loading & validation
    ├── cycle.rs                # RT cycle: read → process → write
    ├── state.rs                # State module root
    ├── state/
    │   ├── machine.rs          # LEVEL 1: MachineState transitions
    │   ├── safety.rs           # LEVEL 2: SafetyState management
    │   ├── axis.rs             # LEVEL 3: AxisState container (all 6 orthogonal)
    │   ├── power.rs            # PowerState transitions (POWERING_ON/OFF sequences)
    │   ├── motion.rs           # MotionState transitions
    │   ├── operational.rs      # OperationalMode management
    │   ├── coupling.rs         # CouplingState + sync + error propagation
    │   ├── gearbox.rs          # GearboxState transitions
    │   └── loading.rs          # LoadingState per-axis
    ├── safety.rs               # Safety module root
    ├── safety/
    │   ├── flags.rs            # AxisSafetyState flag evaluation
    │   ├── peripherals.rs      # Tailstock, locking pin, brake, guard logic
    │   ├── stop.rs             # SAFETY_STOP detection + per-axis SafeStopCategory
    │   └── recovery.rs         # Reset + authorization sequence
    ├── control.rs              # Control engine root
    ├── control/
    │   ├── pid.rs              # PID with anti-windup + derivative filter
    │   ├── feedforward.rs      # Velocity/acceleration feedforward + friction
    │   ├── dob.rs              # Disturbance observer
    │   ├── filters.rs          # Notch filter + low-pass filter
    │   ├── output.rs           # ControlOutputVector assembly
    │   └── lag.rs              # Lag monitoring + coupling lag diff
    ├── command.rs              # Command processing root
    ├── command/
    │   ├── source_lock.rs      # Source locking logic
    │   ├── arbitration.rs      # Command arbitration (RE vs RPC)
    │   └── homing.rs           # Homing supervision (6 methods)
    ├── shm.rs                  # SHM integration root
    ├── shm/
    │   ├── segments.rs         # P2P segment connection & lifecycle
    │   ├── reader.rs           # Inbound segment reading (hal_cu, re_cu, rpc_cu)
    │   └── writer.rs           # Outbound segment writing (cu_hal, cu_mqt, cu_re)
    ├── error.rs                # Error module root
    └── error/
        └── propagation.rs      # Hierarchical error propagation rules

evo_common/src/
├── control_unit.rs             # NEW: CU-specific shared types module root (FR-140, FR-141)
├── control_unit/
│   ├── config.rs               # CU configuration structures
│   ├── state.rs                # All state machine enums
│   ├── error.rs                # All error enums (PowerError, MotionError, etc.)
│   ├── safety.rs               # SafeStopCategory, AxisSafetyState, peripherals
│   ├── control.rs              # UniversalControlParameters, ControlOutputVector
│   ├── command.rs              # CommandError, AxisSourceLock, source types
│   └── homing.rs               # HomingMethod, HomingConfig, method params
└── shm/
    └── p2p.rs                  # NEW: P2P segment header (heartbeat, version hash)

evo_control_unit/tests/
├── unit/                       # Unit tests (state transitions, control math)
├── contract/                   # Contract tests (SHM struct layout, config schema)
├── integration/                # Integration tests (full cycle with mock SHM)
└── timing/                     # Timing tests (cycle budget, deadline compliance)
```

**Structure Decision**: Single workspace crate (`evo_control_unit`) with internal module decomposition matching the 5-level architecture. Shared types in `evo_common::control_unit` (FR-140). P2P SHM header extensions in `evo_common::shm::p2p`. Test directories mirror verification hierarchy from constitution Principle II.

## Simulation Mode (Constitution XX)

The Control Unit MUST run on standard Linux (no PREEMPT_RT) for development and testing. An **OS Abstraction Layer (OSAL)** replaces platform-specific RT primitives with deterministic simulation equivalents:

| RT Primitive | Production (PREEMPT_RT) | Simulation (standard kernel) |
|---|---|---|
| `clock_nanosleep(TIMER_ABSTIME)` | Wall-clock sleep to next cycle boundary | Logical time advance — instant return, `cycle_counter += 1` |
| `sched_setscheduler(SCHED_FIFO, 80)` | Real FIFO RT scheduling | No-op (best-effort scheduling sufficient for simulation) |
| `mlockall(MCL_CURRENT \| MCL_FUTURE)` | Lock all pages in RAM | No-op (page faults acceptable in simulation) |
| `sched_setaffinity` / cgroup | Pin to dedicated CPU core | No-op (no core isolation needed) |
| Cycle overrun detection | `CLOCK_MONOTONIC` wall-clock comparison | Logical time — overruns detectable via budget counter, not wall clock |

**Selection mechanism**: Compile-time feature flag `rt` (default: off) or runtime CLI `--simulation`. Mode is an early initialization decision with zero runtime overhead — no `if simulation {}` checks in the cycle loop; OSAL functions are monomorphized at compile time.


## Complexity Tracking

> Violations that must be justified:

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| XVII: CU is application, not library | CU is inherently an application — it owns the RT loop, SHM connections, and process lifecycle. Extractable parts (control engine, state machines) are pure-function modules that CAN be librarized later. | Making CU a library with a thin binary wrapper adds indirection without benefit at this stage. The internal modules (control/, state/) are testable independently. |
| Hard RT deadline (single overrun = SAFETY_STOP) | Constitution says "soft RT" with miss rates. Spec mandates hard deadline. This is MORE conservative than constitution requires. | Allowing any miss rate risks physical damage to machinery. Industrial safety requires hard deadline for motion control. |

## Post-Design Constitution Re-check

*Re-evaluated after Phase 1 design artifacts (data-model.md, contracts/, quickstart.md).*

| Principle | Post-Design Status | Design Evidence |
|-----------|-------------------|-----------------|
| I. RT Performance | ✅ PASS | Cycle budget decomposed: ~40µs/64 axes = 4% of 1ms budget (research.md Topic 11). SHM structs sized: evo_hal_cu ~2.3KB, evo_cu_hal ~3.3KB — both fit L1 cache. All structs `#[repr(C, align(64))]` for zero-copy. |
| II. Test-First | ✅ PASS | Test directory structure defined in quickstart.md. Contract tests cover SHM struct layout validation. Timing tests with criterion benchmarks. |
| III. Code Quality | ✅ PASS | All public types have documented invariants in data-model.md. State machine contracts define pre/post conditions for every transition. Cyclomatic complexity managed: each state machine is a separate module (max ~12 transitions per enum). |
| IV. Consistent Interface | ✅ PASS | SHM segment contracts define stable binary API. `evo_cu_mqt` live status snapshot provides structured observability. All config parameters declared in data-model.md with types and defaults. |
| V. Resource Bounds | ✅ PASS | Memory budget: ~16.6KB axis state + 2.3KB input + 3.3KB output + 4.5KB diagnostic ≈ 26.7KB hot data. CPU: 4% of 1ms cycle budget. |
| VI. Observability | ✅ PASS | evo_cu_mqt segment carries per-cycle telemetry snapshot (machine state, axis states, safety flags, timing). CU writes snapshot every N cycles (configurable) — overhead minimal. No event ring buffer — only live status (Session 2026-02-09). |
| VII. Config & Versioning | ✅ PASS | `struct_version_hash<T>()` const fn defined in contracts. ControlUnitConfig fully specified in data-model.md. Config immutable after STARTING; hot-reload exclusively in SAFETY_STOP via FR-144–FR-147 (atomic shadow-config swap with rollback). All numeric params bounded by const MIN/MAX in evo_common (FR-156). Forward-compatible: serde(default) + ignore unknown fields (FR-157). |
| VIII. Security | ✅ PASS | SHM header magic `"EVO_P2P\0"` + version_hash validation. Source locking prevents unauthorized axis control. No external inputs except validated SHM. |
| IX. Simplicity | ✅ PASS | ~30 types total. No generics on hot path. All enums `#[repr(u8)]` — simple match dispatch. heapless for bounded collections (only in config). No new external deps beyond plan. |
| XI. Spec-Driven | ✅ PASS | Every type in data-model.md traced to FR numbers. Every SHM field mapped to spec requirement. State machine contracts trace to FR transition rules. |
| XII. Error Handling | ✅ PASS | 5 error bitflag sets fully specified (PowerError, MotionError, CommandError, GearboxError, CouplingError). CRITICAL flags identified with → SAFETY_STOP propagation. Recovery requires explicit reset. |
| XIII. Lifecycle | ✅ PASS | 7 MachineState transitions fully contracted with guards and actions. Power-on sequence: 7 steps documented. Shutdown via PoweringOff deterministic. All allocation in Starting state. Hot-reload in SAFETY_STOP only (FR-144–FR-147): RT loop halted, atomic swap, ≤120ms. |
| XIV. Memory | ✅ PASS | All RT structs `#[repr(C)]` with explicit padding. AxisState ~260 bytes × 64 = ~16.6KB (fits L1$). ControlOutputVector 32 bytes (half cache line). No pointer chasing. |
| XV. IPC | ✅ PASS | 6 P2P segments fully contracted in contracts/shm-segments.md. Lock-free via even/odd write_seq. Zero-copy binary. Heartbeat staleness thresholds defined (3 cycles for RT, 1000 for non-RT). |
| XVII. Modular Library-First | ⚠️ JUSTIFIED | Same as pre-design. Additionally: shared types in evo_common::control_unit enable other crates to depend on type definitions without depending on CU binary. |
| XVIII. Deterministic Interfaces | ✅ PASS | All SHM reads/writes are O(1) bounded (fixed-size memcpy). Diagnostic snapshot is end-of-cycle atomic write. |
| XIX. Non-RT Isolation | ✅ PASS | evo_cu_mqt is the ONLY output to non-RT consumers. 10ms update rate decoupled from 1ms RT cycle. Non-RT processes crash without affecting CU. |
| XX. Simulation | ✅ PASS | OSAL abstracts RT primitives: `clock_nanosleep` → logical time step, `sched_setscheduler` → no-op, `mlockall` → no-op. Simulation mode is early init decision (feature flag or CLI). CU reads SHM regardless of HAL mode. Logical time fully controllable: step-by-step, accelerated, real-time pace. Identical inputs produce identical outputs (deterministic). |
| XXIII. Performance Modeling | ✅ PASS | Full budget: SHM read ~2µs + safety check ~1µs + state machines ~5µs + control engine ~25µs (64 axes) + SHM write ~5µs + diagnostic ~2µs = ~40µs total (4% of 1ms). |
| XXIV. Resource Isolation | ✅ PASS | Single-threaded RT loop on dedicated core. mlock'd memory. No shared mutexes. SHM access is the only cross-process interaction. |
| XXV. Error Classification | ✅ PASS | Bitflag errors classified: each bit documented as RECOVERABLE or CRITICAL(→SAFETY_STOP). Response policy per error type in contracts/state-machines.md. |
| XXVII. Timing Test | ✅ PASS | Benchmarks defined in quickstart.md: cycle_benchmark and pid_benchmark. Statistical analysis: mean/p95/p99/p99.9 over ≥10,000 iterations. |

**POST-DESIGN GATE: PASS** — No new violations found. All Phase 1 design artifacts satisfy constitution requirements. Memory budget (~28KB hot) and CPU budget (~4% of 1ms) are well within limits.

## Generated Artifacts

| Artifact | Path | Description |
|----------|------|-------------|
| Feature Spec | [spec.md](spec.md) | ~1070-line specification, FR-001 through FR-157 |
| Implementation Plan | [plan.md](plan.md) | This file |
| Research | [research.md](research.md) | 11 technology topics, 1473 lines |
| Data Model | [data-model.md](data-model.md) | ~40 entity definitions with Rust types (includes IoRole, IoPoint, IoRegistry) |
| SHM Contracts | [contracts/shm-segments.md](contracts/shm-segments.md) | 6 P2P segment struct layouts |
| State Machine Contracts | [contracts/state-machines.md](contracts/state-machines.md) | 8 state machines with transition tables |
| I/O Config Contract | [contracts/io-config.md](contracts/io-config.md) | io.toml schema, role resolution, validation rules (V-IO-1 through V-IO-7) |
| I/O Config Example | [io.toml](io.toml) | Example io.toml with Safety, Pneumatics, Axes, Operator Panel, Diagnostics groups |
| Quickstart | [quickstart.md](quickstart.md) | Build/run/test instructions |
| Agent Context | `.github/agents/copilot-instructions.md` | Updated with CU technology stack |
