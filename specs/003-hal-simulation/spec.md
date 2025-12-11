# Feature Specification: HAL Simulation Driver

**Feature Branch**: `003-hal-simulation`
**Created**: 2025-12-05
**Status**: Draft
**Input**: User description: "Programujemy teraz podstawową funkcjonalność evo-core..."

## Clarifications

### Session 2025-12-10
- Q: How should axis configuration files be organized? → A: **Separate TOML file per axis** - Each axis has its own TOML file (e.g., `axis_01.toml`, `axis_02.toml`). Main `machine.toml` references axis files via path. This allows more parameters per axis, cleaner structure, and easy comparison/diff between axes.
- Q: How should Slave axes couple to their Master axis? → A: **1:1 ratio with preserved offset** - Slave follows master's movement (delta), not absolute position. When coupled, the offset between master and slave is captured. From that point: `slave_position = master_position + captured_offset`. Config adds `coupling_offset` field. Movement is synchronized 1:1, but positions can differ.
- Q: What value range and units should Analog I/O use in SHM? → A: **Dual representation** - Two registers per analog channel: (1) Normalized f64 0.0–1.0 (fundamental for control), (2) Scaled f64 in engineering units. Scaling computed from `min_value`, `max_value`, and `curve_type` (Linear|Parabolic|Cubic). If no scaling defined, both registers show identical values.
- Q: What should be the initial state of axes when HAL starts? → A: **Position 0.0, unreferenced + state persistence** - First run: axes start at 0.0 with `Status.Referenced=false`. HAL persists state to file on shutdown and restores on startup (simulates absolute encoders / powered-off machine). Combines safety with realistic behavior.
- Q: What referencing types should be supported? → A: **6 modes (0-5)** with full configuration:
  - Mode 0: No referencing needed
  - Mode 1: Move to reference switch, then find K0 (index pulse) as reference point
  - Mode 2: Move to reference switch, use that position as reference
  - Mode 3: Find K0 index pulse only, use as reference
  - Mode 4: Like mode 1, but use limit switch instead of separate reference switch
  - Mode 5: Like mode 2, but use limit switch instead of separate reference switch
  - Config parameters: `referencing_required` (yes/perhaps/no), `referencing_mode` (0-5), `reference_switch` (DI number), `normally_closed_reference_switch` (bool), `negative_referencing_direction` (bool, default: yes), `referencing_speed` (user units/s), `show_k0_distance_error` (bool)
- Q: Where should HAL state persistence file be stored? → A: **Configurable** - Add `state_file_path` to config for maximum flexibility in different deployment scenarios.

### Session 2025-12-09
- Q: How should the HAL Simulation driver locate its configuration file? → A: **Command-line argument with default constant** - Use `--config <path>` arg. Default path defined as a constant in `evo_common` (e.g., `evo_common::config::DEFAULT_CONFIG_PATH`).
- Q: Where should HAL configuration structures be defined? → A: **evo_common::hal::config** - All HAL config structures (MachineConfig, AxisConfig, IOConfig) defined in evo_common, HAL Sim only consumes them.
- Q: How should "Simulated Load/Blockage" be triggered? → A: **Natural Physics Limits** - No explicit fault injection. Lag error occurs naturally when the Control Unit commands motion exceeding the simulated axis's `max_velocity` or `max_acceleration`.
- Q: How should Real-Time thread properties be configured? → A: **External Management** - Out of scope for HAL Sim. An external supervisor (e.g., `evo-watchdog`) or systemd unit handles `chrt`/`taskset`. HAL Sim just runs.
- Q: How should the HAL Simulation handle "Referencing"? → A: **Simulated Offset** - When unreferenced, the axis takes unreferenced distance to zero as distance to move. Referencing command triggers a simulated move to zero (or index pulse) and resets the internal offset, setting `Status.Referenced = true`. Needs the proper referencing type in config.
- Q: How should the HAL Simulation handle "Emergency Stop" (E-Stop)? → A: **Control Unit Responsibility** - HAL Sim does not handle E-Stop logic internally. It assumes the Control Unit stops sending position updates or handles the safety state. HAL just follows the `TargetPosition`.
- Q: Where should system limit constants be defined? → A: **evo_common::hal::consts** - All HAL limits (MAX_AXES, MAX_DI, MAX_DO, MAX_AI, MAX_AO) in one place. Note: evo_common is aliased as `evo` in Cargo.toml across all crates.
- Q: Should axis parameters have default values or be required? → A: **Required, but context-dependent** - Parameters are required only if relevant for the axis type. E.g., on/off axis without encoder does not require max_velocity/max_acceleration. Validation logic must consider axis type.
- Q: What axis types should be supported? → A: **Full set (4 types)**: `Simple` (type 0, on/off), `Positioning` (type 1, with encoder and full kinematics), `Slave` (type 2, coupled to master axis), `Measurement` (type 3, encoder without drive). Each type has different required parameters.
- Q: Where should master-slave axis validation occur? → A: **evo::hal::config** - All validation including master-slave relationships in `MachineConfig::validate()`, shared across all HAL drivers.

