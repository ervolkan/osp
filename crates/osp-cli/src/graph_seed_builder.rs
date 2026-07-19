//! One-shot GraphSeedBuilder — ortak graph invariant sınırı (B1 + O1 + O3).
//!
//! İki source (analysis + legacy JSON) ayrı modelleriyle `GraphSeedNodeDraft` üretir;
//! builder source semantiğini BİLMEZ — yalnız graph-level invariantları uygular:
//! node-id collision, duplicate node, stable ordering (source-provided), bucket placement.
//!
//! # Ordering (O1)
//! Builder source draft sırasını değiştirmez; yeni sıra ÜRETMEZ. Deterministik ordering
//! source modellerinin sorumluluğu: AnalysisCandidateSeed `(identity_key, display_path)` sort;
//! CandidateSeedFile mevcut JSON insertion-order. Builder insertion-order korur.
//!
//! # One-shot (B1)
//! `build(drafts) -> Result<GraphSeed, _>` — ya tam GraphSeed, ya hata. Partial state dışarı sızamaz.

// seed_file.rs + commands/graph.rs integration tamamlanana kadar dead-code.

use std::collections::HashMap;

use osp_core::anchoring::types::{ConceptNode, ConceptNodeId, ConceptNodeKind, GraphSeed};
use osp_core::anchoring::{DecisionStatus, PositionFamily};

/// Graph node taslağı — private fields (O3). Illegal state constructor sınırında.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphSeedNodeDraft {
    id: ConceptNodeId,
    canonical: String,
    aliases: Vec<String>,
    kind: ConceptNodeKind,
    status: DecisionStatus,
    family: PositionFamily,
}

impl GraphSeedNodeDraft {
    /// Analysis source: Candidate + PhysicalCode + CodeEntityCandidate + aliases=[].
    /// INV-C5 sınırı: Accepted üretemez (constructor'baked).
    pub(crate) fn analysis_code_entity(id: ConceptNodeId, canonical: String) -> Self {
        Self {
            id,
            canonical,
            aliases: Vec::new(),
            kind: ConceptNodeKind::CodeEntityCandidate,
            status: DecisionStatus::Candidate,
            family: PositionFamily::PhysicalCode,
        }
    }

    /// Legacy JSON source: Candidate + ConceptualIntent (F1). seed_file.rs adapter.
    pub(crate) fn legacy_candidate(
        id: ConceptNodeId,
        canonical: String,
        aliases: Vec<String>,
        kind: ConceptNodeKind,
    ) -> Self {
        Self {
            id,
            canonical,
            aliases,
            kind,
            status: DecisionStatus::Candidate,
            family: PositionFamily::ConceptualIntent,
        }
    }

    #[allow(dead_code)] // analysis bridge into_drafts tamamlanınca kullanılacak
    pub(crate) fn id(&self) -> &ConceptNodeId {
        &self.id
    }

    /// Material özeti — duplicate/collision karşılaştırması (6 alan).
    fn material_matches(&self, other: &Self) -> bool {
        self.canonical == other.canonical
            && self.aliases == other.aliases
            && self.kind == other.kind
            && self.status == other.status
            && self.family == other.family
    }
}

/// One-shot builder — source sırasını korur, partial state imkânsız.
pub(crate) struct GraphSeedBuilder;

