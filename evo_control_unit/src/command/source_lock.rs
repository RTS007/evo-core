//! Source locking logic (T077, FR-135, FR-136, FR-137).
//!
//! AxisSourceLock acquisition/release/rejection with blocking source
//! identification and pause target preservation across SAFETY_STOP.
//!
//! Safety has unconditional override priority (FR-137) — it can pause any
//! source at any time, but does not interfere with command ownership or
//! target memory.

use evo_common::control_unit::command::{
    AxisSourceLock, CommandSource, LockReason, PauseTargets,
};
use evo_common::control_unit::error::CommandError;
use evo_common::control_unit::state::OperationalMode;

/// Result of a source lock operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockResult {
    /// Lock acquired successfully.
    Acquired,
    /// Lock released successfully.
    Released,
    /// Operation rejected — axis locked by another source.
    Rejected {
        /// Who holds the lock.
        held_by: CommandSource,
        /// Why the lock is held.
        reason: LockReason,
    },
    /// Lock was not held by caller — release ignored.
    NotHeld,
}

/// Manage source lock operations for a single axis (FR-135).
///
/// Wraps `AxisSourceLock` with higher-level command rejection logic
/// and SAFETY_STOP pause/resume handling.

/// Attempt to acquire the lock for a command source.
///
/// Returns `LockResult::Acquired` if successful, or `Rejected` with
/// the blocking source information.
pub fn try_acquire(
    lock: &mut AxisSourceLock,
    source: CommandSource,
    reason: LockReason,
) -> (LockResult, CommandError) {
    if lock.acquire(source, reason) {
        (LockResult::Acquired, CommandError::empty())
    } else {
        (
            LockResult::Rejected {
                held_by: lock.locked_source,
                reason: lock.lock_reason,
            },
            CommandError::SOURCE_LOCKED,
        )
    }
}

/// Release the lock for a command source.
pub fn try_release(lock: &mut AxisSourceLock, source: CommandSource) -> LockResult {
    if lock.release(source) {
        LockResult::Released
    } else if lock.is_locked() {
        LockResult::Rejected {
            held_by: lock.locked_source,
            reason: lock.lock_reason,
        }
    } else {
        LockResult::NotHeld
    }
}

/// Check if a source can command this axis.
///
/// Returns `Ok(())` or a `CommandError` indicating rejection.
#[inline]
pub fn check_authority(
    lock: &AxisSourceLock,
    source: CommandSource,
) -> Result<(), CommandError> {
    if lock.can_command(source) {
        Ok(())
    } else {
        Err(CommandError::SOURCE_LOCKED)
    }
}

/// Pause motion targets on SAFETY_STOP (FR-136).
///
/// Preserves the current target position, velocity, and operational mode
/// so they can be restored after recovery. Source lock is NOT released.
pub fn pause_for_safety(
    lock: &mut AxisSourceLock,
    target_position: f64,
    target_velocity: f64,
    operational_mode: OperationalMode,
) {
    lock.pre_pause_targets = Some(PauseTargets {
        target_position,
        target_velocity,
        operational_mode,
    });
}

/// Resume from SAFETY_STOP pause (FR-136).
///
/// Returns the preserved targets if they exist, or `None` if there
/// was no paused state.
pub fn resume_from_safety(lock: &mut AxisSourceLock) -> Option<PauseTargets> {
    lock.pre_pause_targets.take()
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_and_release() {
        let mut lock = AxisSourceLock::default();
        let (r, e) = try_acquire(&mut lock, CommandSource::RecipeExecutor, LockReason::RecipeRunning);
        assert_eq!(r, LockResult::Acquired);
        assert!(e.is_empty());
        assert!(lock.is_locked());

        let r = try_release(&mut lock, CommandSource::RecipeExecutor);
        assert_eq!(r, LockResult::Released);
        assert!(!lock.is_locked());
    }

    #[test]
    fn reject_different_source() {
        let mut lock = AxisSourceLock::default();
        try_acquire(&mut lock, CommandSource::RecipeExecutor, LockReason::RecipeRunning);

        let (r, e) = try_acquire(&mut lock, CommandSource::GrpcApi, LockReason::ManualControl);
        assert!(matches!(r, LockResult::Rejected { held_by: CommandSource::RecipeExecutor, .. }));
        assert!(e.contains(CommandError::SOURCE_LOCKED));
    }

    #[test]
    fn check_authority_ok() {
        let mut lock = AxisSourceLock::default();
        try_acquire(&mut lock, CommandSource::GrpcApi, LockReason::ManualControl);

        assert!(check_authority(&lock, CommandSource::GrpcApi).is_ok());
        assert!(check_authority(&lock, CommandSource::RecipeExecutor).is_err());
    }

    #[test]
    fn safety_pause_preserves_targets() {
        let mut lock = AxisSourceLock::default();
        try_acquire(&mut lock, CommandSource::RecipeExecutor, LockReason::RecipeRunning);

        pause_for_safety(&mut lock, 100.0, 50.0, OperationalMode::Position);
        assert!(lock.pre_pause_targets.is_some());
        assert!(lock.is_locked()); // Lock NOT released by safety (FR-137)

        let targets = resume_from_safety(&mut lock).unwrap();
        assert_eq!(targets.target_position, 100.0);
        assert_eq!(targets.target_velocity, 50.0);
        assert_eq!(targets.operational_mode, OperationalMode::Position);
        assert!(lock.pre_pause_targets.is_none()); // Consumed
    }

    #[test]
    fn release_wrong_source_returns_rejected() {
        let mut lock = AxisSourceLock::default();
        try_acquire(&mut lock, CommandSource::RecipeExecutor, LockReason::RecipeRunning);

        let r = try_release(&mut lock, CommandSource::GrpcApi);
        assert!(matches!(r, LockResult::Rejected { .. }));
        assert!(lock.is_locked()); // Still locked
    }

    #[test]
    fn release_unlocked_returns_not_held() {
        let mut lock = AxisSourceLock::default();
        let r = try_release(&mut lock, CommandSource::GrpcApi);
        assert_eq!(r, LockResult::NotHeld);
    }
}
