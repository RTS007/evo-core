# Tasks: Control Unit — Axis Control Brain

**Input**: Design documents from `/specs/005-control-unit/`  
**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, contracts/ ✅, quickstart.md ✅  
**Tests**: Each phase includes TDD unit test tasks per Constitution Principle II. Integration tests in Phase 11.

**Organization**: Tasks grouped by user story for independent implementation and testing.  
**User Stories**: 7 stories (2×P1, 2×P2, 3×P3) from spec.md

## Format: `[ID] [P?] [Story?] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- Exact file paths included in every task description

---

## Phase 1: Setup (Project Initialization)

**Purpose**: Configure crate, dependencies, and directory skeleton per plan.md

- [ ] T001 Update crate manifest with required dependencies (evo_common, evo_shared_memory, nix, libc, toml, serde, heapless, bitflags, static_assertions) in `evo_control_unit/Cargo.toml`
- [ ] T002 Create module directory skeleton matching plan.md project structure — `evo_control_unit/src/{lib.rs, config.rs, cycle.rs, state/, safety/, control/, command/, shm/, error/}` with empty `mod.rs` files and module re-exports in `lib.rs`
- [ ] T003 [P] Create `evo_common/src/control_unit/mod.rs` module root with sub-module declarations (state, error, safety, control, command, homing, config) and add `pub mod control_unit;` to `evo_common/src/lib.rs`
- [ ] T004 [P] Create `evo_common/src/shm/p2p.rs` with `P2pSegmentHeader` struct and `ModuleAbbrev` enum per contracts/shm-segments.md, and add `pub mod p2p;` to `evo_common/src/shm/mod.rs`
- [ ] T005 [P] Add `#![deny(clippy::disallowed_types)]` configuration and `static_assertions` compile-time size/alignment checks for all `#[repr(C)]` structs as they are created (ongoing — initial setup of clippy.toml or lib.rs attributes)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared types, config loading, SHM integration, and RT cycle skeleton that ALL user stories depend on

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

### Shared Types in evo_common (FR-140)

- [ ] T006 [P] Define global state enums (`MachineState`, `SafetyState`, `SafeStopCategory`) with `#[repr(u8)]` in `evo_common/src/control_unit/state.rs` per data-model.md LEVEL 1-2
- [ ] T007 [P] Define per-axis state enums (`PowerState`, `MotionState`, `OperationalMode`, `CouplingState`, `GearboxState`, `LoadingState`) with `#[repr(u8)]` in `evo_common/src/control_unit/state.rs` per data-model.md LEVEL 3
- [ ] T008 [P] Define error bitflag types (`PowerError`, `MotionError`, `CommandError`, `GearboxError`, `CouplingError`) using `bitflags` crate in `evo_common/src/control_unit/error.rs` per data-model.md LEVEL 5
- [ ] T009 [P] Define `AxisSafetyState` struct (8 boolean flags) in `evo_common/src/control_unit/safety.rs` per data-model.md LEVEL 4
- [ ] T010 [P] Define `ControlOutputVector` (`#[repr(C)]`, 32 bytes, 4×f64) and `UniversalControlParameters` structs in `evo_common/src/control_unit/control.rs` per data-model.md
- [ ] T011 [P] Define `CommandSource` enum, `AxisSourceLock` struct, and `PauseTargets` struct in `evo_common/src/control_unit/command.rs` per data-model.md
- [ ] T012 [P] Define `HomingMethod` enum, `HomingDirection` enum (FR-033a), and `HomingConfig` struct with all method-specific parameters in `evo_common/src/control_unit/homing.rs` per data-model.md
- [ ] T013 [P] Define safety peripheral config types (`TailstockType`, `TailstockConfig`, `IndexConfig`, `BrakeConfig`, `GuardConfig`) in `evo_common/src/control_unit/safety.rs` per data-model.md
- [ ] T014 [P] Define `CouplingConfig` struct (master_axis, slave_axes, coupling_ratio, modulation_offset, sync_timeout, max_lag_diff) in `evo_common/src/control_unit/state.rs` per data-model.md
- [ ] T015 [P] Define `AxisId` type alias, `AxisState` container struct, `AxisControlState` (PID/DOB/filter state, 80 bytes), and `PowerSequenceState` in `evo_common/src/control_unit/state.rs` per data-model.md
- [ ] T016 [P] Define `AxisErrorState` container (power + motion + command + gearbox + coupling bitflags) in `evo_common/src/control_unit/error.rs` per data-model.md

### SHM Segment Payload Types in evo_common (FR-131–FR-136)

- [ ] T017 [P] Define `HalToCuSegment`, `HalAxisFeedback` structs (`#[repr(C, align(64))]`) with drive_status bitfield in `evo_common/src/control_unit/shm.rs` per contracts/shm-segments.md §1
- [ ] T018 [P] Define `CuToHalSegment`, `CuAxisCommand` structs (`#[repr(C, align(64))]`) in `evo_common/src/control_unit/shm.rs` per contracts/shm-segments.md §2
- [ ] T019 [P] Define `ReToCuSegment`, `ReCommand`, `ReAxisTarget`, `ReCommandType` enum in `evo_common/src/control_unit/shm.rs` per contracts/shm-segments.md §3
- [ ] T020 [P] Define `CuToMqtSegment`, `AxisStateSnapshot` in `evo_common/src/control_unit/shm.rs` per contracts/shm-segments.md §4
- [ ] T021 [P] Define `RpcToCuSegment`, `RpcCommand`, `RpcCommandType` enum in `evo_common/src/control_unit/shm.rs` per contracts/shm-segments.md §5
- [ ] T022 [P] Define `CuToReSegment` placeholder struct in `evo_common/src/control_unit/shm.rs` per contracts/shm-segments.md §6
- [ ] T023 Implement `struct_version_hash<T>()` const fn in `evo_common/src/shm/p2p.rs` per contracts/shm-segments.md Version Hash Contract (FR-130d) — hash from `size_of::<T>()` and `align_of::<T>()`
- [ ] T024 Add `static_assertions` size and alignment checks for all SHM segment structs (P2pSegmentHeader=64, HalAxisFeedback=24, CuAxisCommand=40, ControlOutputVector=32, AxisStateSnapshot=56) in `evo_common/src/control_unit/shm.rs`

