//! AnchorStore trait + InMemoryAnchorStore + OperatorAcceptance (Faz 1-3, §11, D7).
//!
//! # Faz 3 — AnchorStore abstraction
//! [`AnchorStore`] trait'i `InMemoryAnchorStore` (Faz 0-2) ve `KuzuAnchorStore`
//! (Faz 3, ayrı `osp-kuzu` crate) için ortak abstraction. `osp-core` Kuzu bilmez (D7).
//!
//! INV-C3: `OperatorAcceptance` capability token — `_private: ()` + `pub(crate) issue`.
//! Downstream crate'ler (osp-cli/mcp/osp-kuzu) ve integration test'ler üretemez.
//! Faz 8'de gerçek operator console bu gate'i açer.
//!
//! INV-C5: `apply_plan` her zaman Candidate yazar. Promotion `OperatorReviewSession`
//! ile (INV-C12/C13 denetimli `apply_decision`).

use crate::anchoring::types::{
    AnchorPlan, ConceptEdge, ConceptGraph, ConceptNode, ConceptNodeId, GraphSeed,
};
use crate::anchoring::DecisionStatus;

// ═══════════════════════════════════════════════════════════════════════════════
// OperatorAcceptance — INV-C3 capability token (compile-time gate)
// ═══════════════════════════════════════════════════════════════════════════════

/// Operator kabul yeteneği. INV-C3: Candidate → Accepted geçişi sadece operator.
///
/// # Güvenlik modeli
/// `_private: ()` field'ı private → dış crate'ler construct edemez.
/// `issue_for_tests()` `pub(crate)` → sadece osp-core içi (unit testler).
/// Integration test (`tests/`) ayrı crate derlendiği için erişemez.
/// Downstream (osp-cli/mcp/osp-kuzu) de üretemez. Faz 8 operator API'si bu gate'i gerçek açar.
#[derive(Debug, Clone, Copy)]
pub struct OperatorAcceptance {
    _private: (),
}

