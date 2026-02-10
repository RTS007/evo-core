# Feature Specification: Control Unit - Axis Control Brain

**Feature Branch**: `005-control-unit`  
**Created**: 2025-01-06  
**Updated**: 2026-02-04  
**Status**: Draft  
**Input**: User description: "Control Unit - axis control brain for safety, peripheral management, and motion coordination integrated with HAL and shared memory"

## Clarifications

### Session 2026-01-06

- Q: What is the SHM ownership model between evo_hal and control_unit? → A: **Two segments with clear ownership** - `hal_status` (single-writer: evo_hal) + `hal_command` (single-writer: control_unit).

### Session 2026-02-04

- Q: What is the state machine architecture? → A: **Hierarchical orthogonal state machines** with 5 levels inspired by PackML/ISA-88.

### Session 2026-02-05

- Q: What is the configuration file structure? → A: **1 main file + 1 file per machine + helper files** (to be created).
- Q: What is the SHM structure for state machines? → A: **One complete structure for machine state and all axes**, updated every cycle.
- Q: How is loading mode configured per axis? → A: Each axis has **loading_blocked=true/false** and **loading_manual=true/false** in config.
- Q: How is manual mode controlled? → A: **Recipe Executor or any program** sends `AllowManualMode` command to Control Unit. Only then axis moves manually, regardless of program state. Not allowed when axis already in motion.
- Q: What is the error recovery process? → A: Requires **reset button + manual authorization** for motion (additional confirmation on screen or start button).

### Session 2026-02-07

- Q: How does Control Unit receive commands? → A: **P2P SHM segments** — each module pair has a dedicated segment (`evo_[SOURCE]_[DESTINATION]`). Commands from Recipe Executor via `evo_re_cu`, from gRPC via `evo_rpc_cu`. Writer creates segment, reader connects only to segments addressed to it.
- Q: What is the crash recovery / watchdog strategy? → A: **External evo_watchdog** program monitors heartbeat of all system programs. Uses existing evo_common mechanisms. Not part of Control Unit spec.
- Q: Is homing mandatory before production motion? → A: **HAL informs CU** about referencing need. Without homing: only MANUAL or SERVICE at **5% max speed**. Safety stops (lag, limits, etc.) still enforced. CU supervises homing, command comes from Recipe Executor. RE cannot run other programs until referenced (RE out of scope).
- Q: What happens if control cycle exceeds 1ms deadline? → A: **Hard deadline** — single overrun triggers `SAFETY_STOP`.
- Q: How are conflicting simultaneous commands resolved? → A: **Source locking with error reporting**. Active source (e.g., Recipe Executor) owns axis control until release. Other sources get rejection with reason (who blocks). Safety has override priority but **pauses** rather than cancels — after safety condition cleared, work resumes with pre-e-stop targets unless conditions changed (e.g., recipe stopped during e-stop).
- Q: How does CU detect a crashed P2P SHM writer (stale segment)? → A: **Heartbeat counter in segment** — each writer increments a monotonic cycle counter; reader detects stale data if counter unchanged for N cycles. Watchdog serves as secondary backstop.
- Q: How to handle 002-shm-lifecycle broadcast→P2P incompatibility? → A: **Both**: Declare P2P requirements in this spec AND include a migration appendix documenting the delta between current broadcast API and required P2P API. 002-shm-lifecycle gets a separate update pass.
- Q: What about missing `evo_cu_re` segment? → A: **Prepared but not filled** — `evo_cu_re` segment is declared with a placeholder struct; content definition will be developed in a future spec iteration.
- Q: How to prevent SHM struct mismatch between CU and HAL versions? → A: **Struct version hash in header** — writer stores a compile-time hash of the struct layout at segment creation; reader validates at connect time and refuses on mismatch.
- Q: What if optional SHM segments (RE, gRPC) are missing at CU startup? → A: **HAL mandatory, others optional** — CU starts with only `evo_hal_cu`; command sources connect/disconnect dynamically; missing source = no commands from that source.
- Q: What is the RT cycle memory allocation policy? → A: **Defined in project constitution** (Principles XIII, XIV, XXIV) — pre-allocate at startup, zero allocation in RT cycle, mlock/hugetlbfs for RT memory.
- Q: What is the canonical axis identification scheme? → A: **Numeric index (1-based)** — axes addressed 1..N matching industrial convention; SHM uses fixed-size arrays; config maps index ↔ human name.
- Q: SC-002/SC-004 use "typical" — how to make testable? → A: **Reference config**: SC-002 = 8-axis machine with brake+tailstock+guard; SC-004 = axis with 10kg load, 1m/s max velocity, 500mm travel.
- Q: How does CU log during RT cycle without I/O? → A: **SHM-only diagnostics** — all diagnostic data flows through `evo_cu_mqt` segment; no file I/O from CU; downstream consumers handle persistence.

### Session 2026-02-09

- Q: Can `AllowManualMode` be sent from gRPC (`evo_rpc_cu`) in addition to Recipe Executor (`evo_re_cu`)? → A: **Both segments** — `AllowManualMode` available via `evo_re_cu` and `evo_rpc_cu`, so operators can jog axes from dashboard without RE running.
- Q: What does the `evo_cu_mqt` event ring contain (event types, ring size)? → A: **No ring buffer** — `evo_cu_mqt` contains only the live current status snapshot of CU (states, flags, errors). Event history/logging is out of scope.
- Q: Should soft limits trigger immediately on boundary crossing? → A: **Tolerance band** — use HAL's `in_position_window` parameter as tolerance. Axis is allowed to reach the limit value; minor oscillations or vibrations within `in_position_window` beyond the limit MUST NOT trigger `ERR_SOFT_LIMIT`. Error triggers only when `position < min_pos - in_position_window` or `position > max_pos + in_position_window`.
- Q: Is homing approach direction mandatory and configurable? → A: **Mandatory for all movement-based homing methods** — `approach_direction` (Positive/Negative) is a required parameter. Critical for rotary axes with material: homing MUST proceed in the unwinding direction to prevent material damage or catastrophic entanglement.

---

## 🏗️ Architecture Overview

This Control Unit implements a **hierarchical state machine architecture** based on industrial standards (PackML/ISA-88 inspired).

### Key Architectural Decisions

1. **Orthogonal State Machines** - Each axis has multiple independent state dimensions
2. **Loading State per Axis** - Not a global machine state
3. **Homing as Dedicated MotionState** - Unique characteristics require separation
4. **Hierarchical Error Propagation** - Errors propagate up master-slave chains
5. **Gear Assist Motion** - Separate motion state for gear change assistance

### Architecture Levels

```
LEVEL 1: Machine State (global system state)
         ├── MachineState enum (STOPPED, IDLE, ACTIVE, etc.)
         └── Affects all components globally

LEVEL 2: Safety State (global safety state)
         ├── SafetyState enum (SAFE, SAFETY_STOP, etc.)
         └── Overrides machine state when triggered

LEVEL 3: Axis State (per-axis, orthogonal)
         ├── PowerState (power/brake management)
         ├── MotionState (movement state)
         ├── OperationalMode (control mode)
         ├── CouplingState (master-slave coordination)
         ├── GearboxState (gear management)
         └── LoadingState (loading mode per axis)

LEVEL 4: Axis Safety Flags (per-axis safety conditions)
         └── Boolean flags for tailstock, locking pin, brake, guards, limits

LEVEL 5: Error Flags (hierarchical error management)
         └── Per-level error states with propagation rules
```

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Basic Axis Power Lifecycle Management (Priority: P1)

As a control engineer, I want to start and stop an axis through its complete power lifecycle using orthogonal state machines (PowerState transitions: POWER_OFF → POWERING_ON → STANDBY → MOTION → POWERING_OFF → POWER_OFF), so that I have deterministic control over axis states and safe operation.

**Why this priority**: This is the core functionality of the Control Unit. Without proper axis state management, no motion is possible. Every other feature depends on this foundation.

**Independent Test**: Configure a single axis, issue a start command, verify state transitions through SHM, issue a stop command, verify return to POWER_OFF state. All orthogonal states observable via SHM.

**Acceptance Scenarios**:

1. **Given** an axis in `PowerState::POWER_OFF` with all safety conditions met (brake engaged, locking pin locked), **When** a motion request is issued, **Then** the axis transitions to `PowerState::POWERING_ON` and begins the startup sequence (locking pin retract, drive enable, brake release).
2. **Given** an axis in `PowerState::POWERING_ON` with brake released and locking pin retracted, **When** drive confirms ready and position is stable, **Then** the axis transitions to `PowerState::STANDBY`.
3. **Given** an axis in `PowerState::STANDBY` receiving a motion command, **When** the command is valid and safety conditions met, **Then** the axis transitions to `PowerState::MOTION` and `MotionState` changes from `STANDSTILL` to appropriate motion state.
4. **Given** an axis in `PowerState::MOTION` receiving a stop command, **When** deceleration completes, **Then** the axis transitions through `MotionState::STOPPING` → `MotionState::STANDSTILL`, then `PowerState::POWERING_OFF` → `PowerState::POWER_OFF`.
5. **Given** an axis in any state, **When** a critical error occurs, **Then** `MotionState` transitions to `EMERGENCY_STOP`, then `MOTION_ERROR`, and `SafetyState` changes to `SAFETY_STOP`.

---

### User Story 2 - Safety Peripheral Integration (Priority: P1)

As a safety engineer, I want the Control Unit to continuously monitor safety peripherals (tailstock, locking pin, brake, safety guards) and block or halt motion when conditions are unsafe, so that machine operation meets safety requirements.

**Why this priority**: Safety is non-negotiable. Without peripheral monitoring, the system cannot prevent hazardous situations. Equally important as axis lifecycle.

**Independent Test**: Configure safety peripherals in config, simulate unsafe conditions (open guard, missing brake confirmation), verify axis cannot start or stops immediately via `SafetyState` transition.

**Acceptance Scenarios**:

1. **Given** a tailstock configured for an axis, **When** the tailstock is open (di_tailstock_closed = false), **Then** the axis MUST NOT transition from `PowerState::POWER_OFF` and `AxisSafetyState.tailstock_ok = false` with error `ERR_DRIVE_TAIL_OPEN`.
2. **Given** an axis with `PowerState::MOTION` and tailstock configured, **When** the tailstock opens during operation, **Then** immediate `SafetyState::SAFETY_STOP` is triggered with `PowerError::ERR_DRIVE_TAIL_OPEN`.
3. **Given** a locking pin configured for an axis, **When** the locking pin is still locked (di_index_locked = true), **Then** `AxisSafetyState.lock_pin_ok = false` and axis remains in `PowerState::POWERING_ON` until cleared or timeout with `ERR_LOCK_PIN_TIMEOUT`.
4. **Given** a brake configured for an axis, **When** brake release confirmation is not received within timeout, **Then** the system reports `PowerError::ERR_BRAKE_TIMEOUT` and axis returns to `PowerState::POWER_OFF`.
5. **Given** a safety guard configured with secure_speed limit, **When** axis speed exceeds secure_speed and guard is not closed/locked, **Then** `SafetyState::SAFETY_STOP` is triggered and axis executes its configured SafeStopCategory (STO/SS1/SS2).

---

### User Story 3 - Master-Slave Synchronization (Priority: P2)

As an integrator, I want multiple axes to synchronize using master-slave coupling (CouplingState: MASTER, SLAVE_COUPLED, SLAVE_MODULATED, WAITING_SYNC, SYNCHRONIZED), ensuring deterministic multi-axis behavior for coordinated moves or synchronized winding.