### Configuration (FR-141, FR-142)

- [ ] T025 Define `ControlUnitConfig`, `CuMachineConfig`, and `GlobalSafetyConfig` structs with serde `Deserialize` in `evo_common/src/control_unit/config.rs` per data-model.md — include const `MIN`/`MAX` bounds for all numeric parameters (FR-156) and `#[serde(default)]` on optional fields for forward-compatible deserialization (FR-157)
- [ ] T026 Define `CuAxisConfig` struct (axis_id, name, control params, safe_stop, homing, peripherals, coupling, loading flags) with serde `Deserialize` in `evo_common/src/control_unit/config.rs` per data-model.md — all peripheral I/O fields use `IoRole` instead of string names; include const `MIN`/`MAX` bounds for all numeric parameters (FR-156) and `#[serde(default, deny_unknown_fields)]` for forward-compatible deserialization with unknown field rejection (FR-157)

### I/O Configuration (FR-148–FR-155)

- [ ] T026a [P] Define `IoRole` enum, `IoPointType` enum, `DiLogic` enum in `evo_common/src/io/role.rs` per data-model.md — implement `FromStr` parser with `FunctionAxisNumber` convention (e.g., `"LimitMin1"` → `IoRole::LimitMin(1)`)
- [ ] T026b [P] Define `IoPoint`, `IoGroup`, `IoConfig` config structs with serde `Deserialize` in `evo_common/src/io/config.rs` per data-model.md and contracts/io-config.md
- [ ] T026c [P] Define `IoBinding`, `IoRegistry` runtime structs with `read_di`/`read_ai`/`write_do`/`write_ao` methods in `evo_common/src/io/registry.rs` per contracts/io-config.md §5
- [ ] T026d [P] Implement `IoRegistry::from_config()` with validation: pin uniqueness (V-IO-1), role uniqueness (V-IO-2), role type correctness (V-IO-3), analog range validity (V-IO-6) in `evo_common/src/io/registry.rs`
- [ ] T026e [P] Implement `IoRegistry::validate_roles_for_axis()` — per-axis role completeness check (V-IO-4) + global role completeness (V-IO-5: EStop required) in `evo_common/src/io/registry.rs`
- [ ] T026f [P] Add `pub mod io;` to `evo_common/src/lib.rs` with sub-module declarations (role, config, registry)
- [ ] T026g [P] Write unit tests for IoRole parsing (string → enum round-trip), IoRegistry construction, all validation rules (V-IO-1 through V-IO-7), read_di NC/NO logic, read_ai scaling in `evo_common/tests/unit/io_config.rs`

- [ ] T027 Implement TOML config loader with validation (axis ID uniqueness, coupling graph acyclicity, required peripheral sensors present, **io.toml role completeness per axis** via `IoRegistry::validate_roles_for_axis`) in `evo_control_unit/src/config.rs`

### SHM Integration Layer (FR-130, FR-139)

- [ ] T028 Implement P2P segment connection manager — create writer segments (evo_cu_hal, evo_cu_mqt, evo_cu_re), attach reader segments (evo_hal_cu, evo_re_cu, evo_rpc_cu) with version hash validation in `evo_control_unit/src/shm/segments.rs`
- [ ] T029 Implement inbound segment reader with heartbeat staleness detection (N=3 for RT, configurable for non-RT) and version hash check in `evo_control_unit/src/shm/reader.rs`
- [ ] T030 Implement outbound segment writer with heartbeat increment and write_seq lock-free protocol (odd=writing, even=committed) in `evo_control_unit/src/shm/writer.rs`

### RT Cycle Skeleton (FR-133, FR-138, FR-138a)

- [ ] T031 Implement RT setup sequence (pre-allocate state arrays, mlockall, prefault pages, sched_setaffinity, sched_setscheduler SCHED_FIFO 80) in `evo_control_unit/src/main.rs` per research.md Topics 1-3
- [ ] T032 Implement deterministic cycle loop with clock_nanosleep(TIMER_ABSTIME), cycle time measurement, and overrun detection (>cycle_time → ERR_CYCLE_OVERRUN → SAFETY_STOP) in `evo_control_unit/src/cycle.rs` per research.md Topic 4
- [ ] T033 Implement cycle body skeleton: read inbound SHM → process (placeholder) → write outbound SHM, with timing instrumentation per cycle phase in `evo_control_unit/src/cycle.rs`
- [ ] T034 Implement pre-allocated runtime state: `[AxisState; 64]` array, global `MachineState`, global `SafetyState` in `evo_control_unit/src/lib.rs` or `evo_control_unit/src/cycle.rs`

### Error Infrastructure

- [ ] T035 Implement hierarchical error evaluation — classify each error flag as CRITICAL or non-critical, implement propagation rules (FR-091, FR-092) in `evo_control_unit/src/error/propagation.rs`

