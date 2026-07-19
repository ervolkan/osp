//! Evidence projection — draft→evidence conversion boundary (PR D).
//!
//! Bu modül CLI metric draft'larını (`ProjectedCodeMetric`) core evidence'a
//! (`ObservedCodeEvidence` via `ObservedPhysicalMetrics`) dönüştürür. Bu, CLI içindeki
//! **tek** core evidence construction boundary'sidir — `tests/architecture_guards.rs`
//! ownership guard bunu mekanik doğrular.
//!
//! # Conversion akışı
//! ```text
//! ProjectedCodeMetric[] (metric_projection.rs — validated draft)
//!   → ConceptNodeId bazında group
//!   → PhysicalCodeAxis → PhysicalCodeMetricAxis anti-corruption map
//!   → MetricConfidence → EvidenceStrength (re-validate)
//!   → MetricCoverage → EvidenceCoverage (zero coverage reject + re-validate)
//!   → MetricAxisValue → raw f64 (core constructor kendi validation'ını yapar)
//!   → ObservedPhysicalMetric::new → ObservedPhysicalMetrics::try_new
//!   → ObservedCodeEvidence::new(node_id, observations, measured_at)
//! ```
//!
//! # Input yüzeyi sınırı
//! `metrics` yalnız **emit edilmiş** (admitted) metric'leri içerir — Placeholder/Heuristic/
//! zero-confidence metric'ler `project_code_metrics` tarafından zaten çıkarıldı. Bu nedenle
//! her grouped node'un en az bir metric'i vardır; `ObservedPhysicalMetricsError::Empty` bu
//! conversion yolunda unreachable (defensive handle edilir). Hiç projected metric'i olmayan
//! analysis node'ları bu boundary'nin dışında kalır ve doğal olarak provider lookup'ında bulunmaz.
//!
//! # Temporal nondeterminism
//! `measured_at` context'ten inject edilir — bu modül wall-clock okumaz. Deterministic test +
//! replay için `EvidenceProjectionContext` caller'da doldurulur.

use crate::metric_projection::{PhysicalCodeAxis, ProjectedCodeMetric};
use osp_core::anchoring::types::{
    ConceptNodeId, EvidenceCoverage, EvidenceStrength, EvidenceStrengthOutOfRange,
    ObservedCodeEvidence, ObservedPhysicalMetric, ObservedPhysicalMetricError,
    ObservedPhysicalMetrics, ObservedPhysicalMetricsError,
};
use osp_core::anchoring::{
    MetricScalarViolation as CoreMetricScalarViolation, PhysicalCodeMetricAxis,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Conversion context + output
// ═══════════════════════════════════════════════════════════════════════════════

/// Conversion context — wall-clock inject edilir (temporal nondeterminism yalnız caller'da).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvidenceProjectionContext {
    pub(crate) measured_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EvidenceProjectionOutput {
    pub(crate) evidence: Vec<ObservedCodeEvidence>,
    pub(crate) report: EvidenceProjectionReport,
}

/// Conversion report — input yüzeyiyle uyumlu.
///
/// Conversion yalnız emit edilmiş metric'leri görür. Hiç projected metric'i olmayan analysis
/// node'ları bu boundary'nin dışında kalır ve doğal olarak provider lookup'ında bulunmaz.
/// `input_metric_values` = grouped metric sayısı; `evidence_objects_created` = node sayısı;
/// `partial_evidence_objects` = < 5 axis (try_to_physical_vector Err).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvidenceProjectionReport {
    pub(crate) input_metric_values: usize,
    pub(crate) evidence_objects_created: usize,
    pub(crate) partial_evidence_objects: usize,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Typed error model
// ═══════════════════════════════════════════════════════════════════════════════

/// Conversion hatası — node/axis context korunur (anyhow YOK).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum EvidenceProjectionError {
    /// EvidenceStrength dönüşümü hatası (defensive contract-drift — projection confidence'ı
    /// zaten [0,1] doğruluyor, ama typed conversion sınırında eksiksiz olmalı).
    #[error("{node_id} {axis:?} axis EvidenceStrength geçersiz: {source}")]
    InvalidStrength {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
        source: EvidenceStrengthOutOfRange,
    },
    /// Zero coverage reject (contract drift defense).
    ///
    /// Semantik: "sıfır coverage'a sahip metric observation değildir; PR B'de omission olması
    /// gerekirdi". coverage=0, strength>0 core'da temsil edilebilir ama PR B confidence formülü
    /// (confidence coverage içerir) + zero-confidence omission ile tutarsız → conversion reject.
    #[error(
        "{node_id} {axis:?} axis zero coverage — observation değildir (PR B omission beklenir)"
    )]
    ZeroCoverage {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
    },
    #[error("{node_id} {axis:?} axis EvidenceCoverage geçersiz: {source}")]
    InvalidCoverage {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
        source: CoreMetricScalarViolation,
    },
    /// ObservedPhysicalMetric::new hatası (InvalidValue + ZeroStrength dahil; core constructor
    /// axis/value context zaten taşıyor — InvalidAxisValue ayrı varyant YOK).
    #[error("{node_id} {axis:?} axis observation contract mismatch: {source}")]
    InvalidObservation {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
        source: ObservedPhysicalMetricError,
    },
    /// PR F review bakım — identity-key aggregation sonrası collection hatası.
    ///
    /// Identity aggregation sonrası `ObservedPhysicalMetrics::try_new` hatası için — `identity_key`
    /// context (R1a bakım: `derive_entity_id()` sentetik entity ID göstermez). Pratikte
    /// unreachable (aggregation önce axis dedup/conflict yapar) ama future collection invariant'ı
    /// için dürüst context.
    #[error("identity {identity_key:?} collection contract mismatch: {source}")]
    InvalidIdentityCollection {
        identity_key: osp_core::anchoring::identity::CodeIdentityKey,
        source: ObservedPhysicalMetricsError,
    },
    /// PR F — Duplicate code identity binding for same node (fail-fast; sessiz overwrite YOK).
    ///
    /// Store canonical disiplin mirror: aynı node için batch içindeki her duplicate binding reject.
    /// Explicit index-building `BTreeMap::insert` duplicate'i yakalar.
    #[error("duplicate code identity binding for node {node_id}")]
    DuplicateBindingNode { node_id: ConceptNodeId },
    /// PR F — Projected metric node has no code identity binding (fail-fast reject).
    ///
    /// Semantik: "metric node için binding yoksa evidence üretilemez". PR D `ZeroCoverage`
    /// pattern analog — sessiz skip YOK (tutarlılık > kullanılabilirlik).
    #[error("projected metric node has no code identity binding: {node_id}")]
    UnboundNode { node_id: ConceptNodeId },
    /// PR F review P1-1 — Aynı identity key'e bağlı birden fazla candidate node aynı axis
    /// için conflicting (value/provenance/strength/coverage) metric taşıyorsa reject.
    ///
    /// EI4-c N:1 convergence: birden fazla candidate aynı key'e paylaşabilir ve tek evidence'ya
    /// converge olur — ama aynı axis için farklı ölçümler conflict'tir (data integrity). Aynı
    /// değer+provenance+strength+coverage birebir aynıysa deduplicate (idempotent replay).
    #[error("conflicting {axis:?} observation for identity {identity_key:?} (node {first_node} vs {second_node})")]
    ConflictingIdentityObservation {
        identity_key: osp_core::anchoring::identity::CodeIdentityKey,
        axis: PhysicalCodeAxis,
        first_node: ConceptNodeId,
        second_node: ConceptNodeId,
    },
}

