//! AnchorGate — threshold + INV-C7/C8 + §6.4.1/§8.6 + ImplementedBy/Supersedes.
//!
//! `Vec<AnchorCandidate>` → `AnchorPlan`. Faz 1 (§8.2, §8.4, §6.4.1, §8.6, INV-C7, INV-C8).
//!
//! # Kural özeti
//! - **§8.2 threshold** (`total_clamped`): ≥0.80 StrongLink, [0.60,0.80) TentativeLink,
//!   [0.40,0.60) CreateNode, <0.40 MarkUnanchored.
//! - **INV-C7**: high-stake edge explanation zorunlu.
//! - **INV-C8 canon gate** (CreateNode öncesi): exact/alias/edit≤2 → redirect (hata değil).
//! - **§6.4.1**: Contradicts → MarkContradiction + negative_assertions.
//! - **§8.6**: yüksek abstraction → doğrudan CodeEntity yasak.
//! - **ImplementedBy**: Faz 1'de code evidence yok → yasak.
//! - **Supersedes**: authority gerektirir (INV-C4).

use crate::anchoring::classifier::Glossary;
use crate::anchoring::edit_distance::within_edit_distance_2;
use crate::anchoring::types::{
    AnchorCandidate, AnchorPlan, CanonicalRedirect, CanonicalRedirectReason, ConceptGraph,
    ConceptNodeId, ConceptPacketId,
};
use crate::anchoring::{AnchorDecisionKind, ConceptEdgeKind, ThresholdBand};

/// Anchor gate hatası — invariant ihlalleri.
#[derive(Debug, Clone, PartialEq)]
pub enum GateError {
    /// INV-C7: high-stake edge explanation yok.
    MissingExplanation { edge_kind: ConceptEdgeKind },
    /// §8.6: yüksek abstraction → doğrudan CodeEntity.
    IllegalDirectCodeBinding { from: ConceptNodeId, to: ConceptNodeId },
    /// Faz 1: ImplementedBy code evidence (Faz 4) olmadan üretilemez.
    ImplementedByRequiresCodeEvidence,
    /// INV-C4: Supersedes authority yok.
    SupersedeAuthorityRequired,
}

impl std::fmt::Display for GateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExplanation { edge_kind } => {
                write!(f, "INV-C7: high-stake edge {:?} explanation zorunlu", edge_kind)
            }
            Self::IllegalDirectCodeBinding { from, to } => {
                write!(f, "§8.6: yasak doğrudan kod bağlantısı {} → {}", from.0, to.0)
            }
            Self::ImplementedByRequiresCodeEvidence => {
                write!(f, "ImplementedBy code evidence (Faz 4) gerektirir")
            }
            Self::SupersedeAuthorityRequired => {
                write!(f, "INV-C4: Supersedes authority gerekli")
            }
        }
    }
}

impl std::error::Error for GateError {}

/// Anchor gate. Pipeline'ın son aşaması — candidate'leri plana dönüştürür.
pub struct AnchorGate {
    glossary: Glossary,
}

impl AnchorGate {
    pub fn new(glossary: Glossary) -> Self {
        Self { glossary }
    }

