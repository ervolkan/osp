//! Koordinat sistemi — Raw vs Derived (OSP-formalism.md §2.1, inv #4).
//!
//! **Faz 1.4:** `RawPosition` / `DerivedPosition` / `Position` ayrımı.
//! θ hesabı SADECE `RawPosition`'ı okur; `DerivedPosition` (u, θ, D) asla θ girdisi
//! olamaz → dairesellik **yapısal garanti** (compile-time, runtime-check değil).

use crate::space::{Node, Space};

// ═══════════════════════════════════════════════════════════════════════════════
// RawPosition — 5 bağımsız eksen, θ'nın GİRDİSİ (inv #4)
// ═══════════════════════════════════════════════════════════════════════════════

/// 5 bağımsız (raw) eksen. θ sapma hesabının girdisi.
///
/// Eksenler:
/// - `x` coupling (Faz 1.3 ✓)
/// - `y` cohesion — LCOM4 (Faz 1.9)
/// - `z` instability — Martin `I` saf (inv #10, Faz 1.9)
/// - `w` entropy (Faz 1.3 ✓)
/// - `v` witness-depth (Faz 1.3 ✓)
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct RawPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
    pub v: f64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// DerivedPosition — Raw + θ'dan türetilmiş, θ'nın ÇIKTISI (inv #4)
// ═══════════════════════════════════════════════════════════════════════════════

/// Raw pozisyon + Vizyon'dan türetilmiş metrikler. θ hesabına **girdi olamaz**.
///
/// - `u` vision alignment = `1 − θ_norm`
/// - `theta` sapma açısı (raw'dan `DeviationMetric::theta` ile, §5)
/// - `risk_score` composite risk (Faz 2)
/// - `main_sequence_distance` `D = |A + I − 1|` (Martin, inv #10 — ayrı metric, z'ye gömülü DEĞİL)
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct DerivedPosition {
    pub u: f64,
    pub theta: f64,
    pub risk_score: f64,
    pub main_sequence_distance: f64,
}

/// Tam konum: raw + derived. `Node.position`'ın tipi.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Position {
    pub raw: RawPosition,
    pub derived: DerivedPosition,
}

// ═══════════════════════════════════════════════════════════════════════════════
// MetricValue + MetricSource (canonical — scip-analyzer-design.md §6.1)
// ═══════════════════════════════════════════════════════════════════════════════

/// Custom axis değeri için provenance modeli (scip-analyzer-design.md §6.1, agent-prompt-semantics.md §2.2).
///
/// `confidence = source_base × coverage × stale_penalty`.
/// Core axis'ler şu an plain `f64` (deterministik, implicit full-confidence).
/// Custom axis'ler `MetricValue` kullanır (Faz 5+ — security/wcag/performance vb.).
/// Analyzer (osp-analyzer) bu tipti üretir (tree-sitter/SCIP); re-export ile kullanır.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MetricValue {
    /// Metric değeri (NaN/Inf yasak — §12 Analysis Quality Rules #7).
    pub value: f64,
    /// Değerin kaynağı.
    pub source: MetricSource,
    /// [0,1] — `source_base × coverage × stale_penalty`.
    pub confidence: f64,
    /// [0,1] — SCIP coverage ratio veya tree-sitter parse coverage.
    pub coverage: f64,
}

/// Metric'in kaynağı (provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MetricSource {
    /// Tier 1 syntactic (tree-sitter).
    TreeSitter,
    /// Tier 2 semantic (SCIP index).
    Scip,
    /// Veri yok — placeholder.
    Placeholder,
    /// Yaklaşık hesap (ör. proxy formula).
    Heuristic,
}

impl std::fmt::Display for MetricSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TreeSitter => write!(f, "tree-sitter"),
            Self::Scip => write!(f, "scip"),
            Self::Placeholder => write!(f, "placeholder"),
            Self::Heuristic => write!(f, "heuristic"),
        }
    }
}

