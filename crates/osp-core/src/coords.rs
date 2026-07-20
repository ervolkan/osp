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
/// Core axes expose a legacy `f64` projection through `compute()`, while provenance-
/// sensitive authority/evidence paths use `AxisMeasurement { value, source }` through
/// `measure()` (INV-T9 #70 — Commit 1 semantic v2).
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
///
/// **INV-T9 #70:** `Mixed` varyantı yalnız heterojen aggregation çıktısıdır (Commit 2
/// `aggregate_source`). Doğrudan axis kaynağı olarak kabul edilemez —
/// `validate_direct_source` ve axis constructor'larında `MixedCannotBeDeclaredDirectly`
/// ile reddedilir.
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
    /// Heterojen aggregation çıktısı (birden fazla source tek measurer'da birleşti).
    /// Doğrudan axis kaynağı olarak kabul EDİLEMEZ.
    Mixed,
}

impl MetricSource {
    /// **INV-T9 #70 (P1-1 stable byte ID):** Descriptor parameter identity için stable
    /// source ID byte'ları. coords katmanı `CanonicalMetricSourceTag` KULLANMAZ (ters
    /// bağımlılık — canonical_tags coords'a bağlı olmalı). Authorization wire
    /// representation ayrı katmandır (`CanonicalMetricSourceTag`).
    pub(crate) fn descriptor_id(self) -> &'static [u8] {
        match self {
            Self::TreeSitter => b"tree-sitter",
            Self::Scip => b"scip",
            Self::Placeholder => b"placeholder",
            Self::Heuristic => b"heuristic",
            Self::Mixed => b"mixed",
        }
    }
}

impl std::fmt::Display for MetricSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TreeSitter => write!(f, "tree-sitter"),
            Self::Scip => write!(f, "scip"),
            Self::Placeholder => write!(f, "placeholder"),
            Self::Heuristic => write!(f, "heuristic"),
            Self::Mixed => write!(f, "mixed"),
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
// AxisMeasurement + MeasuredRawPosition (INV-T9 #70 — provenance-native neutral layer)
//
// Neutral coords-layer per-axis ölçüm tipi. `value + source` pair; validation non-finite
// + [0,1] range defensive. Authority/evidence yolları (Commit 2 `measured_position_of`)
// her axis output'unu `validate_direct_axis_output()` ile defensive re-validate eder.
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-T9 #70 — Tek eksen ölçümü (coords-layer neutral). `value + source` pair.
///
/// **Validation contract:** `try_new` non-finite (NaN/±Inf) ve [0,1] range dışı değeri
/// reddeder. Public fields ile struct literal bypass mümkün — wire-bypass `Deserialize`
/// ile kapatıldı (P1-2: `try_new` validation guaranteed). Authority path defensive
/// re-validate Commit 2 `measured_position_of` ile.
///
/// **Source provenance:** `Mixed` yalnız aggregation çıktısıdır; doğrudan axis kaynağı
/// olarak kabul edilemez (`AxisSourceError::MixedCannotBeDeclaredDirectly`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct AxisMeasurement {
    /// Metric değeri — `[0,1]` normalize (finite invariant).
    pub value: f64,
    /// Değerin kaynağı (provenance).
    pub source: MetricSource,
}

impl AxisMeasurement {
    /// Validated constructor — non-finite + [0,1] range defensive (P1-5).
    pub fn try_new(value: f64, source: MetricSource) -> Result<Self, AxisMeasurementError> {
        if !value.is_finite() {
            return Err(AxisMeasurementError::NonFiniteValue);
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(AxisMeasurementError::OutOfRange(value));
        }
        Ok(Self { value, source })
    }

    /// Defensive re-validation (Commit 2 `measured_position_of` her axis output'ını
    /// çağırır). Public field bypass'a karşı defensive.
    pub fn validate(&self) -> Result<(), AxisMeasurementError> {
        Self::try_new(self.value, self.source).map(|_| ())
    }
}

/// **P1-2 (wire integrity):** Custom Deserialize — wire bypass kapanır. `try_new`
/// validation guaranteed; `deny_unknown_fields` unknown field'ları reddeder.
///
/// Diskten `{"value": 2.0, "source": "Scip"}` reddedilir; `{"value": 0.5, "source": "Scip",
/// "extra": true}` da reddedilir (strict authority surface).
impl<'de> serde::Deserialize<'de> for AxisMeasurement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            value: f64,
            source: MetricSource,
        }
        let wire = <Wire as serde::Deserialize>::deserialize(deserializer)?;
        AxisMeasurement::try_new(wire.value, wire.source).map_err(serde::de::Error::custom)
    }
}

/// Axis measurement içeriği hataları (yalnız measurement — descriptor ayrı).
///
/// **P1-1:** `Eq` derive EDİLMEZ — `OutOfRange(f64)` f64 içerir, f64 `Eq` değil.
/// `PartialEq` test `assert_eq!` için yeterli.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AxisMeasurementError {
    #[error("non-finite axis value (NaN/Inf rejected)")]
    NonFiniteValue,
    #[error("axis value out of range [0,1]: {0}")]
    OutOfRange(f64),
}

