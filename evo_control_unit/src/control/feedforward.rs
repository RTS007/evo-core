//! Feedforward controller.
//!
//! Velocity FF (Kvff × target_velocity), acceleration FF (Kaff × target_acceleration),
//! static friction compensation (Friction × sign(velocity)).
//! Zero gains disable each component.

/// Feedforward gains — extracted from `UniversalControlParameters`.
#[derive(Debug, Clone, Copy)]
pub struct FeedforwardGains {
    /// Velocity feedforward gain (0 = disabled).
    pub kvff: f64,
    /// Acceleration feedforward gain (0 = disabled).
    pub kaff: f64,
    /// Static friction offset [Nm] (0 = disabled).
    pub friction: f64,
}

/// Compute feedforward torque contribution.
///
/// ```text
/// ff = Kvff × target_velocity + Kaff × target_acceleration + Friction × sign(target_velocity)
/// ```
///
/// Each term is independently disabled when its gain is zero.
///
/// # Arguments
/// - `gains`: Feedforward gains for this axis.
/// - `target_velocity`: Commanded velocity [mm/s].
/// - `target_acceleration`: Commanded acceleration [mm/s²].
///
/// # Returns
/// Total feedforward output [Nm].
#[inline]
pub fn feedforward_compute(
    gains: &FeedforwardGains,
    target_velocity: f64,
    target_acceleration: f64,
) -> f64 {
    let mut output = 0.0;

    // Velocity feedforward
    if gains.kvff != 0.0 {
        output += gains.kvff * target_velocity;
    }

    // Acceleration feedforward
    if gains.kaff != 0.0 {
        output += gains.kaff * target_acceleration;
    }

    // Static friction compensation
    if gains.friction != 0.0 && target_velocity != 0.0 {
        output += gains.friction * target_velocity.signum();
    }

    output
}

/// Compute torque-offset component (FF-only, no PID).
///
/// This is stored separately in `ControlOutputVector::torque_offset`
/// for drives that support feedforward injection (FR-132a).
///
/// # Returns
/// Acceleration FF + DOB contribution [Nm].
#[inline]
pub fn torque_offset_compute(kaff: f64, target_acceleration: f64, dob_output: f64) -> f64 {
    let mut offset = 0.0;
    if kaff != 0.0 {
        offset += kaff * target_acceleration;
    }
    offset += dob_output;
    offset
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_gains() -> FeedforwardGains {
        FeedforwardGains {
            kvff: 0.0,
            kaff: 0.0,
            friction: 0.0,
        }
    }

    #[test]
    fn zero_gains_produce_zero() {
        let g = zero_gains();
        let out = feedforward_compute(&g, 100.0, 50.0);
        assert!((out).abs() < 1e-12);
    }

    #[test]
    fn velocity_ff_only() {
        let g = FeedforwardGains {
            kvff: 0.5,
            kaff: 0.0,
            friction: 0.0,
        };
        let out = feedforward_compute(&g, 200.0, 0.0);
        assert!((out - 100.0).abs() < 1e-12);
    }

    #[test]
    fn acceleration_ff_only() {
        let g = FeedforwardGains {
            kvff: 0.0,
            kaff: 0.01,
            friction: 0.0,
        };
        let out = feedforward_compute(&g, 0.0, 1000.0);
        assert!((out - 10.0).abs() < 1e-12);
    }

    #[test]
    fn friction_positive_velocity() {
        let g = FeedforwardGains {
            kvff: 0.0,
            kaff: 0.0,
            friction: 2.0,
        };
        let out = feedforward_compute(&g, 50.0, 0.0);
        assert!((out - 2.0).abs() < 1e-12);
    }

    #[test]
    fn friction_negative_velocity() {
        let g = FeedforwardGains {
            kvff: 0.0,
            kaff: 0.0,
            friction: 2.0,
        };
        let out = feedforward_compute(&g, -50.0, 0.0);
        assert!((out - (-2.0)).abs() < 1e-12);
    }

    #[test]
    fn friction_zero_velocity_disabled() {
        let g = FeedforwardGains {
            kvff: 0.0,
            kaff: 0.0,
            friction: 2.0,
        };
        let out = feedforward_compute(&g, 0.0, 0.0);
        assert!((out).abs() < 1e-12);
    }

    #[test]
    fn combined_ff() {
        let g = FeedforwardGains {
            kvff: 0.5,
            kaff: 0.01,
            friction: 1.0,
        };
        // ff = 0.5*100 + 0.01*500 + 1.0*sign(100) = 50 + 5 + 1 = 56
        let out = feedforward_compute(&g, 100.0, 500.0);
        assert!((out - 56.0).abs() < 1e-12);
    }

    #[test]
    fn torque_offset_kaff_and_dob() {
        let off = torque_offset_compute(0.01, 1000.0, 5.0);
        // 0.01 * 1000 + 5.0 = 15.0
        assert!((off - 15.0).abs() < 1e-12);
    }

    #[test]
    fn torque_offset_zero_kaff() {
        let off = torque_offset_compute(0.0, 1000.0, 3.0);
        assert!((off - 3.0).abs() < 1e-12);
    }
}
