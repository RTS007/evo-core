# Tasks: HAL Core + Simulation Driver

**Input**: Design documents from `/specs/003-hal-simulation/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Following TDD (Constitution II) - test tasks (T0XX-test) precede implementation tasks.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Project Initialization)

**Purpose**: Create project structure and basic configuration

- [ ] T001 Create `evo_hal/Cargo.toml` with dependencies (evo_common, evo_shared_memory, serde, toml, bincode, clap, tracing, thiserror)
- [ ] T002 [P] Create `evo_hal/src/lib.rs` with module declarations
- [ ] T003 [P] Create `evo_hal/src/main.rs` with CLI skeleton using clap (--config, --simulate, --driver, --verbose)
- [ ] T004 [P] Create `evo_hal/src/drivers/mod.rs` with driver registry exports

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure in `evo_common` that ALL user stories depend on

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

### HAL Constants (evo_common)

- [ ] T005 [P] Add `DEFAULT_CYCLE_TIME_US` (1000) and `DEFAULT_CYCLE_TIME` (Duration) to `evo_common/src/prelude.rs`
- [ ] T006 [P] Create `evo_common/src/hal/mod.rs` with module exports (config, consts, driver, types)
- [ ] T007 [P] Create `evo_common/src/hal/consts.rs` with MAX_AXES, MAX_DI, MAX_DO, MAX_AI, MAX_AO, DEFAULT_CONFIG_PATH

### HAL Types (evo_common)

- [ ] T008-test [P] Write unit tests for HalCommands, AxisCommand, HalStatus, AxisStatus, AnalogValue in `evo_common/src/hal/types.rs`
- [ ] T008 [P] Create `evo_common/src/hal/types.rs` with HalCommands, AxisCommand, HalStatus, AxisStatus, AnalogValue structs
- [ ] T009-test [P] Write unit tests for HalDriver trait and HalError in `evo_common/src/hal/driver.rs`
- [ ] T009 [P] Create `evo_common/src/hal/driver.rs` with HalDriver trait and HalError enum
- [ ] T010 Add DriverFactory type alias and DriverDiagnostics struct in `evo_common/src/hal/driver.rs`

### HAL Configuration (evo_common)

- [ ] T011-test Write unit tests for SharedConfig, MachineConfig (including cycle_time_us default) in `evo_common/src/hal/config.rs`
- [ ] T011 Create `evo_common/src/hal/config.rs` with SharedConfig, MachineConfig structs (with cycle_time_us field)
- [ ] T012 [P] Add AxisType enum (Simple, Positioning, Slave, Measurement) in `evo_common/src/hal/config.rs`
- [ ] T013 [P] Add ReferencingMode enum (6 modes) in `evo_common/src/hal/config.rs`
- [ ] T014 [P] Add ReferencingRequired enum (Yes, Perhaps, No) in `evo_common/src/hal/config.rs`
- [ ] T015-test Write unit tests for AxisConfig (including coupling_offset for Slave type) in `evo_common/src/hal/config.rs`
- [ ] T015 Add AxisConfig struct with all fields (including coupling_offset) in `evo_common/src/hal/config.rs`
- [ ] T016 [P] Add ReferencingConfig struct in `evo_common/src/hal/config.rs`
- [ ] T017 [P] Add DigitalIOConfig struct in `evo_common/src/hal/config.rs`
- [ ] T018 [P] Add AnalogIOConfig and AnalogCurveType in `evo_common/src/hal/config.rs`
- [ ] T019-test Write unit tests for MachineConfig::validate() covering all validation rules in `evo_common/src/hal/config.rs`
- [ ] T019 Implement `MachineConfig::validate()` method with all validation rules in `evo_common/src/hal/config.rs`
- [ ] T020-test Write unit tests for AxisConfig::validate() in `evo_common/src/hal/config.rs`
- [ ] T020 Implement `AxisConfig::validate()` method in `evo_common/src/hal/config.rs`

### HAL Core Infrastructure (evo_hal)

- [ ] T021-test Write unit tests for driver_registry (registration, lookup) in `evo_hal/src/driver_registry.rs`
- [ ] T021 Create `evo_hal/src/driver_registry.rs` with driver registration functions and get_driver_factory()
- [ ] T022-test Write contract tests for HalShmData, HalShmHeader layout matching contracts/shm_layout.md
- [ ] T022 Create `evo_hal/src/shm.rs` with HalShmData, HalShmHeader structs matching contracts/shm_layout.md
- [ ] T023 Add AxisShmData struct with command/status sections in `evo_hal/src/shm.rs`
- [ ] T024 Add AnalogShmData struct (dual f64) in `evo_hal/src/shm.rs`
- [ ] T025-test Write integration tests for SHM initialization using evo_shared_memory
- [ ] T025 Implement SHM initialization using evo_shared_memory in `evo_hal/src/shm.rs`
- [ ] T026-test Write unit tests for read_commands() and write_status() functions
- [ ] T026 Implement SHM read_commands() and write_status() functions in `evo_hal/src/shm.rs`
- [ ] T027 Create `evo_hal/src/core.rs` with HalCore struct skeleton

**Checkpoint**: Foundation ready - user story implementation can now begin

---

## Phase 3: User Story 1 - Configure and Start HAL (Priority: P1) 🎯 MVP

**Goal**: Load config, validate, initialize SHM, run basic RT loop with driver

**Independent Test**: Create config file, run `evo_hal -s --config config/machine.toml`, verify SHM segment exists

### Implementation for User Story 1

- [ ] T028-test [US1] Write unit tests for config loading from TOML (machine.toml + axis files)
- [ ] T028 [US1] Implement config loading from TOML in `evo_hal/src/core.rs` (machine.toml + axis files)
- [ ] T029-test [US1] Write unit tests for config path resolution (relative to main config dir)
- [ ] T029 [US1] Add config path resolution (relative to main config dir) in `evo_hal/src/core.rs`
- [ ] T030-test [US1] Write unit tests for CLI argument parsing (--config, --simulate, --driver flags)
- [ ] T030 [US1] Implement CLI argument parsing in `evo_hal/src/main.rs` (--config, --simulate, --driver flags)
- [ ] T031-test [US1] Write unit tests for driver selection logic (--simulate exclusive priority)
- [ ] T031 [US1] Implement driver selection logic in `evo_hal/src/main.rs` (--simulate priority over config)
- [ ] T032-test [US1] Write unit tests for HalCore::new() with config validation (including 0 axes/IOs edge case)
- [ ] T032 [US1] Implement HalCore::new() with config validation in `evo_hal/src/core.rs`
- [ ] T033-test [US1] Write integration tests for HalCore::init() (load driver, init SHM, handle SHM lock failure)
- [ ] T033 [US1] Implement HalCore::init() - load driver, init SHM in `evo_hal/src/core.rs`
- [ ] T034-test [US1] Write unit tests for RT loop using cycle_time_us from config
- [ ] T034 [US1] Implement basic RT loop in HalCore::run() using cycle_time_us from config in `evo_hal/src/core.rs`
- [ ] T034a [US1] Implement RT mode auto-detection via sched_getscheduler() in `evo_hal/src/core.rs`
- [ ] T035 [US1] Add tracing/logging setup in `evo_hal/src/main.rs`
- [ ] T036 [US1] Implement graceful shutdown (SIGINT/SIGTERM handling) in `evo_hal/src/main.rs`

### Simulation Driver Skeleton for User Story 1

- [ ] T037 [US1] Create `evo_hal/src/drivers/simulation/mod.rs` with module exports
- [ ] T038 [US1] Create `evo_hal/src/drivers/simulation/driver.rs` with SimulationDriver struct
- [ ] T039-test [US1] Write unit tests for SimulationDriver HalDriver trait implementation
- [ ] T039 [US1] Implement HalDriver trait skeleton for SimulationDriver (name, version, init, cycle, shutdown)
- [ ] T040 [US1] Register simulation driver in `evo_hal/src/drivers/mod.rs`
- [ ] T041-test [US1] Write unit tests for SimulationDriver::init() (load axis configs, initialize state)
- [ ] T041 [US1] Implement SimulationDriver::init() - load axis configs, initialize state

**Checkpoint**: HAL starts with config, creates SHM, runs empty RT loop - User Story 1 complete

---

## Phase 4: User Story 2 - Digital and Analog IO Control (Priority: P2)

**Goal**: Read/write digital and analog I/O via SHM with linked input simulation

**Independent Test**: Write DO[0]=true in SHM, verify simulation sees it; verify linked DI changes after configured delay

### Implementation for User Story 2

- [ ] T042-test [US2] Write unit tests for IOSimulator struct in `evo_hal/src/drivers/simulation/io.rs`
- [ ] T042 [US2] Create `evo_hal/src/drivers/simulation/io.rs` with IOSimulator struct
- [ ] T043-test [US2] Write unit tests for digital input/output state tracking
- [ ] T043 [US2] Implement digital input/output state tracking in IOSimulator
- [ ] T043a-test [US2] Write unit tests for LinkedDigitalInput delayed reactions (DO→DI linking)
- [ ] T043a [US2] Implement LinkedDigitalInput processing with delay queue in IOSimulator
- [ ] T043b [US2] Add pending DI change queue with timestamps for delayed state changes
- [ ] T044-test [US2] Write unit tests for analog I/O dual representation (normalized + scaled)
- [ ] T044 [US2] Implement analog input/output with dual representation (normalized + scaled) in IOSimulator
- [ ] T045-test [US2] Write unit tests for AnalogCurve polynomial scaling
- [ ] T045 [US2] Add AnalogCurve polynomial scaling in `evo_hal/src/drivers/simulation/io.rs`
- [ ] T046-test [US2] Write unit tests for inverse scaling (Newton-Raphson for polynomial)
- [ ] T046 [US2] Add inverse scaling (scaled → normalized) in `evo_hal/src/drivers/simulation/io.rs`
- [ ] T047 [US2] Integrate IOSimulator into SimulationDriver::cycle() in `evo_hal/src/drivers/simulation/driver.rs`
- [ ] T048 [US2] Update SHM digital I/O bitfield read/write in `evo_hal/src/shm.rs`

**Checkpoint**: I/O values flow between SHM and simulation, linked DI reactions work - User Story 2 complete

---

## Phase 5: User Story 3 - Axis Motion Simulation (Priority: P3)

**Goal**: Command axis positions via SHM, see realistic physics-based motion

**Independent Test**: Set TargetPosition=1000, observe ActualPosition gradually increasing with acceleration/velocity limits

### Implementation for User Story 3

- [ ] T049 [US3] Create `evo_hal/src/drivers/simulation/physics/mod.rs` with module exports
- [ ] T050-test [US3] Write unit tests for AxisSimulator struct in `evo_hal/src/drivers/simulation/physics/axis.rs`
- [ ] T050 [US3] Create `evo_hal/src/drivers/simulation/physics/axis.rs` with AxisSimulator struct
- [ ] T051-test [US3] Write unit tests for kinematic model (velocity ramping, acceleration limits)
- [ ] T051 [US3] Implement kinematic model (velocity ramping, acceleration limits) in AxisSimulator
- [ ] T052-test [US3] Write unit tests for position integration in AxisSimulator::update()
- [ ] T052 [US3] Implement position integration in AxisSimulator::update()
- [ ] T053-test [US3] Write unit tests for Simple axis type handling (on/off, no physics)
- [ ] T053 [US3] Add Simple axis type handling (on/off, no physics) in AxisSimulator
- [ ] T054-test [US3] Write unit tests for Positioning axis type handling (full kinematics)
- [ ] T054 [US3] Add Positioning axis type handling (full kinematics) in AxisSimulator
- [ ] T055-test [US3] Write unit tests for Measurement axis type handling (encoder only, no drive)
- [ ] T055 [US3] Add Measurement axis type handling (encoder only, no drive) in AxisSimulator
- [ ] T056-test [US3] Write unit tests for Slave axis coupling (1:1 ratio + captured offset using coupling_offset)
- [ ] T056 [US3] Implement Slave axis coupling (1:1 ratio + captured offset) in AxisSimulator
- [ ] T057-test [US3] Write unit tests for ReferencingStateMachine in `evo_hal/src/drivers/simulation/physics/referencing.rs`
- [ ] T057 [US3] Create `evo_hal/src/drivers/simulation/physics/referencing.rs` with ReferencingStateMachine
- [ ] T058-test [US3] Write unit tests for 6 referencing modes state machine
- [ ] T058 [US3] Implement 6 referencing modes state machine in ReferencingStateMachine
- [ ] T059 [US3] Implement referencing states (Unreferenced, SearchingSwitch, SearchingIndex, Referenced, Error)
- [ ] T060 [US3] Integrate AxisSimulator into SimulationDriver::cycle()
- [ ] T061 [US3] Update axis status fields (moving, in_position, referenced, referencing) in cycle()

**Checkpoint**: Axes move with realistic physics, referencing works - User Story 3 complete

---

## Phase 6: User Story 4 - Lag Error Detection (Priority: P4)

**Goal**: Detect and report lag errors, implement error recovery protocol

**Independent Test**: Command fast motion exceeding max_velocity, observe lag error grow until error triggers

### Implementation for User Story 4

- [ ] T062-test [US4] Write unit tests for lag error calculation in AxisSimulator::update()
- [ ] T062 [US4] Add lag error calculation in AxisSimulator::update()
- [ ] T063-test [US4] Write unit tests for lag error limit check and error triggering
- [ ] T063 [US4] Implement lag error limit check and error triggering in AxisSimulator
- [ ] T064 [US4] Add error_code field updates (LAG_ERROR = 0x0001) in AxisSimulator
- [ ] T065-test [US4] Write unit tests for two-phase error recovery (Reset only when Enable=false)
- [ ] T065 [US4] Implement two-phase error recovery (Reset only when Enable=false) in AxisSimulator
- [ ] T066 [US4] Update axis ready/error status in SimulationDriver::cycle()

**Checkpoint**: Lag errors detected and reported, recovery works - User Story 4 complete

---

## Phase 7: State Persistence (Cross-Cutting)

**Purpose**: Persist and restore axis state across restarts (FR-011, FR-012)

- [ ] T067-test Write unit tests for PersistedState, PersistedAxisState structs in `evo_hal/src/drivers/simulation/state.rs`
- [ ] T067 Create `evo_hal/src/drivers/simulation/state.rs` with PersistedState, PersistedAxisState structs
- [ ] T068-test Write unit tests for state serialization using bincode
- [ ] T068 Implement state serialization using bincode in `evo_hal/src/drivers/simulation/state.rs`
- [ ] T069-test Write unit tests for StatePersistence::save()
- [ ] T069 Implement StatePersistence::save() in `evo_hal/src/drivers/simulation/state.rs`
- [ ] T070-test Write unit tests for StatePersistence::load()
- [ ] T070 Implement StatePersistence::load() in `evo_hal/src/drivers/simulation/state.rs`
- [ ] T071 Integrate state restore in SimulationDriver::init()
- [ ] T072 Integrate state save in SimulationDriver::shutdown()
- [ ] T073-test Write unit tests for referencing_required=perhaps vs yes logic
- [ ] T073 Handle referencing_required=perhaps vs yes logic based on persisted state

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [ ] T074 [P] Add comprehensive error messages for config validation failures
- [ ] T075 [P] Add structured JSON logging mode (--json flag or config option)
- [ ] T076 [P] Add timing violation detection and logging in RT loop
- [ ] T077 [P] Add SHM version protocol (odd = write in progress) in `evo_hal/src/shm.rs`
- [ ] T078 [P] Create example config files in `config/machine.toml`, `config/axes/axis_01.toml`, etc.
- [ ] T079 Run quickstart.md validation - verify documented commands work
- [ ] T080 Add --version flag showing crate version

---

## Dependencies & Execution Order

### Phase Dependencies

```
Phase 1 (Setup)
    │
    ▼