/// INV-T9 #70 — Mixed doğrudan axis kaynağı olarak reddi. `Mixed` yalnız heterojen
/// aggregation çıktısıdır (Commit 2 `aggregate_source`); axis constructor'larında
/// `validate_direct_source` ile derleme zamanı değil runtime guard.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AxisSourceError {
    #[error("Mixed source cannot be declared directly (only aggregation output)")]
    MixedCannotBeDeclaredDirectly,
}

/// Mixed olmayan direct source doğrulaması — axis constructor'ları bunu çağırır.
pub fn validate_direct_source(source: MetricSource) -> Result<MetricSource, AxisSourceError> {
    if source == MetricSource::Mixed {
        Err(AxisSourceError::MixedCannotBeDeclaredDirectly)
    } else {
        Ok(source)
    }
}

/// INV-T9 #70 — 5 core axis provenance'lı ölçüm (coords-layer neutral).
///
/// `MeasuredRawPosition` `to_raw()` value-only projection sağlar; `axis()` PredicateAxis
/// accessor trajectory.rs'te inherent impl olarak yaşar (neutral katman PredicateAxis
/// bağımlılığı yok — P1-4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeasuredRawPosition {
    pub coupling: AxisMeasurement,
    pub cohesion: AxisMeasurement,
    pub instability: AxisMeasurement,
    pub entropy: AxisMeasurement,
    pub witness_depth: AxisMeasurement,
}

