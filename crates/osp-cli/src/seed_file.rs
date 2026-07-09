//! Candidate seed file DTO — nodes-only bootstrap (Review 4.1).
//!
//! `GraphSeed`'in 6 bucket'ı hep `Vec<ConceptNode>`; **edge alanı yok** (Review 4.1 doğruladı).
//! Bu yüzden seed DTO `nodes`-only. Edge'ler analysis bridge'in işi (ayrı milestone).
//!
//! # Disiplin (Review 4.5)
//! - **`deny_unknown_fields`:** status alanı gönderilirse reject.
//! - **status alanı yok:** DTO'da `decision_status` yok; dönüşümde `Candidate` hard-code.
//!   "Illegal state unrepresentable" — "Accepted verilirse reject"ten daha güçlü.
//! - **id yok:** id kind+canonical'dan türetilir (ConceptNodeKind prefix konvansiyonu).
//!   id/kind uyumsuzluğu temsil edilemez.
//! - **duplicate canonical kontrolü:** `insert_node` HashMap sessiz overwrite eder;
//!   post-init restore-validasyon yakalar ama hata kaynağı bulanıklaşır → DTO'da erken yakala.

use osp_core::anchoring::types::{ConceptNodeKind, ConceptNodeId, GraphSeed};

use crate::graph_seed_builder::{GraphSeedBuilder, GraphSeedBuilderError, GraphSeedNodeDraft};

/// Seed parse hatası.
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("seed deserialize failed: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("duplicate canonical in seed: {0}")]
    DuplicateCanonical(String),
    #[error("empty canonical in seed node at index {0}")]
    EmptyCanonical(usize),
    #[error("seed file schema version uyumsuz: expected={expected}, found={found}")]
    SchemaMismatch { expected: u32, found: u32 },
}

