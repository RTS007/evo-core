//! I/O configuration structs (FR-148, FR-153, FR-154).
//!
//! Deserialized from `io.toml` at startup. Each group contains
//! an array of I/O points with type-specific fields.

use serde::{Deserialize, Serialize};

use super::role::{DiLogic, IoPointType};

// ─── Analog Scaling Curve ───────────────────────────────────────────

/// Scaling curve for analog I/O.
///
/// `f(n) = a·n³ + b·n² + c·n + offset`
/// where `n = (raw - min) / (max - min)` (normalized 0.0–1.0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnalogCurve {
    /// Named preset: `"linear"`, `"quadratic"`, `"cubic"`.
    Preset(CurvePreset),
    /// Custom polynomial coefficients `[a, b, c]`.
    Custom([f64; 3]),
}

impl Default for AnalogCurve {
    fn default() -> Self {
        Self::Preset(CurvePreset::Linear)
    }
}

impl AnalogCurve {
    /// Evaluate the curve for a normalized input `n` in `[0.0, 1.0]`.
    pub fn evaluate(&self, n: f64) -> f64 {
        let (a, b, c) = self.coefficients();
        a * n * n * n + b * n * n + c * n
    }

    /// Extract polynomial coefficients `(a, b, c)`.
    pub fn coefficients(&self) -> (f64, f64, f64) {
        match self {
            Self::Preset(p) => p.coefficients(),
            Self::Custom([a, b, c]) => (*a, *b, *c),
        }
    }
}

/// Named curve presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CurvePreset {
    Linear,
    Quadratic,
    Cubic,
}

impl CurvePreset {
    pub fn coefficients(self) -> (f64, f64, f64) {
        match self {
            Self::Linear => (0.0, 0.0, 1.0),
            Self::Quadratic => (0.0, 1.0, 0.0),
            Self::Cubic => (1.0, 0.0, 0.0),
        }
    }
}

// ─── IoPoint ────────────────────────────────────────────────────────

/// A single I/O point definition from `io.toml`.
///
/// Type-specific fields use `Option` / `#[serde(default)]` — irrelevant
/// fields for a given `io_type` are ignored at parse time and validated
/// at registry construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoPoint {
    /// I/O type discriminator.
    #[serde(rename = "type")]
    pub io_type: IoPointType,

    /// Physical pin number.
    pub pin: u16,

    /// Functional role string (parsed into `IoRole` at registry construction).
    #[serde(default)]
    pub role: Option<String>,

    /// Human-readable display name.
    #[serde(default)]
    pub name: Option<String>,

    // ── DI-specific ─────────────────────────────────────────────────

    /// NO (Normally Open) or NC (Normally Closed). Default: NO.
    #[serde(default)]
    pub logic: Option<DiLogic>,

    /// Debounce filter time [ms]. Default: 15.
    #[serde(default)]
    pub debounce: Option<u16>,

    /// Conditional enable — second DI pin that must be active.
    #[serde(default)]
    pub enable_pin: Option<u16>,

    /// Required state of `enable_pin`. Default: true.
    #[serde(default)]
    pub enable_state: Option<bool>,

    /// Max time between signals [ms] for two-hand operation (0 = none).
    #[serde(default)]
    pub enable_timeout: Option<u32>,

    // ── DO-specific ─────────────────────────────────────────────────

    /// Initial logical state (before inversion). Default: false.
    #[serde(default)]
    pub init: Option<bool>,

    /// Invert logic-to-pin mapping. Default: false.
    #[serde(default)]
    pub inverted: Option<bool>,

    /// Watchdog pulse ms — auto-OFF without refresh (0 = none).
    #[serde(default)]
    pub pulse: Option<u32>,

    /// Do NOT reset on E-Stop. Default: false.
    #[serde(default)]
    pub keep_estop: Option<bool>,

    // ── AI/AO-specific ──────────────────────────────────────────────

    /// Engineering range minimum. Default: 0.0.
    #[serde(default)]
    pub min: Option<f64>,

    /// Engineering range maximum. Required for AI/AO.
    #[serde(default)]
    pub max: Option<f64>,

    /// Unit of measure. Default: "V".
    #[serde(default)]
    pub unit: Option<String>,

    /// Moving average sample count (AI only, 1–1000). Default: 5.
    #[serde(default)]
    pub average: Option<u16>,

    /// Scaling curve (AI/AO). Default: "linear".
    #[serde(default)]
    pub curve: Option<AnalogCurve>,

    /// Output offset added after curve scaling.
    #[serde(default)]
    pub offset: Option<f64>,

    // ── Simulation ──────────────────────────────────────────────────

    /// Simulation value (bool for DI, f64 for AI — stored as f64;
    /// DI: 0.0 = false, nonzero = true).
    #[serde(default)]
    pub sim: Option<f64>,
}