**Why this priority**: Many industrial applications require coordinated motion. Without synchronization, complex machine sequences cannot be implemented. Important for MVP completeness.

**Independent Test**: Configure master with 2 slaves, issue start command, verify all axes wait in `CouplingState::WAITING_SYNC` until all are ready, then all transition to `SYNCHRONIZED` in the same cycle.

**Acceptance Scenarios**:

1. **Given** axis A configured as `MASTER` with axes B and C as `SLAVE_COUPLED`, **When** axis A receives start command, **Then** axis A transitions to `CouplingState::WAITING_SYNC` and waits for B and C.
2. **Given** all axes in a coupling chain are in `WAITING_SYNC`, **When** the last axis (deepest slave) reaches `WAITING_SYNC`, **Then** synchronization cascades bottom-up and ALL transition to `SYNCHRONIZED` in the same cycle.
3. **Given** slave axis B fails to reach `WAITING_SYNC` within its timeout, **When** timeout expires, **Then** slave B gets `CouplingError::ERR_SYNC_TIMEOUT`, master A gets `CouplingError::ERR_SLAVE_FAULT` (propagated), and motion is blocked.
4. **Given** axis B is `SLAVE_MODULATED` to master A, **When** master moves, **Then** slave position = `master_pos × coupling_ratio + modulation_offset`.

---

### User Story 4 - Universal Position Control Engine with Lag Monitoring (Priority: P2)

As a control engineer, I want the Control Unit to execute a universal motion controller with modular components (PID, feedforward, disturbance observer, filters) where I can activate/deactivate each element by setting its gain parameters, and monitor lag error (Schleppfehler) for safety, so that I can optimize control performance for any mechanical configuration while ensuring safe operation and blockage detection.

**Why this priority**: Position control quality, flexibility and error detection are essential for any positioning application. Universal approach allows one system to handle everything from simple linear actuators to complex multi-axis robots while maintaining safety through lag monitoring.

**Independent Test**: Configure axis with different gain combinations (pure P, PI+FF, full PID+DOB+filters), observe control response, verify that zero gains completely disable components, artificially increase lag beyond limit to verify safety monitoring works independently of control algorithm selection.

**Acceptance Scenarios**:

1. **Given** an axis with `Kp=5.0, Ki=0.0, Kd=0.0` (pure P control), **When** Control Unit cycle executes, **Then** control output = `Kp * position_error` only, with I and D terms completely inactive.
2. **Given** an axis with `Kvff=2.0, Kaff=0.1, Kp=1.0` (feedforward + P), **When** trajectory command issued, **Then** feedforward immediately provides base current while P term handles remaining error.
3. **Given** an axis with `gDOB=50.0, Jn=0.5, Bn=0.02` (active disturbance observer), **When** external load applied, **Then** DOB detects disturbance and compensates within observer bandwidth timeframe.
4. **Given** an axis with `fNotch=120.0, BWnotch=10.0` (notch filter active), **When** control signal contains 120Hz resonance, **Then** notch filter eliminates frequency component while preserving other frequencies.
5. **Given** an axis with all gains set to 0.0 except `Friction=0.5`, **When** motion commanded, **Then** only friction compensation current is applied, demonstrating complete component modularity.
6. **Given** an axis with lag_error_limit = 2.0mm, **When** |target_pos - actual_pos| > 2.0mm, **Then** behavior depends on `lag_policy`: `Critical` → `MotionError::ERR_LAG_CRITICAL` set, global `SafetyState::SAFETY_STOP` for ALL axes. `Unwanted` (default) → `MotionError::ERR_LAG_EXCEED` set, axis-local `MotionState::MOTION_ERROR`. `Neutral` → `MotionError::ERR_LAG_EXCEED` set as informational flag, no stop. `Desired` → no error flag set.
7. **Given** a `SLAVE_COUPLED` axis, **When** slave lag differs from master lag by more than max_lag_diff, **Then** `CouplingError::ERR_LAG_DIFF_EXCEED` is set for both and ALL coupled axes enter `MotionState::EMERGENCY_STOP`.
8. **Given** an axis with lag_policy = Critical, **When** lag error occurs, **Then** ALL axes globally enter `MotionState::EMERGENCY_STOP` (critical error propagation).

---

### User Story 5 - Motion Range Monitoring (Priority: P3)

As a safety engineer, I want the Control Unit to monitor both hardware limit switches and software position limits, preventing motion beyond safe boundaries and reducing speed when approaching limits.

**Why this priority**: Prevents mechanical damage and ensures safe operation. Important but secondary to basic lifecycle and safety peripherals.

**Independent Test**: Configure soft limits and connect hardware limit switches, approach limits, verify motion blocks via `AxisSafetyState.limit_switch_ok` / `soft_limit_ok` and correct error codes are reported.

**Acceptance Scenarios**:

1. **Given** hardware limit switch triggered (considering NC/NO configuration), **When** Control Unit reads input, **Then** `AxisSafetyState.limit_switch_ok = false`, `MotionError::ERR_HARD_LIMIT` is set, motion in that direction is blocked.
2. **Given** axis position exceeds configured soft_limit_max by more than `in_position_window`, **When** Control Unit checks limits, **Then** `AxisSafetyState.soft_limit_ok = false`, `MotionError::ERR_SOFT_LIMIT` is set, motion in that direction is blocked. Minor oscillations within `in_position_window` beyond the limit do NOT trigger the error.
3. **Given** axis approaching a limit within configured deceleration distance, **When** Control Unit calculates limits, **Then** speed is reduced to ensure axis can stop before exceeding boundary.

---

### User Story 6 - Machine State and Loading Mode Management (Priority: P3)

As an operator, I want the system to support different machine states (STOPPED, IDLE, MANUAL, ACTIVE, SERVICE) and per-axis loading modes, so that I have proper operational contexts and safe part changeover.

**Why this priority**: Enables different operational contexts. Loading mode is essential for safe part changeover. Service mode aids commissioning.

**Independent Test**: Switch between machine states, verify axis behavior changes accordingly. Verify per-axis `LoadingState` blocks critical axes during loading while allowing manual positioning on non-critical axes.

**Acceptance Scenarios**:

1. **Given** system in `MachineState::IDLE`, **When** all safety conditions are met and a valid motion command is received, **Then** axes in `PowerState::STANDBY` can transition to `PowerState::MOTION` and `MachineState` transitions to `ACTIVE` (if command from RE) or `MANUAL` (if manual command with `AllowManualMode`).
2. **Given** system in `MachineState::SYSTEM_ERROR` (triggered by `SafetyState::SAFETY_STOP`), **When** any axis start is requested, **Then** request is rejected and axes remain in `PowerState::POWER_OFF`.
3. **Given** an axis with `loading_blocked=true` in loading mode, **When** motion command is issued, **Then** command is rejected with ERR_LOADING_MODE_ACTIVE and axis remains in `LoadingState::LOADING_BLOCKED`.
4. **Given** an axis with `loading_manual=true` in loading mode and `AllowManualMode` command received, **When** manual positioning command is issued, **Then** command is accepted and axis can be moved via `OperationalMode::MANUAL`.
5. **Given** `MachineState::SERVICE` is active, **When** technician issues commands, **Then** configurable safety limits are bypassed per FR-001a (soft limits, guard requirement, unreferenced speed cap) while hardware safety remains active, and `SafetyState::SAFE_REDUCED_SPEED` hardware speed limits apply.

---

### User Story 7 - Role-Based I/O Configuration (Priority: P3)

As an integrator, I want all I/O points defined in a dedicated `io.toml` file with functional roles (e.g., `EStop`, `LimitMin1`, `BrakeOut1`), so that the Control Unit and HAL resolve I/O by role rather than by hardcoded names or indices, and NC/NO logic is handled per-point in the config.

**Why this priority**: Centralizes I/O configuration, eliminates duplicate definitions across HAL and CU configs, supports any sensor wiring convention, and scales to large machines.

**Independent Test**: Define `io.toml` with roles for limit switches (NC), brake (inverted), E-Stop (NC). Start CU, verify role resolution maps roles to correct pins. Change NC→NO in config, verify correct logic interpretation. Add missing required role, verify startup validation rejects config.

**Acceptance Scenarios**:

1. **Given** a limit switch configured with `role="LimitMin1"` and `logic="NC"`, **When** input reads FALSE, **Then** limit is considered ACTIVE (triggered), `AxisSafetyState.limit_switch_ok = false`.
2. **Given** a limit switch configured with `role="LimitMin1"` and `logic="NO"`, **When** input reads TRUE, **Then** limit is considered ACTIVE (triggered), `AxisSafetyState.limit_switch_ok = false`.
3. **Given** a brake confirmation input configured with `role="BrakeIn1"` and `logic="NC"`, **When** wire breaks (input = FALSE), **Then** system fails safe by interpreting as "brake not released", `AxisSafetyState.brake_ok = false`.
4. **Given** `io.toml` is missing a role required by an axis config (e.g., axis 1 has tailstock but no `TailClosedN` role), **When** CU starts, **Then** startup fails with `ERR_IO_ROLE_MISSING` listing the missing role.
5. **Given** a role `PressureOk` assigned to an AI pin with `max=10.0, unit="bar"`, **When** CU reads I/O, **Then** the analog value is available via `IoRegistry::read_ai(IoRole::PressureOk)` in engineering units.

---

### Edge Cases

- What happens when brake release times out during `POWERING_ON`? → Axis returns to `PowerState::POWER_OFF` with `PowerError::ERR_BRAKE_TIMEOUT`.
- How does system handle conflicting sensor states (e.g., tailstock both open and closed)? → ERR_SENSOR_CONFLICT, `AxisSafetyState.tailstock_ok = false`, axis blocked.
- What happens if coupling group has only master (no slaves)? → Master operates normally without waiting for synchronization.
- How does system handle drive reporting motion while commanded speed is zero? → `MotionError::ERR_W_DRIVE_ZEROSPEED`, immediate `SafetyState::SAFETY_STOP`.
- What happens when SHM communication with HAL fails? → Heartbeat counter stale for N cycles (FR-130c) → `ERR_HAL_COMMUNICATION`, `SAFETY_STOP` → `MachineState::SYSTEM_ERROR`.
- How does system recover from `SAFETY_STOP`? → Requires reset button press, all safety conditions satisfied, manual authorization (screen confirmation/start button), then `SafetyState::SAFE`. SS2 axes can resume immediately, SS1/STO axes need drive restart.
- What happens when slave axis has critical error? → Error propagates up chain via `CouplingError::ERR_SLAVE_FAULT`, all axes in chain execute their configured SafeStopCategory.
- How does SS2 behave during power loss? → SS2 axes transition to STO (power cut), engage brake, lose position holding capability.
- What if Recipe Executor and gRPC both try to command same axis? → Source locking: first source to take control wins. Second source gets `CommandError::ERR_SOURCE_LOCKED` with identifier of blocking source.
- What happens after SAFETY_STOP recovery when recipe was running? → If recipe is still active after recovery, motion resumes with pre-e-stop targets. If recipe was stopped during e-stop, source lock is released and targets are discarded.
- Can unreferenced axis trigger SAFETY_STOP? → Yes, all safety monitoring (lag, hardware limits, brake, tailstock) is active. Only software limits are disabled.
- What if cycle overruns during SAFETY_STOP execution? → Still triggers ERR_CYCLE_OVERRUN. SAFETY_STOP reaction is designed to complete within cycle time.
- What if SHM struct version doesn't match between writer and reader? → `ERR_SHM_VERSION_MISMATCH` at connect time; mandatory segments (evo_hal_cu) prevent CU startup, optional segments are rejected silently.
- What if Recipe Executor or gRPC is not running at CU startup? → CU starts normally with HAL only (FR-139); optional sources connect/disconnect dynamically.
- What if axis oscillates around a soft limit due to vibrations? → `in_position_window` tolerance prevents false triggers. Error fires only when `position` exceeds limit by more than `in_position_window` (FR-111). Pre-move target validation still uses exact limit.
- What if homing approach_direction is not specified in config? → Config validation error at startup (`ConfigError::ValidationError`) for all movement-based homing methods (FR-033a). No default direction — integrator must choose explicitly.
- What if homing proceeds in wrong direction on rotary axis with material? → Catastrophic material entanglement / mechanical damage. Mitigated by mandatory `approach_direction` config (FR-033a) and commissioning procedure.