### Session 2025-12-05
- Q: How should the simulation loop cycle time be determined? → A: **Configurable**: Allow the user to define the cycle time in the TOML config (default 1ms).
- Q: How should position data be represented in Shared Memory? → A: **User Units (f64)**: SHM stores positions in mm/degrees. HAL handles the conversion to/from hardware increments based on config.
- Q: How should the Control Unit command the HAL? → A: **Cyclic Setpoint (Streaming)**: Control Unit updates `TargetPosition` every cycle. HAL Simulation tries to follow this value immediately (subject to physics).
- Q: How should the Shared Memory layout be structured? → A: **Fixed Maximums (Static Struct)**: Define hard limits (64 Axes, 1024 DI, 1024 DO, 1024 AI, 1024 AO). SHM is always the size of the max config. Simple, safe, zero-copy.
- Q: How should the Control Interface be defined? → A: **Simplified (Enable/Reset)**: SHM uses generic booleans/bits: `Command.Enable`, `Command.Reset`, `Status.Ready`, `Status.Error`.
- Q: What configuration format should be used? → A: **TOML (Serde)**: Standard for EVO. Human-readable, supports comments.
- Q: How should recovery from Lag Error be handled? → A: **Reset only with Enable=false; re-arm via Enable**.
- Q: How should HAL handle system-level failures (SHM corruption, memory allocation failures)? → A: Fail gracefully with error logging and allow clean restart.
- Q: How should configuration validation be handled? → A: Validate all config values at startup, reject invalid configs with specific error messages.
- Q: How should SHM synchronization be handled for concurrent access? → A: Use evo_shared_memory library, where everything is already prepared.
- Q: How should timing violations be handled in simulation? → A: RT environment: strict timing must be maintained even in simulation. Non-RT environment: timing should be maintained but violations are tolerated with logging.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Configure and Start HAL Simulation (Priority: P1)

As an integrator, I want to define the machine hardware (IOs, Axes) in a configuration file and start the HAL Simulation driver, so that the system initializes the Shared Memory (SHM) with the correct structure.

**Why this priority**: Foundation for all other operations. Without config and SHM, nothing works.

**Independent Test**: Create a config file, run the HAL binary, and verify SHM is created with correct size and fields using a monitoring tool.

**Acceptance Scenarios**:

1. **Given** a configuration file with 2 Axes and 8 Digital IOs, **When** HAL starts, **Then** SHM is initialized with space for 2 Axes and 8 DIOs.
2. **Given** an invalid configuration file, **When** HAL starts, **Then** it exits with a clear error message.

---

### User Story 2 - Digital and Analog IO Control (Priority: P2)

As a developer, I want to write to Digital/Analog Outputs in SHM and see them reflected in the Simulation state, and read simulated Digital/Analog Inputs from SHM, so that I can verify IO handling.

**Why this priority**: Basic connectivity test before complex axis control.

**Independent Test**: Use a test tool to write DO=True in SHM, verify HAL reads it. Simulate DI=True in HAL, verify SHM updates.

**Acceptance Scenarios**:

1. **Given** HAL is running, **When** I set Digital Output 1 to TRUE in SHM, **Then** the Simulation internal state for DO 1 becomes TRUE.
2. **Given** HAL is running, **When** the Simulation logic toggles Digital Input 1 to TRUE, **Then** the SHM value for DI 1 becomes TRUE.

---

### User Story 3 - Axis Motion Simulation (Priority: P3)

As a control engineer, I want to command an axis to move to a position via SHM and see the actual position update over time with realistic physics (acceleration/velocity), so that I can tune the control loop.

**Why this priority**: Core value of the simulation - testing motion logic without hardware.