impl OperatorAcceptance {
    /// Sadece osp-core içi (unit testler). `pub(crate)` → dış crate erişemez.
    /// Faz 8 operator console gerçek API ile bu gate'i açar.
    #[allow(dead_code)] // osp-core unit test'lerinde kullanılıyor; normal build'de unused
    pub(crate) fn issue_for_tests() -> Self {
        Self { _private: () }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// StoreError — thiserror + serde (Faz 3: Kuzu hataları persist edilebilir)
// ═══════════════════════════════════════════════════════════════════════════════

/// InMemory store hatası.
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum StoreError {
    #[error("node bulunamadı: {0}")]
    NodeNotFound(ConceptNodeId),
    #[error("node zaten Accepted: {0}")]
    AlreadyAccepted(ConceptNodeId),
    #[error("node Candidate değil: {0}")]
    NotCandidate(ConceptNodeId),
    #[error("node {0:?} durumundan promote edilemez (Accepted/Deprecated/Rejected)")]
    NotPromotableFrom(DecisionStatus),
    #[error("basis candidate mismatch: basis={basis}, application={application}")]
    BasisCandidateMismatch {
        basis: ConceptNodeId,
        application: ConceptNodeId,
    },
    #[error("snapshot version uyumsuz: expected={expected}, found={found}")]
    InvalidSnapshotVersion { expected: u32, found: u32 },
}

/// `PresentedBasis`'in deterministic fingerprint'i → `[u8; 32]`.
/// `DecisionRecord.basis_fingerprint` için. v1'de FNV-based (harici crate yok);
/// audit kayıt bütünlüğü için yeterli, cryptographic security değil (doc'a not).
/// İleri sürüm: gerçek sha256 crate + serde serialize.
fn basis_fingerprint(basis: &crate::anchoring::review::PresentedBasis) -> [u8; 32] {
    let fnv = |seed: u64, bytes: &[u8]| -> u64 {
        let mut h = seed;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x6c62272e07bb0142;
    h1 = fnv(h1, basis.canonical().as_bytes());
    h1 = fnv(h1, &basis.node_digest().get().to_le_bytes());
    h2 = fnv(h2, basis.candidate_id().0.as_bytes());
    let mut out = [0u8; 32];
    out[..8].copy_from_slice(&h1.to_le_bytes());
    out[8..16].copy_from_slice(&h2.to_le_bytes());
    out[16..24].copy_from_slice(&(h1 ^ h2).to_le_bytes());
    out[24..32].copy_from_slice(&(h2.wrapping_add(h1)).to_le_bytes());
    out
}

impl std::fmt::Display for ConceptNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AnchorStore trait — Faz 3 (D7 abstraction)
// ═══════════════════════════════════════════════════════════════════════════════

/// Anchor store abstraction (D7). `InMemoryAnchorStore` + `KuzuAnchorStore` aynı trait.
///
/// # Fallible
/// Backend (Kuzu IO, schema, connection) fail olabilir → tüm metodlar `Result`.
/// Associated `type Error` — InMemory `StoreError`, Kuzu `KuzuStoreError`.
///
/// # Owned returns
/// `Vec<ConceptNode>` (owned) — borrow-tied `impl Iterator` trait method olamaz
/// (GAT/lifetime). Clone maliyeti persistence boundary'de kabul edilebilir.
///
/// # INV-C3/C8 persistence boundary (Faz 3 ana invariant)
/// - `apply_plan`: Candidate-only write (INV-C5).
/// - `apply_decision`: `OperatorReviewSession` ile INV-C12/C13 denetimli promotion
///   (Faz 8a; legacy `promote_to_accepted` Faz 8c'de kaldırıldı).
/// - `seed_trusted`: trusted bootstrap/restore (Accepted node yükleyebilir —
///   trusted boundary, normal mutation değil).
///
/// **"Persistence does not weaken epistemic gates."**
pub trait AnchorStore {
    /// Backend-specific error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Trusted bootstrap/restore. Seed Accepted node yükleyebilir — trusted boundary
    /// (fixture pre-state, snapshot restore). Normal mutation DEĞİL.
    fn seed_trusted(&mut self, seed: &GraphSeed) -> Result<(), Self::Error>;

    /// AnchorPlan uygula. Tüm yeni node/edge'ler Candidate (INV-C5).
    fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, Self::Error>;

    /// INV-C3: Candidate → Accepted. `OperatorAcceptance` gerekir.
    ///
    /// INV-C12/C13 (Faz 8a): Reviewed promotion/reject + ledger append atomik.
    /// `DecisionApplication` opaque (private ctor, Deserialize YOK) — tek üretici
    /// `OperatorReviewSession`. Store uygulama + `DecisionRecord` üretimi +
    /// append'in tek işlemde yapılmasından sorumludur (`seq`, `prior_status`,
    /// `new_status`, `at` store/apply anına ait).
    ///
    /// INV-C13 kapsamı: bu metod *reviewed operator decision path*'tir.
    /// `seed_trusted` (bootstrap) bu invariantın kapsam dışındadır.
    fn apply_decision(
        &mut self,
        application: crate::anchoring::review::DecisionApplication,
    ) -> Result<crate::anchoring::review::DecisionRecord, Self::Error>;

    /// INV-C13: Append-only decision ledger — sorgulanabilir. v1 InMemory;
    /// graph backend Faz 8b+ transaction garantisi ister.
    fn decision_ledger(&self) -> Vec<crate::anchoring::review::DecisionRecord>;

    /// INV-C8: canonical exact match (canon gate için).
    fn find_concepts_by_canonical(&self, name: &str) -> Result<Vec<ConceptNode>, Self::Error>;

    /// INV-C3: mainline knowledge — sadece Accepted.
    fn mainline_query(&self) -> Result<Vec<ConceptNode>, Self::Error>;

    /// Candidate lane — işlem bekleyen.
    fn candidate_query(&self) -> Result<Vec<ConceptNode>, Self::Error>;

    fn node_count(&self) -> Result<usize, Self::Error>;
    fn edge_count(&self) -> Result<usize, Self::Error>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// InMemoryAnchorStore
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory concept anchoring store. `ConceptGraph` sarmalar.
///
/// # INV-C3 / INV-C5 disiplini
/// - `apply_plan`: tüm yeni node/edge'ler **Candidate** yazılır (INV-C5).
/// - `mainline_query`: sadece **Accepted** filtre (INV-C3 — Candidate mainline değil).
/// - `apply_decision`: `OperatorReviewSession` ile INV-C12/C13 denetimli promotion
///   (Faz 8a; legacy `promote_to_accepted` Faz 8c'de kaldırıldı).
/// - `restore_trusted_snapshot`: trusted restore (Faz 3, INV-C3 persistence boundary).
pub struct InMemoryAnchorStore {
    graph: ConceptGraph,
    /// INV-C13 (Faz 8a): append-only decision ledger. `apply_decision` atomik olarak
    /// promotion + append yapar; ikisi ayrılamaz. v1 InMemory.
    decision_ledger: Vec<crate::anchoring::review::DecisionRecord>,
    /// Ledger sequence counter — atomik kayıt üretimi için.
    decision_seq: u64,
}

impl std::fmt::Debug for InMemoryAnchorStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryAnchorStore")
            .field("node_count", &self.graph.node_count())
            .field("edge_count", &self.graph.edge_count())
            .field("decision_ledger_len", &self.decision_ledger.len())
            .finish()
    }
}

impl Default for InMemoryAnchorStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryAnchorStore {
    pub fn new() -> Self {
        Self {
            graph: ConceptGraph::new(),
            decision_ledger: Vec::new(),
            decision_seq: 0,
        }
    }

    /// Graph seed ile başlat (fixture given'dan dönüştürülmüş).
    pub fn with_seed(seed: GraphSeed) -> Self {
        let mut s = Self::new();
        s.seed_trusted(&seed).expect("in-memory seed infallible");
        s
    }

    /// Trusted restore — `ConceptGraphSnapshot`'tan graph'ı geri yükle (Faz 3).
    /// INV-C3 persistence boundary: bu trusted restore path (operator-belirlenmiş
    /// Accepted node'lar dahil). Normal mutation DEĞİL — snapshot deserialize.
    ///
    /// # schema_version kontrolü
    /// Snapshot `schema_version` mevcut `SCHEMA_VERSION` ile eşleşmeli; değilse
    /// `StoreError::InvalidSnapshotVersion`. Trusted restore boundary'nin en hassas
    /// kapısı — Accepted node içerebilir, o yüzden version mismatch reject.
    pub fn restore_trusted_snapshot(
        snapshot: crate::anchoring::types::ConceptGraphSnapshot,
    ) -> Result<Self, StoreError> {
        use crate::anchoring::types::ConceptGraphSnapshot;
        if snapshot.schema_version != ConceptGraphSnapshot::SCHEMA_VERSION {
            return Err(StoreError::InvalidSnapshotVersion {
                expected: ConceptGraphSnapshot::SCHEMA_VERSION,
                found: snapshot.schema_version,
            });
        }
        let mut s = Self::new();
        for node in snapshot.nodes {
            s.graph.insert_node(node);
        }
        for edge in snapshot.edges {
            s.graph.insert_edge(edge);
        }
        Ok(s)
    }

    /// Graph referansı (read-only — gate/extractor için). Trait dışı özel accessor.
    pub fn graph(&self) -> &ConceptGraph {
        &self.graph
    }

    /// Test-only graph mut accessor (TOCTOU testleri için — node canonical değiştirme).
    #[cfg(test)]
    pub(crate) fn graph_mut(&mut self) -> &mut ConceptGraph {
        &mut self.graph
    }

    // Faz 3 backward-compat inherent wrapper'lar — downstream crate'ler `use AnchorStore`
    // import etmeden eski API'yi kullanabilsin diye. Trait abstraction korunur,
    // kaynak uyumluluğu kırılmaz.
    pub fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, StoreError> {
        <Self as AnchorStore>::apply_plan(self, plan)
    }
    pub fn seed_trusted(&mut self, seed: &GraphSeed) -> Result<(), StoreError> {
        <Self as AnchorStore>::seed_trusted(self, seed)
    }
    pub fn find_concepts_by_canonical(&self, name: &str) -> Result<Vec<ConceptNode>, StoreError> {
        <Self as AnchorStore>::find_concepts_by_canonical(self, name)
    }
    pub fn mainline_query(&self) -> Result<Vec<ConceptNode>, StoreError> {
        <Self as AnchorStore>::mainline_query(self)
    }
    pub fn candidate_query(&self) -> Result<Vec<ConceptNode>, StoreError> {
        <Self as AnchorStore>::candidate_query(self)
    }
    pub fn node_count(&self) -> Result<usize, StoreError> {
        <Self as AnchorStore>::node_count(self)
    }
    pub fn edge_count(&self) -> Result<usize, StoreError> {
        <Self as AnchorStore>::edge_count(self)
    }

    // Inherent apply_plan (trait'in arkasında kullanılan gerçek implementasyon)
    fn apply_plan_inner(&mut self, plan: &AnchorPlan) -> ApplyResult {
        let mut new_nodes = 0u32;
        let mut new_edges = 0u32;

        for c in &plan.candidates {
            // Target node yoksa ghost node oluştur (Candidate, INV-C5)
            if self.graph.node(&c.target_node_id).is_none() {
                let (kind, canonical) = parse_target(&c.target_node_id.0);
                let node = ConceptNode {
                    id: c.target_node_id.clone(),
                    canonical,
                    aliases: Vec::new(),
                    node_kind: kind,
                    decision_status: DecisionStatus::Candidate,
                    position_family: crate::anchoring::PositionFamily::ConceptualIntent,
                };
                self.graph.insert_node(node);
                new_nodes += 1;
            }

            // Edge yaz (Candidate status, INV-C5)
            let edge = ConceptEdge {
                from: plan.packet_id.clone().into_node_id(),
                to: c.target_node_id.clone(),
                kind: c.edge_kind,
                decision_status: DecisionStatus::Candidate,
                explanation: c.explanation.clone(),
            };
            self.graph.insert_edge(edge);
            new_edges += 1;
        }

        ApplyResult {
            new_nodes,
            new_edges,
        }
    }
}

impl AnchorStore for InMemoryAnchorStore {
    type Error = StoreError;

    fn seed_trusted(&mut self, seed: &GraphSeed) -> Result<(), Self::Error> {
        // Faz 5a: 3 yeni candidate bucket (rule/task/risk candidates) — Patch 6.
        // Deterministik sıra all_nodes() ile uyumlu.
        for node in seed
            .concepts
            .iter()
            .chain(&seed.decisions)
            .chain(&seed.code_entities)
            .chain(&seed.rule_candidates)
            .chain(&seed.task_candidates)
            .chain(&seed.risk_candidates)
        {
            self.graph.insert_node(node.clone());
        }
        Ok(())
    }

    fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, Self::Error> {
        Ok(self.apply_plan_inner(plan))
    }

    /// INV-C12/C13 (Faz 8a): reviewed promotion/reject + ledger append atomik.
    /// `seq`, `prior_status`, `new_status`, `at` burada üretilir — session değil.
    fn apply_decision(
        &mut self,
        application: crate::anchoring::review::DecisionApplication,
    ) -> Result<crate::anchoring::review::DecisionRecord, Self::Error> {
        use crate::anchoring::review::{DecisionKind, DecisionRecord};

        let id = application.candidate_id();
        let decision = application.decision();

        // Defense-in-depth (Review 1 tasarım gözlemi): basis.candidate_id ile
        // application.candidate_id eşleşmeli. Session bu kontrolü yapar ama apply_decision
        // da yapmalı — NotPromotableFrom için aynı defense-in-depth argümanı.
        // **Kontrol sırası:** id-mismatch ÖNCE, sonra NotPromotableFrom.
        let basis = application.basis();
        if basis.candidate_id() != id {
            return Err(StoreError::BasisCandidateMismatch {
                basis: basis.candidate_id().clone(),
                application: id.clone(),
            });
        }

        // Node'u bul + prior_status + NotPromotable kontrolü.
        let node = self
            .graph
            .node_mut(id)
            .ok_or_else(|| StoreError::NodeNotFound(id.clone()))?;
        let prior_status = node.decision_status;

        // NotPromotable: Accepted/Deprecated/Rejected'dan accept/reject geçersiz.
        // (Diriltme ayrı mekanizma — v1 dışı.)
        match (prior_status, decision) {
            (DecisionStatus::Accepted, _) => {
                return Err(StoreError::NotPromotableFrom(prior_status));
            }
            (DecisionStatus::Deprecated, _) => {
                return Err(StoreError::NotPromotableFrom(prior_status));
            }
            (DecisionStatus::Rejected, _) => {
                return Err(StoreError::NotPromotableFrom(prior_status));
            }
            _ => {}
        }

        // Status geçişini uygula.
        let new_status = match decision {
            DecisionKind::Accept => DecisionStatus::Accepted,
            DecisionKind::Reject => DecisionStatus::Rejected,
        };
        node.decision_status = new_status;

        // INV-C13: DecisionRecord üret + ledger'a atomik append.
        self.decision_seq += 1;
        let seq = self.decision_seq;
        let basis = application.basis();
        // basis_fingerprint: PresentedBasis seçili alanlarının (canonical + node_digest +
        // candidate_id) FNV-based deterministic fingerprint'i. Audit-security hash DEĞİL
        // (v1'de harici crate yok); cryptographic için ileri sürüm sha2 crate.
        let basis_fp = basis_fingerprint(basis);
        let record = DecisionRecord {
            seq,
            session_id: application.session_id().clone(),
            operator: application.operator().clone(),
            candidate_id: id.clone(),
            node_digest_serde: basis.node_digest().get(),
            decision,
            reason: application.reason().clone(),
            basis_fingerprint: basis_fp,
            prior_status,
            new_status,
            at: application.decided_at(),
        };
        self.decision_ledger.push(record.clone());
        Ok(record)
    }

    fn decision_ledger(&self) -> Vec<crate::anchoring::review::DecisionRecord> {
        self.decision_ledger.clone()
    }

    fn find_concepts_by_canonical(&self, name: &str) -> Result<Vec<ConceptNode>, Self::Error> {
        Ok(self
            .graph
            .find_concept_by_canonical(name)
            .into_iter()
            .cloned()
            .collect())
    }

    fn mainline_query(&self) -> Result<Vec<ConceptNode>, Self::Error> {
        Ok(self
            .graph
            .nodes_iter()
            .filter(|n| matches!(n.decision_status, DecisionStatus::Accepted))
            .cloned()
            .collect())
    }

    fn candidate_query(&self) -> Result<Vec<ConceptNode>, Self::Error> {
        Ok(self
            .graph
            .nodes_iter()
            .filter(|n| matches!(n.decision_status, DecisionStatus::Candidate))
            .cloned()
            .collect())
    }

    fn node_count(&self) -> Result<usize, Self::Error> {
        Ok(self.graph.node_count())
    }

    fn edge_count(&self) -> Result<usize, Self::Error> {
        Ok(self.graph.edge_count())
    }
}

/// Apply plan sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyResult {
    pub new_nodes: u32,
    pub new_edges: u32,
}

/// "Concept:Payment" → (ConceptNodeKind, canonical).
fn parse_target(id: &str) -> (crate::anchoring::types::ConceptNodeKind, String) {
    use crate::anchoring::types::ConceptNodeKind;
    if let Some((prefix, name)) = id.split_once(':') {
        let kind = ConceptNodeKind::from_prefix(prefix).unwrap_or(ConceptNodeKind::Concept);
        (kind, name.to_string())
    } else {
        (ConceptNodeKind::Concept, id.to_string())
    }
}

/// ConceptPacketId'den ConceptNodeId'ye (ConceptPacket node'u graph'ta).
impl crate::anchoring::types::ConceptPacketId {
    pub fn into_node_id(&self) -> ConceptNodeId {
        ConceptNodeId(format!("ConceptPacket:{}", self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchoring::types::{
        AnchorCandidate, AnchorScoreBreakdown, ConceptNodeKind, GraphSeed,
    };
    use crate::anchoring::{AnchorDecisionKind, ConceptEdgeKind, ThresholdBand};

    fn make_plan(candidates: Vec<AnchorCandidate>) -> AnchorPlan {
        AnchorPlan {
            packet_id: crate::anchoring::types::ConceptPacketId("pkt:1".into()),
            candidates,
            decision: AnchorDecisionKind::TentativeLink,
            threshold_band: ThresholdBand::Tentative,
            requires_operator_review: false,
            negative_assertions: vec![],
            redirects: vec![],
        }
    }

    fn candidate(target: &str, kind: ConceptEdgeKind) -> AnchorCandidate {
        AnchorCandidate {
            packet_id: crate::anchoring::types::ConceptPacketId("pkt:1".into()),
            target_node_id: ConceptNodeId(target.into()),
            edge_kind: kind,
            score: AnchorScoreBreakdown::zeroed(),
            explanation: Some(
                crate::anchoring::types::NonEmptyExplanation::from_validated("test".into()),
            ),
        }
    }

    #[test]
    fn apply_plan_creates_ghost_nodes_as_candidate() {
        let mut store = InMemoryAnchorStore::new();
        let plan = make_plan(vec![
            candidate("Concept:Payment", ConceptEdgeKind::Mentions),
            candidate("RiskCandidate:X", ConceptEdgeKind::DerivesRisk),
        ]);
        let res = store.apply_plan(&plan).expect("apply");
        assert_eq!(res.new_nodes, 2);
        assert_eq!(res.new_edges, 2);
        // INV-C5: tüm yeni node'lar Candidate
        for n in store.graph().nodes_iter() {
            assert_eq!(n.decision_status, DecisionStatus::Candidate);
        }
    }

    #[test]
    fn mainline_query_empty_when_all_candidate() {
        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:X",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();
        assert_eq!(
            store.mainline_query().unwrap().len(),
            0,
            "INV-C3: Candidate mainline değil"
        );
    }

    #[test]
    fn candidate_query_returns_pending() {
        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:X",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();
        assert_eq!(store.candidate_query().unwrap().len(), 1);
    }

    #[test]
    fn store_promotion_via_review_session_moves_to_mainline() {
        // Faz 8c: promote_to_accepted legacy path kaldırıldı.
        // Promotion artık OperatorReviewSession + apply_decision ile (INV-C12/C13).
        use crate::anchoring::review::{OperatorId, OperatorReviewSession, PresentedBasis};
        use crate::anchoring::types::NonEmptyExplanation;

        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:Payment",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();

        let node_id = ConceptNodeId("Concept:Payment".into());
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("test-operator"));
        let basis = PresentedBasis::compile(&store, &node_id).expect("basis compile");
        let reason = NonEmptyExplanation::from_validated("test promotion".into());
        session
            .accept(&mut store, &node_id, basis, reason)
            .expect("accept");

        // Artık mainline'da (INV-C3 — Candidate→Accepted promotion)
        assert_eq!(store.mainline_query().unwrap().len(), 1);
        assert_eq!(store.candidate_query().unwrap().len(), 0);
        // INV-C13: ledger'a kayıt düştü
        assert_eq!(store.decision_ledger().len(), 1);
    }

    // Faz 8c: 'apply_decision_rejects_unknown_node' testi kaldırıldı.
    // Bilinmeyen node ve NotPromotableFrom yolları review.rs'te zaten thorough test ediliyor:
    //   - review_session_not_found_rejects_unknown_candidate
    //   - review_session_not_promotable_rejects_accepted_node
    //   - apply_decision_rejects_accepted_node_not_promotable_from

    #[test]
    fn seed_loads_graph_state() {
        let mut seed = GraphSeed::default();
        seed.concepts.push(ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        });
        let store = InMemoryAnchorStore::with_seed(seed);
        assert_eq!(store.node_count().unwrap(), 1);
        assert_eq!(
            store.mainline_query().unwrap().len(),
            1,
            "seed Accepted mainline"
        );
    }

    #[test]
    fn find_concepts_by_canonical() {
        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:Payment",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();
        let found = store.find_concepts_by_canonical("Payment").unwrap();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn seed_trusted_via_trait() {
        // Faz 3: seed_trusted trait metodu
        let mut store = InMemoryAnchorStore::new();
        let mut seed = GraphSeed::default();
        seed.concepts.push(ConceptNode {
            id: ConceptNodeId("Concept:Auth".into()),
            canonical: "Auth".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        });
        AnchorStore::seed_trusted(&mut store, &seed).unwrap();
        assert_eq!(store.mainline_query().unwrap().len(), 1);
    }

    #[test]
    fn restore_trusted_snapshot_roundtrip() {
        // Faz 3: ConceptGraphSnapshot restore (INV-C3 trusted boundary)
        use crate::anchoring::types::ConceptGraphSnapshot;
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
        let store = InMemoryAnchorStore::restore_trusted_snapshot(snapshot).unwrap();
        assert_eq!(store.node_count().unwrap(), 1);
        assert_eq!(
            store.mainline_query().unwrap().len(),
            1,
            "restored Accepted"
        );
    }

    #[test]
    fn restore_trusted_snapshot_rejects_version_mismatch() {
        // Faz 3 #2: schema_version mismatch → InvalidSnapshotVersion
        use crate::anchoring::types::ConceptGraphSnapshot;
        let snapshot = ConceptGraphSnapshot {
            nodes: vec![],
            edges: vec![],
            schema_version: 999, // mismatch
        };
        let err = InMemoryAnchorStore::restore_trusted_snapshot(snapshot).unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::InvalidSnapshotVersion {
                    expected: 1,
                    found: 999
                }
            ),
            "version mismatch reject"
        );
    }

