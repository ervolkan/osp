// INV-C13 compile-fail: external crate DecisionApplication literal construct edemez.
// Field'lar private + constructor YOK (in-crate only). Tek üretici: OperatorReviewSession.
// "Token içeride kalır; trait dışarıda kalır; kapı yalnız session'dan geçer."
use osp_core::anchoring::review::DecisionApplication;
use osp_core::anchoring::ConceptNodeId;

fn main() {
    // Bu satır derlenmemeli: field'lar private.
    let _app = DecisionApplication {
        candidate_id: ConceptNodeId("RuleCandidate:X".into()),
    };
}
