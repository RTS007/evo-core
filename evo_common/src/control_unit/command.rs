//! Command types for the Control Unit (FR-135–FR-137).
//!
//! Defines `CommandSource`, `LockReason`, `AxisSourceLock`, `PauseTargets`,
//! and `ServiceBypassConfig`.

use serde::{Deserialize, Serialize};

use super::state::{AxisId, OperationalMode};

/// Source of commands for an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum CommandSource {
    /// No lock held.
    None = 0,
    /// Recipe Executor (via evo_re_cu).
    RecipeExecutor = 1,
    /// gRPC API (via evo_rpc_cu).
    GrpcApi = 2,
    /// Internal safety override.
    Safety = 3,
}

impl CommandSource {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::RecipeExecutor),
            2 => Some(Self::GrpcApi),
            3 => Some(Self::Safety),
            _ => None,
        }
    }
}

impl Default for CommandSource {
    fn default() -> Self {
        Self::None
    }
}

/// Reason for axis lock acquisition (FR-135).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum LockReason {
    /// Recipe Executor is executing a program on this axis.
    RecipeRunning = 0,
    /// Operator has manual control via gRPC/HMI.
    ManualControl = 1,
    /// Homing sequence active (FR-030).
    HomingInProgress = 2,
    /// Axis in SERVICE operational mode.
    ServiceMode = 3,
    /// Safety system paused motion (SAFETY_STOP active).
    SafetyPause = 4,
    /// Gear change assistance in progress (FR-062).
    GearAssist = 5,
}

impl LockReason {
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::RecipeRunning),
            1 => Some(Self::ManualControl),
            2 => Some(Self::HomingInProgress),
            3 => Some(Self::ServiceMode),
            4 => Some(Self::SafetyPause),
            5 => Some(Self::GearAssist),
            _ => None,
        }
    }
}

impl Default for LockReason {
    fn default() -> Self {
        Self::RecipeRunning
    }
}

/// Preserved targets across SAFETY_STOP pause.
#[derive(Debug, Clone, Copy, Default)]
pub struct PauseTargets {
    /// Target position before pause [mm].
    pub target_position: f64,
    /// Target velocity before pause [mm/s].
    pub target_velocity: f64,
    /// Operational mode before pause.
    pub operational_mode: OperationalMode,
}

/// Per-axis command source lock (FR-135).
///
/// Tracks which source holds control of this axis and preserves
/// pre-pause targets across SAFETY_STOP events.
#[derive(Debug, Clone, Copy, Default)]
pub struct AxisSourceLock {
    /// Who holds the lock.
    pub locked_source: CommandSource,
    /// Why the lock was acquired.
    pub lock_reason: LockReason,
    /// Preserved targets when SAFETY_STOP pauses motion (FR-136, FR-137).
    pub pre_pause_targets: Option<PauseTargets>,
}

impl AxisSourceLock {
    /// Returns true if the axis is locked by any source.
    #[inline]
    pub const fn is_locked(&self) -> bool {
        !matches!(self.locked_source, CommandSource::None)
    }

    /// Returns true if the given source can command this axis.
    #[inline]
    pub fn can_command(&self, source: CommandSource) -> bool {
        matches!(self.locked_source, CommandSource::None) || self.locked_source == source
    }

    /// Acquire the lock for the given source and reason.
    /// Returns `true` if lock was acquired, `false` if already locked by a different source.
    pub fn acquire(&mut self, source: CommandSource, reason: LockReason) -> bool {
        if self.can_command(source) {
            self.locked_source = source;
            self.lock_reason = reason;
            true
        } else {
            false
        }
    }

    /// Release the lock (only if held by the given source).
    pub fn release(&mut self, source: CommandSource) -> bool {
        if self.locked_source == source {
            self.locked_source = CommandSource::None;
            self.lock_reason = LockReason::RecipeRunning;
            self.pre_pause_targets = None;
            true
        } else {
            false
        }
    }

    /// Force-release the lock (used by safety system).
    pub fn force_release(&mut self) {
        self.locked_source = CommandSource::None;
        self.lock_reason = LockReason::RecipeRunning;
        self.pre_pause_targets = None;
    }
}

/// Service mode bypass configuration (FR-001a).
///
/// Only axes in `bypass_axes` may be operated during SERVICE mode.
/// All other axes remain locked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceBypassConfig {
    /// Axes allowed to operate in SERVICE mode (max 64).
    pub bypass_axes: heapless::Vec<AxisId, 64>,
    /// Velocity limit during SERVICE mode [mm/s] (SAFE_REDUCED_SPEED hardware limit).
    #[serde(default = "default_max_service_velocity")]
    pub max_service_velocity: f64,
}

fn default_max_service_velocity() -> f64 {
    50.0
}

impl Default for ServiceBypassConfig {
    fn default() -> Self {
        Self {
            bypass_axes: heapless::Vec::new(),
            max_service_velocity: 50.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_source_roundtrip() {
        for v in 0..=3u8 {
            let src = CommandSource::from_u8(v).unwrap();
            assert_eq!(src as u8, v);
        }
        assert!(CommandSource::from_u8(4).is_none());
    }

    #[test]
    fn lock_reason_roundtrip() {
        for v in 0..=5u8 {
            let reason = LockReason::from_u8(v).unwrap();
            assert_eq!(reason as u8, v);
        }
        assert!(LockReason::from_u8(6).is_none());
    }

    #[test]
    fn source_lock_acquire_release() {
        let mut lock = AxisSourceLock::default();
        assert!(!lock.is_locked());
        assert!(lock.can_command(CommandSource::RecipeExecutor));
        assert!(lock.can_command(CommandSource::GrpcApi));

        // Acquire by RE
        assert!(lock.acquire(CommandSource::RecipeExecutor, LockReason::RecipeRunning));
        assert!(lock.is_locked());
        assert!(lock.can_command(CommandSource::RecipeExecutor));
        assert!(!lock.can_command(CommandSource::GrpcApi));

        // Cannot acquire by RPC
        assert!(!lock.acquire(CommandSource::GrpcApi, LockReason::ManualControl));

        // Release by RE
        assert!(lock.release(CommandSource::RecipeExecutor));
        assert!(!lock.is_locked());

        // Cannot release by wrong source
        lock.acquire(CommandSource::GrpcApi, LockReason::ManualControl);
        assert!(!lock.release(CommandSource::RecipeExecutor));
        assert!(lock.is_locked());

        // Force release
        lock.force_release();
        assert!(!lock.is_locked());
    }
}
