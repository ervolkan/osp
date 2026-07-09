//! Rich supersede-preview integration test matrisi.
//!
//! `osp review supersede-preview <old> <new>` — read-only rich preview query. Standalone
//! ineligible dahil tüm durumlar exit 0 (başarılı query). Mutlu yol, incompatible, cycle,
//! lineage chain, non-current, missing, ineligible JSON, self-supersede.

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

/// `osp graph init` helper — seed yaz, store path dön.
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

/// Node digest (hex) — show --format json ile al.
fn show_digest(store: &std::path::Path, id: &str) -> String {
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
    v["node"]["node_digest_hex"].as_str().unwrap().to_string()
}

/// İki Accepted node'lu store (OldRule + NewRule) — supersede için hazır.
fn setup_two_accepted(dir: &std::path::Path) -> std::path::PathBuf {
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "OldRule", "kind": "RuleCandidate"},
        {"canonical": "NewRule", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir, seed);
    for id in ["RuleCandidate:OldRule", "RuleCandidate:NewRule"] {
        let d = show_digest(&store, id);
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
                &d,
            ])
            .assert()
            .success();
    }
    store
}

/// Committed supersede kur (preview test setup için).
fn supersede(store: &std::path::Path, superseded: &str, successor: &str) {
    let sup_d = show_digest(store, superseded);
    let suc_d = show_digest(store, successor);
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            superseded,
            successor,
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "ok",
            "--yes",
            "--superseded-digest",
            &sup_d,
            "--successor-digest",
            &suc_d,
        ])
        .assert()
        .success();
}

/// Mutlu yol: iki Accepted, eligible → text render + structurally_eligible yes + freshness tokens.
#[test]
fn supersede_preview_happy_path_text() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede-preview",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("Structurally eligible"), "expected eligibility: {out}");
    assert!(out.contains(": yes"), "expected yes: {out}");
    assert!(out.contains("Proposed committed edge"), "expected proposed edge: {out}");
    assert!(out.contains("NewRule --Supersedes--> RuleCandidate:OldRule"), "expected direction: {out}");
    // Freshness tokens emit edilir (eligible durumda).
    assert!(out.contains("--superseded-digest"), "expected freshness tokens: {out}");
}

/// Mutlu yol JSON — 8+ alan contract.
#[test]
fn supersede_preview_happy_path_json() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede-preview",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
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
    assert_eq!(v["structurally_eligible"], true);
    assert_eq!(v["primary_structural_blocker"], serde_json::Value::Null);
    assert_eq!(v["blocking_reasons"].as_array().unwrap().len(), 0);
    assert_eq!(v["proposed_edge"]["from"], "RuleCandidate:NewRule");
    assert_eq!(v["proposed_edge"]["to"], "RuleCandidate:OldRule");
    assert_eq!(v["proposed_edge"]["kind"], "Supersedes");
    assert!(v["compatibility"]["kind_compatible"].as_bool().unwrap());
    assert!(v["lineage"]["root"].as_str().unwrap() == "RuleCandidate:NewRule");
    assert!(v["superseded"]["node_digest_hex"].as_str().unwrap().len() == 16);
}

/// Incompatible endpoints — farklı kind (Rule vs Concept). structurally_eligible false.
#[test]
fn supersede_preview_incompatible_kind() {
    let dir = tempdir().unwrap();
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "OldRule", "kind": "RuleCandidate"},
        {"canonical": "NewConcept", "kind": "Concept"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    for id in ["RuleCandidate:OldRule", "Concept:NewConcept"] {
        let d = show_digest(&store, id);
        Command::cargo_bin("osp")
            .unwrap()
            .args([
                "review", "accept", id, "--store", store.to_str().unwrap(),
                "--operator", "t", "--reason", "ok", "--yes", "--basis-digest", &d,
            ])
            .assert()
            .success();
    }
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:OldRule", "Concept:NewConcept",
            "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success() // ineligible dahil exit 0
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["structurally_eligible"], false);
    assert_eq!(v["primary_structural_blocker"], "incompatible_kind");
    assert!(!v["compatibility"]["kind_compatible"].as_bool().unwrap());
}

