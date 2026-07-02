// INV-C3 compile-fail (Faz 3 persistence boundary): ConceptGraph Deserialize YOK.
// Private field'lar serde ile reconstruct edilemesin (Accepted node bypass engeli).
// Trusted restore için ConceptGraphSnapshot (ayrı tip) kullanılır.
use osp_core::anchoring::ConceptGraph;

fn main() {
    // Bu satır derlenmemeli: ConceptGraph Deserialize impl'i yok.
    let _graph: ConceptGraph = serde_json::from_str("{}").unwrap();
}
