//! Architecture guards — source-scan invariants (lokal + CI).
//!
//! C2+N1: metric_projection.rs modülü core'un tam evidence/vector tiplerini
//! ÜRETMEZ. Bu guard substring kontrolü ile bunu mekanik olarak doğrular.
//! metric_projection.rs doc disiplini (N1): bu adlar yorumda bile geçmez.

/// C2+N1: metric_projection.rs complete core evidence/vector construction YOK.
#[test]
fn metric_projection_does_not_construct_complete_core_evidence() {
    let src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/metric_projection.rs"
    ));
    assert!(
        !src.contains("ObservedCodeEvidence"),
        "metric projection must not construct complete core evidence"
    );
    assert!(
        !src.contains("PhysicalCodeVector"),
        "metric projection must not construct a complete physical vector"
    );
}