/// Prospective cycle: A→B committed (supersede), sonra preview B→A → cycle_risk true.
#[test]
fn supersede_preview_reports_prospective_cycle() {
    let dir = tempdir().unwrap();
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "A", "kind": "RuleCandidate"},
        {"canonical": "B", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    for id in ["RuleCandidate:A", "RuleCandidate:B"] {
        let d = show_digest(&store, id);
        Command::cargo_bin("osp")
            .unwrap()
            .args([
                "review", "accept", id, "--store", store.to_str().unwrap(),
                "--operator", "t", "--reason", "ok", "--yes", "--basis-digest", &d,
            ])
            .assert()
            .success();
    }
    supersede(&store, "RuleCandidate:A", "RuleCandidate:B"); // committed B→A
    // Preview: supersede B, successor A → proposed A→B; B→A mevcut → cycle.
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:B", "RuleCandidate:A",
            "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["structurally_eligible"], false);
    assert_eq!(v["compatibility"]["cycle_risk"], true);
    let blockers = v["blocking_reasons"].as_array().unwrap();
    assert!(blockers.iter().any(|b| b["code"] == "cycle"));
}

/// Lineage chain: supersede(A, B) → B→A committed. Preview successor=B → lineage nodes [B, A].
#[test]
fn supersede_preview_lineage_chain() {
    let dir = tempdir().unwrap();
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "A", "kind": "RuleCandidate"},
        {"canonical": "B", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    for id in ["RuleCandidate:A", "RuleCandidate:B"] {
        let d = show_digest(&store, id);
        Command::cargo_bin("osp")
            .unwrap()
            .args([
                "review", "accept", id, "--store", store.to_str().unwrap(),
                "--operator", "t", "--reason", "ok", "--yes", "--basis-digest", &d,
            ])
            .assert()
            .success();
    }
    supersede(&store, "RuleCandidate:A", "RuleCandidate:B"); // B→A
    // Preview successor=B (outgoing chain [B, A]); superseded=A ineligible ama lineage gösterilir.
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:A", "RuleCandidate:B",
            "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["lineage"]["root"], "RuleCandidate:B");
    let nodes: Vec<String> = v["lineage"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(nodes, vec!["RuleCandidate:B", "RuleCandidate:A"]);
    // superseded_incoming accessor'dan beslenir (A zaten superseded).
    assert_eq!(v["lineage"]["superseded_incoming"], "RuleCandidate:B");
}

/// Missing endpoint → NotFound hard error (exit non-zero).
#[test]
fn supersede_preview_missing_endpoint_error() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:MISSING", "RuleCandidate:NewRule",
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains("not found"));
}

/// Self-supersede standalone → exit 0, SelfSupersede blocker, lineage üretilir.
#[test]
fn supersede_preview_self_supersede_eligible_query() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:OldRule", "RuleCandidate:OldRule",
            "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success() // ineligible dahil exit 0
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["structurally_eligible"], false);
    assert_eq!(v["primary_structural_blocker"], "self_supersede");
    assert_eq!(v["lineage"]["root"], "RuleCandidate:OldRule");
    assert!(!v["compatibility"]["cycle_risk"].as_bool().unwrap()); // cycle bastırılır
}

/// Non-Accepted endpoint → blocking_reason (hard error DEĞİL, exit 0).
#[test]
fn supersede_preview_non_accepted_blocker() {
    let dir = tempdir().unwrap();
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "Old", "kind": "RuleCandidate"},
        {"canonical": "New", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    // Yalnız New'i accept et; Old Candidate kalır.
    let d = show_digest(&store, "RuleCandidate:New");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "accept", "RuleCandidate:New", "--store", store.to_str().unwrap(),
            "--operator", "t", "--reason", "ok", "--yes", "--basis-digest", &d,
        ])
        .assert()
        .success();
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:Old", "RuleCandidate:New",
            "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success() // non-accepted = blocking_reason, exit 0
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["structurally_eligible"], false);
    assert_eq!(v["primary_structural_blocker"], "superseded_not_current");
}

