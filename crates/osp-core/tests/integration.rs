//! Faz 2.7-2.8 — End-to-end integration test.
//!
//! Config → Engine → Commit → Restore → Verify lifecycle.
//! All 4 scenarios exercise the full public API.

use osp_core::axes::{EntropyAxis, WitnessDepthAxis};
use osp_core::coords::{CoordinateSystem, MetricSource, RawPosition};
use osp_core::engine::{EngineCommitError, EngineConfig, SpaceEngine};
use osp_core::persistence::SnapshotStore;
use osp_core::space::{Edge, EdgeKind, Node, NodeKind, Space};
use osp_core::vision_config::VisionConfig;
use osp_core::witness::{Claim, EvidenceEvent, EvidenceId, Intent, WitnessKind, WitnessSet};

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

fn mod_node(id: u64) -> Node {
    Node {
        id,
        kind: NodeKind::Module,
        ..Default::default()
    }
}

fn import_edge(from: u64, to: u64) -> Edge {
    Edge {
        from,
        to,
        kind: EdgeKind::Imports,
        ..Default::default()
    }
}

fn ev(id: EvidenceId, actor: u64) -> EvidenceEvent {
    EvidenceEvent::new(id, &format!("src-{id}"), WitnessKind::MergeCommit, actor, 1)
}

fn two_witnesses() -> WitnessSet {
    WitnessSet::new(vec![ev(1, 200), ev(2, 300)])
}

fn make_coord_system() -> CoordinateSystem {
    CoordinateSystem::default_raw_five(
        // INV-T9 #70: integration test fixture — Placeholder topology + Placeholder cohesion.
        MetricSource::Placeholder,
        osp_core::axes::CohesionAxis::try_from_normalized(0.7)
            .expect("integration test fallback cohesion 0.7"),
        EntropyAxis::from_commit_entropy(6.5),
        WitnessDepthAxis::from_witness(0.35, 30),
    )
    .expect("integration test axis registration: 5 distinct core axes")
}

fn claim_aligned(id: u64, author: u64, vision_raw: RawPosition) -> Claim {
    Claim {
        id,
        intent: Intent::new(author, vision_raw),
        author,
        computed_raw: vision_raw,
        delta_nodes: vec![mod_node(100 + id)],
        delta_edges: vec![],
        task_id: None,         // standalone (Paper 1, INV-T5)
        removed_edges: vec![], // G2c-2
    }
}

fn claim_bad(id: u64, author: u64) -> Claim {
    // Zero-vector → CosineDeviation θ=1.0 → Q5 reject
    Claim {
        id,
        intent: Intent::new(author, RawPosition::default()),
        author,
        computed_raw: RawPosition::default(),
        delta_nodes: vec![mod_node(200 + id)],
        delta_edges: vec![],
        task_id: None,         // standalone (Paper 1, INV-T5)
        removed_edges: vec![], // G2c-2
    }
}

const VISION_TOML: &str = r#"
[raw]
x = 0.4
y = 0.7
z = 0.5
w = 0.5
v = 0.5
"#;

const VISION_RAW: RawPosition = RawPosition {
    x: 0.4,
    y: 0.7,
    z: 0.5,
    w: 0.5,
    v: 0.5,
};

// ─────────────────────────────────────────────────────────────────────────────
// Scenario 1: Full lifecycle (config → engine → commit → verify)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn scenario_1_full_lifecycle() {
    // 1. Load config
    let config = VisionConfig::from_str(VISION_TOML).expect("TOML parse");

    // 2. Build engine
    let space = Space::new();
    let cs = make_coord_system();
    let mut engine = SpaceEngine::from_vision_config(space, cs, &config);
    assert_eq!(engine.t_c(), 0);

    // 3. Commit aligned claim
    let claim = claim_aligned(1, 100, VISION_RAW);
    let omega = two_witnesses();
    let outcome = engine.commit(&claim, &omega).expect("commit");

    // 4. Verify
    assert_eq!(outcome.t_c, 1);
    assert!(!outcome.safety_weakened);
    assert_eq!(engine.space().node_count(), 1);
    assert!(engine.space().nodes.contains_key(&101));

    // 5. Second commit
    let claim2 = claim_aligned(2, 100, VISION_RAW);
    let outcome2 = engine.commit(&claim2, &omega).expect("commit 2");
    assert_eq!(outcome2.t_c, 2);
    assert_eq!(engine.space().node_count(), 2);
}

