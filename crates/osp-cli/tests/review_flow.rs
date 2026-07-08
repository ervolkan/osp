//! CLI integration test'leri — `osp graph` + `osp review` uçtan uca akışlar.
//!
//! `assert_cmd` ile `osp` binary'sini çalıştırır; real subprocess (unit test değil).
//! Kabul kriterleri (plan):
//! - graph init (Candidate-only, deny_unknown_fields)
//! - review list (deterministic)
//! - accept/reject (--basis-digest, confirmation)
//! - stale basis (digest mismatch → fail + store unchanged)
//! - restart-safe (process restart → state+ledger+audit_seq korunuyor)
//! - corrupt snapshot fail-closed
//! - canonical bit-identik (one-shot vs interactive same snapshot)
//! - unrelated revision (digest aynı → başarı)

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

/// `osp graph init` helper — seed yaz, store path dön (custom path).
fn init_store(dir: &std::path::Path, seed_json: &str) -> std::path::PathBuf {
    let seed_path = dir.join("seed.json");
    std::fs::write(&seed_path, seed_json).unwrap();
    let store_path = dir.join("store.json");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--seed",
            seed_path.to_str().unwrap(),
            "--store",
            store_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Graph initialized"));
    store_path
}

/// Default-path store init — `osp review` argümansız default `.osp/anchor-store.json`.
/// `work_dir` altında `.osp/anchor-store.json` kurar (graph init create_dir_all yapar).
fn init_default_store(work_dir: &std::path::Path, seed_json: &str) {
    let seed_path = work_dir.join("seed.json");
    std::fs::write(&seed_path, seed_json).unwrap();
    Command::cargo_bin("osp")
        .unwrap()
        .current_dir(work_dir)
        .args([
            "graph",
            "init",
            "--seed",
            "seed.json",
            "--store",
            ".osp/anchor-store.json",
        ])
        .assert()
        .success()
        .stdout(contains("Graph initialized"));
}

const TWO_CANDIDATES_SEED: &str = r#"{
    "schema_version": 1,
    "nodes": [
        {"canonical": "CouplingMustNot", "kind": "RuleCandidate"},
        {"canonical": "Payment", "kind": "Concept"}
    ]
}"#;

/// `osp graph init` → 2 candidate node yüklenir.
#[test]
fn graph_init_loads_candidate_nodes() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("candidates: 2"));
}

/// `osp graph init` existing store → fail (overwrite gerekmez).
#[test]
fn graph_init_existing_store_fails_without_force() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    let seed_path = dir.path().join("seed.json");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--seed",
            seed_path.to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains("already exists"));
}

/// Seed'de `decision_status` alanı → deny_unknown_fields reject.
#[test]
fn graph_init_rejects_seed_with_status_field() {
    let dir = tempdir().unwrap();
    let seed = r#"{ "schema_version": 1, "nodes": [{"canonical": "X", "kind": "RuleCandidate", "decision_status": "Accepted"}] }"#;
    let seed_path = dir.path().join("seed.json");
    std::fs::write(&seed_path, seed).unwrap();
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--seed",
            seed_path.to_str().unwrap(),
            "--store",
            dir.path().join("s.json").to_str().unwrap(),
        ])
        .assert()
        .failure();
}

/// `osp review list` → deterministic sıralı (NodeId ascending).
#[test]
fn review_list_shows_candidates() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .args(["review", "list", "--store", store.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("Concept:Payment"))
        .stdout(contains("RuleCandidate:CouplingMustNot"));
}

/// `osp review accept` non-interactive (--yes --basis-digest) → Accepted + revision increment.
#[test]
fn review_accept_noninteractive_succeeds() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);

    // Digest al.
    let show_output = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "show",
            "RuleCandidate:CouplingMustNot",
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
    let show_json: serde_json::Value = serde_json::from_slice(&show_output).unwrap();
    let digest_hex = show_json["node"]["basis_digest_hex"].as_str().unwrap();

    // Accept.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "accept",
            "RuleCandidate:CouplingMustNot",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "tester",
            "--reason",
            "approved threshold rule",
            "--yes",
            "--basis-digest",
            digest_hex,
        ])
        .assert()
        .success()
        .stdout(contains("Accepted"));

    // Revision 0 → 1.
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 1"))
        .stdout(contains("candidates: 1"))
        .stdout(contains("accepted: 1"));
}

