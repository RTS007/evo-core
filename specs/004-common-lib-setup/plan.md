# Implementation Plan: Common Library Setup

**Branch**: `004-common-lib-setup` | **Date**: 2025-12-08 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/004-common-lib-setup/spec.md`

## Summary

Build a foundational `evo_common` library with modular architecture for shared constants and configuration loading across all EVO crates. Move existing constants from `evo_shared_memory` to establish single source of truth. Implement `ConfigLoader` trait for TOML-based configuration with composition pattern.

## Technical Context

**Language/Version**: Rust 2024 Edition (as per existing workspace)
**Primary Dependencies**: `serde` (serialization), `toml` (config parsing), `thiserror` (error handling)
**Storage**: TOML configuration files
**Testing**: `cargo test` with unit tests for constants and config loading
**Target Platform**: Linux (standard and PREEMPT_RT)
**Project Type**: Library crate within workspace monorepo
**Performance Goals**: Zero-cost abstractions; config loading is startup-only (not RT critical)
**Constraints**: No circular dependencies; `evo_common` must be dependency-free from other workspace crates
**Scale/Scope**: Foundation for 10+ crates in workspace

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

### Pre-Design Check

| Principle | Status | Notes |
|-----------|--------|-------|
| XVII (Modular Library-First) | ✅ PASS | Building standalone library with well-defined boundaries |
| IX (Simplicity & Minimal Dependencies) | ✅ PASS | Only essential deps: `serde`, `toml`, `thiserror` |
| XI (Specification-Driven) | ✅ PASS | Implementation derives from spec.md |
| VII (Configuration & Versioning) | ✅ PASS | Centralized config with TOML format |
| III (Code Quality) | ⏳ PENDING | Will verify with tests and clippy |

### Post-Design Check (Phase 1 Complete)

| Principle | Status | Notes |
|-----------|--------|-------|
| XVII (Modular Library-First) | ✅ PASS | `evo_common` is zero-dependency on workspace crates |
| IX (Simplicity & Minimal Dependencies) | ✅ PASS | 1 new dep (`toml`), others already in workspace |
| XI (Specification-Driven) | ✅ PASS | Data model and contracts documented |
| VII (Configuration & Versioning) | ✅ PASS | Immutable config after START, TOML format |
| III (Code Quality) | ✅ PASS | ConfigLoader trait has documented contract |
| XVIII (Deterministic Interface) | ✅ PASS | ConfigLoader has bounded, predictable behavior |
| XII (Error Handling) | ✅ PASS | ConfigError enum with explicit variants |

## Project Structure

### Documentation (this feature)

```text
specs/004-common-lib-setup/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   └── config_loader.rs # Trait definition
└── tasks.md             # Phase 2 output (NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
evo_common/
├── Cargo.toml
└── src/
    ├── lib.rs                          # pub mod shm, hal, config, prelude
    ├── prelude.rs                      # Cross-cutting: SharedConfig, LogLevel, ConfigLoader
    ├── config.rs                       # ConfigLoader, ConfigError, SharedConfig, LogLevel
    ├── shm/
    │   ├── mod.rs                      # pub mod consts, config
    │   ├── consts.rs                   # EVO_SHM_MAGIC, SHM_MIN_SIZE, etc.
    │   └── config.rs                   # ShmConfig (future)
    └── hal/
        ├── mod.rs                      # pub mod consts, config
        ├── consts.rs                   # HAL constants (future)
        └── config.rs                   # HalConfig (future)
```

**Structure Decision**: Domain modules (`shm`, `hal`) with `consts` and `config` submodules for clear separation. Use `evo` alias in Cargo.toml for short imports: `use evo::shm::consts::*;`

## Complexity Tracking

> No violations identified. Design follows constitution principles.
