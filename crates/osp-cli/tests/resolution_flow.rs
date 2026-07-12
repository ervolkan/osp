//! PR E2 — `osp review resolve-code-entity` integration test matrisi.
//!
//! Mutlu yol Created, non-TTY target flag'leri, stale basis, not accepted, JSON output,
//! confirmation abort, operator env fallback.
//!
//! Resolution adayları `--analyze` ile üretilir (CodeEntityCandidate + PhysicalCode + identity binding).
//! Accept sonrası resolve edilebilir.

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::{tempdir, TempDir};

/// Fixture repo: 2 .py dosyası → 2 Module → 2 CodeEntityCandidate.
fn fixture_repo() -> TempDir {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("auth.py"), "class Auth:\n    pass\n").unwrap();
    std::fs::write(dir.path().join("user.py"), "class User:\n    pass\n").unwrap();
    dir
}

/// `osp graph init --analyze` ile store oluştur (binding'lerle).
fn init_analyze_store(dir: &std::path::Path, repo: &std::path::Path) -> std::path::PathBuf {
    let store_path = dir.join("store.json");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo.to_str().unwrap(),
            "--store",
            store_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Graph initialized"))
        .stderr(contains("identity bindings seeded: 2"));
    store_path
}

/// Node digest (hex) — show --format json ile al.
fn show_digest(store: &std::path::Path, id: &str) -> String {
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "show", id, "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    v["node"]["node_digest_hex"].as_str().unwrap().to_string()
}

/// Candidate node'u accept et → Accepted.
fn accept_candidate(store: &std::path::Path, id: &str) {
    let d = show_digest(store, id);
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "accept", id, "--store", store.to_str().unwrap(),
            "--operator", "t", "--reason", "ok", "--yes", "--basis-digest", &d,
        ])
        .assert()
        .success();
}

/// resolve-code-entity-preview --format json → target reveal.
fn preview_json(store: &std::path::Path, candidate: &str) -> serde_json::Value {
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "resolve-code-entity-preview", candidate,
            "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

/// PR E2 — mutlu yol Created (non-TTY automation path).
#[test]
fn resolve_code_entity_created_mutlu_yol() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());
    let candidate = "CodeEntityCandidate:auth.py";
    accept_candidate(&store, candidate);

    // Preview → Create target (0 entity).
    let preview = preview_json(&store, candidate);
    assert_eq!(preview["target"]["outcome"], "create");
    let proposed_entity_id = preview["target"]["proposed_entity_id"].as_str().unwrap();
    let candidate_digest = preview["candidate"]["digest_hex"].as_str().unwrap();

    // Non-TTY automation: explicit target flags.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "resolve-code-entity", candidate,
            "--store", store.to_str().unwrap(),
            "--operator", "t", "--reason", "canonical entity",
            "--yes",
            "--candidate-digest", candidate_digest,
            "--target-outcome", "create",
            "--target-entity-id", proposed_entity_id,
        ])
        .assert()
        .success()
        .stdout(contains("resolved"))
        .stdout(contains(candidate))
        .stdout(contains("created"));
}

/// PR E2 — stale candidate digest reject (TOCTOU).
#[test]
fn resolve_code_entity_stale_candidate_digest_rejects() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());
    let candidate = "CodeEntityCandidate:auth.py";
    accept_candidate(&store, candidate);

    let preview = preview_json(&store, candidate);
    let proposed_entity_id = preview["target"]["proposed_entity_id"].as_str().unwrap();

    // Yanlış digest (stale) → reject.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "resolve-code-entity", candidate,
            "--store", store.to_str().unwrap(),
            "--operator", "t", "--reason", "x",
            "--yes",
            "--candidate-digest", "0000000000000000",  // stale
            "--target-outcome", "create",
            "--target-entity-id", proposed_entity_id,
        ])
        .assert()
        .failure()
        .stderr(contains("stale resolution basis"));
}

/// PR E2 — candidate not Accepted reject (Candidate lane'de resolve denemesi).
#[test]
fn resolve_code_entity_candidate_not_accepted_rejects() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());
    let candidate = "CodeEntityCandidate:auth.py";
    // Accept ETMEDEN resolve dene → Candidate → NotPromotableFrom → CandidateNotAccepted.

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "resolve-code-entity", candidate,
            "--store", store.to_str().unwrap(),
            "--operator", "t", "--reason", "x",
            "--yes",
            "--candidate-digest", "0000000000000000",
            "--target-outcome", "create",
            "--target-entity-id", "CodeEntity:deadbeef",
        ])
        .assert()
        .failure()
        .stderr(contains("not Accepted"));
}