---

## Requirements *(mandatory)*

### Functional Requirements

#### LEVEL 1: Machine State Management

- **FR-001**: System MUST support the following global machine states (`MachineState`):
  - `STOPPED`: Initial state after boot, drive disabled
  - `STARTING`: System initialization in progress
  - `IDLE`: Ready for operation, no active programs or manual motion
  - `MANUAL`: Manual operation, axes moved by operator (pedal, joystick)
  - `ACTIVE`: Recipe/program execution in progress
  - `SERVICE`: Service mode, configurable safety bypass (see FR-001a)
  - `SYSTEM_ERROR`: System-wide error (power, hydraulics, air, communication)

- **FR-001a**: In `MachineState::SERVICE`, the following safety limits MAY be bypassed (each individually configurable per axis):
  - Software position limits (`soft_limit_ok` check disabled)
  - Guard-closed requirement at low speed (guard can remain open if speed < `secure_speed`)
  - Unreferenced axis restrictions relaxed (manual motion allowed without 5% speed cap)
  - **NOT bypassable**: Hardware limit switches, brake monitoring, tailstock safety, SAFETY_STOP triggers, cycle overrun detection. These remain active to protect hardware.

- **FR-002**: `MachineState` transitions MUST follow defined rules:
  - `STOPPED → STARTING`: On power-on sequence
  - `STARTING → IDLE`: All systems initialized successfully
  - `IDLE ↔ MANUAL`: On first/last manual command (with configurable timeout)
  - `IDLE/MANUAL → ACTIVE`: When program starts
  - `ACTIVE → IDLE/MANUAL`: When program completes or is stopped
  - `any → SERVICE`: When service mode activated (requires authorization)
  - `any → SYSTEM_ERROR`: On critical system fault or `SafetyState::SAFETY_STOP`

- **FR-003**: Machine enters `MANUAL` when **first** axis gets manual command, exits when **all** axes stop manual operations (with timeout).

- **FR-004**: Manual axis motion MUST require explicit `AllowManualMode` command from Recipe Executor or any external program. Control Unit only supervises motion parameters and safety.

#### LEVEL 2: Safety State Management

- **FR-010**: System MUST support the following global safety states (`SafetyState`):
  - `SAFE`: All safety conditions met, normal operation
  - `SAFE_REDUCED_SPEED`: Hardware-enforced speed reduction (maintenance/service)
  - `SAFETY_STOP`: E-Stop, light curtains, safety doors - triggers per-axis safe stop categories

- **FR-011**: `SafetyState` MUST override `MachineState` behavior:
  - `SAFE`: Normal operation
  - `SAFE_REDUCED_SPEED`: Continue with velocity clamped per-axis to `CuAxisConfig.safe_reduced_speed_limit` (user units — mm/s for linear, rpm for rotary)
  - `SAFETY_STOP`: Execute per-axis safe stop according to configured category, then `MachineState::SYSTEM_ERROR`

- **FR-012**: Trigger conditions for safety states:
  | Trigger | SafetyState |
  |---------|-------------|
  | All safety OK | `SAFE` |
  | Maintenance key + guard open | `SAFE_REDUCED_SPEED` |
  | E-Stop, light curtain, safety door, critical fault | `SAFETY_STOP` |

#### LEVEL 3: Axis State Machines (Orthogonal)

- **FR-013**: Each axis MUST support configurable safe stop category (`SafeStopCategory` enum):
  - `STO` (Safe Torque Off): Immediate power cut, motor stops by inertia
    - Fastest reaction time
    - Motor generates no torque
    - Axis stops by mechanical friction and inertia
    - Used for light/fast axes or emergency situations
  - `SS1` (Safe Stop 1): Controlled deceleration, then STO
    - Controlled ramp-down with MaxDec deceleration
    - After stop: power cut (STO)
    - Best for heavy axes with braking resistors
    - **Default choice when not specified**
  - `SS2` (Safe Stop 2): Controlled deceleration, maintain position
    - Controlled ramp-down with MaxDec deceleration  
    - After stop: motor remains powered (SOS - Safe Operating Stop)
    - Actively maintains position with holding torque
    - Fastest return to operation (no drive restart needed)

- **FR-014**: Per-axis safe stop execution MUST follow category-specific logic:
  - `STO`: Immediate `MotionState::EMERGENCY_STOP`, disable drive, engage brake
  - `SS1`: `MotionState::EMERGENCY_STOP` with MaxDec, then disable drive after stop, engage brake
  - `SS2`: `MotionState::EMERGENCY_STOP` with MaxDec, keep drive enabled after stop, maintain position

- **FR-015**: Safe stop category configuration per axis:
  - `safe_stop_category`: SafeStopCategory (default: SS1)
  - `max_decel_safe`: Maximum safe deceleration for SS1/SS2 (higher than normal MaxDec)
  - `sto_brake_delay`: Delay between STO and brake engagement (mechanical settling)
  - `ss2_holding_torque`: Holding torque percentage for SS2 (default: 20%)

Each axis maintains **six orthogonal state machines** simultaneously:

##### A) PowerState

- **FR-020**: Each axis MUST maintain `PowerState`:
  - `POWER_OFF`: Drive disabled, brake engaged
  - `POWERING_ON`: Startup sequence in progress
  - `STANDBY`: Power ON, no motion, ready for commands
  - `MOTION`: Active, in motion
  - `POWERING_OFF`: Shutdown sequence in progress
  - `NO_BRAKE`: Service mode - drive OFF, brake released (manual positioning)
  - `POWER_ERROR`: Power-level error

- **FR-021**: `POWERING_ON` sequence MUST:
  1. Check motion_enable input (if configured)
  2. Verify tailstock closed (if type requires it)
  3. Retract locking pin, wait for confirmation (timeout!)
  4. Enable drive, wait for drive_ready
  5. Apply holding torque at current position
  6. Wait for position stability (holding_time for vertical axes)
  7. Release brake, wait for confirmation (timeout!)
  8. Overlap period: both drive and brake active
  9. Verify no position drop (gravity-affected axes)
  10. Transition to `STANDBY`

- **FR-022**: `POWERING_OFF` sequence MUST:
  1. Check position for locking pin insertion (zero window)
  2. Engage brake, wait for confirmation
  3. Verify position held
  4. Reduce drive torque gradually (200-500ms)
  5. Disable drive
  6. Extend locking pin (if applicable)
  7. Transition to `POWER_OFF`

##### B) MotionState

- **FR-030**: Each axis MUST maintain `MotionState`:
  - `STANDSTILL`: Stopped, position maintained
  - `ACCELERATING`: Accelerating to target velocity
  - `CONSTANT_VELOCITY`: Moving at constant velocity
  - `DECELERATING`: Decelerating to lower velocity or stop
  - `STOPPING`: Active controlled stopping
  - `EMERGENCY_STOP`: Maximum deceleration emergency stop
  - `HOMING`: Searching for reference/zero point
  - `GEAR_ASSIST_MOTION`: Micro-movements during gear shifting
  - `MOTION_ERROR`: Stopped due to error

- **FR-031**: `HOMING` state MUST have unique characteristics:
  - Unknown position boundaries (soft limits may not apply)
  - Very low speed (configurable `homing_speed`)
  - High lag sensitivity (early collision detection)
  - Reduced torque/power (configurable `homing_torque_limit`)
  - Success: reference point found → position set to zero → `STANDSTILL`
  - Failure: lag exceed or external stop → `MOTION_ERROR`

- **FR-032**: System MUST support six homing methods (`HomingMethod` enum):
  - `HARD_STOP`: Detect hard mechanical stop via current increase
    - Monitor drive current/torque feedback
    - When current exceeds threshold → stop movement
    - Current position becomes zero reference
    - No additional sensors required
  - `HOME_SENSOR`: Dedicated reference sensor (e.g., inductive)
    - Move until di_home_sensor triggers
    - When sensor activated → stop movement
    - Sensor position becomes zero reference
    - Requires home sensor input configuration
  - `LIMIT_SWITCH`: Use existing limit switch as reference
    - Move toward configured limit (low/high)
    - When limit switch triggers → stop movement
    - Limit position becomes zero reference
    - Reuses existing di_end_switch inputs
  - `INDEX_PULSE`: Encoder index pulse (most accurate)
    - Move until home sensor triggers (coarse positioning)
    - Continue slowly until encoder index pulse (fine positioning)
    - Index pulse position becomes zero reference
    - Requires both home sensor and encoder index
  - `ABSOLUTE`: Absolute encoder (no movement needed)
    - Read absolute position from encoder
    - Calculate offset to desired zero point
    - Apply offset to position calculation
    - No physical movement required
  - `NO_HOMING`: Current position as reference
    - Current position immediately becomes zero
    - Set Referenced = true without movement
    - Used for axes that don't require positioning

- **FR-033**: Each homing method MUST have specific parameters:
  - `HARD_STOP`: current_threshold, timeout, **approach_direction** (REQUIRED)
  - `HOME_SENSOR`: `sensor_role: IoRole` (e.g., `Ref1`), **approach_direction** (REQUIRED), sensor_nc_config
  - `LIMIT_SWITCH`: limit_direction (low/high), **approach_direction** (REQUIRED)
  - `INDEX_PULSE`: `sensor_role: IoRole` (e.g., `Ref1`), `index_role: IoRole`, **approach_direction** (REQUIRED), sensor_nc_config
  - `ABSOLUTE`: zero_offset_position
  - `NO_HOMING`: (no additional parameters)

- **FR-033a**: `approach_direction` is an enum `HomingDirection { Positive, Negative }` and MUST be explicitly configured for every movement-based homing method (HARD_STOP, HOME_SENSOR, LIMIT_SWITCH, INDEX_PULSE). No default value — missing `approach_direction` is a config validation error (`ConfigError::ValidationError`). **Safety rationale**: For rotary axes with wound material, homing in the wrong direction can cause material entanglement or mechanical damage. The integrator MUST choose the safe approach direction (e.g., unwinding direction) during commissioning.

- **FR-034**: Homing sequence MUST follow method-specific logic:
  1. Verify axis in `PowerState::STANDBY` or `PowerState::MOTION` AND `MotionState::STANDSTILL` (axis must be powered but stationary)
  2. Enter `MotionState::HOMING` 
  3. Execute method-specific search algorithm
  4. On success: set Referenced = true, position = 0.0, enter `STANDSTILL`
  5. On failure: set `MotionError::ERR_HOMING_FAILED`, enter `MOTION_ERROR`
  6. Timeout protection for all methods (configurable per method)