impl MetricValue {
    /// Placeholder: veri yok, confidence=0.0.
    pub fn placeholder(value: f64) -> Self {
        Self {
            value,
            source: MetricSource::Placeholder,
            confidence: 0.0,
            coverage: 0.0,
        }
    }

    /// Tree-sitter: confidence = 0.75 × coverage.
    /// Coverage < 1.0 olabilir (parse error, unsupported extension, exclude).
    pub fn tree_sitter(value: f64, coverage: f64) -> Self {
        Self {
            value,
            source: MetricSource::TreeSitter,
            confidence: 0.75 * coverage,
            coverage,
        }
    }

    /// SCIP: confidence = 0.95 × coverage × stale_penalty.
    /// `coverage` = `SemanticCoverage.coverage_ratio` ile aynı.
    pub fn scip(value: f64, coverage: f64, stale: bool) -> Self {
        let stale_penalty = if stale { 0.5 } else { 1.0 };
        Self {
            value,
            source: MetricSource::Scip,
            confidence: 0.95 * coverage * stale_penalty,
            coverage,
        }
    }

    /// Heuristic: approximate confidence.
    pub fn heuristic(value: f64, confidence: f64) -> Self {
        Self {
            value,
            source: MetricSource::Heuristic,
            confidence,
            coverage: 1.0,
        }
    }

    /// §12 #7 — finite invariant: value finite, confidence ∈ [0,1], coverage ∈ [0,1].
    pub fn validate(&self) -> Result<(), MetricValueError> {
        if !self.value.is_finite() {
            return Err(MetricValueError::NonFiniteValue);
        }
        if !(0.0..=1.0).contains(&self.confidence) {
            return Err(MetricValueError::ConfidenceOutOfRange(self.confidence));
        }
        if !(0.0..=1.0).contains(&self.coverage) {
            return Err(MetricValueError::CoverageOutOfRange(self.coverage));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MetricValueError {
    #[error("MetricValue.value NaN/Inf")]
    NonFiniteValue,
    #[error("MetricValue.confidence out of range [0,1]: {0}")]
    ConfidenceOutOfRange(f64),
    #[error("MetricValue.coverage out of range [0,1]: {0}")]
    CoverageOutOfRange(f64),
}

/// Custom axis tanımlayıcısı — `"security.audit"`, `"wcag.compliance"` (formalism §2.2).
pub type AxisId = String;

/// Custom raw axis değerleri (Faz 5 stub).
///
/// **Şu an kullanılmıyor** — `RawPosition` flat kalır (5 core f64).
/// Faz 5'te `RawPosition { core: CoreRawPosition, custom: CustomRawPosition }` split
/// yapıldığında bu tip `HashMap<AxisId, MetricValue>` içerecek (formalism §2.2, §2.4).
/// Şimdi sadece tip tanımı mevcut — downstream tipler/impl Faz 5'te gelir.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CustomRawPosition {
    // Faz 5: pub values: HashMap<AxisId, MetricValue>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Axis trait + CoordinateSystem (pluggable, §2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek bir koordinat eksenini temsil eden trait.
///
/// Domain'e özel eksenler (security, accessibility) bu trait'i implement ederek
/// `CoordinateSystem`'e eklenebilir. `compute` dönüşü **[0,1]** aralığında normalize.
pub trait Axis: Send + Sync {
    /// Eksen adı — `raw_position_of` isme göre mapler (sıra değil).
    /// Standart adlar: `"coupling"`, `"cohesion"`, `"instability"`, `"entropy"`, `"witness_depth"`.
    fn name(&self) -> &'static str;

    /// Düğümün bu eksenindeki değerini `[0,1]` aralığında hesapla.
    fn compute(&self, node: &Node, space: &Space) -> f64;
}

/// Koordinat sistemi — eksen koleksiyonu (`Vec<Box<dyn Axis>>`).
pub struct CoordinateSystem {
    pub axes: Vec<Box<dyn Axis>>,
}

impl CoordinateSystem {
    pub fn empty() -> Self {
        Self { axes: vec![] }
    }

    /// Eksen sayısı.
    pub fn dim(&self) -> usize {
        self.axes.len()
    }

    /// Eksen adları (raporlama/debug).
    pub fn axis_names(&self) -> Vec<&'static str> {
        self.axes.iter().map(|a| a.name()).collect()
    }

    /// Generic: tüm eksen değerleri `Vec<f64>` olarak (eksen sırasına göre).
    /// Custom axis kombinasyonları için. OSP preset için `raw_position_of` tercih edilir.
    pub fn position_of(&self, node: &Node, space: &Space) -> Vec<f64> {
        self.axes.iter().map(|a| a.compute(node, space)).collect()
    }

    /// Typed: `RawPosition` — eksen **ADINA** göre mapler (sıra değil).
    ///
    /// Faz 1.4 preset (coupling, entropy, witness_depth) → `x, w, v` dolu; `y, z = 0.0`.
    /// Faz 1.9'da cohesion + instability eklenince `y, z` de dolar.
    /// Bilinmeyen adlar (custom axes) yok sayılır — `RawPosition`'a dahil edilmez.
    pub fn raw_position_of(&self, node: &Node, space: &Space) -> RawPosition {
        let mut pos = RawPosition::default();
        for axis in &self.axes {
            let v = axis.compute(node, space);
            match axis.name() {
                "coupling" => pos.x = v,
                "cohesion" => pos.y = v,
                "instability" => pos.z = v,
                "entropy" => pos.w = v,
                "witness_depth" => pos.v = v,
                _ => {} // custom axis — RawPosition'a dahil değil
            }
        }
        pos
    }

    /// Builder: yeni eksen ekle.
    pub fn with_axis<A: Axis + 'static>(mut self, axis: A) -> Self {
        self.axes.push(Box::new(axis));
        self
    }
}

impl Default for CoordinateSystem {
    fn default() -> Self {
        Self::empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::NodeKind;

    /// Test amaçlı sabit-değer eksen.
    struct ConstantAxis {
        name: &'static str,
        value: f64,
    }

    impl Axis for ConstantAxis {
        fn name(&self) -> &'static str {
            self.name
        }
        fn compute(&self, _node: &Node, _space: &Space) -> f64 {
            self.value
        }
    }

