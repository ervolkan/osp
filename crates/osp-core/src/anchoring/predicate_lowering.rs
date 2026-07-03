//! Predicate lowering — RuleCandidate → PredicateStub (Faz 5a, INV-P1, D16).
//!
//! # Ana tez
//! *A rule is not a predicate. A predicate is a rule whose measurable slots have
//! been bound.* — `RuleCandidate` insan niyeti seviyesinde, `PredicateSet` (Paper 2)
//! çalıştırılabilir ölçüm seviyesinde. Arada `PredicateStub` epistemik tampon.
//!
//! # INV-P1 (yeni, D16)
//! Ölçülebilir slotları bağlanmamış RuleCandidate, ExecutablePredicateSet üretemez.
//! - **INV-P1a (PR33a):** RuleCandidate lowering `PredicateStub` üretir,
//!   ExecutablePredicateSet **DEĞİL**.
//! - **INV-P1b (PR33b):** PredicateStub → ExecutablePredicateSet sadece slot binding
//!   (operator/evidence-backed) ile.
//!
//! # Structured uncertainty
//! `PredicateStub` boş bir "bilmiyorum" DEĞİL — neyi bilmediğini (`unresolved_slots`),
//! neden bilmediğini (`reason`), hangi kalıplara uyabileceğini (`suggested_templates`)
//! ölçülü şekilde temsil eder. *"A PredicateStub is not absence of knowledge; it is
//! structured uncertainty."*
//!
//! # PR33a kapsamı
//! Bu modül sadece `PredicateStub` üretir. Navigator bağlantısı, executable predicate,
//! slot binding hepsi PR33b'ye. `lower_rule_to_predicate_stub` her zaman `Stub` döner.

use crate::anchoring::types::{ConceptNode, ConceptNodeId};
use crate::anchoring::ConceptNodeKind;

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateSlot — ölçülebilir slot (Patch 5 serde: Serialize + Deserialize)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir predicate'in ölçülebilir slot'u (henüz bağlı olmayan parametre).
///
/// Patch 5 serde politikası: `Serialize + Deserialize` (operator console slot seçimi
/// JSON ile gelebilir). `PredicateStub` ise Serialize-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PredicateSlot {
    /// Hangi metric? (coupling/cohesion/instability/...)
    Metric,
    /// Hangi eşik? (0.55 / repo-average / ...)
    Threshold,
    /// Hangi kapsam? (hangi modül/node/subgraph)
    Scope,
    /// Hangi karşılaştırma? (< / ≤ / > / ≥)
    Comparator,
}

/// Tüm slot evreni (PR33a — 4 slot). `completeness()` için sabit.
pub const ALL_SLOTS: [PredicateSlot; 4] = [
    PredicateSlot::Metric,
    PredicateSlot::Threshold,
    PredicateSlot::Scope,
    PredicateSlot::Comparator,
];

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateTemplateId — önerilen template (Patch 5 serde: Serialize + Deserialize)
// ═══════════════════════════════════════════════════════════════════════════════

/// PR33a'da sadece ID/stub — executable logic PR33b. Rule canonical'ından keyword
/// mapping ile önerilir; ama **executable predicate üretmez** (sadece "bu template
/// önerildi" der).
///
/// Patch 5 serde politikası: `Serialize + Deserialize` (operator console template
/// seçimi JSON ile).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PredicateTemplateId {
    /// `metric(target, coupling) < threshold` — coupling/cohesion/instability eşik.
    MetricThreshold,
    /// `metric_after < metric_before` — progress checkpoint (Paper 2 loss azalma).
    MetricDelta,
    /// edge/claim için evidence var mı (Faz 4 ObservedCodeEvidence'e bağlanır).
    EvidenceRequired,
    /// `Concept --ImplementedBy--> CodeEntity` var mı (Faz 4'e bağlanır).
    RelationExists,
}

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateStubReason — neden executable değil
// ═══════════════════════════════════════════════════════════════════════════════

/// Stub'ın executable olmadığının nedeni.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum PredicateStubReason {
    /// "coupling" mi "instability" mi net değil.
    MetricUnresolved,
    /// 0.55 mi repo-average mi net değil.
    ThresholdUnresolved,
    /// Hangi modül/node net değil.
    ScopeUnresolved,
    /// < mi ≤ mi net değil.
    ComparatorUnresolved,
    /// Hiçbir template uymadı (suggested_templates boş olmalı).
    NoTemplateMatch,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 5b — PhysicalCodeMetricAxis (Patch 5: aileyi açık eden isim)
// ═══════════════════════════════════════════════════════════════════════════════

/// PhysicalCode metric ekseni — Paper 1'in ölçülebilir fiziksel eksenleri (Patch 5).
///
/// `trajectory::PredicateAxis` yerine Faz 5b'de kullanılır çünkü:
/// - **Cross-family riskini type-level vurgular** (ConceptualIntent → PhysicalCode).
/// - PhysicalCode subset'i (Coupling/Cohesion/Instability/Entropy/WitnessDepth) sınırlar;
///   `RiskScore`/`MainSequenceDistance`/`Custom` derived eksenler Faz 5.1'e.
/// - `bind_metric_threshold` axis mismatch kontrolü (Kontrol 5) bu tip ile yapılır.
///
/// INV-P2: keyword hint bu ekseni önerebilir, ama executable predicate için operator
/// binding zorunlu. *"A conceptual rule may suggest a physical metric, but only bound
/// slots can create an executable predicate."*
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PhysicalCodeMetricAxis {
    Coupling,
    Cohesion,
    Instability,
    Entropy,
    WitnessDepth,
}

