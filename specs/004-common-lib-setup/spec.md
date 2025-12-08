# Feature Specification: Common Library Setup

**Feature Branch**: `004-common-lib-setup`
**Created**: 2025-12-05
**Status**: Draft
**Input**: User description: "Chciałbym zbudować podstawy evo_common..."

## Clarifications

### Session 2025-12-08
- Q: Should ConfigLoader support environment variable overrides? → A: No, file configuration only (Option B).
- Q: What fields should SharedConfig contain? → A: Minimal set: `log_level` (Enum) and `service_name` (String).
- Q: How should log_level be represented? → A: Always use Enums where possible. `LogLevel` enum with variants: `Trace`, `Debug`, `Info`, `Warn`, `Error`.
- Q: What should the ConfigLoader trait API look like? → A: Simple with Result: `fn load(path: &Path) -> Result<Self, ConfigError>`.
- Q: What error variants should ConfigError include? → A: Essential: `FileNotFound`, `ParseError(String)`, `ValidationError(String)`.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Shared Constants (Priority: P1)

As a developer, I want to access shared constants from a central library, so that I don't have to duplicate magic numbers and limits across different crates.

**Why this priority**: Reduces code duplication and ensures consistency.

**Independent Test**:
1. Create a test in `evo_common` that asserts the values of exported constants.
2. Verify `evo_shared_memory` compiles and runs using the imported constants.

**Acceptance Scenarios**:
1. **Given** `evo_common` is added as a dependency, **When** I import `EVO_SHM_MAGIC`, **Then** I get the correct value `0x45564F5F53484D00`.

### User Story 2 - Configuration Loading (Priority: P2)

As a developer, I want a standard way to load configuration that includes common settings, so that all apps have a consistent base configuration.

**Why this priority**: Standardizes configuration management.

**Independent Test**:
1. Define a struct `AppConfig` that includes `SharedConfig`.
2. Create a config file (e.g., `config.toml`).
3. Use `ConfigLoader` to load the file into `AppConfig`.

**Acceptance Scenarios**:
1. **Given** a config file with shared settings, **When** I load it using `ConfigLoader`, **Then** the `SharedConfig` fields are populated correctly.

### Edge Cases

- **Circular Dependencies**: Ensure `evo_common` does not depend on any crate that depends on it.
- **Missing Config File**: `ConfigLoader` should return a clear error if the config file is missing.
- **Invalid Config Format**: `ConfigLoader` should return a clear error if the config file has invalid syntax or structure.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: ~~`evo_common` MUST have a modular structure with `config` and `constants` top-level modules.~~ *Superseded by FR-006a: domain modules pattern (`shm`, `hal`) with `consts` and `config` submodules.*
- **FR-002**: ~~`constants` module MUST be subdivided into `shared` (global constants) and `apps` (application-specific constants).~~ *Superseded by FR-006a.*
- **FR-003**: ~~`config` module MUST be subdivided into `shared` (common config structs) and `apps` (application-specific config structs).~~ *Superseded by FR-006a.*
- **FR-004**: `evo_common` MUST define a `ConfigLoader` trait with signature `fn load(path: &Path) -> Result<Self, ConfigError>` for loading configuration from TOML files.
- **FR-004a**: `evo_common` MUST define a `ConfigError` enum with variants: `FileNotFound`, `ParseError(String)`, `ValidationError(String)`.
- **FR-005**: `evo_common` MUST define a `SharedConfig` struct containing exactly two fields: `log_level` (`LogLevel` enum with variants `Trace`, `Debug`, `Info`, `Warn`, `Error`) and `service_name` (String identifying the application instance).
- **FR-005a**: `evo_common` MUST prefer enums over strings for all typed configuration fields to ensure compile-time safety.
- **FR-006**: The following constants MUST be moved from `evo_shared_memory` to `evo_common::shm::consts` (imported as `evo::shm::consts`):
    - `EVO_SHM_MAGIC`
    - `SHM_MIN_SIZE`
    - `SHM_MAX_SIZE`
    - `CACHE_LINE_SIZE`
- **FR-006a**: `evo_common` MUST use domain modules (`shm`, `hal`) with `consts` and `config` submodules for clear separation between constants and configuration.
- **FR-006b**: Consumer crates SHOULD use alias `evo = { package = "evo_common", ... }` for shorter imports.
- **FR-007**: `evo_shared_memory` MUST be updated to:
    - Add `evo_common` as dependency
    - Remove constant definitions from `segment.rs`
    - Remove constant re-exports from `lib.rs`
    - Import constants directly from `evo_common`
- **FR-007a**: All examples, tests, and documentation in `evo_shared_memory` MUST be updated to import constants from `evo_common` (no backward compatibility re-exports).

### Key Entities

- **SharedConfig**: Struct with common configuration fields.
- **ConfigLoader**: Trait for loading configuration.

## Success Criteria *(mandatory)*

- **SC-001**: `evo_common` library compiles without errors.
- **SC-002**: `evo_shared_memory` compiles and passes tests using constants from `evo_common`.
- **SC-003**: A unit test in `evo_common` demonstrates loading a configuration struct composed of `SharedConfig` and app-specific fields.

## Assumptions

- `serde` will be used for serialization/deserialization.
- `toml` crate will be used for TOML parsing (selected per research.md over config-rs/figment for simplicity).
