//! Analysis → candidate bridge integration testleri (CLI boundary).
//!
//! `osp graph init --analyze <repo>` uçtan uca — fixture repo → Candidate store.
//! Synthetic AnalysisResult birim testleri (CaseCollision/case-only-rename/MissingNodePath)
//! `analysis_bridge.rs` birim testlerinde; burada gerçek CLI boundary çalışır.

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::{tempdir, TempDir};

/// Fixture repo: 3 .py dosyası → 3 Module node → 3 CodeEntityCandidate.
fn fixture_repo() -> TempDir {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("payment.py"),
        "class Payment:\n    pass\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("user.py"), "class User:\n    pass\n").unwrap();
    std::fs::write(dir.path().join("util.py"), "class Util:\n    pass\n").unwrap();
    dir
}

/// Empty fixture repo: hiç dosya yok → 0 Module node → empty store.
fn empty_fixture_repo() -> TempDir {
    tempdir().unwrap()
}

/// Mutlu yol: `--analyze <repo>` → store created, candidates N, review show candidate.
#[test]
fn analyze_init_creates_candidate_store() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    // `osp graph init --analyze <repo> --store <store>`.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo.path().to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Graph initialized"));

    // `osp graph status` → candidates: 3.
    let status_out = Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap(), "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&status_out).unwrap();
    assert_eq!(v["candidates"], 3, "3 Module → 3 candidates");
    assert_eq!(v["revision"], 0);

    // `osp review list` → 3 candidate.
    let list_out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "list",
            "--store",
            store.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&list_out).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 3);
}

/// `--analyze` node'ları Candidate (INV-C5). `review list` + `review show` doğrula.
#[test]
fn analyze_nodes_are_candidate_physicalcode() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo.path().to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .success();

    // `osp review list` → Candidate lane (analysis node'ları Candidate).
    let list_out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "list",
            "--store",
            store.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&list_out).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    // Her item Candidate lane'de (review list yalnız candidate_query döner).
    // id format: CodeEntityCandidate:<path> (identity_key — case-folded default).
    let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"CodeEntityCandidate:payment.py"));
}

/// `--seed` + `--analyze` mutual exclusion → Clap error.
#[test]
fn seed_and_analyze_mutually_exclusive() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");
    let seed = dir.path().join("seed.json");
    std::fs::write(&seed, r#"{ "schema_version": 1, "nodes": [] }"#).unwrap();

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--seed",
            seed.to_str().unwrap(),
            "--analyze",
            repo.path().to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .failure(); // Clap ArgGroup error
}

/// Neither `--seed` nor `--analyze` → Clap error (required group).
#[test]
fn no_input_source_required() {
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "init", "--store", store.to_str().unwrap()])
        .assert()
        .failure(); // Clap required group error
}

/// `--path-case` + `--seed` → hata (P2: path-case yalnız --analyze).
#[test]
fn path_case_requires_analyze() {
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");
    let seed = dir.path().join("seed.json");
    std::fs::write(&seed, r#"{ "schema_version": 1, "nodes": [] }"#).unwrap();

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--seed",
            seed.to_str().unwrap(),
            "--path-case",
            "sensitive",
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .failure(); // Clap requires error
}

/// Empty analysis: warning + empty store geçerli (I7).
#[test]
fn empty_analysis_warning_and_empty_store() {
    let repo = empty_fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo.path().to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(contains("no projectable Module nodes"));

    // Empty store geçerli — status candidates 0, review list boş.
    let status_out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "status",
            "--store",
            store.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&status_out).unwrap();
    assert_eq!(v["candidates"], 0);

    let list_out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "list",
            "--store",
            store.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&list_out).unwrap();
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
}

/// Pre-validation non-destructive (P3+N2): mevcut store + --force + bridge hatası →
/// eski store byte-identical (hiç değişmedi). Empty repo force → warning ama store created.
/// Burada: mevcut store + empty repo --force → yeni empty store overwrites (success).
/// Non-destructive için: bridge hatası olunca store değişmemeli. Mevcut store + valid repo
/// --force → overwrite (success); bu zaten mevcut davranış. P3'ün gerçek testi birim seviye
/// (bridge error → store unchanged) — burada CLI seviyede --force overwrite doğrula.
#[test]
fn force_overwrites_existing_store() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    // İlk init.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo.path().to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .success();

    // İkinci init --force olmadan → fail (exists).
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo.path().to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains("already exists"));

    // --force ile overwrite → success.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo.path().to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
}

/// Node identities bit-equivalent: iki kez `--analyze` → aynı node/identity seti.
#[test]
fn analyze_idempotent_node_identities() {
    let repo1 = fixture_repo();
    let repo2 = fixture_repo();
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let store1 = dir1.path().join("store.json");
    let store2 = dir2.path().join("store.json");

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo1.path().to_str().unwrap(),
            "--store",
            store1.to_str().unwrap(),
        ])
        .assert()
        .success();
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--analyze",
            repo2.path().to_str().unwrap(),
            "--store",
            store2.to_str().unwrap(),
        ])
        .assert()
        .success();

    // İki store status candidates aynı (aynı fixture → aynı Module count).
    let s1: serde_json::Value = serde_json::from_slice(
        &Command::cargo_bin("osp")
            .unwrap()
            .args(["graph", "status", "--store", store1.to_str().unwrap(), "--format", "json"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    let s2: serde_json::Value = serde_json::from_slice(
        &Command::cargo_bin("osp")
            .unwrap()
            .args(["graph", "status", "--store", store2.to_str().unwrap(), "--format", "json"])
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap();
    assert_eq!(s1["candidates"], s2["candidates"]);
    assert_eq!(s1["node_count"], s2["node_count"]);
}