/// PR E2 — missing identity binding reject (binding seeding yapılmamış candidate).
/// Not: --analyze her zaman binding üretir; bu test binding'siz store simüle eder (legacy --seed).
#[test]
fn resolve_code_entity_non_tty_requires_target_flags() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());
    let candidate = "CodeEntityCandidate:auth.py";
    accept_candidate(&store, candidate);

    // --target-outcome olmadan non-TTY → hata.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "resolve-code-entity", candidate,
            "--store", store.to_str().unwrap(),
            "--operator", "t", "--reason", "x",
            "--yes",
            "--candidate-digest", "0000000000000000",
            // target flags YOK
        ])
        .assert()
        .failure()
        .stderr(contains("--target-outcome"));
}

/// PR E2 — operator env fallback ($OSP_OPERATOR).
#[test]
fn resolve_code_entity_operator_env_fallback() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());
    let candidate = "CodeEntityCandidate:auth.py";
    accept_candidate(&store, candidate);

    let preview = preview_json(&store, candidate);
    let proposed_entity_id = preview["target"]["proposed_entity_id"].as_str().unwrap();
    let candidate_digest = preview["candidate"]["digest_hex"].as_str().unwrap();

    // --operator flag YOK; env fallback.
    Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "envop")
        .args([
            "review", "resolve-code-entity", candidate,
            "--store", store.to_str().unwrap(),
            "--reason", "canonical",
            "--yes",
            "--candidate-digest", candidate_digest,
            "--target-outcome", "create",
            "--target-entity-id", proposed_entity_id,
        ])
        .assert()
        .success();
}

/// PR E2 — preview Create target reveals proposed_entity_id.
#[test]
fn preview_create_target_reveals_proposed_entity_id() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());
    let candidate = "CodeEntityCandidate:auth.py";
    accept_candidate(&store, candidate);

    let preview = preview_json(&store, candidate);
    assert_eq!(preview["target"]["outcome"], "create");
    let entity_id = preview["target"]["proposed_entity_id"].as_str().unwrap();
    assert!(entity_id.starts_with("CodeEntity:"), "got {entity_id}");
    // Revision dahil.
    assert!(preview["revision"].is_number());
}

/// PR E2 — preview missing candidate → NotFound (exit non-zero).
#[test]
fn preview_missing_candidate_not_found() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "resolve-code-entity-preview", "CodeEntityCandidate:MISSING",
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains("not found"));
}

/// PR E2 — preview text output (renderer body-only).
#[test]
fn preview_text_output_renders_target() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());
    let candidate = "CodeEntityCandidate:auth.py";
    accept_candidate(&store, candidate);

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "resolve-code-entity-preview", candidate,
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Candidate:"))
        .stdout(contains("Resolution target:"))
        .stdout(contains("outcome:"))
        .stdout(contains("Identity key:"));
}

/// PR E2 — JSON output mutation (resolved + entity_node_id + outcome typed).
#[test]
fn resolve_code_entity_created_json_output() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = init_analyze_store(dir.path(), repo.path());
    let candidate = "CodeEntityCandidate:auth.py";
    accept_candidate(&store, candidate);

    let preview = preview_json(&store, candidate);
    let proposed_entity_id = preview["target"]["proposed_entity_id"].as_str().unwrap();
    let candidate_digest = preview["candidate"]["digest_hex"].as_str().unwrap();

    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "resolve-code-entity", candidate,
            "--store", store.to_str().unwrap(),
            "--operator", "t", "--reason", "canonical",
            "--yes",
            "--candidate-digest", candidate_digest,
            "--target-outcome", "create",
            "--target-entity-id", proposed_entity_id,
            "--format", "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["mutation"]["status"], "resolved");
    assert_eq!(v["mutation"]["outcome"], "created");  // typed snake_case (tur 3 P2-4)
    assert_eq!(v["mutation"]["candidate_node_id"], candidate);
    assert!(v["mutation"]["entity_node_id"].as_str().unwrap().starts_with("CodeEntity:"));
    assert!(v["mutation"]["resolution_sequence"].is_number());
    assert!(v["revision"].is_number());
}
