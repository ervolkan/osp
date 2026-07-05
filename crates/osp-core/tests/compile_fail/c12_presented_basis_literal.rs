// INV-C12 compile-fail: external crate PresentedBasis literal construct edemez.
// Field'lar private → harici `PresentedBasis { ... }` engelli.
// Tek üretim yolu: `PresentedBasis::compile(store, id)` — store'dan derlenir.
// "Karar anında sunulan temel uydurulamaz; store'dan derlenir."
use osp_core::anchoring::review::PresentedBasis;
use osp_core::anchoring::ConceptNodeId;

fn main() {
    // Bu satır derlenmemeli: field'lar private.
    let _basis = PresentedBasis {
        candidate_id: ConceptNodeId("RuleCandidate:X".into()),
    };
}
