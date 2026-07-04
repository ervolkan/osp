// INV-P3 compile-fail (serde boundary): CrossFamilyHint Deserialize YOK.
// Translation metadata yeniden apply edilemesin (INV-P3 boundary).
// Serialize-only (audit); trusted restore PR30/Faz4/5a/5b paternini izler.
use osp_core::anchoring::CrossFamilyHint;

fn main() {
    // Bu satır derlenmemeli: CrossFamilyHint Deserialize impl'i yok.
    let _hint: CrossFamilyHint = serde_json::from_str("{}").unwrap();
}
