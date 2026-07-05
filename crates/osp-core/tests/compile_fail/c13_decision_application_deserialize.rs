// INV-C13 compile-fail: DecisionApplication Deserialize YOK — yeniden apply edilememeli.
// "Bir karar application'ı diskten okunup yeniden apply edilemez; forgery-enabled."
// Serde boundary pattern (AcceptedTaskCandidateRef / PresentedBasis ile aynı).
use osp_core::anchoring::review::DecisionApplication;

fn main() {
    // Bu satır derlenmemeli: DecisionApplication Deserialize impl'lemiyor.
    let _: DecisionApplication = serde_json::from_str("{}").unwrap();
}
