//! Faz 5 End-to-End Demo — Mock LLM pipeline.
//!
//! Bu test OSP'nin tam Agent→Engine→Commit akışını gösterir:
//!
//! 1. Space (mevcut kod) → Intent (Agent görevi)
//! 2. compute_space_slice → Agent'ın gördüğü alt-graf
//! 3. Mock LLM → DeltaProposal (structural changes only, NO positions)
//! 4. OutputContract::validate → Q4 Syntax check
//! 5. compute_raw_from_delta → Engine measures actual position (LLM declare ETMEZ)
//! 6. SpaceEngine::commit → Q4-Q6 gates → Q1-Q3 witness → Commit/Reject
//!
//! İki senaryo:
//! - **PASS:** Agent geçerli auth modülü ekler → commit başarılı
//! - **FAIL:** Agent self-import önerir → Q4 syntax gate reddeder

use osp_core::agent::{
    compute_space_slice, EvidenceSummary, NewNodeSpec, OutputContract, PermissionMask, SpaceSlice,
};
use osp_core::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
use osp_core::coords::{CoordinateSystem, RawPosition};
use osp_core::engine::{EngineCommitError, EngineConfig, SpaceEngine};
use osp_core::space::{Edge, EdgeKind, Node, NodeKind, Space};
use osp_core::vision::VisionVector;
use osp_core::witness::{EvidenceEvent, EvidenceId, Intent, WitnessKind, WitnessSet};

// ═══════════════════════════════════════════════════════════════════════════════
// Test Space: küçük proje (config, utils, main)
// ═══════════════════════════════════════════════════════════════════════════════

fn make_project_space() -> Space {
    let mut space = Space::new();
    // config.py (node 1) — stable foundation, nobody imports from it except main
    space.insert_node(Node {
        id: 1,
        kind: NodeKind::Module,
        mass: 30.0,
        ..Default::default()
    });
    // utils.py (node 2)
    space.insert_node(Node {
        id: 2,
        kind: NodeKind::Module,
        mass: 20.0,
        ..Default::default()
    });
    // main.py (node 3) — imports config and utils
    space.insert_node(Node {
        id: 3,
        kind: NodeKind::Module,
        mass: 50.0,
        ..Default::default()
    });
    // main → config, main → utils
    space.insert_edge(Edge {
        from: 3,
        to: 1,
        kind: EdgeKind::Imports,
        ..Default::default()
    });
    space.insert_edge(Edge {
        from: 3,
        to: 2,
        kind: EdgeKind::Imports,
        ..Default::default()
    });
    space
}

fn make_engine(space: Space) -> SpaceEngine {
    let cs = CoordinateSystem::default_raw_five(
        CohesionAxis::new(),
        EntropyAxis::from_commit_entropy(6.0),
        WitnessDepthAxis::from_witness(0.5, 3),
    );
    let vision = VisionVector::new(RawPosition {
        x: 0.4,
        y: 0.6,
        z: 0.5,
        w: 0.5,
        v: 0.5,
    });
    SpaceEngine::with_default_rules(space, cs, vision, EngineConfig::default_calibrated())
}

fn ev(id: EvidenceId, actor: u64) -> EvidenceEvent {
    EvidenceEvent::new(id, "github", WitnessKind::MergeCommit, actor, 1)
}

fn two_witnesses() -> WitnessSet {
    WitnessSet::new(vec![ev(1, 200), ev(2, 201)])
}

fn one_witness() -> WitnessSet {
    WitnessSet::new(vec![ev(1, 200)])
}

// ═══════════════════════════════════════════════════════════════════════════════
// Mock LLM: structural changes only (NO positions declared)
// ═══════════════════════════════════════════════════════════════════════════════

/// Mock LLM — "auth modülü ekle, config'e bağımlı" önerisi.
///
/// **KRİTİK (inv #4):** Pozisyon İÇERMEZ — sadece structural değişiklik.
/// Engine compute_raw_from_delta ile gerçek pozisyonu ölçer.
fn mock_llm_add_auth() -> (Vec<Node>, Vec<Edge>) {
    let auth_node = Node {
        id: 4,
        kind: NodeKind::Module,
        mass: 40.0,
        ..Default::default()
    };
    let auth_import = Edge {
        from: 4,
        to: 1,
        kind: EdgeKind::Imports,
        ..Default::default()
    };
    (vec![auth_node], vec![auth_import])
}