**Independent Test**: Set Target Position = 1000 in SHM. Monitor Actual Position in SHM. It should increase gradually, not jump instantly.

**Acceptance Scenarios**:

1. **Given** Axis 1 is at position 0, **When** Target Position is set to 1000, **Then** Actual Position increases over multiple cycles until it reaches 1000.
2. **Given** Axis 1 is moving, **When** Target Velocity is set to 0, **Then** Axis decelerates to a stop.

---

### User Story 4 - Lag Error Detection (Priority: P4)

As a safety engineer, I want the axis to stop immediately if the difference between Target and Actual position exceeds a configured limit (Lag Error), so that I can prevent damage in a real system (simulated here).

**Why this priority**: Critical safety feature simulation.

**Independent Test**: Configure low max speed for simulation but command high speed change in SHM. Lag should grow until error triggers.

**Acceptance Scenarios**:

1. **Given** Lag Limit is 10 units, **When** Lag exceeds 10 units (e.g. due to simulated blockage or aggressive command), **Then** Axis State changes to ERROR and motion stops.

### Edge Cases

- What happens when SHM is locked by another process? HAL should retry or fail gracefully.
- How does system handle configuration with 0 axes or 0 IOs? Should run but do nothing.
- What happens if simulation cycle time is exceeded? RT environment: critical error, may need to stop. Non-RT environment: log warning and continue simulation. 

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: HAL MUST load configuration from structured **TOML** files. The main config path SHALL be provided via command-line argument (e.g., `--config`), defaulting to a standard constant defined in `evo_common` if omitted. Configuration is split into:
    - **Main machine config** (`machine.toml`): Optional `cycle_time_us` (defaults to `evo_common::prelude::DEFAULT_CYCLE_TIME_US` = 1000μs if omitted), Digital/Analog I/O definitions, list of axis file paths, `state_file`.
    - **Separate axis config files** (e.g., `axis_01.toml`, `axis_02.toml`): Each axis has its own TOML file referenced from main config. This allows extensive per-axis parameters and easy comparison between axes.
    - Supported axis types (defined in `evo::hal::config::AxisType`):
        - **Simple (0)**: On/off axis without position feedback. No motion parameters required.
        - **Positioning (1)**: Full servo axis with encoder. Required: encoder resolution, max velocity, max acceleration, lag error limit.
        - **Slave (2)**: Axis coupled to a master axis. Required: master_axis reference, plus encoder parameters. Follows master position.
        - **Measurement (3)**: Encoder-only axis without drive. Required: encoder resolution. Used for length/position measurement.
    - Configuration structures (`MachineConfig`, `AxisConfig`, `IOConfig`, `AxisType`) SHALL be defined in `evo_common::hal::config` as the single source of truth.
- **FR-001a**: HAL MUST validate all configuration values at startup and reject invalid configurations with specific error messages, including: negative/zero cycle times, negative/zero velocities or accelerations, counts exceeding system limits (>64 axes, >1024 IOs), duplicate names, invalid encoder resolutions, and master-slave relationships (master axis must exist, must not be a Slave type, must have lower index than slave). System limits SHALL be defined as constants in `evo_common::hal::consts` (imported as `evo::hal::consts`). Validation logic SHALL be implemented in `MachineConfig::validate()` method.
- **FR-002**: HAL MUST initialize the Shared Memory (SHM) structure using the existing `evo_shared_memory` library, matching the configuration with a fixed-size layout (Max 64 Axes, Max 1024 DI, Max 1024 DO, Max 1024 AI, Max 1024 AO).
- **FR-003**: HAL Driver Simulation MUST read "Command" values (Target Position [User Units], Command Flags [Enable, Reset], IO States) from SHM cyclically.
- **FR-004**: HAL Driver Simulation MUST write "Status" values (Actual Position [User Units], Actual Velocity, Status Flags [Ready, Error], IO States) to SHM cyclically.
- **FR-004a**: Analog I/O SHALL use **dual representation** in SHM - two registers per channel: (1) Normalized f64 (0.0–1.0, linear scale), (2) Scaled f64 (engineering units). Scaling is computed from `min_value`, `max_value`, and `curve_type` (Linear|Parabolic|Cubic). If no scaling config, both registers show identical values.
- **FR-005**: HAL Driver Simulation MUST simulate axis physics:
    - Calculate required velocity to reach Target Position.
    - Apply Acceleration/Deceleration limits to current velocity.
    - Integrate limited Velocity to update Actual Position.
    - **Slave Axis Coupling**: Slave axes (type 2) follow master axis movement with 1:1 ratio and preserved offset. When coupled, `slave_position = master_position + coupling_offset`. The offset is captured at coupling time and preserved during motion.
    - **Referencing Simulation**: Support 6 referencing modes (0-5):
        - Mode 0: No referencing needed
        - Mode 1: Move to reference switch, then find K0 (index pulse)
        - Mode 2: Move to reference switch, use that position
        - Mode 3: Find K0 index pulse only
        - Mode 4: Like mode 1, using limit switch instead of reference switch
        - Mode 5: Like mode 2, using limit switch instead of reference switch
    - Referencing config includes: `referencing_required` (yes/no), `referencing_mode`, `reference_switch` (DI), `normally_closed_reference_switch`, `negative_referencing_direction`, `referencing_speed`, `show_k0_distance_error`.