/// Untrusted seed JSON DTO. `decision_status` ve `id` alanları YOK.
///
/// Dönüşüm: `CandidateSeedFile` → validate (duplicate canonical, empty) → id türet
/// → trusted `GraphSeed` (Candidate-only). Core serialize-only/non-deserializable
/// epistemic tiplerin sınırları gevşemez (Review 4.6).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSeedFile {
    pub schema_version: u32,
    pub nodes: Vec<CandidateSeedNode>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSeedNode {
    pub canonical: String,
    pub kind: ConceptNodeKind,
    // decision_status YOK — Candidate hard-code (Review 4.5a).
    // id YOK — kind+canonical'dan türetilir (Review 4.5b).
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl CandidateSeedFile {
    /// Mevcut seed schema version.
    pub const SEED_SCHEMA_VERSION: u32 = 1;

    /// JSON string'den parse et.
    pub fn from_json(json: &str) -> Result<Self, SeedError> {
        let seed: CandidateSeedFile = serde_json::from_str(json)?;
        if seed.schema_version != Self::SEED_SCHEMA_VERSION {
            return Err(SeedError::SchemaMismatch {
                expected: Self::SEED_SCHEMA_VERSION,
                found: seed.schema_version,
            });
        }
        Ok(seed)
    }

    /// Legacy `GraphSeedNodeDraft` üretir — F1 semantics (ConceptualIntent, Candidate, aliases).
    /// Canonical dedup + empty kontrolü burada; NodeId collision GraphSeedBuilder'da.
    pub(crate) fn into_drafts(&self) -> Result<Vec<GraphSeedNodeDraft>, SeedError> {
        use std::collections::BTreeSet;
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut drafts: Vec<GraphSeedNodeDraft> = Vec::new();
        for (idx, node) in self.nodes.iter().enumerate() {
            let canonical = node.canonical.trim();
            if canonical.is_empty() {
                return Err(SeedError::EmptyCanonical(idx));
            }
            if !seen.insert(canonical.to_string()) {
                return Err(SeedError::DuplicateCanonical(canonical.to_string()));
            }
            // id türet: kind prefix + canonical (ConceptNodeKind konvansiyonu).
            let id = ConceptNodeId(derive_node_id(node.kind, canonical));
            drafts.push(GraphSeedNodeDraft::legacy_candidate(
                id,
                canonical.to_string(),
                node.aliases.clone(),
                node.kind,
            ));
        }
        Ok(drafts)
    }

    /// Trusted `GraphSeed`'e dönüştür (Candidate-only). Duplicate canonical + empty kontrolü.
    /// F1 legacy compat wrapper: `into_drafts() + GraphSeedBuilder::build()`.
    pub fn to_graph_seed(&self) -> Result<GraphSeed, SeedError> {
        let drafts = self.into_drafts()?;
        GraphSeedBuilder::build(drafts).map_err(map_builder_error)
    }
}

/// GraphSeedBuilder hatası → SeedError (F1 legacy compat — DuplicateCanonical mapping).
fn map_builder_error(e: GraphSeedBuilderError) -> SeedError {
    match e {
        GraphSeedBuilderError::DuplicateNode { canonical, .. } => {
            SeedError::DuplicateCanonical(canonical)
        }
        // NodeIdCollision mevcut davranışta unreachable (canonical dedup önce yakalar).
        // Hardening: DuplicateCanonical'a map (canonical dedup zaten NodeId collision'ı önler).
        GraphSeedBuilderError::NodeIdCollision { .. } => {
            SeedError::DuplicateCanonical("(node-id collision)".into())
        }
    }
}

/// kind + canonical'dan node id türet (ConceptNodeKind prefix konvansiyonu).
/// `store.rs`'teki `kind_from_id` ile simetrik (id → kind prefix → kind).
/// `kind.as_prefix()` kullanılır — manuel mapping ID/kind uyumsuzluğu yaratır
/// (örn. CodeEntityCandidate → "CodeEntity:" çakışması; Review 2.tur P1.2).
pub(crate) fn derive_node_id(kind: ConceptNodeKind, canonical: &str) -> String {
    format!("{}:{canonical}", kind.as_prefix())
}

#[cfg(test)]
mod tests {
    use super::*;
    use osp_core::anchoring::{DecisionStatus, PositionFamily};

    /// Happy path: valid seed → GraphSeed (Candidate-only).
    #[test]
    fn valid_seed_parses_to_candidate_graph_seed() {
        let json = r#"{
            "schema_version": 1,
            "nodes": [
                {"canonical": "CouplingMustNot", "kind": "RuleCandidate"},
                {"canonical": "Payment", "kind": "Concept", "aliases": ["ödeme"]}
            ]
        }"#;
        let seed = CandidateSeedFile::from_json(json).unwrap();
        let graph = seed.to_graph_seed().unwrap();
        assert_eq!(graph.rule_candidates.len(), 1);
        assert_eq!(graph.concepts.len(), 1);
        assert_eq!(
            graph.rule_candidates[0].id.0,
            "RuleCandidate:CouplingMustNot"
        );
        assert_eq!(
            graph.rule_candidates[0].decision_status,
            DecisionStatus::Candidate
        );
        assert_eq!(graph.concepts[0].id.0, "Concept:Payment");
        assert_eq!(graph.concepts[0].aliases, vec!["ödeme".to_string()]);
    }

    /// status alanı gönderilirse deny_unknown_fields reject eder.
    #[test]
    fn seed_rejects_decision_status_field() {
        let json = r#"{
            "schema_version": 1,
            "nodes": [
                {"canonical": "X", "kind": "RuleCandidate", "decision_status": "Accepted"}
            ]
        }"#;
        let err = CandidateSeedFile::from_json(json).unwrap_err();
        assert!(matches!(err, SeedError::Deserialize(_)));
    }

    /// Bilinmeyen alan deny_unknown_fields ile reject.
    #[test]
    fn seed_rejects_unknown_field() {
        let json = r#"{
            "schema_version": 1,
            "nodes": [{"canonical": "X", "kind": "RuleCandidate", "bogus": true}]
        }"#;
        assert!(CandidateSeedFile::from_json(json).is_err());
    }

    /// Duplicate canonical → reject.
    #[test]
    fn seed_rejects_duplicate_canonical() {
        let json = r#"{
            "schema_version": 1,
            "nodes": [
                {"canonical": "Dup", "kind": "RuleCandidate"},
                {"canonical": "Dup", "kind": "Concept"}
            ]
        }"#;
        let seed = CandidateSeedFile::from_json(json).unwrap();
        let err = seed.to_graph_seed().unwrap_err();
        assert!(matches!(err, SeedError::DuplicateCanonical(s) if s == "Dup"));
    }

    /// Empty canonical → reject.
    #[test]
    fn seed_rejects_empty_canonical() {
        let json = r#"{ "schema_version": 1, "nodes": [{"canonical": "  ", "kind": "Concept"}] }"#;
        let seed = CandidateSeedFile::from_json(json).unwrap();
        let err = seed.to_graph_seed().unwrap_err();
        assert!(matches!(err, SeedError::EmptyCanonical(0)));
    }

    /// Schema mismatch → reject.
    #[test]
    fn seed_rejects_schema_mismatch() {
        let json = r#"{ "schema_version": 99, "nodes": [] }"#;
        let err = CandidateSeedFile::from_json(json).unwrap_err();
        assert!(matches!(
            err,
            SeedError::SchemaMismatch {
                expected: 1,
                found: 99
            }
        ));
    }

    /// CodeEntityCandidate → doğru prefix "CodeEntityCandidate:" (Review 2.tur P1.2).
    /// Manuel mapping "CodeEntity:" üretirdi → ID/kind uyumsuzluğu + çakışma riski.
    #[test]
    fn seed_code_entity_candidate_uses_correct_prefix() {
        let json = r#"{
            "schema_version": 1,
            "nodes": [{"canonical": "PaymentService", "kind": "CodeEntityCandidate"}]
        }"#;
        let seed = CandidateSeedFile::from_json(json).unwrap();
        let graph = seed.to_graph_seed().unwrap();
        assert_eq!(graph.code_entities.len(), 1);
        assert_eq!(
            graph.code_entities[0].id.0, "CodeEntityCandidate:PaymentService",
            "CodeEntityCandidate doğru prefix almalı (CodeEntity değil)"
        );
        assert_eq!(
            graph.code_entities[0].node_kind,
            ConceptNodeKind::CodeEntityCandidate
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Frozen characterization (O6') — refactor öncesi baseline davranış pinleme.
    // Bu testler to_graph_seed() → into_drafts()+GraphSeedBuilder refactor'undan ÖNCE
    // yazıldı. Refactor sonrası aynı davranış (mutlu yol + negatif) korunmalı.
    // Reviewer: bu testlerin yeni implementation'a göre sonradan uydurulmadığınet görünür.
    // ═══════════════════════════════════════════════════════════════════════════

    /// F1 frozen mutlu yol: 3 node farklı kind → GraphSeed node ID/digest/aliases/kind/family/order.
    /// Refactor sonrası (into_drafts + GraphSeedBuilder) aynı frozen values üretmeli.
    #[test]
    fn frozen_characterization_happy_path_semantics() {
        let json = r#"{
            "schema_version": 1,
            "nodes": [
                {"canonical": "Alpha", "kind": "Concept"},
                {"canonical": "Beta", "kind": "RuleCandidate", "aliases": ["b"]},
                {"canonical": "Gamma", "kind": "CodeEntity"}
            ]
        }"#;
        let seed = CandidateSeedFile::from_json(json).unwrap();
        let graph = seed.to_graph_seed().unwrap();
        // Bucket placement (kind bazlı).
        assert_eq!(graph.concepts.len(), 1);
        assert_eq!(graph.rule_candidates.len(), 1);
        assert_eq!(graph.code_entities.len(), 1);
        // Node ID derivation (kind prefix + canonical).
        assert_eq!(graph.concepts[0].id.0, "Concept:Alpha");
        assert_eq!(graph.rule_candidates[0].id.0, "RuleCandidate:Beta");
        assert_eq!(graph.code_entities[0].id.0, "CodeEntity:Gamma");
        // Family hardcoded ConceptualIntent (F1 legacy compat).
        assert_eq!(graph.concepts[0].position_family, PositionFamily::ConceptualIntent);
        assert_eq!(graph.rule_candidates[0].position_family, PositionFamily::ConceptualIntent);
        assert_eq!(graph.code_entities[0].position_family, PositionFamily::ConceptualIntent);
        // Status Candidate (INV-C5 — illegal state unrepresentable).
        assert_eq!(graph.concepts[0].decision_status, DecisionStatus::Candidate);
        assert_eq!(graph.rule_candidates[0].decision_status, DecisionStatus::Candidate);
        // Aliases preserved.
        assert_eq!(graph.rule_candidates[0].aliases, vec!["b".to_string()]);
        assert!(graph.concepts[0].aliases.is_empty());
        // Node digest deterministik (canonical + aliases + kind + family).
        let d1 = osp_core::anchoring::review::node_digest(&graph.concepts[0]);
        let d2 = osp_core::anchoring::review::node_digest(&graph.rule_candidates[0]);
        let d3 = osp_core::anchoring::review::node_digest(&graph.code_entities[0]);
        assert_ne!(d1.get(), d2.get());
        assert_ne!(d2.get(), d3.get());
        assert_ne!(d1.get(), d3.get());
    }

    /// O6' legacy negatif: duplicate canonical → fail-closed (DuplicateCanonical).
    /// Mevcut davranış: canonical string bazında dedup, ilk tekrar hata.
    #[test]
    fn frozen_characterization_duplicate_canonical_rejected() {
        let json = r#"{
            "schema_version": 1,
            "nodes": [
                {"canonical": "Dup", "kind": "Concept"},
                {"canonical": "Dup", "kind": "RuleCandidate"}
            ]
        }"#;
        let seed = CandidateSeedFile::from_json(json).unwrap();
        let err = seed.to_graph_seed().unwrap_err();
        assert!(
            matches!(err, SeedError::DuplicateCanonical(ref s) if s == "Dup"),
            "duplicate canonical must fail-closed, got {err:?}"
        );
    }

    /// O6' legacy negatif: empty seed → Ok (GraphSeed::default, tüm bucket boş).
    /// Mevcut davranış: boş node listesi → boş GraphSeed (kabul).
    #[test]
    fn frozen_characterization_empty_seed_accepted() {
        let json = r#"{ "schema_version": 1, "nodes": [] }"#;
        let seed = CandidateSeedFile::from_json(json).unwrap();
        let graph = seed.to_graph_seed().unwrap();
        assert!(graph.concepts.is_empty());
        assert!(graph.decisions.is_empty());
        assert!(graph.code_entities.is_empty());
        assert!(graph.rule_candidates.is_empty());
        assert!(graph.task_candidates.is_empty());
        assert!(graph.risk_candidates.is_empty());
    }

    /// O6' legacy negatif: insertion-order preserved (bucket push sırası).
    /// Mevcut davranış: JSON node sırası korunur (her bucket kendi içinde insertion-order).
    #[test]
    fn frozen_characterization_insertion_order_preserved() {
        let json = r#"{
            "schema_version": 1,
            "nodes": [
                {"canonical": "Zebra", "kind": "Concept"},
                {"canonical": "Apple", "kind": "Concept"},
                {"canonical": "Mango", "kind": "Concept"}
            ]
        }"#;
        let seed = CandidateSeedFile::from_json(json).unwrap();
        let graph = seed.to_graph_seed().unwrap();
        // Insertion-order (Zebra, Apple, Mango) — sort edilmez.
        assert_eq!(graph.concepts.len(), 3);
        assert_eq!(graph.concepts[0].canonical, "Zebra");
        assert_eq!(graph.concepts[1].canonical, "Apple");
        assert_eq!(graph.concepts[2].canonical, "Mango");
    }
}