/// Mock LLM (HATALI) — "auth modülü kendini import ediyor" → Q4 reject.
fn mock_llm_self_import() -> (Vec<Node>, Vec<Edge>) {
    let auth_node = Node {
        id: 4,
        kind: NodeKind::Module,
        mass: 40.0,
        ..Default::default()
    };
    // BUG: auth imports itself
    let self_loop = Edge {
        from: 4,
        to: 4,
        kind: EdgeKind::Imports,
        ..Default::default()
    };
    (vec![auth_node], vec![self_loop])
}

// ═══════════════════════════════════════════════════════════════════════════════
// PASS: Agent adds valid auth module
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn e2e_agent_adds_valid_module_passes_all_gates() {
    let space = make_project_space();
    let mut engine = make_engine(space);

    // Step 1: Intent — Agent wants to work near main module
    let intent = Intent::new(42, RawPosition::default());

    // Step 2: compute_space_slice — Agent sees the project
    let mask = PermissionMask::full_access();
    let slice: SpaceSlice = compute_space_slice(
        &[3], // target: main module
        engine.space(),
        &[],
        &mask,
        &EvidenceSummary::empty(),
        2, // k=2 hops
    );
    assert!(slice.node_count() >= 3, "Agent should see main + neighbors");

    // Step 3: Mock LLM produces DeltaProposal (structural only)
    let (delta_nodes, delta_edges) = mock_llm_add_auth();

    // Step 4: OutputContract validates the proposal (Q4 Agent shell)
    let contract = OutputContract::strict();
    // Build a minimal DeltaProposal for validation
    let proposal = osp_core::agent::DeltaProposal {
        new_nodes: delta_nodes
            .iter()
            .map(|n| NewNodeSpec {
                kind: n.kind,
                initial_mass: n.mass,
                connected_to: vec![],
            })
            .collect(),
        reasoning: "Auth module needs config for settings".to_string(),
        ..Default::default()
    };
    assert!(
        contract.validate(&proposal).is_ok(),
        "DeltaProposal should pass OutputContract"
    );

    // Step 5: Engine computes actual position (LLM never declares — inv #4)
    let computed_raw = engine.compute_raw_from_delta(&delta_nodes, &delta_edges);
    assert!(computed_raw.x.is_finite(), "coupling must be measured");
    assert!(
        computed_raw.z >= 0.0 && computed_raw.z <= 1.0,
        "instability ∈ [0,1]"
    );

    // Step 6: Build Claim with engine-computed position
    let claim = osp_core::witness::Claim {
        id: 1,
        intent,
        author: 42,
        computed_raw,
        delta_nodes,
        delta_edges,
        task_id: None, // standalone (Paper 1, INV-T5)
    };

    // Step 7: Commit pipeline — Q4 → Q5 → Q6 → Q1-Q3
    let omega = two_witnesses();
    let result = engine.commit(&claim, &omega);

    assert!(
        result.is_ok(),
        "Valid auth module should commit — got: {:?}",
        result.as_ref().err()
    );

    let outcome = result.unwrap();
    assert_eq!(outcome.t_c, 1, "time should advance to t_c=1");
    assert_eq!(outcome.event.new_nodes.len(), 1, "1 new node (auth)");
    assert_eq!(outcome.event.new_edges.len(), 1, "1 new edge (auth→config)");
}

// ═══════════════════════════════════════════════════════════════════════════════
// FAIL: Agent proposes self-import → Q4 rejects
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn e2e_self_import_rejected_by_q4_syntax_gate() {
    let space = make_project_space();
    let mut engine = make_engine(space);

    // Mock LLM (buggy): auth imports itself
    let (delta_nodes, delta_edges) = mock_llm_self_import();

    // Engine computes position (even for bad proposals — measurement is neutral)
    let computed_raw = engine.compute_raw_from_delta(&delta_nodes, &delta_edges);

    let claim = osp_core::witness::Claim {
        id: 1,
        intent: Intent::new(42, RawPosition::default()),
        author: 42,
        computed_raw,
        delta_nodes,
        delta_edges,
        task_id: None, // standalone (Paper 1, INV-T5)
    };

    let result = engine.commit(&claim, &two_witnesses());

    // Q4 should catch the self-import BEFORE Q5/Q6/witness
    assert!(
        matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
        "Self-import should be caught by Q4 Syntax Gate — got: {:?}",
        result.as_ref().err()
    );

    // NO mutation — Safety guarantee
    assert_eq!(
        engine.space().node_count(),
        3,
        "space unchanged after Q4 reject"
    );
    assert_eq!(engine.t_c(), 0, "time not advanced");
}