impl PhysicalCodeMetricAxis {
    /// `trajectory::PredicateAxis`'e map (Faz 5b PhysicalCode subset).
    pub fn to_predicate_axis(self) -> crate::trajectory::PredicateAxis {
        match self {
            Self::Coupling => crate::trajectory::PredicateAxis::Coupling,
            Self::Cohesion => crate::trajectory::PredicateAxis::Cohesion,
            Self::Instability => crate::trajectory::PredicateAxis::Instability,
            Self::Entropy => crate::trajectory::PredicateAxis::Entropy,
            Self::WitnessDepth => crate::trajectory::PredicateAxis::WitnessDepth,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 5b — NormalizedMetricThreshold (Patch 3: [0,1] range-checked newtype)
// ═══════════════════════════════════════════════════════════════════════════════

/// Normalize edilmiş physical coordinate threshold `[0,1]` (Patch 3 + D1 öneri 3 isim).
///
/// Paper 1 eksenleri (coupling/cohesion/instability/entropy) normalize edilmiştir;
/// WitnessDepth gibi raw değer eksenleri için gelecekte ayrı tip (Faz 5.1). Şimdilik
/// `[0,1]` yeterli — `EvidenceStrength`/`ScalarSimilarity` paterni (is_finite + range).
///
/// # INV-P2 serde hijyeni
/// Custom `Deserialize` — `serde_json::from_str("2.0")` reject. Constructor bypass
/// edilemez (EvidenceStrength/ScalarSimilarity ile aynı standard).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalizedMetricThreshold(f64);

impl NormalizedMetricThreshold {
    /// `[0,1]` range-check + finiteness. NaN, ±∞, negatif, >1 → error.
    pub fn new(value: f64) -> Result<Self, NormalizedMetricThresholdError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(NormalizedMetricThresholdError { value })
        }
    }
    pub fn get(&self) -> f64 {
        self.0
    }
}

impl serde::Serialize for NormalizedMetricThreshold {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for NormalizedMetricThreshold {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        NormalizedMetricThreshold::new(value).map_err(serde::de::Error::custom)
    }
}

/// `NormalizedMetricThreshold` değer aralığı dışı hatası.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalizedMetricThresholdError {
    pub value: f64,
}

impl std::fmt::Display for NormalizedMetricThresholdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NormalizedMetricThreshold [0,1] dışı veya non-finite: {} (INV-P2)",
            self.value
        )
    }
}

impl std::error::Error for NormalizedMetricThresholdError {}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 5b — MetricThresholdBinding (Patch 4: private + smart ctor)
// ═══════════════════════════════════════════════════════════════════════════════

/// Operator-bound MetricThreshold slot binding (Patch 4 — private fields + smart ctor).
///
/// PredicateStub'un Metric/Threshold/Scope/Comparator slot'larını bağlar → ExecutablePredicateSet.
/// Faz 4/5a paterni: private fields + `new()` smart constructor. Literal construct engelli.
///
/// # INV-P2
/// Sadece `bind_metric_threshold(stub, binding, cap)` ile executable predicate üretir.
/// Axis, stub'ın `suggested_axis` ile uyuşmalı (Kontrol 5 — mismatch reject).
#[derive(Debug, Clone, PartialEq)]
pub struct MetricThresholdBinding {
    axis: PhysicalCodeMetricAxis,
    scope: crate::trajectory::PredicateScope,
    comparator: crate::trajectory::ComparisonOp,
    threshold: NormalizedMetricThreshold,
}

impl MetricThresholdBinding {
    /// Public smart constructor (Patch 4). Tüm slot'lar operator tarafından bağlanır.
    pub fn new(
        axis: PhysicalCodeMetricAxis,
        scope: crate::trajectory::PredicateScope,
        comparator: crate::trajectory::ComparisonOp,
        threshold: NormalizedMetricThreshold,
    ) -> Self {
        Self {
            axis,
            scope,
            comparator,
            threshold,
        }
    }
    pub fn axis(&self) -> PhysicalCodeMetricAxis {
        self.axis
    }
    pub fn scope(&self) -> &crate::trajectory::PredicateScope {
        &self.scope
    }
    pub fn comparator(&self) -> crate::trajectory::ComparisonOp {
        self.comparator
    }
    pub fn threshold(&self) -> NormalizedMetricThreshold {
        self.threshold
    }
}

/// `bind_metric_threshold` hatası (INV-P2 — executable predicate boundary).
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum BindingError {
    #[error("stub MetricThreshold template önermiyor — bind edilemez")]
    TemplateNotSuggested,
    #[error("axis mismatch: stub {stub_axis:?}, binding {binding_axis:?}")]
    AxisMismatch {
        stub_axis: Option<PhysicalCodeMetricAxis>,
        binding_axis: PhysicalCodeMetricAxis,
    },
}

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateStub — structured uncertainty (Patch 1/2, Faz 4 ObservedCodeEvidence paterni)
// ═══════════════════════════════════════════════════════════════════════════════