- **FR-006**: HAL Driver Simulation MUST calculate Lag Error = |Target Position - Actual Position|.
- **FR-007**: HAL Driver Simulation MUST trigger an error state if Lag Error > Configured Limit.
- **FR-007a**: Error recovery semantics: `Command.Reset` SHALL clear `Status.Error` only when `Command.Enable=false`. Motion is permitted again only after `Command.Enable=true` following a successful reset. This prevents unintended immediate restart.
- **FR-009**: HAL Driver Simulation MUST execute the simulation loop at the configured cycle time (e.g., using a high-resolution timer). RT environment SHALL be auto-detected at startup using `sched_getscheduler()` - if running with SCHED_FIFO or SCHED_RR, RT mode is active. In RT mode, strict timing compliance is required even in simulation. In non-RT mode, timing violations SHALL be logged but simulation continues.
- **FR-010**: HAL Driver Simulation MUST handle system-level failures (SHM corruption, memory allocation failures) by logging clear error messages and terminating gracefully to allow clean restart without affecting other system components.
- **FR-011**: HAL MUST persist axis state (positions, referenced status) to a file on shutdown and restore on startup. The state file path SHALL be configurable via `state_file` in config. On first run (no state file), axes start at position 0.0 with `Status.Referenced=false`. This simulates absolute encoder behavior across power cycles.
- **FR-012**: Axes with `referencing_required=perhaps` SHALL use persisted position if available, otherwise require referencing. Axes with `referencing_required=yes` SHALL always require referencing regardless of persisted state.

### Key Entities

- **MachineConfig**: Configuration structure loaded from file (in `evo::hal::config`).
- **AxisConfig**: Configuration for a single axis including type and type-specific parameters (in `evo::hal::config`).
- **AxisType**: Enum defining axis types: Simple(0), Positioning(1), Slave(2), Measurement(3) (in `evo::hal::config`).
- **IOConfig**: Configuration for Digital/Analog IO points (in `evo::hal::config`). Analog IO includes `min_value`, `max_value`, `curve_type` for scaling.
- **AnalogCurveType**: Enum defining scaling curves: Linear, Parabolic, Cubic (in `evo::hal::config`).
- **ReferencingMode**: Enum defining referencing modes 0-5 (in `evo::hal::config`).
- **ReferencingConfig**: Configuration for axis referencing: mode, required, switch, direction, speed (in `evo::hal::config`).
- **AxisState**: Current state of an axis (Position, Velocity, Status Word, Referenced).
- **IOState**: Current state of IOs.
- **SimulationModel**: The physics model for an axis (simulation-specific, in HAL Sim crate).

## Success Criteria *(mandatory)*

- **SC-001**: HAL starts successfully with a valid configuration.
- **SC-002**: Axis moves in simulation with realistic acceleration (not instant jump).
- **SC-003**: Lag Error is correctly detected and reported in SHM when forced.
- **SC-004**: CPU usage of the simulation loop is minimal (<5% on standard PC) to allow other processes to run.

## Assumptions

- We are using the existing `evo_shared_memory` library with its built-in synchronization mechanisms.
- We are running on Linux (standard or PREEMPT_RT).
- Configuration format is standard (e.g., TOML).
- All EVO crates import `evo_common` under the alias `evo` (e.g., `evo = { package = "evo_common", path = "../evo_common" }`), so imports use `evo::hal::config`, `evo::hal::consts`, etc.