**Checkpoint**: Foundation ready — all shared types defined, SHM connected, config loaded, RT cycle running with read→(empty)→write. User story implementation can now begin.

### Unit Tests (TDD — Constitution Principle II)

- [ ] T035a [P] Write unit tests for all `#[repr(u8)]` enum conversions (round-trip u8→enum→u8) and bitflag operations in `evo_common/src/control_unit/` — state.rs, error.rs, safety.rs, command.rs, homing.rs
- [ ] T035b [P] Write contract tests validating `static_assertions` for all SHM struct sizes/alignments, plus `struct_version_hash` determinism in `evo_common/src/control_unit/shm.rs`
- [ ] T035c [P] Write unit tests for config loader — valid config, missing fields, invalid axis IDs, cyclic coupling graph, missing peripheral sensors, **io.toml role completeness validation (V-IO-4, V-IO-5)** in `evo_control_unit/tests/unit/config.rs`
- [ ] T035d [P] Write unit tests for error propagation — CRITICAL flag→SAFETY_STOP, non-critical→axis-local only in `evo_control_unit/tests/unit/error.rs`

---

## Phase 3: User Story 1 — Basic Axis Power Lifecycle Management (Priority: P1) 🎯 MVP

**Goal**: Start and stop an axis through its complete power lifecycle (POWER_OFF → POWERING_ON → STANDBY → MOTION → POWERING_OFF → POWER_OFF) with deterministic state transitions observable via SHM.

**Independent Test**: Configure a single axis, issue start command via mock evo_re_cu, verify PowerState transitions through evo_cu_mqt, issue stop command, verify return to POWER_OFF.

### Implementation

- [ ] T036 [US1] Implement `MachineState` transition logic (Stopped→Starting→Idle, Idle↔Manual, Idle/Manual→Active, any→SystemError) with guards and actions per contracts/state-machines.md §1 in `evo_control_unit/src/state/machine.rs`
- [ ] T037 [US1] Implement `PowerState` transition logic with full POWERING_ON 10-step sequence (check safety → enable drive → release brake → verify position → Standby) and POWERING_OFF 7-step sequence per contracts/state-machines.md §3 in `evo_control_unit/src/state/power.rs`
- [ ] T038 [US1] Implement `PowerSequenceState` step tracking with per-step timers and timeout detection (brake_timeout → PowerError, lock_pin_timeout → PowerError, drive_not_ready → PowerError) in `evo_control_unit/src/state/power.rs`
- [ ] T039 [US1] Implement `MotionState` transition logic (Standstill→Accelerating→ConstantVelocity→Decelerating→Standstill, any_moving→Stopping, any→EmergencyStop, any→MotionError) per contracts/state-machines.md §4 in `evo_control_unit/src/state/motion.rs`
- [ ] T040 [US1] Implement `OperationalMode` transition logic with guards (only when Standstill+Standby, **reject** if coupled slave per FR-042 — override logic deferred to T059) per contracts/state-machines.md §5 in `evo_control_unit/src/state/operational.rs`
- [ ] T041 [US1] Implement axis state update orchestration — per-cycle call to evaluate PowerState, MotionState, OperationalMode for each active axis, write results to `AxisState` array in `evo_control_unit/src/cycle.rs`
- [ ] T042 [US1] Implement `evo_cu_mqt` diagnostic snapshot writer — populate `AxisStateSnapshot` for all axes, write `MachineState`, `SafetyState`, maintain `EventEntry` ring buffer with `EventType` encoding per contracts/shm-segments.md §4 in `evo_control_unit/src/shm/writer.rs`
- [ ] T043 [US1] Implement command dispatch — read `ReCommand` from evo_re_cu, parse `ReCommandType` (EnableAxis, DisableAxis, MoveAbsolute, Stop), route to PowerState/MotionState machines in `evo_control_unit/src/command/arbitration.rs`
- [ ] T044 [US1] Implement `evo_cu_hal` command writer — populate `CuAxisCommand` (placeholder zero `ControlOutputVector` + enable + mode) for all axes per cycle; real control output filled by T067 in Phase 6 in `evo_control_unit/src/shm/writer.rs`

**Checkpoint**: Single axis can power on, accept motion commands, transition through MotionState, power off. All state changes visible in evo_cu_mqt. MVP complete.

### Unit Tests (TDD — Constitution Principle II)

- [ ] T044a [P] [US1] Write unit tests for MachineState transitions — all valid transitions, all invalid transitions rejected, SystemError exit only via reset in `evo_control_unit/tests/unit/state_machine.rs`
- [ ] T044b [P] [US1] Write unit tests for PowerState sequences — POWERING_ON 10-step success, POWERING_ON timeout at each step, POWERING_OFF sequence, NoBrake guard in `evo_control_unit/tests/unit/state_power.rs`
- [ ] T044c [P] [US1] Write unit tests for MotionState transitions — all valid/invalid transitions, EmergencyStop from every moving state in `evo_control_unit/tests/unit/state_motion.rs`

---

## Phase 4: User Story 2 — Safety Peripheral Integration (Priority: P1)

**Goal**: Continuously monitor safety peripherals (tailstock, locking pin, brake, safety guards) and block/halt motion when conditions are unsafe. Implement SAFETY_STOP with per-axis SafeStopCategory execution.

**Independent Test**: Configure safety peripherals, simulate unsafe conditions via mock evo_hal_cu (open guard, missing brake confirmation), verify axis cannot start or stops immediately via SafetyState transition in evo_cu_mqt.

### Implementation