impl MeasuredRawPosition {
    /// Sadece değerleri RawPosition'a indirge (loss/distance hesabı için, source'suz).
    pub fn to_raw(&self) -> RawPosition {
        RawPosition {
            x: self.coupling.value,
            y: self.cohesion.value,
            z: self.instability.value,
            w: self.entropy.value,
            v: self.witness_depth.value,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AxisDescriptor + AxisParameterEncoder (INV-T9 Adım 3 — canonical measurement context)
//
// Axis descriptor = axis implementation identity + semantics version + canonical
// parameters (effective normalized runtime state + formula semantics). Mirror of
// `RuleDescriptor` (authorization.rs), ama neutral coords layer'da yaşar.
// ═══════════════════════════════════════════════════════════════════════════════

/// Canonical axis descriptor — `MeasurementInputDigest` için axis implementation identity.
///
/// **reviewer (effective normalized model):** Descriptor, constructor'a verilen ham
/// argümanları DEĞİL, validation/normalization sonrasında `compute()` davranışını
/// gerçekten etkileyen effective runtime state'i bağlar. Ham constructor argümanları
/// authorization identity'nin parçası DEĞİL — ancak normalization sonrası behaviorally
/// relevant kaldıkça. Örn `EntropyAxis::from_commit_entropy(13)` ve `(100)` clamp
/// sonrası aynı value üretirse → aynı descriptor (gereksiz staleness yok).
///
/// **Mirror of `RuleDescriptor`** (authorization.rs:721): axis_id + semantics_version +
/// canonical_parameters. Koordinat axis'leri için canonical measurement context'i bağlar.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AxisDescriptor {
    axis_id: String,
    semantics_version: u32,
    canonical_parameters: Vec<u8>,
}

impl AxisDescriptor {
    /// Güvenilir runtime üretimi — `AxisParameterEncoder`'dan.
    /// Axis implementasyonları bunu `descriptor()` içinde çağırır.
    pub fn try_new(
        axis_id: &str,
        semantics_version: u32,
        parameters: AxisParameterEncoder,
    ) -> Result<Self, AxisDescriptorError> {
        Self::try_from_parts(axis_id.to_owned(), semantics_version, parameters.finish())
    }

    /// Deserialize sınırı — opaque canonical bytes korunur (içerik doğrulanamaz).
    ///
    /// **Güven sınırı:** Deserialized `canonical_parameters` byte'ları içeriğini korur;
    /// semantic validation post-serialization imkânsız — yalnız descriptor identity
    /// (axis_id non-empty) ve version structure (semantics_version > 0) doğrulanır.
    /// Byte'lar yalnızca `AxisParameterEncoder` tarafından üretildiyse trust edilir.
    fn try_from_parts(
        axis_id: String,
        semantics_version: u32,
        canonical_parameters: Vec<u8>,
    ) -> Result<Self, AxisDescriptorError> {
        if axis_id.is_empty() {
            return Err(AxisDescriptorError::EmptyAxisId);
        }
        if semantics_version == 0 {
            return Err(AxisDescriptorError::InvalidSemanticsVersion(
                semantics_version,
            ));
        }
        Ok(Self {
            axis_id,
            semantics_version,
            canonical_parameters,
        })
    }

    pub fn axis_id(&self) -> &str {
        &self.axis_id
    }
    pub fn semantics_version(&self) -> u32 {
        self.semantics_version
    }
    pub fn canonical_parameters(&self) -> &[u8] {
        &self.canonical_parameters
    }
}

/// Custom `Deserialize` — `try_from_parts` üzerinden. Diskten gelen opaque canonical
/// byte'lar korunur; axis_id/semantics_version yapısı doğrulanır.
impl<'de> serde::Deserialize<'de> for AxisDescriptor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct AxisDescriptorWire {
            axis_id: String,
            semantics_version: u32,
            canonical_parameters: Vec<u8>,
        }
        let wire = AxisDescriptorWire::deserialize(deserializer)?;
        AxisDescriptor::try_from_parts(
            wire.axis_id,
            wire.semantics_version,
            wire.canonical_parameters,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Axis descriptor içeriği hataları (yalnız descriptor — collection/context DEĞİL).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AxisDescriptorError {
    #[error("empty axis_id in descriptor")]
    EmptyAxisId,
    #[error("invalid semantics_version (must be > 0): {0}")]
    InvalidSemanticsVersion(u32),
    #[error("non-finite canonical parameter (NaN/±Infinity rejected)")]
    NonFiniteParameter,
    #[error("canonical length overflow in {field}")]
    LengthOverflow { field: &'static str },
}

/// Axis registration sınırı hataları (CoordinateSystem::register_axis/try_with_axis).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AxisRegistrationError {
    #[error("empty runtime axis name")]
    EmptyAxisName,
    #[error("duplicate axis_id at registration: {0}")]
    DuplicateAxisId(String),
    #[error("axis descriptor identity mismatch: runtime name={runtime_name}, descriptor_id={descriptor_id}")]
    IdentityMismatch {
        runtime_name: String,
        descriptor_id: String,
    },
    #[error("descriptor production failed: {0}")]
    DescriptorFailed(#[from] AxisDescriptorError),
    /// **INV-T9 #70:** Mixed source doğrudan axis kaynağı olarak red.
    #[error("invalid axis source: {0}")]
    InvalidAxisSource(#[from] AxisSourceError),
}

/// Defensive axis-context validation hataları (canonical_raw_axis_descriptors —
/// registration DEĞİL, mevcut collection'ın defensive doğrulanması).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AxisContextError {
    #[error("descriptor production failed: {0}")]
    DescriptorFailed(#[from] AxisDescriptorError),
    #[error("duplicate axis_id in context: {0}")]
    DuplicateAxisId(String),
    #[error("axis descriptor identity mismatch: runtime name={runtime_name}, descriptor_id={descriptor_id}")]
    IdentityMismatch {
        runtime_name: String,
        descriptor_id: String,
    },
}

/// Canonical axis parameter encoder — authorization encoder'ın float kurallarını uygular.
///
/// **reviewer P0-2:** Raw `f64::to_le_bytes()` NaN/Inf kabul eder, `-0.0`/`+0.0` ayrım
/// yapar. Bu encoder: non-finite reject, -0.0→+0.0 normalize, length-prefix bytes,
/// checked `u64` conversion (`u64::try_from`, typed `LengthOverflow`). Her axis
/// `descriptor()` impl'i bunu kullanır — doğrudan `to_le_bytes` YOK.
#[derive(Debug, Clone, Default)]
pub struct AxisParameterEncoder {
    bytes: Vec<u8>,
}

impl AxisParameterEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    /// Checked u64 — `usize → u64` taşması typed hata verir.
    pub fn push_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Canonical f64 — non-finite reject, -0.0 → +0.0 normalize.
    pub fn push_f64(&mut self, value: f64) -> Result<(), AxisDescriptorError> {
        if !value.is_finite() {
            return Err(AxisDescriptorError::NonFiniteParameter);
        }
        // -0.0 → +0.0 (bit-level canonicalization: clear sign bit of zero).
        let canonical = if value == 0.0 { 0.0f64 } else { value };
        self.bytes.extend_from_slice(&canonical.to_le_bytes());
        Ok(())
    }

    /// Length-prefixed opaque bytes (checked length).
    pub fn push_bytes(&mut self, value: &[u8]) -> Result<(), AxisDescriptorError> {
        let len = u64::try_from(value.len()).map_err(|_| AxisDescriptorError::LengthOverflow {
            field: "push_bytes.len",
        })?;
        self.push_u64(len);
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Axis trait + CoordinateSystem (pluggable, §2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek bir koordinat eksenini temsil eden trait.
///
/// Domain'e özel eksenler (security, accessibility) bu trait'i implement ederek
/// `CoordinateSystem`'e eklenebilir. Ölçüm değeri **[0,1]** aralığında normalize.
///
/// **INV-T9 Adım 3 — descriptor() contract:**
/// - **Zorunlu**, default impl YOK. İki custom axis aynı `name()` + farklı `compute()`
///   aynı digest ürememeli — her axis explicit descriptor beyan etmeli.
/// - **Deterministic ve saf:** Aynı immutable axis state için her çağrıda aynı sonuç.
///   Axis interior mutability/nondeterministik descriptor üretirse digest güvenilir olmaz.
/// - **Effective normalized binding:** descriptor, `compute()` davranışını etkileyen
///   effective runtime state + formula semantics version bağlar; ham constructor
///   argümanları DEĞİL.
///
/// **INV-T9 #70 — measure() authoritative:**
/// - `measure()` ölçüm authority'sidir: value + source döner, fallible + validated.
///   Authority/evidence yolları yalnız bunu kullanmalı.
/// - `try_compute()` fallible value projection (measure() üzerinden).
/// - `compute()` legacy infallible value-only projection. `#[deprecated]` attribute
///   Commit 4 cleanup'ta eklenecek (mevcut caller churn'ü Commit 1'de YOK).
pub trait Axis: Send + Sync {
    /// Eksen adı — `raw_position_of` isme göre mapler (sıra değil).
    /// Standart adlar: `"coupling"`, `"cohesion"`, `"instability"`, `"entropy"`, `"witness_depth"`.
    fn name(&self) -> &'static str;

    /// **INV-T9 Adım 3:** Axis descriptor'ı — canonical measurement context için.
    /// Deterministic, saf, fallible (non-finite parametre fail-closed). Her axis
    /// explicit implement eder; default impl güvenli olmadığı için YOK.
    fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError>;

    /// **INV-T9 #70 authoritative:** Ölçüm + provenance üretir (fallible, validated).
    /// Authority/evidence yolları yalnız bunu kullanmalı. `try_new` validation non-finite
    /// + [0,1] range defensive; axis `try_new` kullanmalı.
    fn measure(&self, node: &Node, space: &Space) -> Result<AxisMeasurement, AxisMeasurementError>;

    /// Fallible value projection — `measure()` üzerinden value'yu döner.
    fn try_compute(&self, node: &Node, space: &Space) -> Result<f64, AxisMeasurementError> {
        Ok(self.measure(node, space)?.value)
    }

    /// **Legacy value-only projection.** Authority/evidence paths `measure()` kullanmalı.
    /// (`#[deprecated]` attribute Commit 4 cleanup'ta eklenecek — mevcut caller churn'ü
    /// Commit 1'de YOK.)
    fn compute(&self, node: &Node, space: &Space) -> f64;
}

/// INV-T9 Adım 3 — core raw axis ID'leri. Tek kaynak: hem `raw_position_of` mapping
/// hem `canonical_raw_axis_descriptors` filtresi bunu kullanır. Gelecekte core axis adı
/// değişirse ölçüm/digest kapsamı ayrışmaz.
const CORE_RAW_AXIS_IDS: [&str; 5] = [
    "coupling",
    "cohesion",
    "instability",
    "entropy",
    "witness_depth",
];

/// `id` core raw axis'lerinden biri mi? (raw_position_of + canonical_raw_axis_descriptors).
///
/// **pub(crate):** `authorization::MeasurementInputContext::try_from_parts` core-only
/// invariant'ı için de kullanır — dışarıdan custom axis descriptor context'e giremesin.
pub(crate) fn is_core_raw_axis_id(id: &str) -> bool {
    CORE_RAW_AXIS_IDS.contains(&id)
}

/// Koordinat sistemi — eksen koleksiyonu.
///
/// **INV-T9 Adım 3:** `axes` field private — dış erişim kapalı. Ekleme yalnız
/// `try_with_axis`/`register_axis` üzerinden (validated, duplicate reject). Koleksiyon
/// kapalı (closed collection) — invariant registration sırasında korunur.
pub struct CoordinateSystem {
    axes: Vec<Box<dyn Axis>>,
}

impl std::fmt::Debug for CoordinateSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoordinateSystem")
            .field("axes", &self.axes.len())
            .field("axis_names", &self.axis_names())
            .finish()
    }
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
    ///
    /// **INV-T9 Adım 3 (P1-c):** Mapping sınırı `CORE_RAW_AXIS_IDS` tek kaynağından
    /// `is_core_raw_axis_id` ile — `canonical_raw_axis_descriptors` filtresiyle aynı set.
    pub fn raw_position_of(&self, node: &Node, space: &Space) -> RawPosition {
        let mut pos = RawPosition::default();
        for axis in &self.axes {
            let name = axis.name();
            if !is_core_raw_axis_id(name) {
                continue; // custom axis — RawPosition'a dahil değil
            }
            let v = axis.compute(node, space);
            match name {
                "coupling" => pos.x = v,
                "cohesion" => pos.y = v,
                "instability" => pos.z = v,
                "entropy" => pos.w = v,
                "witness_depth" => pos.v = v,
                _ => unreachable!("is_core_raw_axis_id guarantees known name"),
            }
        }
        pos
    }

    /// **INV-T9 Adım 3:** Validated axis registration. Tüm doğrulama collection'a push'tan
    /// ÖNCE — hata durumunda koleksiyon kısmen mutate EDİLMEZ.
    ///
    /// Sıra: (1) runtime name boş mu? (2) duplicate name mi? (3) descriptor() üret,
    /// (4) descriptor.axis_id == axis.name() doğrula, (5) push.
    pub fn register_axis(&mut self, axis: Box<dyn Axis>) -> Result<(), AxisRegistrationError> {
        let runtime_name = axis.name();
        if runtime_name.is_empty() {
            return Err(AxisRegistrationError::EmptyAxisName);
        }
        if self.axes.iter().any(|a| a.name() == runtime_name) {
            return Err(AxisRegistrationError::DuplicateAxisId(
                runtime_name.to_owned(),
            ));
        }
        let descriptor = axis
            .descriptor()
            .map_err(AxisRegistrationError::DescriptorFailed)?;
        if descriptor.axis_id() != runtime_name {
            return Err(AxisRegistrationError::IdentityMismatch {
                runtime_name: runtime_name.to_owned(),
                descriptor_id: descriptor.axis_id().to_owned(),
            });
        }
        self.axes.push(axis);
        Ok(())
    }

    /// **INV-T9 Adım 3:** Builder — `register_axis`'e delegasyon. Duplicate mantığını
    /// tekrar yazmaz.
    pub fn try_with_axis<A: Axis + 'static>(
        mut self,
        axis: A,
    ) -> Result<Self, AxisRegistrationError> {
        self.register_axis(Box::new(axis))?;
        Ok(self)
    }

    /// **INV-T9 Adım 3:** Core raw axis descriptor'ları (seçenek B — yalnız 5 core axis).
    ///
    /// `raw_position_of` ile aynı name set (`CORE_RAW_AXIS_IDS`). Custom axis'ler yok
    /// sayılır — INV-T9 task evaluation yalnız core raw `RawPosition` semantiğini kullanır.
    ///
    /// **Defensive validation:** identity mismatch (axis_id vs runtime name) + duplicate
    /// axis_id reddi. Registration sırasında zaten doğrulanmış ama defensive tekrar
    /// (invariant drift tespiti). Hata tipi `AxisContextError` (registration DEĞİL).
    pub fn canonical_raw_axis_descriptors(&self) -> Result<Vec<AxisDescriptor>, AxisContextError> {
        let mut descriptors: Vec<AxisDescriptor> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for axis in &self.axes {
            let runtime_name = axis.name();
            if !is_core_raw_axis_id(runtime_name) {
                continue; // custom axis — core raw measurement context'e dahil değil
            }
            let descriptor = axis
                .descriptor()
                .map_err(AxisContextError::DescriptorFailed)?;
            if descriptor.axis_id() != runtime_name {
                return Err(AxisContextError::IdentityMismatch {
                    runtime_name: runtime_name.to_owned(),
                    descriptor_id: descriptor.axis_id().to_owned(),
                });
            }
            if seen.iter().any(|id| id == descriptor.axis_id()) {
                return Err(AxisContextError::DuplicateAxisId(
                    descriptor.axis_id().to_owned(),
                ));
            }
            seen.push(descriptor.axis_id().to_owned());
            descriptors.push(descriptor);
        }
        Ok(descriptors)
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
        fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
            // Test axis'i — parametresiz, formula marker 0.
            let mut params = AxisParameterEncoder::new();
            params.push_u8(0);
            AxisDescriptor::try_new(self.name, 1, params)
        }
        fn measure(
            &self,
            _node: &Node,
            _space: &Space,
        ) -> Result<AxisMeasurement, AxisMeasurementError> {
            AxisMeasurement::try_new(self.value, MetricSource::Placeholder)
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
            .try_with_axis(ConstantAxis {
                name: "a",
                value: 0.1,
            })
            .unwrap()
            .try_with_axis(ConstantAxis {
                name: "b",
                value: 0.2,
            })
            .unwrap()
            .try_with_axis(ConstantAxis {
                name: "c",
                value: 0.3,
            })
            .unwrap();
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
            .try_with_axis(ConstantAxis {
                name: "entropy",
                value: 0.5,
            })
            .unwrap() // → w
            .try_with_axis(ConstantAxis {
                name: "coupling",
                value: 0.7,
            })
            .unwrap() // → x
            .try_with_axis(ConstantAxis {
                name: "witness_depth",
                value: 0.3,
            })
            .unwrap(); // → v
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
        let cs = CoordinateSystem::empty()
            .try_with_axis(ConstantAxis {
                name: "security",
                value: 0.99,
            })
            .unwrap();
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
            fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
                let mut params = AxisParameterEncoder::new();
                params.push_u8(0);
                AxisDescriptor::try_new(self.name(), 1, params)
            }
            fn measure(
                &self,
                _node: &Node,
                space: &Space,
            ) -> Result<AxisMeasurement, AxisMeasurementError> {
                AxisMeasurement::try_new(
                    (space.node_count() as f64 / 100.0).min(1.0),
                    MetricSource::Placeholder,
                )
            }
            fn compute(&self, _node: &Node, space: &Space) -> f64 {
                (space.node_count() as f64 / 100.0).min(1.0)
            }
        }

        let mut space = Space::new();
        for i in 0..50 {
            space.insert_node(node(i));
        }
        let cs = CoordinateSystem::empty()
            .try_with_axis(NodeCountAxis)
            .unwrap();
        let pos = cs.position_of(&node(0), &space);
        assert!((pos[0] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn builder_chain_compiles_and_works() {
        let cs = CoordinateSystem::empty()
            .try_with_axis(ConstantAxis {
                name: "a",
                value: 0.0,
            })
            .unwrap()
            .try_with_axis(ConstantAxis {
                name: "b",
                value: 1.0,
            })
            .unwrap();
        assert_eq!(cs.dim(), 2);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Adım 3 — AxisDescriptor / AxisParameterEncoder / registration testleri
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn axis_descriptor_rejects_empty_axis_id() {
        let mut params = AxisParameterEncoder::new();
        params.push_u8(0);
        let err = AxisDescriptor::try_new("", 1, params).unwrap_err();
        assert_eq!(err, AxisDescriptorError::EmptyAxisId);
    }

    #[test]
    fn axis_descriptor_rejects_zero_semantics_version() {
        let mut params = AxisParameterEncoder::new();
        params.push_u8(0);
        let err = AxisDescriptor::try_new("coupling", 0, params).unwrap_err();
        assert_eq!(err, AxisDescriptorError::InvalidSemanticsVersion(0));
    }

    #[test]
    fn axis_descriptor_rejects_non_finite_parameter() {
        let mut params = AxisParameterEncoder::new();
        let err = params.push_f64(f64::NAN).unwrap_err();
        assert_eq!(err, AxisDescriptorError::NonFiniteParameter);
        let err = params.push_f64(f64::INFINITY).unwrap_err();
        assert_eq!(err, AxisDescriptorError::NonFiniteParameter);
    }

    #[test]
    fn axis_descriptor_normalizes_negative_zero() {
        // -0.0 ve +0.0 aynı canonical byte üretmeli (axis parameter encoder).
        let mut p_neg = AxisParameterEncoder::new();
        p_neg.push_f64(-0.0f64).unwrap();
        let mut p_pos = AxisParameterEncoder::new();
        p_pos.push_f64(0.0f64).unwrap();
        assert_eq!(
            p_neg.finish(),
            p_pos.finish(),
            "-0.0 and +0.0 must normalize"
        );
    }

    #[test]
    fn axis_descriptor_deserialization_rejects_empty_axis_id() {
        let wire_json = r#"{"axis_id":"","semantics_version":1,"canonical_parameters":[]}"#;
        let err = serde_json::from_str::<AxisDescriptor>(wire_json).unwrap_err();
        assert!(
            err.to_string().contains("empty axis_id"),
            "deserialize must reject empty axis_id: {err}"
        );
    }

    #[test]
    fn axis_descriptor_deserialization_rejects_zero_semantics_version() {
        let wire_json = r#"{"axis_id":"coupling","semantics_version":0,"canonical_parameters":[]}"#;
        let err = serde_json::from_str::<AxisDescriptor>(wire_json).unwrap_err();
        assert!(
            err.to_string().contains("invalid semantics_version"),
            "deserialize must reject semver 0: {err}"
        );
    }

    #[test]
    fn axis_descriptor_deserialization_preserves_canonical_bytes() {
        // Opaque canonical_parameters byte'ları deserialize sırasında korunur.
        let original = AxisDescriptor::try_new("entropy", 1, {
            let mut p = AxisParameterEncoder::new();
            p.push_u8(2);
            p.push_f64(0.42).unwrap();
            p
        })
        .unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let restored: AxisDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored, "canonical bytes must round-trip");
    }

    #[test]
    fn coordinate_system_rejects_duplicate_axis_at_registration() {
        let err = CoordinateSystem::empty()
            .try_with_axis(ConstantAxis {
                name: "coupling",
                value: 0.5,
            })
            .unwrap()
            .try_with_axis(ConstantAxis {
                name: "coupling",
                value: 0.7,
            })
            .unwrap_err();
        assert_eq!(
            err,
            AxisRegistrationError::DuplicateAxisId("coupling".into())
        );
    }

    #[test]
    fn coordinate_system_rejects_empty_axis_name() {
        let err = CoordinateSystem::empty()
            .try_with_axis(ConstantAxis {
                name: "",
                value: 0.5,
            })
            .unwrap_err();
        assert_eq!(err, AxisRegistrationError::EmptyAxisName);
    }

    #[test]
    fn coordinate_system_register_axis_failure_does_not_mutate_collection() {
        // İlk axis başarılı, ikinci (duplicate) başarısız → collection yalnız 1 axis.
        let mut cs = CoordinateSystem::empty()
            .try_with_axis(ConstantAxis {
                name: "coupling",
                value: 0.5,
            })
            .unwrap();
        let err = cs
            .register_axis(Box::new(ConstantAxis {
                name: "coupling",
                value: 0.9,
            }))
            .unwrap_err();
        assert_eq!(
            err,
            AxisRegistrationError::DuplicateAxisId("coupling".into())
        );
        assert_eq!(
            cs.dim(),
            1,
            "failed registration must not mutate collection"
        );
    }

    #[test]
    fn coordinate_system_rejects_axis_descriptor_identity_mismatch() {
        // Axis runtime name "coupling" ama descriptor "entropy" beyan ediyor → mismatch.
        struct MismatchedAxis;
        impl Axis for MismatchedAxis {
            fn name(&self) -> &'static str {
                "coupling"
            }
            fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
                AxisDescriptor::try_new("entropy", 1, AxisParameterEncoder::new())
            }
            fn measure(
                &self,
                _: &Node,
                _: &Space,
            ) -> Result<AxisMeasurement, AxisMeasurementError> {
                AxisMeasurement::try_new(0.0, MetricSource::Placeholder)
            }
            fn compute(&self, _: &Node, _: &Space) -> f64 {
                0.0
            }
        }
        let err = CoordinateSystem::empty()
            .try_with_axis(MismatchedAxis)
            .unwrap_err();
        match err {
            AxisRegistrationError::IdentityMismatch {
                runtime_name,
                descriptor_id,
            } => {
                assert_eq!(runtime_name, "coupling");
                assert_eq!(descriptor_id, "entropy");
            }
            other => panic!("expected IdentityMismatch, got {other:?}"),
        }
    }

    #[test]
    fn axis_descriptor_is_deterministic_for_unchanged_axis_state() {
        // Aynı immutable axis → tekrar tekrar descriptor() aynı sonucu vermeli.
        let axis = ConstantAxis {
            name: "coupling",
            value: 0.5,
        };
        let d1 = axis.descriptor().unwrap();
        let d2 = axis.descriptor().unwrap();
        let d3 = axis.descriptor().unwrap();
        assert_eq!(d1, d2);
        assert_eq!(d2, d3);
    }

    #[test]
    fn ignored_custom_axis_does_not_change_core_raw_measurement_digest() {
        // Seçenek B — custom axis "security" core raw descriptor listesine dahil edilmez.
        // Aynı 5 core axis + farklı custom axis → aynı canonical_raw_axis_descriptors.
        let cs_core_only = CoordinateSystem::empty()
            .try_with_axis(ConstantAxis {
                name: "coupling",
                value: 0.5,
            })
            .unwrap();
        let cs_with_custom = CoordinateSystem::empty()
            .try_with_axis(ConstantAxis {
                name: "coupling",
                value: 0.5,
            })
            .unwrap()
            .try_with_axis(ConstantAxis {
                name: "security",
                value: 0.99,
            })
            .unwrap();
        let core = cs_core_only.canonical_raw_axis_descriptors().unwrap();
        let with_custom = cs_with_custom.canonical_raw_axis_descriptors().unwrap();
        assert_eq!(
            core, with_custom,
            "custom axis must be filtered out of core raw descriptors"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 — MetricSource::Mixed + descriptor_id + AxisMeasurement validation
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn metric_source_descriptor_id_stable_bytes() {
        // P1-1 — stable byte ID (canonical tag DEĞİL). coords → canonical_tags bağımlılık YOK.
        assert_eq!(MetricSource::TreeSitter.descriptor_id(), b"tree-sitter");
        assert_eq!(MetricSource::Scip.descriptor_id(), b"scip");
        assert_eq!(MetricSource::Placeholder.descriptor_id(), b"placeholder");
        assert_eq!(MetricSource::Heuristic.descriptor_id(), b"heuristic");
        assert_eq!(MetricSource::Mixed.descriptor_id(), b"mixed");
    }

    #[test]
    fn metric_source_display_includes_mixed() {
        assert_eq!(MetricSource::Mixed.to_string(), "mixed");
        assert_eq!(MetricSource::TreeSitter.to_string(), "tree-sitter");
    }

    #[test]
    fn axis_measurement_try_new_accepts_valid_range() {
        assert!(AxisMeasurement::try_new(0.0, MetricSource::Scip).is_ok());
        assert!(AxisMeasurement::try_new(1.0, MetricSource::Scip).is_ok());
        assert!(AxisMeasurement::try_new(0.5, MetricSource::Scip).is_ok());
    }

    #[test]
    fn axis_measurement_try_new_rejects_non_finite() {
        assert_eq!(
            AxisMeasurement::try_new(f64::NAN, MetricSource::Scip).unwrap_err(),
            AxisMeasurementError::NonFiniteValue
        );
        assert_eq!(
            AxisMeasurement::try_new(f64::INFINITY, MetricSource::Scip).unwrap_err(),
            AxisMeasurementError::NonFiniteValue
        );
        assert_eq!(
            AxisMeasurement::try_new(f64::NEG_INFINITY, MetricSource::Scip).unwrap_err(),
            AxisMeasurementError::NonFiniteValue
        );
    }

    #[test]
    fn axis_measurement_try_new_rejects_out_of_range() {
        let err = AxisMeasurement::try_new(-0.1, MetricSource::Scip).unwrap_err();
        assert!(matches!(err, AxisMeasurementError::OutOfRange(v) if (v + 0.1).abs() < 1e-9));
        let err = AxisMeasurement::try_new(1.1, MetricSource::Scip).unwrap_err();
        assert!(matches!(err, AxisMeasurementError::OutOfRange(v) if (v - 1.1).abs() < 1e-9));
    }

    #[test]
    fn axis_measurement_try_new_accepts_mixed_source() {
        // Mixed constructor guard yalnız axis constructor'larında; AxisMeasurement
        // aggregation çıktısı için Mixed kabul eder (Commit 2 `aggregate_source`).
        let m = AxisMeasurement::try_new(0.5, MetricSource::Mixed).unwrap();
        assert_eq!(m.source, MetricSource::Mixed);
    }

    #[test]
    fn validate_direct_source_rejects_mixed() {
        assert_eq!(
            validate_direct_source(MetricSource::Mixed).unwrap_err(),
            AxisSourceError::MixedCannotBeDeclaredDirectly
        );
        assert_eq!(
            validate_direct_source(MetricSource::TreeSitter).unwrap(),
            MetricSource::TreeSitter
        );
        assert_eq!(
            validate_direct_source(MetricSource::Scip).unwrap(),
            MetricSource::Scip
        );
    }

    #[test]
    fn axis_measurement_deserialize_rejects_json_null_value() {
        // JSON null value → type mismatch (f64 bekliyor). Wire integrity için geçerli
        // rejection test. NaN binary pattern test'i için `axis_measurement_bincode_deserialize_rejects_nan`.
        let json = r#"{"value":null,"source":"Scip"}"#;
        let res: Result<AxisMeasurement, _> = serde_json::from_str(json);
        assert!(
            res.is_err(),
            "JSON null value must be rejected on deserialize"
        );
    }

    #[test]
    fn axis_measurement_deserialize_rejects_unknown_field() {
        // P1-2 — deny_unknown_fields: strict authority surface.
        let json = r#"{"value":0.5,"source":"Scip","unrecognized_authority":true}"#;
        let err = serde_json::from_str::<AxisMeasurement>(json).unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "unknown field must be rejected: {err}"
        );
    }

    #[test]
    fn axis_measurement_deserialize_rejects_out_of_range() {
        let json = r#"{"value":2.0,"source":"Scip"}"#;
        let err = serde_json::from_str::<AxisMeasurement>(json).unwrap_err();
        assert!(
            err.to_string().contains("out of range"),
            "value=2.0 must be rejected: {err}"
        );
    }

    #[test]
    fn axis_measurement_deserialize_rejects_unknown_source_variant() {
        // JSON type-safe source enum — unknown variant rejected by serde derive.
        let json = r#"{"value":0.5,"source":"Bogus"}"#;
        let err = serde_json::from_str::<AxisMeasurement>(json).unwrap_err();
        assert!(
            err.to_string().contains("unknown variant") || err.to_string().contains("Bogus"),
            "unknown source variant must be rejected: {err}"
        );
    }

    #[test]
    fn axis_measurement_bincode_deserialize_rejects_nan() {
        // P2-1 (review v6): JSON null test NaN test etmiyordu — bincode gerçek NaN
        // bit pattern'ini taşıyabilir. Custom Deserialize → try_new() zinciri NaN'ı
        // reddetmeli (binary snapshot integrity — Faz 2 event-sourcing).
        let forged = AxisMeasurement {
            value: f64::NAN,
            source: MetricSource::Scip,
        };
        let bytes = bincode::serialize(&forged).unwrap();
        let err = bincode::deserialize::<AxisMeasurement>(&bytes).unwrap_err();
        assert!(
            err.to_string().contains("non-finite"),
            "NaN value must be rejected on bincode deserialize: {err}"
        );
    }

    #[test]
    fn axis_measurement_bincode_deserialize_rejects_out_of_range() {
        // P2-1: Bincode 2.0 gibi NaN-dışı validation da reject olmalı.
        let forged = AxisMeasurement {
            value: 2.0,
            source: MetricSource::Scip,
        };
        let bytes = bincode::serialize(&forged).unwrap();
        let err = bincode::deserialize::<AxisMeasurement>(&bytes).unwrap_err();
        assert!(
            err.to_string().contains("out of range"),
            "value=2.0 must be rejected on bincode deserialize: {err}"
        );
    }

    #[test]
    fn axis_measurement_bincode_round_trip_valid() {
        // Regression: valid AxisMeasurement bincode round-trip stabil.
        let original = AxisMeasurement::try_new(0.7, MetricSource::Scip).unwrap();
        let bytes = bincode::serialize(&original).unwrap();
        let restored: AxisMeasurement = bincode::deserialize(&bytes).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn axis_measurement_deserialize_round_trip() {
        let original = AxisMeasurement::try_new(0.7, MetricSource::Scip).unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let restored: AxisMeasurement = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn measured_raw_position_to_raw_projects_values_only() {
        let mrp = MeasuredRawPosition {
            coupling: AxisMeasurement::try_new(0.1, MetricSource::TreeSitter).unwrap(),
            cohesion: AxisMeasurement::try_new(0.2, MetricSource::Scip).unwrap(),
            instability: AxisMeasurement::try_new(0.3, MetricSource::TreeSitter).unwrap(),
            entropy: AxisMeasurement::try_new(0.4, MetricSource::Heuristic).unwrap(),
            witness_depth: AxisMeasurement::try_new(0.5, MetricSource::Heuristic).unwrap(),
        };
        let raw = mrp.to_raw();
        assert!((raw.x - 0.1).abs() < 1e-9);
        assert!((raw.y - 0.2).abs() < 1e-9);
        assert!((raw.z - 0.3).abs() < 1e-9);
        assert!((raw.w - 0.4).abs() < 1e-9);
        assert!((raw.v - 0.5).abs() < 1e-9);
    }
}
