//! Concept Anchoring — Genesis Layer (Paper 3).
//!
//! [`docs/concept-anchoring-design.md`] (v0.2.1) §9 (INV-C1..C8) ve §10 (D1-D13)
//! kararlarının Rust gerçeklemesi.
//!
//! # Fazlar
//! - **Faz 0:** çekirdek domain enum'ları (aşağıda) — golden fixture serde'si.
//! - **Faz 1:** in-memory deterministic MVP — [`types`] (runtime tipler), [`classifier`],
//!   [`extractor`], [`scorer`], [`gate`], [`store`], [`pipeline`]. LLM/embedding/Kuzu yok.
//! - **Faz 2 (BU DOKÜMANSAL BLOK):** INV-C1..C8 type-level enforcement hardening.
//!   Runtime kontroller → compile-time/type-level garantiler ("make illegal states
//!   unrepresentable"). Sealed trait / PhantomData / opaque newtype primitifleri.
//! - **Faz 3+:** Kuzu persistence, code evidence, embedding, LLM.
//!
//! # API stabilitesi
//! Faz 0 enum'ları (`PositionFamily`, `DecisionStatus`, `ConceptPacketType`,
//! `ConceptEdgeKind`, `AnchorDecisionKind`, `ThresholdBand`) bu modülün kökünde
//! **doğrudan tanımlı** — downstream crate'ler ve Faz 0 testleri
//! `osp_core::anchoring::PositionFamily` yolunu kullanmaya devam eder.
//! [`types`] alt modülündeki runtime tipleri de `pub use` ile kökten erişilebilir.
//!
//! # Faz 2 TCB (Trusted Computing Base) notu
//! `pub(crate)` constructor'lar (örn. [`store::OperatorAcceptance::issue_for_tests`],
//! `AnchorPlan::from_gate`) crate içi API'dır — osp-core modülleri TCB içinde
//! (store/gate/pipeline). External crate'ler (osp-cli/osp-mcp) ve integration
//! test'leri (`tests/`) bu constructor'ları çağıramaz → invariant by-pass
//! compile-time engellenir. Faz 8 operator console gerçek API ile bu gate'leri açar.

// Faz 1/2/4/5a modülleri
pub mod classifier;
pub mod code_evidence;
pub mod edit_distance;
pub mod extractor;
pub mod gate;
pub mod pipeline;
pub mod predicate_lowering;
pub mod scorer;
pub mod store;
pub mod typed_ref;
pub mod types;

