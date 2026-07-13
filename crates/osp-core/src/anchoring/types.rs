//! Concept Anchoring runtime domain tipleri (Faz 1).
//!
//! [`crate::anchoring`] kökündeki enum'ların (Faz 0) üzerinde yaşayan struct'lar.
//! Faz 1 §11: "ConceptPacket, AnchorCandidate, AnchorPlan, PositionSnapshot tipleri".

use crate::anchoring::{
    AnchorDecisionKind, ConceptEdgeKind, ConceptPacketType, DecisionStatus, PhysicalCodeMetricAxis,
    PositionFamily, ThresholdBand,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Newtype ID'ler — typed-prefix string ("Concept:Payment" vb.)
// ═══════════════════════════════════════════════════════════════════════════════

/// ConceptPacket newtype ID'si.
///
/// PR G: `Ord`/`PartialOrd` eklendi (BTreeMap key — resolved implementation grouping
/// deterministik sort için). String Ord olduğu için lexicographic sıralama güvenli.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ConceptPacketId(pub String);

/// ConceptNode newtype ID'si — typed-prefix formatı ("Concept:Payment").
///
/// PR E: `Ord`/`PartialOrd` eklendi (BTreeMap key — store-owned identity bindings deterministik
/// sort için). String Ord olduğu için lexicographic sıralama güvenli.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ConceptNodeId(pub String);

/// `ConceptPacketId::try_from_node_id` hatası — non-canonical / empty packet node ID.
///
/// PR G — canonical reverse parse. `ConceptPacketId` public tuple newtype olduğu için
/// boş değer temsil edilebilir; `try_from_node_id` non-empty contract'ı uygular.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("not a valid ConceptPacket node id: {node_id}")]
pub struct InvalidConceptPacketNodeId {
    pub node_id: ConceptNodeId,
}

impl ConceptPacketId {
    /// Single source-of-truth prefix constant (PR G — literal duplication yok).
    pub const NODE_PREFIX: &'static str = "ConceptPacket:";

    /// ConceptPacket node ID'sine çevir (graph'ta ConceptPacket node'u).
    ///
    /// PR G — store.rs'tan types.rs'e taşındı (ID formatı store davranışı değil,
    /// ID tipinin sorumluluğu).
    pub fn into_node_id(&self) -> ConceptNodeId {
        ConceptNodeId(format!("{}{}", Self::NODE_PREFIX, self.0))
    }

    /// Canonical reverse parse (PR G).
    ///
    /// `into_node_id` round-trip: non-empty packet IDs için total. Empty packet ID
    /// `into_node_id` ile `"ConceptPacket:"` üretir ama `try_from_node_id` reject eder
    /// (non-empty contract). Bu yüzden round-trip partial — empty reject edilir.
    pub fn try_from_node_id(node_id: &ConceptNodeId) -> Result<Self, InvalidConceptPacketNodeId> {
        let raw = node_id
            .0
            .strip_prefix(Self::NODE_PREFIX)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| InvalidConceptPacketNodeId {
                node_id: node_id.clone(),
            })?;
        Ok(Self(raw.to_owned()))
    }
}

/// PositionSnapshot newtype ID'si.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct PositionSnapshotId(pub String);

// ═══════════════════════════════════════════════════════════════════════════════
// NonEmptyExplanation — INV-C7 newtype (Faz 2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Boş olmayan explanation (INV-C7 — high-stake edge'lerde zorunlu).
///
/// # Yapısal garanti
/// Private inner `String` + fallible `new(...)`. Boş/whitespace string reject edilir
/// (`EmptyExplanation`). Bu, "boş explanation graph'a girer" hatasını type-level engeller:
/// `Option<NonEmptyExplanation>` ya `None`'dır ya da kesinlikle boş olmayan bir açıklama.
///
/// High-stake edge'lerde explanation *presence* hâlâ runtime (gate, `is_high_stake()`
/// runtime) — type-level olan emptiness kontrolüdür.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonEmptyExplanation(String);

impl NonEmptyExplanation {
    /// Boş/whitespace string reject. Başarılıysa `NonEmptyExplanation`.
    pub fn new(s: impl Into<String>) -> Result<Self, EmptyExplanation> {
        let s = s.into();
        if s.trim().is_empty() {
            Err(EmptyExplanation)
        } else {
            Ok(Self(s))
        }
    }

    /// TCB içi: validation'sız construct (extractor zaten non-empty üretir).
    pub(crate) fn from_validated(s: String) -> Self {
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NonEmptyExplanation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for NonEmptyExplanation {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl serde::Serialize for NonEmptyExplanation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for NonEmptyExplanation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// Boş explanation hatası.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyExplanation;

impl std::fmt::Display for EmptyExplanation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "explanation boş/whitespace olamaz (INV-C7 NonEmptyExplanation)"
        )
    }
}

impl std::error::Error for EmptyExplanation {}

// ═══════════════════════════════════════════════════════════════════════════════
// ScalarSimilarity — INV-C1 newtype (Faz 2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Skalar semantic similarity [0,1] — INV-C1: embedding vektörü DEĞİL.
///
/// # INV-C1 yapısal garanti
/// [`AnchorScorer`](crate::anchoring::scorer::AnchorScorer) embedding **vektörünü**
/// görmez; sadece bu skalar newtype'ı alır. `new()` `[0,1]` range-check yapar —
/// değer aralığı dışı → `SimilarityOutOfRange`. Bu, scorer'a "embedding benzerliği"
/// adı altında rastgele değer sızmasını type-level engeller.
///
/// Faz 7'de gerçek embedding geldiğinde `Embedding` type doldurulur (sealed mod),
/// `cosine` private kalır, scorer hala `ScalarSimilarity` görür.
///
/// # Serde boundary (hardening PR — EvidenceStrength R1 ile aynı açık)
/// `Serialize`/`Deserialize` derive **kullanılmaz** — derive, `new()` constructor'ını
/// bypass edip range-check'i deler (`serde_json::from_str("2.0")` geçerli üretir).
/// Custom `Deserialize` impl `new()` üzerinden geçer; range-dışı değer reject.
/// Bu, EvidenceStrength (INV-C6) ile aynı serde hijyeni standardını INV-C1'e taşır.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalarSimilarity(f64);

impl ScalarSimilarity {
    /// `[0,1]` range-check. Dışarıda → `SimilarityOutOfRange`.
    pub fn new(value: f64) -> Result<Self, SimilarityOutOfRange> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(SimilarityOutOfRange { value })
        }
    }

    /// TCB içi: validation'sız construct (Faz 7 embedding cosine sonucu için hazır API).
    #[allow(dead_code)] // Faz 7 placeholder — scorer şu an zero() kullanır
    pub(crate) fn from_validated(value: f64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0.0)
    }
    pub fn one() -> Self {
        Self(1.0)
    }
    pub fn get(&self) -> f64 {
        self.0
    }
}

/// Custom Serialize — inner f64'yi transparent serialize (round-trip uyumlu).
impl serde::Serialize for ScalarSimilarity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

/// Custom Deserialize — `new()` üzerinden range-check (serde hijyeni, EvidenceStrength R1 ile aynı).
/// `serde_json::from_str("2.0")` / `"-1.0"` / `"NaN"` reject edilir; constructor bypass
/// edilemez.
impl<'de> serde::Deserialize<'de> for ScalarSimilarity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        ScalarSimilarity::new(value).map_err(serde::de::Error::custom)
    }
}

/// `ScalarSimilarity` değer aralığı dışı hatası.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimilarityOutOfRange {
    pub value: f64,
}

impl std::fmt::Display for SimilarityOutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ScalarSimilarity [0,1] dışı: {} (INV-C1)", self.value)
    }
}

impl std::error::Error for SimilarityOutOfRange {}

// ═══════════════════════════════════════════════════════════════════════════════
// ObservedCodeMetricSource + EvidenceStrength + ObservedCodeEvidence — INV-C6 (Faz 4)
// ═══════════════════════════════════════════════════════════════════════════════
//
// INV-C6 (§7.2/§9, düzeltilmiş): kod metric'leri = **Observed** (Paper 1 ontolojisi);
// koddan çıkarılan *niyet* = **Inferred** (Candidate). İkisi karıştırılamaz.
//
// Faz 4 modelleme kararı (D15 — provenance yorumu): "Observed" yeni bir `DecisionStatus`
// variantı DEĞİLDİR. İki lane net ayrılır:
//
//   DecisionStatus        = graph acceptance lane (Candidate→Accepted)
//   ObservedCodeEvidence  = epistemik provenance lane (MetricSource'tan)
//
// "Observed code reality is evidence, not acceptance." — bir CodeEntity node'unun
// observed olması operator-accepted decision anlamına gelmez; Candidate kalır, observed
// olma durumu `ObservedCodeEvidence` içinde taşınır (Patch 5).

/// Observed kod kanıtının metric kaynağı (INV-C6 — type-level filtre).
///
/// Genel [`crate::coords::MetricSource`] (Paper 1'de `Placeholder`/`Heuristic` içerir)
/// yerine yalnızca gözlemlenmiş kaynakları kabul eden typed enum. Bu, "bir LLM çıktısı
/// veya insan tahmini asla kod kanıtı sayılamaz" kuralını tip sistemine taşır (Patch 2):
/// `HumanVision`/`LlmGuess`/`InferredIntent` imkansız.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ObservedCodeMetricSource {
    /// SCIP semantic index (en yüksek güven — gerçek field-access/cross-ref).
    Scip,
    /// tree-sitter syntactic extraction (imports, class defs).
    TreeSitter,
    /// Diğer static analyzer (Faz 4 stub sonrası, osp-analyzer bridge).
    StaticAnalyzer,
}

/// Observed kod kanıt gücü `[0,1]` (INV-C6 type-level, Patch 3).
///
/// `ScalarSimilarity` (INV-C1) paterninin "kanıt gücü" için uygulanması. Çıplak `f64`
/// tekrar riskini (`-1.0`, `2.0`, `NaN`) type-level engeller. [`crate::anchoring`] (scorer
/// `code_evidence_score`, gate `evidence_strength`) bu newtype'ı görür.
///
/// # Serde boundary (review patch R1)
/// `Serialize`/`Deserialize` derive **kullanılmaz** — derive, `new()` constructor'ını
/// bypass edip range-check'i deler (`serde_json::from_str("2.0")` geçerli üretir).
/// Bunun yerine **custom impl** ile Deserialize `new()` üzerinden geçer; range-dışı
/// değer reject edilir. Bu, "evidence strength serde ile forged edilemez" garantisi.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceStrength(f64);

impl EvidenceStrength {
    /// `[0,1]` range-check + finiteness. NaN, ±∞, negatif, >1 → `EvidenceStrengthOutOfRange`.
    /// Not 1: `is_finite()` öncelikli — NaN/inf range-check'i yanıltmasın.
    pub fn new(value: f64) -> Result<Self, EvidenceStrengthOutOfRange> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(EvidenceStrengthOutOfRange { value })
        }
    }

    pub fn zero() -> Self {
        Self(0.0)
    }
    pub fn one() -> Self {
        Self(1.0)
    }
    pub fn get(&self) -> f64 {
        self.0
    }
}