- [ ] T045 [P] [US2] Implement tailstock monitoring logic for types 0-4 (read DI via evo_hal_cu, apply NC/NO config, evaluate `tailstock_ok` flag, set `ERR_DRIVE_TAIL_OPEN` on violation, detect `ERR_SENSOR_CONFLICT` when conflicting sensor states are read simultaneously) in `evo_control_unit/src/safety/peripherals.rs`
- [ ] T046 [P] [US2] Implement locking pin monitoring (read di_locked/di_middle/di_free, validate states, timeout detection for retract/insert, set `ERR_LOCK_PIN_TIMEOUT`) in `evo_control_unit/src/safety/peripherals.rs`
- [ ] T047 [P] [US2] Implement brake monitoring (read di_brake_released, release/engage timeout detection, set `ERR_BRAKE_TIMEOUT`, `ERR_DRIVE_BRAKE_LOCKED`) in `evo_control_unit/src/safety/peripherals.rs`
- [ ] T048 [P] [US2] Implement safety guard monitoring (read di_guard_closed/di_guard_locked, check speed vs secure_speed with 2s open_delay timer, set guard_ok flag) in `evo_control_unit/src/safety/peripherals.rs`
- [ ] T049 [US2] Implement `AxisSafetyState` flag evaluation — aggregate all peripheral flags per axis per cycle, enforce "motion blocked when any flag false" (FR-081) in `evo_control_unit/src/safety/flags.rs`
- [ ] T050 [US2] Implement `SafetyState` transition logic (Safe↔SafeReducedSpeed, Safe/SafeReducedSpeed→SafetyStop, SafetyStop→Safe on reset) per contracts/state-machines.md §2 in `evo_control_unit/src/state/safety.rs`
- [ ] T050a [US2] Implement `SAFE_REDUCED_SPEED` velocity clamping — when SafetyState==SafeReducedSpeed, enforce hardware speed limit on all axes by clamping TargetVelocity in ControlOutputVector before writing to evo_cu_hal per FR-011 in `evo_control_unit/src/state/safety.rs`
- [ ] T051 [US2] Implement SAFETY_STOP execution — per-axis SafeStopCategory protocol (STO: immediate disable+brake, SS1: MaxDec→disable+brake, SS2: MaxDec→hold torque), force MachineState→SystemError per FR-121 in `evo_control_unit/src/safety/stop.rs`
- [ ] T052 [US2] Implement recovery sequence — require reset button press + all AxisSafetyState flags true + manual authorization, then SafetyState→Safe per FR-122 in `evo_control_unit/src/safety/recovery.rs`
- [ ] T053 [US2] Integrate safety evaluation into cycle — call safety flag evaluation and SAFETY_STOP detection between SHM read and state machine processing in `evo_control_unit/src/cycle.rs`

**Checkpoint**: All safety peripherals monitored every cycle. Unsafe conditions block power-on or trigger SAFETY_STOP. Recovery requires operator reset. US1 + US2 both functional.

### Unit Tests (TDD — Constitution Principle II)

- [ ] T053a [P] [US2] Write unit tests for each safety peripheral (tailstock types 0-4, locking pin, brake, guard) — normal operation, fault detection, NC/NO interpretation in `evo_control_unit/tests/unit/safety_peripherals.rs`
- [ ] T053b [P] [US2] Write unit tests for SAFETY_STOP execution — STO/SS1/SS2 protocols, MachineState→SystemError, recovery sequence pass/fail in `evo_control_unit/tests/unit/safety_stop.rs`
- [ ] T053c [P] [US2] Write unit tests for SAFE_REDUCED_SPEED velocity clamping — verify velocity limited when SafeReducedSpeed active, restored when Safe in `evo_control_unit/tests/unit/safety_reduced.rs`

---

## Phase 5: User Story 3 — Master-Slave Synchronization (Priority: P2)

**Goal**: Synchronize multiple axes using master-slave coupling with deterministic multi-axis behavior — bottom-up synchronization, same-cycle SYNCHRONIZED transition, error propagation through coupling chains.

**Independent Test**: Configure master + 2 slaves, issue start, verify all wait in WAITING_SYNC then transition to SYNCHRONIZED in same cycle. Trigger slave fault, verify cascade to master via ERR_SLAVE_FAULT.

### Implementation

- [ ] T054 [P] [US3] Implement `CouplingState` transition logic (Uncoupled→Coupling→Master/WaitingSync, WaitingSync→SlaveCoupled/SlaveModulated/SyncLost, any_coupled→Decoupling→Uncoupled) per contracts/state-machines.md §6 in `evo_control_unit/src/state/coupling.rs`
- [ ] T055 [US3] Implement bottom-up synchronization algorithm — detect when deepest slaves reach WAITING_SYNC, cascade SYNCHRONIZED upward to master, all axes in chain transition in same cycle (FR-052) in `evo_control_unit/src/state/coupling.rs`
- [ ] T056 [US3] Implement slave position calculation — `SLAVE_COUPLED: target = master_pos × ratio`, `SLAVE_MODULATED: target = master_pos × ratio + offset` (FR-051) in `evo_control_unit/src/state/coupling.rs`
- [ ] T057 [US3] Implement coupling error propagation — slave fault → ERR_SLAVE_FAULT on master, master fault → cascade decouple all slaves, sync timeout handling (FR-053) in `evo_control_unit/src/state/coupling.rs`
- [ ] T058 [US3] Implement master-slave lag difference monitoring — `|master_lag - slave_lag| > max_lag_diff` → ERR_LAG_DIFF_EXCEED (CRITICAL → SAFETY_STOP for all coupled axes) per FR-104 in `evo_control_unit/src/state/coupling.rs`
- [ ] T059 [US3] Implement OperationalMode **override** for coupled slaves — actively lock slave mode to match master, mirror mode changes from master (complements T040 rejection guard) per FR-041/FR-042 in `evo_control_unit/src/state/operational.rs`
- [ ] T060 [US3] Integrate coupling into cycle — evaluate CouplingState after PowerState/MotionState, before control engine, apply slave position targets in `evo_control_unit/src/cycle.rs`

