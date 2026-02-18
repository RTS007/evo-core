//! # Watchdog Trait
//!
//! Defines the supervisor contract for process lifecycle management.
//! The `evo` binary implements this trait to spawn, monitor, restart,
//! and shut down child processes (HAL, CU, etc.).
//!
//! # Design
//!
//! The trait is deliberately thin — it captures the four core operations
//! that any watchdog implementation must provide, without mandating a
//! specific process management strategy (fork, systemd, container, etc.).

use std::path::Path;

/// Identifies a managed child module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManagedModule {
    /// Hardware Abstraction Layer (`evo_hal`).
    Hal,
    /// Control Unit (`evo_control_unit`).
    Cu,
    /// Recipe Executor (`evo_recipe_executor`).
    RecipeExecutor,
    /// gRPC bridge (`evo_grpc`).
    Grpc,
    /// MQTT bridge (`evo_mqtt`).
    Mqtt,
}

/// Health status returned by [`Watchdog::health_check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// Module is running and its heartbeat is current.
    Healthy,
    /// Module process is alive but heartbeat is stale (possible hang).
    Stale {
        /// Seconds since last heartbeat update.
        age_secs: u64,
    },
    /// Module process has exited.
    Dead {
        /// Exit code if available.
        exit_code: Option<i32>,
    },
    /// Module was never started or is not being tracked.
    Unknown,
}

/// Error type for watchdog operations.
#[derive(Debug, thiserror::Error)]
pub enum WatchdogError {
    /// Failed to spawn the requested module.
    #[error("failed to spawn {module:?}: {reason}")]
    SpawnFailed {
        module: ManagedModule,
        reason: String,
    },

    /// Module did not become ready within the expected timeout.
    #[error("{module:?} not ready after {timeout_s:.1}s")]
    ReadyTimeout {
        module: ManagedModule,
        timeout_s: f64,
    },

    /// Maximum restart attempts exhausted.
    #[error("max restarts ({max}) exhausted for {module:?}")]
    RestartsExhausted {
        module: ManagedModule,
        max: u32,
    },

    /// Generic I/O or system error.
    #[error("watchdog error: {0}")]
    Other(String),
}

/// Supervisor contract for EVO process lifecycle management.
///
/// Implementors manage child process spawning, health monitoring,
/// restart with backoff, and coordinated shutdown.
///
/// # Example
///
/// ```rust,ignore
/// struct EvoSupervisor { /* ... */ }
///
/// impl Watchdog for EvoSupervisor {
///     fn spawn_module(&mut self, module: ManagedModule, config_dir: &Path)
///         -> Result<u32, WatchdogError> { /* ... */ }
///     fn health_check(&self, module: ManagedModule) -> HealthStatus { /* ... */ }
///     fn restart_module(&mut self, module: ManagedModule)
///         -> Result<u32, WatchdogError> { /* ... */ }
///     fn shutdown_all(&mut self) -> Result<(), WatchdogError> { /* ... */ }
/// }
/// ```
pub trait Watchdog {
    /// Spawn a child module process.
    ///
    /// Returns the OS PID of the spawned process on success.
    /// The implementation should forward `config_dir` to the child
    /// via `--config-dir` CLI argument.
    fn spawn_module(
        &mut self,
        module: ManagedModule,
        config_dir: &Path,
    ) -> Result<u32, WatchdogError>;

    /// Query the health of a managed module.
    ///
    /// Combines process-level checks (is the PID alive?) with
    /// optional SHM heartbeat probing for hang detection.
    fn health_check(&self, module: ManagedModule) -> HealthStatus;

    /// Restart a module that has died or become unhealthy.
    ///
    /// The implementation should:
    /// 1. Terminate the existing process (if still alive).
    /// 2. Clean up associated SHM segments.
    /// 3. Re-spawn with the same config.
    /// 4. Return the new PID.
    fn restart_module(
        &mut self,
        module: ManagedModule,
    ) -> Result<u32, WatchdogError>;

    /// Shut down all managed modules in reverse-startup order.
    ///
    /// Expected sequence:
    /// 1. Send SIGTERM to each child (CU first, then HAL, etc.).
    /// 2. Wait up to a timeout for graceful exit.
    /// 3. Escalate to SIGKILL for unresponsive processes.
    /// 4. Clean up all `evo_*` SHM segments.
    fn shutdown_all(&mut self) -> Result<(), WatchdogError>;
}
