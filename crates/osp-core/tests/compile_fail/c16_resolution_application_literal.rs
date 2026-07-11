// INV-C16 compile-fail (PR E): external crate ResolutionApplication literal construct edemez.
// Field'lar private → harici `ResolutionApplication { ... }` engelli.
// Sadece `CodeEntityResolutionSession::resolve` opaque application üretir (pub(crate) new).
// "Resolution application almak = session'dan geçmiş olmak."
use osp_core::anchoring::review::{NonEmptyExplanation, PresentedResolutionBasis};
use osp_core::anchoring::SessionId;

fn main() {
    // Bu satır derlenmemeli: field'lar private.
    let _app = osp_core::anchoring::review::ResolutionApplication {
        candidate_id: osp_core::anchoring::ConceptNodeId("CodeEntityCandidate:X".into()),
        basis: presented_basis(),
        reason: NonEmptyExplanation::new("test").unwrap(),
        session_id: SessionId("s".into()),
        operator: osp_core::anchoring::review::OperatorId("op".into()),
        resolved_at: std::time::SystemTime::now(),
    };
}

/// Helper — bu fonksiyon derlense bile main() içindeki literal engellenmeli.
fn presented_basis() -> PresentedResolutionBasis {
    unimplemented!()
}