Phase 2 (Foundational) ◄─── BLOCKS ALL USER STORIES
    │
    ├──────────────────────────────────┐
    ▼                                  ▼
Phase 3 (US1: Config+Start)    Can start after Phase 2
    │
    ▼
Phase 4 (US2: I/O) ◄─── Depends on US1 driver skeleton
    │
    ▼
Phase 5 (US3: Motion) ◄─── Depends on US1 driver, may use US2 I/O for switches
    │
    ▼
Phase 6 (US4: Lag Error) ◄─── Depends on US3 motion physics
    │
    ▼
Phase 7 (State Persistence) ◄─── Depends on US3 axis state
    │
    ▼
Phase 8 (Polish)
```

### Within Each User Story

- Models/structs before logic
- Core implementation before integration
- SHM interfaces before driver code that uses them

### Parallel Opportunities per Phase

**Phase 2 (Foundational)**:
```bash
# Can run in parallel:
T005, T006, T007          # Constants and module setup
T008, T009                # Types
T012, T013, T014          # Enums
T016, T017, T018          # Config structs
```

**Phase 3 (US1)**:
```bash
# Can run in parallel after T028:
T037, T038                # Simulation driver files
```

**Phase 5 (US3)**:
```bash
# Can run in parallel:
T049, T050                # Physics module setup
T053, T054, T055          # Axis type handlers
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: Run `evo_hal -s --config config/machine.toml`, verify SHM created
5. Deploy/demo if basic startup is needed

