// Test + CLI integration tamamlanana kadar dead-code.
#![allow(dead_code)]

//! Analysis → candidate bridge — AnalysisResult → AnalysisCandidateSeed (identity-only).
//!
//! Saf projeksiyon: analyzer tarafından gözlemlenen fiziksel modül kimliğini Concept
//! Anchoring'in Candidate lane'ine taşır. INV-C5 (yalnız Candidate), INV-C2 (PhysicalCode).
//!
//! # Identity-durum sözleşmesi (F-yeni)
//! NodeId (identity_key, AnalysisIdentityScheme+policy'ye bağlı) = kalıcı entity kimliği.
//! canonical (display_path) = gözlemlenen mevcut repository spelling. Case-only rename →
//! aynı NodeId, farklı canonical = INV-C12 muhafazakâr (representation refresh, supersede değil).
//!
//! # Out-of-scope (PR B/C/D/E)
//! ObservedCodeEvidence (INV-C6), ConceptEdge (Imports), ConceptNode attribute expansion
//! (classification/role), ObservedEntityRefresh (incremental representation change).

// commands/graph.rs integration tamamlanana kadar dead-code (test modülü kullanıyor).

use std::collections::BTreeMap;

use osp_analyzer::contract::AnalysisResult;
use osp_core::anchoring::types::{ConceptNodeId, ConceptNodeKind};
use osp_core::space::{NodeId, NodeKind};

use crate::canonical_identity::{CanonicalCodeIdentity, CanonicalIdentityError, PathCasePolicy};
use crate::graph_seed_builder::GraphSeedNodeDraft;
use crate::seed_file::derive_node_id;

/// O2' — typed identity scheme (passive metadata → active identity boundary).
/// PathV2 gelirse NodeId algoritması görünür değişir; hash materyali/Unicode/namespace
/// sessizce değişemez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisIdentityScheme {
    /// NodeId = derive_node_id(CodeEntityCandidate, identity_key).
    PathV1,
}

impl AnalysisIdentityScheme {
    /// NodeId üret — scheme algoritmasını seçer (O2').
    pub fn derive_node_id(self, kind: ConceptNodeKind, identity_key: &str) -> ConceptNodeId {
        match self {
            Self::PathV1 => ConceptNodeId(derive_node_id(kind, identity_key)),
        }
    }

    /// Display label (BridgeRunReport için).
    pub fn label(self) -> &'static str {
        match self {
            Self::PathV1 => "analysis-path-v1",
        }
    }
}

/// Code entity candidate — identity-only (M1) + pre-derived ConceptNodeId (R1).
/// ID tek noktada türetilir (project_candidate_nodes); into_drafts scheme almaz.
/// classification/role/aliases/kind/family YOK — ConceptNode taşımıyor (semantic drift).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeEntityCandidate {
    identity: CanonicalCodeIdentity,
    concept_node_id: ConceptNodeId,  // R1: pre-derived, scheme-parametre YOK
}

impl CodeEntityCandidate {
    /// Private constructor — R1 structural invariant: yalnız `project_candidate_nodes`
    /// (aynı modül) üretici. identity + concept_node_id tutarlılığı caller convention değil
    /// type boundary ile korunur. Test modülü aynı modül olduğu için erişebilir.
    fn new(identity: CanonicalCodeIdentity, concept_node_id: ConceptNodeId) -> Self {
        Self { identity, concept_node_id }
    }

    pub fn into_parts(self) -> (CanonicalCodeIdentity, ConceptNodeId) {
        (self.identity, self.concept_node_id)
    }

    pub fn identity(&self) -> &CanonicalCodeIdentity {
        &self.identity
    }

    pub fn concept_node_id(&self) -> &ConceptNodeId {
        &self.concept_node_id
    }
}

/// Analysis candidate seed — identity-only entities (private fields + try_new).
/// Deterministik sort: (identity_key, display_path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisCandidateSeed {
    entities: Vec<CodeEntityCandidate>,
}

