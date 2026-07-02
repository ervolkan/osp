//! InMemoryAnchorStore + OperatorAcceptance (Faz 1, §11).
//!
//! INV-C3: `OperatorAcceptance` capability token — `_private: ()` + `pub(crate) issue`.
//! Downstream crate'ler (osp-cli/mcp) ve integration test'ler (`tests/`) üretemez.
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
/// Downstream (osp-cli/mcp) de üretemez. Faz 8 operator API'si bu gate'i gerçek açar.
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

/// Store hatası.
#[derive(Debug, Clone, PartialEq)]
pub enum StoreError {
    NodeNotFound(ConceptNodeId),
    AlreadyAccepted(ConceptNodeId),
    NotCandidate(ConceptNodeId),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeNotFound(id) => write!(f, "node bulunamadı: {}", id.0),
            Self::AlreadyAccepted(id) => write!(f, "node zaten Accepted: {}", id.0),
            Self::NotCandidate(id) => write!(f, "node Candidate değil: {}", id.0),
        }
    }
}

impl std::error::Error for StoreError {}

// ═══════════════════════════════════════════════════════════════════════════════
// InMemoryAnchorStore
// ═══════════════════════════════════════════════════════════════════════════════

/// In-memory concept anchoring store. `ConceptGraph` sarmalar.
///
/// # INV-C3 / INV-C5 disiplini
/// - `apply_plan`: tüm yeni node/edge'ler **Candidate** yazılır (INV-C5).
/// - `mainline_query`: sadece **Accepted** filtre (INV-C3 — Candidate mainline değil).
/// - `promote_to_accepted`: `OperatorAcceptance` gerekir (INV-C3 kapı).
pub struct InMemoryAnchorStore {
    graph: ConceptGraph,
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
        s.seed(seed);
        s
    }

    /// Seed yükle. Pre-state concept/decision/code_entities.
    pub fn seed(&mut self, seed: GraphSeed) {
        for node in seed
            .concepts
            .into_iter()
            .chain(seed.decisions)
            .chain(seed.code_entities)
        {
            self.graph.insert_node(node);
        }
    }

    /// INV-C8: canonical exact match arama (canon gate için).
    pub fn find_concepts_by_canonical(&self, name: &str) -> Vec<&ConceptNode> {
        self.graph.find_concept_by_canonical(name)
    }

    /// Graph referansı (read-only — gate/extractor için).
    pub fn graph(&self) -> &ConceptGraph {
        &self.graph
    }

    /// AnchorPlan uygula. Tüm yeni node/edge'ler **Candidate** (INV-C5).
    pub fn apply_plan(&mut self, plan: &AnchorPlan) -> ApplyResult {
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

    /// INV-C3: Candidate → Accepted geçişi. `OperatorAcceptance` gerekir.
    pub fn promote_to_accepted(
        &mut self,
        node_id: &ConceptNodeId,
        _cap: &OperatorAcceptance,
    ) -> Result<(), StoreError> {
        let node = self
            .graph
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| StoreError::NodeNotFound(node_id.clone()))?;

        if matches!(node.decision_status, DecisionStatus::Accepted) {
            return Err(StoreError::AlreadyAccepted(node_id.clone()));
        }
        node.decision_status = DecisionStatus::Accepted;
        Ok(())
    }

    /// INV-C3: mainline knowledge sorgusu — sadece Accepted.
    pub fn mainline_query(&self) -> impl Iterator<Item = &ConceptNode> {
        self.graph
            .nodes
            .values()
            .filter(|n| matches!(n.decision_status, DecisionStatus::Accepted))
    }

    /// Candidate lane sorgusu — işlem bekleyen.
    pub fn candidate_query(&self) -> impl Iterator<Item = &ConceptNode> {
        self.graph
            .nodes
            .values()
            .filter(|n| matches!(n.decision_status, DecisionStatus::Candidate))
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
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
            explanation: Some("test".into()),
        }
    }

    #[test]
    fn apply_plan_creates_ghost_nodes_as_candidate() {
        let mut store = InMemoryAnchorStore::new();
        let plan = make_plan(vec![
            candidate("Concept:Payment", ConceptEdgeKind::Mentions),
            candidate("RiskCandidate:X", ConceptEdgeKind::DerivesRisk),
        ]);
        let res = store.apply_plan(&plan);
        assert_eq!(res.new_nodes, 2);
        assert_eq!(res.new_edges, 2);
        // INV-C5: tüm yeni node'lar Candidate
        for n in store.graph.nodes.values() {
            assert_eq!(n.decision_status, DecisionStatus::Candidate);
        }
    }

    #[test]
    fn mainline_query_empty_when_all_candidate() {
        let mut store = InMemoryAnchorStore::new();
        store.apply_plan(&make_plan(vec![candidate(
            "Concept:X",
            ConceptEdgeKind::Mentions,
        )]));
        assert_eq!(
            store.mainline_query().count(),
            0,
            "INV-C3: Candidate mainline değil"
        );
    }

    #[test]
    fn candidate_query_returns_pending() {
        let mut store = InMemoryAnchorStore::new();
        store.apply_plan(&make_plan(vec![candidate(
            "Concept:X",
            ConceptEdgeKind::Mentions,
        )]));
        assert_eq!(store.candidate_query().count(), 1);
    }

    #[test]
    fn store_promotion_requires_operator_acceptance() {
        let mut store = InMemoryAnchorStore::new();
        store.apply_plan(&make_plan(vec![candidate(
            "Concept:Payment",
            ConceptEdgeKind::Mentions,
        )]));

        // INV-C3: promotion OperatorAcceptance ile
        let cap = OperatorAcceptance::issue_for_tests();
        let node_id = ConceptNodeId("Concept:Payment".into());
        store.promote_to_accepted(&node_id, &cap).unwrap();

        // Artık mainline'da
        assert_eq!(store.mainline_query().count(), 1);
        assert_eq!(store.candidate_query().count(), 0);
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
        assert_eq!(store.node_count(), 1);
        assert_eq!(store.mainline_query().count(), 1, "seed Accepted mainline");
    }

    #[test]
    fn find_concepts_by_canonical() {
        let mut store = InMemoryAnchorStore::new();
        store.apply_plan(&make_plan(vec![candidate(
            "Concept:Payment",
            ConceptEdgeKind::Mentions,
        )]));
        let found = store.find_concepts_by_canonical("Payment");
        assert_eq!(found.len(), 1);
    }
}