**Checkpoint**: Multi-axis coupling works with deterministic synchronization. Fault propagation through coupling chains. US1–US3 all functional.

### Unit Tests (TDD — Constitution Principle II)

- [ ] T060a [P] [US3] Write unit tests for CouplingState transitions — couple/decouple, bottom-up sync (same-cycle SYNCHRONIZED), sync timeout in `evo_control_unit/tests/unit/coupling.rs`
- [ ] T060b [P] [US3] Write unit tests for coupling error propagation — slave fault cascade, master fault decouples all, lag diff CRITICAL in `evo_control_unit/tests/unit/coupling.rs`

---

## Phase 6: User Story 4 — Universal Position Control Engine with Lag Monitoring (Priority: P2)

**Goal**: Execute modular motion controller (PID + feedforward + DOB + filters) where each component is activated/deactivated by setting its gain parameters. Monitor lag error for safety. Produce complete `ControlOutputVector` every cycle.

**Independent Test**: Configure axis with different gain combinations (pure P, PI+FF, full PID+DOB+filters), observe output via evo_cu_hal, verify zero gains disable components. Set lag_error_limit, exceed it, verify ERR_LAG_EXCEED.

### Implementation

- [ ] T061 [P] [US4] Implement PID controller with backward Euler integration, derivative filter (Tf), anti-windup via back-calculation (Tt) — zero Ki disables integral, zero Kd disables derivative in `evo_control_unit/src/control/pid.rs`
- [ ] T062 [P] [US4] Implement feedforward controller — velocity FF (Kvff × target_velocity), acceleration FF (Kaff × target_acceleration), static friction compensation (Friction × sign(velocity)) — zero gains disable each in `evo_control_unit/src/control/feedforward.rs`
- [ ] T063 [P] [US4] Implement disturbance observer (DOB) — estimate disturbance from nominal model (Jn, Bn) and actual response, filter with gDOB bandwidth — zero gDOB disables entirely in `evo_control_unit/src/control/dob.rs`
- [ ] T064 [P] [US4] Implement signal conditioning filters — biquad notch filter (fNotch, BWnotch) and 1st-order low-pass filter (flp) — zero frequency disables each filter in `evo_control_unit/src/control/filters.rs`
- [ ] T065 [US4] Implement `ControlOutputVector` assembly — sum PID + FF + DOB, apply notch → lowpass → clamp(OutMax), populate all 4 fields (calculated_torque, target_velocity, target_position, torque_offset) per FR-102/FR-105 in `evo_control_unit/src/control/output.rs`
- [ ] T066 [US4] Implement lag error monitoring — compute `|target_pos - actual_pos|`, compare to `lag_error_limit`, dispatch per `lag_policy` (Critical→SAFETY_STOP all axes, Unwanted→axis MOTION_ERROR, Neutral→flag only, Desired→suppress) per FR-103 in `evo_control_unit/src/control/lag.rs`
- [ ] T067 [US4] Integrate control engine into cycle — for each axis in PowerState::Motion, compute control output via PID+FF+DOB+filters, **replace placeholder** from T044 with real ControlOutputVector in CuAxisCommand in `evo_control_unit/src/cycle.rs`
- [ ] T068 [US4] Implement `AxisControlState` reset on axis disable and mode change — zero PID integral, DOB state, filter state per invariants I-PW-4 and I-OM-4 in `evo_control_unit/src/control/output.rs`

**Checkpoint**: Full control engine runs every cycle. Lag monitoring active. Any gain combination works via zero-disable. US1–US4 all functional.

### Unit Tests (TDD — Constitution Principle II)

- [ ] T068a [P] [US4] Write unit tests for PID controller — pure P, PI, PID, anti-windup saturation, zero-gain disable, derivative filter in `evo_control_unit/tests/unit/control_pid.rs`
- [ ] T068b [P] [US4] Write unit tests for feedforward — velocity FF, acceleration FF, friction, zero-gain disable in `evo_control_unit/tests/unit/control_ff.rs`
- [ ] T068c [P] [US4] Write unit tests for DOB — disturbance estimation, zero-gDOB disable in `evo_control_unit/tests/unit/control_dob.rs`
- [ ] T068d [P] [US4] Write unit tests for filters — notch at resonance, low-pass smoothing, zero-frequency disable in `evo_control_unit/tests/unit/control_filters.rs`
- [ ] T068e [P] [US4] Write unit tests for lag monitoring — non-critical ERR_LAG_EXCEED, critical→SAFETY_STOP, coupling lag diff in `evo_control_unit/tests/unit/control_lag.rs`
- [ ] T068f [P] [US4] Write contract test asserting `ControlOutputVector` has all 4 non-NaN fields after every control cycle (FR-105/FR-132a compliance) in `evo_control_unit/tests/contract/control_output.rs`

---

## Phase 7: User Story 5 — Motion Range Monitoring (Priority: P3)

**Goal**: Monitor hardware limit switches and software position limits. Block motion beyond safe boundaries and reduce speed near limits.

**Independent Test**: Configure soft limits and connect hardware limits via mock evo_hal_cu. Approach limits, verify motion blocks and correct error codes in evo_cu_mqt.

### Implementation