impl AnalysisCandidateSeed {
    /// Duplicate (aynı identity_key + aynı display) / CaseCollision (aynı key + farklı display)
    /// kontrolü + deterministic sort (O5).
    pub fn try_new(
        entities: impl IntoIterator<Item = CodeEntityCandidate>,
    ) -> Result<Self, AnalysisSeedError> {
        let mut entities: Vec<CodeEntityCandidate> = entities.into_iter().collect();
        // Deterministic sort: (identity_key, display_path).
        entities.sort_by(|a, b| {
            let (ad, ak) = (a.identity.display_path(), a.identity.identity_key());
            let (bd, bk) = (b.identity.display_path(), b.identity.identity_key());
            ak.cmp(bk).then(ad.cmp(bd))
        });

        // Duplicate/collision dedup (identity_key bazında).
        let mut seen_keys: BTreeMap<&str, &str> = BTreeMap::new(); // key → display
        for entity in &entities {
            let key = entity.identity.identity_key();
            let display = entity.identity.display_path();
            if let Some(existing_display) = seen_keys.get(key) {
                if *existing_display == display {
                    return Err(AnalysisSeedError::DuplicateCanonical {
                        display_path: display.to_string(),
                    });
                } else {
                    return Err(AnalysisSeedError::CaseCollision {
                        first: existing_display.to_string(),
                        second: display.to_string(),
                        identity_key: key.to_string(),
                    });
                }
            }
            seen_keys.insert(key, display);
        }

        Ok(Self { entities })
    }

    /// Entity'lere read-only erişim (test + future downstream).
    #[allow(dead_code)]
    pub fn entities(&self) -> &[CodeEntityCandidate] {
        &self.entities
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Draft'lara dönüştür (graph mutation için) — R1: scheme almaz, ID hazır taşınır.
    pub(crate) fn into_drafts(self) -> Vec<GraphSeedNodeDraft> {
        self.entities
            .into_iter()
            .map(|entity| {
                let (identity, concept_node_id) = entity.into_parts();
                GraphSeedNodeDraft::analysis_code_entity(
                    concept_node_id,           // R1: pre-derived
                    identity.display_path().to_owned(), // canonical = gözlemlenen yazım
                )
            })
            .collect()
    }
}

/// AnalysisProjectionIndex — analyzer NodeId → ConceptNodeId lookup (R1: tek türetim).
/// PR B metric projection bunu tüketir; scheme/policy almaz.
#[derive(Debug, Clone, Default)]
pub struct AnalysisProjectionIndex {
    by_analysis_node: BTreeMap<NodeId, ConceptNodeId>,
}

impl AnalysisProjectionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert — duplicate analysis NodeId key → error (source key uniqueness).
    /// NOT injective on ConceptNodeId value: many-to-one (two NodeId → same ConceptNodeId)
    /// is allowed at index level; detected downstream by DuplicateProjectedAxis in metric projection.
    /// Production candidate seed dedup (try_new) catches this before metric projection in assembler path.
    fn insert(
        &mut self,
        analysis_node_id: NodeId,
        concept_node_id: ConceptNodeId,
    ) -> Result<(), BridgeError> {
        if self
            .by_analysis_node
            .insert(analysis_node_id, concept_node_id)
            .is_some()
        {
            return Err(BridgeError::DuplicateAnalysisNodeIdentity { analysis_node_id });
        }
        Ok(())
    }

    /// ConceptNodeId lookup — PR B metric projection bunu çağırır.
    pub fn concept_node_id_for(&self, analysis_node_id: NodeId) -> Option<ConceptNodeId> {
        self.by_analysis_node.get(&analysis_node_id).cloned()
    }

    /// Test constructor — production insert sınırını kullanır (C4).
    #[cfg(test)]
    pub fn for_tests(
        entries: impl IntoIterator<Item = (NodeId, ConceptNodeId)>,
    ) -> Result<Self, BridgeError> {
        let mut index = Self::new();
        for (analysis_node_id, concept_node_id) in entries {
            index.insert(analysis_node_id, concept_node_id)?;
        }
        Ok(index)
    }
}