impl GraphSeedBuilder {
    /// Draftları GraphSeed'e dönüştür — ya tam seed, ya hata (B1).
    /// Source draft sırasını korur (O1); insertion-order preserved.
    pub fn build(
        drafts: impl IntoIterator<Item = GraphSeedNodeDraft>,
    ) -> Result<GraphSeed, GraphSeedBuilderError> {
        // seen: collision/duplicate dedup (HashMap<id, first_draft>).
        // ordered: source-provided insertion-order (Vec).
        let mut seen: HashMap<ConceptNodeId, GraphSeedNodeDraft> = HashMap::new();
        let mut ordered: Vec<GraphSeedNodeDraft> = Vec::new();

        for draft in drafts {
            if let Some(existing) = seen.get(&draft.id) {
                // 6-alan material karşılaştırması (N').
                if existing.material_matches(&draft) {
                    return Err(GraphSeedBuilderError::DuplicateNode {
                        node_id: draft.id.clone(),
                        canonical: draft.canonical.clone(),
                    });
                } else {
                    return Err(GraphSeedBuilderError::NodeIdCollision {
                        node_id: draft.id.clone(),
                        first: existing.material_summary(),
                        second: draft.material_summary(),
                    });
                }
            }
            seen.insert(draft.id.clone(), draft.clone());
            ordered.push(draft);
        }

        // Source-provided insertion-order ile GraphSeed üret (bucket placement).
        let mut seed = GraphSeed::default();
        for draft in ordered {
            let concept_node = ConceptNode {
                id: draft.id,
                canonical: draft.canonical,
                aliases: draft.aliases,
                node_kind: draft.kind,
                decision_status: draft.status,
                position_family: draft.family,
            };
            match concept_node.node_kind {
                ConceptNodeKind::Concept => seed.concepts.push(concept_node),
                ConceptNodeKind::Decision => seed.decisions.push(concept_node),
                ConceptNodeKind::CodeEntity => seed.code_entities.push(concept_node),
                ConceptNodeKind::RuleCandidate => seed.rule_candidates.push(concept_node),
                ConceptNodeKind::TaskCandidate => seed.task_candidates.push(concept_node),
                ConceptNodeKind::RiskCandidate => seed.risk_candidates.push(concept_node),
                ConceptNodeKind::Risk => seed.risk_candidates.push(concept_node),
                ConceptNodeKind::CodeEntityCandidate => seed.code_entities.push(concept_node),
            }
        }
        Ok(seed)
    }
}

/// Material özeti — collision diagnostic (tüm karşılaştırma alanları).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MaterialSummary {
    pub canonical: String,
    pub aliases: Vec<String>,
    pub kind: ConceptNodeKind,
    pub status: DecisionStatus,
    pub family: PositionFamily,
}

impl GraphSeedNodeDraft {
    fn material_summary(&self) -> MaterialSummary {
        MaterialSummary {
            canonical: self.canonical.clone(),
            aliases: self.aliases.clone(),
            kind: self.kind,
            status: self.status,
            family: self.family,
        }
    }
}

