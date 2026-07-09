//! Supersession integration test matrisi (R3#9).
//!
//! `osp review supersede` uçtan uca — mutlu yol, yön assert, endpoint-specific stale,
//! swapped digest, missing/non-current, self, consolidation, chain, restart-safe,
//! rename regression, negatif (digest eksik/confirmation n), interactive, JSON.

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

/// Mutlu yol + yön assert (R2-R1): old SupersededAccepted, new Accepted, successor gösteriliyor.
#[test]
fn supersede_happy_path_direction_assert() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    let new_d = show_digest(&store, "RuleCandidate:NewRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "replacement",
            "--yes",
            "--superseded-digest",
            &old_d,
            "--successor-digest",
            &new_d,
        ])
        .assert()
        .success()
        .stdout(contains("supersedes"))
        // Yön: successor (NewRule) superseder, superseded (OldRule) subject.
        .stdout(contains(
            "RuleCandidate:NewRule supersedes RuleCandidate:OldRule",
        ));
    // Yön assert: old SupersededAccepted + successor.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "show",
            "RuleCandidate:OldRule",
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .stdout(contains("SupersededAccepted"))
        .stdout(contains("Superseded by: RuleCandidate:NewRule"));
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "show",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .stdout(contains("Accepted"));
}

/// Supersede JSON çıktı (R4).
#[test]
fn supersede_json_output() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    let new_d = show_digest(&store, "RuleCandidate:NewRule");
    let out = Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &old_d,
            "--successor-digest",
            &new_d,
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["mutation"]["status"], "superseded");
    assert_eq!(v["mutation"]["superseded_node_id"], "RuleCandidate:OldRule");
    assert_eq!(v["mutation"]["successor_node_id"], "RuleCandidate:NewRule");
}

/// Stale superseded digest → fail + store unchanged (atomicity, R1#4 endpoint-specific).
#[test]
fn supersede_stale_superseded_digest_rejected() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let new_d = show_digest(&store, "RuleCandidate:NewRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            "0000000000000000",
            "--successor-digest",
            &new_d,
        ])
        .assert()
        .failure()
        .stderr(contains("stale superseded basis"));
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 2"));
}

/// Swapped digest (R2-R1): old digest successor flag'ine → endpoint-specific stale.
#[test]
fn supersede_swapped_digests_rejected() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    let new_d = show_digest(&store, "RuleCandidate:NewRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &new_d,
            "--successor-digest",
            &old_d,
        ])
        .assert()
        .failure();
    // T1 atomicity: store unchanged — revision aynı, statuses hâlâ Accepted.
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 2"))
        .stdout(contains("superseded: 0"));
}

/// Missing endpoint → NotFound (R1#2).
#[test]
fn supersede_missing_endpoint_not_found() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let new_d = show_digest(&store, "RuleCandidate:NewRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:Ghost",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            "0000000000000000",
            "--successor-digest",
            &new_d,
        ])
        .assert()
        .failure()
        .stderr(contains("not found"));
}

/// Self-supersede → reject.
#[test]
fn supersede_self_rejected() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let d = show_digest(&store, "RuleCandidate:OldRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:OldRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &d,
            "--successor-digest",
            &d,
        ])
        .assert()
        .failure()
        .stderr(contains("self-supersede"));
}

/// Negatif: successor-digest eksik → fail (R3#7).
#[test]
fn supersede_missing_successor_digest_rejected() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &old_d,
        ])
        .assert()
        .failure()
        .stderr(contains("--successor-digest"));
}

/// Restart-safe: supersede sonrası state korunuyor.
#[test]
fn supersede_restart_safe() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    let new_d = show_digest(&store, "RuleCandidate:NewRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &old_d,
            "--successor-digest",
            &new_d,
        ])
        .assert()
        .success();
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 3"))
        .stdout(contains("Supersede records: 1"));
}

/// Rename regression (R3#10): Accepted node show → node_digest_hex dolu.
#[test]
fn accepted_node_show_has_digest() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "show",
            "RuleCandidate:OldRule",
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .stdout(contains("Node digest:"));
}

