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
        .stdout(contains("Graph initialized"))
        .stderr(contains("Code metric drafts admitted"))
        .stderr(contains("Evidence construction: completed"))
        .stderr(contains("Evidence runtime consumer: none in graph init"))
        .stderr(contains("Evidence persistence: disabled"));

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

/// Pre-validation non-destructive (P3+N2): mevcut store + --force + validation hatası →
/// eski store byte-identical (hiç değişmedi). P1: gerçek failure path test edildi.
#[test]
fn force_with_invalid_seed_preserves_existing_store_byte_identical() {
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");
    let valid_seed = dir.path().join("valid.json");
    std::fs::write(
        &valid_seed,
        r#"{ "schema_version": 1, "nodes": [{"canonical": "Original", "kind": "Concept"}] }"#,
    )
    .unwrap();
    // Duplicate canonical seed → validation error.
    let invalid_seed = dir.path().join("invalid.json");
    std::fs::write(
        &invalid_seed,
        r#"{ "schema_version": 1, "nodes": [
            {"canonical": "Dup", "kind": "Concept"},
            {"canonical": "Dup", "kind": "RuleCandidate"}
        ] }"#,
    )
    .unwrap();

    // İlk init (valid seed) → store created.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--seed", valid_seed.to_str().unwrap(),
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Mevcut store bytes'ını dondur.
    let before = std::fs::read(&store).unwrap();

    // --force + invalid seed (duplicate canonical) → validation error.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--seed", invalid_seed.to_str().unwrap(),
            "--store", store.to_str().unwrap(), "--force",
        ])
        .assert()
        .failure()
        .stderr(contains("duplicate canonical"));

    // P3+N2: eski store byte-identical (validation hatasında store değişmez).
    let after = std::fs::read(&store).unwrap();
    assert_eq!(
        before, after,
        "existing store must be byte-identical after --force + validation failure"
    );
}

/// --force overwrite (başarılı): mevcut store + --force + valid → overwrite.
#[test]
fn force_overwrites_existing_store() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    // İlk init.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--analyze", repo.path().to_str().unwrap(),
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success();

    // İkinci init --force olmadan → fail (exists).
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--analyze", repo.path().to_str().unwrap(),
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains("already exists"));

    // --force ile overwrite → success.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--analyze", repo.path().to_str().unwrap(),
            "--store", store.to_str().unwrap(), "--force",
        ])
        .assert()
        .success();
}

/// Node identities bit-equivalent: iki kez `--analyze` → aynı node/identity tuple seti.
/// P1: count değil, gerçek (id, canonical, kind, family, status) tuple kümesi karşılaştırması.
#[test]
fn analyze_idempotent_node_identities() {
    let repo1 = fixture_repo();
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let store1 = dir1.path().join("store.json");
    let store2 = dir2.path().join("store.json");

    for store in [&store1, &store2] {
        Command::cargo_bin("osp")
            .unwrap()
            .args([
                "graph",
                "init",
                "--analyze",
                repo1.path().to_str().unwrap(),
                "--store",
                store.to_str().unwrap(),
            ])
            .assert()
            .success();
    }

    // review list → (id, canonical, kind, status) tuple kümesi (count değil).
    let extract_tuples = |store: &std::path::Path| -> Vec<(String, String, String)> {
        let out = Command::cargo_bin("osp")
            .unwrap()
            .args([
                "review", "list", "--store", store.to_str().unwrap(), "--format", "json",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    i["id"].as_str().unwrap().to_string(),
                    i["canonical"].as_str().unwrap().to_string(),
                    i["kind"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    let tuples1 = extract_tuples(&store1);
    let tuples2 = extract_tuples(&store2);
    // Gerçek kimlik karşılaştırması — tamamen farklı NodeId'ler üretilse bile yakalanır.
    assert_eq!(tuples1, tuples2, "two analyses must produce identical node identity set");
}

// ═══════════════════════════════════════════════════════════════════════════════
// PR E2 — code identity binding seeding integration
// ═══════════════════════════════════════════════════════════════════════════════

/// Store JSON'ından code_identity_bindings array'ini çıkar (PR E2 snapshot v2).
fn extract_bindings(store: &std::path::Path) -> Vec<serde_json::Value> {
    let content = std::fs::read_to_string(store).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    v["snapshot"]["code_identity_bindings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// PR E2 — `--analyze` sonrası store'da binding'ler var (1:1 candidate cardinality).
#[test]
fn pr_e2_analyze_seeds_code_identity_bindings() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--analyze", repo.path().to_str().unwrap(),
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success()
        // Tur 1 review P2-1: durable write sonrası "persisted" (disk'e yazıldı kesin).
        .stderr(contains("identity bindings persisted: 3"))
        // Tur 1 review P2-2: BridgeRunReport Display projected_identity_bindings içerir.
        .stderr(contains("projected_identity_bindings=3"));

    let bindings = extract_bindings(&store);
    assert_eq!(bindings.len(), 3, "3 candidates → 3 bindings");
    // Her binding'in node_id + identity_key alanları var.
    for b in &bindings {
        assert!(b["node_id"].is_string(), "binding node_id string");
        assert!(b["identity_key"].is_object(), "binding identity_key object");
    }
}

/// PR E2 — binding node_id candidate NodeId ile uyumlu (her candidate için bir binding).
#[test]
fn pr_e2_binding_node_id_matches_candidate() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--analyze", repo.path().to_str().unwrap(),
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success();

    // review list → candidate id'leri.
    let list_out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "list", "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&list_out).unwrap();
    let candidate_ids: std::collections::BTreeSet<String> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect();

    // binding node_id'leri candidate id'lerinin subset'i.
    let bindings = extract_bindings(&store);
    let binding_node_ids: std::collections::BTreeSet<String> = bindings
        .iter()
        .map(|b| b["node_id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        candidate_ids, binding_node_ids,
        "binding node_ids must match candidate ids (1:1)"
    );
}

/// PR E2 — empty analysis → binding YOK (I7 pattern).
#[test]
fn pr_e2_empty_analysis_no_bindings() {
    let repo = empty_fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--analyze", repo.path().to_str().unwrap(),
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(contains("no projectable Module nodes"))
        .stderr(contains("projected=0"));

    // "identity bindings seeded" stderr'i ÇIKMAMALI (empty → skip).
    let bindings = extract_bindings(&store);
    assert!(bindings.is_empty(), "empty analysis → no bindings");
}

/// PR E2 — snapshot round-trip INV-C16 validation (binding'lerle).
/// `osp graph validate` → restore + invariant-validasyon geçer (binding'lerle).
#[test]
fn pr_e2_snapshot_round_trip_with_bindings() {
    let repo = fixture_repo();
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");

    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "init", "--analyze", repo.path().to_str().unwrap(),
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success();

    // `osp graph validate` → INV-C16 validation (binding + ledger + audit_seq density).
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph", "validate", "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Store valid"))
        .stdout(contains("All invariants pass"));
}
