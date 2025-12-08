# Tasks: Common Library Setup

**Input**: Design documents from `/specs/004-common-lib-setup/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2)
- Include exact file paths in descriptions

## Path Conventions

Based on plan.md structure:
```
evo_common/src/
├── lib.rs
├── prelude.rs
├── config.rs
├── shm/
│   ├── mod.rs
│   ├── consts.rs
│   └── config.rs
└── hal/
    ├── mod.rs
    ├── consts.rs
    └── config.rs
```

---

## Phase 1: Setup

**Purpose**: Initialize evo_common crate structure and dependencies

- [X] T001 Update `evo_common/Cargo.toml` with dependencies: serde, toml, thiserror
- [X] T002 [P] Create module structure in `evo_common/src/lib.rs` (pub mod shm, hal, config, prelude)
- [X] T003 [P] Create empty `evo_common/src/shm/mod.rs` with `pub mod consts; pub mod config;`
- [X] T004 [P] Create empty `evo_common/src/hal/mod.rs` with `pub mod consts; pub mod config;`

---

## Phase 2: Foundational

**Purpose**: None needed - this is a library feature with no blocking infrastructure

**Checkpoint**: Setup complete, user story implementation can begin

---

## Phase 3: User Story 1 - Shared Constants (Priority: P1) 🎯 MVP

**Goal**: Centralize SHM constants in evo_common and migrate evo_shared_memory

**Independent Test**: `cargo test -p evo_common` passes; `cargo build -p evo_shared_memory` compiles

### Implementation for User Story 1

- [X] T005 [P] [US1] Create `evo_common/src/shm/consts.rs` with EVO_SHM_MAGIC, SHM_MIN_SIZE, SHM_MAX_SIZE, CACHE_LINE_SIZE
- [X] T006 [P] [US1] Create empty `evo_common/src/shm/config.rs` (placeholder for future ShmConfig)
- [X] T007 [P] [US1] Create empty `evo_common/src/hal/consts.rs` (placeholder for future HAL constants)
- [X] T008 [P] [US1] Create empty `evo_common/src/hal/config.rs` (placeholder for future HalConfig)
- [X] T009 [US1] Add unit tests for constants in `evo_common/src/shm/consts.rs` (inline #[cfg(test)])
- [X] T010 [US1] Update `evo_shared_memory/Cargo.toml` to add evo_common dependency with evo alias
- [X] T011 [US1] Remove constant definitions from `evo_shared_memory/src/segment.rs` and import from evo::shm::consts
- [X] T012 [US1] Update `evo_shared_memory/src/lib.rs` to remove constant re-exports
- [X] T013 [US1] Update all imports in `evo_shared_memory/src/*.rs` to use evo::shm::consts
- [X] T014 [US1] Update imports in `evo_shared_memory/examples/*.rs` to use evo::shm::consts
- [X] T015 [US1] Update imports in `evo_shared_memory/tests/*.rs` to use evo::shm::consts
- [X] T016 [US1] Verify `cargo test -p evo_shared_memory` passes

**Checkpoint**: Constants centralized, evo_shared_memory uses evo_common as single source of truth

---

## Phase 4: User Story 2 - Configuration Loading (Priority: P2)

**Goal**: Implement ConfigLoader trait and SharedConfig for standardized configuration

**Independent Test**: Unit test loads a sample config.toml into SharedConfig

### Implementation for User Story 2

- [X] T017 [P] [US2] Create `evo_common/src/config.rs` with ConfigError enum (FileNotFound, ParseError, ValidationError)
- [X] T018 [US2] Add LogLevel enum to `evo_common/src/config.rs` (Trace, Debug, Info, Warn, Error)
- [X] T019 [US2] Add SharedConfig struct to `evo_common/src/config.rs` (log_level: LogLevel, service_name: String)
- [X] T020 [US2] Implement ConfigLoader trait with default `fn load(path: &Path) -> Result<Self, ConfigError>`
- [X] T021 [US2] Add validation for SharedConfig (service_name non-empty) - validate after deserialize in load(), return ValidationError if invalid
- [X] T022 [P] [US2] Create `evo_common/src/prelude.rs` with re-exports: SharedConfig, LogLevel, ConfigLoader, ConfigError
- [X] T023 [US2] Update `evo_common/src/lib.rs` to export config and prelude modules
- [X] T024 [US2] Add unit tests for ConfigLoader in `evo_common/src/config.rs` (inline #[cfg(test)])
- [X] T025 [US2] Add unit tests for LogLevel serialization/deserialization
- [X] T026 [US2] Add unit test loading sample config.toml with SharedConfig + app-specific fields

**Checkpoint**: ConfigLoader trait works, can load TOML files with SharedConfig

---

## Phase 5: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, cleanup, validation

- [X] T027 [P] Update `evo_common/src/lib.rs` with module-level documentation
- [X] T028 [P] Add doc comments to all public types and functions
- [X] T029 Run `cargo clippy -p evo_common` and fix warnings
- [X] T030 Run `cargo fmt -p evo_common`
- [X] T031 Verify `cargo doc -p evo_common` generates clean documentation
- [X] T032 Update `evo/src/main.rs` to use evo alias for evo_common (demonstration)
- [X] T033 Run full workspace build: `cargo build --workspace`
- [X] T034 Run full workspace tests: `cargo test --workspace`
- [X] T035 Verify `evo_common` has zero workspace dependencies: `cargo tree -p evo_common --edges normal | grep -v evo_common` should show no evo_* crates

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - start immediately
- **Foundational (Phase 2)**: N/A for this feature
- **User Story 1 (Phase 3)**: Depends on Setup (T001-T004)
- **User Story 2 (Phase 4)**: Depends on Setup (T001-T004), can run parallel with US1
- **Polish (Phase 5)**: Depends on US1 and US2 completion

### User Story Dependencies

- **User Story 1 (P1)**: Independent - just needs Setup
- **User Story 2 (P2)**: Independent - just needs Setup, can run parallel with US1

### Within User Story 1

```
T005, T006, T007, T008 (parallel) → T009 → T010 → T011 → T012 → T013 → T014, T015 (parallel) → T016
```

### Within User Story 2

```
T017 → T018 → T019 → T020 → T021 → T022 (parallel with T021) → T023 → T024, T025, T026 (parallel)
```

---

## Parallel Opportunities

### Phase 1 (Setup)
```bash
# All can run in parallel:
T002: lib.rs module structure
T003: shm/mod.rs
T004: hal/mod.rs
```

### Phase 3 (User Story 1)
```bash
# Create all placeholder files in parallel:
T005: shm/consts.rs (main work)
T006: shm/config.rs (placeholder)
T007: hal/consts.rs (placeholder)
T008: hal/config.rs (placeholder)

# Update examples/tests in parallel:
T014: examples/*.rs
T015: tests/*.rs
```

### Phase 4 (User Story 2)
```bash
# Parallel at end:
T024: ConfigLoader tests
T025: LogLevel tests
T026: Integration test
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001-T004)
2. Complete Phase 3: User Story 1 (T005-T016)
3. **STOP and VALIDATE**: `cargo test --workspace`
4. Deploy/demo: Constants centralized, single source of truth established

### Full Feature

1. Complete Setup (Phase 1)
2. Complete User Story 1 (Phase 3) → Validate
3. Complete User Story 2 (Phase 4) → Validate
4. Complete Polish (Phase 5)
5. Final validation: `cargo test --workspace && cargo clippy --workspace`

---

## Notes

- Use `evo = { package = "evo_common", path = "../evo_common" }` in all Cargo.toml files
- Constants use `SCREAMING_SNAKE_CASE`
- All public items need doc comments
- ConfigLoader uses blanket implementation for any `DeserializeOwned` type
- `const` is a Rust keyword - use `consts` for module name
