//! Concept Anchoring runtime domain tipleri (Faz 1).
//!
//! [`crate::anchoring`] kökündeki enum'ların (Faz 0) üzerinde yaşayan struct'lar.
//! Faz 1 §11: "ConceptPacket, AnchorCandidate, AnchorPlan, PositionSnapshot tipleri".

use crate::anchoring::{
    AnchorDecisionKind, ConceptEdgeKind, ConceptPacketType, DecisionStatus, PositionFamily,
    ThresholdBand,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Newtype ID'ler — typed-prefix string ("Concept:Payment" vb.)
// ═══════════════════════════════════════════════════════════════════════════════

/// ConceptPacket newtype ID'si.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ConceptPacketId(pub String);

/// ConceptNode newtype ID'si — typed-prefix formatı ("Concept:Payment").
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ConceptNodeId(pub String);

/// PositionSnapshot newtype ID'si.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct PositionSnapshotId(pub String);

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
    /// INV-C7 — high-stake edge'lerde zorunlu (gate doğrular).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ExtractedAnchorCandidate — extractor çıktısı (score'suz)
// ═══════════════════════════════════════════════════════════════════════════════

/// Extractor tarafından üretilen, henüz skorlanmamış aday.
/// Pipeline: extract → [`ExtractedAnchorCandidate`] → score → [`AnchorCandidate`].
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedAnchorCandidate {
    pub packet_id: ConceptPacketId,
    pub target_node_id: ConceptNodeId,
    pub edge_kind: ConceptEdgeKind,
    /// High-stake edge'lerde doldurulur (gate INV-C7 doğrular).
    pub explanation: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorScoreBreakdown — §8.1 hibrit skor (7 pozitif + 2 penalty)
// ═══════════════════════════════════════════════════════════════════════════════

/// 7 bileşenli hibrit skor + 2 penalty (§8.1, D5). INV-C1: `semantic_similarity`
/// skalar — embedding vektörü görülmez (Faz 7 embedding).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnchorScoreBreakdown {
    /// Faz 1: 0.0 placeholder (lexical classifier, embedding yok).
    pub semantic_similarity: f64,
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
            semantic_similarity: 0.0,
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
        0.25 * self.semantic_similarity
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
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorCandidate {
    pub packet_id: ConceptPacketId,
    pub target_node_id: ConceptNodeId,
    pub edge_kind: ConceptEdgeKind,
    pub score: AnchorScoreBreakdown,
    /// Extractor'dan gelir; high-stake'te zorunlu (INV-C7).
    pub explanation: Option<String>,
}

impl AnchorCandidate {
    /// Extracted adaydan + skor → AnchorCandidate.
    pub fn from_scored(extracted: ExtractedAnchorCandidate, score: AnchorScoreBreakdown) -> Self {
        Self {
            packet_id: extracted.packet_id,
            target_node_id: extracted.target_node_id,
            edge_kind: extracted.edge_kind,
            score,
            explanation: extracted.explanation,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorPlan — gate çıktısı
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir ConceptPacket'ten üretilen toplam karar planı.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorPlan {
    pub packet_id: ConceptPacketId,
    /// Graph'a yazılacak edge'ler (Candidate status, INV-C3).
    pub candidates: Vec<AnchorCandidate>,
    /// §8.2 threshold kararı.
    pub decision: AnchorDecisionKind,
    pub threshold_band: ThresholdBand,
    /// INV-C7 ↔ D6: high-stake edge varsa true.
    pub requires_operator_review: bool,
    /// §6.4.1 mapping (fix_007 — kod gerçekliği SUPERSEDES yapamaz).
    pub negative_assertions: Vec<String>,
    /// INV-C8 canon gate intercept'leri (hata DEĞİL — başarılı redirect).
    pub redirects: Vec<CanonicalRedirect>,
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
/// Faz 1'de detaylı eksenler YOK — `family` ayrımı yeterli (Faz 2 calibration).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PositionSnapshot {
    pub id: PositionSnapshotId,
    pub node_id: ConceptNodeId,
    pub family: PositionFamily,
    pub vector: PositionVector,
    pub confidence: f64,
    /// Faz 1: u64 epoch (chrono yok).
    pub measured_at: u64,
}

/// Position eksenleri — Faz 1'de minimal (family ayrımı INV-C2 için yeterli).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PositionVector {
    /// Faz 1: tek ölçüm (domain_term gibi). Faz 2: family'ye göre çoklu eksen.
    pub primary: f64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ConceptGraph — in-memory graph (space.rs desenine sadık)
// ═══════════════════════════════════════════════════════════════════════════════

/// Concept anchoring graph'ı. `Space`'ten ayrı (`EdgeKind` 8 varyant,
/// `ConceptEdgeKind` 15). `HashMap<id, node>` + `Vec<edge>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConceptGraph {
    pub nodes: std::collections::HashMap<ConceptNodeId, ConceptNode>,
    pub edges: Vec<ConceptEdge>,
}

impl ConceptGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_node(&mut self, node: ConceptNode) -> &mut Self {
        self.nodes.insert(node.id.clone(), node);
        self
    }

    pub fn insert_edge(&mut self, edge: ConceptEdge) -> &mut Self {
        self.edges.push(edge);
        self
    }

    pub fn node(&self, id: &ConceptNodeId) -> Option<&ConceptNode> {
        self.nodes.get(id)
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphSeed {
    pub concepts: Vec<ConceptNode>,
    pub decisions: Vec<ConceptNode>,
    pub code_entities: Vec<ConceptNode>,
}

impl GraphSeed {
    pub fn is_empty(&self) -> bool {
        self.concepts.is_empty() && self.decisions.is_empty() && self.code_entities.is_empty()
    }

    /// Tüm seed node'larını tek iteratörde.
    pub fn all_nodes(&self) -> impl Iterator<Item = &ConceptNode> {
        self.concepts
            .iter()
            .chain(self.decisions.iter())
            .chain(self.code_entities.iter())
    }
}
