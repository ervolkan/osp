// INV-P2 compile-fail (serde boundary): ExecutablePredicateSet Deserialize YOK.
// Slot'ları bağlanmış predicate set yeniden apply edilemesin (INV-P1b boundary).
// Serialize-only (audit); trusted restore PR30/Faz4/5a paternini izler.
use osp_core::anchoring::ExecutablePredicateSet;

fn main() {
    // Bu satır derlenmemeli: ExecutablePredicateSet Deserialize impl'i yok.
    let _eps: ExecutablePredicateSet = serde_json::from_str("{}").unwrap();
}
