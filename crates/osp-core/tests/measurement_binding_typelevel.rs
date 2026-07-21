//! INV-T9 #70 Commit 4b Faz 3 — Measurement binding non-forgeability type-level tests.
//!
//! **Reviewer v6 P2-4/P2-5:** Non-forgeability Faz 3'te (privacy assertion + trybuild
//! compile-fail guard). Faz 9 AST call-count + Faz 10 type-level suite ayrı seviyelerde.
//!
//! ## Evidence ayrımı (reviewer v6 P2-5 — net pinli)
//!
//! `VerifiedTaskMeasurementBinding` non-forgeability multi-layer:
//!
//! 1. **Rust module privacy (engine.rs sibling):** constructor `fn new` private —
//!    osp-core içindeki diğer modüller (navigator, measurement, authorization) doğrudan
//!    çağıramaz. Sadece `engine` modülü içindeki `verify_measurement_binding` çağırır.
//!    Bu testte KANITLANAMAZ — external crate tipin kendisine erişemez (`pub(crate)`).
//!
//! 2. **Trybuild (crate-dışı erişim engelli):** BU test. External crate
//!    `osp_core::engine::VerifiedTaskMeasurementBinding` tipini adıyla kullanamaz.
//!
//! 3. **Faz 9 AST call-count:** `engine_measurement_single_producer.rs` — production
//!    non-test code'da `VerifiedMeasurementBinding::new` + outer proof construction
//!    call count pinli, enclosing function doğrulanır. (EngineMeasurement producer
//!    için — outer proof constructor için Faz 9 general suite'e genişletilecek.)
//!
//! 4. **Faz 10 trybuild type-suite:** genişletilmiş type-level regression suite.
//!
//! Bu harness sadece layer 2'yi (crate-dışı erişim engeli) doğrular. Layer 1 ve 3
//! farklı test seviyelerinde ele alınır.

#[test]
fn measurement_binding_compile_fail_boundaries() {
    let t = trybuild::TestCases::new();
    // External crate VerifiedTaskMeasurementBinding tipini göremez (pub(crate)).
    t.compile_fail("tests/compile_fail/verified_task_measurement_binding_external.rs");
}