    #[test]
    fn store_error_serde_roundtrip() {
        // Faz 3 #3: StoreError thiserror+serde — #[serde(tag="kind")] + newtype kombinasyonu
        let err = StoreError::NodeNotFound(ConceptNodeId("Concept:X".into()));
        let json = serde_json::to_string(&err).unwrap();
        let back: StoreError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);

        let err2 = StoreError::InvalidSnapshotVersion {
            expected: 1,
            found: 2,
        };
        let json2 = serde_json::to_string(&err2).unwrap();
        let back2: StoreError = serde_json::from_str(&json2).unwrap();
        assert_eq!(err2, back2);
    }

    // ── Faz 5a (T6, Patch 4/5): TaskCandidate promote — INV-T2 boundary ───────

    #[test]
    fn promote_task_candidate_does_not_create_trajectory_task() {
        // Faz 8c: promote_to_accepted kaldırıldı; OperatorReviewSession kullanılır.
        // Patch 4/5: Accepted TaskCandidate ≠ trajectory::Task.
        // PR33a anchoring içinde kalır — trajectory genesis'e (OperatorCapability,
        // INV-T2) dokunmaz. Bu test promote sonrası node'un hala TaskCandidate
        // olduğunu + status Accepted olduğunu doğrular. trajectory::Task yaratımı
        // PR33b'ye (operator console / bridge).
        use crate::anchoring::review::{OperatorId, OperatorReviewSession, PresentedBasis};
        use crate::anchoring::types::NonEmptyExplanation;

        let task_node = ConceptNode {
            id: ConceptNodeId("TaskCandidate:AuthServiceRefactor".into()),
            canonical: "AuthServiceRefactor".into(),
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::TaskCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        };
        let mut store = InMemoryAnchorStore::with_seed(GraphSeed {
            task_candidates: vec![task_node],
            ..Default::default()
        });

        // Faz 8c: OperatorReviewSession ile promote (INV-C12/C13).
        let node_id = ConceptNodeId("TaskCandidate:AuthServiceRefactor".into());
        let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("test-operator"));
        let basis = PresentedBasis::compile(&store, &node_id).expect("basis compile");
        let reason = NonEmptyExplanation::from_validated("task candidate promote".into());
        session
            .accept(&mut store, &node_id, basis, reason)
            .expect("TaskCandidate promote");

