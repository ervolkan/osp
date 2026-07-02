//! Concept Anchoring — Genesis Layer (Paper 3).
//!
//! [`docs/concept-anchoring-design.md`] (v0.2.1) §9 (INV-C1..C8) ve §10 (D1-D13)
//! kararlarının Rust gerçeklemesi.
//!
//! # Fazlar
//! - **Faz 0:** çekirdek domain enum'ları (aşağıda) — golden fixture serde'si.
//! - **Faz 1 (BU DOKÜMANSAL BLOK):** in-memory deterministic MVP —
//!   [`types`] (runtime tipler), [`classifier`], [`extractor`], [`scorer`],
//!   [`gate`], [`store`], [`pipeline`]. LLM/embedding/Kuzu yok (§11 disiplini).
//! - **Faz 2:** INV-C1..C8 type-level enforcement + scoring calibration.
//! - **Faz 3+:** Kuzu persistence, code evidence, embedding, LLM.
//!
//! # API stabilitesi
//! Faz 0 enum'ları (`PositionFamily`, `DecisionStatus`, `ConceptPacketType`,
//! `ConceptEdgeKind`, `AnchorDecisionKind`, `ThresholdBand`) bu modülün kökünde
//! `pub use` ile re-export edilir — downstream crate'ler ve Faz 0 testleri
//! `osp_core::anchoring::PositionFamily` yolunu kullanmaya devam eder.

// Faz 1 modülleri
pub mod classifier;
pub mod edit_distance;
pub mod extractor;
pub mod gate;
pub mod pipeline;
pub mod scorer;
pub mod store;
pub mod typed_ref;
pub mod types;

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
