//! AnchorStore trait + InMemoryAnchorStore + OperatorAcceptance (Faz 1-3, §11, D7).
//!
//! # Faz 3 — AnchorStore abstraction
//! [`AnchorStore`] trait'i `InMemoryAnchorStore` (Faz 0-2) ve `KuzuAnchorStore`
//! (Faz 3, ayrı `osp-kuzu` crate) için ortak abstraction. `osp-core` Kuzu bilmez (D7).
//!
//! INV-C3: `OperatorAcceptance` capability token — `_private: ()` + `pub(crate) issue`.
//! Downstream crate'ler (osp-cli/mcp/osp-kuzu) ve integration test'ler üretemez.
//! Faz 8'de gerçek operator console bu gate'i açar.
//!
//! INV-C5: `apply_plan` her zaman Candidate yazar. `promote_to_accepted` operator ister.

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
    #[error("snapshot version uyumsuz: expected={expected}, found={found}")]
    InvalidSnapshotVersion { expected: u32, found: u32 },
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
/// - `promote_to_accepted`: `OperatorAcceptance` gerekir (osp-kuzu üretemez; Faz 8
///   AnchorService osp-core'da token ile çağırır).
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

    /// INV-C3: Candidate → Accepted. `OperatorAcceptance` gerekir (Faz 8 operator).
    fn promote_to_accepted(
        &mut self,
        node_id: &ConceptNodeId,
        _cap: &OperatorAcceptance,
    ) -> Result<(), Self::Error>;

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
/// - `promote_to_accepted`: `OperatorAcceptance` gerekir (INV-C3 kapı).
/// - `restore_trusted_snapshot`: trusted restore (Faz 3, INV-C3 persistence boundary).
pub struct InMemoryAnchorStore {
    graph: ConceptGraph,
}

impl std::fmt::Debug for InMemoryAnchorStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryAnchorStore")
            .field("node_count", &self.graph.node_count())
            .field("edge_count", &self.graph.edge_count())
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

    // Faz 3 backward-compat inherent wrapper'lar — downstream crate'ler `use AnchorStore`
    // import etmeden eski API'yi kullanabilsin diye. Trait abstraction korunur,
    // kaynak uyumluluğu kırılmaz.
    pub fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, StoreError> {
        <Self as AnchorStore>::apply_plan(self, plan)
    }
    pub fn promote_to_accepted(
        &mut self,
        node_id: &ConceptNodeId,
        cap: &OperatorAcceptance,
    ) -> Result<(), StoreError> {
        <Self as AnchorStore>::promote_to_accepted(self, node_id, cap)
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

    // Inherent promote (trait'in arkasında)
    fn promote_to_accepted_inner(
        &mut self,
        node_id: &ConceptNodeId,
        _cap: &OperatorAcceptance,
    ) -> Result<(), StoreError> {
        let node = self
            .graph
            .node_mut(node_id)
            .ok_or_else(|| StoreError::NodeNotFound(node_id.clone()))?;

        if matches!(node.decision_status, DecisionStatus::Accepted) {
            return Err(StoreError::AlreadyAccepted(node_id.clone()));
        }
        node.decision_status = DecisionStatus::Accepted;
        Ok(())
    }
}

impl AnchorStore for InMemoryAnchorStore {
    type Error = StoreError;

    fn seed_trusted(&mut self, seed: &GraphSeed) -> Result<(), Self::Error> {
        for node in seed
            .concepts
            .iter()
            .chain(&seed.decisions)
            .chain(&seed.code_entities)
        {
            self.graph.insert_node(node.clone());
        }
        Ok(())
    }

    fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, Self::Error> {
        Ok(self.apply_plan_inner(plan))
    }

    fn promote_to_accepted(
        &mut self,
        node_id: &ConceptNodeId,
        _cap: &OperatorAcceptance,
    ) -> Result<(), Self::Error> {
        self.promote_to_accepted_inner(node_id, _cap)
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
    fn store_promotion_requires_operator_acceptance() {
        let mut store = InMemoryAnchorStore::new();
        store
            .apply_plan(&make_plan(vec![candidate(
                "Concept:Payment",
                ConceptEdgeKind::Mentions,
            )]))
            .unwrap();

        // INV-C3: promotion OperatorAcceptance ile
        let cap = OperatorAcceptance::issue_for_tests();
        let node_id = ConceptNodeId("Concept:Payment".into());
        store.promote_to_accepted(&node_id, &cap).unwrap();

        // Artık mainline'da
        assert_eq!(store.mainline_query().unwrap().len(), 1);
        assert_eq!(store.candidate_query().unwrap().len(), 0);
    }

    #[test]
    fn promotion_rejects_unknown_node() {
        let mut store = InMemoryAnchorStore::new();
        let cap = OperatorAcceptance::issue_for_tests();
        let err = store
            .promote_to_accepted(&ConceptNodeId("Concept:Yok".into()), &cap)
            .unwrap_err();
        assert!(matches!(err, StoreError::NodeNotFound(_)));
    }

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
}