/// Custom Serialize — inner f64'yi transparent serialize (round-trip uyumlu).
impl serde::Serialize for EvidenceStrength {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

/// Custom Deserialize — `new()` üzerinden range-check (R1 review patch).
/// `serde_json::from_str("2.0")` / `"-1.0"` / `"NaN"` reject edilir; constructor bypass
/// edilemez.
impl<'de> serde::Deserialize<'de> for EvidenceStrength {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        EvidenceStrength::new(value).map_err(serde::de::Error::custom)
    }
}

/// `EvidenceStrength` değer aralığı dışı hatası.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceStrengthOutOfRange {
    pub value: f64,
}

impl std::fmt::Display for EvidenceStrengthOutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "EvidenceStrength [0,1] dışı veya non-finite: {} (INV-C6)",
            self.value
        )
    }
}

impl std::error::Error for EvidenceStrengthOutOfRange {}

// ═══════════════════════════════════════════════════════════════════════════════
// PR C — Axis-granular evidence: PhysicalAxisValue + EvidenceCoverage + collection
// ═══════════════════════════════════════════════════════════════════════════════
//
// INV-C6 axis-granular modeli (PR C). `ObservedCodeEvidence` artık tek bir
// `PhysicalCodeVector` + tek `confidence` taşımaz; her Paper 1 ekseni için ayrı
// observation taşır (`ObservedPhysicalMetrics`). Bu:
//
//   1. Per-axis provenance — Coupling TreeSitter, Cohesion Scip aynı evidence içinde.
//   2. Per-axis strength + coverage.
//   3. Uniform [0,1] validation — `PhysicalAxisValue::new(value)` tüm 5 eksende
//      normalize değeri range-check eder (raw entropy/witness_depth ENJEKSİYONU
//      type-level engelli).
//   4. Zero-strength reject — `ObservedPhysicalMetric::new` strength=0 reject eder.
//      "Evidence var ama strength=0" artık temsil edilemez → korunmuş kenar durum yok.
//   5. Normative `minimum_observed_strength()` policy — provider bunu kullanır.

/// Uniform `[0,1]` normalize physical axis değeri (PR C — tüm 5 eksende normalize).
///
/// `EvidenceStrength` (INV-C6) / `ScalarSimilarity` (INV-C1) / `NormalizedMetricThreshold`
/// (INV-P2) paterni: private inner f64, `is_finite() + (0.0..=1.0)`, custom serde
/// (constructor bypass engelli).
///
/// # Neden axis parametresi YOK
/// `new(value)` sadece değeri validate eder; axis context `ObservedPhysicalMetric`
/// seviyesinde `ObservedPhysicalMetricError::InvalidValue { axis, .. }` ile verilir.
/// Bu, validation logic'ini tek yerde tutar (DRY) ve error context'ini caller'da
/// zenginleştirir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicalAxisValue(f64);

impl PhysicalAxisValue {
    /// `[0,1]` range-check + finiteness. NaN, ±∞, negatif, >1 → `MetricScalarViolation`.
    pub fn new(value: f64) -> Result<Self, MetricScalarViolation> {
        if !value.is_finite() {
            return Err(MetricScalarViolation::NonFinite);
        }
        if value < 0.0 {
            return Err(MetricScalarViolation::BelowMinimum);
        }
        if value > 1.0 {
            return Err(MetricScalarViolation::AboveMaximum);
        }
        Ok(Self(value))
    }

    pub fn zero() -> Self {
        Self(0.0)
    }

    pub fn one() -> Self {
        Self(1.0)
    }

    pub fn get(&self) -> f64 {
        self.0
    }
}

impl serde::Serialize for PhysicalAxisValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for PhysicalAxisValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        PhysicalAxisValue::new(value).map_err(serde::de::Error::custom)
    }
}

/// Uniform `[0,1]` evidence coverage (PR C — per-axis coverage).
///
/// `PhysicalAxisValue` ile aynı validation paterni; semantik olarak farklı (ölçüm
/// coverage'sı, axis değeri değil) → ayrı newtype. Upstream confidence zaten
/// coverage içerir; bu değer per-axis zenginleştirme içindir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvidenceCoverage(f64);

impl EvidenceCoverage {
    /// `[0,1]` range-check + finiteness. NaN, ±∞, negatif, >1 → `MetricScalarViolation`.
    pub fn new(value: f64) -> Result<Self, MetricScalarViolation> {
        if !value.is_finite() {
            return Err(MetricScalarViolation::NonFinite);
        }
        if value < 0.0 {
            return Err(MetricScalarViolation::BelowMinimum);
        }
        if value > 1.0 {
            return Err(MetricScalarViolation::AboveMaximum);
        }
        Ok(Self(value))
    }

    pub fn zero() -> Self {
        Self(0.0)
    }

    pub fn one() -> Self {
        Self(1.0)
    }

    pub fn get(&self) -> f64 {
        self.0
    }
}

impl serde::Serialize for EvidenceCoverage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for EvidenceCoverage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        EvidenceCoverage::new(value).map_err(serde::de::Error::custom)
    }
}

/// `[0,1]` skalar newtype violation sebebi (`PhysicalAxisValue` + `EvidenceCoverage`).
///
/// Üç ayrı variant hatayı self-documenting yapar ve caller'ın axis context
/// (`InvalidValue { axis, value, violation }`) ile zenginleştirmesine olanak tanır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricScalarViolation {
    /// NaN veya ±∞.
    NonFinite,
    /// `< 0.0`.
    BelowMinimum,
    /// `> 1.0`.
    AboveMaximum,
}

impl std::fmt::Display for MetricScalarViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite => write!(f, "non-finite (NaN/±∞)"),
            Self::BelowMinimum => write!(f, "0.0 altında"),
            Self::AboveMaximum => write!(f, "1.0 üstünde"),
        }
    }
}

impl std::error::Error for MetricScalarViolation {}

// ═══════════════════════════════════════════════════════════════════════════════
// ObservedPhysicalMetric — per-axis observation (PR C)
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek bir Paper 1 ekseni için observed (ölçülmüş) değer + provenance (PR C).
///
/// # INV-C6 axis-granular yapısal garanti
/// - **Private field'lar + public smart constructor `new`**: external crate struct literal
///   construct edemez (trybuild `c6_observed_physical_metrics_literal`). İnvariant'lar
///   constructor içinde korunur.
/// - **`value: PhysicalAxisValue`** (uniform `[0,1]`): raw entropy/witness_depth enjeksiyonu
///   type-level engelli — `PhysicalAxisValue::new` range-check yapar.
/// - **`strength: EvidenceStrength` + zero-strength reject**: `strength=0` →
///   `ZeroStrength { axis }`. "Evidence var ama strength=0" temsil edilemez → gate/scorer
///   ayrımı korunur ama korunmuş kenar durum (Not 5 güçlenme) yok.
///
/// # Not 5 güçlenme
/// Önceki modelde "evidence object var, `confidence=0`" temsil edilebiliyordu ve
/// gate (object presence) / scorer (strength > 0) ayrımı bu kenar duruma dayanıyordu.
/// Zero-strength reject bunu imkansız kılar: gate object presence kontrolünü hala yapar
/// ama artık "strength=0 evidence" hiç oluşamaz.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ObservedPhysicalMetric {
    axis: PhysicalCodeMetricAxis,
    value: PhysicalAxisValue,
    source: ObservedCodeMetricSource,
    strength: EvidenceStrength,
    coverage: EvidenceCoverage,
}

impl ObservedPhysicalMetric {
    /// Public smart constructor — value `[0,1]` validation + strength > 0 reject.
    ///
    /// # Hatalar
    /// - `InvalidValue { axis, value, violation }`: `value` `[0,1]` dışı veya non-finite.
    /// - `ZeroStrength { axis }`: `strength` `EvidenceStrength::zero()` (0.0).
    ///
    /// Not: `EvidenceStrength::new(0.0).is_ok()` boundary geçerlidir (newtype aralıkta),
    /// ama `ObservedPhysicalMetric` seviyesinde 0 strength observation reject edilir.
    pub fn new(
        axis: PhysicalCodeMetricAxis,
        value: f64,
        source: ObservedCodeMetricSource,
        strength: EvidenceStrength,
        coverage: EvidenceCoverage,
    ) -> Result<Self, ObservedPhysicalMetricError> {
        let value = PhysicalAxisValue::new(value).map_err(|violation| {
            ObservedPhysicalMetricError::InvalidValue {
                axis,
                value,
                violation,
            }
        })?;
        if strength.get() == 0.0 {
            return Err(ObservedPhysicalMetricError::ZeroStrength { axis });
        }
        Ok(Self {
            axis,
            value,
            source,
            strength,
            coverage,
        })
    }

    /// Bu observation'ın ekseni.
    pub fn axis(&self) -> PhysicalCodeMetricAxis {
        self.axis
    }

    /// Normalize edilmiş axis değeri `[0,1]`.
    pub fn value(&self) -> PhysicalAxisValue {
        self.value
    }

    /// Metric kaynağı (INV-C6 — observed provenance).
    pub fn source(&self) -> ObservedCodeMetricSource {
        self.source
    }

    /// Per-axis kanıt gücü `[0,1]` (zero-strength reject ile > 0 garanti).
    pub fn strength(&self) -> EvidenceStrength {
        self.strength
    }

    /// Per-axis ölçüm coverage'sı `[0,1]`.
    pub fn coverage(&self) -> EvidenceCoverage {
        self.coverage
    }
}

/// `ObservedPhysicalMetric` constructor hatası (axis/value context).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ObservedPhysicalMetricError {
    /// `value` `[0,1]` dışı veya non-finite.
    #[error("{axis:?} axis değeri {value} geçersiz: {violation}")]
    InvalidValue {
        axis: PhysicalCodeMetricAxis,
        value: f64,
        violation: MetricScalarViolation,
    },
    /// `strength` 0.0 — zero-strength observation temsil edilemez.
    #[error("{axis:?} axis için zero-strength observation reject (Not 5 güçlenme)")]
    ZeroStrength { axis: PhysicalCodeMetricAxis },
}

// ═══════════════════════════════════════════════════════════════════════════════
// ObservedPhysicalMetrics — validated per-axis collection (PR C)
// ═══════════════════════════════════════════════════════════════════════════════

/// Non-empty, unique-axis, deterministic-order per-axis observation collection (PR C).
///
/// # INV-C6 yapısal garanti
/// - **Private `Vec` + `try_new`**: external crate literal construct edemez
///   (trybuild `c6_observed_physical_metrics_literal`). İnvariant'lar constructor içinde
///   korunur.
/// - **Non-empty**: `Empty` → error. "Hiç observation yok" collection seviyesinde engelli.
/// - **Unique axis**: `DuplicateAxis { axis }` → error. Aynı eksende iki observation yok.
/// - **Deterministic order**: `sort_order()`'a göre sıralı → `missing_axes()` ve
///   `try_to_physical_vector()` deterministik.
///
/// # Serialize-only
/// `Deserialize` YOK — `ObservedPhysicalMetric` Deserialize'a sahip değildir (field'lar
/// private + zero-strength invariant) ve collection'un da sahip olmaması gerekir (serde
/// ile bypass engeli). Diskten reconstruct edip INV-C6 boundary'yi delmek imkansız.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ObservedPhysicalMetrics {
    values: Vec<ObservedPhysicalMetric>,
}

