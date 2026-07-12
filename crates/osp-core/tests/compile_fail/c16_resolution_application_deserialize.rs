// INV-C16 compile-fail (PR E, serde boundary): ResolutionApplication Deserialize YOK.
// Private field'lar serde ile reconstruct edilemesin (resolution bypass engeli).
// "Diskten resolution application reconstruct edip INV-C16 boundary'yi bypass etmek imkansız."
use osp_core::anchoring::review::ResolutionApplication;

fn main() {
    // Bu satır derlenmemeli: ResolutionApplication Deserialize impl'i yok.
    let _: ResolutionApplication = serde_json::from_str("{}").unwrap();
}
