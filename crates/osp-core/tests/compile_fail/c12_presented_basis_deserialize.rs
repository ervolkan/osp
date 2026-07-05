// INV-C12 compile-fail: PresentedBasis Deserialize YOK — yeniden apply edilememeli.
// "Bir karar temeli diskten okunup yeniden apply edilemez; forgery-enabled."
// Serde boundary pattern (PR30/Faz4/5a/5.1 ile aynı).
use osp_core::anchoring::review::PresentedBasis;

fn main() {
    // Bu satır derlenmemeli: PresentedBasis Deserialize impl'lemiyor.
    let _: PresentedBasis = serde_json::from_str("{}").unwrap();
}