impl ObservedPhysicalMetrics {
    /// Non-empty + unique-axis + deterministic-order validation.
    ///
    /// # Hatalar
    /// - `Empty`: boş vec.
    /// - `DuplicateAxis { axis }`: aynı eksende iki observation.
    ///
    /// Sıralama: `PhysicalCodeMetricAxis::sort_order()`'a göre (Coupling→Cohesion→
    /// Instability→Entropy→WitnessDepth). Bu, `missing_axes()` ve `try_to_physical_vector()`
    /// çıktılarını deterministik yapar.
    pub fn try_new(
        values: Vec<ObservedPhysicalMetric>,
    ) -> Result<Self, ObservedPhysicalMetricsError> {
        if values.is_empty() {
            return Err(ObservedPhysicalMetricsError::Empty);
        }
        // Duplicate axis kontrolü (sort öncesi, sıra bağımsız).
        let mut seen: Vec<PhysicalCodeMetricAxis> = Vec::with_capacity(values.len());
        for v in &values {
            if seen.contains(&v.axis) {
                return Err(ObservedPhysicalMetricsError::DuplicateAxis { axis: v.axis });
            }
            seen.push(v.axis);
        }
        // Deterministic sort by sort_order.
        let mut values = values;
        values.sort_by_key(|v| v.axis.sort_order());
        Ok(Self { values })
    }

    /// Tüm observation'lar (sort_order sıralı).
    pub fn values(&self) -> &[ObservedPhysicalMetric] {
        &self.values
    }

    /// Bu eksende observation var mı?
    pub fn contains(&self, axis: PhysicalCodeMetricAxis) -> bool {
        self.values.iter().any(|v| v.axis == axis)
    }

    /// Sort-order sıralı iterator (ExactSizeIterator — len biliniyor).
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ObservedPhysicalMetric> {
        self.values.iter()
    }

    /// Mevcut olmayan eksneler (sort_order sıralı, deterministik).
    ///
    /// Missing axes are absent, not zero-strength observations. Bu, "eksik eksen =
    /// ölçülmemiş" semantiğini korur; `try_to_physical_vector()` missing → error döner
    /// (zero-fill YOK).
    pub fn missing_axes(&self) -> Vec<PhysicalCodeMetricAxis> {
        let present: std::collections::HashSet<PhysicalCodeMetricAxis> =
            self.values.iter().map(|v| v.axis).collect();
        let all = [
            PhysicalCodeMetricAxis::Coupling,
            PhysicalCodeMetricAxis::Cohesion,
            PhysicalCodeMetricAxis::Instability,
            PhysicalCodeMetricAxis::Entropy,
            PhysicalCodeMetricAxis::WitnessDepth,
        ];
        all.into_iter()
            .filter(|a| !present.contains(a))
            .collect()
    }

    /// Normative min-over-axes strength policy (PR C — provider bunu kullanır).
    ///
    /// Mevcut eksneler arasındaki **minimum** `EvidenceStrength`. Coverage katılmaz
    /// (upstream confidence zaten coverage içerir — double-counting engeli). Missing
    /// axes are absent, not zero-strength observations (eksik eksen min'i düşürmez).
    ///
    /// Zero-strength reject guarantee'si nedeniyle dönen değer her zaman > 0.
    pub fn minimum_observed_strength(&self) -> EvidenceStrength {
        // Zero-strength reject (ObservedPhysicalMetric::new) + non-empty (try_new) →
        // min her zaman (0,1] aralığında; EvidenceStrength::new unwrap güvenli.
        let min = self
            .values
            .iter()
            .map(|v| v.strength().get())
            .fold(f64::INFINITY, f64::min);
        EvidenceStrength::new(min).expect(
            "minimum_observed_strength: zero-strength reject + non-empty invariant \
             delinmiş — min her zaman (0,1] aralığında olmalı",
        )
    }

    /// All-5-axes → `PhysicalCodeVector`; missing → error (zero-fill YOK).
    ///
    /// Missing axes deterministik `sort_order` sırasında (`missing_axes()` ile aynı).
    /// Bu, "eksik ölçüm = sıfır varsay" hatasını imkansız kılar: ya 5 eksende de
    /// ölçüm var (PhysicalCodeVector üretilebilir) ya da eksik açık belirtilir.
    pub fn try_to_physical_vector(&self) -> Result<PhysicalCodeVector, IncompletePhysicalVector> {
        let missing = self.missing_axes();
        if !missing.is_empty() {
            return Err(IncompletePhysicalVector { missing });
        }
        // 5 axes present — value'ları topla.
        let mut coupling = 0.0;
        let mut cohesion = 0.0;
        let mut instability = 0.0;
        let mut entropy = 0.0;
        let mut witness_depth = 0.0;
        for v in &self.values {
            match v.axis {
                PhysicalCodeMetricAxis::Coupling => coupling = v.value.get(),
                PhysicalCodeMetricAxis::Cohesion => cohesion = v.value.get(),
                PhysicalCodeMetricAxis::Instability => instability = v.value.get(),
                PhysicalCodeMetricAxis::Entropy => entropy = v.value.get(),
                PhysicalCodeMetricAxis::WitnessDepth => witness_depth = v.value.get(),
            }
        }
        Ok(PhysicalCodeVector::new(
            coupling,
            cohesion,
            instability,
            entropy,
            witness_depth,
        ))
    }
}

/// `ObservedPhysicalMetrics::try_new` hatası.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ObservedPhysicalMetricsError {
    /// Boş vec — en az bir observation gerekli.
    #[error("ObservedPhysicalMetrics boş olamaz (en az bir axis observation gerekli)")]
    Empty,
    /// Aynı eksende iki observation.
    #[error("{axis:?} axis duplicate — aynı eksende iki observation olamaz")]
    DuplicateAxis { axis: PhysicalCodeMetricAxis },
}

/// `try_to_physical_vector` missing-axes hatası (zero-fill YOK).
///
/// Private `missing` + accessor — `missing()` deterministik `sort_order` sıralı.
#[derive(Debug, Clone, PartialEq)]
pub struct IncompletePhysicalVector {
    missing: Vec<PhysicalCodeMetricAxis>,
}

impl IncompletePhysicalVector {
    /// Missing eksneler (sort_order sıralı, deterministik).
    pub fn missing(&self) -> &[PhysicalCodeMetricAxis] {
        &self.missing
    }
}

impl std::fmt::Display for IncompletePhysicalVector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PhysicalCodeVector incomplete — missing axes: {:?}",
            self.missing
        )
    }
}

impl std::error::Error for IncompletePhysicalVector {}

/// Bir CodeEntity için observed (ölçülmüş) kod kanıtı (INV-C6 — Patch 1/2/5, PR C).
///
/// # INV-C6 yapısal garanti
/// - **Private field'lar + public smart constructor `new`** (Patch 1): dışarıdan struct
///   literal construct edilemez (trybuild `c6_observed_evidence_literal`); ama **gelecekteki
///   osp-analyzer bridge crate'i `new(...)` çağırabilir** (`pub(crate)` DEĞİL — dış provider
///   geçerli evidence üretebilir). İnvariant'lar constructor içinde korunur.
/// - **`code_identity_key: CodeIdentityKey`** (PR F): evidence artık fiziksel code identity'ye
///   bağlanır, graph node ID'ye DEĞİL. Anti-corruption boundary: graph dünyası (`ConceptNodeId`)
///   ile identity/evidence dünyası (`CodeIdentityKey`) ayrı. [`CodeIdentityBindingLookup`] adapter
///   tek geçiş noktası. Eski `code_entity_id: ConceptNodeId` kaldırıldı.
/// - **`observations: ObservedPhysicalMetrics`** (PR C): axis-granular observation
///   collection. Her Paper 1 ekseni ayrı `ObservedPhysicalMetric` (per-axis provenance +
///   strength + coverage). Tek bir `PhysicalCodeVector` + tek `confidence` yerine.
/// - **`ObservedCodeMetricSource` typed enum** (Patch 2): `MetricSource::Placeholder`/
///   `Heuristic` imkansız — sadece gözlemlenmiş kaynaklar.
/// - **Serialize-only** (Not 3): `Deserialize` YOK — diskten evidence reconstruct edip
///   INV-C6 boundary'yi bypass etmeyi engeller (PR30 serde boundary paterni).
///
/// # Observed ≠ Accepted (Patch 5)
/// Bu evidence bir CodeEntity'nin *ölçülmüş fiziksel yapısını* taşır. Node'un graph
/// acceptance status'unu (Candidate→Accepted) değiştirmez. Operator acceptance ayrı
/// lane'dir (INV-C3).
///
/// [`CodeIdentityBindingLookup`]: crate::anchoring::code_evidence::CodeIdentityBindingLookup
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ObservedCodeEvidence {
    code_identity_key: crate::anchoring::identity::CodeIdentityKey,
    observations: ObservedPhysicalMetrics,
    /// Unix epoch saniye (chrono bağımlılığı yok — Faz 1 stratejisi).
    measured_at: u64,
}

impl ObservedCodeEvidence {
    /// Public smart constructor — invariant'lar `ObservedPhysicalMetrics::try_new`'de
    /// korunur (Patch 1 + PR C). Caller valid bir collection geçmelidir.
    ///
    /// **PR F migration:** İlk parametre artık `CodeIdentityKey` (eski `ConceptNodeId` değil).
    /// Dış crate (osp-analyzer bridge, test) geçerli observed evidence üretebilir; ama
    /// field'lar private olduğu için struct literal ile *geçersiz* evidence enjekte edemez.
    pub fn new(
        code_identity_key: crate::anchoring::identity::CodeIdentityKey,
        observations: ObservedPhysicalMetrics,
        measured_at: u64,
    ) -> Self {
        Self {
            code_identity_key,
            observations,
            measured_at,
        }
    }

    /// Bu evidence'ın bağlı olduğu code identity key (PR F — anti-corruption boundary).
    ///
    /// Graph node ID değil; fiziksel code identity. Node-facing lookup
    /// ([`CodeIdentityBindingLookup`]) `ConceptNodeId → CodeIdentityKey` resolve eder.
    ///
    /// [`CodeIdentityBindingLookup`]: crate::anchoring::code_evidence::CodeIdentityBindingLookup
    pub fn code_identity_key(&self) -> &crate::anchoring::identity::CodeIdentityKey {
        &self.code_identity_key
    }

    /// Axis-granular observation collection (PR C — per-axis provenance/strength/coverage).
    pub fn observations(&self) -> &ObservedPhysicalMetrics {
        &self.observations
    }

