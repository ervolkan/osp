//! Lexical/domain Extractor (Faz 1, §11).
//!
//! ConceptPacket + ConceptGraph → `Vec<ExtractedAnchorCandidate>`.
//! Glossary match, typed-prefix parse, rule/risk sinyali tespiti. §8.6 zincir kuralı.

use crate::anchoring::classifier::{Classifier, Glossary};
use crate::anchoring::types::{
    ConceptGraph, ConceptNodeKind, ConceptNodeId, ConceptPacket, ExtractedAnchorCandidate,
};
use crate::anchoring::ConceptEdgeKind;
use crate::anchoring::typed_ref::TypedNodeRef;

/// Lexical/domain extractor. Glossary + classifier'a bağlı.
pub struct Extractor<'a> {
    glossary: &'a Glossary,
    classifier: &'a Classifier,
}

impl<'a> Extractor<'a> {
    pub fn new(glossary: &'a Glossary, classifier: &'a Classifier) -> Self {
        Self { glossary, classifier }
    }

    /// Packet'ten aday edge/node'ları çıkar (score'suz — scorer ekler).
    ///
    /// Not: `graph` parametresi Faz 1'de kullanılmaz (extractor glossary-driven);
    /// Faz 2'de graph context (mevcut node/komşu) zenginleştirmesi için hazır.
    #[allow(unused_variables)]
    pub fn extract(&self, packet: &ConceptPacket, graph: &ConceptGraph) -> Vec<ExtractedAnchorCandidate> {
        let mut candidates = Vec::new();
        let text_lower = packet.text.to_lowercase();

        // 1. Glossary term match → Concept Mentions (düşük stake)
        for entry in self.glossary.entries() {
            let terms = std::iter::once(&entry.canonical).chain(entry.aliases.iter());
            for term in terms {
                if text_lower.contains(&term.to_lowercase()) {
                    let target = ConceptNodeId(format!("Concept:{}", entry.canonical));
                    candidates.push(ExtractedAnchorCandidate {
                        packet_id: packet.id.clone(),
                        target_node_id: target,
                        edge_kind: ConceptEdgeKind::Mentions,
                        explanation: None, // düşük stake, opsiyonel
                    });
                    break; // her canonical için bir Mentions
                }
            }
        }

        // 2. Typed-prefix referansları parse (CodeEntityCandidate:Foo, Decision:Bar)
        for word in packet.text.split_whitespace() {
            let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != '_');
            if let Some(r) = TypedNodeRef::parse(cleaned) {
                let edge_kind = match r.kind {
                    ConceptNodeKind::CodeEntityCandidate => ConceptEdgeKind::ExpectedImplementation,
                    ConceptNodeKind::Decision => {
                        if self.is_contradiction_context(&text_lower) {
                            ConceptEdgeKind::Contradicts
                        } else {
                            ConceptEdgeKind::DependsOnDecision
                        }
                    }
                    ConceptNodeKind::RiskCandidate => ConceptEdgeKind::DerivesRisk,
                    ConceptNodeKind::CodeEntity => {
                        // §8.6: gerçek CodeEntity doğrudan bağlanamaz — candidate olmadan
                        continue;
                    }
                    _ => ConceptEdgeKind::Mentions,
                };
                let explanation = if edge_kind.is_high_stake() {
                    Some(format!("Extracted typed ref {}:{}", r.kind.as_prefix(), r.name))
                } else {
                    None
                };
                candidates.push(ExtractedAnchorCandidate {
                    packet_id: packet.id.clone(),
                    target_node_id: r.to_node_id(),
                    edge_kind,
                    explanation,
                });
            }
        }

        // 3. Rule sinyali → DerivesRule (high stake)
        if self.classifier.has_rule_signal(&packet.text) {
            // Mevcut RuleCandidate yoksa yeni türet
            let rule_name = self.derive_rule_name(&packet.text);
            let target = ConceptNodeId(format!("RuleCandidate:{rule_name}"));
            candidates.push(ExtractedAnchorCandidate {
                packet_id: packet.id.clone(),
                target_node_id: target,
                edge_kind: ConceptEdgeKind::DerivesRule,
                explanation: Some(format!("Rule derived from: {}", packet.text)),
            });
        }

        // 4. Risk sinyali → DerivesRisk (high stake, packet type'dan bağımsız)
        if self.classifier.has_risk_signal(&packet.text) {
            let risk_name = self.derive_risk_name(&packet.text);
            let target = ConceptNodeId(format!("RiskCandidate:{risk_name}"));
            candidates.push(ExtractedAnchorCandidate {
                packet_id: packet.id.clone(),
                target_node_id: target,
                edge_kind: ConceptEdgeKind::DerivesRisk,
                explanation: Some(format!("Risk derived from: {}", packet.text)),
            });
        }

        // 5. AntiGoalOf — packet type AntiGoal ise mevcut Concept'e
        if matches!(packet.packet_type, crate::anchoring::ConceptPacketType::AntiGoal) {
            if let Some(target_concept) = self.first_concept_target(&candidates) {
                candidates.push(ExtractedAnchorCandidate {
                    packet_id: packet.id.clone(),
                    target_node_id: target_concept.clone(),
                    edge_kind: ConceptEdgeKind::AntiGoalOf,
                    explanation: Some(format!("AntiGoal of {}", target_concept.0)),
                });
            }
        }

        // Dedup: aynı (target, kind) çifti tekrar etmesin
        dedup_candidates(&mut candidates);

