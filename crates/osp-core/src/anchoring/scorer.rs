//! AnchorScorer — lexical scorer (Faz 1, §8.1, D5).
//!
//! `ExtractedAnchorCandidate` → `AnchorCandidate` (score ekler).
//! INV-C1: `semantic_similarity` skalar 0.0 placeholder (embedding vektörü GÖRİLMEZ).
//! 7 pozitif + 2 penalty bileşen. `raw_total()` + `total_clamped()`.

use crate::anchoring::types::{
    AnchorCandidate, AnchorScoreBreakdown, ConceptGraph, ExtractedAnchorCandidate,
    PacketSource,
};
use crate::anchoring::ConceptEdgeKind;

/// Lexical anchor scorer. INV-C1: embedding vector görmez, sadece skalar similarity.
pub struct AnchorScorer;

impl Default for AnchorScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl AnchorScorer {
    pub fn new() -> Self {
        Self
    }

    /// Extracted adayı skorla → AnchorCandidate.
    pub fn score(
        &self,
        extracted: ExtractedAnchorCandidate,
        graph: &ConceptGraph,
        packet_source: PacketSource,
    ) -> AnchorCandidate {
        let breakdown = self.score_breakdown(&extracted, graph, packet_source);
        AnchorCandidate::from_scored(extracted, breakdown)
    }

    fn score_breakdown(
        &self,
        c: &ExtractedAnchorCandidate,
        graph: &ConceptGraph,
        packet_source: PacketSource,
    ) -> AnchorScoreBreakdown {
        let mut b = AnchorScoreBreakdown::zeroed();

        // semantic_similarity: Faz 1 placeholder (INV-C1 — embedding Faz 7)
        b.semantic_similarity = 0.0;

        // ontology_type_compatibility: edge kind ↔ target node kind uyumu
        b.ontology_type_compatibility = self.ontology_compat(c.edge_kind, c.target_node_id.0.as_str(), graph);

        // graph_context_score: target node'un graph'ta varlığı + komşu sayısı
        b.graph_context_score = self.graph_context(&c.target_node_id, graph);

        // domain_term_match: target adında glossary terimi (yüksek güven)
        b.domain_term_match = self.domain_term_strength(&c.target_node_id, graph);

        // code_evidence_score: Faz 1 = 0.0 (code analizi Faz 4)
        b.code_evidence_score = 0.0;

        // temporal_trust_score: kaynak güveni (§6.2 hiyerarşi)
        b.temporal_trust_score = self.temporal_trust(packet_source);

        // decision_status_score: target node Accepted mı (INV-C3 mainline)
        b.decision_status_score = self.decision_status_score(&c.target_node_id, graph);

        // contradiction_penalty: Contradicts edge + Accepted decision → ceza (INV-C4)
        if c.edge_kind == ConceptEdgeKind::Contradicts {
            b.contradiction_penalty = 0.15;
        }

        // staleness_penalty: Faz 1 fresh = 0.0
        b.staleness_penalty = 0.0;

        b
    }

    fn ontology_compat(&self, kind: ConceptEdgeKind, target_id: &str, _graph: &ConceptGraph) -> f64 {
        // Edge kind ↔ target node kind uyumu (prefix'ten)
        let target_kind = target_id.split(':').next().unwrap_or("");
        match (kind, target_kind) {
            (ConceptEdgeKind::Mentions, "Concept") => 1.0,
            (ConceptEdgeKind::DerivesRule, "RuleCandidate") => 1.0,
            (ConceptEdgeKind::DerivesTask, "TaskCandidate") => 1.0,
            (ConceptEdgeKind::DerivesRisk, "RiskCandidate") => 1.0,
            (ConceptEdgeKind::ExpectedImplementation, "CodeEntityCandidate") => 1.0,
            (ConceptEdgeKind::ImplementedBy, "CodeEntity") => 1.0,
            (ConceptEdgeKind::Contradicts | ConceptEdgeKind::DependsOnDecision | ConceptEdgeKind::Supersedes, "Decision") => 1.0,
            (ConceptEdgeKind::AntiGoalOf, "Concept") => 1.0,
            // Kısmi uyum
            (_, "Concept") => 0.7,
            (_, _) if target_kind.is_empty() => 0.3,
            (_, _) => 0.5,
        }
    }

    fn graph_context(&self, target_id: &crate::anchoring::types::ConceptNodeId, graph: &ConceptGraph) -> f64 {
        // Target graph'ta varsa + komşuları varsa yüksek
        match graph.node(target_id) {
            Some(_node) => {
                let neighbor_count = graph
                    .edges
                    .iter()
                    .filter(|e| &e.to == target_id || &e.from == target_id)
                    .count();
                (0.5 + (neighbor_count as f64 * 0.1)).min(1.0)
            }
            None => 0.2, // ghost node — düşük context
        }
    }

    fn domain_term_strength(
        &self,
        target_id: &crate::anchoring::types::ConceptNodeId,
        _graph: &ConceptGraph,
    ) -> f64 {
        // Target adı anlamlı domain terimi mi (basit heuristic)
        if let Some((_, name)) = target_id.0.split_once(':') {
            if name.len() >= 3 && name.chars().next().is_some_and(|c| c.is_uppercase()) {
                return 1.0;
            }
            0.5
        } else {
            0.1
        }
    }

