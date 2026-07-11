//! Architecture guards — source-scan invariants (lokal + CI).
//!
//! İki guard:
//! 1. **metric_projection.rs draft boundary** (C2+N1): core'un tam evidence/vector tiplerini
//!    ÜRETMEZ. Substring kontrolü ile doğrulanır.
//! 2. **evidence_projection.rs ownership** (PR D): core evidence construction token'ları
//!    (`ObservedPhysicalMetric::new`, `ObservedPhysicalMetrics::try_new`,
//!    `ObservedCodeEvidence::new`) yalnız `src/evidence_projection.rs`'de bulunabilir.

use std::path::{Path, PathBuf};

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

/// PR D ownership: core evidence construction yalnız evidence_projection.rs'de.
///
/// Tüm CLI production source dosyalarını (`src/**/*.rs`) tara; şu token'lar yalnız
/// `src/evidence_projection.rs`'de bulunabilir:
/// - `ObservedPhysicalMetric::new`
/// - `ObservedPhysicalMetrics::try_new`
/// - `ObservedCodeEvidence::new`
///
/// Bu guard alias/helper ile aşılabilir ama ownership iddiasını doğrudan ifade eder.
/// Test modülleri aynı source dosyasında bulunduğundan authorized `evidence_projection.rs`
/// içindeki token'lar doğal olarak kabul edilir.
#[test]
fn core_evidence_construction_owned_by_evidence_projection() {
    let authorized = "evidence_projection.rs";
    let forbidden_tokens = [
        "ObservedPhysicalMetric::new",
        "ObservedPhysicalMetrics::try_new",
        "ObservedCodeEvidence::new",
    ];

    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&src_dir, &mut files);

    for file in &files {
        let rel = file.strip_prefix(&src_dir).unwrap_or(file);
        let is_authorized = rel.to_string_lossy().ends_with(authorized);
        let src = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));
        for token in &forbidden_tokens {
            if src.contains(token) {
                assert!(
                    is_authorized,
                    "core evidence construction token `{token}` found in {} — only `{authorized}` may construct core evidence",
                    rel.display()
                );
            }
        }
    }
}

/// `src/**/*.rs` dosyalarını recursive topla (std::fs — yeni dep YOK).
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}
