//! Signal conditioning filters.
//!
//! Biquad notch filter (fNotch, BWnotch) and 1st-order low-pass filter (flp).
//! Zero frequency disables each filter.
//!
//! The notch filter eliminates mechanical resonance from the control signal.
//! The low-pass filter provides general smoothing of the output.
//! Processing order: notch → low-pass (per FR-102).

use core::f64::consts::PI;

// ─── Notch Filter (2nd-order biquad) ────────────────────────────────

/// Biquad notch filter coefficients.
///
/// Transfer function (z-domain) for a notch at frequency `f0` with bandwidth `bw`:
/// ```text
/// H(z) = (b0 + b1·z⁻¹ + b2·z⁻²) / (1 + a1·z⁻¹ + a2·z⁻²)
/// ```
#[derive(Debug, Clone, Copy)]
pub struct NotchCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// Compute biquad notch filter coefficients.
///
/// Returns `None` if `f_notch <= 0.0` (disabled) or `sample_rate <= 0.0`.
pub fn notch_coefficients(f_notch: f64, bw_notch: f64, sample_rate: f64) -> Option<NotchCoeffs> {
    if f_notch <= 0.0 || sample_rate <= 0.0 {
        return None;
    }

    let bw = if bw_notch <= 0.0 { f_notch * 0.1 } else { bw_notch };
    let omega0 = 2.0 * PI * f_notch / sample_rate;
    let cos_w0 = omega0.cos();
    let alpha = omega0.sin() * (bw * PI / sample_rate).tanh();

    // Avoid division by zero
    let a0 = 1.0 + alpha;
    if a0.abs() < 1e-15 {
        return None;
    }

    Some(NotchCoeffs {
        b0: 1.0 / a0,
        b1: -2.0 * cos_w0 / a0,
        b2: 1.0 / a0,
        a1: -2.0 * cos_w0 / a0,
        a2: (1.0 - alpha) / a0,
    })
}

/// Internal state of the biquad notch filter (Direct Form I).
#[derive(Debug, Clone, Copy, Default)]
pub struct NotchState {
    x1: f64, // x[n-1]
    x2: f64, // x[n-2]
    y1: f64, // y[n-1]
    y2: f64, // y[n-2]
}

impl NotchState {
    /// Reset filter state to zero.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Apply one sample through the biquad notch filter.
#[inline]
pub fn notch_apply(state: &mut NotchState, coeffs: &NotchCoeffs, input: f64) -> f64 {
    let output = coeffs.b0 * input + coeffs.b1 * state.x1 + coeffs.b2 * state.x2
        - coeffs.a1 * state.y1
        - coeffs.a2 * state.y2;

    state.x2 = state.x1;
    state.x1 = input;
    state.y2 = state.y1;
    state.y1 = output;

    output
}

// ─── Low-Pass Filter (1st-order) ────────────────────────────────────

/// Internal state of the 1st-order low-pass filter.
#[derive(Debug, Clone, Copy, Default)]
pub struct LowPassState {
    /// Previous output.
    prev_output: f64,
}

impl LowPassState {
    /// Reset filter state to zero.
    #[inline]
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Apply one sample through the 1st-order low-pass filter.
///
/// ```text
/// alpha = 2π·flp·dt / (1 + 2π·flp·dt)
/// y[n] = y[n-1] + alpha × (x[n] - y[n-1])
/// ```
///
/// Returns `input` unchanged when `flp <= 0.0` (disabled).
#[inline]
pub fn lowpass_apply(state: &mut LowPassState, flp: f64, input: f64, dt: f64) -> f64 {
    if flp <= 0.0 || dt <= 0.0 {
        return input;
    }

    let omega = 2.0 * PI * flp * dt;
    let alpha = omega / (1.0 + omega);
    let output = state.prev_output + alpha * (input - state.prev_output);
    state.prev_output = output;
    output
}

// ─── Combined filter chain state ────────────────────────────────────

/// Combined state for the signal conditioning filter chain (notch + low-pass).
#[derive(Debug, Clone, Copy, Default)]
pub struct FilterChainState {
    /// Biquad notch filter state.
    pub notch: NotchState,
    /// Pre-computed notch coefficients (None = disabled).
    notch_coeffs: Option<NotchCoeffsStored>,
    /// 1st-order low-pass state.
    pub lowpass: LowPassState,
}

/// Stored notch coefficients (wrapped to be Copy-able in Option).
#[derive(Debug, Clone, Copy)]
struct NotchCoeffsStored(NotchCoeffs);

impl FilterChainState {
    /// Initialize the filter chain with the given parameters.
    ///
    /// Call this once at startup or when parameters change.
    pub fn init(&mut self, f_notch: f64, bw_notch: f64, flp: f64, sample_rate: f64) {
        let _ = flp; // flp is used per-sample, not precomputed
        self.notch_coeffs = notch_coefficients(f_notch, bw_notch, sample_rate)
            .map(NotchCoeffsStored);
        self.notch.reset();
        self.lowpass.reset();
    }

