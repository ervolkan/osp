// Test modülü yazılana kadar dead-code (accessor/newtype production'da henüz read yok).
#![allow(dead_code)]

//! Analysis metric projection — axis-granular metric draft (NOT core evidence).
//!
//! Bu modül core'un tam 5-axis evidence tipini ÜRETMEZ (bkz. anchoring/types.rs);
//! yalnız analyzer ölçümlerini kabul edilen axis'lere projekte eder.
//! INV-C6: eksik veri tam veri gibi gösterilmez (entropy/witness_depth üretilmez).
//!
//! **GUARD KURALI (N1):** bu dosyada anchoring/types.rs'teki tam evidence/vector tip adları
//! yorumda bile geçmez — tests/architecture_guards.rs substring kontrolü. Dolaylama kullanılır.
//!
//! # C1 doğrulama sırası
//! value → confidence → coverage doğrulaması source admission'dan ÖNCE. Placeholder + NaN
//! sessizce skip edilmez → InvalidMetric error. Tutarlılık > kullanılabilirlik.

use osp_analyzer::contract::AnalysisResult;
use osp_core::anchoring::types::{ConceptNodeId, ObservedCodeMetricSource};
use osp_core::coords::MetricSource;
use osp_core::space::NodeId;

// ═══════════════════════════════════════════════════════════════════════════════
// Axis model
// ═══════════════════════════════════════════════════════════════════════════════

/// Physical code axis (5-axis core uzayı — Paper 1 sabit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PhysicalCodeAxis {
    Coupling,
    Cohesion,
    Instability,
    Entropy,
    WitnessDepth,
}

impl PhysicalCodeAxis {
    /// Deterministik sort sırası (enum declaration'a bağımlı değil).
    pub(crate) const fn stable_rank(self) -> u8 {
        match self {
            Self::Coupling => 0,
            Self::Cohesion => 1,
            Self::Instability => 2,
            Self::Entropy => 3,
            Self::WitnessDepth => 4,
        }
    }
}

/// 5-elemanlı sabit axis alanı bitset (BTreeSet/Ord gerektirmez, sıralama bağımlılığı yok).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AxisSet(u8);

impl AxisSet {
    const COUPLING: u8 = 1 << 0;
    const COHESION: u8 = 1 << 1;
    const INSTABILITY: u8 = 1 << 2;
    const ENTROPY: u8 = 1 << 3;
    const WITNESS_DEPTH: u8 = 1 << 4;

    pub(crate) const fn from_axes(axes: &[PhysicalCodeAxis]) -> Self {
        let mut bits = 0u8;
        let mut i = 0;
        while i < axes.len() {
            bits |= match axes[i] {
                PhysicalCodeAxis::Coupling => Self::COUPLING,
                PhysicalCodeAxis::Cohesion => Self::COHESION,
                PhysicalCodeAxis::Instability => Self::INSTABILITY,
                PhysicalCodeAxis::Entropy => Self::ENTROPY,
                PhysicalCodeAxis::WitnessDepth => Self::WITNESS_DEPTH,
            };
            i += 1;
        }
        Self(bits)
    }