    /// Skorlanmış adayları AnchorPlan'a dönüştür.
    ///
    /// Sıra: invariant kontrolleri → threshold karar → canon gate → negative assertions.
    pub fn decide(
        &self,
        packet_id: &ConceptPacketId,
        candidates: Vec<AnchorCandidate>,
        graph: &ConceptGraph,
    ) -> Result<AnchorPlan, GateError> {
        // 1. INV-C7: high-stake explanation kontrolü
        for c in &candidates {
            if c.edge_kind.is_high_stake() && c.explanation.is_none() {
                return Err(GateError::MissingExplanation { edge_kind: c.edge_kind });
            }
        }

        // 2. §8.6 + ImplementedBy + Supersedes kontrolleri
        for c in &candidates {
            self.validate_edge_kind(c, graph)?;
        }

        // 3. Threshold karar (en yüksek skorlu aday üzerinden)
        let max_score = candidates.iter().map(|c| c.score.total_clamped()).fold(0.0_f64, f64::max);
        let (decision, band) = self.threshold_decision(max_score);

        // 4. INV-C8 canon gate (CreateNode ise redirect kontrolü)
        let mut redirects = Vec::new();
        let mut adjusted_candidates = candidates;
        if matches!(decision, AnchorDecisionKind::CreateNode) {
            for c in &mut adjusted_candidates {
                if let Some(redirect) = self.canon_gate_check(c, graph) {
                    // Redirect varsa: yeni node yerine mevcut'a TentativeLink
                    c.target_node_id = redirect.existing_node.clone();
                    redirects.push(redirect);
                }
            }
        }

        // 5. §6.4.1 Contradicts → MarkContradiction + negative assertions
        let has_contradicts = adjusted_candidates
            .iter()
            .any(|c| c.edge_kind == ConceptEdgeKind::Contradicts);
        let final_decision = if has_contradicts && !matches!(decision, AnchorDecisionKind::MarkUnanchored) {
            AnchorDecisionKind::MarkContradiction
        } else {
            decision
        };
        let final_band = if has_contradicts && !matches!(band, ThresholdBand::Unanchored) {
            // Contradiction band'ı düşürme yok; ama decision değişti
            band
        } else {
            band
        };

        // 6. Negative assertions (fix_007 — §6.4.1 mapping)
        let negative_assertions = if has_contradicts {
            self.contradiction_negative_assertions(&adjusted_candidates)
        } else {
            Vec::new()
        };

        // 7. requires_operator_review (INV-C7 ↔ D6)
        let requires_operator_review = adjusted_candidates.iter().any(|c| c.edge_kind.is_high_stake())
            || matches!(
                final_decision,
                AnchorDecisionKind::RequireOperatorReview
                    | AnchorDecisionKind::TentativeLink
                    | AnchorDecisionKind::MarkContradiction
            );

        Ok(AnchorPlan {
            packet_id: packet_id.clone(),
            candidates: adjusted_candidates,
            decision: final_decision,
            threshold_band: final_band,
            requires_operator_review,
            negative_assertions,
            redirects,
        })
    }

    /// §8.2 threshold → (decision, band).
    fn threshold_decision(&self, score: f64) -> (AnchorDecisionKind, ThresholdBand) {
        if score >= 0.80 {
            (AnchorDecisionKind::StrongLink, ThresholdBand::Strong)
        } else if score >= 0.60 {
            (AnchorDecisionKind::TentativeLink, ThresholdBand::Tentative)
        } else if score >= 0.40 {
            (AnchorDecisionKind::CreateNode, ThresholdBand::Weak)
        } else {
            (AnchorDecisionKind::MarkUnanchored, ThresholdBand::Unanchored)
        }
    }

    /// §8.6 + ImplementedBy + Supersedes validation.
    fn validate_edge_kind(&self, c: &AnchorCandidate, _graph: &ConceptGraph) -> Result<(), GateError> {
        // Faz 1: ImplementedBy yasak (code evidence Faz 4)
        if c.edge_kind == ConceptEdgeKind::ImplementedBy {
            return Err(GateError::ImplementedByRequiresCodeEvidence);
        }
        // INV-C4: Supersedes authority (Faz 1'de hiçbir kaynak yetkili değil — operator API Faz 8)
        if c.edge_kind == ConceptEdgeKind::Supersedes {
            return Err(GateError::SupersedeAuthorityRequired);
        }
        // §8.6: ConceptPacket → doğrudan CodeEntity (Candidate değil) yasak
        if c.edge_kind == ConceptEdgeKind::ExpectedImplementation
            && c.target_node_id.0.starts_with("CodeEntity:")
        {
            return Err(GateError::IllegalDirectCodeBinding {
                from: c.packet_id.clone().into_node_id(),
                to: c.target_node_id.clone(),
            });
        }
        Ok(())
    }