/// Node projection iç ara sonuç — project_candidate_nodes çıktısı.
#[derive(Debug, Clone)]
pub(crate) struct CandidateProjectionOutput {
    pub(crate) candidate_seed: AnalysisCandidateSeed,
    pub(crate) identity_index: AnalysisProjectionIndex,
    pub(crate) graph_report: BridgeRunReport,
}

/// Node projection — tek türetim noktası (R1). ID bir kez üretilir, hem entity'ye hem index'e.
pub(crate) fn project_candidate_nodes(
    analysis: &AnalysisResult,
    policy: PathCasePolicy,
    scheme: AnalysisIdentityScheme,
) -> Result<CandidateProjectionOutput, BridgeError> {
    let mut entities: Vec<CodeEntityCandidate> = Vec::new();
    let mut identity_index = AnalysisProjectionIndex::new();
    let mut projected_modules = 0usize;
    let mut skipped_non_module = 0usize;
    let mut classifications_observed: BTreeMap<String, usize> = BTreeMap::new();
    let mut roles_observed: BTreeMap<String, usize> = BTreeMap::new();

    // Deterministik node sırası (NodeId ascending — HashMap traversal random).
    let mut node_ids: Vec<NodeId> = analysis.space.nodes.keys().copied().collect();
    node_ids.sort();

    for node_id in node_ids {
        let node = &analysis.space.nodes[&node_id];

        // Yalnızca Module node'ları (kriter 1).
        if node.kind != NodeKind::Module {
            skipped_non_module += 1;
            continue;
        }

        // Path çöz — MissingNodePath typed error (I3).
        let path = analysis.node_paths.get(&node_id).ok_or_else(|| {
            BridgeError::MissingNodePath { node_id }
        })?;

        // Canonical identity üret (lexical normalizasyon + case fold).
        let identity = CanonicalCodeIdentity::new(path, policy)?;

        // R1: ID tek noktada türetilir — hem entity'ye hem index'e.
        let concept_node_id =
            scheme.derive_node_id(ConceptNodeKind::CodeEntityCandidate, identity.identity_key());
        identity_index.insert(node_id, concept_node_id.clone())?;

        // Metadata observable (BridgeRunReport) — graph'a DÖNÜŞMEZ (M1).
        *classifications_observed
            .entry(format!("{:?}", node.classification))
            .or_default() += 1;
        *roles_observed.entry(format!("{:?}", node.role)).or_default() += 1;

        entities.push(CodeEntityCandidate::new(identity, concept_node_id));
        projected_modules += 1;
    }

    let candidate_seed = AnalysisCandidateSeed::try_new(entities)?;
    let graph_report = BridgeRunReport {
        identity_scheme: scheme,
        path_case_policy: policy,
        repository_head: Some(analysis.semantic_coverage.repo_head.clone()),
        projected_modules,
        skipped_non_module,
        classifications_observed,
        roles_observed,
    };
    Ok(CandidateProjectionOutput {
        candidate_seed,
        identity_index,
        graph_report,
    })
}

/// Bridge run output — tek assembler (pub(crate), tüm parçalar).
/// N2: --analyze davranış sözleşmesi — InvalidMetric durumunda candidate seeding dahil
/// tamamen düşer (tutarlılık > kullanılabilirlik).
#[derive(Debug, Clone)]
pub(crate) struct BridgeRunOutput {
    pub(crate) candidate_seed: AnalysisCandidateSeed,
    pub(crate) identity_index: AnalysisProjectionIndex,
    pub(crate) graph_report: BridgeRunReport,
    pub(crate) metric_projection: crate::metric_projection::AnalysisMetricProjection,
}

