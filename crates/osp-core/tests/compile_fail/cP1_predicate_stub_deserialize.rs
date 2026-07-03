// INV-P1 compile-fail (serde boundary): PredicateStub Deserialize YOK.
// Private field'lar serde ile reconstruct edilemesin (stub yeniden apply edilememeli).
// Serialize-only (audit); trusted restore PR30/Faz4 paternini izler (AnchorPlan/
// ConceptGraph/ObservedCodeEvidence). "Stub yeniden apply edilemez — INV-P1."
use osp_core::anchoring::PredicateStub;

fn main() {
    // Bu satır derlenmemeli: PredicateStub Deserialize impl'i yok.
    let _stub: PredicateStub = serde_json::from_str("{}").unwrap();
}