    fn temporal_trust(&self, source: PacketSource) -> f64 {
        // §6.2 hiyerarşi: ExplicitUser > Document > Agent
        match source {
            PacketSource::Operator => 1.0,
            PacketSource::ExplicitUser => 0.9,
            PacketSource::Document => 0.6,
            PacketSource::Agent => 0.3,
        }
    }

    fn decision_status_score(
        &self,
        target_id: &crate::anchoring::types::ConceptNodeId,
        graph: &ConceptGraph,
    ) -> f64 {
        match graph.node(target_id) {
            Some(n) => match n.decision_status {
                crate::anchoring::DecisionStatus::Accepted => 1.0,
                crate::anchoring::DecisionStatus::InReview => 0.7,
                crate::anchoring::DecisionStatus::Candidate => 0.5,
                crate::anchoring::DecisionStatus::Deprecated => 0.2,
                crate::anchoring::DecisionStatus::Rejected => 0.0,
            },
            None => 0.5, // ghost — Candidate varsay
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchoring::types::{ConceptNode, ConceptNodeKind, ConceptPacketId, ConceptNodeId, ExtractedAnchorCandidate};
    use crate::anchoring::{ConceptEdgeKind, DecisionStatus, PositionFamily};

    fn extracted(target: &str, kind: ConceptEdgeKind) -> ExtractedAnchorCandidate {
        ExtractedAnchorCandidate {
            packet_id: ConceptPacketId("pkt:1".into()),
            target_node_id: ConceptNodeId(target.into()),
            edge_kind: kind,
            explanation: None,
        }
    }

    #[test]
    fn semantic_similarity_is_zero_faz1() {
        // INV-C1: Faz 1 placeholder
        let s = AnchorScorer::new();
        let ac = s.score(extracted("Concept:Payment", ConceptEdgeKind::Mentions), &ConceptGraph::new(), PacketSource::ExplicitUser);
        assert_eq!(ac.score.semantic_similarity, 0.0);
    }

    #[test]
    fn ontology_compat_mentiones_concept_full() {
        let s = AnchorScorer::new();
        let ac = s.score(extracted("Concept:Payment", ConceptEdgeKind::Mentions), &ConceptGraph::new(), PacketSource::ExplicitUser);
        assert_eq!(ac.score.ontology_type_compatibility, 1.0);
    }

    #[test]
    fn ontology_compat_derives_risk_full() {
        let s = AnchorScorer::new();
        let ac = s.score(extracted("RiskCandidate:X", ConceptEdgeKind::DerivesRisk), &ConceptGraph::new(), PacketSource::ExplicitUser);
        assert_eq!(ac.score.ontology_type_compatibility, 1.0);
    }

    #[test]
    fn temporal_trust_explicit_user_high() {
        let s = AnchorScorer::new();
        let ac = s.score(extracted("Concept:X", ConceptEdgeKind::Mentions), &ConceptGraph::new(), PacketSource::ExplicitUser);
        assert!(ac.score.temporal_trust_score > 0.8);
    }

    #[test]
    fn temporal_trust_agent_low() {
        let s = AnchorScorer::new();
        let ac = s.score(extracted("Concept:X", ConceptEdgeKind::Mentions), &ConceptGraph::new(), PacketSource::Agent);
        assert!(ac.score.temporal_trust_score < 0.5);
    }

    #[test]
    fn contradiction_penalty_applied() {
        let s = AnchorScorer::new();
        let ac = s.score(extracted("Decision:X", ConceptEdgeKind::Contradicts), &ConceptGraph::new(), PacketSource::ExplicitUser);
        assert!(ac.score.contradiction_penalty > 0.0, "Contradicts → penalty");
    }

    #[test]
    fn total_clamped_in_range() {
        let s = AnchorScorer::new();
        let ac = s.score(extracted("Concept:Payment", ConceptEdgeKind::Mentions), &ConceptGraph::new(), PacketSource::ExplicitUser);
        let total = ac.score.total_clamped();
        assert!((0.0..=1.0).contains(&total), "total_clamped [0,1]");
    }

    #[test]
    fn total_clamped_bounds_negative_penalty() {
        // Aşırı penalty raw_total negatif yapsa bile clamp 0
        let mut b = AnchorScoreBreakdown::zeroed();
        b.contradiction_penalty = 5.0;
        b.staleness_penalty = 5.0;
        assert!(b.raw_total() < 0.0, "raw negatif olabilmeli");
        assert_eq!(b.total_clamped(), 0.0, "clamp alt sınır 0");
    }

    #[test]
    fn accepted_decision_target_boosts_score() {
        let s = AnchorScorer::new();
        let mut graph = ConceptGraph::new();
        let node = ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        };
        graph.insert_node(node);
        let ac = s.score(extracted("Concept:Payment", ConceptEdgeKind::Mentions), &graph, PacketSource::ExplicitUser);
        assert_eq!(ac.score.decision_status_score, 1.0);
    }
}