/// Consolidation (R2): bir successor iki farklı old supersede.
#[test]
fn supersede_consolidation() {
    let dir = tempdir().unwrap();
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "OldA", "kind": "RuleCandidate"},
        {"canonical": "OldB", "kind": "RuleCandidate"},
        {"canonical": "New", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    for id in [
        "RuleCandidate:OldA",
        "RuleCandidate:OldB",
        "RuleCandidate:New",
    ] {
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
    for old in ["RuleCandidate:OldA", "RuleCandidate:OldB"] {
        let old_d = show_digest(&store, old);
        let new_d = show_digest(&store, "RuleCandidate:New");
        Command::cargo_bin("osp")
            .unwrap()
            .args([
                "review",
                "supersede",
                old,
                "RuleCandidate:New",
                "--store",
                store.to_str().unwrap(),
                "--operator",
                "t",
                "--reason",
                "consolidate",
                "--yes",
                "--superseded-digest",
                &old_d,
                "--successor-digest",
                &new_d,
            ])
            .assert()
            .success();
    }
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Supersede records: 2"))
        .stdout(contains("superseded: 2"));
}

/// Chain (Review 1): B→A, C→B; lineage + superseded_by.
#[test]
fn supersede_chain_lineage() {
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
    let a_d = show_digest(&store, "RuleCandidate:A");
    let b_d = show_digest(&store, "RuleCandidate:B");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:A",
            "RuleCandidate:B",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "b>a",
            "--yes",
            "--superseded-digest",
            &a_d,
            "--successor-digest",
            &b_d,
        ])
        .assert()
        .success();
    let b_d2 = show_digest(&store, "RuleCandidate:B");
    let c_d = show_digest(&store, "RuleCandidate:C");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:B",
            "RuleCandidate:C",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "c>b",
            "--yes",
            "--superseded-digest",
            &b_d2,
            "--successor-digest",
            &c_d,
        ])
        .assert()
        .success();
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "show",
            "RuleCandidate:A",
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .stdout(contains("Superseded by: RuleCandidate:B"));
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "show",
            "RuleCandidate:B",
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .stdout(contains("Superseded by: RuleCandidate:C"));
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "show",
            "RuleCandidate:C",
            "--store",
            store.to_str().unwrap(),
        ])
        .assert()
        .stdout(contains("Accepted"));
}

/// Interactive supersede (piped): presentation → confirm(y) → reason → superseded.
#[test]
fn interactive_supersede_piped() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "t")
        .args([
            "review",
            "session",
            "--store",
            store.to_str().unwrap(),
        ])
        .write_stdin("supersede RuleCandidate:OldRule RuleCandidate:NewRule\ny\nreplacement approved\nquit\n")
        .assert()
        .success()
        .stdout(contains("supersedes"))
        .stdout(contains("RuleCandidate:NewRule supersedes RuleCandidate:OldRule"));
}

/// Interactive supersede confirmation n → abort, store unchanged (R3#7).
#[test]
fn interactive_supersede_confirmation_n_aborts() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "t")
        .args(["review", "session", "--store", store.to_str().unwrap()])
        .write_stdin("supersede RuleCandidate:OldRule RuleCandidate:NewRule\nn\nquit\n")
        .assert()
        .success()
        .stdout(contains("aborted by operator"));
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 2"))
        .stdout(contains("superseded: 0"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Review test boşlukları (P1.2 gate + eksik senaryolar)
// ═══════════════════════════════════════════════════════════════════════════════

/// Stale successor digest (R1#4 endpoint-specific — superseded değil).
#[test]
fn supersede_stale_successor_digest_rejected() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &old_d,
            "--successor-digest",
            "0000000000000000",
        ])
        .assert()
        .failure()
        .stderr(contains("stale successor basis"));
    // T1 atomicity: store unchanged.
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 2"))
        .stdout(contains("superseded: 0"));
}

/// Existing-but-non-Accepted superseded → EndpointNotCurrent (R1#2). Reject sonrası node.
#[test]
fn supersede_non_accepted_superseded_endpoint_not_current() {
    let dir = tempdir().unwrap();
    // OldRule reject et (Candidate → Rejected), NewRule accept.
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "OldRule", "kind": "RuleCandidate"},
        {"canonical": "NewRule", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "reject",
            "RuleCandidate:OldRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "bad",
            "--yes",
            "--basis-digest",
            &old_d,
        ])
        .assert()
        .success();
    let new_d = show_digest(&store, "RuleCandidate:NewRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "accept",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "ok",
            "--yes",
            "--basis-digest",
            &new_d,
        ])
        .assert()
        .success();
    // Supersede: OldRule Rejected (non-Accepted) → EndpointNotCurrent.
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &old_d,
            "--successor-digest",
            &new_d,
        ])
        .assert()
        .failure()
        .stderr(contains("not current Accepted"));
}