/// Stale basis: yanlış digest → fail + store unchanged (revision same).
#[test]
fn review_accept_stale_digest_rejected_store_unchanged() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);

    // Yanlış digest ile accept → fail.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "accept",
            "RuleCandidate:CouplingMustNot",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "tester",
            "--reason",
            "stale attempt",
            "--yes",
            "--basis-digest",
            "0000000000000000",
        ])
        .assert()
        .failure()
        .stderr(contains("stale basis"));

    // Store unchanged — revision hala 0.
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 0"))
        .stdout(contains("candidates: 2"));
}

/// Restart-safe: accept sonrası yeni process status → decision record + audit_seq korunuyor.
#[test]
fn review_state_survives_process_restart() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);

    // Accept (ilk process).
    let digest = {
        let out = Command::cargo_bin("osp")
            .unwrap()
            .args([
                "review",
                "show",
                "RuleCandidate:CouplingMustNot",
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
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        v["node"]["basis_digest_hex"].as_str().unwrap().to_string()
    };
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "accept",
            "RuleCandidate:CouplingMustNot",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "tester",
            "--reason",
            "ok",
            "--yes",
            "--basis-digest",
            &digest,
        ])
        .assert()
        .success();

    // Yeni process status → state korunuyor.
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 1"))
        .stdout(contains("Decision records: 1"))
        .stdout(contains("Audit sequence: 1"));
}

/// `osp review accept` `--operator` olmadan → fail (generic "operator" default yok).
#[test]
fn review_accept_requires_operator_identity() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .env_remove("OSP_OPERATOR")
        .args([
            "review",
            "accept",
            "RuleCandidate:CouplingMustNot",
            "--store",
            store.to_str().unwrap(),
            "--reason",
            "no operator",
            "--yes",
            "--basis-digest",
            "0000000000000000",
        ])
        .assert()
        .failure()
        .stderr(contains("Operator identity is required"));
}

/// `osp graph validate` → valid store tüm invariant'lar geçer.
#[test]
fn graph_validate_passes_for_valid_store() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "validate", "--store", store.to_str().unwrap()])
        .assert()
        .success()
        .stdout(contains("Store valid"));
}

/// Corrupt snapshot → `osp graph validate` fail-closed.
#[test]
fn graph_validate_fails_for_corrupt_store() {
    let dir = tempdir().unwrap();
    let store = dir.path().join("store.json");
    // Geçersiz JSON yaz.
    std::fs::write(&store, "{ not valid json }").unwrap();
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "validate", "--store", store.to_str().unwrap()])
        .assert()
        .failure();
}

/// `osp graph status --format json` → structured output (CI/script).
#[test]
fn graph_status_json_output() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    let output = Command::cargo_bin("osp")
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
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(v["candidates"], 2);
    assert_eq!(v["revision"], 0);
}

/// Argümansız `osp review` (default store, working dir'de .osp/) → interactive session açar (Review P1.1).
/// Root --store flag YOK (sessiz yok sayılma riski — P1.1 düzeltme). Operator prompt.
#[test]
fn argumanliz_osp_review_opens_interactive_session() {
    let dir = tempdir().unwrap();
    // Default store path: working-dir/.osp/anchor-store.json
    init_default_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .env_remove("OSP_OPERATOR")
        .current_dir(dir.path())
        .arg("review")
        .write_stdin("tester\nquit\n")
        .assert()
        .success()
        .stdout(contains("Operator identity:"))
        .stdout(contains("OSP review session"));
}

/// Argümansız `osp review` OSP_OPERATOR set ise prompt sormaz (default store).
#[test]
fn argumanliz_osp_review_uses_env_operator() {
    let dir = tempdir().unwrap();
    init_default_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "env-op")
        .current_dir(dir.path())
        .arg("review")
        .write_stdin("quit\n")
        .assert()
        .success()
        .stdout(contains("operator: env-op"))
        .stdout(contains("candidates awaiting review"));
}

/// Interactive informed-acceptance: accept → basis göster → confirm(y) → reason → Accepted.
/// Review P1.2: operator basis'i GÖRDÜKTEN sonra karar verir. (default store)
#[test]
fn interactive_accept_shows_basis_before_confirmation() {
    let dir = tempdir().unwrap();
    init_default_store(dir.path(), TWO_CANDIDATES_SEED);
    // accept <id> → confirmation y → reason → quit.
    Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "tester")
        .current_dir(dir.path())
        .arg("review")
        .write_stdin("accept RuleCandidate:CouplingMustNot\ny\napproved rule\nquit\n")
        .assert()
        .success()
        .stdout(contains("this exact basis?"))
        .stdout(contains("Accepted"));
}