- **FR-035**: Unreferenced axis motion policy:
  - HAL reports per-axis `referenced` flag via `evo_hal_cu` segment
  - Unreferenced axes (`referenced = false`) MUST be limited to:
    - `OperationalMode::MANUAL` or `MachineState::SERVICE` only
    - Maximum **5% of configured max velocity** (`CuAxisConfig.max_velocity` — configured per-axis in CU, mirrors HAL's mechanical limit; not derived from HAL at runtime)
    - Software limits (`soft_limit_ok`) are disabled (position unknown)
    - Hardware limits, lag monitoring, and all other safety checks remain active
  - `MachineState::ACTIVE` (production) MUST reject commands for unreferenced axes with `MotionError::ERR_NOT_REFERENCED`
  - CU supervises homing execution; homing command originates from Recipe Executor (via `evo_re_cu`)
  - Absolute encoders: HAL sets `referenced = true` on startup (no motion needed)

##### C) OperationalMode

- **FR-040**: Each axis MUST maintain `OperationalMode`:
  - `POSITION`: Position control (PID to target position)
  - `VELOCITY`: Velocity control (speed setpoint)
  - `TORQUE`: Torque/force control (current setpoint)
  - `MANUAL`: Manual control (pedal, joystick)
  - `TEST`: Test/diagnostic mode

- **FR-041**: When axis is `SLAVE_COUPLED` or `SLAVE_MODULATED`, the `OperationalMode` is **overridden** by master control - slave's `OperationalMode` setting is ignored.

- **FR-042**: `OperationalMode` transitions MUST be **blocked** when axis is in slave coupling state. Slave must be decoupled first.

##### D) CouplingState

- **FR-050**: Each axis MUST maintain `CouplingState`:
  - `UNCOUPLED`: Independent axis
  - `MASTER`: Master for other axes
  - `SLAVE_COUPLED`: Coupled with fixed ratio to master
  - `SLAVE_MODULATED`: Coupled with position modulation (e.g., wire winding)
  - `WAITING_SYNC`: Waiting for synchronization
  - `SYNCHRONIZED`: Synchronized, moving in group
  - `SYNC_LOST`: Lost synchronization
  - `COUPLING`: In process of coupling
  - `DECOUPLING`: In process of decoupling

- **FR-051**: Slave position calculation:
  - `SLAVE_COUPLED`: `target_pos = master_pos × coupling_ratio`
  - `SLAVE_MODULATED`: `target_pos = master_pos × coupling_ratio + modulation_offset`

- **FR-052**: Synchronization MUST be bottom-up:
  - Each slave knows its immediate master only
  - Each master knows its direct slaves only
  - Deepest slaves ready first, then cascades upward
  - All axes in chain transition to `SYNCHRONIZED` in same cycle

- **FR-053**: Error propagation in coupling chains:
  - Critical errors propagate **upward** with source information
  - Slave error: master gets `CouplingError::ERR_SLAVE_FAULT`
  - Master doesn't timeout if slave timed out (slave's error)

##### E) GearboxState

- **FR-060**: Each axis MUST maintain `GearboxState`:
  - `NO_GEARBOX`: Axis without physical gearbox (virtual parameters only)
  - `GEAR_1`, `GEAR_2`, ... `GEAR_N`: Physical gear engaged
  - `NEUTRAL`: No gear engaged (maintenance)
  - `SHIFTING`: Gear change in progress
  - `GEARBOX_ERROR`: Sensor conflict, timeout
  - `UNKNOWN`: Cannot determine gear (startup)

- **FR-061**: Physical gears require sensor confirmation with timeout. Virtual gears (parameter sets) switch instantly.

- **FR-062**: Gear shifting with assist motion:
  1. Transition `MotionState` to `GEAR_ASSIST_MOTION`
  2. Execute small oscillations
  3. Command gear change, wait for sensor
  4. Stop oscillation, return to `STANDSTILL`

##### F) LoadingState (Per Axis)

- **FR-070**: Each axis MUST maintain `LoadingState`:
  - `PRODUCTION`: Normal production operation
  - `READY_FOR_LOADING`: Axis safe, loading can begin
  - `LOADING_BLOCKED`: Axis blocked during loading (critical axis)
  - `LOADING_MANUAL_ALLOWED`: Manual positioning allowed during loading

- **FR-071**: Loading is NOT a global machine state but per-axis:
  - Different axes have different safety criticality
  - Critical axes MUST stop (e.g., spindle, cutting tool)
  - Non-critical axes CAN continue or allow manual positioning

- **FR-072**: Axis parameters for loading:
  - `loading_blocked`: bool - axis blocked during loading mode (critical axis)
  - `loading_manual`: bool - manual positioning allowed during loading mode

- **FR-073**: Global loading mode button triggers per-axis state transitions:
  - Axes with `loading_blocked=true` → `LoadingState::LOADING_BLOCKED`
  - Axes with `loading_manual=true` → `LoadingState::LOADING_MANUAL_ALLOWED`
  - Other axes → `LoadingState::PRODUCTION` (continue normally)

#### LEVEL 4: Axis Safety Flags

- **FR-080**: Each axis MUST maintain `AxisSafetyState` with boolean flags:
  - `tailstock_ok`: Tailstock closed/acceptable for motion
  - `lock_pin_ok`: Locking pin retracted/free
  - `brake_ok`: Brake released when needed
  - `guard_ok`: Guard closed/locked (if speed > secure_speed)
  - `limit_switch_ok`: No hard limits triggered
  - `soft_limit_ok`: Within software boundaries
  - `motion_enable_ok`: External enable signal active
  - `gearbox_ok`: Valid gear engaged

- **FR-081**: Motion MUST be blocked when any safety flag is false, with specific blocking reason reported.

##### Safety Peripheral Details

- **FR-082**: System MUST support **Tailstock** with types 0-4:
  - Type 0: No tailstock or manually managed — `tailstock_ok` always true
  - Type 1: Standard tailstock with sensors (closed, closed_nc, open) — motion requires `di_closed` active
  - Type 2: Sliding tailstock with clamp on guide rail — motion requires `di_closed` active AND `di_clamp_locked` active
  - Type 3: Like Type 1+2, can operate with open tailstock if clamp locked — motion requires (`di_closed` active) OR (`di_clamp_locked` active)
  - Type 4: Like Type 2, automatic clamp control — CU issues clamp engage command via DO, then waits for `di_clamp_locked` confirmation with timeout; motion requires both closed and clamped

- **FR-083**: System MUST support **Locking Pin** with sensors:
  - di_index_locked, di_index_middle (optional), di_index_free
  - Valid state for motion: !locked && !middle && free
  - Configurable retraction/insertion timeout