/// Tam analysis bridge — tek assembler (R1 tek türetim + metric projection).
/// N7: MetricProjectionError BridgeError::MetricProjection'a map.
pub(crate) fn project_analysis(
    analysis: &AnalysisResult,
    policy: PathCasePolicy,
) -> Result<BridgeRunOutput, BridgeError> {
    let scheme = AnalysisIdentityScheme::PathV1;
    let candidate_proj = project_candidate_nodes(analysis, policy, scheme)?;
    let metric_projection = crate::metric_projection::project_code_metrics(
        analysis,
        &candidate_proj.identity_index,
    )
    .map_err(BridgeError::MetricProjection)?;
    Ok(BridgeRunOutput {
        candidate_seed: candidate_proj.candidate_seed,
        identity_index: candidate_proj.identity_index,
        graph_report: candidate_proj.graph_report,
        metric_projection,
    })
}

/// Bridge run report — semantic seed DIŞI (F2). stderr'e basılır, persisted değil,
/// node identity'ye katılmaz. Deterministik (wall-clock/local_path YOK).
#[derive(Debug, Clone)]
pub struct BridgeRunReport {
    pub identity_scheme: AnalysisIdentityScheme,
    pub path_case_policy: PathCasePolicy,
    pub repository_head: Option<String>,
    pub projected_modules: usize,
    pub skipped_non_module: usize,
    // Metadata counts — Display'de yazılmaz ama observable (test + future structured output).
    #[allow(dead_code)]
    pub classifications_observed: BTreeMap<String, usize>,
    #[allow(dead_code)]
    pub roles_observed: BTreeMap<String, usize>,
}

impl std::fmt::Display for BridgeRunReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "analysis bridge: scheme={}, policy={}, projected={}, skipped_non_module={}",
            self.identity_scheme.label(),
            self.path_case_policy.label(),
            self.projected_modules,
            self.skipped_non_module,
        )?;
        if let Some(head) = &self.repository_head {
            write!(f, ", head={}", &head[..head.len().min(7)])?;
        }
        Ok(())
    }
}