// Faz 2/4: runtime tipleri kökten erişilebilir (API stabilitesi)
pub use types::{
    AnchorCandidate, AnchorPlan, AnchorScoreBreakdown, CanonicalRedirect, CanonicalRedirectReason,
    ConceptEdge, ConceptGraph, ConceptGraphSnapshot, ConceptNode, ConceptNodeId, ConceptNodeKind,
    ConceptPacket, ConceptPacketId, ConceptualIntentVector, EmptyExplanation, EvidenceStrength,
    EvidenceStrengthOutOfRange, EvidenceVector, ExtractedAnchorCandidate, GraphSeed,
    NonEmptyExplanation, ObservedCodeEvidence, ObservedCodeMetricSource, PacketSource,
    PersistedAnchorCandidateAudit, PersistedAnchorPlanAudit, PersistedRedirectAudit,
    PhysicalCodeVector, PositionSnapshot, PositionSnapshotId, PositionVector, ScalarSimilarity,
    SimilarityOutOfRange,
};
// Faz 5a/5b/5.1 — predicate lowering tipleri
pub use predicate_lowering::{
    bind_metric_threshold, lower_rule_to_predicate_stub, merge_axis_hints, AxisHint,
    AxisHintConfidence, AxisHintConfidenceError, AxisHintSource, BindingError, CrossFamilyHint,
    CrossFamilyHintError, ExecutablePredicateSet, MetricThresholdBinding,
    NormalizedMetricThreshold, NormalizedMetricThresholdError, PhysicalCodeMetricAxis,
    PredicateLoweringError, PredicateLoweringOutcome, PredicateSlot, PredicateStub,
    PredicateStubError, PredicateStubReason, PredicateTemplateId, TranslationAmbiguity, ALL_SLOTS,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 2 primitif altyapı — sealed trait + marker tipler (crate'te ilk kullanım)
// ═══════════════════════════════════════════════════════════════════════════════

/// Sealed trait primitifi — external impl engeller (closed set of implementors).
/// Kullanım: `pub trait Foo: sealed::Sealed {}` (re-export ETME — dış crate trait'i
/// isimlendiremesin). Faz 2'de aktif kullanılmıyor (placeholder); Faz 3+ trait'lerde
/// kullanıma hazır.
#[allow(dead_code)] // Faz 2 placeholder — Faz 3+ trait'lerde kullanılacak
mod sealed {
    pub trait Sealed {}
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-C1: sealed embedding mod (Faz 2 — Faz 7 placeholder)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-C1 sealed embedding modülü.
///
/// # Yapısal garanti (Faz 7 hazır)
/// `Embedding` type **private inner** (`Vec<f32>`) — modül dışından inspect edilemez.
/// Hiçbir public fonksiyon `Embedding` veya `Vec<f32>` dönmez; sadece [`ScalarSimilarity`]
/// (skalar) publica. [`AnchorScorer`](scorer::AnchorScorer) embedding vektörünü görmez;
/// sadece `cosine` tarafından üretilen skalar alır.
///
/// Faz 2: placeholder (gerçek embedding yok, scorer 0.0 kullanır). Faz 7'de `Embedding`
/// gerçek implementasyonla doldurulur ama API aynı kalır — scorer hala `ScalarSimilarity`.
///
/// [`ScalarSimilarity`]: types::ScalarSimilarity
mod embedding {
    use crate::anchoring::types::ScalarSimilarity;

    /// Embedding vector — **private inner**. Faz 7'de doldurulur.
    /// Modül dışından erişilemez → INV-C1 yapısal garanti.
    #[allow(dead_code)] // Faz 7 placeholder
    pub(crate) struct Embedding(Vec<f32>);

    impl Embedding {
        /// Faz 7: gerçek embedding construction.
        #[allow(dead_code)]
        pub(crate) fn new(values: Vec<f32>) -> Self {
            Self(values)
        }
    }

    /// Cosine similarity — private. Sadece modül içi; `ScalarSimilarity` döner.
    /// Scorer bunu çağırır, ama vector'ü asla dışarı vermez.
    #[allow(dead_code)]
    pub(crate) fn cosine(_a: &Embedding, _b: &Embedding) -> ScalarSimilarity {
        // Faz 7: gerçek cosine implementasyonu. Faz 2: placeholder.
        ScalarSimilarity::zero()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Faz 0 çekirdek enum'ları — API sabit (pub use ile kök erişim)
// ═══════════════════════════════════════════════════════════════════════════════

/// Üç position family — INV-C2 gereği karıştırılammaz.
///
/// Her family'nin eksen seti tanımlı (§4.1):
/// - `PhysicalCode` (Paper 1): coupling/cohesion/instability/entropy/witness_depth
/// - `ConceptualIntent` (Paper 3): abstraction/vision_alignment/implementation/
///   confidence/risk/code_alignment
/// - `Evidence` (Paper 1+3): confidence/coverage/recency/stability/source_reliability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PositionFamily {
    PhysicalCode,
    ConceptualIntent,
    Evidence,
}

/// AnchorResolver tarafından üretilen her adayın epistemik durumu (§5.4, INV-C3/INV-C5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DecisionStatus {
    Candidate,
    InReview,
    Accepted,
    Deprecated,
    Rejected,
}

/// İnsan/metin girdisinin ontolojik paket türü (§12 Q1).
///
/// # `RuleCandidate` isim notu
/// İnsan metninin açıkça kural biçiminde geldiği durumlar. Anchoring *sonucu*
/// türetilen `RuleCandidate` node ayrı ontolojik varlıktır (D2). İsim çakışması
/// bilinçlidir; Faz 1'de ayrıştırma değerlendirilebilir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConceptPacketType {
    UserVision,
    Requirement,
    RuleCandidate,
    Risk,
    Decision,
    Assumption,
    /// Faz 0 enum coverage; classification logic Faz 1+.
    AntiGoal,
}

/// Concept graph edge türleri (§8.3). 15 = 14 ontolojik + 1 meta.
///
/// High-stake (10): INV-C7 gereği explanation zorunlu. Düşük-stake (4): opsiyonel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConceptEdgeKind {
    // --- 14 ontolojik ---
    Mentions,
    Refines,
    DerivesRule,
    DerivesTask,
    DerivesRisk,
    Constrains,
    ExpectedImplementation,
    ImplementedBy,
    EvidencedBy,
    Contradicts,
    Supersedes,
    RelatedTo,
    AntiGoalOf,
    DependsOnDecision,
    // --- 1 meta ---
    HasPosition,
}

impl ConceptEdgeKind {
    /// High-stake edge mi? INV-C7 explanation zorunluluğu için (§8.4).
    pub fn is_high_stake(self) -> bool {
        matches!(
            self,
            Self::DerivesRule
                | Self::DerivesTask
                | Self::DerivesRisk
                | Self::Constrains
                | Self::ExpectedImplementation
                | Self::ImplementedBy
                | Self::EvidencedBy
                | Self::Contradicts
                | Self::Supersedes
                | Self::AntiGoalOf
        )
    }
}

/// Anchor resolver'ın verdiği karar türü (§8.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AnchorDecisionKind {
    StrongLink,
    TentativeLink,
    CreateNode,
    CreateIntermediateNode,
    MarkContradiction,
    MarkUnanchored,
    RequireOperatorReview,
}

/// Symbolic threshold bandı (§8.2). Numeric policy değişse bile fixture semantiği korunur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ThresholdBand {
    Strong,
    Tentative,
    Weak,
    Unanchored,
}