- **FR-084**: System MUST support **Axis Brake** with:
  - Output: do_brake (TRUE = release, FALSE = engage, unless inverted)
  - Input: di_brake_released (confirmation)
  - Parameter: brake_always_free (some axes don't need holding)

- **FR-085**: System MUST support **Safety Guard** with:
  - Inputs: di_guard_closed, di_guard_locked
  - Parameter: secure_speed
  - If speed > secure_speed → guard MUST be closed AND locked
  - Guard can open only when speed < secure_speed for at least 2 seconds

- **FR-086**: All safety-relevant inputs MUST be configurable as NC or NO:
  - NC: FALSE = triggered/active
  - NO: TRUE = triggered/active
  - Applies to: limit switches, brake, tailstock, locking pin, guards, fences

#### LEVEL 5: Error Flags (Hierarchical)

- **FR-090**: Each axis MUST maintain hierarchical error state:
  - `PowerError`: brake_timeout, lock_pin_timeout, drive_fault, drive_not_ready, motion_enable_lost, drive_tail_open, drive_lock_pin_locked, drive_brake_locked
  - `MotionError`: lag_exceed, lag_critical (dispatched via `LagPolicy` — see data-model.md), hard_limit, soft_limit, overspeed, acceleration_limit, homing_failed, collision_detected, encoder_fault, drive_zerospeed, cycle_overrun, not_referenced
  - `CommandError`: source_locked (axis owned by another source), source_not_authorized (insufficient priority), source_timeout (writer heartbeat stale per FR-130c)
  - `GearboxError`: gear_timeout, gear_sensor_conflict, no_gearstep, gear_change_denied
  - `CouplingError`: sync_timeout, slave_fault, master_lost, lag_diff_exceed

- **FR-091**: Error propagation rules:
  - Non-critical errors: affect only faulting axis, allow reduced operation
  - Critical errors: trigger `SafetyState::SAFETY_STOP` for all axes

- **FR-092**: Critical errors that trigger global emergency stop:
  - `MotionError::ERR_LAG_CRITICAL` (if axis.lag_policy == Critical)
  - `CouplingError::ERR_LAG_DIFF_EXCEED`
  - `PowerError::ERR_DRIVE_TAIL_OPEN`, `ERR_DRIVE_LOCK_PIN_LOCKED`, `ERR_DRIVE_BRAKE_LOCKED`
  - `MotionError::ERR_W_DRIVE_ZEROSPEED`
  - `GearboxError::ERR_NO_GEARSTEP`
  - `MotionError::ERR_CYCLE_OVERRUN`

#### Universal Position Control Engine

- **FR-100**: System MUST implement universal motion controller with modular components activated by gain values:

  **Feedback Control Loop (PID)**:
  - `Kp` (Proportional Gain): Position error gain - system stiffness
  - `Ki` (Integral Gain): Error accumulation over time - eliminates steady-state error
  - `Kd` (Derivative Gain): Error rate response - damping and overshoot prevention
  - `Tf` (Derivative Filter): Low-pass filter time constant for Kd - prevents encoder noise amplification
  - `Tt` (Anti-Windup Tracking): Back-calculation time constant - prevents I-term windup during saturation

  **Feedforward Control**:
  - `Kvff` (Velocity Feedforward): Current proportional to commanded velocity - overcomes viscous friction
  - `Kaff` (Acceleration Feedforward): Current proportional to commanded acceleration - compensates inertia
  - `Friction` (Static Friction Offset): Constant current offset during motion - overcomes stiction

  **Disturbance Observer (DOB)**:
  - `Jn` (Nominal Inertia): Expected system mass/inertia for disturbance estimation
  - `Bn` (Nominal Damping): Expected system friction for disturbance estimation  
  - `gDOB` (Observer Bandwidth): Observer response speed - disturbance correction aggressiveness

  **Signal Conditioning**:
  - `fNotch` (Notch Frequency): Resonance frequency to eliminate from control signal
  - `BWnotch` (Notch Bandwidth): Width of notch filter frequency cut
  - `flp` (Low-Pass Filter): General output signal smoothing frequency
  - `OutMax` (Output Limit): Maximum control signal saturation for hardware protection

- **FR-101**: Component activation MUST be controlled by gain values:
  - Set gain = 0.0 to completely disable any component
  - No predefined presets or combinations - full programmer flexibility
  - All components can operate independently or in any combination
  - System automatically handles zero-gain components in calculations

- **FR-102**: Control calculation MUST follow modular approach:
  ```
  // Feedback (PID)
  error = target_pos - actual_pos
  pid_output = Kp * error + Ki * integral_term + Kd * derivative_term
  
  // Feedforward
  ff_output = Kvff * target_velocity + Kaff * target_acceleration + Friction * sign(target_velocity)
  
  // Disturbance Observer (if gDOB > 0)
  disturbance_estimate = DOB_algorithm(Jn, Bn, gDOB, actual_response)
  
  // Signal Processing
  raw_output = pid_output + ff_output + disturbance_estimate
  filtered_output = apply_notch_filter(raw_output, fNotch, BWnotch)
  final_output = apply_lowpass_filter(filtered_output, flp)
  control_output = clamp(final_output, -OutMax, +OutMax)
  ```

- **FR-103**: System MUST monitor lag error for safety:
  - Condition: |target_position - actual_position| > lag_error_limit
  - Reactions based on axis configuration and coupling status
  - Lag monitoring operates independently of control algorithm selection

- **FR-104**: For coupled axes, system MUST monitor master-slave lag difference:
  - Condition: |master_lag - slave_lag| > max_lag_diff
  - Reaction: `CouplingError::ERR_LAG_DIFF_EXCEED`, all coupled axes emergency stop
  - Lag difference monitoring operates regardless of individual axis control configuration

- **FR-105**: Control Unit MUST produce a complete `ControlOutputVector` per axis every cycle, regardless of drive mode:
  ```
  struct ControlOutputVector {
      CalculatedTorque: f64,  // [Nm] Full control output (PID + FF + DOB)
      TargetVelocity:   f64,  // [mm/s] or [rpm] Commanded velocity
      TargetPosition:   f64,  // [mm] or [rev] Commanded position
      TorqueOffset:     f64,  // [Nm] Feedforward component (Kaff + DOB only)
  }
  ```
  - All four fields are always calculated, never conditionally skipped
  - HAL decides which fields to use based on drive configuration
  - Control Unit does NOT know or care about drive communication mode

- **FR-106**: `ControlOutputVector` field definitions:
  - `CalculatedTorque`: Sum of PID + Feedforward + DOB outputs, representing the total torque the control algorithm requests
  - `TargetVelocity`: Velocity setpoint from trajectory generator or manual command
  - `TargetPosition`: Position setpoint from trajectory generator or manual command
  - `TorqueOffset`: Only the feedforward components (`Kaff * acceleration + DOB disturbance estimate`), separated for drives that support torque feedforward injection alongside velocity/position control

#### Motion Range Monitoring

- **FR-110**: System MUST read hardware limit switches via IoRole with NC/NO configuration:
  - `hard_low_limit = io_registry.read_di(IoRole::LimitMin(N), &di_bank)` — NC/NO applied automatically by IoRegistry
  - `hard_high_limit = io_registry.read_di(IoRole::LimitMax(N), &di_bank)` — NC/NO applied automatically by IoRegistry
  - IoRole→pin mapping and NC/NO logic resolved at startup from `io.toml` (FR-148, FR-152)
  - Triggered limit → `MotionError::ERR_HARD_LIMIT`, block motion in that direction

- **FR-111**: System MUST enforce software limits with tolerance:
  - Soft limit violation is evaluated using the axis `in_position_window` parameter (from HAL config, shared via `evo_common`) as a tolerance band:
    - `soft_low_limit = (position < min_pos - in_position_window)`
    - `soft_high_limit = (position > max_pos + in_position_window)`
  - Axis is allowed to reach exactly `min_pos` / `max_pos`; minor oscillations or vibrations within `in_position_window` beyond the boundary MUST NOT trigger an error
  - Triggered limit (beyond tolerance) → `MotionError::ERR_SOFT_LIMIT`, block motion in that direction
  - Motion commands targeting positions beyond `min_pos` / `max_pos` MUST still be rejected (pre-move check uses exact limit, no tolerance)
  - `in_position_window` is already defined per axis in HAL config; CU reads it from `evo_common` shared types — no duplication

- **FR-112**: System MUST reduce speed when approaching limits to guarantee stopping before boundary (exact `min_pos` / `max_pos`, not the tolerance-extended boundary).

#### SAFETY_STOP (Emergency Stop) Conditions

- **FR-120**: System MUST trigger `SafetyState::SAFETY_STOP` for:
  | Condition | Error Code |
  |-----------|------------|
  | Drive active with open tailstock | `PowerError::ERR_DRIVE_TAIL_OPEN` |
  | Drive active with locked locking pin | `PowerError::ERR_DRIVE_LOCK_PIN_LOCKED` |
  | Drive active with engaged brake | `PowerError::ERR_DRIVE_BRAKE_LOCKED` |
  | Motion enable signal lost | `PowerError::ERR_MOTION_ENABLE_LOST` |
  | Drive reports motion at zero command | `MotionError::ERR_W_DRIVE_ZEROSPEED` |
  | Invalid gearbox position | `GearboxError::ERR_NO_GEARSTEP` |
  | Light curtain broken | External safety trigger |
  | E-Stop pressed | External safety trigger |
  | Safety door open | External safety trigger |

- **FR-121**: SAFETY_STOP reaction sequence (per-axis based on SafeStopCategory):
  1. `SafetyState` → `SAFETY_STOP`
  2. For each axis, execute safe stop according to configured category:
     - `STO`: Immediate drive disable → `MotionState::EMERGENCY_STOP` → engage brake
     - `SS1`: `MotionState::EMERGENCY_STOP` with MaxDec → drive disable after stop → engage brake  
     - `SS2`: `MotionState::EMERGENCY_STOP` with MaxDec → maintain holding torque → brake remains released
  3. `MachineState` → `SYSTEM_ERROR`
  4. Report error via `evo_cu_mqt` with timestamp, cause, and per-axis stop category executed (no file I/O in RT cycle — FR-134)

- **FR-122**: Recovery from `SAFETY_STOP` MUST require:
  - Explicit reset button press
  - All safety conditions cleared (all `AxisSafetyState` flags true)
  - Manual authorization for motion (additional confirmation via screen/start button)
  - `SafetyState` → `SAFE`, then `MachineState` transitions `SYSTEM_ERROR` → `IDLE` (normal recovery path). All axes remain in `PowerState::POWER_OFF` until explicitly enabled.
  - For unrecoverable faults (SHM version mismatch, config corruption): `MachineState` transitions `SYSTEM_ERROR` → `STOPPED`, requiring full CU restart.

#### P2P Shared Memory Integration

- **FR-130**: Control Unit MUST communicate via **P2P (Point-to-Point) SHM segments** following naming convention `evo_[SOURCE]_[DESTINATION]`:
  - `evo_hal_cu`: HAL → Control Unit (sensor feedback, drive status)
  - `evo_cu_hal`: Control Unit → HAL (setpoints, drive commands)
  - `evo_re_cu`: Recipe Executor → Control Unit (motion requests, program commands)
  - `evo_rpc_cu`: gRPC API → Control Unit (external commands, e.g., GUI)
  - `evo_cu_mqt`: Control Unit → MQTT (telemetry, trace data)
  - `evo_cu_re`: Control Unit → Recipe Executor (reserved — placeholder, content TBD)

- **FR-130a**: Each P2P segment has exactly **one writer and one reader**:
  - Writer creates the segment (`shm_open` + `ftruncate`)
  - Reader connects (`mmap`) only to segments where its module abbreviation is in `[DESTINATION]` position
  - Attempting to read a segment not addressed to the module MUST be rejected by evo_shared_memory with a configuration error

- **FR-130b**: Module abbreviation registry:
  - `cu` = evo_control_unit
  - `hal` = evo_hal
  - `re` = evo_recipe_executor
  - `mqt` = evo_mqtt
  - `rpc` = evo_grpc

- **FR-130c**: Every P2P segment MUST include a **monotonic heartbeat counter** in its header:
  - Writer increments counter on every write cycle
  - Reader checks counter on every read cycle; if counter unchanged for `N` consecutive reads → segment is **stale**
  - Stale `evo_hal_cu` → `ERR_HAL_COMMUNICATION`, immediate `SAFETY_STOP`
  - Stale `evo_re_cu` / `evo_rpc_cu` → `ERR_SOURCE_TIMEOUT`, release source lock, pause commands (non-safety — no SAFETY_STOP)
  - `N` (staleness threshold) is configurable per segment (default: 3 cycles)
  - `evo_watchdog` serves as secondary backstop for process-level crash detection

- **FR-130d**: Every P2P segment MUST include a **struct version hash** in its header:
  - Writer stores a compile-time hash of the segment struct layout at creation time
  - Reader validates hash at connect time (`mmap`); mismatch → `ERR_SHM_VERSION_MISMATCH`, connection refused
  - Prevents silent data corruption when writer and reader binaries are built against different struct definitions
  - Hash is a `const fn` computed from `core::mem::size_of::<T>()` and `core::mem::align_of::<T>()` (canonical algorithm defined in contracts/shm-segments.md §Version Hash Contract). Detects size/alignment changes; field reordering in `#[repr(C)]` structs caught indirectly via padding changes (see contracts §Version Hash — Canonical Algorithm for known limitations)

#### P2P Library API & Transport

- **FR-130e**: evo_shared_memory MUST expose a generic P2P transport API:
  - `SegmentWriter::<T>::create(name: &str, source: ModuleAbbrev, dest: ModuleAbbrev) -> Result<SegmentWriter<T>, ShmError>` — creates segment via `shm_open(O_CREAT | O_RDWR)` + `ftruncate(header_size + size_of::<T>())`; initializes header (magic, version_hash, heartbeat=0, write_seq=0, source/dest module); acquires exclusive `flock(LOCK_EX | LOCK_NB)`
  - `SegmentReader::<T>::attach(name: &str, my_module: ModuleAbbrev) -> Result<SegmentReader<T>, ShmError>` — opens via `shm_open(O_RDONLY)` + `mmap`; validates in order: (1) magic == `b"EVO_P2P\0"`, (2) dest_module == my_module, (3) version_hash == `struct_version_hash::<T>()`, (4) acquires shared `flock(LOCK_SH | LOCK_NB)` for single-reader enforcement
  - `SegmentWriter::write(&self, data: &T)` — lock-free write: set write_seq odd (Release), copy payload, increment heartbeat, set write_seq even (Release)
  - `SegmentReader::read(&self) -> Result<T, ShmError>` — lock-free read per FR-130g
  - Library auto-computes segment size from `size_of::<P2pSegmentHeader>() + size_of::<T>()`; caller does not provide raw byte size

- **FR-130f**: Single-writer/single-reader enforcement MUST use POSIX advisory file locks (`flock`):
  - Writer: `flock(fd, LOCK_EX | LOCK_NB)` at creation — failure returns `ShmError::WriterAlreadyExists`
  - Reader: `flock(fd, LOCK_SH | LOCK_NB)` at attach — second reader returns `ShmError::ReaderAlreadyConnected`
  - Locks automatically released on process exit (including crashes); kernel reclaims advisory locks
  - Combined semantics: exactly one LOCK_EX (writer) + at most one LOCK_SH (reader) per segment

- **FR-130g**: Lock-free read protocol MUST follow bounded retry for RT determinism:
  - Algorithm: (1) load write_seq (Acquire), (2) if odd → retry, (3) copy payload bytes, (4) reload write_seq (Acquire), (5) if changed → retry from step 1
  - Maximum 3 retries; if exhausted → return `ShmError::ReadContention`
  - Memory ordering: writer stores write_seq with `Release`; reader loads with `Acquire`
  - `write_seq` stored as `AtomicU32`; initial value 0 (even = committed). `u32` range (4B writes ≈ 49 days at 1ms) is safe because protocol checks odd/even and changed/unchanged, not magnitude — wrapping preserves semantics

- **FR-130h**: evo_shared_memory `ShmError` MUST include P2P-specific variants:
  - `InvalidMagic` — segment magic ≠ `b"EVO_P2P\0"`
  - `VersionMismatch { expected: u32, found: u32 }` — struct hash mismatch at connect time
  - `DestinationMismatch { expected: ModuleAbbrev, found: ModuleAbbrev }` — reader module ≠ segment dest_module (connect-time rejection)
  - `WriterAlreadyExists` — exclusive flock failed
  - `ReaderAlreadyConnected` — shared flock limit reached
  - `ReadContention` — lock-free retry limit (3) exceeded
  - `SegmentNotFound` — shm_open ENOENT
  - `PermissionDenied` — shm_open EACCES

- **FR-130i**: P2P segment discovery:
  - `SegmentDiscovery::list_segments() -> Vec<SegmentInfo>` enumerates `/dev/shm/evo_*`, parses `evo_[SRC]_[DST]` to extract `ModuleAbbrev` pairs
  - `SegmentDiscovery::list_for(module: ModuleAbbrev) -> Vec<SegmentInfo>` returns segments addressed to the given module
  - `SegmentInfo { name: String, source_module: ModuleAbbrev, dest_module: ModuleAbbrev, size_bytes: usize, writer_alive: bool }` — `writer_alive` probed via non-blocking `flock(LOCK_EX)` test (fails if writer holds lock)

- **FR-130j**: P2P segment lifecycle management:
  - **Lifecycle states**: NonExistent → Created (writer init) → Connected (reader attached) → Stale (heartbeat frozen) → Cleaned (unlinked)
  - **Writer cleanup**: `SegmentWriter::drop()` calls `shm_unlink` to remove `/dev/shm` entry; reader retains mmap until detach (POSIX: unlinked file accessible via existing fd)
  - **Reader cleanup**: `SegmentReader::drop()` calls `munmap` + releases flock
  - **Writer crash**: Orphan segment persists with stale heartbeat; reader detects via FR-130c; evo_watchdog calls `shm_unlink` for orphans (A-008)
  - **Dual crash (writer + reader)**: evo_watchdog detects both deaths, calls `shm_unlink`; on restart, `SegmentWriter::create()` uses `O_CREAT` (without `O_EXCL`) to overwrite stale segments
  - **Writer restart**: New writer creates segment with same name (`O_CREAT`). Old reader's mmap references the unlinked file — detects heartbeat freeze, detaches, re-attaches to new segment on next cycle

- **FR-130k**: P2P segment naming and POSIX conventions:
  - Filesystem path: `/dev/shm/evo_[SOURCE]_[DESTINATION]` (e.g., `/dev/shm/evo_hal_cu`)
  - Fixed names without PID suffix — deterministic across restarts (PID suffix from broadcast era is eliminated)
  - Uniqueness guaranteed by closed `ModuleAbbrev` registry (FR-130b): each ordered source-dest pair produces a unique name
  - Permissions: `0600` (owner read/write); all evo modules run under same system user
  - `shm_open` flags: writer `O_CREAT | O_RDWR` (overwrites stale segments from previous runs); reader `O_RDONLY`

- **FR-130l**: P2P segment size constraints:
  - Minimum: `size_of::<P2pSegmentHeader>()` = 64 bytes (no 4KB minimum — broadcast-era `SHM_MIN_SIZE` does not apply to P2P)
  - Maximum: 1 MB practical bound enforced by library (all current segments < 8 KB)
  - `ftruncate` size computed by library as header + payload size, page-aligned by kernel

- **FR-130m**: P2P transport types MUST be `Send` but not `Sync`:
  - `SegmentWriter<T>: Send` — movable to dedicated thread, not shareable across threads
  - `SegmentReader<T>: Send` — same semantics
  - Neither is `Sync` — mmap'd pointer requires external synchronization for cross-thread sharing
  - Read API returns `T` by value (byte-copy from mmap'd region); zero-copy `read_ref()` via guard type is a future optimization

- **FR-130n**: evo_shared_memory MUST emit `tracing` events for P2P lifecycle (non-RT only):
  - `info!` on segment create, attach, detach (startup/shutdown path only)
  - `warn!` on version mismatch, destination mismatch, reader-already-connected (connect-time failures)
  - `error!` on `ReadContention` (indicates abnormally long write)
  - **No logging on RT read/write hot path** — heartbeat/staleness monitoring is consumer-level (CU reports via evo_cu_mqt). Constitution Principle XIX: CU is pure RT, no logging in cycle loop

- **FR-130o**: `evo_common::shm::consts` MUST be updated for P2P:
  - Add `pub const P2P_SHM_MAGIC: [u8; 8] = *b"EVO_P2P\0"`
  - Retain `EVO_SHM_MAGIC: u64` with `#[deprecated(note = "Use P2P_SHM_MAGIC")]` during migration
  - Add `pub const P2P_SHM_MAX_SIZE: usize = 1_048_576` (1 MB)
  - `SHM_MIN_SIZE` (4096) retained for broadcast compatibility only; P2P has no minimum beyond header size
  - `P2pSegmentHeader`, `ModuleAbbrev` in `evo_common::shm::p2p` (new module, per FR-140)

- **FR-130p**: Broadcast-to-P2P migration in evo_shared_memory requires:
  - **Remove**: `segment.rs::SegmentHeader` (128-byte broadcast header) — replaced by `P2pSegmentHeader`
  - **Remove**: `reader_count` tracking, multi-reader scaling logic
  - **Remove**: `monitoring.rs` broadcast metrics (`MemoryMonitor`, `AlertHandler`, reader-scaling telemetry)
  - **Rework**: `version.rs::VersionCounter` → `write_seq: AtomicU32` even/odd protocol
  - **Rework**: `error.rs::ShmError` → add P2P variants (FR-130h), remove broadcast-specific variants
  - **Rework**: `data/` module → payload types migrate to evo_common (FR-140); data/ becomes empty or removed
  - **Add**: `p2p.rs` → `SegmentWriter<T>`, `SegmentReader<T>`, `SegmentDiscovery`, `SegmentInfo`
  - Broadcast multi-reader tests replaced with P2P single-reader tests

- **FR-131**: `evo_hal_cu` segment MUST contain: heartbeat counter, actual axis positions/velocities, drive readiness/fault flags, current DI/AI values, per-axis `referenced` flag.

- **FR-132**: `evo_cu_hal` segment MUST contain: per-axis `ControlOutputVector`, enable commands, desired DO/AO values.

- **FR-132a**: HAL MUST select fields from `ControlOutputVector` based on drive communication mode:
  - **Drive in Torque mode**: Use `CalculatedTorque`, convert Nm → % rated current using motor parameters known only to HAL. `TargetVelocity` and `TargetPosition` may be sent as safety limits only.
  - **Drive in Velocity mode**: Use `TargetVelocity`, convert mm/s → RPM. Optionally use `TorqueOffset` as torque feedforward injection if drive supports it.
  - **Drive in Position mode**: Use `TargetPosition` only, convert mm → encoder counts. All other fields ignored (drive handles control internally).
  - Unit conversion and scaling is HAL's responsibility — Control Unit operates in user units (mm, mm/s, Nm)

- **FR-132b**: `evo_re_cu` segment MUST contain: motion requests (axis, target, mode), program lifecycle commands (start, stop, pause), `AllowManualMode` commands, homing requests.

- **FR-132c**: `evo_rpc_cu` segment MUST contain: single-axis external commands (manual jog, service mode activation, parameter changes, config reload, `AllowManualMode`). Uses `RpcCommand` struct with scalar parameters — different layout from multi-axis `ReCommand` in `evo_re_cu` (see contracts/shm-segments.md §5 for layout). `AllowManualMode` is available via both `evo_re_cu` (FR-132b) and `evo_rpc_cu`, enabling manual axis jogging from dashboard/gRPC without requiring Recipe Executor.

- **FR-133**: Control Unit cycle MUST follow:
  1. Read all inbound P2P segments (`evo_hal_cu`, `evo_re_cu`, `evo_rpc_cu`)
  2. Process: command arbitration, axis control, safety monitoring, state machines
  3. Write all outbound P2P segments (`evo_cu_hal`, `evo_cu_mqt`, `evo_cu_re`)

- **FR-134**: `evo_cu_mqt` segment MUST contain **one complete structure** for machine state and all axes, updated every cycle with:
  - Heartbeat counter (monotonic, per FR-130c)
  - Global: `MachineState`, `SafetyState`
  - Per-axis: All 6 orthogonal state machines + safety flags + error states
  - **No event ring buffer** — segment is a live status snapshot only; event history and logging are out of scope for Control Unit
  - `evo_cu_mqt` is the **sole diagnostic output** of Control Unit — no file I/O in RT cycle; downstream consumers (evo_mqtt, evo_dashboard) handle persistence, event history, and forwarding

- **FR-134a**: `evo_cu_re` segment is **reserved for future use**:
  - Segment is created by Control Unit at startup (writer role)
  - Initial content: heartbeat counter + struct version hash + empty placeholder struct
  - Content definition (command acknowledgments, axis state summary, homing status) will be specified in a future iteration
  - Recipe Executor MUST NOT rely on `evo_cu_re` content until formally specified

#### Command Source Locking

- **FR-135**: Control Unit MUST implement **source locking** for axis control ownership:
  - When a source (e.g., Recipe Executor via `evo_re_cu`) takes control of an axis, that axis is **locked** to that source
  - Other sources attempting to command a locked axis MUST receive a rejection with:
    - Error code identifying the blocking source
    - Human-readable reason (e.g., "Axis 1 locked by Recipe Executor — program running")
  - Source releases lock explicitly (program ends, manual mode released) or on SAFETY_STOP

- **FR-136**: Safety signals MUST NOT cancel active commands but **pause** execution:
  - On `SAFETY_STOP`: all axes execute their SafeStopCategory, motion targets are preserved in memory
  - After recovery (reset + authorization): system can resume with pre-e-stop targets
  - Resume is invalid if conditions changed during stop (e.g., recipe was stopped, source released lock)
  - Source lock is NOT released by SAFETY_STOP — the owning source retains control after recovery

- **FR-137**: Safety has **unconditional override** priority — it can pause any source at any time, but does not interfere with command ownership or target memory.

#### Cycle Timing Enforcement

- **FR-138**: Control Unit cycle MUST complete within the configured cycle time (target: <1ms):
  - A single cycle overrun MUST trigger `SAFETY_STOP` (hard real-time deadline)
  - Cycle time is measured from start of read phase to end of write phase
  - Overrun error: `MotionError::ERR_CYCLE_OVERRUN`

#### RT Memory & Allocation Policy

- **FR-138a**: Control Unit RT cycle MUST comply with project constitution memory policies (Principles XIII, XIV, XXIV):
  - All buffers, state arrays, SHM mappings, and axis data structures pre-allocated during `MachineState::STARTING`
  - Zero dynamic allocation (`Vec::push`, `String::from`, `Box::new`, etc.) in RT cycle loop
  - RT process memory locked via `mlock` / `hugetlbfs` to prevent page faults
  - Cache-aligned data layouts for axis arrays and SHM structures
  - Constitution reference: `.specify/memory/constitution.md`

#### Hot-Reload Configuration (Constitution XIII)

- **FR-144**: System MUST support safe hot-reload of configuration parameters **without full application restart** (Constitution Principle XIII).

- **FR-145**: Hot-reconfiguration MUST be permitted **only** when `SafetyState == SAFETY_STOP` (E-STOP active):
  - RT loop is naturally halted (no timing-critical operations to violate)
  - Reload triggered by explicit `RELOAD_CONFIG` command via `evo_rpc_cu` (primary mechanism). Config file timestamp polling is safe during SAFETY_STOP since the RT loop is suspended and file I/O does not violate RT constraints.
  - If `SafetyState != SAFETY_STOP` when reload is requested → reject with `CommandError::ERR_RELOAD_DENIED` and reason "Reload requires E-STOP state"
  - **Reloadable scope**: PID gains, lag_error_limit, lag_policy, safe_stop timings, peripheral timeouts, homing parameters, feedforward/DOB/filter gains, guard secure_speed. **NOT reloadable**: axis count, axis_id assignments, coupling topology (master/slave relationships), SHM segment configuration. Topology changes require full CU restart.

- **FR-146**: Hot-reload execution MUST be atomic with rollback:
  1. Parse new config file into temporary `shadow_config` structure (outside RT loop, on non-RT thread or during E-STOP idle)
  2. Full validation of `shadow_config` (same rules as startup: axis ID uniqueness, coupling graph acyclicity, parameter bounds)
  3. If validation passes → atomic pointer swap: `active_config ← shadow_config` (single atomic operation)
  4. If validation fails → `shadow_config` discarded, existing `active_config` unchanged, `CommandError::ERR_RELOAD_VALIDATION_FAILED` reported via `evo_cu_mqt` with details
  5. After successful swap, system remains in `SAFETY_STOP` with new parameters; next E-STOP recovery uses updated config

- **FR-147**: Hot-reload performance constraints:
  - Maximum reload duration: ≤ 120 ms (worst-case, including parse + validate + swap)
  - Zero allocation in RT cycle — all reload work happens during E-STOP when RT loop is suspended
  - No race conditions: reload is sequenced with the halted RT loop (E-STOP guarantees no concurrent cycle execution)
  - On successful reload, updated config values are reflected in the next `evo_cu_mqt` status snapshot

#### Role-Based I/O Configuration (io.toml)

- **FR-148**: All discrete and analog I/O points MUST be defined in a single `io.toml` file, separate from axis and machine configuration. This file is the **single source of truth** for pin assignments, NC/NO logic, scaling, debounce, and functional roles. Both HAL and CU read the same `io.toml`.

- **FR-149**: Each I/O point in `io.toml` MAY carry a `role` field — a string that maps to the `IoRole` enum defined in `evo_common`. The `IoRole` enum is **one flat list** covering all components (HAL, CU, RE, diagnostics). Convention: `FunctionAxisNumber` (e.g., `LimitMin1`, `BrakeOut3`, `Ref2`, `EStop`). Roles without a numeric suffix are global (e.g., `EStop`, `PressureOk`, `SafetyGate`).

- **FR-150**: At startup, each program (HAL, CU) MUST build an `IoRegistry` by:
  1. Parsing `io.toml` into `Vec<IoGroup>` (groups like `[Safety]`, `[Axes]`, `[Pneumatics]`)
  2. For each I/O point with a `role`, inserting into a `HashMap<IoRole, IoBinding>` that maps role → (pin, type, logic, scaling, group)
  3. Validating **role completeness**: for each axis in machine config, all roles required by that axis's peripherals (tailstock, brake, index, guard, homing sensor, limit switches) MUST be present. Missing role → `ERR_IO_ROLE_MISSING`, startup refused.
  4. Validating **type correctness**: role expected as DI must map to a `type="di"` point, role expected as DO must map to `type="do"`, etc. Mismatch → `ERR_IO_ROLE_TYPE_MISMATCH`.
  5. Validating **uniqueness**: no two I/O points may share the same `role`. Duplicate → `ERR_IO_ROLE_DUPLICATE`.

- **FR-151**: The `IoRole` enum MUST include at minimum the following roles (extensible per project):
  - **Safety**: `EStop`, `SafetyGate`, `EStopReset`
  - **Control**: `Start`, `Stop`, `Reset`, `Pause`
  - **Per-axis DI**: `LimitMinN`, `LimitMaxN`, `RefN`, `EnableN` (N = axis number)
  - **Per-axis peripherals**: `TailClosedN`, `TailOpenN`, `TailClampN`, `IndexLockedN`, `IndexMiddleN`, `IndexFreeN`, `BrakeInN` (confirmation), `BrakeOutN` (command), `GuardClosedN`, `GuardLockedN`
  - **Pneumatics/general**: `PressureOk`, `VacuumOk`
  - **Custom**: Project-specific roles added via `IoRole::Custom(heapless::String<32>)`

- **FR-152**: Runtime I/O access in CU and HAL MUST use `IoRegistry` role-based lookup:
  - `io_registry.read_di(IoRole::LimitMin1) -> bool` (applies NC/NO logic automatically)
  - `io_registry.read_ai(IoRole::PressureOk) -> f64` (applies scaling curve + offset)
  - `io_registry.write_do(IoRole::BrakeOut1, true)` (applies inversion)
  - `io_registry.write_ao(IoRole::ValveCmd1, 50.0)` (applies scaling)
  - Direct pin-number access is **forbidden** in application code — all I/O through roles.

- **FR-153**: `io.toml` I/O point types and parameters:
  - **Digital Input (di)**: pin, role, name, logic (NO/NC, default NO), debounce (ms, default 15), sim (bool), enable_pin/enable_state/enable_timeout (conditional enable)
  - **Digital Output (do)**: pin, role, name, init (default false), inverted (default false), pulse (watchdog ms, 0=none), keep_estop (default false)
  - **Analog Input (ai)**: pin, role, name, min, max (REQUIRED), unit (default "V"), average (1-1000, default 5), curve (preset or [a,b,c] polynomial), offset, sim (float)
  - **Analog Output (ao)**: pin, role, name, min, max (REQUIRED), unit (default "V"), init, pulse (watchdog ms), curve, offset

- **FR-154**: I/O points are organized into named groups in `io.toml` (e.g., `[Safety]`, `[Axes]`, `[Pneumatics]`, `[Operator_Panel]`, `[Diagnostics]`). Groups are for human organization only — the runtime `IoRegistry` indexes by role, not by group. Group `name` is a display label for operator interfaces.

- **FR-155**: This `io.toml` role-based approach **replaces** the previous inline I/O definition pattern (`[[digital_inputs]]`, `[[analog_outputs]]`, etc.) in both HAL `machine.toml` and CU configuration. Old-style inline I/O arrays MUST NOT be used. Axis configs that previously referenced I/O by name string or array index (e.g., `reference_switch = 0`, `sensor_input_name = "home_1"`) MUST instead reference by `IoRole` enum variant.

#### CU Startup & Segment Availability

- **FR-139**: Control Unit startup MUST follow tiered segment availability:
  - **Mandatory**: `evo_hal_cu` — CU MUST NOT enter `MachineState::IDLE` until this segment is connected, heartbeat validated, and struct version verified (FR-130d)
  - **Optional**: `evo_re_cu`, `evo_rpc_cu` — CU starts without these; missing source = no commands from that source, not an error
  - When an optional segment appears (writer creates it), CU detects and connects on next cycle
  - When an optional segment becomes stale (FR-130c), CU releases associated source locks and continues operating

#### evo_common Integration

- **FR-140**: All shared structures MUST be defined in evo_common:
  - `MachineState`, `SafetyState` enums
  - `SafeStopCategory` enum for per-axis safe stop behavior
  - Axis state enums: `PowerState`, `MotionState`, `OperationalMode`, `CouplingState`, `GearboxState`, `LoadingState`
  - Error enums: `PowerError`, `MotionError`, `GearboxError`, `CouplingError`
  - `HomingMethod` enum, `HomingDirection` enum, and method-specific parameter structures
  - `ControlOutputVector` struct for per-axis cycle output
  - Safety peripheral types (TailstockType, etc.)
  - Control state structures for SHM
  - `IoRole` enum and I/O configuration types (`IoPoint`, `IoGroup`, `IoConfig`) — shared between HAL and CU (FR-149)

- **FR-141**: Configuration structures for Control Unit MUST be defined in evo_common::control_unit::config.

- **FR-142**: Configuration file structure MUST follow:
  - **Main file**: Global Control Unit parameters and machine references
  - **Per-machine file**: Machine-specific axis lists and safety configurations. Parameters are grouped into logical TOML sections by function (e.g., `[homing]`, `[safe_stop]`, `[control]`), regardless of which program consumes them — HAL, CU, or both.
  - **I/O file**: `io.toml` — all discrete and analog I/O points with pin assignments, roles, NC/NO logic, scaling (FR-148). Shared between HAL and CU.
  - **Helper files**: Reusable parameter sets and templates (to be created)
  - CU MUST NOT duplicate parameters already defined by HAL. Axis parameters (kinematics, limits, driver config) live in HAL config and are consumed by CU via evo_common shared types only.

- **FR-156**: All configuration parameters MUST have **min/max bounds** defined as `const` in `evo_common`:
  - Each parameter struct or module declares `const MIN_<PARAM>` and `const MAX_<PARAM>` for every numeric parameter
  - Config validation at load time checks all values against their bounds; out-of-range → `ConfigError::ValidationError`
  - Example: `const MIN_CYCLE_TIME_US: u32 = 100; const MAX_CYCLE_TIME_US: u32 = 10_000;`

- **FR-157**: Configuration loading MUST be **forward-compatible**:
  - All struct fields use `#[serde(default)]` where a sensible default exists — an older config file missing new fields loads successfully with defaults
  - Unknown/extra fields in TOML are silently ignored (serde default behavior) — a newer config file with fields not yet known to an older binary loads without error
  - Missing mandatory fields (no default) produce a clear `ConfigError::ParseError` at load time, not a runtime panic
  - This enables rolling updates where config files and binaries may be at different versions

#### Axis Identification

- **FR-143**: Axes MUST be identified by **1-based numeric index** (1..N):
  - SHM per-axis arrays use fixed-size layout indexed by `axis_id - 1` (0-based in memory, 1-based in user/config space)
  - Configuration maps `axis_id` ↔ human-readable name (e.g., `1 = "Spindle"`, `2 = "X-Axis"`)
  - All error messages, source lock reports, and coupling references use 1-based axis ID
  - Maximum axis count defined in configuration (up to 64 per SC-001)

### Key Entities

- **MachineState**: Global system state (STOPPED, STARTING, IDLE, MANUAL, ACTIVE, SERVICE, SYSTEM_ERROR)
- **SafetyState**: Global safety state (SAFE, SAFE_REDUCED_SPEED, SAFETY_STOP)
- **SafeStopCategory**: Per-axis safe stop category (STO, SS1, SS2)
- **AxisState**: Complete per-axis state containing all orthogonal state machines
- **PowerState**: Per-axis power/brake management state
- **MotionState**: Per-axis movement state
- **OperationalMode**: Per-axis control mode (POSITION, VELOCITY, TORQUE, MANUAL, TEST)
- **CouplingState**: Per-axis master-slave coordination state
- **GearboxState**: Per-axis gear management state
- **LoadingState**: Per-axis loading mode state
- **AxisSafetyState**: Per-axis safety flags (boolean conditions)
- **AxisErrorState**: Per-axis hierarchical error state (PowerError, MotionError, GearboxError, CouplingError)
- **HomingMethod**: Axis homing/referencing method (HARD_STOP, HOME_SENSOR, LIMIT_SWITCH, INDEX_PULSE, ABSOLUTE, NO_HOMING)
- **HomingDirection**: Mandatory approach direction for movement-based homing (Positive, Negative) — no default, explicit config required (FR-033a)
- **TailstockConfig**: Configuration for tailstock type and sensor mappings
- **IndexConfig**: Configuration for locking pin sensors and timing
- **BrakeConfig**: Configuration for brake output/input and timing
- **GuardConfig**: Configuration for safety guard with secure_speed
- **CouplingConfig**: Master-slave relationships, ratios, modulation parameters
- **HomingConfig**: Homing method selection and method-specific parameters
- **SafeStopConfig**: Per-axis safe stop category and related timing parameters
- **ControlUnitConfig**: Main configuration referencing axis configs and peripheral mappings
- **UniversalControlParameters**: Modular motion control gains (PID, feedforward, DOB, filters) with zero-gain component disabling
- **ControlOutputVector**: Per-axis cycle output (CalculatedTorque, TargetVelocity, TargetPosition, TorqueOffset) — always fully calculated, HAL selects fields by drive mode
- **CommandError**: Source locking and authorization errors with blocking source identification
- **AxisSourceLock**: Per-axis ownership tracking (locked source ID, LockReason enum, pre-pause targets)
- **IoRole**: Functional role enum for I/O points (e.g., `LimitMin1`, `EStop`, `BrakeOut3`) — single flat list shared by HAL and CU, convention `FunctionAxisNumber`
- **IoRegistry**: Runtime role→pin resolver built from `io.toml` at startup. Provides type-safe `read_di`/`read_ai`/`write_do`/`write_ao` by `IoRole`. No direct pin access in application code.
- **AxisId**: 1-based numeric index (1..N) for axis identification; maps to 0-based array index in SHM structs

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Control Unit cycle time < 1ms with all axes processed (up to 64 axes)
- **SC-002**: Axis start sequence (POWER_OFF → STANDBY) completes within 500ms for reference configuration (8-axis machine, each axis with brake + tailstock + safety guard). Measured from CU detecting valid `evo_hal_cu` heartbeat to first axis reaching Standby.
- **SC-003**: SAFETY_STOP reaction triggers within 1 cycle of condition detection
- **SC-004**: Position regulator achieves < 0.1mm steady-state error for reference axis (10kg load, 1m/s max velocity, 500mm travel)
- **SC-005**: Multi-axis sync deviation < 1 cycle (all axes reach SYNCHRONIZED in same cycle)
- **SC-006**: All safety peripheral faults detected within 1 cycle of occurrence
- **SC-007**: System processes complete state machines for 8 axes simultaneously without timing violations
- **SC-008**: Zero false-positive SAFETY_STOP triggers during normal operation over 24-hour test, using SC-002 reference config (8-axis machine) with continuous mixed motion profiles (position, velocity, homing cycles)
- **SC-009**: Recovery from SYSTEM_ERROR to IDLE < 100ms after conditions cleared and reset issued

#### P2P Library Performance Criteria

- **SC-010**: Single-reader enforcement: second `SegmentReader::attach()` to same segment returns `ShmError::ReaderAlreadyConnected` within 1 ms
- **SC-011**: P2P write latency (heartbeat increment + write_seq protocol + payload copy): WCET ≤ 5 µs for segments ≤ 8 KB on x86_64
- **SC-012**: P2P read latency (write_seq validation + payload copy): WCET ≤ 2 µs for segments ≤ 8 KB on x86_64
- **SC-013**: Heartbeat staleness detection within N+1 read cycles of writer stopping (N=3 → ≤ 4 ms for RT segments)
- **SC-014**: Version hash validation at connect time: single `u32` comparison (< 1 ns); `struct_version_hash<T>()` is `const fn` (zero runtime cost)
- **SC-015**: Destination enforcement at connect time: `ShmError::DestinationMismatch` returned before any data read (zero runtime overhead per cycle)
- **SC-016**: System operates 6 concurrent P2P segments for CU without timing violations; library supports ≥ 16 total system-wide segments

---

## Assumptions

- **A-001**: HAL is already running and its writer SHM segments (`evo_hal_cu`) are initialized before Control Unit starts. Other inbound segments (`evo_re_cu`, `evo_rpc_cu`) are optional — CU starts and operates without them; command sources connect/disconnect dynamically (FR-139)
- **A-002**: All hardware I/O pin mapping is defined in `io.toml` (FR-148) and resolved by functional role (`IoRole` enum). Both HAL and CU use the same `io.toml` — HAL performs physical I/O, CU reads/writes logical values via `IoRegistry` role-based API (FR-152)
- **A-003**: Real-time kernel (PREEMPT_RT) is required for **production deployment** to guarantee deterministic cycle timing. For development and testing, CU runs on standard Linux using the OSAL simulation mode (logical time), which preserves all functional behavior without RT scheduling (see plan.md Simulation Mode section)
- **A-004**: Axis parameters (kinematics, limits) are defined in HAL config, not duplicated
- **A-005**: evo_shared_memory library provides P2P (Point-to-Point) single-writer/single-reader segments with naming convention `evo_[SOURCE]_[DESTINATION]`
- **A-006**: Safety-critical functions (E-Stop relay, STO) are implemented in dedicated hardware per ISO 13849-1; Control Unit provides software-level coordination only
- **A-007**: All safety sensors use fail-safe wiring (NC preferred for critical functions)
- **A-008**: External `evo_watchdog` program monitors heartbeat/liveness of all system programs (including Control Unit) using evo_common mechanisms; watchdog design is outside this spec
- **A-009**: Recipe Executor enforces homing-before-production policy on its side (does not start non-reference programs for unreferenced axes); this is outside Control Unit scope
- **A-010**: RT cycle memory management follows project constitution (Principles XIII, XIV, XXIV): all buffers pre-allocated at startup, zero dynamic allocation during RT loop, mlock/hugetlbfs for page-fault prevention

---

## Dependencies

- **D-001**: evo_hal - Hardware Abstraction Layer (already specified in 003-hal-simulation)
- **D-002**: evo_shared_memory - Lock-free shared memory (specified in 002-shm-lifecycle; **requires P2P migration** — see Appendix: P2P SHM Migration Impact)
- **D-003**: evo_common - Shared structures and constants (already specified in 004-common-lib-setup)
- **D-004**: Linux PREEMPT_RT kernel for deterministic timing
- **D-005**: evo_watchdog - External liveness monitoring for all system programs

---

## Scope & Boundaries

### In Scope

- Hierarchical state machine architecture (5 levels)
- Global MachineState and SafetyState management
- Orthogonal per-axis state machines (PowerState, MotionState, OperationalMode, CouplingState, GearboxState, LoadingState)
- Per-axis safety flags (AxisSafetyState)
- Hierarchical error management with propagation rules
- Safety peripheral monitoring and blocking logic
- Master-slave coupling with synchronization
- Universal position control engine (PID + feedforward + DOB + filters)
- Per-cycle ControlOutputVector generation for HAL consumption
- Lag error detection and response
- Motion range monitoring (hardware and software limits)
- SAFETY_STOP detection and reaction
- P2P SHM integration (evo_hal_cu, evo_cu_hal, evo_re_cu, evo_rpc_cu, evo_cu_mqt, evo_cu_re)
- SHM segment header protocol (heartbeat counter, struct version hash)
- Command source locking with error reporting
- Safety pause/resume semantics (preserve targets across SAFETY_STOP)
- Unreferenced axis motion restrictions (5% speed, MANUAL/SERVICE only)
- Hard real-time cycle enforcement (overrun → SAFETY_STOP)
- RT memory policy (pre-allocation, zero-alloc cycle, constitution compliance)
- SHM-only diagnostic output (no file I/O in RT cycle)
- 1-based axis identification scheme
- Role-based I/O configuration via `io.toml` (IoRole enum, IoRegistry, NC/NO logic, analog scaling)

### Out of Scope

- Motion interpolation (trajectory generation) - evo_recipe_executor (separate module)
- Recipe execution and high-level sequencing - evo_recipe_executor (separate module)
- Hardware I/O access - evo_hal (separate module)
- User interface - evo_dashboard (separate module)
- Network communication (MQTT, gRPC) - evo_mqtt, evo_grpc (separate module)
- Functional safety certification (SIL) - hardware safety system
- State Freeze / Snapshot / Trace - separate module

---

## Appendix: P2P SHM Migration Impact

> **Context**: The existing `evo_shared_memory` library (specified in 002-shm-lifecycle) implements a **broadcast model** (single writer, many readers). This spec requires a **P2P (Point-to-Point) model** (single writer, single reader per segment). This appendix documents the architectural delta to guide the 002-shm-lifecycle update.

### Current Model (002-shm-lifecycle, broadcast)

- One writer, up to 1000 concurrent readers per segment (SC-001/SC-005 in 002)
- Segment discovery via filesystem (`/dev/shm/evo_segment_name`)
- No destination enforcement — any process can read any segment
- No heartbeat/staleness detection in segment header
- No struct versioning in segment header
- Reader count tracking and scaling metrics

### Required Model (P2P)

- **Exactly one writer and one reader** per segment (FR-130a)
- **Naming convention**: `evo_[SOURCE]_[DESTINATION]` with module abbreviation registry (FR-130b)
- **Destination enforcement**: Reader MUST only connect to segments where its abbreviation is in `[DESTINATION]` position; library rejects mismatches (FR-130a)
- **Heartbeat counter**: Monotonic counter in segment header, incremented every write cycle (FR-130c)
- **Struct version hash**: Compile-time layout hash in header, validated at connect time (FR-130d)
- Reader count fixed at 1 — no scaling metrics needed

### Breaking Changes Summary

| Area | Broadcast (current) | P2P (required) | Impact |
|------|---------------------|----------------|--------|
| Reader cardinality | 1..1000 | Exactly 1 | API change: `SegmentReader` creation must enforce single-reader |
| Access control | Open (any reader) | Destination-validated | New: module abbreviation registry + connect-time validation |
| Segment header | Version + size only | + heartbeat counter + struct hash | Header struct expansion; existing readers incompatible |
| Discovery | Filesystem enumeration | Name-based with `evo_[SRC]_[DST]` convention | `SegmentDiscovery` must parse naming convention |
| SC-001 (readers) | 10–1000 concurrent | 1 | Success criterion invalidated; replace with P2P-specific criteria |
| SC-005 (scaling) | Linear to 1000 | N/A | Remove or replace |
| Staleness detection | None (external only) | Built-in heartbeat | New header field + reader-side validation API |
| Version safety | None | Struct hash at connect | New header field + connect-time validation |

### Affected Modules

- **evo_shared_memory**: Core library rework — header format, reader cardinality enforcement, destination validation, heartbeat API, struct version hash API
- **evo_hal** (003-hal-simulation): Already uses evo_shared_memory; must migrate to P2P API for `evo_hal_cu` (writer) and `evo_cu_hal` (reader)
- **evo_watchdog**: Secondary crash detection; must understand new segment naming and heartbeat counters
- **002-shm-lifecycle spec**: Requires update to reflect P2P model; broadcast-specific success criteria (SC-001, SC-005) must be replaced

### Recommended Migration Sequence

1. Update 002-shm-lifecycle spec to P2P model
2. Implement P2P `evo_shared_memory` API (header, enforcement, heartbeat, version hash)
3. Update `evo_hal` to use new P2P segments
4. Implement `evo_control_unit` (this spec)
5. Update `evo_watchdog` for P2P awareness
