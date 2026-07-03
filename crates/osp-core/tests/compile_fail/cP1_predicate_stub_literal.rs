// INV-P1 compile-fail: external crate PredicateStub literal construct edemez.
// Field'lar private → harici `PredicateStub { ... }` engelli.
// Sadece public smart constructor `new(...)` ile üretilebilir (Patch 1).
// "A PredicateStub is not absence of knowledge; it is structured uncertainty."
use osp_core::anchoring::{PredicateSlot, PredicateStub, PredicateStubReason, PredicateTemplateId};
use osp_core::anchoring::ConceptNodeId;

fn main() {
    // Bu satır derlenmemeli: field'lar private.
    let _stub = PredicateStub {
        rule_id: ConceptNodeId("RuleCandidate:X".into()),
        reason: PredicateStubReason::MetricUnresolved,
        unresolved_slots: vec![PredicateSlot::Metric],
        suggested_templates: vec![PredicateTemplateId::MetricThreshold],
    };
}
