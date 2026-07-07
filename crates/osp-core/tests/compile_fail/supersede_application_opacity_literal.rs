// Faz 8b opacity boundary: SupersedeApplication struct literal ile üretemez.
// Private fields + pub(crate) ctor — token dışarı çıkmaz, session çıkar (PR #50).
// (C13-paralel opaklık; C15 runtime semantiğini değil construction boundary'yi korur.)
use osp_core::anchoring::review::SupersedeApplication;

fn main() {
    // Bu satır derlenmemeli: private fields literal ile erişilebilir değil.
    let _app = SupersedeApplication {
        superseded: osp_core::anchoring::types::ConceptNodeId("x".into()),
        successor: osp_core::anchoring::types::ConceptNodeId("y".into()),
    };
}
