//! State persistence for simulation driver.
//!
//! This module handles saving and loading axis state across restarts.
//! State is persisted using bincode for efficient binary serialization.

use evo_common::hal::config::ReferencingRequired;
use evo_common::hal::driver::HalError;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::Path;
use tracing::{debug, info, warn};

/// Persisted state for a single axis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistedAxisState {
    /// Axis name (for matching on load)
    pub name: String,
    /// Last known position
    pub position: f64,
    /// Whether axis was referenced
    pub referenced: bool,
    /// Last known error code (0 = no error)
    pub error_code: u16,
}

impl Default for PersistedAxisState {
    fn default() -> Self {
        Self {
            name: String::new(),
            position: 0.0,
            referenced: false,
            error_code: 0,
        }
    }
}

/// Persisted state for the entire driver.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PersistedState {
    /// Version of state format (for migration)
    pub version: u32,
    /// Axis states
    pub axes: Vec<PersistedAxisState>,
    /// Timestamp of last save (Unix epoch seconds)
    pub saved_at: u64,
}

impl PersistedState {
    /// Current state format version.
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new persisted state.
    pub fn new() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            axes: Vec::new(),
            saved_at: 0,
        }
    }
}

/// State persistence manager.
pub struct StatePersistence {
    /// Path to state file
    path: std::path::PathBuf,
}

impl StatePersistence {
    /// Create a new persistence manager.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Save state to file.
    pub fn save(&self, state: &PersistedState) -> Result<(), HalError> {
        debug!("Saving state to {:?}", self.path);

        // Create parent directories if needed
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                HalError::PersistenceError(format!("Failed to create directory: {}", e))
            })?;
        }

        // Write state file with timestamp
        let mut state = state.clone();
        state.saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let file = File::create(&self.path).map_err(|e| {
            HalError::PersistenceError(format!("Failed to create state file: {}", e))
        })?;

        let writer = BufWriter::new(file);
        bincode::serialize_into(writer, &state).map_err(|e| {
            HalError::PersistenceError(format!("Failed to serialize state: {}", e))
        })?;

        info!(
            "Saved state for {} axes to {:?}",
            state.axes.len(),
            self.path
        );
        Ok(())
    }

    /// Load state from file.
    pub fn load(&self) -> Result<Option<PersistedState>, HalError> {
        debug!("Loading state from {:?}", self.path);

        if !self.path.exists() {
            debug!("State file does not exist, starting fresh");
            return Ok(None);
        }

        let file = File::open(&self.path).map_err(|e| {
            HalError::PersistenceError(format!("Failed to open state file: {}", e))
        })?;

        let reader = BufReader::new(file);
        let state: PersistedState = bincode::deserialize_from(reader).map_err(|e| {
            warn!("Failed to deserialize state file, starting fresh: {}", e);
            HalError::PersistenceError(format!("Failed to deserialize state: {}", e))
        })?;

        // Check version compatibility
        if state.version != PersistedState::CURRENT_VERSION {
            warn!(
                "State file version {} differs from current {}, starting fresh",
                state.version,
                PersistedState::CURRENT_VERSION
            );
            return Ok(None);
        }

        info!(
            "Loaded state for {} axes from {:?} (saved at {})",
            state.axes.len(),
            self.path,
            state.saved_at
        );
        Ok(Some(state))
    }

    /// Delete state file.
    pub fn delete(&self) -> Result<(), HalError> {
        if self.path.exists() {
            fs::remove_file(&self.path).map_err(|e| {
                HalError::PersistenceError(format!("Failed to delete state file: {}", e))
            })?;
            info!("Deleted state file {:?}", self.path);
        }
        Ok(())
    }
}

/// Determine if axis needs referencing based on config and persisted state.
///
/// # Rules:
/// - `ReferencingRequired::Yes` - Always need referencing
/// - `ReferencingRequired::No` - Never need referencing
/// - `ReferencingRequired::Perhaps` - Use persisted state if available and referenced
pub fn needs_referencing(
    required: ReferencingRequired,
    persisted_referenced: Option<bool>,
) -> bool {
    match required {
        ReferencingRequired::Yes => true,
        ReferencingRequired::No => false,
        ReferencingRequired::Perhaps => {
            // If we have persisted state showing referenced, don't need referencing
            !persisted_referenced.unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_persisted_axis_state_default() {
        let state = PersistedAxisState::default();
        assert_eq!(state.name, "");
        assert_eq!(state.position, 0.0);
        assert!(!state.referenced);
        assert_eq!(state.error_code, 0);
    }

    #[test]
    fn test_persisted_state_new() {
        let state = PersistedState::new();
        assert_eq!(state.version, PersistedState::CURRENT_VERSION);
        assert!(state.axes.is_empty());
        assert_eq!(state.saved_at, 0);
    }

    #[test]
    fn test_state_serialization() {
        let state = PersistedState {
            version: 1,
            axes: vec![
                PersistedAxisState {
                    name: "axis_1".to_string(),
                    position: 100.5,
                    referenced: true,
                    error_code: 0,
                },
                PersistedAxisState {
                    name: "axis_2".to_string(),
                    position: -50.0,
                    referenced: false,
                    error_code: 1,
                },
            ],
            saved_at: 1234567890,
        };

        // Serialize
        let bytes = bincode::serialize(&state).unwrap();

        // Deserialize
        let loaded: PersistedState = bincode::deserialize(&bytes).unwrap();

        assert_eq!(state, loaded);
    }

    #[test]
    fn test_persistence_save_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.bin");
        let persistence = StatePersistence::new(&path);

        let state = PersistedState {
            version: PersistedState::CURRENT_VERSION,
            axes: vec![PersistedAxisState {
                name: "test_axis".to_string(),
                position: 42.0,
                referenced: true,
                error_code: 0,
            }],
            saved_at: 0,
        };

        // Save
        persistence.save(&state).unwrap();
        assert!(path.exists());

        // Load
        let loaded = persistence.load().unwrap().unwrap();
        assert_eq!(loaded.version, state.version);
        assert_eq!(loaded.axes.len(), 1);
        assert_eq!(loaded.axes[0].name, "test_axis");
        assert_eq!(loaded.axes[0].position, 42.0);
        assert!(loaded.axes[0].referenced);
        assert!(loaded.saved_at > 0); // Timestamp was set

        // Delete
        persistence.delete().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_persistence_load_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.bin");
        let persistence = StatePersistence::new(&path);

        let result = persistence.load().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_needs_referencing_yes() {
        assert!(needs_referencing(ReferencingRequired::Yes, None));
        assert!(needs_referencing(ReferencingRequired::Yes, Some(false)));
        assert!(needs_referencing(ReferencingRequired::Yes, Some(true)));
    }

    #[test]
    fn test_needs_referencing_no() {
        assert!(!needs_referencing(ReferencingRequired::No, None));
        assert!(!needs_referencing(ReferencingRequired::No, Some(false)));
        assert!(!needs_referencing(ReferencingRequired::No, Some(true)));
    }

    #[test]
    fn test_needs_referencing_perhaps() {
        // No persisted state - need referencing
        assert!(needs_referencing(ReferencingRequired::Perhaps, None));
        // Was not referenced - need referencing
        assert!(needs_referencing(ReferencingRequired::Perhaps, Some(false)));
        // Was referenced - don't need referencing
        assert!(!needs_referencing(ReferencingRequired::Perhaps, Some(true)));
    }
}
