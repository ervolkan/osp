// INV-C6 compile-fail: external crate ObservedCodeEvidence literal construct edemez.
// Field'lar private → harici `ObservedCodeEvidence { ... }` engelli.
// Sadece public smart constructor `new(...)` ile üretilebilir (Patch 1, PR C).
// "Observed evidence almak = smart constructor'dan geçmiş olmak."
//
// PR C: struct artık 3 field taşır (code_entity_id, observations, measured_at).
// Eski field'lar (physical_vector, metric_source, confidence) kaldırıldı —
// axis-granular `observations: ObservedPhysicalMetrics` yerine geçti.
use osp_core::anchoring::types::{
    EvidenceCoverage, EvidenceStrength, ObservedCodeEvidence, ObservedCodeMetricSource,
    ObservedPhysicalMetric, ObservedPhysicalMetrics,
};
use osp_core::anchoring::{ConceptNodeId, PhysicalCodeMetricAxis};

fn main() {
    // observations değerini constructor ile üret (bu satır derlenir).
    let observations = ObservedPhysicalMetrics::try_new(vec![ObservedPhysicalMetric::new(
        PhysicalCodeMetricAxis::Coupling,
        0.1,
        ObservedCodeMetricSource::Scip,
        EvidenceStrength::one(),
        EvidenceCoverage::new(1.0).unwrap(),
    )
    .unwrap()])
    .unwrap();

    // Bu satır derlenmemeli: field'lar private.
    let _evidence = ObservedCodeEvidence {
        code_entity_id: ConceptNodeId("CodeEntity:X".into()),
        observations,
        measured_at: 0,
    };
}