- [ ] T069 [P] [US5] Implement hardware limit switch reading via IoRole — `hard_low_limit = io_registry.read_di(IoRole::LimitMin(axis_id), &di_bank)`, `hard_high_limit = io_registry.read_di(IoRole::LimitMax(axis_id), &di_bank)` (NC/NO logic applied automatically by IoRegistry) → set ERR_HARD_LIMIT, update limit_switch_ok flag per FR-110 in `evo_control_unit/src/safety/flags.rs`
- [ ] T070 [P] [US5] Implement software limit enforcement — check position vs min_pos/max_pos with `in_position_window` tolerance band (position ≥ max_pos − tolerance → ERR_SOFT_LIMIT), set ERR_SOFT_LIMIT, update soft_limit_ok flag, disable soft limits for unreferenced axes per FR-111/FR-035 in `evo_control_unit/src/safety/flags.rs`
- [ ] T071 [US5] Implement approach-speed reduction — calculate deceleration distance to boundary, reduce velocity command when within deceleration zone to guarantee stop before limit per FR-112 in `evo_control_unit/src/control/output.rs`

**Checkpoint**: Motion range fully protected. Hardware and software limits enforced. Speed reduction near boundaries.

---

## Phase 8: User Story 6 — Machine State and Loading Mode Management (Priority: P3)

**Goal**: Support different machine states (STOPPED→IDLE→MANUAL→ACTIVE→SERVICE→SYSTEM_ERROR) with proper operational contexts. Per-axis loading modes with configurable blocking/manual behavior.

**Independent Test**: Switch between machine states, verify axis behavior changes. Verify per-axis LoadingState blocks critical axes during loading while allowing manual positioning on non-critical axes.

### Implementation

- [ ] T072 [US6] Implement `LoadingState` transition logic — config-driven LOADING_BLOCKED/LOADING_MANUAL_ALLOWED, global loading trigger → per-axis transitions per contracts/state-machines.md §8 in `evo_control_unit/src/state/loading.rs`
- [ ] T073 [US6] Implement loading mode enforcement — reject motion commands for LOADING_BLOCKED axes (ERR_LOADING_MODE_ACTIVE), apply manual speed limits for LOADING_MANUAL_ALLOWED axes in `evo_control_unit/src/state/loading.rs`
- [ ] T074 [US6] Implement Manual mode management — enter MANUAL on first manual command, exit when all axes stop manual ops (with configurable timeout, default 30s), require AllowManualMode command per FR-003/FR-004 in `evo_control_unit/src/state/machine.rs`
- [ ] T075 [US6] Implement Service mode — authorization check, per-axis `ServiceBypassConfig` enforcement (only bypass_axes may operate, others locked per FR-001a), preserve axis states, apply SAFE_REDUCED_SPEED hardware limits, allow NoBrake power state per MachineState transition table in `evo_control_unit/src/state/machine.rs`
- [ ] T076 [US6] Implement `GearboxState` transition logic (Unknown→Neutral/GearN, GearN→Shifting→GearN/Neutral, Shifting→GearboxError, GearAssistMotion for oscillation) per contracts/state-machines.md §7 in `evo_control_unit/src/state/gearbox.rs`
- [ ] T077 [US6] Implement command source locking — AxisSourceLock acquisition/release/rejection with blocking source identification, pause target preservation across SAFETY_STOP per FR-135/FR-136/FR-137 in `evo_control_unit/src/command/source_lock.rs`
- [ ] T078 [US6] Implement RPC command dispatch — read `RpcCommand` from evo_rpc_cu, parse `RpcCommandType` (Jog, MoveAbsolute, Enable/Disable, Home, ResetError, SetMachineState, AllowManualMode, AcquireLock/ReleaseLock, ReloadConfig), enforce source lock rules per contracts/shm-segments.md §5 in `evo_control_unit/src/command/arbitration.rs`

**Checkpoint**: Full machine state management. Loading modes per-axis. Gearbox transitions. Dual command source (RE + RPC) with arbitration.

---

## Phase 9: User Story 7 — Role-Based I/O Configuration (Priority: P3)

**Goal**: All I/O points defined in `io.toml` with functional roles. CU and HAL resolve I/O by role via `IoRegistry`. NC/NO logic, analog scaling, and inversion handled per-point in config.

**Independent Test**: Define `io.toml` with roles for limit switches (NC), brake (inverted), E-Stop (NC). Start CU, verify role resolution. Change NC→NO, verify logic. Remove required role, verify startup rejection.

### Implementation

- [ ] T079 [US7] Implement `io.toml` loading in CU startup — parse `IoConfig` from file path in `ControlUnitConfig.io_config_path`, build `IoRegistry`, run all validations (V-IO-1 through V-IO-7), validate role completeness for each axis, refuse startup on any validation failure in `evo_control_unit/src/config.rs`
- [ ] T080 [US7] Replace all direct DI/AI/DO/AO index access in CU with `IoRegistry` role-based API — audit and update all I/O reads in `evo_control_unit/src/safety/peripherals.rs`, `evo_control_unit/src/safety/flags.rs`, `evo_control_unit/src/command/homing.rs` to use `io_registry.read_di(role, &di_bank)` etc. per FR-152
- [ ] T080a [US7] Implement conditional enable logic for two-hand operation — DI points with `enable_pin`/`enable_state`/`enable_timeout` fields, validate both signals within timeout window in `evo_common/src/io/registry.rs`
- [ ] T080b [US7] Create example `config/io.toml` for test_8axis reference config with all required roles (EStop, 8× LimitMin/LimitMax, homing sensors, brake, guard, tailstock) per contracts/io-config.md