// ═══════════════════════════════════════════════════════════════════════════════
// FAIL: Agent's position deviates too much from vision → Q5 rejects
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn e2e_vision_violation_rejected_by_q5() {
    let space = make_project_space();
    let mut engine = make_engine(space);

    // Agent proposes a module with extreme position (far from vision)
    let claim = osp_core::witness::Claim {
        id: 1,
        intent: Intent::new(42, RawPosition::default()),
        author: 42,
        // Zero-vector → θ=1.0 (maximum deviation from any vision)
        computed_raw: RawPosition::default(), // all zeros → max θ
        delta_nodes: vec![Node {
            id: 4,
            kind: NodeKind::Module,
            mass: 40.0,
            ..Default::default()
        }],
        delta_edges: vec![],
        task_id: None, // standalone (Paper 1, INV-T5)
    };

    let result = engine.commit(&claim, &two_witnesses());

    // Q4 passes (valid syntax), but Q5 catches vision deviation
    assert!(
        matches!(result, Err(EngineCommitError::VisionViolation { .. })),
        "Zero-vector position should fail Q5 Vision Gate — got: {:?}",
        result.as_ref().err()
    );
    assert_eq!(
        engine.space().node_count(),
        3,
        "no mutation after Q5 reject"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// HOLD: Insufficient witnesses → Q1 rejects
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn e2e_insufficient_witnesses_hold() {
    let space = make_project_space();
    let mut engine = make_engine(space);

    let (delta_nodes, delta_edges) = mock_llm_add_auth();
    let computed_raw = engine.compute_raw_from_delta(&delta_nodes, &delta_edges);

    let claim = osp_core::witness::Claim {
        id: 1,
        intent: Intent::new(42, RawPosition::default()),
        author: 42,
        computed_raw,
        delta_nodes,
        delta_edges,
        task_id: None, // standalone (Paper 1, INV-T5)
    };

    // Only 1 witness → Hold (min_approvers=2 not met)
    let result = engine.commit(&claim, &one_witness());

    assert!(
        matches!(result, Err(EngineCommitError::Witness(_))),
        "1 witness should Hold — got: {:?}",
        result.as_ref().err()
    );
    assert_eq!(engine.space().node_count(), 3, "no mutation on Hold");
}

// ═══════════════════════════════════════════════════════════════════════════════
// FULL PIPELINE PRINT: diagnostic (not a pass/fail test, just shows the flow)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn e2e_pipeline_diagnostic() {
    let space = make_project_space();
    let mut engine = make_engine(space);

    eprintln!("\n═══════════════════════════════════════════════════");
    eprintln!("  OSP Faz 5 End-to-End Pipeline (Mock LLM)");
    eprintln!("═══════════════════════════════════════════════════\n");

    // 1. Space slice
    let mask = PermissionMask::full_access();
    let slice = compute_space_slice(
        &[3],
        engine.space(),
        &[],
        &mask,
        &EvidenceSummary::empty(),
        2,
    );
    eprintln!("Step 1: compute_space_slice([main], k=2)");
    eprintln!(
        "  → Agent sees {} nodes, {} edges",
        slice.node_count(),
        slice.edge_count()
    );

    // 2. Mock LLM
    let (delta_nodes, delta_edges) = mock_llm_add_auth();
    eprintln!("\nStep 2: Mock LLM → DeltaProposal");
    eprintln!(
        "  → {} new node(s), {} new edge(s)",
        delta_nodes.len(),
        delta_edges.len()
    );
    eprintln!("  → NO positions declared (engine measures)");

    // 3. Position computation
    let computed_raw = engine.compute_raw_from_delta(&delta_nodes, &delta_edges);
    eprintln!("\nStep 3: compute_raw_from_delta (engine measures)");
    eprintln!("  → coupling = {:.3}", computed_raw.x);
    eprintln!("  → cohesion = {:.3}", computed_raw.y);
    eprintln!("  → instability = {:.3}", computed_raw.z);

    // 4. Commit
    let claim = osp_core::witness::Claim {
        id: 1,
        intent: Intent::new(42, RawPosition::default()),
        author: 42,
        computed_raw,
        delta_nodes,
        delta_edges,
        task_id: None, // standalone (Paper 1, INV-T5)
    };
    let result = engine.commit(&claim, &two_witnesses());
    eprintln!("\nStep 4: commit → Q4→Q5→Q6→Q1-Q3");
    match &result {
        Ok(outcome) => {
            eprintln!("  → COMMIT ✓ (t_c={})", outcome.t_c);
            eprintln!("  → {} nodes in space", engine.space().node_count());
        }
        Err(e) => eprintln!("  → REJECTED: {}", e),
    }
    eprintln!("\n═══════════════════════════════════════════════════\n");

    assert!(result.is_ok());
}