/// Bridge hatası.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("missing node_paths entry for node {node_id}")]
    MissingNodePath { node_id: u64 },
    #[error("canonical identity error: {0}")]
    CanonicalIdentity(#[from] CanonicalIdentityError),
    #[error("analysis seed error: {0}")]
    Seed(#[from] AnalysisSeedError),
    #[error("duplicate analysis node identity mapping for node {analysis_node_id}")]
    DuplicateAnalysisNodeIdentity { analysis_node_id: NodeId },
    #[error("metric projection error: {0}")]
    MetricProjection(#[from] crate::metric_projection::MetricProjectionError),
}

/// Analysis seed validation hatası (O5).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisSeedError {
    #[error("duplicate canonical (same identity_key + same display): {display_path}")]
    DuplicateCanonical { display_path: String },
    #[error("case collision (same identity_key, different display): first={first}, second={second}, key={identity_key}")]
    CaseCollision {
        first: String,
        second: String,
        identity_key: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use osp_analyzer::contract::SemanticCoverage;
    use osp_core::space::{Node, NodeClassification, NodeId, NodeRole, Space};
    use std::collections::HashMap;

    /// Synthetic AnalysisResult builder — test fixture (module_metrics TreeSitter ile).
    fn analysis_result(nodes: Vec<(NodeId, &str, NodeClassification, NodeRole)>) -> AnalysisResult {
        let mut space = Space::default();
        let mut node_paths = HashMap::new();
        let mut module_metrics = HashMap::new();
        for (id, path, classification, role) in nodes {
            space.nodes.insert(
                id,
                Node {
                    id,
                    kind: NodeKind::Module,
                    mass: 10.0,
                    classification,
                    role,
                    ..Default::default()
                },
            );
            node_paths.insert(id, path.to_string());
            // PR B: her Module için geçerli ModuleMetrics (TreeSitter, conf>0).
            module_metrics.insert(
                id,
                osp_analyzer::contract::ModuleMetrics {
                    coupling: osp_core::coords::MetricValue::tree_sitter(0.5, 1.0),
                    cohesion: osp_core::coords::MetricValue::tree_sitter(0.7, 1.0),
                    instability: osp_core::coords::MetricValue::tree_sitter(0.3, 1.0),
                },
            );
        }
        AnalysisResult {
            space,
            module_metrics,
            node_paths,
            node_semantics: HashMap::new(),
            node_witnesses: HashMap::new(),
            repo_metrics: osp_analyzer::contract::RepoMetrics {
                abstractness: osp_core::coords::MetricValue::placeholder(0.0),
                main_sequence_distance: osp_core::coords::MetricValue::placeholder(0.0),
                abstractness_by_package: None,
            },
            semantic_coverage: SemanticCoverage::none("testhead".into()),
            diagnostics: vec![],
        }
    }

    // ── Mutlu yol ────────────────────────────────────────────────────────────

    #[test]
    fn happy_path_3_modules_3_candidates() {
        let analysis = analysis_result(vec![
            (1, "src/payment.rs", NodeClassification::Production, NodeRole::Core),
            (2, "src/user.rs", NodeClassification::Production, NodeRole::Adapter),
            (3, "src/util.rs", NodeClassification::Production, NodeRole::Utility),
        ]);
        let bridge =
            project_analysis(&analysis, PathCasePolicy::CaseSensitive).unwrap();
        assert_eq!(bridge.candidate_seed.entities().len(), 3);
        assert_eq!(bridge.graph_report.projected_modules, 3);
        assert_eq!(bridge.graph_report.skipped_non_module, 0);
        // Deterministic sort (identity_key ascending).
        assert_eq!(bridge.candidate_seed.entities()[0].identity().display_path(), "src/payment.rs");
        assert_eq!(bridge.candidate_seed.entities()[1].identity().display_path(), "src/user.rs");
        assert_eq!(bridge.candidate_seed.entities()[2].identity().display_path(), "src/util.rs");
    }

    // ── NodeId identity_keyden; canonical display_path (F-yeni) ───────────────

    #[test]
    fn node_id_from_identity_key_canonical_display() {
        let analysis = analysis_result(vec![
            (1, "src/Payment.rs", NodeClassification::Production, NodeRole::Core),
        ]);
        let bridge =
            project_analysis(&analysis, PathCasePolicy::AsciiCaseInsensitive).unwrap();
        let drafts = bridge.candidate_seed.clone().into_drafts();
        assert_eq!(drafts.len(), 1);
        // NodeId identity_key'den (case-folded).
        assert_eq!(drafts[0].id().0, "CodeEntityCandidate:src/payment.rs");
    }

    #[test]
    fn case_only_rename_same_node_id_different_canonical_and_digest() {
        // F-yeni identity-durum sözleşmesi (üçlü assert): aynı NodeId, farklı canonical,
        // farklı NodeDigest. INV-C12 muhafazakâr — operatöre sunulan basis değişti.
        let a = analysis_result(vec![
            (1, "src/Payment.cs", NodeClassification::Production, NodeRole::Core),
        ]);
        let b = analysis_result(vec![
            (1, "src/payment.cs", NodeClassification::Production, NodeRole::Core),
        ]);
        let bridge_a =
            project_analysis(&a, PathCasePolicy::AsciiCaseInsensitive).unwrap();
        let bridge_b =
            project_analysis(&b, PathCasePolicy::AsciiCaseInsensitive).unwrap();
        let drafts_a = bridge_a.candidate_seed.clone().into_drafts();
        let drafts_b = bridge_b.candidate_seed.clone().into_drafts();
        // GraphSeed'e dönüştür (canonical + digest karşılaştırması için).
        let graph_a = crate::graph_seed_builder::GraphSeedBuilder::build(drafts_a).unwrap();
        let graph_b = crate::graph_seed_builder::GraphSeedBuilder::build(drafts_b).unwrap();
        let node_a = &graph_a.code_entities[0];
        let node_b = &graph_b.code_entities[0];
        // (1) Aynı NodeId (identity_key aynı — case-folded).
        assert_eq!(node_a.id, node_b.id, "case-only rename must preserve NodeId");
        // (2) Farklı canonical (display_path — case korunur).
        assert_ne!(
            node_a.canonical, node_b.canonical,
            "case-only rename must change canonical (observed spelling)"
        );
        // (3) Farklı NodeDigest (canonical digest'e girer — freshness değişti).
        assert_ne!(
            osp_core::anchoring::review::node_digest(node_a).get(),
            osp_core::anchoring::review::node_digest(node_b).get(),
            "case-only rename must change NodeDigest (canonical → freshness)"
        );
    }

    #[test]
    fn case_sensitive_policy_different_node_ids() {
        // Case-sensitive: Payment.cs + payment.cs → farklı identity_key → farklı NodeId.
        let a = CanonicalCodeIdentity::new("src/Payment.cs", PathCasePolicy::CaseSensitive).unwrap();
        let b = CanonicalCodeIdentity::new("src/payment.cs", PathCasePolicy::CaseSensitive).unwrap();
        assert_ne!(a.identity_key(), b.identity_key());
    }

    // ── I6: metadata-bağımsızlık ─────────────────────────────────────────────

    #[test]
    fn same_path_different_metadata_same_node_id() {
        // Aynı path farklı classification/role → aynı NodeId (metadata graph'a sızmaz).
        let a = analysis_result(vec![
            (1, "src/payment.rs", NodeClassification::Production, NodeRole::Core),
        ]);
        let b = analysis_result(vec![
            (1, "src/payment.rs", NodeClassification::Test, NodeRole::Support),
        ]);
        let bridge_a =
            project_analysis(&a, PathCasePolicy::CaseSensitive).unwrap();
        let bridge_b =
            project_analysis(&b, PathCasePolicy::CaseSensitive).unwrap();
        let drafts_a = bridge_a.candidate_seed.clone().into_drafts();
        let drafts_b = bridge_b.candidate_seed.clone().into_drafts();
        assert_eq!(drafts_a[0].id(), drafts_b[0].id()); // aynı NodeId
        // Report farklı (metadata observable, graph değil).
        assert_ne!(bridge_a.graph_report.classifications_observed, bridge_b.graph_report.classifications_observed);
        assert_ne!(bridge_a.graph_report.roles_observed, bridge_b.graph_report.roles_observed);
    }

    // ── O5: error-matrix ──────────────────────────────────────────────────────

    #[test]
    fn duplicate_canonical_same_key_same_display() {
        // Aynı identity_key + aynı display → DuplicateCanonical.
        let id1 = CanonicalCodeIdentity::new("src/x.rs", PathCasePolicy::CaseSensitive).unwrap();
        let id2 = CanonicalCodeIdentity::new("src/x.rs", PathCasePolicy::CaseSensitive).unwrap();
        let entities = vec![
            CodeEntityCandidate::new(id1, ConceptNodeId("CodeEntityCandidate:src/x.rs".into())),
            CodeEntityCandidate::new(id2, ConceptNodeId("CodeEntityCandidate:src/x.rs".into())),
        ];
        let err = AnalysisCandidateSeed::try_new(entities).unwrap_err();
        assert!(matches!(err, AnalysisSeedError::DuplicateCanonical { .. }));
    }

    #[test]
    fn case_collision_same_key_different_display() {
        // Aynı identity_key (ascii-insensitive) + farklı display → CaseCollision.
        let id1 = CanonicalCodeIdentity::new("src/Payment.cs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        let id2 = CanonicalCodeIdentity::new("src/payment.cs", PathCasePolicy::AsciiCaseInsensitive).unwrap();
        let entities = vec![
            CodeEntityCandidate::new(id1, ConceptNodeId("CodeEntityCandidate:src/payment.cs".into())),
            CodeEntityCandidate::new(id2, ConceptNodeId("CodeEntityCandidate:src/payment.cs".into())),
        ];
        let err = AnalysisCandidateSeed::try_new(entities).unwrap_err();
        assert!(matches!(err, AnalysisSeedError::CaseCollision { .. }));
    }

    // ── I3: MissingNodePath ───────────────────────────────────────────────────

    #[test]
    fn missing_node_path_typed_error() {
        // Module node var ama node_paths kaydı yok → MissingNodePath.
        let mut space = Space::default();
        space.nodes.insert(
            42,
            Node {
                id: 42,
                kind: NodeKind::Module,
                mass: 10.0,
                classification: NodeClassification::Production,
                role: NodeRole::Core,
                ..Default::default()
            },
        );
        let analysis = AnalysisResult {
            space,
            module_metrics: HashMap::new(),
            node_paths: HashMap::new(), // boş — kayıt yok
            node_semantics: HashMap::new(),
            node_witnesses: HashMap::new(),
            repo_metrics: osp_analyzer::contract::RepoMetrics {
                abstractness: osp_core::coords::MetricValue::placeholder(0.0),
                main_sequence_distance: osp_core::coords::MetricValue::placeholder(0.0),
                abstractness_by_package: None,
            },
            semantic_coverage: SemanticCoverage::none("testhead".into()),
            diagnostics: vec![],
        };
        let err = project_analysis(&analysis, PathCasePolicy::CaseSensitive).unwrap_err();
        assert!(matches!(err, BridgeError::MissingNodePath { node_id: 42 }));
    }

    // ── INV-C5 negatif: Accepted üretilemez ───────────────────────────────────

    #[test]
    fn analysis_never_produces_accepted() {
        let analysis = analysis_result(vec![
            (1, "src/a.rs", NodeClassification::Production, NodeRole::Core),
        ]);
        let bridge =
            project_analysis(&analysis, PathCasePolicy::CaseSensitive).unwrap();
        let drafts = bridge.candidate_seed.clone().into_drafts();
        // Tüm drafts Candidate (INV-C5 — analysis_code_entity constructor baked).
        let graph_seed = crate::graph_seed_builder::GraphSeedBuilder::build(drafts).unwrap();
        for node in &graph_seed.code_entities {
            assert_eq!(
                node.decision_status,
                osp_core::anchoring::DecisionStatus::Candidate
            );
        }
    }

    // ── I7: empty analysis ────────────────────────────────────────────────────

    #[test]
    fn empty_analysis_accepted() {
        let analysis = analysis_result(vec![]);
        let bridge =
            project_analysis(&analysis, PathCasePolicy::CaseSensitive).unwrap();
        assert!(bridge.candidate_seed.is_empty());
        assert_eq!(bridge.graph_report.projected_modules, 0);
    }

    // ── Determinism (F2) ──────────────────────────────────────────────────────

    #[test]
    fn bridge_run_report_deterministic_no_wall_clock() {
        let analysis = analysis_result(vec![
            (1, "src/a.rs", NodeClassification::Production, NodeRole::Core),
        ]);
        let bridge =
            project_analysis(&analysis, PathCasePolicy::CaseSensitive).unwrap();
        let report = &bridge.graph_report;
        // repository_head Option<String> — wall_clock/local_path YOK.
        let display = format!("{report}");
        assert!(display.contains("scheme=analysis-path-v1"));
        assert!(display.contains("policy=case-sensitive"));
        assert!(display.contains("projected=1"));
    }

    #[test]
    fn same_analysis_bit_equivalent_seed() {
        let analysis = || {
            analysis_result(vec![
                (1, "src/zebra.rs", NodeClassification::Production, NodeRole::Core),
                (2, "src/apple.rs", NodeClassification::Production, NodeRole::Adapter),
            ])
        };
        let bridge_a =
            project_analysis(&analysis(), PathCasePolicy::CaseSensitive).unwrap();
        let bridge_b =
            project_analysis(&analysis(), PathCasePolicy::CaseSensitive).unwrap();
        // Node identities bit-equivalent (deterministic sort).
        assert_eq!(bridge_a.candidate_seed, bridge_b.candidate_seed);
    }
}