    /// INV-C8 canon gate: CreateNode öncesi mevcut node redirect kontrolü (3 katman).
    /// Match varsa Some(redirect) — hata değil, başarılı redirect.
    fn canon_gate_check(&self, c: &AnchorCandidate, graph: &ConceptGraph) -> Option<CanonicalRedirect> {
        // Sadece yeni Concept node adayları için (target Concept: prefix)
        let target = &c.target_node_id.0;
        let (_, name) = target.split_once(':')?;
        let name = name.trim();

        // 1. Exact canonical match
        let exact = graph.find_concept_by_canonical(name);
        if let Some(existing) = exact.first() {
            return Some(CanonicalRedirect {
                attempted: name.to_string(),
                existing_node: existing.id.clone(),
                reason: CanonicalRedirectReason::ExactCanonicalMatch,
            });
        }

        // 2. Glossary alias match
        if let Some(canonical) = self.glossary.canonical_for(name) {
            let canonical_id = ConceptNodeId(format!("Concept:{canonical}"));
            if graph.node(&canonical_id).is_some() {
                return Some(CanonicalRedirect {
                    attempted: name.to_string(),
                    existing_node: canonical_id,
                    reason: CanonicalRedirectReason::GlossaryAliasMatch,
                });
            }
        }

        // 3. Edit distance ≤2 — mevcut concept canonical'larına karşı
        for node in graph.nodes.values() {
            if matches!(node.node_kind, crate::anchoring::types::ConceptNodeKind::Concept) {
                if let Some(distance) = within_edit_distance_2(name, &node.canonical) {
                    return Some(CanonicalRedirect {
                        attempted: name.to_string(),
                        existing_node: node.id.clone(),
                        reason: CanonicalRedirectReason::EditDistanceLe2 { distance },
                    });
                }
            }
        }

        None
    }