### Incremental Delivery

1. Setup + Foundational → Foundation ready
2. Add US1 → HAL starts, SHM created (MVP!)
3. Add US2 → I/O working → Demo I/O control
4. Add US3 → Axes move → Demo motion simulation
5. Add US4 → Safety features → Production-ready simulation
6. Add State Persistence → Full feature set
7. Polish → Release quality

---

## Summary

| Phase | Tasks | Test Tasks | Parallel Tasks | Key Deliverable |
|-------|-------|------------|----------------|-----------------|
| 1. Setup | T001-T004 | 0 | 3 | Project structure |
| 2. Foundational | T005-T027 | 10 | 14 | evo_common HAL types, SHM layout |
| 3. US1 Config+Start | T028-T041 | 11 | 4 | HAL starts with config |
| 4. US2 I/O | T042-T048 | 5 | 0 | Digital/Analog I/O working |
| 5. US3 Motion | T049-T061 | 10 | 5 | Axis physics simulation |
| 6. US4 Lag Error | T062-T066 | 3 | 0 | Safety feature |
| 7. State Persistence | T067-T073 | 5 | 0 | Restart recovery |
| 8. Polish | T074-T080 | 0 | 5 | Production quality |
| **Total** | **81 impl** | **44 test** | **31 parallel** | **125 total tasks** |