// ─────────────────────────────────────────────────────────────────────────────
// Scenario 2: Q5 vision rejection (Safety — reviewer #1; Q4-Q6 split sonrası Q5)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn scenario_2_q5_vision_rejection() {
    let config = VisionConfig::from_str(VISION_TOML).unwrap();
    let cs = make_coord_system();
    let mut engine = SpaceEngine::from_vision_config(Space::new(), cs, &config);

    // Bad claim (zero-vector → θ=1.0 > bound=0.3)
    let bad = claim_bad(1, 100);
    let omega = two_witnesses();

    let result = engine.commit(&bad, &omega);

    assert!(
        matches!(result, Err(EngineCommitError::VisionViolation { .. })),
        "Q5 vision violation must reject"
    );
    // NO mutation — Safety guarantee
    assert_eq!(
        engine.space().node_count(),
        0,
        "no mutation after Q5 reject"
    );
    assert_eq!(engine.t_c(), 0, "t_c unchanged after Q5 reject");

    // Now commit a GOOD claim — should succeed (space still clean)
    let good = claim_aligned(2, 100, VISION_RAW);
    let outcome = engine.commit(&good, &omega).expect("good commit");
    assert_eq!(outcome.t_c, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Scenario 3: Drift warning (post-mutation neighbor degradation)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn scenario_3_drift_warning() {
    // Build a space with an extreme node (high coupling)
    let mut space = Space::new();
    space.insert_node(mod_node(1)); // hub
    for i in 2..15 {
        space.insert_node(mod_node(i));
        space.insert_edge(import_edge(1, i)); // node 1 imports everything
    }

    let cs = make_coord_system();
    // Vision with opposite pattern — high where raw is low, low where raw is high
    let vision = osp_core::vision::VisionVector::new(RawPosition {
        x: 0.0,
        y: 1.0,
        z: 0.0,
        w: 1.0,
        v: 0.0,
    });
    // IMPORTANT: cosine deviation with [0,1]-normalized values → θ_max = 0.5 (orthogonal limit).
    // theta_bound=0.5 unreachable for non-zero all-positive vectors.
    // Set theta_bound=0.2 for realistic drift detection. TDA diffusion (Faz 5+) resolves this.
    let mut config = EngineConfig::default_calibrated();
    config.theta_bound = 0.2;
    let mut engine = SpaceEngine::new(space, cs, vision, config);

    // full_reposition — node 1 has high coupling (x≈0.93) vs vision (x=0.2)
    let warnings = engine.full_reposition();

    assert!(
        !warnings.is_empty(),
        "node 1 high coupling → drift warning expected"
    );
    assert!(
        warnings.iter().any(|w| w.node_id == 1),
        "node 1 should have drift warning"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Scenario 4: Event-sourcing (milestone + delta → restore)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn scenario_4_event_sourcing_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let config = VisionConfig::from_str(VISION_TOML).unwrap();
    let cs = make_coord_system();
    let mut engine = SpaceEngine::from_vision_config(Space::new(), cs, &config)
        .with_persistence(tmp.path())
        .expect("persistence");

    let omega = two_witnesses();

    // Milestone at t_c=0 (empty)
    engine.save_milestone().unwrap();

    // 3 commits → t_c=1,2,3 (nodes 101, 102, 103)
    for i in 1..=3u64 {
        let claim = claim_aligned(i, 100, VISION_RAW);
        engine.commit(&claim, &omega).unwrap();
    }
    assert_eq!(engine.t_c(), 3);
    assert_eq!(engine.space().node_count(), 3);

    // Restore to t_c=2 → 2 nodes, 1 delta replayed (from milestone 0)
    let replayed = engine.restore(2).unwrap();
    assert_eq!(replayed, 2); // deltas t_c=1,2 replayed
    assert_eq!(engine.t_c(), 2);
    assert_eq!(engine.space().node_count(), 2);
    assert!(engine.space().nodes.contains_key(&101));
    assert!(engine.space().nodes.contains_key(&102));
    assert!(!engine.space().nodes.contains_key(&103)); // t_c=3 not replayed

    // Restore to t_c=0 (milestone only)
    let replayed0 = engine.restore(0).unwrap();
    assert_eq!(replayed0, 0); // no deltas to replay
    assert_eq!(engine.space().node_count(), 0); // empty milestone

    // Restore forward to t_c=3 → all 3 nodes back
    let replayed3 = engine.restore(3).unwrap();
    assert_eq!(replayed3, 3);
    assert_eq!(engine.space().node_count(), 3);
}

// ─────────────────────────────────────────────────────────────────────────────
// Scenario 5: Config validation (colleague #1 — [0,1] enforcement)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn scenario_5_config_validation_rejects_out_of_range() {
    let bad_toml = r#"
[raw]
x = 1.5  # out of range!
y = 0.5
z = 0.5
w = 0.5
v = 0.5
"#;
    let result = VisionConfig::from_str(bad_toml);
    assert!(result.is_err(), "x=1.5 must be rejected");
}

// ─────────────────────────────────────────────────────────────────────────────
// Scenario 6: Milestone at interval (periodic full snapshot)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn scenario_6_milestone_at_interval() {
    let tmp = tempfile::tempdir().unwrap();
    let config = VisionConfig::from_str(VISION_TOML).unwrap();

    // Override milestone_interval to 3 (for testing)
    let mut engine_config = EngineConfig::from_vision_config(&config);
    engine_config.milestone_interval = 3;

    let cs = make_coord_system();
    let mut engine = SpaceEngine::new(Space::new(), cs, config.to_vision_vector(), engine_config)
        .with_persistence(tmp.path())
        .unwrap();

    let omega = two_witnesses();
    for i in 1..=3u64 {
        let claim = claim_aligned(i, 100, VISION_RAW);
        engine.commit(&claim, &omega).unwrap();
    }

    // t_c=3 → milestone saved (3 % 3 == 0)
    let store = SnapshotStore::new(tmp.path()).unwrap();
    let milestones = store.list_milestones().unwrap();
    assert!(
        milestones.contains(&3),
        "milestone at t_c=3 (interval=3): {:?}",
        milestones
    );
}
