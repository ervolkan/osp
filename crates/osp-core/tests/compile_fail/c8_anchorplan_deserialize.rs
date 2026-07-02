// INV-C8 compile-fail (Faz 3 persistence boundary): AnchorPlan Deserialize YOK.
// "AnchorPlan almak = canon gate'ten geçmiş olmak" — serde ile reconstruct engelli.
// DB read için PersistedAnchorPlanAudit (apply edilemez audit record).
use osp_core::anchoring::types::AnchorPlan;

fn main() {
    // Bu satır derlenmemeli: AnchorPlan Deserialize impl'i yok.
    let _plan: AnchorPlan = serde_json::from_str("{}").unwrap();
}