/// Standalone ineligible query exit 0 + blocking_reasons array (otomasyon contract).
#[test]
fn supersede_preview_ineligible_exit_zero() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    // Self-supersede ineligible → exit 0 (başarılı query).
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:OldRule", "RuleCandidate:OldRule",
            "--store", store.to_str().unwrap(), "--format", "json",
        ])
        .assert()
        .success();
}

/// Wizard ineligible → prompt gösterilmez, session'a dönüş.
#[test]
fn wizard_supersede_ineligible_no_prompt() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let out = Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "t")
        .args(["review", "session", "--store", store.to_str().unwrap()])
        .write_stdin("supersede RuleCandidate:OldRule RuleCandidate:OldRule\nquit\n")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    // Rich preview render edilir — self blocker gösterilir.
    assert!(out.contains("Self supersede"), "expected self blocker: {out}");
    // ineligible → confirmation prompt yok.
    assert!(
        !out.contains("Apply this exact supersession?"),
        "no prompt for ineligible: {out}"
    );
}

/// Ineligible preview state transition göstermez (Review P1-a). Successor Candidate →
/// "currently blocked" var, "remains current Accepted" yok.
#[test]
fn supersede_preview_ineligible_hides_state_transition() {
    let dir = tempdir().unwrap();
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "Old", "kind": "RuleCandidate"},
        {"canonical": "New", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    // Yalnız New'i accept et; Old Candidate kalır → ineligible (superseded_not_current).
    let d = show_digest(&store, "RuleCandidate:New");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "accept", "RuleCandidate:New", "--store", store.to_str().unwrap(),
            "--operator", "t", "--reason", "ok", "--yes", "--basis-digest", &d,
        ])
        .assert()
        .success();
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:Old", "RuleCandidate:New",
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    // ineligible → blocked mesajı, state transition yok.
    assert!(out.contains("currently blocked"), "expected blocked msg: {out}");
    assert!(
        !out.contains("remains current Accepted"),
        "ineligible must not show state transition: {out}"
    );
}

/// Consolidation lineage DAG — edge-list gösterimi (sahte chain yok). C→A, C→B branching
/// korunur; "C → A → B" sahte chain'i üretilmez (Review P1-b).
#[test]
fn supersede_preview_consolidation_dag_not_fake_chain() {
    let dir = tempdir().unwrap();
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "A", "kind": "RuleCandidate"},
        {"canonical": "B", "kind": "RuleCandidate"},
        {"canonical": "C", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    for id in ["RuleCandidate:A", "RuleCandidate:B", "RuleCandidate:C"] {
        let d = show_digest(&store, id);
        Command::cargo_bin("osp")
            .unwrap()
            .args([
                "review", "accept", id, "--store", store.to_str().unwrap(),
                "--operator", "t", "--reason", "ok", "--yes", "--basis-digest", &d,
            ])
            .assert()
            .success();
    }
    supersede(&store, "RuleCandidate:A", "RuleCandidate:C"); // C→A
    supersede(&store, "RuleCandidate:B", "RuleCandidate:C"); // C→B
    // Preview successor=C → consolidation DAG (C→A, C→B branching).
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review", "supersede-preview", "RuleCandidate:A", "RuleCandidate:C",
            "--store", store.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();
    // Edge-list gösterimi — iki ayrı edge (branching korunur).
    assert!(
        out.contains("RuleCandidate:C --Supersedes--> RuleCandidate:A"),
        "expected C→A edge in DAG view: {out}"
    );
    assert!(
        out.contains("RuleCandidate:C --Supersedes--> RuleCandidate:B"),
        "expected C→B edge in DAG view: {out}"
    );
    // Sahte chain üretilmemeli (C → A → B branching değil).
    assert!(
        !out.contains("RuleCandidate:C → RuleCandidate:A → RuleCandidate:B"),
        "must not produce fake chain for consolidation: {out}"
    );
}
