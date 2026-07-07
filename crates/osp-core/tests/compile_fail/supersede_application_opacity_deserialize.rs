// Faz 8b opacity boundary: SupersedeApplication Deserialize YOK — yeniden apply edilememeli.
// "Bir supersede application'ı diskten okunup yeniden apply edilemez; forgery-enabled."
// Serde boundary pattern (DecisionApplication / PresentedBasis ile aynı).
use osp_core::anchoring::review::SupersedeApplication;

fn main() {
    // Bu satır derlenmemeli: SupersedeApplication Deserialize impl'lemiyor.
    let _: SupersedeApplication = serde_json::from_str("{}").unwrap();
}