    fn node(id: u64) -> Node {
        Node {
            id,
            ..Default::default()
        }
    }

    // --- RawPosition / DerivedPosition / Position tipleri ---

    #[test]
    fn raw_position_default_is_all_zero() {
        let r = RawPosition::default();
        assert!(r.x.abs() < 1e-9);
        assert!(r.y.abs() < 1e-9);
        assert!(r.z.abs() < 1e-9);
        assert!(r.w.abs() < 1e-9);
        assert!(r.v.abs() < 1e-9);
    }

    #[test]
    fn derived_position_default_is_all_zero() {
        let d = DerivedPosition::default();
        assert!(d.u.abs() < 1e-9);
        assert!(d.theta.abs() < 1e-9);
        assert!(d.risk_score.abs() < 1e-9);
        assert!(d.main_sequence_distance.abs() < 1e-9);
    }

    #[test]
    fn derived_has_four_fields_main_sequence_distance_present() {
        // inv #10 — D ayrı field (z'ye gömülü değil)
        let d = DerivedPosition {
            main_sequence_distance: 0.42,
            ..Default::default()
        };
        assert!((d.main_sequence_distance - 0.42).abs() < 1e-9);
    }

    #[test]
    fn position_has_raw_and_derived_components() {
        let p = Position {
            raw: RawPosition {
                x: 0.1,
                w: 0.5,
                ..Default::default()
            },
            derived: DerivedPosition {
                u: 0.9,
                ..Default::default()
            },
        };
        assert!((p.raw.x - 0.1).abs() < 1e-9);
        assert!((p.raw.w - 0.5).abs() < 1e-9);
        assert!((p.derived.u - 0.9).abs() < 1e-9);
    }