/// Rule'ın predicate olmak için ne eksik olduğu — INV-P1 structured uncertainty.
///
/// # Yapısal garanti (Patch 1)
/// Private field'lar + public smart constructor `new`. Dış crate literal construct
/// edemez (trybuild `cP1_predicate_stub_literal`); ama `new()` ile geçerli stub
/// üretebilir (operator console / bridge). Faz 4 `ObservedCodeEvidence` paterni.
///
/// # Non-empty invariant (Patch 2 — structured uncertainty type-level)
/// Stub **gerçekten boş değil** — consistency kontrolü:
/// - `unresolved_slots` boş VE `reason != NoTemplateMatch` → `EmptyUnresolvedSlots`.
/// - `reason == NoTemplateMatch` VE `suggested_templates` dolu →
///   `NoTemplateMatchCannotSuggestTemplate` (çelişki).
/// *"A PredicateStub is not absence of knowledge; it is structured uncertainty."*
///
/// # Serde boundary (Patch 5)
/// `Serialize`-only (audit). `Deserialize` YOK — stub yeniden apply edilememeli
/// (PR30/Faz4 serde boundary paterni). `PredicateSlot`/`PredicateTemplateId` ayrı
/// (Serialize + Deserialize — operator console seçim).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PredicateStub {
    rule_id: ConceptNodeId,
    reason: PredicateStubReason,
    unresolved_slots: Vec<PredicateSlot>,
    suggested_templates: Vec<PredicateTemplateId>,
    /// Faz 5b — PhysicalCode axis hint (cross-family translation, INV-P2).
    /// `None` = hint yok. `bind_metric_threshold` axis mismatch kontrolü (Kontrol 5).
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_axis: Option<PhysicalCodeMetricAxis>,
}

/// `PredicateStub::new` consistency hatası (Patch 2).
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum PredicateStubError {
    #[error("unresolved_slots boş ama reason NoTemplateMatch değil — stub boş olamaz")]
    EmptyUnresolvedSlots,
    #[error("reason NoTemplateMatch ama suggested_templates dolu — çelişki")]
    NoTemplateMatchCannotSuggestTemplate,
}

impl PredicateStub {
    /// Public smart constructor — Patch 2 consistency kontrolü (PR33a API, backward-compat).
    ///
    /// Faz 5b (Kontrol 6): axis hint olmadan — `new_with_axis_hint(..., None)`'e delegate.
    /// PR33a callers bu imzayı kullanmaya devam eder.
    pub fn new(
        rule_id: ConceptNodeId,
        reason: PredicateStubReason,
        unresolved_slots: Vec<PredicateSlot>,
        suggested_templates: Vec<PredicateTemplateId>,
    ) -> Result<Self, PredicateStubError> {
        Self::new_with_axis_hint(rule_id, reason, unresolved_slots, suggested_templates, None)
    }

    /// Faz 5b — axis hint ile constructor (Kontrol 6, cross-family translation INV-P2).
    ///
    /// `lower_rule_to_predicate_stub` PR33b'de "coupling" gibi keyword'lere PhysicalCode
    /// axis hint ekler. `bind_metric_threshold` axis mismatch kontrolü (Kontrol 5) için.
    pub fn new_with_axis_hint(
        rule_id: ConceptNodeId,
        reason: PredicateStubReason,
        unresolved_slots: Vec<PredicateSlot>,
        suggested_templates: Vec<PredicateTemplateId>,
        suggested_axis: Option<PhysicalCodeMetricAxis>,
    ) -> Result<Self, PredicateStubError> {
        if unresolved_slots.is_empty() && !matches!(reason, PredicateStubReason::NoTemplateMatch) {
            return Err(PredicateStubError::EmptyUnresolvedSlots);
        }
        if matches!(reason, PredicateStubReason::NoTemplateMatch) && !suggested_templates.is_empty()
        {
            return Err(PredicateStubError::NoTemplateMatchCannotSuggestTemplate);
        }
        Ok(Self {
            rule_id,
            reason,
            unresolved_slots,
            suggested_templates,
            suggested_axis,
        })
    }

    pub fn rule_id(&self) -> &ConceptNodeId {
        &self.rule_id
    }
    pub fn reason(&self) -> PredicateStubReason {
        self.reason
    }
    pub fn unresolved_slots(&self) -> &[PredicateSlot] {
        &self.unresolved_slots
    }
    pub fn suggested_templates(&self) -> &[PredicateTemplateId] {
        &self.suggested_templates
    }
    /// Faz 5b — PhysicalCode axis hint (None = hint yok).
    pub fn suggested_axis(&self) -> Option<PhysicalCodeMetricAxis> {
        self.suggested_axis
    }