    /// Reset all filter state to zero (preserves coefficients).
    #[inline]
    pub fn reset(&mut self) {
        self.notch.reset();
        self.lowpass.reset();
    }

    /// Apply the full filter chain: notch → low-pass.
    ///
    /// Disabled filters pass signal through unchanged.
    #[inline]
    pub fn apply(&mut self, input: f64, flp: f64, dt: f64) -> f64 {
        // Notch filter
        let after_notch = match &self.notch_coeffs {
            Some(c) => notch_apply(&mut self.notch, &c.0, input),
            None => input,
        };

        // Low-pass filter
        lowpass_apply(&mut self.lowpass, flp, after_notch, dt)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f64 = 1000.0; // 1 kHz
    const DT: f64 = 1.0 / SAMPLE_RATE;

    #[test]
    fn notch_disabled_when_freq_zero() {
        assert!(notch_coefficients(0.0, 10.0, SAMPLE_RATE).is_none());
    }

    #[test]
    fn notch_disabled_when_freq_negative() {
        assert!(notch_coefficients(-100.0, 10.0, SAMPLE_RATE).is_none());
    }

    #[test]
    fn notch_coefficients_valid() {
        let c = notch_coefficients(100.0, 20.0, SAMPLE_RATE).unwrap();
        // b0 and b2 should be equal for a symmetric notch
        assert!((c.b0 - c.b2).abs() < 1e-10);
    }

    #[test]
    fn notch_attenuates_resonance_frequency() {
        let f0 = 50.0; // notch at 50 Hz
        let c = notch_coefficients(f0, 10.0, SAMPLE_RATE).unwrap();
        let mut state = NotchState::default();

        // Feed a sine wave at the notch frequency for many cycles
        let mut max_output = 0.0_f64;
        for i in 0..2000 {
            let t = i as f64 * DT;
            let input = (2.0 * PI * f0 * t).sin();
            let output = notch_apply(&mut state, &c, input);
            if i > 500 {
                // skip transient
                max_output = max_output.max(output.abs());
            }
        }
        // Output at notch frequency should be significantly attenuated
        assert!(
            max_output < 0.1,
            "notch should attenuate f0: max={}",
            max_output
        );
    }

    #[test]
    fn notch_passes_other_frequencies() {
        let f0 = 200.0;
        let c = notch_coefficients(f0, 20.0, SAMPLE_RATE).unwrap();
        let mut state = NotchState::default();

        // Feed a sine at a different frequency (50 Hz)
        let f_test = 50.0;
        let mut max_output = 0.0_f64;
        for i in 0..2000 {
            let t = i as f64 * DT;
            let input = (2.0 * PI * f_test * t).sin();
            let output = notch_apply(&mut state, &c, input);
            if i > 500 {
                max_output = max_output.max(output.abs());
            }
        }
        // Should pass through mostly unchanged
        assert!(
            max_output > 0.8,
            "notch should pass other freqs: max={}",
            max_output
        );
    }

    #[test]
    fn lowpass_disabled_when_freq_zero() {
        let mut s = LowPassState::default();
        let out = lowpass_apply(&mut s, 0.0, 42.0, DT);
        assert!((out - 42.0).abs() < 1e-12);
    }

    #[test]
    fn lowpass_smooths_step_input() {
        let mut s = LowPassState::default();
        let flp = 10.0; // 10 Hz cutoff
        // Apply step input: 0→1
        let first = lowpass_apply(&mut s, flp, 1.0, DT);
        // First output should be less than 1.0 (smoothed)
        assert!(first < 1.0);
        assert!(first > 0.0);
        // After many samples, should converge to 1.0
        for _ in 0..10000 {
            lowpass_apply(&mut s, flp, 1.0, DT);
        }
        assert!((s.prev_output - 1.0).abs() < 0.001);
    }

    #[test]
    fn filter_chain_both_disabled() {
        let mut chain = FilterChainState::default();
        chain.init(0.0, 0.0, 0.0, SAMPLE_RATE);
        let out = chain.apply(42.0, 0.0, DT);
        assert!((out - 42.0).abs() < 1e-12);
    }

    #[test]
    fn filter_chain_reset() {
        let mut chain = FilterChainState::default();
        chain.init(100.0, 20.0, 50.0, SAMPLE_RATE);
        // Feed some data
        for _ in 0..100 {
            chain.apply(10.0, 50.0, DT);
        }
        chain.reset();
        assert_eq!(chain.notch.x1, 0.0);
        assert_eq!(chain.lowpass.prev_output, 0.0);
    }
}