/// Candidate successor → confirmation gate (P1.2): presentation gösterilmez, reason sorulmaz.
#[test]
fn supersede_candidate_successor_confirmation_gate() {
    let dir = tempdir().unwrap();
    // OldRule accept, NewRule Candidate (accept etme).
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "OldRule", "kind": "RuleCandidate"},
        {"canonical": "NewRule", "kind": "RuleCandidate"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "accept",
            "RuleCandidate:OldRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "ok",
            "--yes",
            "--basis-digest",
            &old_d,
        ])
        .assert()
        .success();
    // Successor (NewRule) Candidate bırakıldı — accept edilmedi.
    // Interactive: successor Candidate → "remains current Accepted" gösterilmez, EndpointNotCurrent.
    Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "t")
        .args(["review", "session", "--store", store.to_str().unwrap()])
        .write_stdin("supersede RuleCandidate:OldRule RuleCandidate:NewRule\nquit\n")
        .assert()
        .success()
        .stdout(contains("not current Accepted"));
}

/// Negatif: --superseded-digest eksik → fail.
#[test]
fn supersede_missing_superseded_digest_rejected() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let new_d = show_digest(&store, "RuleCandidate:NewRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--successor-digest",
            &new_d,
        ])
        .assert()
        .failure()
        .stderr(contains("--superseded-digest"));
}

/// Malformed successor digest → doğru flag mesajı (P2.3).
#[test]
fn supersede_malformed_successor_digest_correct_flag_message() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "RuleCandidate:NewRule",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &old_d,
            "--successor-digest",
            "xyz",
        ])
        .assert()
        .failure()
        .stderr(contains("--successor-digest"))
        .stderr(contains("invalid"));
}

/// Interactive self-supersede → presentation gate (Review 2.tur P1): çelişkili confirmation
/// gösterilmez, "self-supersede forbidden", Reason sorulmaz, revision değişmez.
#[test]
fn supersede_self_presentation_gate() {
    let dir = tempdir().unwrap();
    let store = setup_two_accepted(dir.path());
    let output = Command::cargo_bin("osp")
        .unwrap()
        .env("OSP_OPERATOR", "t")
        .args(["review", "session", "--store", store.to_str().unwrap()])
        .write_stdin("supersede RuleCandidate:OldRule RuleCandidate:OldRule\nquit\n")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(output).unwrap();
    // Rich preview render edilir — self-supersede blocker gösterilir (tek canonical model).
    assert!(
        out.contains("Self supersede"),
        "expected self-supersede blocker in preview: {out}"
    );
    assert!(
        out.contains("Both endpoint IDs are identical"),
        "expected self-supersede operator note: {out}"
    );
    // ineligible → confirmation prompt gösterilmez.
    assert!(
        !out.contains("Apply this exact supersession?"),
        "confirmation must NOT show for ineligible (self) supersede: {out}"
    );
    assert!(
        !out.contains("Reason:"),
        "reason must NOT be asked for ineligible (self) supersede: {out}"
    );
    // Revision unchanged.
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 2"));
}

/// Incompatible endpoints uçtan uca (Review 1 küçük — production-surface zinciri):
/// CLI command → SupersedeSession → StoreError::IncompatibleSupersedeEndpoints → typed
/// ReviewError → kullanıcı mesajı → revision unchanged. Farklı kind (Rule vs Concept).
#[test]
fn supersede_incompatible_endpoints_e2e() {
    let dir = tempdir().unwrap();
    // OldRule = RuleCandidate, Payment = Concept (farklı kind).
    let seed = r#"{ "schema_version": 1, "nodes": [
        {"canonical": "OldRule", "kind": "RuleCandidate"},
        {"canonical": "Payment", "kind": "Concept"}
    ]}"#;
    let store = init_store(dir.path(), seed);
    for id in ["RuleCandidate:OldRule", "Concept:Payment"] {
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
    let old_d = show_digest(&store, "RuleCandidate:OldRule");
    let new_d = show_digest(&store, "Concept:Payment");
    Command::cargo_bin("osp")
        .unwrap()
        .args([
            "review",
            "supersede",
            "RuleCandidate:OldRule",
            "Concept:Payment",
            "--store",
            store.to_str().unwrap(),
            "--operator",
            "t",
            "--reason",
            "r",
            "--yes",
            "--superseded-digest",
            &old_d,
            "--successor-digest",
            &new_d,
        ])
        .assert()
        .failure()
        .stderr(contains("incompatible"));
    // T1 atomicity: store unchanged.
    Command::cargo_bin("osp")
        .unwrap()
        .args(["graph", "status", "--store", store.to_str().unwrap()])
        .assert()
        .stdout(contains("Revision: 2"))
        .stdout(contains("superseded: 0"));
}