    /// Ölçüm zaman damgası (Unix epoch saniye).
    pub fn measured_at(&self) -> u64 {
        self.measured_at
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptNodeKind — Concept graph node türü
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept graph node türü. Fixture target prefix'leriyle uyumlu
/// (`Concept:`, `RuleCandidate:`, `RiskCandidate:`, `Decision:`,
///  `CodeEntity:`, `CodeEntityCandidate:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConceptNodeKind {
    Concept,
    RuleCandidate,
    TaskCandidate,
    RiskCandidate,
    Risk,
    Decision,
    CodeEntity,
    /// Faz 1'de gerçek code analizi yok — ExpectedImplementation hedefi.
    CodeEntityCandidate,
}

impl ConceptNodeKind {
    /// Typed-prefix string ("Concept", "CodeEntityCandidate").
    pub fn as_prefix(self) -> &'static str {
        match self {
            Self::Concept => "Concept",
            Self::RuleCandidate => "RuleCandidate",
            Self::TaskCandidate => "TaskCandidate",
            Self::RiskCandidate => "RiskCandidate",
            Self::Risk => "Risk",
            Self::Decision => "Decision",
            Self::CodeEntity => "CodeEntity",
            Self::CodeEntityCandidate => "CodeEntityCandidate",
        }
    }