**Checkpoint**: All I/O resolved by role via IoRegistry. NC/NO, scaling, inversion handled automatically. io.toml is single source of truth. US1–US7 all functional.

---

## Phase 10: User Story Supplement — Homing Supervision (P1 dependency, P3 implementation)

**Goal**: Supervise axis homing procedure (6 methods) with unreferenced axis restrictions. CU supervises; command originates from RE.

**Independent Test**: Issue Home command from mock evo_re_cu for each homing method, verify MotionState::Homing with correct method-specific logic, verify unreferenced axis speed restriction (5% max).

### Implementation

- [ ] T081 [P] [US1] Implement unreferenced axis motion policy — read `referenced` flag from evo_hal_cu, restrict unreferenced axes to MANUAL/SERVICE at 5% max velocity, disable soft limits, reject ACTIVE commands with ERR_NOT_REFERENCED per FR-035 in `evo_control_unit/src/state/motion.rs`
- [ ] T082 [US1] Implement homing supervision for all 6 methods (HARD_STOP current threshold, HOME_SENSOR trigger via `IoRole::Ref(N)`, LIMIT_SWITCH trigger via `IoRole::LimitMin/Max(N)`, INDEX_PULSE two-phase via `sensor_role`+`index_role`, ABSOLUTE offset, NO_HOMING immediate) with timeout protection per FR-032/FR-034 in `evo_control_unit/src/command/homing.rs`
- [ ] T083 [US1] Implement MotionState::Homing entry/exit — verify PowerState::Standby/Motion, apply homing speed limit, homing torque limit, success→referenced=true+position=0, failure→ERR_HOMING_FAILED per FR-031 in `evo_control_unit/src/command/homing.rs`

**Checkpoint**: All homing methods supervised. Unreferenced axis restrictions enforced.

---

## Phase 11: Polish & Cross-Cutting Concerns

**Purpose**: Observability, performance validation, documentation

- [ ] T084 [P] Implement evo_cu_mqt snapshot writer — populate `CuToMqtSegment` fields (machine_state, safety_state, per-axis `AxisStateSnapshot` including all state enums, error bitflags, position/velocity/lag/torque) from runtime state every N cycles per FR-134 in `evo_control_unit/src/shm/writer.rs`
- [ ] T085 [P] Implement evo_cu_mqt update rate throttling — write diagnostic segment every N cycles (configurable, default: 10 = 10ms) instead of every cycle per FR-134 in `evo_control_unit/src/shm/writer.rs`
- [ ] T086 [P] Implement optional segment dynamic connect/disconnect — detect when evo_re_cu or evo_rpc_cu appear/disappear, connect on next cycle, release source locks on staleness per FR-139 in `evo_control_unit/src/shm/segments.rs`
- [ ] T087 [P] Create criterion benchmark `evo_control_unit/benches/cycle_benchmark.rs` — measure full cycle time for 8-axis and 64-axis configurations, validate <1ms per SC-001
- [ ] T088 [P] Create criterion benchmark `evo_control_unit/benches/pid_benchmark.rs` — measure control engine throughput for single axis, validate ~0.4µs per axis per research.md Topic 11
- [ ] T089 [P] Create minimal integration test `evo_control_unit/tests/integration/startup.rs` — config load → SHM connect → Starting → Idle transition with mock evo_hal_cu
- [ ] T090 [P] Create integration test `evo_control_unit/tests/integration/safety_stop.rs` — trigger CRITICAL error → SAFETY_STOP → verify per-axis SafeStopCategory execution → recovery
- [ ] T091 [P] Create integration test `evo_control_unit/tests/integration/coupling.rs` — master + 2 slaves → sync → move → fault cascade
- [ ] T092 Create reference test config files `config/test_8axis.toml` + `config/test_io.toml` per quickstart.md (8 axes, mixed params, coupling pair, brake+tailstock+guard; io.toml with all required roles) for SC-002/SC-004 benchmarks
- [ ] T093 Run quickstart.md validation — verify build, run, test commands all work; update quickstart.md if paths or commands changed

### Success Criteria Validation (F15)

- [ ] T094 [P] Create accuracy validation test `evo_control_unit/tests/integration/control_accuracy.rs` — step response with mock HAL, verify steady-state error < 0.1mm for reference axis config per SC-004
- [ ] T095 [P] Create recovery timing benchmark `evo_control_unit/benches/recovery_benchmark.rs` — measure SYSTEM_ERROR→Idle recovery latency, validate <100ms per SC-009
- [ ] T096 Create startup timing test `evo_control_unit/tests/integration/startup_timing.rs` — measure POWER_OFF→STANDBY for 8-axis reference config, validate <500ms per SC-002
- [ ] T097 [P] Create 24-hour soak test `evo_control_unit/tests/integration/soak_24h.rs` — run continuous motion profiles (position+velocity+homing cycles) with simulated HAL for 24 hours, verify zero false-positive SAFETY_STOP triggers and stable memory/timing per SC-008

### Hot-Reload Implementation (FR-144–FR-147)

