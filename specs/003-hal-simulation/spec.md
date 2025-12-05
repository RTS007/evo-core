# Feature Specification: HAL Simulation Driver

**Feature Branch**: `003-hal-simulation`
**Created**: 2025-12-05
**Status**: Draft
**Input**: User description: "Programujemy teraz podstawową funkcjonalność evo-core..."

## Clarifications

### Session 2025-12-05
- Q: How should the simulation loop cycle time be determined? → A: **Configurable**: Allow the user to define the cycle time in the YAML config (default 1ms).
- Q: How should position data be represented in Shared Memory? → A: **User Units (f64)**: SHM stores positions in mm/degrees. HAL handles the conversion to/from hardware increments based on config.
- Q: How should the Control Unit command the HAL? → A: **Cyclic Setpoint (Streaming)**: Control Unit updates `TargetPosition` every cycle. HAL Simulation tries to follow this value immediately (subject to physics).
- Q: How should the Shared Memory layout be structured? → A: **Fixed Maximums (Static Struct)**: Define hard limits (64 Axes, 1024 DI, 1024 DO, 1024 AI, 1024 AO). SHM is always the size of the max config. Simple, safe, zero-copy.
- Q: How should the Control Interface be defined? → A: **Simplified (Enable/Reset)**: SHM uses generic booleans/bits: `Command.Enable`, `Command.Reset`, `Status.Ready`, `Status.Error`.
- Q: What configuration format should be used? → A: **YAML (Serde)**: Standard for EVO. Human-readable, supports comments.
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

- **FR-001**: HAL MUST load configuration from a structured **YAML** file specifying:
    - Simulation Cycle Time (default: 1ms).
    - Number and names of Digital/Analog Inputs/Outputs.
    - Number and names of Axes.
    - Axis parameters: Encoder resolution, Max Velocity, Max Acceleration, Lag Error Limit.
- **FR-001a**: HAL MUST validate all configuration values at startup and reject invalid configurations with specific error messages, including: negative/zero cycle times, negative/zero velocities or accelerations, counts exceeding system limits (>64 axes, >1024 IOs), duplicate names, and invalid encoder resolutions.
- **FR-002**: HAL MUST initialize the Shared Memory (SHM) structure using the existing `evo_shared_memory` library, matching the configuration with a fixed-size layout (Max 64 Axes, Max 1024 DI, Max 1024 DO, Max 1024 AI, Max 1024 AO).
- **FR-003**: HAL Driver Simulation MUST read "Command" values (Target Position [User Units], Command Flags [Enable, Reset], IO States) from SHM cyclically.
- **FR-004**: HAL Driver Simulation MUST write "Status" values (Actual Position [User Units], Actual Velocity, Status Flags [Ready, Error], IO States) to SHM cyclically.
- **FR-005**: HAL Driver Simulation MUST simulate axis physics:
    - Calculate required velocity to reach Target Position.
    - Apply Acceleration/Deceleration limits to current velocity.
    - Integrate limited Velocity to update Actual Position.
- **FR-006**: HAL Driver Simulation MUST calculate Lag Error = |Target Position - Actual Position|.
- **FR-007**: HAL Driver Simulation MUST trigger an error state if Lag Error > Configured Limit.
- **FR-007a**: Error recovery semantics: `Command.Reset` SHALL clear `Status.Error` only when `Command.Enable=false`. Motion is permitted again only after `Command.Enable=true` following a successful reset. This prevents unintended immediate restart.
- **FR-008**: HAL Driver Simulation MUST support "Simulated Load/Blockage" (optional, for testing lag).
- **FR-009**: HAL Driver Simulation MUST execute the simulation loop at the configured cycle time (e.g., using a high-resolution timer). In RT environment, strict timing compliance is required even in simulation. In non-RT environment, timing violations SHALL be logged but simulation continues.
- **FR-010**: HAL Driver Simulation MUST handle system-level failures (SHM corruption, memory allocation failures) by logging clear error messages and terminating gracefully to allow clean restart without affecting other system components.

### Key Entities

- **MachineConfig**: Configuration structure loaded from file.
- **AxisState**: Current state of an axis (Position, Velocity, Status Word).
- **IOState**: Current state of IOs.
- **SimulationModel**: The physics model for an axis.

## Success Criteria *(mandatory)*

- **SC-001**: HAL starts successfully with a valid configuration.
- **SC-002**: Axis moves in simulation with realistic acceleration (not instant jump).
- **SC-003**: Lag Error is correctly detected and reported in SHM when forced.
- **SC-004**: CPU usage of the simulation loop is minimal (<5% on standard PC) to allow other processes to run.

## Assumptions

- We are using the existing `evo_shared_memory` library with its built-in synchronization mechanisms.
- We are running on Linux (standard or PREEMPT_RT).
- Configuration format is standard (e.g., YAML).