    /// Prefix string → kind. Fixture/given parse için.
    pub fn from_prefix(s: &str) -> Option<Self> {
        Some(match s {
            "Concept" => Self::Concept,
            "RuleCandidate" => Self::RuleCandidate,
            "TaskCandidate" => Self::TaskCandidate,
            "RiskCandidate" => Self::RiskCandidate,
            "Risk" => Self::Risk,
            "Decision" => Self::Decision,
            "CodeEntity" => Self::CodeEntity,
            "CodeEntityCandidate" => Self::CodeEntityCandidate,
            _ => return None,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PacketSource — girdi kaynağı provenance
// ═══════════════════════════════════════════════════════════════════════════════

/// ConceptPacket kaynağı — temporal_trust_score için (§8.1, §6.2 hiyerarşi).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PacketSource {
    /// Kullanıcıdan gelen açık vizyon (§6.2 seviye 2).
    ExplicitUser,
    /// Operator kabul etti (seviye 1).
    Operator,
    /// Doküman/ADR (seviye 5 — stale olabilir).
    Document,
    /// Agent hipotezi (seviye 7 — düşük güven).
    Agent,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptPacket — insan/metin girdisinin ontolojik paketi
// ═══════════════════════════════════════════════════════════════════════════════

/// İnsan/metin girdisinin ontolojik paketi (§3.1, §11).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptPacket {
    pub id: ConceptPacketId,
    pub packet_type: ConceptPacketType,
    pub text: String,
    pub language: String,
    pub position_family: PositionFamily,
    /// INV-C5: türetilen başlangıçta Candidate.
    pub decision_status: DecisionStatus,
    pub source: PacketSource,
}

impl ConceptPacket {
    /// Yeni packet — default Candidate (INV-C5), ConceptualIntent family.
    pub fn new(
        id: impl Into<String>,
        packet_type: ConceptPacketType,
        text: impl Into<String>,
        language: impl Into<String>,
        source: PacketSource,
    ) -> Self {
        Self {
            id: ConceptPacketId(id.into()),
            packet_type,
            text: text.into(),
            language: language.into(),
            position_family: PositionFamily::ConceptualIntent,
            decision_status: DecisionStatus::Candidate,
            source,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptNode — graph node
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept graph node (Concept/RuleCandidate/TaskCandidate/Risk/Decision/CodeEntity).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptNode {
    pub id: ConceptNodeId,
    pub canonical: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub node_kind: ConceptNodeKind,
    pub decision_status: DecisionStatus,
    pub position_family: PositionFamily,
}

impl ConceptNode {
    /// Yeni Concept node — default Candidate (INV-C5), ConceptualIntent.
    pub fn new_concept(canonical: impl Into<String>) -> Self {
        let canonical = canonical.into();
        Self {
            id: ConceptNodeId(format!("Concept:{canonical}")),
            canonical,
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptEdge — graph edge
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept graph edge. INV-C7: high-stake kind'larda explanation zorunlu.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptEdge {
    pub from: ConceptNodeId,
    pub to: ConceptNodeId,
    pub kind: ConceptEdgeKind,
    pub decision_status: DecisionStatus,
    /// INV-C7 — high-stake edge'lerde zorunlu (gate doğrular). Faz 2: NonEmptyExplanation
    /// newtype (boş string type-level engelli).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<NonEmptyExplanation>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ExtractedAnchorCandidate — extractor çıktısı (score'suz)
// ═══════════════════════════════════════════════════════════════════════════════

/// Extractor tarafından üretilen, henüz skorlanmamış aday.
/// Pipeline: extract → [`ExtractedAnchorCandidate`] → score → [`AnchorCandidate`].
///
/// # INV-C8 (Faz 2 — type-level)
/// Field'lar `pub(crate)` — crate içi TCB erişir; external crate struct literal
/// construct edemez (Rust tüm field'ların visible olmasını gerektirir). Constructor
/// `pub(crate) fn new(...)` — sadece extractor üretir.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedAnchorCandidate {
    pub(crate) packet_id: ConceptPacketId,
    pub(crate) target_node_id: ConceptNodeId,
    pub(crate) edge_kind: ConceptEdgeKind,
    /// High-stake edge'lerde doldurulur (gate INV-C7 doğrular). Faz 2: NonEmptyExplanation.
    pub(crate) explanation: Option<NonEmptyExplanation>,
}

impl ExtractedAnchorCandidate {
    /// Extractor (TCB içi) constructor.
    pub(crate) fn new(
        packet_id: ConceptPacketId,
        target_node_id: ConceptNodeId,
        edge_kind: ConceptEdgeKind,
        explanation: Option<NonEmptyExplanation>,
    ) -> Self {
        Self {
            packet_id,
            target_node_id,
            edge_kind,
            explanation,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorScoreBreakdown — §8.1 hibrit skor (7 pozitif + 2 penalty)
// ═══════════════════════════════════════════════════════════════════════════════

/// 7 bileşenli hibrit skor + 2 penalty (§8.1, D5). INV-C1: `semantic_similarity`
/// skalar — embedding vektörü görülmez (Faz 7 embedding).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnchorScoreBreakdown {
    /// Faz 1: 0.0 placeholder (lexical classifier, embedding yok). Faz 2: ScalarSimilarity
    /// newtype (INV-C1 — type-level vector değil scalar).
    pub semantic_similarity: ScalarSimilarity,
    pub ontology_type_compatibility: f64,
    pub graph_context_score: f64,
    pub domain_term_match: f64,
    /// Faz 1: 0.0 (code analizi Faz 4).
    pub code_evidence_score: f64,
    pub temporal_trust_score: f64,
    pub decision_status_score: f64,
    /// §8.1 penalty — accepted decision ile çelişki (INV-C4).
    #[serde(default)]
    pub contradiction_penalty: f64,
    /// §8.1 penalty — eski bilgi.
    #[serde(default)]
    pub staleness_penalty: f64,
}

impl Default for AnchorScoreBreakdown {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl AnchorScoreBreakdown {
    /// Tüm bileşenler 0.0 (Faz 1 başlangıç).
    pub fn zeroed() -> Self {
        Self {
            semantic_similarity: ScalarSimilarity::zero(),
            ontology_type_compatibility: 0.0,
            graph_context_score: 0.0,
            domain_term_match: 0.0,
            code_evidence_score: 0.0,
            temporal_trust_score: 0.0,
            decision_status_score: 0.0,
            contradiction_penalty: 0.0,
            staleness_penalty: 0.0,
        }
    }

    /// Ham toplam — penalty dahil, clamp YOK (negatif olabilir, debug için).
    pub fn raw_total(&self) -> f64 {
        0.25 * self.semantic_similarity.get()
            + 0.20 * self.ontology_type_compatibility
            + 0.15 * self.graph_context_score
            + 0.15 * self.domain_term_match
            + 0.10 * self.code_evidence_score
            + 0.10 * self.temporal_trust_score
            + 0.05 * self.decision_status_score
            - self.contradiction_penalty
            - self.staleness_penalty
    }

    /// Threshold policy için clamp'li toplam [0,1] (§8.2).
    pub fn total_clamped(&self) -> f64 {
        self.raw_total().clamp(0.0, 1.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorCandidate — scorer çıktısı (score'lu)
// ═══════════════════════════════════════════════════════════════════════════════

/// Skorlanmış aday. Gate bunları işleyip [`AnchorPlan`] üretir.
///
/// # INV-C8 (Faz 2 — type-level)
/// Field'lar `pub(crate)` — crate içi TCB erişir; external crate literal construct edemez.
/// Faz 3: Serialize var (audit), Deserialize YOK (INV-C8).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AnchorCandidate {
    pub(crate) packet_id: ConceptPacketId,
    pub(crate) target_node_id: ConceptNodeId,
    pub(crate) edge_kind: ConceptEdgeKind,
    pub(crate) score: AnchorScoreBreakdown,
    /// Extractor'dan gelir; high-stake'te zorunlu (INV-C7). Faz 2: NonEmptyExplanation.
    pub(crate) explanation: Option<NonEmptyExplanation>,
}

impl AnchorCandidate {
    /// Scorer (TCB içi) constructor — Extracted adaydan + skor.
    pub(crate) fn from_scored(
        extracted: ExtractedAnchorCandidate,
        score: AnchorScoreBreakdown,
    ) -> Self {
        Self {
            packet_id: extracted.packet_id,
            target_node_id: extracted.target_node_id,
            edge_kind: extracted.edge_kind,
            score,
            explanation: extracted.explanation,
        }
    }

    /// Read accessor — external crate erişebilir (INV-C8: write/construct `pub(crate)`).
    pub fn edge_kind(&self) -> ConceptEdgeKind {
        self.edge_kind
    }
    pub fn target_node_id(&self) -> &ConceptNodeId {
        &self.target_node_id
    }
    pub fn explanation(&self) -> Option<&NonEmptyExplanation> {
        self.explanation.as_ref()
    }
    pub fn score(&self) -> &AnchorScoreBreakdown {
        &self.score
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorPlan — gate çıktısı
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir ConceptPacket'ten üretilen toplam karar planı.
///
/// # INV-C8 (Faz 2 — type-level)
/// Field'lar `pub(crate)` — crate içi TCB erişir. External crate literal construct edemez
/// (Rust tüm field'ların visible olmasını gerektirir). Tek constructor `pub(crate) from_gate`
/// — sadece [`crate::anchoring::gate::AnchorGate::decide`] üretir. **"AnchorPlan almak =
/// canon gate'ten geçmiş olmak"** (yapısal garanti). INV-C8 by-pass: harici kod
/// `AnchorPlan { ... }` uydurup `apply_plan` çağıramaz.
///
/// # Faz 3 serde boundary
/// `Serialize` var (audit write). `Deserialize` **YOK** — serde ile reconstruct
/// engelli (INV-C8). DB read için [`PersistedAnchorPlanAudit`] (apply edilemez audit).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AnchorPlan {
    pub(crate) packet_id: ConceptPacketId,
    /// Graph'a yazılacak edge'ler (Candidate status, INV-C3).
    pub(crate) candidates: Vec<AnchorCandidate>,
    /// §8.2 threshold kararı.
    pub(crate) decision: AnchorDecisionKind,
    pub(crate) threshold_band: ThresholdBand,
    /// INV-C7 ↔ D6: high-stake edge varsa true.
    pub(crate) requires_operator_review: bool,
    /// §6.4.1 mapping (fix_007 — kod gerçekliği SUPERSEDES yapamaz).
    pub(crate) negative_assertions: Vec<String>,
    /// INV-C8 canon gate intercept'leri (hata DEĞİL — başarılı redirect).
    pub(crate) redirects: Vec<CanonicalRedirect>,
}

impl AnchorPlan {
    /// Gate (TCB içi) constructor. External crate erişemez.
    pub(crate) fn from_gate(
        packet_id: ConceptPacketId,
        candidates: Vec<AnchorCandidate>,
        decision: AnchorDecisionKind,
        threshold_band: ThresholdBand,
        requires_operator_review: bool,
        negative_assertions: Vec<String>,
        redirects: Vec<CanonicalRedirect>,
    ) -> Self {
        Self {
            packet_id,
            candidates,
            decision,
            threshold_band,
            requires_operator_review,
            negative_assertions,
            redirects,
        }
    }

    /// Read accessor'lar — external crate erişebilir (INV-C8: write/construct `pub(crate)`).
    pub fn candidates(&self) -> &[AnchorCandidate] {
        &self.candidates
    }
    pub fn decision(&self) -> AnchorDecisionKind {
        self.decision
    }
    pub fn threshold_band(&self) -> ThresholdBand {
        self.threshold_band
    }
    pub fn requires_operator_review(&self) -> bool {
        self.requires_operator_review
    }
    pub fn negative_assertions(&self) -> &[String] {
        &self.negative_assertions
    }
    pub fn redirects(&self) -> &[CanonicalRedirect] {
        &self.redirects
    }
    pub fn packet_id(&self) -> &ConceptPacketId {
        &self.packet_id
    }
}

impl AnchorPlan {
    /// Okunabilir multi-line rapor — Paper 3 evidence / debug için.
    /// Anchor kararının tüm bileşenlerini (candidates, decision, redirects, negative
    /// assertions) human-readable biçimde listeler.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("AnchorPlan[{}]", self.packet_id.0));
        lines.push(format!(
            "  decision: {:?} (band: {:?}, review: {})",
            self.decision, self.threshold_band, self.requires_operator_review
        ));
        if self.candidates.is_empty() {
            lines.push("  candidates: (none — unanchored)".to_string());
        } else {
            lines.push(format!("  candidates ({}):", self.candidates.len()));
            for c in &self.candidates {
                let expl = if c.explanation.is_some() {
                    " [explained]"
                } else {
                    ""
                };
                lines.push(format!(
                    "    - {:?} → {} (score={:.3}){}",
                    c.edge_kind,
                    c.target_node_id.0,
                    c.score.total_clamped(),
                    expl
                ));
            }
        }
        if !self.redirects.is_empty() {
            lines.push(format!("  redirects ({}):", self.redirects.len()));
            for r in &self.redirects {
                lines.push(format!(
                    "    - '{}' → {} ({:?})",
                    r.attempted, r.existing_node.0, r.reason
                ));
            }
        }
        if !self.negative_assertions.is_empty() {
            lines.push(format!(
                "  negative_assertions ({}):",
                self.negative_assertions.len()
            ));
            for n in &self.negative_assertions {
                lines.push(format!("    - {}", n));
            }
        }
        lines.join("\n")
    }

    /// JSON audit raporu — Paper 3 evidence / eval metodolojisi için.
    /// Her karar explainable: packet_id, decision, threshold, candidates (explanation dahil),
    /// redirects, negative_assertions. Manuel JSON (serde_json runtime dep gerekmez).
    pub fn to_audit_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!("  \"packet_id\": \"{}\",\n", self.packet_id.0));
        s.push_str(&format!("  \"decision\": \"{:?}\",\n", self.decision));
        s.push_str(&format!(
            "  \"threshold_band\": \"{:?}\",\n",
            self.threshold_band
        ));
        s.push_str(&format!(
            "  \"requires_operator_review\": {},\n",
            self.requires_operator_review
        ));
        // candidates
        s.push_str("  \"candidates\": [");
        if self.candidates.is_empty() {
            s.push_str("],\n");
        } else {
            s.push('\n');
            for (i, c) in self.candidates.iter().enumerate() {
                s.push_str("    {\n");
                s.push_str(&format!("      \"kind\": \"{:?}\",\n", c.edge_kind));
                s.push_str(&format!("      \"target\": \"{}\",\n", c.target_node_id.0));
                s.push_str(&format!(
                    "      \"score\": {:.3},\n",
                    c.score.total_clamped()
                ));
                match &c.explanation {
                    Some(e) => s.push_str(&format!(
                        "      \"explanation\": \"{}\"\n",
                        escape_json(e.as_str())
                    )),
                    None => s.push_str("      \"explanation\": null\n"),
                }
                s.push_str("    }");
                if i + 1 < self.candidates.len() {
                    s.push(',');
                }
                s.push('\n');
            }
            s.push_str("  ],\n");
        }
        // redirects
        s.push_str(&format!(
            "  \"redirects\": {},\n",
            json_array(
                &self
                    .redirects
                    .iter()
                    .map(|r| {
                        format!(
                            "{{\"attempted\":\"{}\",\"existing\":\"{}\",\"reason\":\"{:?}\"}}",
                            escape_json(&r.attempted),
                            r.existing_node.0,
                            r.reason
                        )
                    })
                    .collect::<Vec<_>>()
            )
        ));
        // negative_assertions
        s.push_str(&format!(
            "  \"negative_assertions\": {}\n",
            json_array(
                &self
                    .negative_assertions
                    .iter()
                    .map(|n| format!("\"{}\"", escape_json(n)))
                    .collect::<Vec<_>>()
            )
        ));
        s.push('}');
        s
    }

    /// Markdown audit raporu — human-readable explainable karar.
    pub fn to_audit_markdown(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# AnchorPlan Audit — {}\n\n", self.packet_id.0));
        s.push_str(&format!("- **Decision:** `{:?}`\n", self.decision));
        s.push_str(&format!(
            "- **Threshold band:** `{:?}`\n",
            self.threshold_band
        ));
        s.push_str(&format!(
            "- **Requires operator review:** {}\n\n",
            self.requires_operator_review
        ));
        s.push_str(&format!("## Candidates ({})\n\n", self.candidates.len()));
        if self.candidates.is_empty() {
            s.push_str("_(none — unanchored)_\n\n");
        } else {
            for c in &self.candidates {
                s.push_str(&format!(
                    "- `{:?}` → `{}` (score {:.3})",
                    c.edge_kind,
                    c.target_node_id.0,
                    c.score.total_clamped()
                ));
                match &c.explanation {
                    Some(e) => s.push_str(&format!("\n  - {}", e.as_str())),
                    None => s.push_str("\n  - _(no explanation)_"),
                }
                s.push('\n');
            }
            s.push('\n');
        }
        if !self.redirects.is_empty() {
            s.push_str(&format!("## Redirects ({})\n\n", self.redirects.len()));
            for r in &self.redirects {
                s.push_str(&format!(
                    "- `{}` → `{}` ({:?})\n",
                    r.attempted, r.existing_node.0, r.reason
                ));
            }
            s.push('\n');
        }
        if !self.negative_assertions.is_empty() {
            s.push_str(&format!(
                "## Negative assertions ({})\n\n",
                self.negative_assertions.len()
            ));
            for n in &self.negative_assertions {
                s.push_str(&format!("- {}\n", n));
            }
        }
        s
    }
}

/// JSON string escape helper (manuel, serde_json runtime dep yok).
fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// JSON array builder (manuel).
fn json_array(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[\n{}\n  ]",
            items
                .iter()
                .map(|i| format!("    {}", i))
                .collect::<Vec<_>>()
                .join(",\n")
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CanonicalRedirect — INV-C8 canon gate sonucu (başarılı)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-C8: CreateNode öncesi canon gate mevcut node'a redirect etti.
/// Bu hata değil — "ödeme" → mevcut `Concept:Payment`'a bağlanma başarısı.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalRedirect {
    /// Yeni node olarak denenmiş terim ("Payments", "ödeme").
    pub attempted: String,
    /// Match edilen mevcut node.
    pub existing_node: ConceptNodeId,
    pub reason: CanonicalRedirectReason,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CanonicalRedirectReason {
    ExactCanonicalMatch,
    GlossaryAliasMatch,
    EditDistanceLe2 { distance: u32 },
}

// ═══════════════════════════════════════════════════════════════════════════════
// PositionSnapshot + PositionVector — §4.2
// ═══════════════════════════════════════════════════════════════════════════════

/// Family'ye göre eksen seti taşıyan position vektörü (INV-C2).
/// PositionSnapshot — INV-C2: `family` ayrı field DEĞİL, vector'dan türetilir.
///
/// # INV-C2 (Faz 2 — type-level, snapshot seviyesi)
/// `family` field kaldırıldı; [`PositionVector::family()`] vector'dan türetilir.
/// Bu, `family: PositionFamily::PhysicalCode` + `vector: PositionVector::Evidence(...)`
/// mismatch'ini imkansız kılar. Field'lar `pub(crate)` — TCB içi (store) erişir,
/// external crate literal construct edemez (serde hariç — deserialize için `#[serde(default)]`
/// veya custom deserialize gerekirse Faz 3'te eklenir).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PositionSnapshot {
    pub id: PositionSnapshotId,
    pub node_id: ConceptNodeId,
    pub vector: PositionVector,
    pub confidence: f64,
    /// Faz 1: u64 epoch (chrono yok).
    pub measured_at: u64,
}

impl PositionSnapshot {
    /// INV-C2: family vector'dan türetilir — mismatch imkansız.
    pub fn family(&self) -> PositionFamily {
        self.vector.family()
    }
}

/// Position eksenleri — Faz 2: family-parametric (INV-C2 type-level separation).
///
/// # INV-C2 yapısal garanti
/// Üç family'nin her biri **ayrı concrete type** (`PhysicalCodeVector`,
/// `ConceptualIntentVector`, `EvidenceVector`). Compiler karıştırmayı reddeder:
/// `PhysicalCodeVector` ile `EvidenceVector` farklı tipler, birbirine atanamaz.
/// `PositionVector` enum wrapper — `PositionSnapshot.vector` bunu taşır (serde-dostu).
///
/// Family-specific constructor'lar axis count'u position-by-position doğrular
/// (array değil, named params → yanlış eksen sayısı compile error).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "family")]
pub enum PositionVector {
    /// §4.1 PhysicalCode (Paper 1): coupling/cohesion/instability/entropy/witness_depth.
    PhysicalCode(PhysicalCodeVector),
    /// §4.1 ConceptualIntent (Paper 3): abstraction/vision_alignment/implementation/
    /// confidence/risk/code_alignment.
    ConceptualIntent(ConceptualIntentVector),
    /// §4.1 Evidence (Paper 1+3): confidence/coverage/recency/stability/source_reliability.
    Evidence(EvidenceVector),
}

impl PositionVector {
    /// INV-C2: family vector'dan türetilir — `PositionSnapshot` ayrı `family` field
    /// taşımaz, mismatch imkansız. Bu metod family/vector tutarlılığını garanti eder.
    pub fn family(&self) -> PositionFamily {
        match self {
            Self::PhysicalCode(_) => PositionFamily::PhysicalCode,
            Self::ConceptualIntent(_) => PositionFamily::ConceptualIntent,
            Self::Evidence(_) => PositionFamily::Evidence,
        }
    }
}

/// PhysicalCode position vector (Paper 1, 5 eksen).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhysicalCodeVector {
    pub coupling: f64,
    pub cohesion: f64,
    pub instability: f64,
    pub entropy: f64,
    pub witness_depth: f64,
}

impl PhysicalCodeVector {
    pub fn new(
        coupling: f64,
        cohesion: f64,
        instability: f64,
        entropy: f64,
        witness_depth: f64,
    ) -> Self {
        Self {
            coupling,
            cohesion,
            instability,
            entropy,
            witness_depth,
        }
    }
}

/// ConceptualIntent position vector (Paper 3, 6 eksen).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptualIntentVector {
    pub abstraction: f64,
    pub vision_alignment: f64,
    pub implementation: f64,
    pub confidence: f64,
    pub risk: f64,
    pub code_alignment: f64,
}

impl ConceptualIntentVector {
    pub fn new(
        abstraction: f64,
        vision_alignment: f64,
        implementation: f64,
        confidence: f64,
        risk: f64,
        code_alignment: f64,
    ) -> Self {
        Self {
            abstraction,
            vision_alignment,
            implementation,
            confidence,
            risk,
            code_alignment,
        }
    }
}

/// Evidence position vector (Paper 1+3, 5 eksen).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceVector {
    pub confidence: f64,
    pub coverage: f64,
    pub recency: f64,
    pub stability: f64,
    pub source_reliability: f64,
}

impl EvidenceVector {
    pub fn new(
        confidence: f64,
        coverage: f64,
        recency: f64,
        stability: f64,
        source_reliability: f64,
    ) -> Self {
        Self {
            confidence,
            coverage,
            recency,
            stability,
            source_reliability,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptGraph — in-memory graph (space.rs desenine sadık)
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept anchoring graph'ı. `Space`'ten ayrı (`EdgeKind` 8 varyant,
/// `ConceptEdgeKind` 15). `HashMap<id, node>` + `Vec<edge>`.
///
/// # INV-C3 (Faz 2 — type-level kapsülleme)
/// `nodes`/`edges` field'ları **private**. Harici mutasyon yok — sadece
/// [`crate::anchoring::store::InMemoryAnchorStore`] (TCB içi) `pub(crate)`
/// metodlarla erişir. External crate `graph.nodes.get_mut(...).decision_status = Accepted`
/// yazamaz (compile error). Bu, INV-C3 "candidate isolation"ın yapısal garantisidir:
/// `Accepted` status sadece [`crate::anchoring::review::OperatorReviewSession::accept`]
/// üzerinden (`apply_decision` ile, INV-C12/C13 denetimli) yazılabilir.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConceptGraph {
    nodes: std::collections::HashMap<ConceptNodeId, ConceptNode>,
    edges: Vec<ConceptEdge>,
}

impl ConceptGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Node ekle (TCB içi — store/graph seed). INV-C3: harici crate erişemez.
    pub(crate) fn insert_node(&mut self, node: ConceptNode) -> &mut Self {
        self.nodes.insert(node.id.clone(), node);
        self
    }

    /// Edge ekle (TCB içi).
    pub(crate) fn insert_edge(&mut self, edge: ConceptEdge) -> &mut Self {
        self.edges.push(edge);
        self
    }

    pub fn node(&self, id: &ConceptNodeId) -> Option<&ConceptNode> {
        self.nodes.get(id)
    }

    /// Tüm node'lara read-only iterator (INV-C3: mutasyon yok).
    pub fn nodes_iter(&self) -> impl Iterator<Item = &ConceptNode> {
        self.nodes.values()
    }

    /// Tüm edge'lere read-only iterator.
    pub fn edges(&self) -> impl Iterator<Item = &ConceptEdge> {
        self.edges.iter()
    }

    /// INV-C8: canonical exact match.
    pub fn find_concept_by_canonical(&self, canonical: &str) -> Vec<&ConceptNode> {
        self.nodes
            .values()
            .filter(|n| n.canonical.eq_ignore_ascii_case(canonical))
            .collect()
    }

    /// INV-C8: canonical veya alias match.
    pub fn find_concepts_by_canonical_or_alias(&self, term: &str) -> Vec<&ConceptNode> {
        let lower = term.to_lowercase();
        self.nodes
            .values()
            .filter(|n| {
                n.canonical.to_lowercase() == lower
                    || n.aliases.iter().any(|a| a.to_lowercase() == lower)
            })
            .collect()
    }

    /// Node'u mutable al (TCB içi — store promote/apply_plan için).
    pub(crate) fn node_mut(&mut self, id: &ConceptNodeId) -> Option<&mut ConceptNode> {
        self.nodes.get_mut(id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GraphSeed — runtime boundary (fixture-bağımsız)
// ═══════════════════════════════════════════════════════════════════════════════

/// Store seed'i. FixtureGiven (test-only) buraya dönüştürülür — store.rs
/// FixtureGiven bilmez (runtime/test boundary).
///
/// # Faz 5a — candidate bucket'ları (Patch 6)
/// PR33a önce yalnızca 3 bucket vardı (concepts/decisions/code_entities); candidate
/// node'ları (RuleCandidate/TaskCandidate/RiskCandidate) seed edilemiyordu. 3 yeni
/// bucket eklendi. Backward-compat: GraphSeed `Default` derive'a sahip; yeni field'lar
/// `Vec::default()` (boş) ile başlar — eski kod `GraphSeed { concepts, decisions,
/// code_entities }` ile çalışmaya devam eder (PartialEq hariç, ama seed literal
/// construct zaten pub(crate)/test-only). `all_nodes()` deterministik sıra: concepts
/// → decisions → code_entities → rule_candidates → task_candidates → risk_candidates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphSeed {
    pub concepts: Vec<ConceptNode>,
    pub decisions: Vec<ConceptNode>,
    pub code_entities: Vec<ConceptNode>,
    /// Faz 5a — RuleCandidate seed'leri.
    pub rule_candidates: Vec<ConceptNode>,
    /// Faz 5a — TaskCandidate seed'leri.
    pub task_candidates: Vec<ConceptNode>,
    /// Faz 5a — RiskCandidate seed'leri.
    pub risk_candidates: Vec<ConceptNode>,
}

impl GraphSeed {
    pub fn is_empty(&self) -> bool {
        self.concepts.is_empty()
            && self.decisions.is_empty()
            && self.code_entities.is_empty()
            && self.rule_candidates.is_empty()
            && self.task_candidates.is_empty()
            && self.risk_candidates.is_empty()
    }

    /// Tüm seed node'larını tek iteratörde (deterministik sıra — Patch 6).
    pub fn all_nodes(&self) -> impl Iterator<Item = &ConceptNode> {
        self.concepts
            .iter()
            .chain(self.decisions.iter())
            .chain(self.code_entities.iter())
            .chain(self.rule_candidates.iter())
            .chain(self.task_candidates.iter())
            .chain(self.risk_candidates.iter())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PR E — CodeIdentityBinding (store-owned identity binding katmanı, tur 3 P1-A)
// ═══════════════════════════════════════════════════════════════════════════════

/// Node ↔ physical code identity binding (store-owned; `ConceptNode` alanı DEĞİL).
///
/// # PR E (tur 3 P1-A)
/// `CodeIdentityKey` node'nun alanı değil; store-owned `BTreeMap<ConceptNodeId, CodeIdentityKey>`
/// üzerinden node'lara bağlanır. Universal `ConceptNode` büyümez; yalnız PhysicalCode node'ları
/// binding taşır. Candidate ve Entity aynı key'e bağlanabilir (R2); snapshot validator R2/R7'yi
/// doğrular. PR F provider migration doğrudan bu indeksi kullanır.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CodeIdentityBinding {
    pub node_id: ConceptNodeId,
    pub identity_key: crate::anchoring::identity::CodeIdentityKey,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 3 — Persistence boundary tipleri (serde boundary, INV-C3/C8 koruması)
// ═══════════════════════════════════════════════════════════════════════════════

/// `ConceptGraph` snapshot — trusted restore için (Faz 3, INV-C3 persistence boundary).
///
/// # INV-C3 boundary
/// [`ConceptGraph`] private field'lara sahip ve `Deserialize` yok (serde bypass
/// edip Accepted node reconstruct etmesin diye). Bu snapshot **trusted restore
/// path** için — operator-belirlenmiş Accepted node'lar dahil tüm graph durumu.
/// [`InMemoryAnchorStore::restore_trusted_snapshot`] üzerinden yüklenir.
///
/// **Normal mutation DEĞİL** — bu deserialize/restore, yeni promotion değil.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConceptGraphSnapshot {
    pub nodes: Vec<ConceptNode>,
    pub edges: Vec<ConceptEdge>,
    /// Paper 1/2 `SNAPSHOT_FORMAT_VERSION` pattern mirror — schema migration hazırlığı.
    pub schema_version: u32,
}

impl ConceptGraphSnapshot {
    /// Faz 3 schema version. `restore_trusted_snapshot` bu değeri kontrol eder —
    /// mismatch → `StoreError::InvalidSnapshotVersion` (trusted restore boundary).
    pub const SCHEMA_VERSION: u32 = 1;
}

/// `AnchorPlan` audit record — DB'den okunan, **apply edilemez** kayıt (Faz 3, INV-C8).
///
/// # INV-C8 boundary
/// [`AnchorPlan`] `Serialize` var ama `Deserialize` **YOK** — "AnchorPlan almak =
/// canon gate'ten geçmiş olmak" garantisi serde ile delinmesin diye. Bu audit tipi
/// DB read için; apply edilemez (uygulama yapılamaz), sadece inspect/audit.
///
/// `AnchorPlan::to_audit_json()` audit write için; bu tip structured DB read için.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedAnchorPlanAudit {
    pub packet_id: String,
    pub decision: String,
    /// §8.2 threshold band (Strong/Tentative/Weak/Unanchored) — kararın skor bandı.
    pub threshold_band: String,
    pub candidates: Vec<PersistedAnchorCandidateAudit>,
    pub redirects: Vec<PersistedRedirectAudit>,
    pub negative_assertions: Vec<String>,
    pub requires_operator_review: bool,
    pub schema_version: u32,
}

/// Persisted candidate audit (apply edilemez).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedAnchorCandidateAudit {
    pub edge_kind: String,
    pub target: String,
    pub score: f64,
    pub explanation: Option<String>,
}

/// Persisted redirect audit.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistedRedirectAudit {
    pub attempted: String,
    pub existing_node: String,
    pub reason: String,
}

impl PersistedAnchorPlanAudit {
    /// Faz 3 schema version (Paper 1/2 SNAPSHOT_FORMAT_VERSION pattern).
    pub const SCHEMA_VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_empty_explanation_rejects_empty() {
        // INV-C7: boş string type-level engelli
        assert!(NonEmptyExplanation::new("").is_err());
        assert!(NonEmptyExplanation::new("   ").is_err());
        assert!(NonEmptyExplanation::new("\t\n").is_err());
    }

    #[test]
    fn non_empty_explanation_accepts_non_empty() {
        let e = NonEmptyExplanation::new("risk derived").unwrap();
        assert_eq!(e.as_str(), "risk derived");
        assert_eq!(e.to_string(), "risk derived");
        assert_eq!(e.as_ref(), "risk derived");
    }

    #[test]
    fn non_empty_explanation_serde_roundtrip() {
        let e = NonEmptyExplanation::new("explanation text").unwrap();
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, "\"explanation text\"");
        let back: NonEmptyExplanation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn non_empty_explanation_serde_rejects_empty() {
        // Boş string deserialize'ta reject (EmptyExplanation → serde error)
        let result: Result<NonEmptyExplanation, _> = serde_json::from_str("\"\"");
        assert!(result.is_err());
    }

    #[test]
    fn anchor_plan_only_constructible_via_from_gate() {
        // INV-C8: AnchorPlan field'ları pub(crate) → bu test (aynı crate) erişebilir,
        // ama external crate (tests/) literal construct edemez. Burada from_gate çalışır.
        let plan = AnchorPlan::from_gate(
            ConceptPacketId("pkt:test".into()),
            vec![],
            crate::anchoring::AnchorDecisionKind::MarkUnanchored,
            crate::anchoring::ThresholdBand::Unanchored,
            false,
            vec![],
            vec![],
        );
        assert_eq!(
            plan.decision(),
            crate::anchoring::AnchorDecisionKind::MarkUnanchored
        );
        assert!(plan.candidates().is_empty());
    }

    #[test]
    fn scalar_similarity_range_check() {
        // INV-C1: [0,1] range-check
        assert!(ScalarSimilarity::new(0.0).is_ok());
        assert!(ScalarSimilarity::new(0.5).is_ok());
        assert!(ScalarSimilarity::new(1.0).is_ok());
        assert!(ScalarSimilarity::new(-0.1).is_err());
        assert!(ScalarSimilarity::new(1.1).is_err());
        assert!(ScalarSimilarity::new(f64::NAN).is_err());
    }

    #[test]
    fn scalar_similarity_zero_one_get() {
        assert_eq!(ScalarSimilarity::zero().get(), 0.0);
        assert_eq!(ScalarSimilarity::one().get(), 1.0);
        let s = ScalarSimilarity::new(0.7).unwrap();
        assert_eq!(s.get(), 0.7);
    }

    #[test]
    fn scalar_similarity_serde_roundtrip() {
        let s = ScalarSimilarity::new(0.42).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "0.42");
        let back: ScalarSimilarity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn scalar_similarity_serde_rejects_out_of_range() {
        // Hardening PR: Deserialize new() üzerinden range-check yapar.
        // serde_json::from_str("2.0") / "-1.0" / "NaN" reject — constructor bypass
        // edilemez (EvidenceStrength R1 ile aynı serde hijyeni).
        assert!(serde_json::from_str::<ScalarSimilarity>("2.0").is_err());
        assert!(serde_json::from_str::<ScalarSimilarity>("-1.0").is_err());
        assert!(serde_json::from_str::<ScalarSimilarity>("\"NaN\"").is_err());
    }

    #[test]
    fn anchor_score_breakdown_uses_scalar_similarity() {
        // INV-C1: breakdown semantic_similarity ScalarSimilarity (vector değil)
        let mut b = AnchorScoreBreakdown::zeroed();
        b.semantic_similarity = ScalarSimilarity::one();
        assert_eq!(b.semantic_similarity.get(), 1.0);
        // raw_total: 0.25 * 1.0 = 0.25 (sadece semantic, diğerleri 0)
        assert!((b.raw_total() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn position_vector_families_are_distinct_types() {
        // INV-C2: üç family ayrı concrete type → compiler karıştırmayı reddeder
        let phys = PhysicalCodeVector::new(0.8, 0.6, 0.4, 0.3, 0.2);
        let intent = ConceptualIntentVector::new(0.5, 0.7, 0.3, 0.6, 0.4, 0.8);
        let evidence = EvidenceVector::new(0.9, 0.85, 0.7, 0.6, 0.95);

        // Farklı tipler — typed accessor'lar
        assert!((phys.coupling - 0.8).abs() < 1e-9);
        assert!((intent.abstraction - 0.5).abs() < 1e-9);
        assert!((evidence.source_reliability - 0.95).abs() < 1e-9);

        // PositionVector enum wrap
        let pv_phys = PositionVector::PhysicalCode(phys);
        let pv_intent = PositionVector::ConceptualIntent(intent);
        let pv_evidence = PositionVector::Evidence(evidence);
        assert!(matches!(pv_phys, PositionVector::PhysicalCode(_)));
        assert!(matches!(pv_intent, PositionVector::ConceptualIntent(_)));
        assert!(matches!(pv_evidence, PositionVector::Evidence(_)));
    }

    #[test]
    fn position_vector_serde_roundtrip() {
        let pv = PositionVector::PhysicalCode(PhysicalCodeVector::new(0.8, 0.6, 0.4, 0.3, 0.2));
        let json = serde_json::to_string(&pv).unwrap();
        let back: PositionVector = serde_json::from_str(&json).unwrap();
        assert_eq!(pv, back);
    }

    #[test]
    fn anchor_plan_audit_json_contains_decision_and_explanations() {
        // INV-C7 audit: her karar explainable
        let plan = AnchorPlan::from_gate(
            ConceptPacketId("pkt:audit".into()),
            vec![AnchorCandidate::from_scored(
                ExtractedAnchorCandidate::new(
                    ConceptPacketId("pkt:audit".into()),
                    ConceptNodeId("Concept:Payment".into()),
                    crate::anchoring::ConceptEdgeKind::Mentions,
                    None,
                ),
                AnchorScoreBreakdown::zeroed(),
            )],
            crate::anchoring::AnchorDecisionKind::TentativeLink,
            crate::anchoring::ThresholdBand::Tentative,
            true,
            vec!["SUPERSEDES yasak".into()],
            vec![],
        );
        let json = plan.to_audit_json();
        assert!(json.contains("\"packet_id\": \"pkt:audit\""));
        assert!(json.contains("\"decision\": \"TentativeLink\""));
        assert!(json.contains("\"SUPERSEDES yasak\""));
        // explanation null (düşük-stake Mentions)
        assert!(json.contains("\"explanation\": null"));
    }

    #[test]
    fn anchor_plan_audit_markdown_readable() {
        let plan = AnchorPlan::from_gate(
            ConceptPacketId("pkt:md".into()),
            vec![],
            crate::anchoring::AnchorDecisionKind::MarkUnanchored,
            crate::anchoring::ThresholdBand::Unanchored,
            false,
            vec![],
            vec![],
        );
        let md = plan.to_audit_markdown();
        assert!(md.contains("# AnchorPlan Audit — pkt:md"));
        assert!(md.contains("**Decision:** `MarkUnanchored`"));
        assert!(md.contains("_(none — unanchored)_"));
    }

    #[test]
    fn anchor_plan_audit_json_is_valid_json() {
        // Manuel JSON üretimi malformed olabilir — serde_json ile parse-validity doğrula.
        let plan = AnchorPlan::from_gate(
            ConceptPacketId("pkt:valid".into()),
            vec![AnchorCandidate::from_scored(
                ExtractedAnchorCandidate::new(
                    ConceptPacketId("pkt:valid".into()),
                    ConceptNodeId("Concept:Payment".into()),
                    crate::anchoring::ConceptEdgeKind::Mentions,
                    None,
                ),
                AnchorScoreBreakdown::zeroed(),
            )],
            crate::anchoring::AnchorDecisionKind::TentativeLink,
            crate::anchoring::ThresholdBand::Tentative,
            true,
            vec!["SUPERSEDES yasak".into()],
            vec![],
        );
        let json = plan.to_audit_json();
        // Geçerli JSON olması lazım (serde_json parse edebilmeli)
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("audit JSON geçerli JSON olmalı");
        assert_eq!(parsed["packet_id"], "pkt:valid");
        assert_eq!(parsed["decision"], "TentativeLink");
        assert!(parsed["candidates"].is_array());
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // PR C — Axis-granular evidence model unit testleri
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Test helper: tek-eksen ObservedPhysicalMetric.
    fn single_metric(
        axis: PhysicalCodeMetricAxis,
        value: f64,
        strength: f64,
    ) -> ObservedPhysicalMetric {
        ObservedPhysicalMetric::new(
            axis,
            value,
            ObservedCodeMetricSource::Scip,
            EvidenceStrength::new(strength).unwrap(),
            EvidenceCoverage::new(1.0).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn physical_axis_value_range_check_uniform() {
        // Uniform [0,1]: tüm 5 eksende aynı validation.
        assert!(PhysicalAxisValue::new(0.0).is_ok());
        assert!(PhysicalAxisValue::new(0.52).is_ok()); // entropy normalize
        assert!(PhysicalAxisValue::new(1.0).is_ok());
        // Boundary violations
        assert_eq!(
            PhysicalAxisValue::new(1.1).unwrap_err(),
            MetricScalarViolation::AboveMaximum
        );
        assert_eq!(
            PhysicalAxisValue::new(-0.01).unwrap_err(),
            MetricScalarViolation::BelowMinimum
        );
        assert_eq!(
            PhysicalAxisValue::new(f64::NAN).unwrap_err(),
            MetricScalarViolation::NonFinite
        );
        assert_eq!(
            PhysicalAxisValue::new(f64::INFINITY).unwrap_err(),
            MetricScalarViolation::NonFinite
        );
    }

    #[test]
    fn evidence_coverage_range_check() {
        assert!(EvidenceCoverage::new(0.0).is_ok());
        assert!(EvidenceCoverage::new(0.5).is_ok());
        assert!(EvidenceCoverage::new(1.0).is_ok());
        assert!(EvidenceCoverage::new(1.01).is_err());
        assert!(EvidenceCoverage::new(-0.1).is_err());
        assert!(EvidenceCoverage::new(f64::NAN).is_err());
    }

    #[test]
    fn observed_physical_metric_zero_strength_reject() {
        // Zero-strength reject: strength=0 → ZeroStrength { axis }.
        let err = ObservedPhysicalMetric::new(
            PhysicalCodeMetricAxis::Coupling,
            0.42,
            ObservedCodeMetricSource::Scip,
            EvidenceStrength::new(0.0).unwrap(), // boundary geçerli newtype
            EvidenceCoverage::new(1.0).unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            ObservedPhysicalMetricError::ZeroStrength {
                axis: PhysicalCodeMetricAxis::Coupling
            }
        );
    }

    #[test]
    fn observed_physical_metric_invalid_value_axis_context() {
        // InvalidValue axis/value context: entropy 1.1 → AboveMaximum.
        let err = ObservedPhysicalMetric::new(
            PhysicalCodeMetricAxis::Entropy,
            1.1,
            ObservedCodeMetricSource::Scip,
            EvidenceStrength::one(),
            EvidenceCoverage::new(1.0).unwrap(),
        )
        .unwrap_err();
        match err {
            ObservedPhysicalMetricError::InvalidValue {
                axis,
                value,
                violation,
            } => {
                assert_eq!(axis, PhysicalCodeMetricAxis::Entropy);
                assert_eq!(value, 1.1);
                assert_eq!(violation, MetricScalarViolation::AboveMaximum);
            }
            other => panic!("InvalidValue bekleniyordu, got {other:?}"),
        }
        // entropy 0.52 → Ok (representative normalized).
        assert!(single_metric(PhysicalCodeMetricAxis::Entropy, 0.52, 0.85).value().get() - 0.52
            < 1e-9);
    }

    #[test]
    fn evidence_strength_new_zero_boundary_unchanged() {
        // EvidenceStrength::new(0.0).is_ok() boundary test dokunulmaz (0 newtype aralıkta geçerli).
        // Zero-strength reject ObservedPhysicalMetric seviyesinde.
        assert!(EvidenceStrength::new(0.0).is_ok());
        assert_eq!(EvidenceStrength::new(0.0).unwrap().get(), 0.0);
    }

    #[test]
    fn observed_physical_metrics_try_new_rejects_empty() {
        assert_eq!(
            ObservedPhysicalMetrics::try_new(vec![]).unwrap_err(),
            ObservedPhysicalMetricsError::Empty
        );
    }

    #[test]
    fn observed_physical_metrics_try_new_rejects_duplicate_axis() {
        // Aynı eksende iki observation → DuplicateAxis.
        let err = ObservedPhysicalMetrics::try_new(vec![
            single_metric(PhysicalCodeMetricAxis::Coupling, 0.4, 0.8),
            single_metric(PhysicalCodeMetricAxis::Coupling, 0.5, 0.7),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            ObservedPhysicalMetricsError::DuplicateAxis {
                axis: PhysicalCodeMetricAxis::Coupling
            }
        );
    }

    #[test]
    fn observed_physical_metrics_try_new_sorts_by_sort_order() {
        // Reverse-order input → deterministic sort_order sıralı çıktı.
        let metrics = ObservedPhysicalMetrics::try_new(vec![
            single_metric(PhysicalCodeMetricAxis::WitnessDepth, 0.2, 0.8),
            single_metric(PhysicalCodeMetricAxis::Coupling, 0.4, 0.8),
            single_metric(PhysicalCodeMetricAxis::Entropy, 0.3, 0.8),
        ])
        .unwrap();
        let axes: Vec<_> = metrics.values().iter().map(|o| o.axis()).collect();
        assert_eq!(
            axes,
            vec![
                PhysicalCodeMetricAxis::Coupling,
                PhysicalCodeMetricAxis::Entropy,
                PhysicalCodeMetricAxis::WitnessDepth,
            ]
        );
        // iter ExactSizeIterator.
        assert_eq!(metrics.iter().len(), 3);
        // contains.
        assert!(metrics.contains(PhysicalCodeMetricAxis::Coupling));
        assert!(!metrics.contains(PhysicalCodeMetricAxis::Cohesion));
    }

    #[test]
    fn observed_physical_metrics_minimum_strength_heterogeneous() {
        // Heterojen pin: [0.9, 0.6, 0.8] → min 0.6.
        let metrics = ObservedPhysicalMetrics::try_new(vec![
            single_metric(PhysicalCodeMetricAxis::Coupling, 0.4, 0.9),
            single_metric(PhysicalCodeMetricAxis::Cohesion, 0.4, 0.6),
            single_metric(PhysicalCodeMetricAxis::Instability, 0.4, 0.8),
        ])
        .unwrap();
        assert!((metrics.minimum_observed_strength().get() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn observed_physical_metrics_minimum_strength_single_axis() {
        // Single axis: 0.8 → 0.8.
        let metrics = ObservedPhysicalMetrics::try_new(vec![single_metric(
            PhysicalCodeMetricAxis::Coupling,
            0.4,
            0.8,
        )])
        .unwrap();
        assert!((metrics.minimum_observed_strength().get() - 0.8).abs() < 1e-9);
    }

    #[test]
    fn try_to_physical_vector_succeeds_all_five_axes() {
        // 5 axes present → Ok(PhysicalCodeVector).
        let metrics = ObservedPhysicalMetrics::try_new(vec![
            single_metric(PhysicalCodeMetricAxis::Coupling, 0.42, 0.85),
            single_metric(PhysicalCodeMetricAxis::Cohesion, 0.78, 0.85),
            single_metric(PhysicalCodeMetricAxis::Instability, 0.30, 0.85),
            single_metric(PhysicalCodeMetricAxis::Entropy, 0.52, 0.85),
            single_metric(PhysicalCodeMetricAxis::WitnessDepth, 0.68, 0.85),
        ])
        .unwrap();
        let pv = metrics.try_to_physical_vector().expect("5 axes → Ok");
        assert!((pv.coupling - 0.42).abs() < 1e-9);
        assert!((pv.cohesion - 0.78).abs() < 1e-9);
        assert!((pv.instability - 0.30).abs() < 1e-9);
        assert!((pv.entropy - 0.52).abs() < 1e-9);
        assert!((pv.witness_depth - 0.68).abs() < 1e-9);
    }

    #[test]
    fn try_to_physical_vector_fails_missing_axes_deterministic() {
        // 3 axes present → missing [Entropy, WitnessDepth] (sort_order sıralı).
        let metrics = ObservedPhysicalMetrics::try_new(vec![
            single_metric(PhysicalCodeMetricAxis::Instability, 0.30, 0.85),
            single_metric(PhysicalCodeMetricAxis::Coupling, 0.42, 0.85),
            single_metric(PhysicalCodeMetricAxis::Cohesion, 0.78, 0.85),
        ])
        .unwrap();
        let err = metrics.try_to_physical_vector().unwrap_err();
        assert_eq!(
            err.missing(),
            &[
                PhysicalCodeMetricAxis::Entropy,
                PhysicalCodeMetricAxis::WitnessDepth,
            ]
        );
        // missing_axes() ile aynı deterministik sıra.
        assert_eq!(err.missing(), metrics.missing_axes().as_slice());
    }

    #[test]
    fn shape_compatibility_mixed_provenance() {
        // 3-axis mixed provenance (Coupling TreeSitter + Cohesion Scip + Instability TreeSitter)
        // → sort_order + missing [Entropy, WitnessDepth].
        let ts = ObservedCodeMetricSource::TreeSitter;
        let scip = ObservedCodeMetricSource::Scip;
        let metrics = ObservedPhysicalMetrics::try_new(vec![
            ObservedPhysicalMetric::new(
                PhysicalCodeMetricAxis::Instability,
                0.30,
                ts,
                EvidenceStrength::new(0.7).unwrap(),
                EvidenceCoverage::new(0.9).unwrap(),
            )
            .unwrap(),
            ObservedPhysicalMetric::new(
                PhysicalCodeMetricAxis::Coupling,
                0.42,
                ts,
                EvidenceStrength::new(0.8).unwrap(),
                EvidenceCoverage::new(1.0).unwrap(),
            )
            .unwrap(),
            ObservedPhysicalMetric::new(
                PhysicalCodeMetricAxis::Cohesion,
                0.78,
                scip,
                EvidenceStrength::new(0.9).unwrap(),
                EvidenceCoverage::new(1.0).unwrap(),
            )
            .unwrap(),
        ])
        .unwrap();
        // Per-axis provenance preserved.
        assert_eq!(metrics.values()[0].source(), ts); // Coupling (sort_order 0)
        assert_eq!(metrics.values()[1].source(), scip); // Cohesion (sort_order 1)
        assert_eq!(metrics.values()[2].source(), ts); // Instability (sort_order 2)
        // Missing deterministik.
        assert_eq!(
            metrics.missing_axes(),
            vec![
                PhysicalCodeMetricAxis::Entropy,
                PhysicalCodeMetricAxis::WitnessDepth,
            ]
        );
        // min = 0.7 (Instability).
        assert!((metrics.minimum_observed_strength().get() - 0.7).abs() < 1e-9);
    }

    #[test]
    fn observed_physical_metric_accessors() {
        let m = ObservedPhysicalMetric::new(
            PhysicalCodeMetricAxis::Coupling,
            0.42,
            ObservedCodeMetricSource::Scip,
            EvidenceStrength::new(0.85).unwrap(),
            EvidenceCoverage::new(0.95).unwrap(),
        )
        .unwrap();
        assert_eq!(m.axis(), PhysicalCodeMetricAxis::Coupling);
        assert!((m.value().get() - 0.42).abs() < 1e-9);
        assert_eq!(m.source(), ObservedCodeMetricSource::Scip);
        assert!((m.strength().get() - 0.85).abs() < 1e-9);
        assert!((m.coverage().get() - 0.95).abs() < 1e-9);
    }
}

#[test]
fn concept_graph_snapshot_serde_roundtrip() {
    // Faz 3: ConceptGraphSnapshot (trusted restore) serde
    let node = ConceptNode {
        id: ConceptNodeId("Concept:Payment".into()),
        canonical: "Payment".into(),
        aliases: vec!["ödeme".into()],
        node_kind: ConceptNodeKind::Concept,
        decision_status: DecisionStatus::Accepted,
        position_family: crate::anchoring::PositionFamily::ConceptualIntent,
    };
    let snapshot = ConceptGraphSnapshot {
        nodes: vec![node],
        edges: vec![],
        schema_version: 1,
    };
    let json = serde_json::to_string(&snapshot).unwrap();
    let back: ConceptGraphSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(snapshot, back);
}

#[test]
fn persisted_anchor_plan_audit_serde_roundtrip() {
    // Faz 3: PersistedAnchorPlanAudit (DB read, apply edilemez) serde
    let audit = PersistedAnchorPlanAudit {
        packet_id: "pkt:audit".into(),
        decision: "TentativeLink".into(),
        threshold_band: "Tentative".into(),
        candidates: vec![PersistedAnchorCandidateAudit {
            edge_kind: "DerivesRisk".into(),
            target: "RiskCandidate:X".into(),
            score: 0.7,
            explanation: Some("risk derived".into()),
        }],
        redirects: vec![PersistedRedirectAudit {
            attempted: "ödeme".into(),
            existing_node: "Concept:Payment".into(),
            reason: "GlossaryAliasMatch".into(),
        }],
        negative_assertions: vec!["SUPERSEDES yasak".into()],
        requires_operator_review: true,
        schema_version: PersistedAnchorPlanAudit::SCHEMA_VERSION,
    };
    let json = serde_json::to_string(&audit).unwrap();
    let back: PersistedAnchorPlanAudit = serde_json::from_str(&json).unwrap();
    assert_eq!(audit, back);
}
