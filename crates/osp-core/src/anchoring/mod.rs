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
pub mod identity;
pub mod pipeline;
pub mod predicate_lowering;
pub mod resolved_implementation;
pub mod review;
pub mod scorer;
pub mod store;
pub mod typed_ref;
pub mod types;

// Faz: CLI osp review — kalıcı store snapshot (graph + ledgers + audit_seq).
pub use store::{AnchorStoreSnapshot, SnapshotError};

// Faz 2/4: runtime tipleri kökten erişilebilir (API stabilitesi)
pub use types::{
    AnchorCandidate, AnchorPlan, AnchorScoreBreakdown, CanonicalRedirect, CanonicalRedirectReason,
    ConceptEdge, ConceptGraph, ConceptGraphSnapshot, ConceptNode, ConceptNodeId, ConceptNodeKind,
    ConceptPacket, ConceptPacketId, ConceptualIntentVector, EmptyExplanation, EvidenceCoverage,
    EvidenceStrength, EvidenceStrengthOutOfRange, EvidenceVector, ExtractedAnchorCandidate,
    GraphSeed, IncompletePhysicalVector, MetricScalarViolation, NonEmptyExplanation,
    ObservedCodeEvidence, ObservedCodeMetricSource, ObservedPhysicalMetric,
    ObservedPhysicalMetricError, ObservedPhysicalMetrics, ObservedPhysicalMetricsError,
    PacketSource, PersistedAnchorCandidateAudit, PersistedAnchorPlanAudit, PersistedRedirectAudit,
    PhysicalAxisValue, PhysicalCodeVector, PositionSnapshot, PositionSnapshotId, PositionVector,
    ScalarSimilarity, SimilarityOutOfRange,
};
// PR E — CodeIdentityBinding (types.rs'te, store-owned binding katmanı)
pub use types::CodeIdentityBinding;
// Faz 5a/5b/5.1 — predicate lowering tipleri
pub use predicate_lowering::{
    bind_metric_threshold, lower_rule_to_predicate_stub, merge_axis_hints, AxisHint,
    AxisHintConfidence, AxisHintConfidenceError, AxisHintSource, BindingError, CrossFamilyHint,
    CrossFamilyHintError, ExecutablePredicateSet, MetricThresholdBinding,
    NormalizedMetricThreshold, NormalizedMetricThresholdError, PhysicalCodeMetricAxis,
    PredicateLoweringError, PredicateLoweringOutcome, PredicateSlot, PredicateStub,
    PredicateStubError, PredicateStubReason, PredicateTemplateId, TranslationAmbiguity, ALL_SLOTS,
};
// PR E — physical code identity tipleri
pub use identity::{
    derive_resolved_code_entity_id, CodeIdentityKey, CodeIdentityKeyError, CodeIdentityScheme,
    CodePathCasePolicy,
};
// PR F — evidence identity layer (anti-corruption boundary)
pub use code_evidence::{
    CodeEvidenceSource, CodeEvidenceSourceBuildError, CodeIdentityBindingLookup,
    CodeIdentityLookupError, InMemoryCodeEvidenceSource, ResolvedCodeEvidenceProvider,
    ResolvedCodeIdentity,
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
///
/// `SupersededAccepted` (Faz 8b) is a terminal acceptance-lane status: a node that was
/// accepted and later replaced by a newer accepted decision. It retains accepted provenance
/// without current effectiveness. The successor-edge invariant is established atomically by
/// `apply_supersede` (PR #49, INV-C15); the production invocation path is `SupersedeSession`
/// (PR #50), which mints `SupersedeAuthority` internally and creates the opaque
/// `SupersedeApplication`. Public construction and deserialization can still represent the
/// status for graph replay. External callers cannot mint `SupersedeAuthority` or construct
/// `SupersedeApplication` directly; they can request the transition only through
/// `SupersedeSession`, whose operator authorization remains an INV-C11 deployment
/// responsibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DecisionStatus {
    Candidate,
    Accepted,
    Deprecated,
    Rejected,
    /// Faz 8b: accepted provenance retained, current effectiveness removed. Distinct from
    /// `Deprecated` (never-accepted retirement under Model A). Produced by PR #49's
    /// `apply_supersede`; visible in `mainline_history`, excluded from `mainline_query`.
    SupersededAccepted,
}