/// Builder hatası — duplicate vs collision ayrımı (N').
#[derive(Debug, thiserror::Error)]
pub enum GraphSeedBuilderError {
    #[error("duplicate node (same material): id={node_id}, canonical={canonical}")]
    DuplicateNode {
        node_id: ConceptNodeId,
        canonical: String,
    },
    #[error(
        "node-id collision (different material): id={node_id}, first={first:?}, second={second:?}"
    )]
    NodeIdCollision {
        node_id: ConceptNodeId,
        first: MaterialSummary,
        second: MaterialSummary,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(id: &str, canonical: &str) -> GraphSeedNodeDraft {
        GraphSeedNodeDraft::analysis_code_entity(ConceptNodeId(id.into()), canonical.into())
    }

    fn legacy(id: &str, canonical: &str, kind: ConceptNodeKind) -> GraphSeedNodeDraft {
        GraphSeedNodeDraft::legacy_candidate(
            ConceptNodeId(id.into()),
            canonical.into(),
            vec![],
            kind,
        )
    }

    #[test]
    fn happy_path_builds_graph_seed() {
        let seed = GraphSeedBuilder::build(vec![
            draft("CodeEntityCandidate:src/a.rs", "src/a.rs"),
            draft("CodeEntityCandidate:src/b.rs", "src/b.rs"),
        ])
        .unwrap();
        assert_eq!(seed.code_entities.len(), 2);
        assert_eq!(seed.code_entities[0].id.0, "CodeEntityCandidate:src/a.rs");
        assert_eq!(seed.code_entities[1].id.0, "CodeEntityCandidate:src/b.rs");
    }

    #[test]
    fn empty_drafts_empty_seed() {
        let seed = GraphSeedBuilder::build(vec![]).unwrap();
        assert!(seed.concepts.is_empty());
        assert!(seed.code_entities.is_empty());
    }

    // ── B1: one-shot, partial imkânsız ───────────────────────────────────────

    #[test]
    fn duplicate_same_material_rejected() {
        // Aynı id + aynı material → DuplicateNode.
        let err = GraphSeedBuilder::build(vec![
            draft("CodeEntityCandidate:src/a.rs", "src/a.rs"),
            draft("CodeEntityCandidate:src/a.rs", "src/a.rs"),
        ])
        .unwrap_err();
        assert!(matches!(err, GraphSeedBuilderError::DuplicateNode { .. }));
    }

    #[test]
    fn collision_different_material_rejected() {
        // Aynı id + farklı material → NodeIdCollision.
        // Analysis_code_entity (PhysicalCode) vs legacy_candidate (ConceptualIntent) aynı id.
        let err = GraphSeedBuilder::build(vec![
            draft("Concept:X", "X"),
            legacy("Concept:X", "X", ConceptNodeKind::Concept), // ConceptualIntent
        ])
        .unwrap_err();
        assert!(matches!(err, GraphSeedBuilderError::NodeIdCollision { .. }));
    }

    // ── O1: source-order preservation ────────────────────────────────────────

    #[test]
    fn source_order_preserved() {
        // Builder input sırasını korur (permutation invariance iddia ETME).
        let seed = GraphSeedBuilder::build(vec![
            draft("CodeEntityCandidate:src/zebra.rs", "src/zebra.rs"),
            draft("CodeEntityCandidate:src/apple.rs", "src/apple.rs"),
            draft("CodeEntityCandidate:src/mango.rs", "src/mango.rs"),
        ])
        .unwrap();
        // Insertion-order (Zebra, Apple, Mango) — sort edilmez.
        assert_eq!(seed.code_entities[0].canonical, "src/zebra.rs");
        assert_eq!(seed.code_entities[1].canonical, "src/apple.rs");
        assert_eq!(seed.code_entities[2].canonical, "src/mango.rs");
    }

    // ── O3: illegal state constructor sınırında ───────────────────────────────

    #[test]
    fn analysis_code_entity_is_candidate_physicalcode() {
        let d = draft("CodeEntityCandidate:x", "x");
        assert_eq!(d.status, DecisionStatus::Candidate);
        assert_eq!(d.family, PositionFamily::PhysicalCode);
        assert_eq!(d.kind, ConceptNodeKind::CodeEntityCandidate);
        assert!(d.aliases.is_empty());
    }

    #[test]
    fn legacy_candidate_is_candidate_conceptualintent() {
        let d = legacy("Concept:X", "X", ConceptNodeKind::Concept);
        assert_eq!(d.status, DecisionStatus::Candidate);
        assert_eq!(d.family, PositionFamily::ConceptualIntent);
        assert_eq!(d.kind, ConceptNodeKind::Concept);
    }

    // ── Bucket placement ──────────────────────────────────────────────────────

    #[test]
    fn bucket_placement_by_kind() {
        let seed = GraphSeedBuilder::build(vec![
            legacy("Concept:A", "A", ConceptNodeKind::Concept),
            legacy("RuleCandidate:B", "B", ConceptNodeKind::RuleCandidate),
            draft("CodeEntityCandidate:src/c.rs", "src/c.rs"),
        ])
        .unwrap();
        assert_eq!(seed.concepts.len(), 1);
        assert_eq!(seed.rule_candidates.len(), 1);
        assert_eq!(seed.code_entities.len(), 1);
    }
}