/// Interactive: confirmation 'n' → abort, mutation uygulanmaz. (default store)
#[test]
fn interactive_confirmation_n_aborts_mutation() {
    let dir = tempdir().unwrap();
    init_default_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "tester")
        .current_dir(dir.path())
        .arg("review")
        .write_stdin("accept RuleCandidate:CouplingMustNot\nn\nquit\n")
        .assert()
        .success()
        .stdout(contains("aborted by operator"));
    // Store unchanged — revision hala 0.
    Command::cargo_bin("osp")
        .unwrap()
        .current_dir(dir.path())
        .args(["graph", "status", "--store", ".osp/anchor-store.json"])
        .assert()
        .stdout(contains("Revision: 0"));
}

/// Boş operator kimliği reject edilir (Review 2.tur P2.3). `--operator ""` → fail.
#[test]
fn review_accept_rejects_empty_operator() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .env_remove("OSP_OPERATOR")
        .args([
            "review",
            "accept",
            "RuleCandidate:CouplingMustNot",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "   ",
            "--reason",
            "ok",
            "--yes",
            "--basis-digest",
            "0000000000000000",
        ])
        .assert()
        .failure()
        .stderr(contains("operator identity cannot be empty"));
}

/// `graph init` nested parent dizini oluşturur (Review 2.tur P2.2).
/// `.osp/` yoksa lock açılamadan fail ederdi; artık create_dir_all.
#[test]
fn graph_init_creates_nested_parent_directory() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("nonexisting").join("nested");
    let store = nested.join("anchor-store.json");
    let seed_path = dir.path().join("seed.json");
    std::fs::write(&seed_path, TWO_CANDIDATES_SEED).unwrap();
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "graph",
            "init",
            "--seed",
            seed_path.to_str().unwrap(),
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Graph initialized"));
    assert!(store.exists(), "nested store created");
}

/// Root `osp review --store X list` artık geçerli değil (root flag yok — Review 2.tur P1.1).
/// Root `osp review --store X` clap tarafından reject edilir (Review 2.tur P1.1 kesin kontrat).
/// Root flag yok → sessiz yok sayılma değil, explicit clap error. Subcommand'lar kendi --store taşır.
#[test]
fn review_root_store_flag_rejected_by_clap() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .args(["review", "--store", store.to_str().unwrap(), "list"])
        .assert()
        .failure()
        .stderr(contains("unexpected argument '--store'"));
}

/// `osp review session --store X` custom store ile interactive session açar (Review 2.tur P1.1).
/// Argümansız `osp review` default store; `session` subcommand custom store/operator.
#[test]
fn review_session_subcommand_uses_custom_store() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "alice")
        .args(["review", "session", "--store", store.to_str().unwrap()])
        .write_stdin("list\nquit\n")
        .assert()
        .success()
        .stdout(contains("2 candidates awaiting review."))
        .stdout(contains("Concept:Payment"));
}

/// `review list --format json` boş listede geçerli JSON üretir (Review 3.tur P2.1).
/// Otomasyon contract: "No candidates awaiting review" geçerli JSON değil.
#[test]
fn review_list_json_empty_produces_valid_json() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    // İki candidate'ı da accept et (non-interactive).
    for id in ["RuleCandidate:CouplingMustNot", "Concept:Payment"] {
        let digest = {
            let out = Command::cargo_bin("osp")
                .unwrap()
                .args([
                    "review",
                    "show",
                    id,
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
            let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
            v["node"]["basis_digest_hex"].as_str().unwrap().to_string()
        };
        Command::cargo_bin("osp")
            .unwrap()
            .args([
                "review",
                "accept",
                id,
                "--store",
                store.to_str().unwrap(),
                "--operator",
                "t",
                "--reason",
                "ok",
                "--yes",
                "--basis-digest",
                &digest,
            ])
            .assert()
            .success();
    }
    // Artık candidate yok → list --format json geçerli JSON (items: []).
    let output = Command::cargo_bin("osp")
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
    let v: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON on empty list");
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
    assert!(v["revision"].as_u64().is_some());
}

/// `review show --format json` revision alanını taşır (Review 3.tur P2.1).
#[test]
fn review_show_json_includes_revision() {
    let dir = tempdir().unwrap();
    let store = init_store(dir.path(), TWO_CANDIDATES_SEED);
    let output = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "show",
            "RuleCandidate:CouplingMustNot",
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
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(v["node"]["id"].as_str().is_some(), "node details present");
    assert!(v["revision"].as_u64().is_some(), "revision present in JSON");
}