- [ ] T098 Implement shadow-config parser — during SAFETY_STOP, read config file into temporary `CuMachineConfig`, run full validation (axis ID uniqueness, coupling graph acyclicity, parameter bounds, reloadable-scope check) per FR-146 in `evo_control_unit/src/config.rs`
- [ ] T099 Implement atomic config swap with rollback — if validation passes: atomic pointer swap `active_config ← shadow_config`; if fails: discard shadow, report `ERR_RELOAD_VALIDATION_FAILED` via `evo_cu_mqt`; enforce ≤120ms total duration per FR-146/FR-147 in `evo_control_unit/src/config.rs`
- [ ] T100 Implement `RELOAD_CONFIG` command handling — accept from `evo_rpc_cu` (`RpcCommandType::ReloadConfig`), reject with `ERR_RELOAD_DENIED` if `SafetyState != SafetyStop`, report reload success via updated `evo_cu_mqt` snapshot per FR-145/FR-147 in `evo_control_unit/src/command/arbitration.rs`
- [ ] T101 [P] Create hot-reload integration test `evo_control_unit/tests/integration/hot_reload.rs` — trigger SAFETY_STOP → send RELOAD_CONFIG with valid/invalid configs → verify atomic swap, rollback on failure, ERR_RELOAD_DENIED outside E-STOP, reload success reflected in `evo_cu_mqt` snapshot per FR-144–FR-147

---

## Dependencies & Execution Order

### Phase Dependencies

```text
Phase 1: Setup ──────────────────────────── (no deps, start immediately)
    │
    ▼
Phase 2: Foundational ──────────────────── (depends on Phase 1)
    │                                         ⚠️ BLOCKS all user stories
    ▼
Phase 3: US1 - Power Lifecycle (P1) ─────── MVP 🎯
    │
    ▼
Phase 4: US2 - Safety Peripherals (P1) ──── (uses safety flags from Phase 2, integrates with US1 power sequences)
    │
    ├──────────────────────┬──────────────────────────┐
    ▼                      ▼                          ▼
Phase 5: US3 (P2)    Phase 6: US4 (P2)         Phase 10: Homing
  Coupling              Control Engine             (US1 supplement)
    │                      │                          │
    ├──────────────────────┘                          │
    ▼                                                 │
Phase 7: US5 - Range Monitoring (P3) ────────────────┘ (needs control output from US4)
    │
    ▼
Phase 8: US6 - Machine State & Loading (P3) ──── (needs all state machines from US1–US5)
    │
    ▼
Phase 9: US7 - NC/NO Config (P3) ─────────────── (cross-cutting: audit all DI reads)
    │
    ▼
Phase 11: Polish ──────────────────────────────── (after all stories complete)
```

### User Story Dependencies

- **US1 (P1)**: Depends on Phase 2 only — **MVP scope**, can be delivered standalone
- **US2 (P1)**: Depends on Phase 2 + partially on US1 (PowerState sequences use safety checks)
- **US3 (P2)**: Depends on Phase 2 + US1 (MotionState/PowerState must exist) — can start after US1
- **US4 (P2)**: Depends on Phase 2 + US1 (MotionState::Motion triggers control) — can run **in parallel with US3**
- **US5 (P3)**: Depends on Phase 2 + US4 (needs control output for speed reduction)
- **US6 (P3)**: Depends on Phase 2 + US1 (MachineState transitions)
- **US7 (P3)**: Depends on US2 (all peripheral monitoring must exist to audit)
- **Homing (Phase 10)**: Depends on US1 (MotionState/PowerState) — can run in parallel with US3/US4

### Within Each User Story

1. Types/enums before logic that uses them (handled in Phase 2)
2. State machine transitions before integration into cycle
3. Core implementation before SHM output
4. Story-specific logic before cross-story integration points

### Parallel Opportunities

**Phase 2** (maximum parallelism — 26 tasks marked [P]):
```
T006 ─┐                T017 ─┐
T007 ─┤ evo_common     T018 ─┤ SHM segment
T008 ─┤ state/error    T019 ─┤ payload types
T009 ─┤ types          T020 ─┤ (all [P])
T010 ─┤                T021 ─┤
T011 ─┤                T022 ─┘
T012 ─┤                
T013 ─┤                T025 ─┐ config
T014 ─┤                T026 ─┘ types
T015 ─┤
T016 ─┘                T026a─┐ I/O types
                       T026b─┤ (all [P])
                       T026c─┤
                       T026d─┤
                       T026e─┤
                       T026f─┤
                       T026g─┘
```

**After Phase 4** (US3 and US4 are fully parallel):
```
Phase 5: US3 (Coupling) ──── in parallel ──── Phase 6: US4 (Control Engine)
                         └─── in parallel ──── Phase 10: Homing
```

**Phase 11** (all [P] tasks independent):
```
T084 ─┬─ T085 ─┬─ T086 ─┬─ T087 ─┬─ T088 ─┬─ T089 ─┬─ T090 ─┬─ T091
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: Single axis powers on, moves, powers off. State visible via SHM.
5. Demo-ready at ~44 tasks (T001–T044)

### Incremental Delivery

1. Setup + Foundational → Foundation ready (T001–T035)
2. Add US1 → Test power lifecycle independently → **MVP** (T036–T044)
3. Add US2 → Test safety independently → Safety-qualified core (T045–T053)
4. Add US3 + US4 in parallel → Multi-axis sync + control quality (T054–T068)
5. Add US5 + US6 + US7 → Full feature set (T069–T083)
6. Polish → Production-ready (T084–T097)
7. Hot-Reload → Config reload during E-STOP (T098–T101)

---

## Notes

- All `evo_common` types use `#[repr(u8)]` enums and `#[repr(C)]` structs — verify with `static_assertions`
- Zero heap allocation in RT cycle — enforced by custom allocator in test builds (research.md Topic 1)
- All state machines use exhaustive `match` — compiler enforces handling of every variant
- P2P SHM requires `evo_shared_memory` P2P migration — if not yet complete, mock SHM layer in Phase 2
- Commit after each task or logical group. Run `cargo check -p evo_control_unit` between tasks.
