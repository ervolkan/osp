// INV-P3 compile-fail: external crate CrossFamilyHint literal construct edemez.
// Private fields → struct literal engelli. Sadece smart constructor `new()` ile.
// "Translation preserves candidate meaning; binding alone creates commitment."
use osp_core::anchoring::{CrossFamilyHint, PhysicalCodeMetricAxis, PositionFamily};

fn main() {
    // Bu satır derlenmemeli: fields private.
    let _hint = CrossFamilyHint {
        from_family: PositionFamily::ConceptualIntent,
        to_family: PositionFamily::PhysicalCode,
        axis_candidates: vec![],
    };
}
