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
use osp_core::anchoring::{MetricScalarViolation as CoreMetricScalarViolation, PhysicalCodeMetricAxis};

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
    #[error("{node_id} {axis:?} axis zero coverage — observation değildir (PR B omission beklenir)")]
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
    #[error("{node_id} collection contract mismatch: {source}")]
    InvalidCollection {
        node_id: ConceptNodeId,
        source: ObservedPhysicalMetricsError,
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
    let strength = EvidenceStrength::new(metric.provenance().confidence().get()).map_err(|source| {
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
/// Input yüzeyi: `metrics` yalnız **emit edilmiş** (admitted) metric'leri içerir. Her grouped
/// node'un en az bir metric'i vardır (`ObservedPhysicalMetricsError::Empty` unreachable).
///
/// # Determinizm
/// Node sırası `ConceptNodeId.0` lexicographic sort ile deterministik. Her node'un observation'ları
/// `ObservedPhysicalMetrics::try_new` içinde `PhysicalCodeMetricAxis::sort_order()` ile sıralanır.
pub(crate) fn project_observed_evidence(
    metrics: &[ProjectedCodeMetric],
    context: EvidenceProjectionContext,
) -> Result<EvidenceProjectionOutput, EvidenceProjectionError> {
    // 1. ConceptNodeId bazında group (deterministik sıra için önceden sort).
    let mut by_node: std::collections::BTreeMap<String, (ConceptNodeId, Vec<&ProjectedCodeMetric>)> =
        std::collections::BTreeMap::new();
    for metric in metrics {
        let entry = by_node
            .entry(metric.node_id().0.clone())
            .or_insert_with(|| (metric.node_id().clone(), Vec::new()));
        entry.1.push(metric);
    }

    let mut evidence: Vec<ObservedCodeEvidence> = Vec::with_capacity(by_node.len());
    let mut partial_count = 0usize;

    for (_, (node_id, node_metrics)) in by_node {
        // 2. Her metric'i observation'a dönüştür.
        let mut observations: Vec<ObservedPhysicalMetric> = Vec::with_capacity(node_metrics.len());
        for metric in &node_metrics {
            observations.push(convert_metric_to_observation(metric)?);
        }

        // 3. Collection validation (non-empty input yüzeyi garantisidir; DuplicateAxis defensive).
        let collection = ObservedPhysicalMetrics::try_new(observations).map_err(|source| {
            EvidenceProjectionError::InvalidCollection {
                node_id: node_id.clone(),
                source,
            }
        })?;

        // 4. Partial check — PhysicalCodeVector üretmeden missing_axes ile.
        if !collection.missing_axes().is_empty() {
            partial_count += 1;
        }

        // 5. Evidence construct.
        evidence.push(ObservedCodeEvidence::new(
            node_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric_projection::projected_metric_for_tests;
    use crate::metric_projection::projected_metric_unchecked_for_contract_tests;
    use osp_core::anchoring::code_evidence::InMemoryCodeEvidenceProvider;
    use osp_core::anchoring::pipeline::AnchorPipeline;
    use osp_core::anchoring::types::ConceptNodeId;
    use osp_core::anchoring::code_evidence::CodeEvidenceProvider;
    use osp_core::anchoring::gate::AnchorGateContext;
    use osp_core::anchoring::types::ObservedCodeMetricSource;
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
        let out = project_observed_evidence(&metrics, ctx()).unwrap();
        // Alpha < Zeta lexicographic → Alpha önce.
        assert_eq!(out.evidence[0].code_entity_id(), &node("CodeEntity:Alpha"));
        assert_eq!(out.evidence[1].code_entity_id(), &node("CodeEntity:Zeta"));
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
        let out = project_observed_evidence(&metrics, ctx()).unwrap();
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
        let out = project_observed_evidence(&metrics, ctx()).unwrap();
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
        let out = project_observed_evidence(&metrics, ctx()).unwrap();
        assert_eq!(out.report.partial_evidence_objects, 1);
        // try_to_physical_vector Err (missing Entropy + WitnessDepth).
        assert!(out.evidence[0]
            .observations()
            .try_to_physical_vector()
            .is_err());
    }

    #[test]
    fn empty_metric_slice_produces_empty_output() {
        let out = project_observed_evidence(&[], ctx()).unwrap();
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

        // 1. Evidence projection — production ID namespace.
        let metrics = vec![projected_metric_for_tests(
            candidate_node.clone(),
            PhysicalCodeAxis::Coupling,
            0.42,
            scip(),
            0.85,
            1.0,
        )];
        let out = project_observed_evidence(&metrics, ctx()).unwrap();

        // 2. Provider'a yükle — production ID altında lookup.
        let provider = InMemoryCodeEvidenceProvider::from_evidence(out.evidence.clone());
        let lookup = provider
            .find_evidence(&candidate_node)
            .unwrap()
            .expect("evidence mevcut (production CodeEntityCandidate: ID)");
        assert!(lookup.observations().minimum_observed_strength().get() > 0.0);

        // 3. evidence_strength — scorer'ın çağırdığı provider method.
        let strength = provider
            .evidence_strength(&candidate_node)
            .unwrap()
            .get();
        assert!(strength > 0.0);

        // 4. AnchorPipeline — CodeEntityCandidate: ref → ExpectedImplementation (extractor.rs:64).
        let gate_ctx = AnchorGateContext::with_code_evidence(None, &provider);
        let pipeline = AnchorPipeline::default_pipeline();
        let text = "CodeEntityCandidate:payment.py implements authentication";
        let plan = pipeline
            .run_with_source(text, "en", &ConceptGraph::new(), PacketSource::Operator, &gate_ctx)
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
        let err = project_observed_evidence(&metrics, ctx()).unwrap_err();
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
        let err = project_observed_evidence(&metrics, ctx()).unwrap_err();
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
        let err = project_observed_evidence(&metrics, ctx()).unwrap_err();
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
        // İki aynı axis (Coupling) aynı node'da → DuplicateAxis.
        // (project_code_metrics dedup yapar ama forged input bypass eder — defensive boundary.)
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
        let err = project_observed_evidence(&metrics, ctx()).unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::InvalidCollection {
                source: ObservedPhysicalMetricsError::DuplicateAxis {
                    axis: PhysicalCodeMetricAxis::Coupling
                },
                ..
            }
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
        let err = project_observed_evidence(&metrics, ctx()).unwrap_err();
        assert!(matches!(
            err,
            EvidenceProjectionError::ZeroCoverage {
                axis: PhysicalCodeAxis::Coupling,
                ..
            }
        ));
    }
}