// ─── IoGroup ────────────────────────────────────────────────────────

/// A named group of I/O points from `io.toml`.
///
/// Each TOML table key becomes the `key` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoGroup {
    /// Group display name.
    #[serde(default)]
    pub name: Option<String>,

    /// I/O points in this group.
    pub io: Vec<IoPoint>,
}

// ─── IoConfig ───────────────────────────────────────────────────────

/// Top-level I/O configuration (FR-148).
///
/// Parsed from `io.toml`. The TOML file is a map of group keys to
/// `IoGroup` structs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoConfig {
    /// Ordered list of groups with their keys.
    #[serde(flatten)]
    pub groups: std::collections::BTreeMap<String, IoGroup>,
}

impl IoConfig {
    /// Parse from TOML string.
    pub fn from_toml(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    /// Iterate all I/O points with their group key.
    pub fn all_points(&self) -> impl Iterator<Item = (&str, usize, &IoPoint)> {
        self.groups.iter().flat_map(|(key, group)| {
            group
                .io
                .iter()
                .enumerate()
                .map(move |(idx, point)| (key.as_str(), idx, point))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_io_toml() {
        let toml_str = r#"
[Safety]
name = "Safety circuits"
io = [
    { type = "di", role = "EStop", pin = 1, logic = "NC", name = "Main E-Stop" },
]

[Axes]
name = "Limit switches"
io = [
    { type = "di", role = "LimitMin1", pin = 30, logic = "NC", name = "Limit switch 1-" },
    { type = "di", role = "LimitMax1", pin = 31, logic = "NC", name = "Limit switch 1+" },
]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        assert_eq!(config.groups.len(), 2);
        assert!(config.groups.contains_key("Safety"));
        assert!(config.groups.contains_key("Axes"));
        assert_eq!(config.groups["Safety"].io.len(), 1);
        assert_eq!(config.groups["Axes"].io.len(), 2);
    }

    #[test]
    fn parse_analog_points() {
        let toml_str = r#"
[Pneumatics]
name = "Pneumatics"
io = [
    { type = "ai", pin = 64, max = 10.0, unit = "bar", average = 10, name = "Pressure value" },
    { type = "ao", pin = 100, min = 0.0, max = 5.0, name = "Valve output" },
]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let pneumatics = &config.groups["Pneumatics"];
        assert_eq!(pneumatics.io.len(), 2);
        assert_eq!(pneumatics.io[0].io_type, IoPointType::Ai);
        assert_eq!(pneumatics.io[0].max, Some(10.0));
        assert_eq!(pneumatics.io[0].average, Some(10));
        assert_eq!(pneumatics.io[1].io_type, IoPointType::Ao);
    }

    #[test]
    fn parse_do_with_options() {
        let toml_str = r#"
[Outputs]
io = [
    { type = "do", pin = 200, init = true, inverted = true, keep_estop = true, pulse = 500, name = "Safety relay" },
]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let point = &config.groups["Outputs"].io[0];
        assert_eq!(point.io_type, IoPointType::Do);
        assert_eq!(point.init, Some(true));
        assert_eq!(point.inverted, Some(true));
        assert_eq!(point.keep_estop, Some(true));
        assert_eq!(point.pulse, Some(500));
    }

    #[test]
    fn analog_curve_evaluate() {
        let linear = AnalogCurve::default();
        assert!((linear.evaluate(0.5) - 0.5).abs() < 1e-10);
        assert!((linear.evaluate(1.0) - 1.0).abs() < 1e-10);

        let quadratic = AnalogCurve::Preset(CurvePreset::Quadratic);
        assert!((quadratic.evaluate(0.5) - 0.25).abs() < 1e-10);

        let cubic = AnalogCurve::Preset(CurvePreset::Cubic);
        assert!((cubic.evaluate(0.5) - 0.125).abs() < 1e-10);

        let custom = AnalogCurve::Custom([0.2, 0.0, 0.8]);
        // f(0.5) = 0.2*0.125 + 0.0*0.25 + 0.8*0.5 = 0.025 + 0.4 = 0.425
        assert!((custom.evaluate(0.5) - 0.425).abs() < 1e-10);
    }

    #[test]
    fn all_points_iterator() {
        let toml_str = r#"
[A]
io = [
    { type = "di", pin = 1 },
    { type = "di", pin = 2 },
]
[B]
io = [
    { type = "do", pin = 100 },
]
"#;
        let config = IoConfig::from_toml(toml_str).unwrap();
        let points: Vec<_> = config.all_points().collect();
        assert_eq!(points.len(), 3);
    }
}