        candidates
    }

    fn is_contradiction_context(&self, text_lower: &str) -> bool {
        text_lower.contains("çeliş") || text_lower.contains("ihlal") || text_lower.contains("contradict")
    }

    fn derive_rule_name(&self, text: &str) -> String {
        // Basit: ilk 3 anlamlı kelime → PascalCase
        let words: Vec<String> = text
            .split_whitespace()
            .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
            .take(3)
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect();
        if words.is_empty() {
            "DerivedRule".to_string()
        } else {
            words.join("")
        }
    }

    fn derive_risk_name(&self, text: &str) -> String {
        // Domain glossary'den concept bul, risk adı üret
        let lower = text.to_lowercase();
        let mut related = Vec::new();
        for entry in self.glossary.entries() {
            let terms = std::iter::once(&entry.canonical).chain(entry.aliases.iter());
            for term in terms {
                if lower.contains(&term.to_lowercase()) {
                    related.push(entry.canonical.clone());
                    break;
                }
            }
        }
        if related.is_empty() {
            "GenericRisk".to_string()
        } else {
            format!("{}Loss", related.join("")) // PaymentTrustLoss gibi
        }
    }

    fn first_concept_target<'b>(
        &self,
        candidates: &'b [ExtractedAnchorCandidate],
    ) -> Option<&'b ConceptNodeId> {
        candidates
            .iter()
            .find(|c| c.edge_kind == ConceptEdgeKind::Mentions)
            .map(|c| &c.target_node_id)
    }
}

fn dedup_candidates(candidates: &mut Vec<ExtractedAnchorCandidate>) {
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert((c.target_node_id.0.clone(), c.edge_kind)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchoring::classifier::{Classifier, Glossary};
    use crate::anchoring::types::{ConceptPacket, PacketSource};
    use crate::anchoring::ConceptPacketType;

    fn make_packet(text: &str, pt: ConceptPacketType) -> ConceptPacket {
        ConceptPacket::new("pkt:test", pt, text, "tr", PacketSource::ExplicitUser)
    }

    #[test]
    fn extract_glossary_mentions_fix_001() {
        let g = Glossary::seed_default();
        let c = Classifier::new();
        let ex = Extractor::new(&g, &c);
        let pkt = make_packet(
            "Kullanıcı ödeme yaparken kendini güvende hissetmeli.",
            ConceptPacketType::UserVision,
        );
        let graph = ConceptGraph::new();
        let cands = ex.extract(&pkt, &graph);

        // Payment + Trust Mentions
        assert!(cands.iter().any(|c| c.target_node_id.0 == "Concept:Payment" && c.edge_kind == ConceptEdgeKind::Mentions));
        assert!(cands.iter().any(|c| c.target_node_id.0 == "Concept:Trust" && c.edge_kind == ConceptEdgeKind::Mentions));
    }

    #[test]
    fn extract_derives_risk_from_signal() {
        let g = Glossary::seed_default();
        let c = Classifier::new();
        let ex = Extractor::new(&g, &c);
        let pkt = make_packet(
            "Kullanıcı ödeme yaparken kendini güvende hissetmeli.",
            ConceptPacketType::UserVision,
        );
        let cands = ex.extract(&pkt, &ConceptGraph::new());

        let risk = cands.iter().find(|c| c.edge_kind == ConceptEdgeKind::DerivesRisk);
        assert!(risk.is_some(), "DerivesRisk üretilmeli");
        let risk = risk.unwrap();
        assert!(risk.target_node_id.0.starts_with("RiskCandidate:"));
        assert!(risk.explanation.is_some(), "high-stake explanation zorunlu");
    }

    #[test]
    fn extract_derives_rule_from_signal() {
        let g = Glossary::seed_default();
        let c = Classifier::new();
        let ex = Extractor::new(&g, &c);
        let pkt = make_packet(
            "Domain katmanı Infrastructure'a bağımlı olmamalı.",
            ConceptPacketType::Requirement,
        );
        let cands = ex.extract(&pkt, &ConceptGraph::new());

        let rule = cands.iter().find(|c| c.edge_kind == ConceptEdgeKind::DerivesRule);
        assert!(rule.is_some(), "DerivesRule üretilmeli");
        assert!(rule.unwrap().target_node_id.0.starts_with("RuleCandidate:"));
    }

    #[test]
    fn extract_dedup_same_target_kind() {
        let g = Glossary::seed_default();
        let c = Classifier::new();
        let ex = Extractor::new(&g, &c);
        // "ödeme" iki kez → tek Mentions
        let pkt = make_packet("ödeme ödeme", ConceptPacketType::UserVision);
        let cands = ex.extract(&pkt, &ConceptGraph::new());
        let payment_mentions = cands
            .iter()
            .filter(|c| c.target_node_id.0 == "Concept:Payment" && c.edge_kind == ConceptEdgeKind::Mentions)
            .count();
        assert_eq!(payment_mentions, 1, "dedup tek Mentions bırakmalı");
    }

    #[test]
    fn extract_unanchored_empty_when_no_match() {
        let g = Glossary::seed_default();
        let c = Classifier::new();
        let ex = Extractor::new(&g, &c);
        let pkt = make_packet(
            "Belki hafta sonu bazı şeyleri gözden geçirmek lazım.",
            ConceptPacketType::Assumption,
        );
        let cands = ex.extract(&pkt, &ConceptGraph::new());
        assert!(cands.is_empty(), "glossary/rule/risk match yok → boş");
    }
}