    #[test]
    fn node_default_position_is_position_struct() {
        // Node.position artık Vec<f64> değil, Position struct
        let n = node(1);
        assert_eq!(n.position.raw, RawPosition::default());
        assert_eq!(n.position.derived, DerivedPosition::default());
        assert_eq!(n.kind, NodeKind::Module);
    }

    // --- CoordinateSystem ---

    #[test]
    fn empty_system_has_zero_dim() {
        let cs = CoordinateSystem::empty();
        assert_eq!(cs.dim(), 0);
        assert!(cs.axis_names().is_empty());
    }

    #[test]
    fn position_of_collects_all_axes_in_order() {
        let cs = CoordinateSystem::empty()
            .with_axis(ConstantAxis {
                name: "a",
                value: 0.1,
            })
            .with_axis(ConstantAxis {
                name: "b",
                value: 0.2,
            })
            .with_axis(ConstantAxis {
                name: "c",
                value: 0.3,
            });
        assert_eq!(cs.dim(), 3);
        assert_eq!(cs.axis_names(), vec!["a", "b", "c"]);

        let space = Space::new();
        let pos = cs.position_of(&node(1), &space);
        assert_eq!(pos, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn raw_position_of_maps_by_axis_name_not_order() {
        // Eksenler "yanlış" sırada ama doğru isimle → RawPosition doğru field'a gider
        let cs = CoordinateSystem::empty()
            .with_axis(ConstantAxis {
                name: "entropy",
                value: 0.5,
            }) // → w
            .with_axis(ConstantAxis {
                name: "coupling",
                value: 0.7,
            }) // → x
            .with_axis(ConstantAxis {
                name: "witness_depth",
                value: 0.3,
            }); // → v
        let space = Space::new();
        let raw = cs.raw_position_of(&node(1), &space);
        assert!((raw.x - 0.7).abs() < 1e-9, "x (coupling) = {}", raw.x);
        assert!((raw.w - 0.5).abs() < 1e-9, "w (entropy) = {}", raw.w);
        assert!((raw.v - 0.3).abs() < 1e-9, "v (witness_depth) = {}", raw.v);
        // y, z preset'te yok → 0.0
        assert!(raw.y.abs() < 1e-9, "y boş kalmalı");
        assert!(raw.z.abs() < 1e-9, "z boş kalmalı");
    }

    #[test]
    fn raw_position_of_ignores_unknown_axis_names() {
        // Custom axis "security" RawPosition'a dahil edilmez (5 standart dışı)
        let cs = CoordinateSystem::empty().with_axis(ConstantAxis {
            name: "security",
            value: 0.99,
        });
        let space = Space::new();
        let raw = cs.raw_position_of(&node(1), &space);
        // security yok sayıldı, tüm standart eksenler 0.0
        assert!(raw.x.abs() < 1e-9);
        assert!(raw.y.abs() < 1e-9);
    }

    #[test]
    fn axis_compute_receives_node_and_space() {
        struct NodeCountAxis;
        impl Axis for NodeCountAxis {
            fn name(&self) -> &'static str {
                "node_count_norm"
            }
            fn compute(&self, _node: &Node, space: &Space) -> f64 {
                (space.node_count() as f64 / 100.0).min(1.0)
            }
        }

        let mut space = Space::new();
        for i in 0..50 {
            space.insert_node(node(i));
        }
        let cs = CoordinateSystem::empty().with_axis(NodeCountAxis);
        let pos = cs.position_of(&node(0), &space);
        assert!((pos[0] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn builder_chain_compiles_and_works() {
        let cs = CoordinateSystem::empty()
            .with_axis(ConstantAxis {
                name: "a",
                value: 0.0,
            })
            .with_axis(ConstantAxis {
                name: "b",
                value: 1.0,
            });
        assert_eq!(cs.dim(), 2);
    }
}