        // Node hala TaskCandidate (kind değişmez), status Accepted.
        let node = store
            .graph()
            .node(&ConceptNodeId("TaskCandidate:AuthServiceRefactor".into()))
            .expect("node mevcut");
        assert_eq!(
            node.node_kind,
            ConceptNodeKind::TaskCandidate,
            "kind değişmez"
        );
        assert_eq!(node.decision_status, DecisionStatus::Accepted);

        // PR33a: trajectory::Task yaratılmaz (compile-level — bu test `trajectory`
        // modülüne reference içermiyor; navigator'a bağlanma PR33b).
        // INV-T2 ihlal yok: Task genesis OperatorCapability gerektirir, PR33a'da yok.
    }

    #[test]
    fn graph_seed_candidate_buckets_backward_compatible() {
        // Patch 6: GraphSeed yeni bucket'lar Default ile backward-compat.
        // Eski yapı (3 bucket) hala çalışır; yeni bucket'lar boş başlar.
        let seed = GraphSeed {
            concepts: vec![ConceptNode {
                id: ConceptNodeId("Concept:Payment".into()),
                canonical: "Payment".into(),
                aliases: Vec::new(),
                node_kind: ConceptNodeKind::Concept,
                decision_status: DecisionStatus::Accepted,
                position_family: crate::anchoring::PositionFamily::ConceptualIntent,
            }],
            ..Default::default()
        };
        let store = InMemoryAnchorStore::with_seed(seed);
        assert_eq!(store.node_count().unwrap(), 1, "concepts seed'lendi");

        // Yeni bucket'lar boş → all_nodes yine 1 node.
        let seed2 = GraphSeed {
            rule_candidates: vec![ConceptNode {
                id: ConceptNodeId("RuleCandidate:NoCoupling".into()),
                canonical: "NoCoupling".into(),
                aliases: Vec::new(),
                node_kind: ConceptNodeKind::RuleCandidate,
                decision_status: DecisionStatus::Candidate,
                position_family: crate::anchoring::PositionFamily::ConceptualIntent,
            }],
            task_candidates: vec![ConceptNode {
                id: ConceptNodeId("TaskCandidate:Refactor".into()),
                canonical: "Refactor".into(),
                aliases: Vec::new(),
                node_kind: ConceptNodeKind::TaskCandidate,
                decision_status: DecisionStatus::Candidate,
                position_family: crate::anchoring::PositionFamily::ConceptualIntent,
            }],
            ..Default::default()
        };
        let store2 = InMemoryAnchorStore::with_seed(seed2);
        assert_eq!(
            store2.node_count().unwrap(),
            2,
            "candidate bucket'lar seed'lendi"
        );
    }

    /// Re-proposal after rejection — characterization (Paper 3 Faz 8a+).
    ///
    /// Bu test normative DEĞİL — mevcut apply_plan davranışını karakterize eder.
    /// "Observed behavior ≠ intended reversal protocol."
    ///
    /// Senaryo: RuleCandidate:X reject edilir. Aynı canonical'a ikinci DerivesRule
    /// candidate içeren plan apply_plan'a verilirse ne olur?
    ///
    /// Gözlenen: apply_plan_inner "node yoksa ghost oluştur" mantığıyla çalışır —
    /// reddedilmiş node zaten var, new_nodes=0, sadece edge eklenir. Status DEĞİŞMEZ.
    /// Phase 8b ReopenSession normative reversal semantics tanımlayacak.
    #[test]
    fn re_proposal_after_rejection_current_semantics_is_characterized() {
        use crate::anchoring::types::{
            AnchorCandidate, AnchorPlan, AnchorScoreBreakdown, ConceptPacketId,
        };
        use crate::anchoring::{AnchorDecisionKind, ConceptEdgeKind, ThresholdBand};

        // Reddedilmiş RuleCandidate:X seed'le.
        let rejected_node = ConceptNode {
            id: ConceptNodeId("RuleCandidate:X".into()),
            canonical: "X".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Rejected,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(rejected_node);
        let mut store = InMemoryAnchorStore::with_seed(seed);

        // Aynı RuleCandidate:X target'ına ikinci DerivesRule edge'li plan.
        let plan = AnchorPlan {
            packet_id: ConceptPacketId("pkt:reproposal".into()),
            candidates: vec![AnchorCandidate {
                packet_id: ConceptPacketId("pkt:reproposal".into()),
                target_node_id: ConceptNodeId("RuleCandidate:X".into()),
                edge_kind: ConceptEdgeKind::DerivesRule,
                score: AnchorScoreBreakdown::zeroed(),
                explanation: Some(
                    crate::anchoring::types::NonEmptyExplanation::new(
                        "re-proposal after rejection",
                    )
                    .unwrap(),
                ),
            }],
            decision: AnchorDecisionKind::TentativeLink,
            threshold_band: ThresholdBand::Tentative,
            requires_operator_review: true,
            negative_assertions: vec![],
            redirects: vec![],
        };

        let apply_result = store.apply_plan(&plan).expect("apply_plan");

        // KARAKTERİZASYON: reddedilmiş node zaten var → new_nodes = 0.
        assert_eq!(
            apply_result.new_nodes, 0,
            "reddedilmiş node zaten var → yeni node doğmuyor (duplicate önleniyor)"
        );
        assert_eq!(apply_result.new_edges, 1, "ikinci DerivesRule edge ekleniyor");

        // Status DEĞİŞMEDİ — hala Rejected.
        let node_after = store
            .graph()
            .node(&ConceptNodeId("RuleCandidate:X".into()))
            .expect("node mevcut");
        assert_eq!(
            node_after.decision_status,
            DecisionStatus::Rejected,
            "re-proposal status'ü DEĞİŞTİRMİYOR — hala Rejected"
        );

        // SONUÇ (Paper 3 §10/§11'e yansır): "The current canon gate preserves canonical
        // identity even across rejected nodes, but this is not yet a reversal protocol;
        // it makes re-proposal visible as a collision with prior rejection (new edge to
        // rejected node, no status change)." Phase 8b ReopenSession normative reversal.
    }
}
