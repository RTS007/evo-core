//! Control types for the Control Unit (FR-100, FR-105).
//!
//! Defines `ControlOutputVector` and `UniversalControlParameters`.

use serde::{Deserialize, Serialize};
use static_assertions::const_assert_eq;

use super::state::LagPolicy;

/// Control output vector — 4 × f64 = 32 bytes (FR-105).
///
/// All 4 fields are always calculated. HAL selects which field to use
/// based on drive operational mode (FR-132a).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ControlOutputVector {
    /// Total PID + FF + DOB output [Nm].
    pub calculated_torque: f64,
    /// Velocity command [mm/s].
    pub target_velocity: f64,
    /// Position command [mm].
    pub target_position: f64,
    /// Feedforward-only component [Nm].
    pub torque_offset: f64,
}

const_assert_eq!(core::mem::size_of::<ControlOutputVector>(), 32);

impl Default for ControlOutputVector {
    fn default() -> Self {
        Self {
            calculated_torque: 0.0,
            target_velocity: 0.0,
            target_position: 0.0,
            torque_offset: 0.0,
        }
    }
}

impl ControlOutputVector {
    /// Zero all outputs.
    #[inline]
    pub fn zero(&mut self) {
        self.calculated_torque = 0.0;
        self.target_velocity = 0.0;
        self.target_position = 0.0;
        self.torque_offset = 0.0;
    }

    /// Returns true if all fields are finite (not NaN, not Inf).
    #[inline]
    pub fn is_finite(&self) -> bool {
        self.calculated_torque.is_finite()
            && self.target_velocity.is_finite()
            && self.target_position.is_finite()
            && self.torque_offset.is_finite()
    }
}

/// Universal control parameters for the position control engine (FR-100).
///
/// Each component is activated/deactivated by setting its gain parameter
/// to zero. Zero gain = component disabled.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct UniversalControlParameters {
    // ─── PID ────────────────────────────────────────
    /// Proportional gain.
    #[serde(default)]
    pub kp: f64,
    /// Integral gain (0 = disabled).
    #[serde(default)]
    pub ki: f64,
    /// Derivative gain (0 = disabled).
    #[serde(default)]
    pub kd: f64,
    /// Derivative filter time constant [s].
    #[serde(default)]
    pub tf: f64,
    /// Anti-windup tracking time constant [s].
    #[serde(default)]
    pub tt: f64,

    // ─── Feedforward ────────────────────────────────
    /// Velocity feedforward gain (0 = disabled).
    #[serde(default)]
    pub kvff: f64,
    /// Acceleration feedforward gain (0 = disabled).
    #[serde(default)]
    pub kaff: f64,
    /// Static friction offset (0 = disabled).
    #[serde(default)]
    pub friction: f64,

    // ─── DOB ────────────────────────────────────────
    /// Nominal inertia [kg·m²].
    #[serde(default)]
    pub jn: f64,
    /// Nominal damping [N·s/m].
    #[serde(default)]
    pub bn: f64,
    /// Observer bandwidth [rad/s] (0 = disabled).
    #[serde(default)]
    pub gdob: f64,

    // ─── Filters ────────────────────────────────────
    /// Notch filter frequency [Hz] (0 = disabled).
    #[serde(default)]
    pub f_notch: f64,
    /// Notch filter bandwidth [Hz].
    #[serde(default)]
    pub bw_notch: f64,
    /// Low-pass cutoff frequency [Hz] (0 = disabled).
    #[serde(default)]
    pub flp: f64,
    /// Output saturation limit [Nm].
    #[serde(default = "default_out_max")]
    pub out_max: f64,

    // ─── Lag Monitoring ─────────────────────────────
    /// Maximum allowed lag error [mm].
    #[serde(default = "default_lag_error_limit")]
    pub lag_error_limit: f64,
    /// Behavior when lag_error_limit exceeded.
    #[serde(default)]
    pub lag_policy: LagPolicy,
}

fn default_out_max() -> f64 {
    100.0
}

fn default_lag_error_limit() -> f64 {
    1.0
}

impl Default for UniversalControlParameters {
    fn default() -> Self {
        Self {
            kp: 0.0,
            ki: 0.0,
            kd: 0.0,
            tf: 0.0,
            tt: 0.0,
            kvff: 0.0,
            kaff: 0.0,
            friction: 0.0,
            jn: 0.0,
            bn: 0.0,
            gdob: 0.0,
            f_notch: 0.0,
            bw_notch: 0.0,
            flp: 0.0,
            out_max: 100.0,
            lag_error_limit: 1.0,
            lag_policy: LagPolicy::Unwanted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_output_vector_size() {
        assert_eq!(core::mem::size_of::<ControlOutputVector>(), 32);
    }

    #[test]
    fn control_output_vector_zero() {
        let mut v = ControlOutputVector {
            calculated_torque: 10.0,
            target_velocity: 20.0,
            target_position: 30.0,
            torque_offset: 5.0,
        };
        v.zero();
        assert_eq!(v.calculated_torque, 0.0);
        assert_eq!(v.target_velocity, 0.0);
        assert_eq!(v.target_position, 0.0);
        assert_eq!(v.torque_offset, 0.0);
    }

    #[test]
    fn control_output_vector_is_finite() {
        let v = ControlOutputVector::default();
        assert!(v.is_finite());

        let nan_v = ControlOutputVector {
            calculated_torque: f64::NAN,
            ..Default::default()
        };
        assert!(!nan_v.is_finite());

        let inf_v = ControlOutputVector {
            target_velocity: f64::INFINITY,
            ..Default::default()
        };
        assert!(!inf_v.is_finite());
    }
}
