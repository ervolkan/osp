//! Vision config — TOML → `VisionVector` + engine policies (§5.1).
//!
//! **Faz 2.2:** elle-deklare vision parse. Validation (colleague #1): raw değerler [0,1].
//! Defaults (colleague #2): policies/thresholds/diagnostics opsiyonel. Error (colleague #3):
//! `VisionConfigError` — osp-core config-layer error tipi.

use std::path::Path;

use crate::coords::RawPosition;
use crate::space::NodeRole;
use crate::vision::VisionVector;

// ═══════════════════════════════════════════════════════════════════════════════
// VisionConfigError (colleague #3 — config-layer error)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum VisionConfigError {
    #[error("vision config dosya okuma hatası: {0}")]
    Io(#[from] std::io::Error),
    #[error("vision config TOML parse hatası: {0}")]
    Toml(#[from] toml::de::Error),
    /// colleague #1 — raw değer [0,1] dışında.
    #[error("vision raw değer aralık dışı: {field}={value} (beklenen [0.0, 1.0])")]
    OutOfRange { field: &'static str, value: f64 },
}

// ═══════════════════════════════════════════════════════════════════════════════
// Default value functions (serde `default = "..."`)
// ═══════════════════════════════════════════════════════════════════════════════

fn default_min_approvers() -> usize {
    2
}
fn default_quorum_threshold() -> f64 {
    1.5
}
fn default_merge_ratio_observable() -> f64 {
    0.10 // Faz 1.11 kalibrasyon korpusu ile doğrulandı
}
fn default_theta_bound() -> f64 {
    0.3 // cosine deviation [0,1] değerlerde θ_max=0.5; 0.3 realistic (OSP-formalism.md §5.2 NOT)
}
fn default_milestone_interval() -> u64 {
    1000
}
fn default_abstractness() -> f64 {
    0.5 // Faz 3 SCIP placeholder
}

// ═══════════════════════════════════════════════════════════════════════════════
// VisionConfig — TOML deserialize (3 bölüm: raw zorunlu, diğerleri opsiyonel)
// ═══════════════════════════════════════════════════════════════════════════════

/// TOML vision deklarasyonu. `osp-vision.toml` → bu struct'a deserialize.
///
/// Sadece `[raw]` zorunlu; `[policies]` / `[thresholds]` / `[diagnostics]`
/// opsiyonel — sensible defaults (colleague #2).
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct VisionConfig {
    pub raw: VisionRawConfig,
    #[serde(default)]
    pub policies: VisionPoliciesConfig,
    #[serde(default)]
    pub thresholds: VisionThresholdsConfig,
    #[serde(default)]
    pub diagnostics: VisionDiagnosticsConfig,
    /// Role-aware vision overrides: her mimari rol için ayrı (x,y,z) hedefi.
    /// Olmayan roller global `[raw]` default'a düşer. Değerlendirmenin
    /// "3114 false-reject" sorununu çözer — TypeSurface için coupling düşük vb.
    #[serde(default)]
    pub role_overrides: std::collections::HashMap<String, RoleVisionOverride>,
}

/// `[raw]` — vizyon pozisyonu (zorunlu). Değerler [0,1] validate edilir.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct VisionRawConfig {
    pub x: f64, // coupling
    pub y: f64, // cohesion
    pub z: f64, // instability (Martin I saf)
    pub w: f64, // entropy
    pub v: f64, // witness-depth
}

/// `[role_overrides.<Role>]` — belirli bir mimari rol için vision override.
///
/// Sadece x/y/z (coupling/cohesion/instability) override edilir; w/v global'den
/// inherit edilir. Olmayan alanlar global `[raw]` default'a düşer.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct RoleVisionOverride {
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub z: Option<f64>,
}

/// `[policies]` — şahitlik politikaları (opsiyonel, default'lu).
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct VisionPoliciesConfig {
    #[serde(default = "default_min_approvers")]
    pub min_approvers: usize,
    #[serde(default = "default_quorum_threshold")]
    pub quorum_threshold: f64,
    #[serde(default = "default_merge_ratio_observable")]
    pub merge_ratio_observable: f64,
}