// ═══════════════════════════════════════════════════════════════════════════════
// CLI → core anti-corruption map (PhysicalCodeAxis → PhysicalCodeMetricAxis)
// ═══════════════════════════════════════════════════════════════════════════════

/// CLI draft axis → core axis (anti-corruption map, 5 variant exhaustive).
///
/// "adopt" DEĞİL — CLI `PhysicalCodeAxis` korunur; explicit conversion. 5 variant exhaustive
/// test drift riskini azaltır (tur 3 P3-8).
fn map_axis(axis: PhysicalCodeAxis) -> PhysicalCodeMetricAxis {
    match axis {
        PhysicalCodeAxis::Coupling => PhysicalCodeMetricAxis::Coupling,
        PhysicalCodeAxis::Cohesion => PhysicalCodeMetricAxis::Cohesion,
        PhysicalCodeAxis::Instability => PhysicalCodeMetricAxis::Instability,
        PhysicalCodeAxis::Entropy => PhysicalCodeMetricAxis::Entropy,
        PhysicalCodeAxis::WitnessDepth => PhysicalCodeMetricAxis::WitnessDepth,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Single metric → observation conversion
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek draft metric'i core observation'a dönüştürür.
///
/// Duplicate validation YOK: value raw `f64` olarak `ObservedPhysicalMetric::new`'e geçer
/// (core kendi `PhysicalAxisValue::new` validation'ını yapar). Strength + coverage re-validate.
fn convert_metric_to_observation(
    metric: &ProjectedCodeMetric,
) -> Result<ObservedPhysicalMetric, EvidenceProjectionError> {
    let node_id = metric.node_id().clone();
    let draft_axis = metric.axis();
    let axis = map_axis(draft_axis);
    let source = metric.provenance().source();

    // Strength re-validate (EvidenceStrength::new — InvalidStrength).
    let strength =
        EvidenceStrength::new(metric.provenance().confidence().get()).map_err(|source| {
            EvidenceProjectionError::InvalidStrength {
                node_id: node_id.clone(),
                axis: draft_axis,
                source,
            }
        })?;

    // Zero coverage reject (tur 4 karar 3 — contract drift defense).
    let coverage_raw = metric.provenance().coverage().get();
    if coverage_raw == 0.0 {
        return Err(EvidenceProjectionError::ZeroCoverage {
            node_id,
            axis: draft_axis,
        });
    }

    // Coverage re-validate (EvidenceCoverage::new — InvalidCoverage).
    let coverage = EvidenceCoverage::new(coverage_raw).map_err(|source| {
        EvidenceProjectionError::InvalidCoverage {
            node_id: node_id.clone(),
            axis: draft_axis,
            source,
        }
    })?;

    // Observation — value raw f64 (core kendi PhysicalAxisValue::new validation'ını yapar).
    ObservedPhysicalMetric::new(axis, metric.value().get(), source, strength, coverage).map_err(
        |source| EvidenceProjectionError::InvalidObservation {
            node_id,
            axis: draft_axis,
            source,
        },
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// project_observed_evidence — top-level conversion
// ═══════════════════════════════════════════════════════════════════════════════

/// Draft metric'leri core evidence'a dönüştürür.
///
/// **PR F migration:** Artık `bindings: &[CodeIdentityBinding]` ayrı parametre alır
/// (R1a P0-2 — `EvidenceProjectionContext`'e KOYULMAZ, cycle yaratır). Bindings candidate
/// projection'dan co-derived gelir (`project_analysis` içinde).
///
/// **PR F review P1-1 — identity-key aggregation:** Metric'ler `ConceptNodeId`'ye göre değil,
/// `CodeIdentityKey`'ye göre gruplanır. Birden fazla candidate node aynı identity key'e
/// bağlıysa (EI4-c N:1 convergence) tek `ObservedCodeEvidence` üretir. Aynı key+axis için
/// conflicting metric (value/provenance/strength/coverage farklı) → `ConflictingIdentityObservation`
/// reject; birebir aynı → deduplicate (idempotent replay).
///
/// Input yüzeyi: `metrics` yalnız **emit edilmiş** (admitted) metric'leri içerir.
///
/// # Determinizm
/// Identity key sırası `CodeIdentityKey` `Ord` (scheme + case policy + canonical key) ile
/// deterministik. Her identity'nin observation'ları `ObservedPhysicalMetrics::try_new` içinde
/// `PhysicalCodeMetricAxis::sort_order()` ile sıralanır.
///
/// # Binding lookup (R1a P2-1 — O(log n))
/// Projection başında `BTreeMap<&ConceptNodeId, &CodeIdentityKey>` kurulur (explicit
/// index-building — duplicate fail-fast reject). Her metric node'u O(log n) lookup.
pub(crate) fn project_observed_evidence(
    metrics: &[ProjectedCodeMetric],
    bindings: &[osp_core::anchoring::types::CodeIdentityBinding],
    context: EvidenceProjectionContext,
) -> Result<EvidenceProjectionOutput, EvidenceProjectionError> {
    use osp_core::anchoring::identity::CodeIdentityKey;

    // PR F — binding index kurulur (explicit insertion, duplicate fail-fast).
    let mut bindings_by_node: std::collections::BTreeMap<&ConceptNodeId, &CodeIdentityKey> =
        std::collections::BTreeMap::new();
    for binding in bindings {
        if bindings_by_node
            .insert(&binding.node_id, &binding.identity_key)
            .is_some()
        {
            return Err(EvidenceProjectionError::DuplicateBindingNode {
                node_id: binding.node_id.clone(),
            });
        }
    }

    // 1. Identity-key aggregation (PR F review P1-1). Her metric node'u → identity key;
    //    metric'ler identity key altında toplanır (ConceptNodeId değil).
    //
    //    Draft yapısı: (axis → (metric, source_node)) map. Aynı key+axis için:
    //    - birebir aynı value/provenance/strength/coverage → deduplicate (idempotent replay).
    //    - farklı → ConflictingIdentityObservation reject (data integrity).
    //
    //    BTreeMap key ordering: CodeIdentityKey Ord → deterministik identity sırası.
    //    İç map: PhysicalCodeAxis (Ord) → deterministik axis sırası.
    let mut by_identity: std::collections::BTreeMap<
        CodeIdentityKey,
        std::collections::BTreeMap<PhysicalCodeAxis, (&ProjectedCodeMetric, ConceptNodeId)>,
    > = std::collections::BTreeMap::new();

    for metric in metrics {
        let node_id = metric.node_id();
        let identity_key =
            bindings_by_node
                .get(node_id)
                .ok_or_else(|| EvidenceProjectionError::UnboundNode {
                    node_id: node_id.clone(),
                })?;
        let draft_axis = metric.axis();

        let identity_entry = by_identity.entry((*identity_key).clone()).or_default();

        match identity_entry.get(&draft_axis) {
            // Bu key+axis için zaten metric var — conflict veya dedup karar ver.
            Some((existing, existing_node)) => {
                if metrics_conflict(existing, metric) {
                    return Err(EvidenceProjectionError::ConflictingIdentityObservation {
                        identity_key: (*identity_key).clone(),
                        axis: draft_axis,
                        first_node: existing_node.clone(),
                        second_node: node_id.clone(),
                    });
                }
                // Birebir aynı → deduplicate (idempotent replay). Yeni metric'i ekleme.
            }
            None => {
                identity_entry.insert(draft_axis, (metric, node_id.clone()));
            }
        }
    }

    // 2. Her identity için tek ObservedCodeEvidence üret.
    let mut evidence: Vec<ObservedCodeEvidence> = Vec::with_capacity(by_identity.len());
    let mut partial_count = 0usize;

    for (identity_key, axis_map) in by_identity {
        // axis_map deterministik sıralı (PhysicalCodeAxis Ord); values() aynı sırayla verir.
        let mut observations: Vec<ObservedPhysicalMetric> = Vec::with_capacity(axis_map.len());
        for (metric, _node_id) in axis_map.values() {
            observations.push(convert_metric_to_observation(metric)?);
        }

        // Collection validation (non-empty input yüzeyi garantisidir; DuplicateAxis defensive).
        // R1a bakım: identity aggregation sonrası collection hatası için identity_key context
        // (derive_entity_id() sentetik entity ID göstermez — daha dürüst).
        let collection = ObservedPhysicalMetrics::try_new(observations).map_err(|source| {
            EvidenceProjectionError::InvalidIdentityCollection {
                identity_key: identity_key.clone(),
                source,
            }
        })?;

        // Partial check — PhysicalCodeVector üretmeden missing_axes ile.
        if !collection.missing_axes().is_empty() {
            partial_count += 1;
        }

        // Evidence construct — PR F: CodeIdentityKey (ConceptNodeId değil).
        evidence.push(ObservedCodeEvidence::new(
            identity_key,
            collection,
            context.measured_at,
        ));
    }

    Ok(EvidenceProjectionOutput {
        report: EvidenceProjectionReport {
            input_metric_values: metrics.len(),
            evidence_objects_created: evidence.len(),
            partial_evidence_objects: partial_count,
        },
        evidence,
    })
}

/// İki projected metric'in conflict edip etmediğini kontrol et (PR F review P1-1).
///
/// Conflict = value, provenance source, strength, coverage'dan herhangi biri farklı.
/// Birebir aynı (idempotent replay) → false (deduplicate). Bu karşılaştırma projection
/// boundary'sinde; core validation `convert_metric_to_observation`'da ayrıca yapılır.
fn metrics_conflict(a: &ProjectedCodeMetric, b: &ProjectedCodeMetric) -> bool {
    a.value().get() != b.value().get()
        || a.provenance().source() != b.provenance().source()
        || a.provenance().confidence().get() != b.provenance().confidence().get()
        || a.provenance().coverage().get() != b.provenance().coverage().get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric_projection::projected_metric_for_tests;
    use crate::metric_projection::projected_metric_unchecked_for_contract_tests;
    use osp_core::anchoring::code_evidence::{
        CodeEvidenceProvider, InMemoryCodeEvidenceSource, ResolvedCodeEvidenceProvider,
    };
    use osp_core::anchoring::code_evidence::{
        CodeIdentityBindingLookup, CodeIdentityLookupError, ResolvedCodeIdentity,
    };
    use osp_core::anchoring::gate::AnchorGateContext;
    use osp_core::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};
    use osp_core::anchoring::pipeline::AnchorPipeline;
    use osp_core::anchoring::types::{
        CodeIdentityBinding, ConceptNodeId, ObservedCodeMetricSource,
    };
    use osp_core::anchoring::{ConceptEdgeKind, ConceptGraph, PacketSource};

    /// Deterministik test timestamp.
    const TEST_MEASURED_AT: u64 = 1_700_000_000;

    fn ctx() -> EvidenceProjectionContext {
        EvidenceProjectionContext {
            measured_at: TEST_MEASURED_AT,
        }
    }

    fn node(id: &str) -> ConceptNodeId {
        ConceptNodeId(id.into())
    }

    fn ts() -> ObservedCodeMetricSource {
        ObservedCodeMetricSource::TreeSitter
    }

    fn scip() -> ObservedCodeMetricSource {
        ObservedCodeMetricSource::Scip
    }

    /// PR F — test identity key üret (CaseSensitive; key olduğu gibi).
    fn identity_key(key: &str) -> CodeIdentityKey {
        CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 {
                case_policy: CodePathCasePolicy::CaseSensitive,
            },
            key,
        )
        .expect("test key geçerli")
    }

    /// PR F — node_id + identity_key binding üret.
    fn binding(node_id: &ConceptNodeId, key: &CodeIdentityKey) -> CodeIdentityBinding {
        CodeIdentityBinding {
            node_id: node_id.clone(),
            identity_key: key.clone(),
        }
    }

    /// PR F — node_id'den key'e binding üret (key = node_id.0, test kolaylığı).
    fn binding_for(node_id: &ConceptNodeId) -> CodeIdentityBinding {
        binding(node_id, &identity_key(&node_id.0))
    }

    /// PR F — metrics'teki her node için binding üret (deterministik; mevcut test'ler için).
    fn bindings_for_metrics(metrics: &[ProjectedCodeMetric]) -> Vec<CodeIdentityBinding> {
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for m in metrics {
            if seen.insert(m.node_id().0.clone()) {
                out.push(binding_for(m.node_id()));
            }
        }
        out
    }

    /// PR F — test lookup stub — birden fazla node için binding döner (adapter wire için).
    struct TestLookup {
        bindings: Vec<CodeIdentityBinding>,
    }
    impl CodeIdentityBindingLookup for TestLookup {
        fn resolve_code_identity(
            &self,
            node_id: &ConceptNodeId,
        ) -> Result<ResolvedCodeIdentity, CodeIdentityLookupError> {
            self.bindings
                .iter()
                .find(|b| &b.node_id == node_id)
                .map(|b| ResolvedCodeIdentity::new(b.node_id.clone(), b.identity_key.clone()))
                .ok_or_else(|| CodeIdentityLookupError::NodeNotFound(node_id.clone()))
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Happy-path tests (validated factory)
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn groups_metrics_by_node_deterministically() {
        // 2 node, farklı axis'ler — deterministic ConceptNodeId.0 sort sırası.
        let metrics = vec![
            projected_metric_for_tests(
                node("CodeEntity:Zeta"),
                PhysicalCodeAxis::Coupling,
                0.4,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:Alpha"),
                PhysicalCodeAxis::Coupling,
                0.5,
                ts(),
                0.9,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:Alpha"),
                PhysicalCodeAxis::Cohesion,
                0.6,
                ts(),
                0.9,
                1.0,
            ),
        ];
        let out = project_observed_evidence(
            &metrics,
            &[
                binding_for(&node("CodeEntity:Zeta")),
                binding_for(&node("CodeEntity:Alpha")),
            ],
            ctx(),
        )
        .unwrap();
        // Alpha < Zeta lexicographic → Alpha önce.
        assert_eq!(
            out.evidence[0].code_identity_key(),
            &identity_key("CodeEntity:Alpha")
        );
        assert_eq!(
            out.evidence[1].code_identity_key(),
            &identity_key("CodeEntity:Zeta")
        );
        assert_eq!(out.report.input_metric_values, 3);
        assert_eq!(out.report.evidence_objects_created, 2);
    }

    #[test]
    fn maps_all_five_axis_variants_exhaustively() {
        // 5 axis tek node'da — map_axis exhaustive + sort_order sıralı.
        let metrics = vec![
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Coupling,
                0.1,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Cohesion,
                0.2,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Instability,
                0.3,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Entropy,
                0.4,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::WitnessDepth,
                0.5,
                ts(),
                0.8,
                1.0,
            ),
        ];
        let out =
            project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx()).unwrap();
        let evidence = &out.evidence[0];
        // 5 axis → try_to_physical_vector Ok (full vector).
        let pv = evidence
            .observations()
            .try_to_physical_vector()
            .expect("5 axes → full PhysicalCodeVector");
        assert_eq!(pv.coupling, 0.1);
        assert_eq!(pv.cohesion, 0.2);
        assert_eq!(pv.instability, 0.3);
        assert_eq!(pv.entropy, 0.4);
        assert_eq!(pv.witness_depth, 0.5);
        assert_eq!(out.report.partial_evidence_objects, 0);
    }

    #[test]
    fn preserves_mixed_provenance() {
        // TreeSitter + Scip aynı node — per-axis source preserved.
        let metrics = vec![
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Coupling,
                0.4,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Cohesion,
                0.6,
                scip(),
                0.9,
                1.0,
            ),
        ];
        let out =
            project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx()).unwrap();
        let observations = out.evidence[0].observations().values();
        assert_eq!(observations[0].axis(), PhysicalCodeMetricAxis::Coupling);
        assert_eq!(observations[0].source(), ts());
        assert_eq!(observations[1].axis(), PhysicalCodeMetricAxis::Cohesion);
        assert_eq!(observations[1].source(), scip());
    }

    #[test]
    fn uses_injected_measured_at() {
        let metrics = vec![projected_metric_for_tests(
            node("CodeEntity:X"),
            PhysicalCodeAxis::Coupling,
            0.4,
            ts(),
            0.8,
            1.0,
        )];
        let out = project_observed_evidence(
            &metrics,
            &bindings_for_metrics(&metrics),
            EvidenceProjectionContext {
                measured_at: TEST_MEASURED_AT,
            },
        )
        .unwrap();
        assert_eq!(out.evidence[0].measured_at(), TEST_MEASURED_AT);
    }

    #[test]
    fn creates_partial_evidence_for_three_axes() {
        // Analyzer 3 axis üretir (Coupling/Cohesion/Instability) → partial.
        let metrics = vec![
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Coupling,
                0.4,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Cohesion,
                0.6,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Instability,
                0.3,
                ts(),
                0.8,
                1.0,
            ),
        ];
        let out =
            project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx()).unwrap();
        assert_eq!(out.report.partial_evidence_objects, 1);
        // try_to_physical_vector Err (missing Entropy + WitnessDepth).
        assert!(out.evidence[0]
            .observations()
            .try_to_physical_vector()
            .is_err());
    }

    #[test]
    fn empty_metric_slice_produces_empty_output() {
        let out = project_observed_evidence(&[], &[], ctx()).unwrap();
        assert_eq!(out.report.input_metric_values, 0);
        assert_eq!(out.report.evidence_objects_created, 0);
        assert!(out.evidence.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // In-crate compatibility proof (tur 4 — ExpectedImplementation scorer seam)
    // ═══════════════════════════════════════════════════════════════════════════
    //
    // P1 (review tur 5 düzeltme): production bridge `CodeEntityCandidate:<path>` namespace
    // üretir (derive_node_id(CodeEntityCandidate, identity_key)). `ImplementedBy` gate ise
    // `CodeEntity:<name>` (operator-promoted) identity arar — production'da henüz oluşmaz.
    // Bu yüzden compatibility proof `ExpectedImplementation` scorer seam'ini kanıtlar
    // (CodeEntityCandidate: + ExpectedImplementation → code_evidence_score > 0).
    // `ImplementedBy` gate evidence presence entity-promotion/identity milestone'una kalır
    // (CodeEntityCandidate → CodeEntity identity transition gerekiyor; prefix değişikliği
    // R1 tek-kimlik yaklaşımını deler).

    /// Compatibility proof: production `CodeEntityCandidate:` ID + ExpectedImplementation scorer seam.
    ///
    /// Production bridge `CodeEntityCandidate:<path>` üretir. Extractor `CodeEntityCandidate:Foo`
    /// ref → `ExpectedImplementation` (extractor.rs:64). Scorer `ExpectedImplementation` code-related
    /// edge → `code_evidence_strength` provider'a sorar (scorer.rs:179). Bu test gerçek production
    /// ID namespace ile evidence → provider → scorer → code_evidence_score > 0 zincirini kanıtlar.
    #[test]
    fn evidence_projection_feeds_expected_implementation_scorer_seam() {
        // Production namespace: CodeEntityCandidate:<path> (derive_node_id ile).
        let candidate_node = node("CodeEntityCandidate:payment.py");

        // 1. Evidence projection — production ID namespace + binding (PR F).
        let metrics = vec![projected_metric_for_tests(
            candidate_node.clone(),
            PhysicalCodeAxis::Coupling,
            0.42,
            scip(),
            0.85,
            1.0,
        )];
        let out =
            project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx()).unwrap();

        // 2. PR F adapter wire — key-faced source + lookup stub + adapter → CodeEvidenceProvider.
        // Production identity key = CodeEntityCandidate:payment.py (binding_for helper).
        let source = InMemoryCodeEvidenceSource::try_from_evidence(out.evidence.clone())
            .expect("distinct identity key");
        let lookup = TestLookup {
            bindings: vec![binding_for(&candidate_node)],
        };
        let provider = ResolvedCodeEvidenceProvider::new(&lookup, &source);

        // find_evidence (gate consumer) — adapter lookup → source.load.
        let found = provider
            .find_evidence(&candidate_node)
            .unwrap()
            .expect("evidence mevcut (production CodeEntityCandidate: ID)");
        assert!(found.observations().minimum_observed_strength().get() > 0.0);

        // 3. evidence_strength — scorer'ın çağırdığı provider method.
        let strength = provider.evidence_strength(&candidate_node).unwrap().get();
        assert!(strength > 0.0);

        // 4. AnchorPipeline — CodeEntityCandidate: ref → ExpectedImplementation (extractor.rs:64).
        let gate_ctx = AnchorGateContext::with_code_evidence(None, &provider);
        let pipeline = AnchorPipeline::default_pipeline();
        let text = "CodeEntityCandidate:payment.py implements authentication";
        let plan = pipeline
            .run_with_source(
                text,
                "en",
                &ConceptGraph::new(),
                PacketSource::Operator,
                &gate_ctx,
            )
            .expect("ExpectedImplementation candidate üretilmeli");

        // 5. Assert: ExpectedImplementation candidate, code_evidence_score > 0 (scorer seam).
        let expected_impl = plan
            .candidates()
            .iter()
            .find(|c| c.edge_kind() == ConceptEdgeKind::ExpectedImplementation)
            .expect("ExpectedImplementation candidate üretilmeli (CodeEntityCandidate: ref)");
        assert_eq!(
            expected_impl.target_node_id(),
            &candidate_node,
            "target = production CodeEntityCandidate: ID"
        );
        assert!(
            expected_impl.score().code_evidence_score > 0.0,
            "ExpectedImplementation scorer seam → code_evidence_score > 0"
        );
    }

    /// Negatif karşı-test: provider yok → ExpectedImplementation code_evidence_score = 0.
    ///
    /// Provider yokken ExpectedImplementation candidate hala üretilir (gate find_evidence
    /// çağırmaz) ama code_evidence_score = 0 olur (scorer provider None → 0.0).
    #[test]
    fn expected_implementation_score_zero_without_provider() {
        let gate_ctx = AnchorGateContext::no_authority();
        let pipeline = AnchorPipeline::default_pipeline();
        let text = "CodeEntityCandidate:payment.py implements authentication";
        let plan = pipeline
            .run_with_source(
                text,
                "en",
                &ConceptGraph::new(),
                PacketSource::Operator,
                &gate_ctx,
            )
            .expect("ExpectedImplementation candidate provider'sız da üretilir");

        let expected_impl = plan
            .candidates()
            .iter()
            .find(|c| c.edge_kind() == ConceptEdgeKind::ExpectedImplementation)
            .expect("ExpectedImplementation candidate mevcut");
        assert_eq!(
            expected_impl.score().code_evidence_score,
            0.0,
            "provider yok → code_evidence_score = 0"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Defensive contract-drift tests (unchecked forged factory)
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn rejects_invalid_strength_with_context() {
        // Forged: confidence > 1.0 → EvidenceStrength::new Err → InvalidStrength.
        let metrics = vec![projected_metric_unchecked_for_contract_tests(
            node("CodeEntity:X"),
            PhysicalCodeAxis::Coupling,
            0.4,
            ts(),
            1.5, // forged — range dışı confidence
            1.0,
        )];
        let err = project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx())
            .unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::InvalidStrength {
                axis: PhysicalCodeAxis::Coupling,
                ..
            }
        ));
    }

    #[test]
    fn rejects_invalid_coverage_with_context() {
        // Forged: coverage > 1.0 → EvidenceCoverage::new Err → InvalidCoverage.
        let metrics = vec![projected_metric_unchecked_for_contract_tests(
            node("CodeEntity:X"),
            PhysicalCodeAxis::Coupling,
            0.4,
            ts(),
            0.8,
            1.5, // forged — range dışı coverage (zero değil, AboveMaximum)
        )];
        let err = project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx())
            .unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::InvalidCoverage {
                axis: PhysicalCodeAxis::Coupling,
                source: CoreMetricScalarViolation::AboveMaximum,
                ..
            }
        ));
    }

    #[test]
    fn defensively_handles_observation_contract_mismatch() {
        // Forged: NaN value → ObservedPhysicalMetric::new InvalidValue → InvalidObservation.
        let metrics = vec![projected_metric_unchecked_for_contract_tests(
            node("CodeEntity:X"),
            PhysicalCodeAxis::Coupling,
            f64::NAN, // forged — NaN value
            ts(),
            0.8,
            1.0,
        )];
        let err = project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx())
            .unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::InvalidObservation {
                axis: PhysicalCodeAxis::Coupling,
                ..
            }
        ));
    }

    #[test]
    fn rejects_duplicate_axis_per_node_with_context() {
        // PR F review P1-1: aynı node + aynı axis + farklı değer → artık identity aggregation'da
        // ConflictingIdentityObservation (InvalidCollection DuplicateAxis DEĞİL — aggregation
        // önce axis dedup/conflict yapıyor, InvalidCollection unreachable for single-identity).
        //
        // Tek node, tek identity, aynı axis'te iki FARKLI metric (value 0.4 vs 0.5) → conflict.
        let metrics = vec![
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Coupling,
                0.4,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Coupling,
                0.5,
                ts(),
                0.7,
                1.0,
            ),
        ];
        let err = project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx())
            .unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::ConflictingIdentityObservation {
                axis: PhysicalCodeAxis::Coupling,
                ..
            }
        ));
    }

    #[test]
    fn same_identity_same_axis_identical_metrics_deduplicates() {
        // PR F review P1-1: aynı identity + aynı axis + BİREBİR AYNI metric → deduplicate
        // (idempotent replay). İki metric → tek observation → tek evidence.
        let metrics = vec![
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Coupling,
                0.4,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                node("CodeEntity:X"),
                PhysicalCodeAxis::Coupling,
                0.4, // birebir aynı
                ts(),
                0.8,
                1.0,
            ),
        ];
        let out =
            project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx()).unwrap();
        assert_eq!(out.evidence.len(), 1, "deduplicate → tek evidence");
        assert_eq!(
            out.evidence[0].observations().values().len(),
            1,
            "tek Coupling observation"
        );
    }

    #[test]
    fn two_candidate_nodes_same_identity_key_emit_one_evidence() {
        // PR F review P1-1 / EI4-c: iki candidate node aynı CodeIdentityKey'e bağlı →
        // tek ObservedCodeEvidence (N:1 convergence). Farklı axis'ler birleşir.
        let candidate_a = node("CodeEntityCandidate:a.py");
        let candidate_b = node("CodeEntityCandidate:b.py");
        let shared_key = identity_key("shared-identity");

        let metrics = vec![
            projected_metric_for_tests(
                candidate_a.clone(),
                PhysicalCodeAxis::Coupling,
                0.4,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                candidate_b.clone(),
                PhysicalCodeAxis::Cohesion,
                0.5,
                ts(),
                0.9,
                1.0,
            ),
        ];
        // İki candidate aynı identity key'e bound.
        let bindings = vec![
            binding(&candidate_a, &shared_key),
            binding(&candidate_b, &shared_key),
        ];
        let out = project_observed_evidence(&metrics, &bindings, ctx()).unwrap();
        assert_eq!(
            out.evidence.len(),
            1,
            "EI4-c: iki candidate aynı key → tek evidence (N:1 convergence)"
        );
        assert_eq!(
            out.evidence[0].code_identity_key(),
            &shared_key,
            "evidence shared key taşıyor"
        );
        // İki farklı axis birleşti → 2 observation.
        assert_eq!(out.evidence[0].observations().values().len(), 2);
    }

    #[test]
    fn two_candidate_nodes_same_identity_key_conflicting_axis_rejects() {
        // PR F review P1-1: iki candidate aynı key + AYNI axis + FARKLI değer → conflict.
        let candidate_a = node("CodeEntityCandidate:a.py");
        let candidate_b = node("CodeEntityCandidate:b.py");
        let shared_key = identity_key("shared-identity");

        let metrics = vec![
            projected_metric_for_tests(
                candidate_a.clone(),
                PhysicalCodeAxis::Coupling,
                0.4,
                ts(),
                0.8,
                1.0,
            ),
            projected_metric_for_tests(
                candidate_b.clone(),
                PhysicalCodeAxis::Coupling, // aynı axis
                0.6,                        // farklı değer → conflict
                ts(),
                0.9,
                1.0,
            ),
        ];
        let bindings = vec![
            binding(&candidate_a, &shared_key),
            binding(&candidate_b, &shared_key),
        ];
        let err = project_observed_evidence(&metrics, &bindings, ctx()).unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::ConflictingIdentityObservation {
                axis: PhysicalCodeAxis::Coupling,
                ..
            }
        ));
    }

    #[test]
    fn projection_rejects_duplicate_binding_node() {
        // PR F review P1-2: aynı node için iki farklı binding → DuplicateBindingNode.
        let n = node("CodeEntity:X");
        let metrics = vec![projected_metric_for_tests(
            n.clone(),
            PhysicalCodeAxis::Coupling,
            0.4,
            ts(),
            0.8,
            1.0,
        )];
        let bindings = vec![
            binding(&n, &identity_key("key-a")),
            binding(&n, &identity_key("key-b")),
        ];
        let err = project_observed_evidence(&metrics, &bindings, ctx()).unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::DuplicateBindingNode { node_id } if node_id == n
        ));
    }

    #[test]
    fn projection_rejects_unbound_node() {
        // PR F review P1-2: metric node'u için binding yok → UnboundNode.
        let n = node("CodeEntity:X");
        let metrics = vec![projected_metric_for_tests(
            n.clone(),
            PhysicalCodeAxis::Coupling,
            0.4,
            ts(),
            0.8,
            1.0,
        )];
        // Boş binding listesi → node'un binding'i yok.
        let err = project_observed_evidence(&metrics, &[], ctx()).unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::UnboundNode { node_id } if node_id == n
        ));
    }

    #[test]
    fn rejects_zero_coverage_positive_strength_contract_drift() {
        // Forged: coverage=0, strength=0.8 → ZeroCoverage (tur 4 karar 3).
        // coverage=0 core'da temsil edilebilir ama PR B omission sözleşmesiyle tutarsız → reject.
        let metrics = vec![projected_metric_unchecked_for_contract_tests(
            node("CodeEntity:X"),
            PhysicalCodeAxis::Coupling,
            0.4,
            ts(),
            0.8,
            0.0, // forged — zero coverage + positive strength
        )];
        let err = project_observed_evidence(&metrics, &bindings_for_metrics(&metrics), ctx())
            .unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::ZeroCoverage {
                axis: PhysicalCodeAxis::Coupling,
                ..
            }
        ));
    }
}