    /// Çözülmüş slot oranı `[0,1]` (D2 öneri 1, Patch 4 sabit formül).
    ///
    /// ```text
    /// NoTemplateMatch → 0.0
    /// otherwise → 1.0 - (unresolved_slots.len() / ALL_SLOTS.len())
    /// ```
    /// Tüm slot'lar unresolved → 0.0; 2 slot unresolved → 0.5. Operator önceliklendirme
    /// için. PR33b'de template-specific slot universe gelebilir.
    pub fn completeness(&self) -> f64 {
        if matches!(self.reason, PredicateStubReason::NoTemplateMatch) {
            return 0.0;
        }
        let total = ALL_SLOTS.len() as f64;
        let unresolved = self.unresolved_slots.len() as f64;
        (1.0 - unresolved / total).clamp(0.0, 1.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PredicateLoweringOutcome — PR33a'da sadece Stub (Patch 3)
// ═══════════════════════════════════════════════════════════════════════════════

/// RuleCandidate lowering sonucu. PR33a'da **her zaman `Stub`** (Patch 3).
/// `RequiresOperatorBinding(UnresolvedPredicateBinding)` PR33b'ye.
#[derive(Debug, Clone, PartialEq)]
pub enum PredicateLoweringOutcome {
    /// PR33a — Rule'ın predicate olmak için eksikleri (structured uncertainty).
    Stub(PredicateStub),
    // PR33b: RequiresOperatorBinding(UnresolvedPredicateBinding),
}

/// `lower_rule_to_predicate_stub` hatası (Son Patch 1).
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum PredicateLoweringError {
    #[error("node RuleCandidate değil: {node_id}")]
    NotRuleCandidate { node_id: ConceptNodeId },
    #[error("stub construct hatası: {0}")]
    InvalidStub(PredicateStubError),
}

// ═══════════════════════════════════════════════════════════════════════════════
// lower_rule_to_predicate_stub — lowering fonksiyonu (Son Patch 1: Result döner)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir RuleCandidate node'unu PredicateStub'a lower et (INV-P1a).
///
/// # INV-P1 (Son Patch 1/2)
/// - Sadece `ConceptNodeKind::RuleCandidate` lowering'e girebilir. Başka kind verilirse
///   `NotRuleCandidate` hatası. *"Sadece RuleCandidate lowering'e girebilir.
///   RuleCandidate bile PredicateSet üretemez; sadece PredicateStub üretir."*
/// - PR33a'da **her zaman Stub** döner — executable predicate üretmez (INV-P1a).
///
/// # Deterministic suggested_templates (cross-family translation yok)
/// Rule canonical'ından keyword'lere göre template **önerilir** (coupling →
/// MetricThreshold, evidence → EvidenceRequired, decrease → MetricDelta, implemented →
/// RelationExists). Ama executable predicate üretmez — sadece "bu template önerildi".
/// Tüm slot'lar (metric/threshold/scope/comparator) unresolved kalır; operator
/// bağlayacak (PR33b).
///
/// # Scope (PR33a)
/// NLP yok — sadece canonical string keyword eşleştirme. No template match ise
/// `reason: NoTemplateMatch`, `suggested_templates: []` (tek geçerli boş durum).
pub fn lower_rule_to_predicate_stub(
    rule_candidate: &ConceptNode,
) -> Result<PredicateLoweringOutcome, PredicateLoweringError> {
    // Son Patch 1: kind kontrolü — sadece RuleCandidate.
    if !matches!(rule_candidate.node_kind, ConceptNodeKind::RuleCandidate) {
        return Err(PredicateLoweringError::NotRuleCandidate {
            node_id: rule_candidate.id.clone(),
        });
    }

    let canonical_lower = rule_candidate.canonical.to_lowercase();

    // Deterministic keyword → suggested_templates (öneri, executable değil).
    let mut suggested: Vec<PredicateTemplateId> = Vec::new();
    // Eksen keyword'lerini ayrı topla — çoklu eksen tespiti için (INV-P2: belirsiz → None).
    let mut detected_axes: Vec<PhysicalCodeMetricAxis> = Vec::new();
    if canonical_lower.contains("coupling") || canonical_lower.contains("bağıml") {
        suggested.push(PredicateTemplateId::MetricThreshold);
        detected_axes.push(PhysicalCodeMetricAxis::Coupling);
    }
    if canonical_lower.contains("cohesion") {
        suggested.push(PredicateTemplateId::MetricThreshold);
        detected_axes.push(PhysicalCodeMetricAxis::Cohesion);
    }
    if canonical_lower.contains("instability") {
        suggested.push(PredicateTemplateId::MetricThreshold);
        detected_axes.push(PhysicalCodeMetricAxis::Instability);
    }
    if canonical_lower.contains("entropy") {
        suggested.push(PredicateTemplateId::MetricThreshold);
        detected_axes.push(PhysicalCodeMetricAxis::Entropy);
    }
    if canonical_lower.contains("witness") {
        suggested.push(PredicateTemplateId::MetricThreshold);
        detected_axes.push(PhysicalCodeMetricAxis::WitnessDepth);
    }
    if canonical_lower.contains("decrease")
        || canonical_lower.contains("reduce")
        || canonical_lower.contains("azalt")
        || canonical_lower.contains("düşür")
    {
        suggested.push(PredicateTemplateId::MetricDelta);
    }
    if canonical_lower.contains("evidence") || canonical_lower.contains("kanıt") {
        suggested.push(PredicateTemplateId::EvidenceRequired);
    }
    if canonical_lower.contains("implement") || canonical_lower.contains("implemente") {
        suggested.push(PredicateTemplateId::RelationExists);
    }
    // Dedup (coupling + cohesion aynı cümlede → MetricThreshold iki kez eklenmesin).
    suggested.dedup();
    detected_axes.dedup();

    // Faz 5b (INV-P2): MetricThreshold axis hint sadece tek bir eksen tespit edildiyse
    // ve MetricThreshold tek başına önerildiyse anlamlı. Çoklu/belirsiz axis → None
    // (operator kendi bağlar, mismatch reject). Diğer template'lerle karışık → None.
    let suggested_axis =
        if suggested == vec![PredicateTemplateId::MetricThreshold] && detected_axes.len() == 1 {
            detected_axes.first().copied()
        } else {
            None
        };

    // Tüm slot'lar unresolved (operator bağlayacak — PR33b). Metric/Threshold/Scope/
    // Comparator hepsi net değil; sadece template önerildi. NoTemplateMatch durumunda
    // suggested boş → unresolved_slots da boş (smart ctor consistency: NoTemplateMatch
    // + boş templates tek geçerli boş durum).
    let reason = if suggested.is_empty() {
        PredicateStubReason::NoTemplateMatch
    } else {
        PredicateStubReason::MetricUnresolved
    };

    let unresolved_slots = if matches!(reason, PredicateStubReason::NoTemplateMatch) {
        Vec::new()
    } else {
        vec![
            PredicateSlot::Metric,
            PredicateSlot::Threshold,
            PredicateSlot::Scope,
            PredicateSlot::Comparator,
        ]
    };

    let stub = PredicateStub::new_with_axis_hint(
        rule_candidate.id.clone(),
        reason,
        unresolved_slots,
        suggested,
        suggested_axis,
    )
    .map_err(PredicateLoweringError::InvalidStub)?;

    Ok(PredicateLoweringOutcome::Stub(stub))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 5b — ExecutablePredicateSet + bind_metric_threshold (INV-P1b, INV-P2)
// ═══════════════════════════════════════════════════════════════════════════════

/// Slot'ları bağlanmış, engine-measured koordinat üzerinde doğrulanabilir predicate set
/// (INV-P1b — PredicateStub'dan slot binding ile üretilir).
///
/// # Boundary (Patch 1, Kontrol 2)
/// - **Private inner** `trajectory::PredicateSet` + accessor. Literal construct engelli.
/// - **Tek üretim yolu** `bind_metric_threshold()` — public `new_empty()` YOK.
/// - **Non-empty by construction** — `bind_metric_threshold` her zaman ≥1 predicate üretir.
/// - **Serialize-only** (audit). Deserialize YOK — yeniden apply edilememeli (PR30/Faz4/5a
///   serde boundary paterni).
///
/// # INV-P2
/// *"A conceptual rule may suggest a physical metric, but only bound slots can create an
/// executable predicate."* — ExecutablePredicateSet, keyword hint değil, operator-bound
/// slot'ların sonucudur.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ExecutablePredicateSet {
    predicate_set: crate::trajectory::PredicateSet,
}

impl ExecutablePredicateSet {
    /// trajectory::PredicateSet'e dönüştür (create_task_from_accepted_candidate içeren).
    /// Consumes self — ExecutablePredicateSet bir kez kullanılır.
    pub fn into_trajectory_predicate_set(self) -> crate::trajectory::PredicateSet {
        self.predicate_set
    }
}

/// PredicateStub + operator-bound MetricThresholdBinding → ExecutablePredicateSet (INV-P1b).
///
/// # Üç kapılı API'nin 2. kapısı (D1/D2)
/// - **OperatorCapability-gated** (Patch 2) — `cap` body'de kullanılmaz (compile-time
///   token) ama imza zorunlu kılar. *"ExecutablePredicateSet operator capability olmadan doğmaz."*
///
/// # INV-P2 kontrolleri
/// - **Template kontrolü (Kontrol 4)**: stub MetricThreshold template önermiyorsa →
///   `TemplateNotSuggested`. Her PredicateStub MetricThreshold'a bind edilemez.
/// - **Axis mismatch kontrolü (Kontrol 5)**: stub'ın `suggested_axis` binding.axis ile
///   uyuşmuyorsa → `AxisMismatch`. Operator override Faz 5.1/Faz 8.
/// - **Non-empty (Kontrol 2)**: her zaman ≥1 WeightedPredicate üretir.
///
/// # keyword ≠ executable
/// "coupling azaltılmalı" → stub axis hint Coupling önerir, ama threshold/scope/comparator
/// hâlâ operator-bound. Bu fonksiyon çağrılınca executable olur.
pub fn bind_metric_threshold(
    stub: &PredicateStub,
    binding: MetricThresholdBinding,
    _cap: &crate::trajectory::OperatorCapability,
) -> Result<ExecutablePredicateSet, BindingError> {
    // Kontrol 4: MetricThreshold template önerilmeli.
    if !stub
        .suggested_templates()
        .contains(&PredicateTemplateId::MetricThreshold)
    {
        return Err(BindingError::TemplateNotSuggested);
    }
    // Kontrol 5: axis hint mismatch. Stub axis varsa binding ile uyuşmalı.
    // Stub axis None (çoklu/belirsiz) → operator herhangi bir axis bağlayabilir.
    if let Some(stub_axis) = stub.suggested_axis() {
        if stub_axis != binding.axis() {
            return Err(BindingError::AxisMismatch {
                stub_axis: Some(stub_axis),
                binding_axis: binding.axis(),
            });
        }
    }

    // MetricThreshold → trajectory::MetricPredicate (tüm slot'lar bağlanmış).
    let metric_predicate = crate::trajectory::MetricPredicate {
        metric: binding.axis().to_predicate_axis(),
        operator: binding.comparator(),
        threshold: binding.threshold().get(),
        scope: binding.scope().clone(),
        // INV-T4: Scip zorunlu — placeholder/heuristic ile task kapatma engellenir.
        required_source: Some(crate::coords::MetricSource::Scip),
        tolerance: 0.0,
    };
    let weighted = crate::trajectory::WeightedPredicate {
        predicate: metric_predicate,
        weight: None,
    };
    let predicate_set = crate::trajectory::PredicateSet {
        mode: crate::trajectory::PredicateMode::All,
        predicates: vec![weighted],
        preferred_vector: None,
    };

    Ok(ExecutablePredicateSet { predicate_set })
}

#[cfg(test)]
mod tests {
    //! predicate_lowering.rs unit testleri — smart ctor consistency (3), non-RuleCandidate
    //! reject, completeness formül, lowering outcome, serde boundary.

    use super::*;
    use crate::anchoring::ConceptNodeKind;

    fn rule_candidate(canonical: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(format!("RuleCandidate:{canonical}")),
            canonical: canonical.into(),
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: crate::anchoring::DecisionStatus::Candidate,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        }
    }

    fn concept_node(kind: ConceptNodeKind, canonical: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(format!("{}:{canonical}", kind.as_prefix())),
            canonical: canonical.into(),
            aliases: Vec::new(),
            node_kind: kind,
            decision_status: crate::anchoring::DecisionStatus::Candidate,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        }
    }

    // ── Patch 2: smart ctor consistency (3 test) ──────────────────────────────

    #[test]
    fn predicate_stub_rejects_empty_uncertainty() {
        // unresolved_slots boş + reason NoTemplateMatch değil → hata.
        let result = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::MetricUnresolved,
            vec![],
            vec![PredicateTemplateId::MetricThreshold],
        );
        assert_eq!(
            result.unwrap_err(),
            PredicateStubError::EmptyUnresolvedSlots,
            "stub boş olamaz — structured uncertainty"
        );
    }