impl DecisionStatus {
    /// INV-C3: currently effective mainline (Accepted only).
    ///
    /// The agent-facing mainline projection filters on this predicate; only nodes that are
    /// *currently binding* qualify. `SupersededAccepted` is historical, not current.
    pub const fn is_current_mainline(self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// INV-C14: statuses that preserve accepted-mainline provenance.
    ///
    /// Model A (normative): `Deprecated` is retirement *without* accepted provenance;
    /// `SupersededAccepted` *retains* accepted provenance without current effectiveness.
    /// The two are mutually exclusive terminal meanings. No `Accepted -> Deprecated`
    /// transition is offered; if one is added, migrate to a lifecycle/outcome split and
    /// revise this predicate.
    pub const fn preserves_accepted_provenance(self) -> bool {
        matches!(self, Self::Accepted | Self::SupersededAccepted)
    }

    /// PR E (tur 3 P2-B) — R7 "live CodeEntity" predicate.
    ///
    /// Bir CodeEntity node'unun "live" (resolvable target / reuse candidate) olup olmadığı.
    /// `Candidate` (yeni oluşturulan entity henüz review'dan geçmemiş) ve `Accepted` live;
    /// `Rejected`/`Deprecated`/`SupersededAccepted` live değil (`EntityNotLiveForResolution`).
    pub const fn is_live_code_identity(self) -> bool {
        matches!(self, Self::Candidate | Self::Accepted)
    }
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

/// Concept graph edge türleri (§8.3). 16 = 15 ontolojik + 1 meta.
///
/// High-stake (11): INV-C7 gereği explanation zorunlu. Düşük-stake (4): opsiyonel.
///
/// PR G: `Ord`/`PartialOrd` eklendi (canonical triple dedup `BTreeSet` key için).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ConceptEdgeKind {
    // --- 15 ontolojik ---
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
    /// PR E — identity resolution: CodeEntityCandidate → CodeEntity (operator-reviewed identity fact).
    /// Semantik: promotion/acceptance DEĞİL; geçici/hipotetik ontolojik temsilin kanonik fiziksel
    /// varlığa bağlanması. Source yalnız CodeEntityCandidate; target yalnız CodeEntity (R3/R4).
    ResolvesTo,
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
                | Self::ResolvesTo
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

#[cfg(test)]
mod decision_status_tests {
    use super::DecisionStatus;

    /// INV-C3 + INV-C14 executable specification: 5 statü × 2 flag projeksiyon matrix'i.
    /// Bu tablo, helper davranışını tek bakışta sabitler — yeni varyant eklenirse
    /// ilk kırılan test budur ve Model A (Deprecated = provenance yok) buradan okunur.
    #[test]
    fn decision_status_projection_matrix_matches_inv_c3_and_c14() {
        // (status, is_current_mainline, preserves_accepted_provenance)
        let cases = [
            (DecisionStatus::Candidate, false, false),
            (DecisionStatus::Accepted, true, true),
            (DecisionStatus::Deprecated, false, false),
            (DecisionStatus::Rejected, false, false),
            (DecisionStatus::SupersededAccepted, false, true),
        ];
        for (status, expected_current, expected_provenance) in cases {
            assert_eq!(
                status.is_current_mainline(),
                expected_current,
                "is_current_mainline mismatch for {status:?}"
            );
            assert_eq!(
                status.preserves_accepted_provenance(),
                expected_provenance,
                "preserves_accepted_provenance mismatch for {status:?}"
            );
        }
    }

    /// Yeni varyant serde round-trip'i — `"SupersededAccepted"` ↔ enum.
    #[test]
    fn decision_status_superseded_accepted_serde_roundtrip() {
        let json = serde_json::to_string(&DecisionStatus::SupersededAccepted).unwrap();
        assert_eq!(json, "\"SupersededAccepted\"");
        let back: DecisionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, DecisionStatus::SupersededAccepted);
    }

    /// Geri uyumluluk: 4 eski token hala aynı varyanta deserialize olur.
    /// Enum'a varyant eklemek token'ları bozmaz (serde isim-bazlı, sona eklenir).
    #[test]
    fn pre_superseded_status_tokens_remain_compatible() {
        let cases = [
            ("Candidate", DecisionStatus::Candidate),
            ("Accepted", DecisionStatus::Accepted),
            ("Deprecated", DecisionStatus::Deprecated),
            ("Rejected", DecisionStatus::Rejected),
        ];
        for (token, expected) in cases {
            let json = format!("\"{token}\"");
            let actual: DecisionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(actual, expected, "token mismatch for {token}");
        }
    }
}
