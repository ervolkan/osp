// INV-T9 #70 Commit 4b Faz 3 compile-fail: external crate `VerifiedTaskMeasurementBinding`
// oluşturamaz. Tip `pub(crate)` — osp-core dışından görünmez.
//
// Reviewer v6 P2-5: non-forgeability evidence ayrımı:
// - Rust module privacy (sibling engine.rs modülleri erişemez — `fn new` private):
//   bu testte KANITLANMAZ (external crate tipi göremez).
// - Trybuild (crate-dışı erişim engelli): BU test.
// - Faz 9 AST (production call-count == 1, caller == verify_measurement_binding):
//   engine_measurement_single_producer.rs AST guard.
// - Faz 10 (trybuild type-suite genişletme): ayrı seviye.
//
// Bu test: external crate `VerifiedTaskMeasurementBinding` tipini adıyla kullanamaz.
fn main() {
    // Bu satır derlenmemeli: tip pub(crate) — external crate'ten görünmez.
    let _proof: osp_core::engine::VerifiedTaskMeasurementBinding = unimplemented!();
}