    pub(crate) const fn contains(self, axis: PhysicalCodeAxis) -> bool {
        let flag = match axis {
            PhysicalCodeAxis::Coupling => Self::COUPLING,
            PhysicalCodeAxis::Cohesion => Self::COHESION,
            PhysicalCodeAxis::Instability => Self::INSTABILITY,
            PhysicalCodeAxis::Entropy => Self::ENTROPY,
            PhysicalCodeAxis::WitnessDepth => Self::WITNESS_DEPTH,
        };
        self.0 & flag != 0
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Validated scalar newtypes (C3 — type invariant, convention değil)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MetricAxisValue(f64);

impl MetricAxisValue {
    pub(crate) fn new(value: f64) -> Result<Self, MetricScalarViolation> {
        if !value.is_finite() {
            return Err(MetricScalarViolation::NonFinite);
        }
        if value < 0.0 {
            return Err(MetricScalarViolation::BelowMinimum);
        }
        if value > 1.0 {
            return Err(MetricScalarViolation::AboveMaximum);
        }
        Ok(Self(value))
    }
    pub(crate) const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MetricConfidence(f64);

impl MetricConfidence {
    pub(crate) fn new(value: f64) -> Result<Self, MetricScalarViolation> {
        if !value.is_finite() {
            return Err(MetricScalarViolation::NonFinite);
        }
        if value < 0.0 {
            return Err(MetricScalarViolation::BelowMinimum);
        }
        if value > 1.0 {
            return Err(MetricScalarViolation::AboveMaximum);
        }
        Ok(Self(value))
    }
    pub(crate) const fn get(self) -> f64 {
        self.0
    }
    pub(crate) const fn is_zero(self) -> bool {
        self.0 == 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MetricCoverage(f64);

impl MetricCoverage {
    pub(crate) fn new(value: f64) -> Result<Self, MetricScalarViolation> {
        if !value.is_finite() {
            return Err(MetricScalarViolation::NonFinite);
        }
        if value < 0.0 {
            return Err(MetricScalarViolation::BelowMinimum);
        }
        if value > 1.0 {
            return Err(MetricScalarViolation::AboveMaximum);
        }
        Ok(Self(value))
    }
    pub(crate) const fn get(self) -> f64 {
        self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Projection model (private fields + accessors)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectedMetricProvenance {
    source: ObservedCodeMetricSource,
    confidence: MetricConfidence,
    coverage: MetricCoverage,
}

impl ProjectedMetricProvenance {
    pub(crate) fn source(&self) -> ObservedCodeMetricSource {
        self.source
    }
    pub(crate) fn confidence(&self) -> MetricConfidence {
        self.confidence
    }
    pub(crate) fn coverage(&self) -> MetricCoverage {
        self.coverage
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectedCodeMetric {
    node_id: ConceptNodeId,
    axis: PhysicalCodeAxis,
    value: MetricAxisValue,
    provenance: ProjectedMetricProvenance,
}

impl ProjectedCodeMetric {
    pub(crate) fn node_id(&self) -> &ConceptNodeId {
        &self.node_id
    }
    pub(crate) fn axis(&self) -> PhysicalCodeAxis {
        self.axis
    }
    pub(crate) fn value(&self) -> MetricAxisValue {
        self.value
    }
    pub(crate) fn provenance(&self) -> &ProjectedMetricProvenance {
        &self.provenance
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AnalysisMetricProjection {
    pub metrics: Vec<ProjectedCodeMetric>,
    pub report: MetricProjectionReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MetricProjectionReport {
    pub module_nodes_seen: usize,
    pub input_axis_values: usize,
    pub projected_axis_values: usize,
    pub skipped_placeholder: usize,
    pub skipped_heuristic: usize,
    pub skipped_zero_confidence: usize,
    pub analyzer_declared_axes: AxisSet,   // capability (analyzer declares 3 axes)
    pub analyzer_unavailable_axes: AxisSet, // capability (entropy/witness_depth)
    pub projected_axes: AxisSet,           // actually projected after admission (distinct from declared)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Source admission + error taksonomisi
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetricSourceRejection {
    PlaceholderSource,
    HeuristicNotAdmitted,
}

/// Exhaustive match → yeni MetricSource compile-time zorlanır.
fn admit_metric_source(source: MetricSource) -> Result<ObservedCodeMetricSource, MetricSourceRejection> {
    match source {
        MetricSource::TreeSitter => Ok(ObservedCodeMetricSource::TreeSitter),
        MetricSource::Scip => Ok(ObservedCodeMetricSource::Scip),
        MetricSource::Placeholder => Err(MetricSourceRejection::PlaceholderSource),
        MetricSource::Heuristic => Err(MetricSourceRejection::HeuristicNotAdmitted),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InvalidMetricField {
    Value,
    Confidence,
    Coverage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetricScalarViolation {
    NonFinite,
    BelowMinimum,
    AboveMaximum,
}

/// Metric projection error — bridge-level (sessiz skip YOK).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MetricProjectionError {
    #[error("missing projected identity for analysis node {analysis_node_id}")]
    MissingProjectedIdentity { analysis_node_id: NodeId },
    #[error("missing module metrics for analysis node {analysis_node_id}")]
    MissingModuleMetrics { analysis_node_id: NodeId },
    #[error("duplicate projected axis: node={node_id:?}, axis={axis:?}")]
    DuplicateProjectedAxis {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
    },
    #[error("invalid metric: node={node_id:?}, axis={axis:?}, field={field:?}, violation={violation:?}")]
    InvalidMetric {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
        field: InvalidMetricField,
        violation: MetricScalarViolation,
    },
}

// ═══════════════════════════════════════════════════════════════════════════════
// project_code_metrics — C1 doğrulama sırası
// ═══════════════════════════════════════════════════════════════════════════════

/// Analyzer ModuleMetrics → axis-granular metric draft projection.
/// Identity PR A'dan tüketilir (scheme/policy YOK — R1).
/// Yalnız coupling/cohesion/instability; entropy/witness_depth üretilmez (INV-C6).
pub(crate) fn project_code_metrics(
    analysis: &AnalysisResult,
    identity_index: &crate::analysis_bridge::AnalysisProjectionIndex,
) -> Result<AnalysisMetricProjection, MetricProjectionError> {
    // Analyzer capability: 3 axis declares, 2 unavailable.
    let analyzer_declared_axes =
        AxisSet::from_axes(&[PhysicalCodeAxis::Coupling, PhysicalCodeAxis::Cohesion, PhysicalCodeAxis::Instability]);
    let analyzer_unavailable_axes =
        AxisSet::from_axes(&[PhysicalCodeAxis::Entropy, PhysicalCodeAxis::WitnessDepth]);

    let mut metrics: Vec<ProjectedCodeMetric> = Vec::new();
    let mut report = MetricProjectionReport {
        analyzer_declared_axes,
        analyzer_unavailable_axes,
        ..Default::default()
    };

    // Deterministik node sırası.
    let mut node_ids: Vec<NodeId> = analysis.space.nodes.keys().copied().collect();
    node_ids.sort();
    // Yalnızca Module node'ları (PR A ile aynı filtre).
    let module_node_ids: Vec<NodeId> = node_ids
        .into_iter()
        .filter(|&id| analysis.space.nodes[&id].kind == osp_core::space::NodeKind::Module)
        .collect();

    // Duplicate (ConceptNodeId, axis) detection — String key (ConceptNodeId Ord değil).
    let mut seen: std::collections::BTreeSet<(String, u8)> = std::collections::BTreeSet::new();

    for node_id in &module_node_ids {
        report.module_nodes_seen += 1;

        // Identity — PR A'dan tüket (R1).
        let concept_id = identity_index
            .concept_node_id_for(*node_id)
            .ok_or(MetricProjectionError::MissingProjectedIdentity {
                analysis_node_id: *node_id,
            })?;

        // Module metrics — MissingModuleMetrics typed error.
        let module_metrics = analysis.module_metrics.get(node_id).ok_or(
            MetricProjectionError::MissingModuleMetrics {
                analysis_node_id: *node_id,
            },
        )?;

        // 3 axis: coupling, cohesion, instability.
        for (axis, metric_value) in [
            (PhysicalCodeAxis::Coupling, &module_metrics.coupling),
            (PhysicalCodeAxis::Cohesion, &module_metrics.cohesion),
            (PhysicalCodeAxis::Instability, &module_metrics.instability),
        ] {
            report.input_axis_values += 1;

            // C1: doğrulama ÖNCE (value → confidence → coverage), source admission'dan önce.
            let value = MetricAxisValue::new(metric_value.value).map_err(|v| {
                MetricProjectionError::InvalidMetric {
                    node_id: concept_id.clone(),
                    axis,
                    field: InvalidMetricField::Value,
                    violation: v,
                }
            })?;
            let confidence = MetricConfidence::new(metric_value.confidence).map_err(|v| {
                MetricProjectionError::InvalidMetric {
                    node_id: concept_id.clone(),
                    axis,
                    field: InvalidMetricField::Confidence,
                    violation: v,
                }
            })?;
            let coverage = MetricCoverage::new(metric_value.coverage).map_err(|v| {
                MetricProjectionError::InvalidMetric {
                    node_id: concept_id.clone(),
                    axis,
                    field: InvalidMetricField::Coverage,
                    violation: v,
                }
            })?;

            // Source admission (Placeholder/Heuristic skip — N4: Heuristic defensive policy).
            let source = match admit_metric_source(metric_value.source) {
                Ok(s) => s,
                Err(MetricSourceRejection::PlaceholderSource) => {
                    report.skipped_placeholder += 1;
                    continue;
                }
                Err(MetricSourceRejection::HeuristicNotAdmitted) => {
                    report.skipped_heuristic += 1;
                    continue;
                }
            };

            // Zero-confidence omission (MetricConfidence::new Ok ama is_zero).
            if confidence.is_zero() {
                report.skipped_zero_confidence += 1;
                continue;
            }

            // Duplicate (ConceptNodeId, axis) — many-to-one collision.
            // Assembler yolundan unreachable (try_new önce); defensive invariant.
            let dedup_key = (concept_id.0.clone(), axis.stable_rank());
            if !seen.insert(dedup_key) {
                return Err(MetricProjectionError::DuplicateProjectedAxis {
                    node_id: concept_id.clone(),
                    axis,
                });
            }

            metrics.push(ProjectedCodeMetric {
                node_id: concept_id.clone(),
                axis,
                value,
                provenance: ProjectedMetricProvenance {
                    source,
                    confidence,
                    coverage,
                },
            });
            report.projected_axis_values += 1;
            // projected_axes: actually emitted axes after admission (distinct from declared).
            report.projected_axes = report.projected_axes.union(AxisSet::from_axes(&[axis]));
        }
    }

    // Deterministik sort: (node_id, axis.stable_rank).
    metrics.sort_by_key(|m| (m.node_id.0.clone(), m.axis.stable_rank()));

    Ok(AnalysisMetricProjection { metrics, report })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test factory'ler (PR D — evidence_projection.rs testleri için)
// ═══════════════════════════════════════════════════════════════════════════════
//
// İki factory: validated (happy-path) + unchecked forged (defensive contract-drift testleri).
// Production constructor DEĞİL — yalnız `#[cfg(test)]`. evidence_projection.rs in-crate testleri
// ProjectedCodeMetric private field'ları construct edemediği için bu factory'lere ihtiyaç duyar.

/// Validated factory — happy-path testler için. Mevcut validated newtype'ları kullanır
/// (`MetricAxisValue::new`, `MetricConfidence::new`, `MetricCoverage::new`). Invalid input
/// panic eder (test yanlış yazılmış).
#[cfg(test)]
pub(crate) fn projected_metric_for_tests(
    node_id: osp_core::anchoring::types::ConceptNodeId,
    axis: PhysicalCodeAxis,
    value: f64,
    source: ObservedCodeMetricSource,
    confidence: f64,
    coverage: f64,
) -> ProjectedCodeMetric {
    ProjectedCodeMetric {
        node_id,
        axis,
        value: MetricAxisValue::new(value)
            .expect("projected_metric_for_tests value must be in [0,1]"),
        provenance: ProjectedMetricProvenance {
            source,
            confidence: MetricConfidence::new(confidence)
                .expect("projected_metric_for_tests confidence must be in [0,1]"),
            coverage: MetricCoverage::new(coverage)
                .expect("projected_metric_for_tests coverage must be in [0,1]"),
        },
    }
}

/// Unchecked forged factory — defensive conversion error testleri için.
///
/// **Intentionally bypasses PR B validation** to simulate cross-version or contract-drift
/// input at the PR D boundary. Tuple newtype alanlarını aynı modül içinde doğrudan kurar
/// (validation bypass). Yalnız defensive conversion error testleri için kullanılır;
/// happy-path testleri bu factory'yi KULLANMAZ.
#[cfg(test)]
pub(crate) fn projected_metric_unchecked_for_contract_tests(
    node_id: osp_core::anchoring::types::ConceptNodeId,
    axis: PhysicalCodeAxis,
    value: f64,
    source: ObservedCodeMetricSource,
    confidence: f64,
    coverage: f64,
) -> ProjectedCodeMetric {
    // SAFETY: test-only; bypass validation to forge contract-drift input (range-dışı value/
    // confidence/coverage, zero coverage + positive strength, vb.). Production constructor
    // (project_code_metrics) bu değerleri asla üretmez; bu factory PR D boundary defense testleri için.
    ProjectedCodeMetric {
        node_id,
        axis,
        value: MetricAxisValue::new(value)
            .unwrap_or(MetricAxisValue(std::f64::NAN)), // forged — NaN bile geçebilir
        provenance: ProjectedMetricProvenance {
            source,
            confidence: MetricConfidence::new(confidence)
                .unwrap_or(MetricConfidence(confidence)), // forged — raw değer
            coverage: MetricCoverage::new(coverage)
                .unwrap_or(MetricCoverage(coverage)), // forged — raw değer
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis_bridge::AnalysisProjectionIndex;
    use osp_analyzer::contract::{AnalysisResult, ModuleMetrics, RepoMetrics, SemanticCoverage};
    use osp_core::anchoring::types::ConceptNodeId;
    use osp_core::coords::MetricValue;
    use osp_core::space::{Node, NodeClassification, NodeKind, NodeRole, Space};
    use std::collections::HashMap;

    /// Synthetic analysis with module_metrics — test fixture.
    fn analysis_with_metrics(
        nodes: Vec<(
            u64,
            &str,
            ModuleMetrics,
        )>,
    ) -> (AnalysisResult, AnalysisProjectionIndex) {
        let mut space = Space::default();
        let mut node_paths = HashMap::new();
        let mut module_metrics = HashMap::new();
        let mut index_entries: Vec<(NodeId, ConceptNodeId)> = Vec::new();
        for (id, path, metrics) in nodes {
            space.nodes.insert(
                id,
                Node {
                    id,
                    kind: NodeKind::Module,
                    mass: 10.0,
                    classification: NodeClassification::Production,
                    role: NodeRole::Core,
                    ..Default::default()
                },
            );
            node_paths.insert(id, path.to_string());
            let concept_id = ConceptNodeId(format!("CodeEntityCandidate:{path}"));
            index_entries.push((id, concept_id));
            module_metrics.insert(id, metrics);
        }
        let analysis = AnalysisResult {
            space,
            module_metrics,
            node_paths,
            node_semantics: HashMap::new(),
            node_witnesses: HashMap::new(),
            repo_metrics: RepoMetrics {
                abstractness: MetricValue::placeholder(0.0),
                main_sequence_distance: MetricValue::placeholder(0.0),
                abstractness_by_package: None,
            },
            semantic_coverage: SemanticCoverage::none("testhead".into()),
            diagnostics: vec![],
        };
        let index = AnalysisProjectionIndex::for_tests(index_entries).unwrap();
        (analysis, index)
    }

    fn ts(value: f64) -> MetricValue {
        MetricValue::tree_sitter(value, 1.0)
    }
    fn scip(value: f64) -> MetricValue {
        MetricValue::scip(value, 1.0, false)
    }
    fn placeholder(value: f64) -> MetricValue {
        MetricValue::placeholder(value)
    }
    fn heuristic(value: f64, conf: f64) -> MetricValue {
        MetricValue::heuristic(value, conf)
    }

    // ── Mutlu yol ────────────────────────────────────────────────────────────

    #[test]
    fn happy_path_3_modules_9_axis_metrics() {
        let metrics = ModuleMetrics {
            coupling: ts(0.5),
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![
            (1, "src/a.rs", metrics.clone()),
            (2, "src/b.rs", metrics.clone()),
            (3, "src/c.rs", metrics),
        ]);
        let proj = project_code_metrics(&analysis, &index).unwrap();
        // N6 invariant: input_axis_values == module_nodes_seen × 3.
        assert_eq!(proj.report.module_nodes_seen, 3);
        assert_eq!(proj.report.input_axis_values, 9);
        assert_eq!(proj.report.projected_axis_values, 9);
        assert_eq!(proj.metrics.len(), 9);
        // No skips.
        assert_eq!(proj.report.skipped_placeholder, 0);
        assert_eq!(proj.report.skipped_heuristic, 0);
        assert_eq!(proj.report.skipped_zero_confidence, 0);
        // Happy path: projected_axes == declared (all admitted).
        assert_eq!(proj.report.projected_axes, proj.report.analyzer_declared_axes);
    }

    // ── AxisSet capability ───────────────────────────────────────────────────

    #[test]
    fn axis_set_provided_union_unavailable_all_5() {
        let metrics = ModuleMetrics {
            coupling: ts(0.5),
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let proj = project_code_metrics(&analysis, &index).unwrap();
        let all = proj.report.analyzer_declared_axes.union(proj.report.analyzer_unavailable_axes);
        assert!(all.contains(PhysicalCodeAxis::Coupling));
        assert!(all.contains(PhysicalCodeAxis::Cohesion));
        assert!(all.contains(PhysicalCodeAxis::Instability));
        assert!(all.contains(PhysicalCodeAxis::Entropy));
        assert!(all.contains(PhysicalCodeAxis::WitnessDepth));
        // No overlap.
        assert!(!proj.report.analyzer_declared_axes.contains(PhysicalCodeAxis::Entropy));
        assert!(!proj.report.analyzer_declared_axes.contains(PhysicalCodeAxis::WitnessDepth));
    }

    // ── Source admission skip (N4: Heuristic defensive policy) ────────────────

    #[test]
    fn placeholder_source_skipped() {
        let metrics = ModuleMetrics {
            coupling: ts(0.5),
            cohesion: placeholder(0.5), // Placeholder → skip
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let proj = project_code_metrics(&analysis, &index).unwrap();
        assert_eq!(proj.report.skipped_placeholder, 1);
        assert_eq!(proj.report.projected_axis_values, 2); // coupling + instability
        // Declared vs projected: Cohesion declared ama projected değil (Placeholder).
        assert!(proj.report.analyzer_declared_axes.contains(PhysicalCodeAxis::Cohesion));
        assert!(!proj.report.projected_axes.contains(PhysicalCodeAxis::Cohesion));
        assert!(proj.report.projected_axes.contains(PhysicalCodeAxis::Coupling));
        assert!(proj.report.projected_axes.contains(PhysicalCodeAxis::Instability));
    }

    #[test]
    fn heuristic_source_skipped() {
        // N4: Heuristic analyzer doğal akıştan üretmez — defensive policy testi.
        let metrics = ModuleMetrics {
            coupling: ts(0.5),
            cohesion: heuristic(0.7, 0.5), // Heuristic → skip
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let proj = project_code_metrics(&analysis, &index).unwrap();
        assert_eq!(proj.report.skipped_heuristic, 1);
        assert_eq!(proj.report.projected_axis_values, 2);
    }

    #[test]
    fn scip_source_admitted() {
        let metrics = ModuleMetrics {
            coupling: scip(0.5),
            cohesion: scip(0.7),
            instability: scip(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let proj = project_code_metrics(&analysis, &index).unwrap();
        assert_eq!(proj.report.projected_axis_values, 3);
        assert_eq!(proj.metrics[0].provenance().source(), ObservedCodeMetricSource::Scip);
    }

    // ── Zero-confidence skip ─────────────────────────────────────────────────

    #[test]
    fn zero_confidence_skipped() {
        // TreeSitter ama confidence=0 → ZeroConfidence skip.
        let metrics = ModuleMetrics {
            coupling: MetricValue::tree_sitter(0.5, 0.0), // coverage=0 → conf=0
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let proj = project_code_metrics(&analysis, &index).unwrap();
        assert_eq!(proj.report.skipped_zero_confidence, 1);
        assert_eq!(proj.report.projected_axis_values, 2);
    }

    // ── C1: doğrulama source admission'dan ÖNCE ──────────────────────────────

    #[test]
    fn c1_placeholder_with_nan_value_is_error_not_skip() {
        let metrics = ModuleMetrics {
            coupling: MetricValue {
                value: f64::NAN,
                source: MetricSource::Placeholder,
                confidence: 0.0,
                coverage: 1.0,
            },
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let err = project_code_metrics(&analysis, &index).unwrap_err();
        assert!(matches!(
            err,
            MetricProjectionError::InvalidMetric {
                field: InvalidMetricField::Value,
                violation: MetricScalarViolation::NonFinite,
                ..
            }
        ));
    }

    #[test]
    fn c1_placeholder_with_nan_confidence_is_error() {
        let metrics = ModuleMetrics {
            coupling: MetricValue {
                value: 0.5,
                source: MetricSource::Placeholder,
                confidence: f64::NAN,
                coverage: 1.0,
            },
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let err = project_code_metrics(&analysis, &index).unwrap_err();
        assert!(matches!(
            err,
            MetricProjectionError::InvalidMetric {
                field: InvalidMetricField::Confidence,
                violation: MetricScalarViolation::NonFinite,
                ..
            }
        ));
    }

    #[test]
    fn c1_placeholder_with_coverage_above_one_is_error() {
        let metrics = ModuleMetrics {
            coupling: MetricValue {
                value: 0.5,
                source: MetricSource::Placeholder,
                confidence: 0.0,
                coverage: 2.0,
            },
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let err = project_code_metrics(&analysis, &index).unwrap_err();
        assert!(matches!(
            err,
            MetricProjectionError::InvalidMetric {
                field: InvalidMetricField::Coverage,
                violation: MetricScalarViolation::AboveMaximum,
                ..
            }
        ));
    }

    // ── Invalid metric ────────────────────────────────────────────────────────

    #[test]
    fn nan_value_is_error() {
        let metrics = ModuleMetrics {
            coupling: MetricValue {
                value: f64::NAN,
                source: MetricSource::TreeSitter,
                confidence: 0.75,
                coverage: 1.0,
            },
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        assert!(project_code_metrics(&analysis, &index).is_err());
    }

    #[test]
    fn value_above_one_is_error() {
        let metrics = ModuleMetrics {
            coupling: MetricValue {
                value: 1.5,
                source: MetricSource::TreeSitter,
                confidence: 0.75,
                coverage: 1.0,
            },
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let err = project_code_metrics(&analysis, &index).unwrap_err();
        assert!(matches!(
            err,
            MetricProjectionError::InvalidMetric {
                field: InvalidMetricField::Value,
                violation: MetricScalarViolation::AboveMaximum,
                ..
            }
        ));
    }

    // ── Missing identity / metrics ────────────────────────────────────────────

    #[test]
    fn missing_projected_identity_is_error() {
        let metrics = ModuleMetrics {
            coupling: ts(0.5),
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, _) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        // Boş index → node 1 için identity yok.
        let empty_index = AnalysisProjectionIndex::for_tests(vec![]).unwrap();
        let err = project_code_metrics(&analysis, &empty_index).unwrap_err();
        assert!(matches!(err, MetricProjectionError::MissingProjectedIdentity { .. }));
    }

    #[test]
    fn missing_module_metrics_is_error() {
        let mut space = Space::default();
        space.nodes.insert(
            1,
            Node {
                id: 1,
                kind: NodeKind::Module,
                mass: 10.0,
                ..Default::default()
            },
        );
        let analysis = AnalysisResult {
            space,
            module_metrics: HashMap::new(), // boş
            node_paths: HashMap::new(),
            node_semantics: HashMap::new(),
            node_witnesses: HashMap::new(),
            repo_metrics: RepoMetrics {
                abstractness: MetricValue::placeholder(0.0),
                main_sequence_distance: MetricValue::placeholder(0.0),
                abstractness_by_package: None,
            },
            semantic_coverage: SemanticCoverage::none("testhead".into()),
            diagnostics: vec![],
        };
        let index = AnalysisProjectionIndex::for_tests(vec![(
            1,
            ConceptNodeId("CodeEntityCandidate:src/a.rs".into()),
        )])
        .unwrap();
        let err = project_code_metrics(&analysis, &index).unwrap_err();
        assert!(matches!(err, MetricProjectionError::MissingModuleMetrics { .. }));
    }

    // ── C5: pass-through (confidence/coverage değiştirilmeden taşınır) ───────

    #[test]
    fn c5_pass_through_preserves_confidence_and_coverage() {
        // C5: pass-through — analyzer MetricValue.confidence/coverage değiştirilmeden taşınır.
        // Formula kopyalanmaz: MetricValue'nun kendi field'ıyla karşılaştır (analyzer katsayısı
        // değişirse test yanlış nedenle kırılmaz).
        let coupling_mv = MetricValue::tree_sitter(0.4, 0.82);
        let metrics = ModuleMetrics {
            coupling: coupling_mv.clone(),
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![(1, "src/a.rs", metrics)]);
        let proj = project_code_metrics(&analysis, &index).unwrap();
        let coupling_metric = proj
            .metrics
            .iter()
            .find(|m| m.axis() == PhysicalCodeAxis::Coupling)
            .unwrap();
        // MetricValue'nun kendi alanlarıyla birebir karşılaştır (formül bilgisi YOK).
        assert_eq!(coupling_metric.provenance().confidence().get(), coupling_mv.confidence);
        assert_eq!(coupling_metric.provenance().coverage().get(), coupling_mv.coverage);
    }

    // ── N6 invariant: input_axis_values == module_nodes_seen × 3 ─────────────

    #[test]
    fn n6_input_axis_values_equals_module_nodes_times_3() {
        let metrics = ModuleMetrics {
            coupling: ts(0.5),
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let (analysis, index) = analysis_with_metrics(vec![
            (1, "src/a.rs", metrics.clone()),
            (2, "src/b.rs", metrics),
        ]);
        let proj = project_code_metrics(&analysis, &index).unwrap();
        assert_eq!(proj.report.input_axis_values, proj.report.module_nodes_seen * 3);
    }

    // ── Many-to-one collision: DuplicateProjectedAxis ─────────────────────────
    // Assembler yolundan unreachable (try_new önce yakalar) ama for_tests ile
    // iki node → aynı ConceptNodeId mümkün. Defensive invariant test.

    #[test]
    fn many_to_one_collision_is_duplicate_axis_error() {
        let metrics = ModuleMetrics {
            coupling: ts(0.5),
            cohesion: ts(0.7),
            instability: ts(0.3),
        };
        let mut space = Space::default();
        let mut node_paths = HashMap::new();
        let mut module_metrics_map = HashMap::new();
        // İki node, farklı path'ler ama index'te aynı ConceptNodeId'ye map.
        for (id, path) in [(1u64, "src/a.rs"), (2u64, "src/b.rs")] {
            space.nodes.insert(
                id,
                Node {
                    id,
                    kind: NodeKind::Module,
                    mass: 10.0,
                    ..Default::default()
                },
            );
            node_paths.insert(id, path.to_string());
            module_metrics_map.insert(id, metrics.clone());
        }
        let analysis = AnalysisResult {
            space,
            module_metrics: module_metrics_map,
            node_paths,
            node_semantics: HashMap::new(),
            node_witnesses: HashMap::new(),
            repo_metrics: RepoMetrics {
                abstractness: MetricValue::placeholder(0.0),
                main_sequence_distance: MetricValue::placeholder(0.0),
                abstractness_by_package: None,
            },
            semantic_coverage: SemanticCoverage::none("testhead".into()),
            diagnostics: vec![],
        };
        // İki analyzer node → aynı ConceptNodeId (many-to-one).
        let shared_concept_id = ConceptNodeId("CodeEntityCandidate:src/shared.rs".into());
        let index = AnalysisProjectionIndex::for_tests(vec![
            (1, shared_concept_id.clone()),
            (2, shared_concept_id.clone()),
        ])
        .unwrap();
        let err = project_code_metrics(&analysis, &index).unwrap_err();
        assert!(
            matches!(err, MetricProjectionError::DuplicateProjectedAxis { .. }),
            "many-to-one collision must produce DuplicateProjectedAxis, got {err:?}"
        );
    }
}
