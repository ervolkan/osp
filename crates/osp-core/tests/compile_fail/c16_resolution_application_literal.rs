// INV-C16 compile-fail (PR E): external crate ResolutionApplication literal construct edemez.
// Field'lar private → harici `ResolutionApplication { ... }` engelli.
// Sadece `CodeEntityResolutionSession::resolve` opaque application üretir (pub(crate) new).
// "Resolution application almak = session'dan geçmiş olmak."
use osp_core::anchoring::review::ResolutionApplication;
use osp_core::anchoring::ConceptNodeId;

fn main() {
    // Bu satır derlenmemeli: field'lar private.
    let _app = ResolutionApplication {
        candidate_id: ConceptNodeId("CodeEntityCandidate:X".into()),
        ..unimplemented!()
    };
}
