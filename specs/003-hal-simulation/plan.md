# Implementation Plan: HAL Core + Simulation Driver

**Branch**: `003-hal-simulation` | **Date**: 2025-12-10 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/003-hal-simulation/spec.md`

## Summary

Implement a Hardware Abstraction Layer (HAL) Core binary that manages pluggable HAL drivers, with a Simulation Driver as the first driver implementation. HAL Core owns the RT loop and SHM communication, while drivers implement the `HalDriver` trait to provide hardware-specific behavior. The simulation driver provides software-emulated motion control, referencing, and I/O for development and testing without physical hardware.

**Architecture**: HAL Core (driver manager) + HAL Drivers (modules via `HalDriver` trait)  
**Components**:
- `evo_hal` - Single crate with core binary and driver modules
- `evo_hal/drivers/simulation/` - Simulation driver implementing `HalDriver` trait

## Technical Context

**Language/Version**: Rust 1.75+ (edition 2024)  
**Primary Dependencies**: `evo_common` (config types, constants), `evo_shared_memory` (SHM library), `serde`, `toml`, `bincode`, `clap`, `tracing`, `thiserror`  
**Storage**: TOML config files (machine.toml + per-axis files), binary state persistence file  
**Testing**: `cargo test` with unit, contract, and integration tests (TDD per Constitution II)  
**Target Platform**: Linux (Ubuntu 22.04+, standard or PREEMPT_RT kernel)  
**RT Detection**: Auto-detect via `sched_getscheduler()` - SCHED_FIFO/SCHED_RR = RT mode  
**Project Type**: Workspace crates (library + binary)  
**Performance Goals**: Configurable cycle time (default 1ms via `CYCLE_TIME_US`), <5% CPU usage on standard PC  
**Constraints**: <1μs per axis physics calculation, <100μs state persistence  
**Scale/Scope**: Max 64 axes, Max 1024 DI/DO/AI/AO (~48KB SHM)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle | Status | Evidence |
|-----------|--------|----------|
| I. Soft Real-Time Performance | ✅ PASS | 1ms cycle time documented (Class B deadline, <0.1% miss rate acceptable); bounded physics ops (<1μs/axis) |
| II. Test-First & Verification | ✅ PASS | Unit tests for physics, contract tests for SHM layout, integration tests for driver lifecycle |
| III. Code Quality | ✅ PASS | `#![deny(warnings)]`, clippy in CI; complexity tracked in Phase 2 tasks |
| IV. Operator Interface | ✅ PASS | CLI with `--config` arg; structured logging with JSON mode; stable error codes |
| V. Resource Bound Guarantees | ✅ PASS | CPU <5%, SHM ~48KB fixed; documented in spec |
| VI. Observability | ✅ PASS | Tracepoints: cycle start/end, deadline miss, state transition; lock-free logging |
| VII. Configuration | ✅ PASS | Immutable after init; TOML with serde validation; version in SHM header |
| VIII. Security | ✅ PASS | Minimal privileges; config validated at startup; memory safety via Rust |
| IX. Simplicity | ✅ PASS | Minimal dependencies (all justify net complexity reduction); pure physics functions |
| X. Change Review | N/A | Process applied during implementation PR |
| XI. Specification-Driven | ✅ PASS | All impl from spec.md; machine-readable contracts in /contracts/ |
| XII. Error Handling | ✅ PASS | Two-phase reset (FR-007a); graceful degradation; non-blocking error queues |
| XIII. Lifecycle | ✅ PASS | Explicit states (Init→Running→Shutdown); state persistence on background thread |

## Project Structure

### Documentation (this feature)

```text
specs/003-hal-simulation/
├── plan.md              # This file (Phase 1 output)
├── research.md          # Phase 0 output - research findings
├── data-model.md        # Phase 1 output - Rust structs and traits
├── quickstart.md        # Phase 1 output - developer guide
├── contracts/           # Phase 1 output - API contracts
│   ├── shm_layout.md    # SHM memory layout specification
│   └── hal_driver_trait.md  # HalDriver trait contract
└── tasks.md             # Phase 2 output (/speckit.tasks command)
```

### Source Code (repository root)

```text
# HAL - Core Binary with Driver Modules
evo_hal/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry point, driver loading
│   ├── lib.rs               # Library exports
│   ├── core.rs              # HalCore struct, RT loop management
│   ├── shm.rs               # SHM initialization and access
│   ├── driver_registry.rs   # Driver factory registration
│   └── drivers/             # HAL driver implementations
│       ├── mod.rs           # Driver module exports, registry
│       └── simulation/      # Simulation driver (this feature)
│           ├── mod.rs       # Driver exports
│           ├── driver.rs    # SimulationDriver impl HalDriver
│           ├── physics/
│           │   ├── mod.rs   # Physics module
│           │   ├── axis.rs  # Axis simulation model
│           │   └── referencing.rs  # Referencing state machine
│           ├── io.rs        # Digital/Analog I/O simulation
│           └── state.rs     # State persistence (bincode)
└── tests/
    ├── contract/            # SHM layout contract tests
    ├── unit/                # Physics calculations, state machine
    └── integration/         # Full system integration tests

# Shared Types (existing crate, additions)
evo_common/
├── src/
│   └── hal/
│       ├── mod.rs       # HAL module exports
│       ├── config.rs    # MachineConfig, AxisConfig, etc. (ADD)
│       ├── consts.rs    # MAX_AXES, MAX_DI, etc. (ADD)
│       ├── driver.rs    # HalDriver trait definition (ADD)
│       └── types.rs     # HalCommands, HalStatus, HalError (ADD)
```

**Structure Decision**: Single `evo_hal` crate with driver modules in `src/drivers/`. All drivers implement the `HalDriver` trait defined in `evo_common`. This keeps drivers co-located and simplifies the build while maintaining the pluggable architecture.

## Complexity Tracking

> No Constitution violations requiring justification. All principles pass.

## Phase 0 Artifacts

- [x] `research.md` - Research findings complete

## Phase 1 Artifacts

- [x] `data-model.md` - Rust structs, enums, traits
- [x] `contracts/shm_layout.md` - SHM memory layout (48KB)
- [x] `contracts/hal_driver_trait.md` - HalDriver trait contract
- [x] `quickstart.md` - Developer quick start guide

## Phase 2: Implementation (via /speckit.tasks)

Run `/speckit.tasks` to generate detailed implementation tasks based on this plan.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| [e.g., 4th project] | [current need] | [why 3 projects insufficient] |
| [e.g., Repository pattern] | [specific problem] | [why direct DB access insufficient] |