    #[test]
    fn predicate_stub_rejects_no_template_with_suggestions() {
        // NoTemplateMatch + suggested_templates dolu → çelişki → hata.
        let result = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::NoTemplateMatch,
            vec![],                                     // NoTemplateMatch için boş olabilir
            vec![PredicateTemplateId::MetricThreshold], // ama template önerilmiş → çelişki
        );
        assert_eq!(
            result.unwrap_err(),
            PredicateStubError::NoTemplateMatchCannotSuggestTemplate
        );
    }

    #[test]
    fn predicate_stub_allows_no_template_match_without_suggestions() {
        // NoTemplateMatch + boş templates → tek geçerli boş durum.
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::NoTemplateMatch,
            vec![],
            vec![],
        )
        .expect("NoTemplateMatch + boş templates geçerli");
        assert_eq!(stub.reason(), PredicateStubReason::NoTemplateMatch);
        assert!(stub.suggested_templates().is_empty());
    }

    // ── Son Patch 2: non-RuleCandidate reject ─────────────────────────────────

    #[test]
    fn lowering_rejects_non_rule_candidate() {
        // INV-P1: sadece RuleCandidate lowering'e girebilir.
        let concept = concept_node(ConceptNodeKind::Concept, "Payment");
        let err = lower_rule_to_predicate_stub(&concept).unwrap_err();
        assert!(
            matches!(err, PredicateLoweringError::NotRuleCandidate { .. }),
            "Concept lowering'e giremez"
        );

        let task = concept_node(ConceptNodeKind::TaskCandidate, "Refactor");
        let err = lower_rule_to_predicate_stub(&task).unwrap_err();
        assert!(
            matches!(err, PredicateLoweringError::NotRuleCandidate { .. }),
            "TaskCandidate lowering'e giremez"
        );

        let code = concept_node(ConceptNodeKind::CodeEntity, "AuthService");
        let err = lower_rule_to_predicate_stub(&code).unwrap_err();
        assert!(
            matches!(err, PredicateLoweringError::NotRuleCandidate { .. }),
            "CodeEntity lowering'e giremez"
        );
    }

    // ── Son Patch 4: completeness formül ──────────────────────────────────────

    #[test]
    fn completeness_all_slots_unresolved_is_zero() {
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::MetricUnresolved,
            vec![
                PredicateSlot::Metric,
                PredicateSlot::Threshold,
                PredicateSlot::Scope,
                PredicateSlot::Comparator,
            ],
            vec![PredicateTemplateId::MetricThreshold],
        )
        .unwrap();
        assert_eq!(stub.completeness(), 0.0, "tüm slot'lar unresolved → 0.0");
    }

    #[test]
    fn completeness_two_slots_unresolved_is_half() {
        // 4 slot'tan 2'si unresolved → 1.0 - 2/4 = 0.5
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::ThresholdUnresolved,
            vec![PredicateSlot::Threshold, PredicateSlot::Scope],
            vec![PredicateTemplateId::MetricThreshold],
        )
        .unwrap();
        assert_eq!(stub.completeness(), 0.5);
    }

    #[test]
    fn completeness_no_template_match_is_zero() {
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:X".into()),
            PredicateStubReason::NoTemplateMatch,
            vec![],
            vec![],
        )
        .unwrap();
        assert_eq!(stub.completeness(), 0.0, "NoTemplateMatch → 0.0");
    }

    // ── Lowering outcome (INV-P1a — her zaman Stub) ───────────────────────────

    #[test]
    fn lowering_coupling_rule_suggests_metric_threshold() {
        let rule = rule_candidate("NoHighCouplingDependency");
        let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
        match outcome {
            PredicateLoweringOutcome::Stub(stub) => {
                assert!(stub
                    .suggested_templates()
                    .contains(&PredicateTemplateId::MetricThreshold));
                assert_eq!(stub.reason(), PredicateStubReason::MetricUnresolved);
                // Tüm slot'lar unresolved (operator bağlayacak — PR33b)
                assert_eq!(stub.unresolved_slots().len(), 4);
                // Executable predicate YOK (INV-P1a)
            }
        }
    }

    #[test]
    fn lowering_no_keyword_rule_yields_no_template_match() {
        let rule = rule_candidate("SomeAbstractConcern");
        let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
        match outcome {
            PredicateLoweringOutcome::Stub(stub) => {
                assert_eq!(stub.reason(), PredicateStubReason::NoTemplateMatch);
                assert!(stub.suggested_templates().is_empty());
                // NoTemplateMatch → unresolved_slots boş (tek geçerli boş durum)
                assert!(stub.unresolved_slots().is_empty());
            }
        }
    }

    #[test]
    fn lowering_always_produces_stub_never_executable() {
        // INV-P1a: PR33a'da her zaman Stub — executable predicate yok.
        for canonical in [
            "CouplingRule",
            "EvidenceRule",
            "DecreaseCoupling",
            "AbstractRule",
        ] {
            let rule = rule_candidate(canonical);
            let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
            assert!(
                matches!(outcome, PredicateLoweringOutcome::Stub(_)),
                "PR33a her zaman Stub: {canonical}"
            );
        }
    }

    // ── Faz 5b (T8): axis hint lowering + bind_metric_threshold ────────────────

    #[test]
    fn lowering_coupling_rule_suggests_coupling_axis() {
        // T5: "coupling" keyword → MetricThreshold + axis hint Coupling.
        let rule = rule_candidate("NoHighCouplingDependency");
        let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
        match outcome {
            PredicateLoweringOutcome::Stub(stub) => {
                assert!(stub
                    .suggested_templates()
                    .contains(&PredicateTemplateId::MetricThreshold));
                assert_eq!(
                    stub.suggested_axis(),
                    Some(PhysicalCodeMetricAxis::Coupling),
                    "coupling keyword → Coupling axis hint"
                );
            }
        }
    }

    #[test]
    fn lowering_cohesion_rule_suggests_cohesion_axis() {
        let rule = rule_candidate("HighCohesionRequired");
        let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
        match outcome {
            PredicateLoweringOutcome::Stub(stub) => {
                assert_eq!(
                    stub.suggested_axis(),
                    Some(PhysicalCodeMetricAxis::Cohesion)
                );
            }
        }
    }

    #[test]
    fn lowering_multi_axis_rule_has_no_axis_hint() {
        // "coupling" + "cohesion" aynı cümlede → belirsiz, axis hint None.
        let rule = rule_candidate("CouplingAndCohesionBalance");
        let outcome = lower_rule_to_predicate_stub(&rule).unwrap();
        match outcome {
            PredicateLoweringOutcome::Stub(stub) => {
                assert_eq!(
                    stub.suggested_axis(),
                    None,
                    "çoklu axis → None (operator kendi bağlar)"
                );
            }
        }
    }

    #[test]
    fn bind_metric_threshold_produces_non_empty_executable_set() {
        // Kontrol 2: bind her zaman ≥1 predicate üretir (non-empty by construction).
        let stub = PredicateStub::new_with_axis_hint(
            ConceptNodeId("RuleCandidate:NoHighCoupling".into()),
            PredicateStubReason::MetricUnresolved,
            vec![
                PredicateSlot::Metric,
                PredicateSlot::Threshold,
                PredicateSlot::Scope,
                PredicateSlot::Comparator,
            ],
            vec![PredicateTemplateId::MetricThreshold],
            Some(PhysicalCodeMetricAxis::Coupling),
        )
        .unwrap();
        let binding = MetricThresholdBinding::new(
            PhysicalCodeMetricAxis::Coupling,
            crate::trajectory::PredicateScope::Node(1),
            crate::trajectory::ComparisonOp::Le,
            NormalizedMetricThreshold::new(0.55).unwrap(),
        );
        let cap = crate::trajectory::OperatorCapability::issue();
        let eps = bind_metric_threshold(&stub, binding, &cap).unwrap();
        // non-empty
        let ps = eps.into_trajectory_predicate_set();
        assert!(!ps.predicates.is_empty(), "non-empty by construction");
        // Coupling ≤ 0.55
        let pred = &ps.predicates[0].predicate;
        assert_eq!(pred.metric, crate::trajectory::PredicateAxis::Coupling);
        assert_eq!(pred.operator, crate::trajectory::ComparisonOp::Le);
        assert!((pred.threshold - 0.55).abs() < 1e-9);
        // INV-T4: Scip required_source
        assert_eq!(
            pred.required_source,
            Some(crate::coords::MetricSource::Scip)
        );
    }

    #[test]
    fn bind_metric_threshold_rejects_non_metric_threshold_stub() {
        // Kontrol 4: stub MetricThreshold önermiyorsa → TemplateNotSuggested.
        let stub = PredicateStub::new(
            ConceptNodeId("RuleCandidate:EvidenceOnly".into()),
            PredicateStubReason::NoTemplateMatch,
            vec![],
            vec![], // NoTemplateMatch — MetricThreshold yok
        )
        .unwrap();
        let binding = MetricThresholdBinding::new(
            PhysicalCodeMetricAxis::Coupling,
            crate::trajectory::PredicateScope::Node(1),
            crate::trajectory::ComparisonOp::Le,
            NormalizedMetricThreshold::new(0.55).unwrap(),
        );
        let cap = crate::trajectory::OperatorCapability::issue();
        let err = bind_metric_threshold(&stub, binding, &cap).unwrap_err();
        assert_eq!(err, BindingError::TemplateNotSuggested);
    }

    #[test]
    fn bind_metric_threshold_rejects_axis_mismatch() {
        // Kontrol 5: stub axis Coupling, binding axis Cohesion → AxisMismatch.
        let stub = PredicateStub::new_with_axis_hint(
            ConceptNodeId("RuleCandidate:NoHighCoupling".into()),
            PredicateStubReason::MetricUnresolved,
            vec![PredicateSlot::Metric],
            vec![PredicateTemplateId::MetricThreshold],
            Some(PhysicalCodeMetricAxis::Coupling),
        )
        .unwrap();
        let binding = MetricThresholdBinding::new(
            PhysicalCodeMetricAxis::Cohesion, // mismatch!
            crate::trajectory::PredicateScope::Node(1),
            crate::trajectory::ComparisonOp::Le,
            NormalizedMetricThreshold::new(0.70).unwrap(),
        );
        let cap = crate::trajectory::OperatorCapability::issue();
        let err = bind_metric_threshold(&stub, binding, &cap).unwrap_err();
        assert!(matches!(err, BindingError::AxisMismatch { .. }));
    }

    #[test]
    fn bind_metric_threshold_allows_any_axis_when_stub_has_no_hint() {
        // Stub axis None (çoklu/belirsiz) → operator herhangi bir axis bağlayabilir.
        let stub = PredicateStub::new_with_axis_hint(
            ConceptNodeId("RuleCandidate:AbstractBalance".into()),
            PredicateStubReason::MetricUnresolved,
            vec![PredicateSlot::Metric],
            vec![PredicateTemplateId::MetricThreshold],
            None, // no hint
        )
        .unwrap();
        let binding = MetricThresholdBinding::new(
            PhysicalCodeMetricAxis::Instability, // operator seçti
            crate::trajectory::PredicateScope::Node(1),
            crate::trajectory::ComparisonOp::Le,
            NormalizedMetricThreshold::new(0.40).unwrap(),
        );
        let cap = crate::trajectory::OperatorCapability::issue();
        let eps = bind_metric_threshold(&stub, binding, &cap)
            .expect("no hint → operator any axis allowed");
        let ps = eps.into_trajectory_predicate_set();
        assert_eq!(
            ps.predicates[0].predicate.metric,
            crate::trajectory::PredicateAxis::Instability
        );
    }

    #[test]
    fn normalized_metric_threshold_rejects_out_of_range() {
        // Patch 3: [0,1] + is_finite — EvidenceStrength/ScalarSimilarity paterni.
        assert!(NormalizedMetricThreshold::new(f64::NAN).is_err());
        assert!(NormalizedMetricThreshold::new(f64::INFINITY).is_err());
        assert!(NormalizedMetricThreshold::new(-0.01).is_err());
        assert!(NormalizedMetricThreshold::new(1.01).is_err());
        assert!(NormalizedMetricThreshold::new(0.0).is_ok());
        assert!(NormalizedMetricThreshold::new(1.0).is_ok());
        assert!(NormalizedMetricThreshold::new(0.55).is_ok());
    }

    #[test]
    fn normalized_metric_threshold_serde_rejects_out_of_range() {
        // Custom Deserialize — constructor bypass edilemez.
        assert!(serde_json::from_str::<NormalizedMetricThreshold>("2.0").is_err());
        assert!(serde_json::from_str::<NormalizedMetricThreshold>("-1.0").is_err());
        // round-trip
        let original = NormalizedMetricThreshold::new(0.55).unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let restored: NormalizedMetricThreshold = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }
}