/// `[thresholds]` — engine eşikleri (opsiyonel, default'lu).
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct VisionThresholdsConfig {
    #[serde(default = "default_theta_bound")]
    pub theta_bound: f64,
    #[serde(default = "default_milestone_interval")]
    pub milestone_interval: u64,
}

/// `[diagnostics]` — Faz 3 placeholder'ları (opsiyonel, default'lu).
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
pub struct VisionDiagnosticsConfig {
    #[serde(default = "default_abstractness")]
    pub abstractness: f64,
}

// Default impl'leri (serde(default) için + programatik kullanım)
impl Default for VisionPoliciesConfig {
    fn default() -> Self {
        Self {
            min_approvers: default_min_approvers(),
            quorum_threshold: default_quorum_threshold(),
            merge_ratio_observable: default_merge_ratio_observable(),
        }
    }
}

impl Default for VisionThresholdsConfig {
    fn default() -> Self {
        Self {
            theta_bound: default_theta_bound(),
            milestone_interval: default_milestone_interval(),
        }
    }
}

impl Default for VisionDiagnosticsConfig {
    fn default() -> Self {
        Self {
            abstractness: default_abstractness(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VisionConfig impl — load + validate + convert
// ═══════════════════════════════════════════════════════════════════════════════

impl VisionConfig {
    /// TOML dosyası yükle + parse + validate.
    pub fn load(path: &Path) -> Result<Self, VisionConfigError> {
        let text = std::fs::read_to_string(path)?;
        Self::from_str(&text)
    }

    /// TOML string'den parse + validate (testler için).
    pub fn from_str(toml_str: &str) -> Result<Self, VisionConfigError> {
        let config: Self = toml::from_str(toml_str)?;
        config.validate()?;
        Ok(config)
    }

    /// Validation (colleague #1): raw değerler [0,1].
    /// Elle-deklare vision'un kalite güvencesi — x=1.5 gibi hataları sessizce kabul etmez.
    fn validate(&self) -> Result<(), VisionConfigError> {
        for (field, value) in [
            ("x", self.raw.x),
            ("y", self.raw.y),
            ("z", self.raw.z),
            ("w", self.raw.w),
            ("v", self.raw.v),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(VisionConfigError::OutOfRange { field, value });
            }
        }
        Ok(())
    }

    /// → `VisionVector` (raw position wrap).
    pub fn to_vision_vector(&self) -> VisionVector {
        VisionVector::new(RawPosition {
            x: self.raw.x,
            y: self.raw.y,
            z: self.raw.z,
            w: self.raw.w,
            v: self.raw.v,
        })
    }

    /// → role-specific `VisionVector`. Override varsa x/y/z onu kullanır,
    /// yoksa (veya alan None ise) global `[raw]` default. w/v her zaman global.
    ///
    /// Bu, değerlendirmenin "her rol için ayrı vision vector" önerisini formal
    /// model seviyesinde uygular. Örn: TypeSurface için coupling=0.05 (declaration
    /// dosyasında runtime deps beklenmez), Core için instability=0.20 (stable).
    pub fn role_vision(&self, role: NodeRole) -> VisionVector {
        let key = format!("{:?}", role);
        let global = self.to_vision_vector();
        match self.role_overrides.get(&key) {
            Some(ovr) => VisionVector::new(RawPosition {
                x: ovr.x.unwrap_or(self.raw.x),
                y: ovr.y.unwrap_or(self.raw.y),
                z: ovr.z.unwrap_or(self.raw.z),
                w: self.raw.w,
                v: self.raw.v,
            }),
            None => global,
        }
    }

    /// Bir role için override tanımlı mı?
    pub fn has_role_override(&self, role: NodeRole) -> bool {
        self.role_overrides.contains_key(&format!("{:?}", role))
    }

    /// Bir role için builtin (hardcoded) sensible-default vision override.
    ///
    /// Kullanıcı TOML'da `[role_overrides.<Role>]` tanımlamasa bile role-aware
    /// vision çalışsın diye. Değerlendirmenin "Runtime/Core için instability
    /// target 0.50 olmamalı" tespitini adresler.
    ///
    /// Default'lar mimari normlardan türetilmiştir:
    /// - TypeSurface (.d.ts): coupling düşük (declaration = runtime deps yok)
    /// - Core: instability düşük (stabil foundation), cohesion yüksek
    /// - Adapter: instability yüksek olabilir (integration boundary)
    /// - Utility: coupling düşük (leaf helper)
    /// - Runtime: global vision'a yakın ama instability biraz düşük (0.35)
    /// - Support (test/migration): instability yüksek doğal, coupling gevşek
    pub fn builtin_role_override(role: NodeRole) -> Option<RoleVisionOverride> {
        use NodeRole as R;
        let ovr = |x: f64, y: f64, z: f64| RoleVisionOverride {
            x: Some(x), y: Some(y), z: Some(z),
        };
        match role {
            R::TypeSurface => Some(ovr(0.05, 0.80, 0.50)), // coupling relaxed
            R::Core => Some(ovr(0.60, 0.75, 0.20)),        // instability low (stabil)
            R::Adapter => Some(ovr(0.80, 0.50, 0.80)),     // instability tolerated
            R::Utility => Some(ovr(0.20, 0.60, 0.50)),     // coupling low
            R::Runtime => Some(ovr(0.40, 0.60, 0.35)),     // instability biraz düşük
            R::Support => None, // Support advisory mode'da, vision relaxation UI'da
        }
    }

    /// EngineConfig henüz tanımlı değil (Faz 2.5 `engine.rs`).
    /// Faz 2.5'te `to_engine_config(&self) -> EngineConfig` eklenecek.
    /// Şimdilik policy/threshold değerlerine direct accessor:
    pub fn min_approvers(&self) -> usize {
        self.policies.min_approvers
    }
    pub fn quorum_threshold(&self) -> f64 {
        self.policies.quorum_threshold
    }
    pub fn merge_ratio_observable(&self) -> f64 {
        self.policies.merge_ratio_observable
    }
    pub fn theta_bound(&self) -> f64 {
        self.thresholds.theta_bound
    }
    pub fn milestone_interval(&self) -> u64 {
        self.thresholds.milestone_interval
    }
    pub fn abstractness(&self) -> f64 {
        self.diagnostics.abstractness
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_CONFIG: &str = r#"
[raw]
x = 0.4
y = 0.7
z = 0.5
w = 0.5
v = 0.5

[policies]
min_approvers = 3
quorum_threshold = 2.0
merge_ratio_observable = 0.15

[thresholds]
theta_bound = 0.6
milestone_interval = 500

[diagnostics]
abstractness = 0.3
"#;

    const MINIMAL_CONFIG: &str = r#"
[raw]
x = 0.4
y = 0.7
z = 0.5
w = 0.5
v = 0.5
"#;

    // --- parse ---

    #[test]
    fn parse_full_config_all_sections() {
        let c = VisionConfig::from_str(FULL_CONFIG).expect("full config parse");
        assert!((c.raw.x - 0.4).abs() < 1e-9);
        assert_eq!(c.policies.min_approvers, 3);
        assert!((c.policies.quorum_threshold - 2.0).abs() < 1e-9);
        assert!((c.policies.merge_ratio_observable - 0.15).abs() < 1e-9);
        assert!((c.thresholds.theta_bound - 0.6).abs() < 1e-9);
        assert_eq!(c.thresholds.milestone_interval, 500);
        assert!((c.diagnostics.abstractness - 0.3).abs() < 1e-9);
    }

    #[test]
    fn parse_minimal_config_defaults_applied() {
        // colleague #2 — sadece [raw], diğerleri default
        let c = VisionConfig::from_str(MINIMAL_CONFIG).expect("minimal config parse");
        assert!((c.raw.x - 0.4).abs() < 1e-9);
        // policies defaults
        assert_eq!(c.policies.min_approvers, 2);
        assert!((c.policies.quorum_threshold - 1.5).abs() < 1e-9);
        assert!((c.policies.merge_ratio_observable - 0.10).abs() < 1e-9);
        // thresholds defaults
        assert!((c.thresholds.theta_bound - 0.3).abs() < 1e-9);
        assert_eq!(c.thresholds.milestone_interval, 1000);
        // diagnostics defaults
        assert!((c.diagnostics.abstractness - 0.5).abs() < 1e-9);
    }

    #[test]
    fn parse_partial_policies_keeps_defaults() {
        // sadece min_approvers belirtilmiş, diğerleri default
        let toml = r#"
[raw]
x = 0.5
y = 0.5
z = 0.5
w = 0.5
v = 0.5

[policies]
min_approvers = 5
"#;
        let c = VisionConfig::from_str(toml).expect("partial policies");
        assert_eq!(c.policies.min_approvers, 5);
        assert!((c.policies.quorum_threshold - 1.5).abs() < 1e-9); // default
        assert!((c.policies.merge_ratio_observable - 0.10).abs() < 1e-9); // default
    }

    // --- validation (colleague #1) ---

    #[test]
    fn validation_rejects_raw_above_one() {
        let toml = r#"
[raw]
x = 1.5  # out of range!
y = 0.5
z = 0.5
w = 0.5
v = 0.5
"#;
        let result = VisionConfig::from_str(toml);
        assert!(matches!(
            result,
            Err(VisionConfigError::OutOfRange { field: "x", value: 1.5 })
        ));
    }

    #[test]
    fn validation_rejects_raw_below_zero() {
        let toml = r#"
[raw]
x = 0.5
y = -0.1  # negative!
z = 0.5
w = 0.5
v = 0.5
"#;
        let result = VisionConfig::from_str(toml);
        assert!(matches!(
            result,
            Err(VisionConfigError::OutOfRange { field: "y", value }) if (value - (-0.1)).abs() < 1e-9
        ));
    }

    #[test]
    fn validation_accepts_boundary_values() {
        let toml = r#"
[raw]
x = 0.0
y = 1.0
z = 0.0
w = 1.0
v = 0.0
"#;
        let c = VisionConfig::from_str(toml).expect("boundary values geçerli");
        assert!((c.raw.x).abs() < 1e-9);
        assert!((c.raw.y - 1.0).abs() < 1e-9);
    }

    // --- TOML parse error ---

    #[test]
    fn malformed_toml_returns_toml_error() {
        let bad = "this is not [valid toml";
        let result = VisionConfig::from_str(bad);
        assert!(matches!(result, Err(VisionConfigError::Toml(_))));
    }

    #[test]
    fn missing_raw_section_returns_toml_error() {
        let no_raw = r#"
[policies]
min_approvers = 2
"#;
        let result = VisionConfig::from_str(no_raw);
        assert!(result.is_err()); // [raw] zorunlu → deserialize error
    }

    // --- to_vision_vector ---

    #[test]
    fn to_vision_vector_preserves_raw_values() {
        let c = VisionConfig::from_str(FULL_CONFIG).unwrap();
        let vv = c.to_vision_vector();
        let raw = vv.raw();
        assert!((raw.x - 0.4).abs() < 1e-9);
        assert!((raw.y - 0.7).abs() < 1e-9);
        assert!((raw.z - 0.5).abs() < 1e-9);
        assert!((raw.w - 0.5).abs() < 1e-9);
        assert!((raw.v - 0.5).abs() < 1e-9);
    }

    // --- accessors (Faz 2.5 EngineConfig için) ---

    #[test]
    fn accessors_return_correct_values() {
        let c = VisionConfig::from_str(FULL_CONFIG).unwrap();
        assert_eq!(c.min_approvers(), 3);
        assert!((c.quorum_threshold() - 2.0).abs() < 1e-9);
        assert!((c.theta_bound() - 0.6).abs() < 1e-9);
        assert_eq!(c.milestone_interval(), 500);
        assert!((c.abstractness() - 0.3).abs() < 1e-9);
    }

    #[test]
    fn merge_ratio_observable_default_matches_calibration() {
        // Faz 1.11: %10 doğrulandı — default bu değerde olmalı
        let c = VisionConfig::from_str(MINIMAL_CONFIG).unwrap();
        assert!((c.merge_ratio_observable() - 0.10).abs() < 1e-9);
    }

    // --- VisionConfigError display ---

    #[test]
    fn error_display_implements_thiserror() {
        let err = VisionConfigError::OutOfRange {
            field: "x",
            value: 1.5,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("aralık dışı"));
        assert!(msg.contains("x"));
        assert!(msg.contains("1.5"));
    }
}