    /// §6.4.1 contradiction negative assertions (fix_007).
    fn contradiction_negative_assertions(&self, candidates: &[AnchorCandidate]) -> Vec<String> {
        let mut out = Vec::new();
        let has_code_entity = candidates.iter().any(|c| {
            c.target_node_id.0.starts_with("CodeEntity:") || c.target_node_id.0.starts_with("CodeEntityCandidate:")
        });
        if has_code_entity {
            out.push(
                "CodeEntity --SUPERSEDES--> Decision yasak (INV-C4, §6.4.1: kod güçlüdür ama karar değildir)".into(),
            );
            out.push(
                "CodeEntity sadece --DRIFTS_FROM--> / --CONTRADICTS?--> üretebilir".into(),
            );
        }
        out.push(
            "Agent kaynağı --SUPERSEDES--> AcceptedDecision yapamaz; sadece CONTRADICTS? önerir (INV-C4 capability gate)".into(),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchoring::types::{AnchorScoreBreakdown, ConceptNode, ConceptNodeKind, ConceptPacketId, ConceptNodeId, PacketSource};
    use crate::anchoring::{ConceptEdgeKind, DecisionStatus, PositionFamily};

    fn glossary() -> Glossary {
        Glossary::seed_default()
    }

    fn candidate(target: &str, kind: ConceptEdgeKind, score: f64, expl: Option<&str>) -> AnchorCandidate {
        // Test helper: tüm pozitif bileşenleri `score`'a eşitle → raw_total ≈ score
        // (ağırlık toplamı 1.0). penalty'ler 0. Böylece threshold testleri `score` ile
        // direkt ilişki kurabilir (0.90 → StrongLink, 0.70 → TentativeLink, vb.).
        let b = AnchorScoreBreakdown {
            semantic_similarity: score,
            ontology_type_compatibility: score,
            graph_context_score: score,
            domain_term_match: score,
            code_evidence_score: score,
            temporal_trust_score: score,
            decision_status_score: score,
            contradiction_penalty: 0.0,
            staleness_penalty: 0.0,
        };
        AnchorCandidate {
            packet_id: ConceptPacketId("pkt:1".into()),
            target_node_id: ConceptNodeId(target.into()),
            edge_kind: kind,
            score: b,
            explanation: expl.map(String::from),
        }
    }

    fn empty_graph() -> ConceptGraph {
        ConceptGraph::new()
    }

    #[test]
    fn threshold_strong() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.90, None);
        let plan = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::StrongLink);
        assert_eq!(plan.threshold_band, ThresholdBand::Strong);
    }

    #[test]
    fn threshold_tentative() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.70, None);
        let plan = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::TentativeLink);
        assert_eq!(plan.threshold_band, ThresholdBand::Tentative);
    }

    #[test]
    fn threshold_unanchored() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.20, None);
        let plan = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::MarkUnanchored);
        assert_eq!(plan.threshold_band, ThresholdBand::Unanchored);
    }

    #[test]
    fn inv_c7_high_stake_requires_explanation() {
        let g = AnchorGate::new(glossary());
        // DerivesRisk high-stake, explanation yok → error
        let c = candidate("RiskCandidate:X", ConceptEdgeKind::DerivesRisk, 0.70, None);
        let err = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap_err();
        assert!(matches!(err, GateError::MissingExplanation { .. }));
    }

    #[test]
    fn inv_c7_high_stake_with_explanation_ok() {
        let g = AnchorGate::new(glossary());
        let c = candidate("RiskCandidate:X", ConceptEdgeKind::DerivesRisk, 0.70, Some("risk derived"));
        let plan = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap();
        assert!(plan.requires_operator_review, "high-stake → review");
    }

    #[test]
    fn gate_rejects_implemented_by_without_code_evidence() {
        let g = AnchorGate::new(glossary());
        let c = candidate("CodeEntity:X", ConceptEdgeKind::ImplementedBy, 0.90, Some("impl"));
        let err = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap_err();
        assert_eq!(err, GateError::ImplementedByRequiresCodeEvidence);
    }

    #[test]
    fn gate_rejects_supersedes_without_authority() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Decision:X", ConceptEdgeKind::Supersedes, 0.90, Some("super"));
        let err = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap_err();
        assert_eq!(err, GateError::SupersedeAuthorityRequired);
    }

    #[test]
    fn section_8_6_illegal_direct_code_binding() {
        let g = AnchorGate::new(glossary());
        // ExpectedImplementation → gerçek CodeEntity (Candidate değil) → §8.6 yasak
        let c = candidate("CodeEntity:PaymentService", ConceptEdgeKind::ExpectedImplementation, 0.90, Some("expected"));
        let err = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap_err();
        assert!(matches!(err, GateError::IllegalDirectCodeBinding { .. }));
    }

    #[test]
    fn inv_c8_canon_gate_exact_match_redirect() {
        let g = AnchorGate::new(glossary());
        // Graph'ta Concept:Payment var; "Payment" için CreateNode → redirect
        let mut graph = empty_graph();
        graph.insert_node(ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        });
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.50, None);
        let plan = g.decide(&ConceptPacketId("p".into()), vec![c], &graph).unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::CreateNode);
        assert_eq!(plan.redirects.len(), 1);
        assert_eq!(plan.redirects[0].reason, CanonicalRedirectReason::ExactCanonicalMatch);
    }

    #[test]
    fn inv_c8_canon_gate_alias_match_redirect() {
        let g = AnchorGate::new(glossary());
        let mut graph = empty_graph();
        graph.insert_node(ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec!["ödeme".into()],
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        });
        // "ödeme" alias → Concept:Payment redirect
        let c = candidate("Concept:ödeme", ConceptEdgeKind::Mentions, 0.50, None);
        let plan = g.decide(&ConceptPacketId("p".into()), vec![c], &graph).unwrap();
        assert!(!plan.redirects.is_empty());
    }

    #[test]
    fn section_6_4_1_contradiction_negative_assertions() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Decision:X", ConceptEdgeKind::Contradicts, 0.50, Some("contradicts"));
        let plan = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap();
        assert_eq!(plan.decision, AnchorDecisionKind::MarkContradiction);
        assert!(!plan.negative_assertions.is_empty(), "§6.4.1 negative assertions doldu");
        assert!(plan.negative_assertions.iter().any(|s| s.contains("SUPERSEDES")));
    }

    #[test]
    fn requires_review_false_when_only_low_stake() {
        let g = AnchorGate::new(glossary());
        let c = candidate("Concept:Payment", ConceptEdgeKind::Mentions, 0.50, None);
        let plan = g.decide(&ConceptPacketId("p".into()), vec![c], &empty_graph()).unwrap();
        assert!(!plan.requires_operator_review, "düşük-stake Mentions → review gerekmez");
    }
}
