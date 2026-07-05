//! Paper 3 evidence generators — binding chain replay + rejected paths.
//!
//! Üç evidence artefaktı üretilir (hepsi gerçek pipeline koşusuyla, helper duplication yok):
//!
//! 1. **E2E binding chain replay** (`e2e-binding-chain-replay.json`) — §2 Motivating Example
//!    ve §7 Verification Evidence için. Cümle → RuleCandidate → PredicateStub →
//!    CrossFamilyHint → operator binding → ExecutablePredicateSet → verify → task → registry.
//!    Adım 1 (cümle→RuleCandidate) **gerçek pipeline koşusu** (Faz 1'i çağırır, seed etmez).
//!    Adım 6 (Candidate→Accepted) INV-C3 gereği **seed edilir** (OperatorAcceptance pub(crate));
//!    promotion in-crate test `store_promotion_requires_operator_acceptance` ile enforced.
//!
//! 2. **E2E rejected paths replay** (`e2e-rejected-paths-replay.json`) — §7 için.
//!    *"A gate that only passes is indistinguishable from no gate."*
//!    4 reddedilen yol: AxisMismatch, AxisNotInCandidates, TemplateNotSuggested, NotAccepted.
//!
//! 3. **Pre-flight canonical/marker tablosu** (Appendix A) — her evidence/held-out cümle için
//!    `ilk-3-kelime canonical → normalize → has_rule_signal → ambiguity/axes` assert.
//!    3 tekrarlanan canonical-kesme tuzağını (A1→B1→B5) + marker-kaçırma tuzağını yapısal
//!    imkânsız kılar. *"Test altına alınmayan invariant ihlal edilir."*
//!
//! # Snapshot disiplini (A5)
//! Normal CI testleri donmuş JSON ile `assert_eq!` yapar — source tree'yi MUTATE ETMEZ.
//! `PAPER3_FREEZE=1 cargo test --test paper3_evidence -- --ignored --nocapture` ile
//! bilinçli yeniden dondurma. Evidence JSON'lar saf deterministik builder çıktısıdır
//! (volatile commit/tarih/toolchain YOK — tek evi `run-metadata.json`).
//!
//! # Çalıştırma
//! ```bash
//! # drift yakalar (normal CI):
//! cargo test -p osp-core --test paper3_evidence
//! # bilinçli dondurma:
//! PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_evidence -- --ignored --nocapture
//! ```

use osp_core::anchoring::classifier::Classifier;
use osp_core::anchoring::gate::AnchorGateContext;
use osp_core::anchoring::pipeline::AnchorPipeline;
use osp_core::anchoring::predicate_lowering::{
    bind_metric_threshold, lower_rule_to_predicate_stub, BindingError, MetricThresholdBinding,
    NormalizedMetricThreshold, PhysicalCodeMetricAxis, PredicateLoweringOutcome,
    TranslationAmbiguity,
};
use osp_core::anchoring::store::InMemoryAnchorStore;
use osp_core::anchoring::types::{ConceptNode, ConceptNodeKind, GraphSeed, PacketSource};
use osp_core::anchoring::{ConceptEdgeKind, ConceptPacketType, DecisionStatus, PositionFamily};
use osp_core::task_bridge::{
    create_task_from_accepted_candidate_default_label, verify_accepted_task_candidate,
    TaskGenesisError,
};
use osp_core::trajectory::{
    InMemoryTaskRegistry, OperatorCapability, PredicateScope, TaskResolver,
};
use serde_json::{json, Value};

// ═══════════════════════════════════════════════════════════════════════════════
// Evidence JSON yolları (CARGO_MANIFEST_DIR = crates/osp-core, 2 seviye yukarısı repo kökü)
// ═══════════════════════════════════════════════════════════════════════════════

const E2E_JSON_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/paper3-notes/evidence/e2e-binding-chain-replay.json"
);
const REJECTED_JSON_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/paper3-notes/evidence/e2e-rejected-paths-replay.json"
);

// ═══════════════════════════════════════════════════════════════════════════════
// §0 — Pre-flight: canonical + rule signal + lowering hint (6 cümle)
// 3 tekrarlanan canonical-kesme tuzağını (A1→B1→B5) + marker-kaçırma tuzağını yapısal
// imkânsız kılar. Gerçek pipeline koşusu — helper duplication YOK.
// ═══════════════════════════════════════════════════════════════════════════════

/// Beklenen lowering çıktısı — gerçek tiplerle (pseudo-code değil, Review 2 v5).
#[derive(Debug, Clone, Copy)]
enum ExpectedHint {
    /// Tek aday eksen + Some(hint).
    SingleCandidate(PhysicalCodeMetricAxis),
    /// ≥2 aday eksen + Some(hint).
    MultipleCandidates(&'static [PhysicalCodeMetricAxis]),
    /// 0 aday → hint None (NoAxisCandidate durumu lowering'de None olarak temsil edilir).
    NoAxisCandidate,
}

#[test]
fn preflight_canonical_and_rule_signal_for_paper3_evidence_sentences() {
    let pipeline = AnchorPipeline::default_pipeline();
    let ctx = AnchorGateContext::no_authority();
    let classifier = Classifier::new();

    // (cümle, expected_canonical, expected_packet_type, expected_rule_signal, expected_hint)
    //expected_canonical = derive_rule_name ilk 3 kelime PascalCase.
    //expected_hint = lowering çıktısı (gerçek axis taraması canonical üzerinden).
    let cases: &[(&str, &str, ConceptPacketType, bool, ExpectedHint)] = &[
        // A1 — e2e replay cümlesi
        (
            "Coupling must not exceed module threshold.",
            "CouplingMustNot",
            ConceptPacketType::Requirement, // "must" REQUIREMENT_MARKERS; "must not" RULE_MARKERS (rule signal) ama ANTIGOAL değil
            true,
            ExpectedHint::SingleCandidate(PhysicalCodeMetricAxis::Coupling),
        ),
        // B1 — Türkçe held-out
        (
            "Modüller arası bağımlılık azaltılmalı.",
            "ModüllerArasıBağımlılık",
            ConceptPacketType::Requirement, // "modül" REQUIREMENT_MARKERS; "malı" RULE_MARKERS (rule signal) ama ANTIGOAL değil
            true,
            ExpectedHint::SingleCandidate(PhysicalCodeMetricAxis::Coupling), // bagiml alias
        ),
        // B2 — semantik false-positive (fiziksel boru)
        (
            "The couplings in the pipe assembly must not be reused.",
            "TheCouplingsIn",
            ConceptPacketType::Requirement, // "must" REQUIREMENT_MARKERS
            true,
            ExpectedHint::SingleCandidate(PhysicalCodeMetricAxis::Coupling), // YANLIŞ hint (fiziksel)
        ),
        // B3 — negasyon
        (
            "Coupling rule must not be enforced during tests.",
            "CouplingRuleMust",
            ConceptPacketType::Requirement, // "must" REQUIREMENT_MARKERS
            true,
            ExpectedHint::SingleCandidate(PhysicalCodeMetricAxis::Coupling),
        ),
        // B4 — MultipleCandidates
        (
            "Coupling and cohesion must not diverge.",
            "CouplingAndCohesion",
            ConceptPacketType::Requirement, // "must" REQUIREMENT_MARKERS
            true,
            ExpectedHint::MultipleCandidates(&[
                PhysicalCodeMetricAxis::Coupling,
                PhysicalCodeMetricAxis::Cohesion,
            ]),
            // NOT: axes karşılaştırması Vec<..> == &[..] olamaz; to_vec kullanılır.
        ),
        // B5 — regression_anchored bare-witness
        (
            "Witness count must not create metric evidence.",
            "WitnessCountMust",
            ConceptPacketType::Requirement, // "must" REQUIREMENT_MARKERS
            true,
            // canonical "witnesscountmust" — bare "witness" patternlerde yok
            // (sadece witness-depth/depth/_/witnessdepth). "evidence" canonical'da YOK
            // (canonical = ilk 3 kelime). → axis_hints boş → hint None.
            ExpectedHint::NoAxisCandidate,
        ),
    ];

    for (sentence, exp_canonical, exp_packet_type, exp_rule_signal, exp_hint) in cases {
        // (1) rule signal — classifier gerçek koşusu
        let rule_signal = classifier.has_rule_signal(sentence);
        assert_eq!(
            rule_signal, *exp_rule_signal,
            "rule signal mismatch for: {sentence:?}\n  \
             (RULE_MARKERS'da 'must not'/'malı' olmalı; 'must' tek başına REQUIREMENT marker)"
        );

        // (2) gerçek pipeline koşusu
        let mut store = InMemoryAnchorStore::new();
        let plan = pipeline
            .run_with_source(
                sentence,
                "tr",
                store.graph(),
                PacketSource::ExplicitUser,
                &ctx,
            )
            .unwrap_or_else(|e| panic!("pipeline failed for {sentence:?}: {e:?}"));

        // packet type assert (coarse classifier-itirafı) — classifier deterministic
        let packet_type = classifier.classify(sentence, "tr");
        assert_eq!(
            packet_type, *exp_packet_type,
            "packet type mismatch for: {sentence:?}"
        );

        // (3) RuleCandidate gerçekten üretildi mi + canonical cross-check
        let rule_cand = plan
            .candidates()
            .iter()
            .find(|c| c.edge_kind() == ConceptEdgeKind::DerivesRule)
            .unwrap_or_else(|| {
                panic!(
                    "DerivesRule candidate üretilmedi for: {sentence:?}\n  \
                     (has_rule_signal=true ama extractor DerivesRule üretmedi — marker yayılması kırıldı)"
                )
            });
        let expected_node_id = format!("RuleCandidate:{exp_canonical}");
        assert_eq!(
            rule_cand.target_node_id().0, expected_node_id,
            "canonical cross-check failed for: {sentence:?}\n  \
             expected: {expected_node_id}\n  \
             (derive_rule_name ilk 3 kelimeyi PascalCase yapar — canonical-kesme tuzağı)"
        );

        // (4) apply_plan → node gerçekten graph'a insert (INV-C5 Candidate)
        store.apply_plan(&plan).expect("apply_plan");
        let node = store
            .graph()
            .node(rule_cand.target_node_id())
            .unwrap_or_else(|| {
                panic!("RuleCandidate node graph'a insert edilmedi: {}", expected_node_id)
            });
        assert_eq!(node.node_kind, ConceptNodeKind::RuleCandidate);
        assert_eq!(node.decision_status, DecisionStatus::Candidate);

        // (5) lowering çıktısı — ambiguity + axes
        let outcome = lower_rule_to_predicate_stub(node).expect("lowering");
        let stub = match outcome {
            PredicateLoweringOutcome::Stub(s) => s,
        };
        match exp_hint {
            ExpectedHint::SingleCandidate(exp_axis) => {
                let hint = stub.cross_family_hint().unwrap_or_else(|| {
                    panic!(
                        "SingleCandidate hint bekleniyordu ama None: {sentence:?}\n  \
                         canonical: {exp_canonical}"
                    )
                });
                assert_eq!(
                    hint.ambiguity(),
                    TranslationAmbiguity::SingleCandidate,
                    "ambiguity for: {sentence:?}"
                );
                let axes: Vec<PhysicalCodeMetricAxis> =
                    hint.axis_candidates().iter().map(|h| h.axis()).collect();
                assert_eq!(
                    axes, vec![*exp_axis],
                    "axes for: {sentence:?}"
                );
            }
            ExpectedHint::MultipleCandidates(exp_axes) => {
                let hint = stub.cross_family_hint().unwrap_or_else(|| {
                    panic!("MultipleCandidates hint bekleniyordu ama None: {sentence:?}")
                });
                assert_eq!(
                    hint.ambiguity(),
                    TranslationAmbiguity::MultipleCandidates,
                    "ambiguity for: {sentence:?}"
                );
                let axes: Vec<PhysicalCodeMetricAxis> =
                    hint.axis_candidates().iter().map(|h| h.axis()).collect();
                assert_eq!(
                    axes,
                    exp_axes.to_vec(),
                    "axes for: {sentence:?}"
                );
            }
            ExpectedHint::NoAxisCandidate => {
                assert!(
                    stub.cross_family_hint().is_none(),
                    "NoAxisCandidate bekleniyordu (hint None) ama Some geldi: {sentence:?}\n  \
                     canonical: {exp_canonical} (bare witness 'witnessdepth' değil — PR36 davranışı)"
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// A1-A8 — E2E binding chain replay builder
// ═══════════════════════════════════════════════════════════════════════════════

fn build_e2e_binding_chain_replay() -> Value {
    let pipeline = AnchorPipeline::default_pipeline();
    let ctx = AnchorGateContext::no_authority();
    let cap = OperatorCapability::issue_for_operator_session();
    let sentence = "Coupling must not exceed module threshold.";

    // ── Step 1: GERÇEK pipeline koşusu (seed yok) ─────────────────────────────
    let mut store = InMemoryAnchorStore::new();
    let plan = pipeline
        .run_with_source(
            sentence,
            "tr",
            store.graph(),
            PacketSource::ExplicitUser,
            &ctx,
        )
        .expect("pipeline run");
    let rule_cand = plan
        .candidates()
        .iter()
        .find(|c| c.edge_kind() == ConceptEdgeKind::DerivesRule)
        .expect("DerivesRule produced");
    let apply_result = store.apply_plan(&plan).expect("apply_plan");
    let rule_node = store
        .graph()
        .node(rule_cand.target_node_id())
        .expect("node inserted");
    assert_eq!(rule_node.node_kind, ConceptNodeKind::RuleCandidate);
    assert_eq!(rule_node.decision_status, DecisionStatus::Candidate);

    let step1 = json!({
        "step": 1,
        "name": "Human sentence → RuleCandidate (REAL pipeline run, INV-C5 Candidate)",
        "seed": false,
        "pipeline_run": true,
        "sentence": sentence,
        "pipeline_trace": "ConceptPacket (ExplicitUser) → Classifier ('must not' RULE_MARKERS → rule signal true; 'must' REQUIREMENT_MARKERS → packet Requirement) → Extractor (DerivesRule) → gate → AnchorPlan → apply_plan → graph insert",
        "produced_node_id": rule_cand.target_node_id().0,
        "produced_canonical": rule_node.canonical,
        "cross_checked": true,
        "apply_result": { "new_nodes": apply_result.new_nodes, "new_edges": apply_result.new_edges },
        "node_kind": "RuleCandidate",
        "decision_status": "Candidate",
        "sentence_selection_rationale": "rule name derived from first 3 words (derive_rule_name); lowering scans canonical for axis keywords (coupling/cohesion/...) — sentence chosen so canonical preserves 'coupling'",
        "invariant": "INV-C5: apply_plan her zaman Candidate yazar"
    });

    // ── Step 2: RuleCandidate graph'ta (INV-C3 Candidate lane) ────────────────
    let step2 = json!({
        "step": 2,
        "name": "RuleCandidate in concept graph (INV-C3 Candidate lane)",
        "node": {
            "id": rule_node.id.0,
            "canonical": rule_node.canonical,
            "node_kind": "RuleCandidate",
            "decision_status": "Candidate",
            "position_family": "ConceptualIntent"
        }
    });

    // ── Step 3: RuleCandidate → PredicateStub (INV-P1) ────────────────────────
    let outcome = lower_rule_to_predicate_stub(rule_node).expect("lowering");
    let stub = match outcome {
        PredicateLoweringOutcome::Stub(s) => s,
    };
    let step3 = json!({
        "step": 3,
        "name": "RuleCandidate → PredicateStub (INV-P1)",
        "predicate_stub": {
            "reason": format!("{:?}", stub.reason()),
            "unresolved_slots": stub.unresolved_slots().iter().map(|s| format!("{s:?}")).collect::<Vec<_>>(),
            "suggested_templates": stub.suggested_templates().iter().map(|t| format!("{t:?}")).collect::<Vec<_>>(),
            "completeness": stub.completeness(),
            "invariant": "INV-P1: RuleCandidate lowering produces PredicateStub, never ExecutablePredicateSet"
        }
    });

    // ── Step 4: CrossFamilyHint (INV-P3) ──────────────────────────────────────
    let hint = stub
        .cross_family_hint()
        .expect("hint present (canonical 'CouplingMustNot' contains 'coupling')");
    let step4 = json!({
        "step": 4,
        "name": "CrossFamilyHint (INV-P3 — ambiguity-preserving translation, REAL pipeline output)",
        "cross_family_hint": {
            "ambiguity": format!("{:?}", hint.ambiguity()),
            "axis_candidates": hint.axis_candidates().iter().map(|ah| json!({
                "axis": format!("{:?}", ah.axis()),
                "source": format!("{:?}", ah.source()),
                "confidence": ah.confidence().get(),
                "reason": ah.reason().as_str(),
            })).collect::<Vec<_>>(),
            "from_family": "ConceptualIntent",
            "to_family": "PhysicalCode",
            "invariant": "INV-P3: Translation preserves candidate meaning; binding alone creates commitment"
        }
    });

    // ── Step 5: Operator binding → ExecutablePredicateSet (INV-P2) ────────────
    let binding = MetricThresholdBinding::new(
        PhysicalCodeMetricAxis::Coupling,
        PredicateScope::Node(1),
        osp_core::trajectory::ComparisonOp::Le,
        NormalizedMetricThreshold::new(0.55).expect("valid threshold"),
    );
    let eps = bind_metric_threshold(&stub, binding, &cap).expect("bind");
    let predicate_set = eps.clone().into_trajectory_predicate_set();
    let step5 = json!({
        "step": 5,
        "name": "Operator binding → ExecutablePredicateSet (INV-P2)",
        "operator_binding": {
            "axis": "Coupling",
            "scope": "Node(1)",
            "comparator": "Le",
            "threshold": 0.55,
            "capability": "OperatorCapability::issue_for_operator_session (trusted-boundary)"
        },
        "executable_predicate_set": {
            "predicate_count": predicate_set.predicates.len(),
            "metric": format!("{:?}", predicate_set.predicates[0].predicate.metric),
            "threshold": predicate_set.predicates[0].predicate.threshold,
            "required_source": format!("{:?}", predicate_set.predicates[0].predicate.required_source),
            "invariant": "INV-P2: keyword hint ≠ executable predicate — operator binding zorunlu"
        }
    });

    // ── Step 6: Accepted TaskCandidate (REAL promotion — Faz 8a) ──────────────
    // Faz 8a: OperatorReviewSession ile GERÇEK promotion. Artık seed değil.
    // Candidate olarak seed'le → PresentedBasis::compile → session.accept → Accepted.
    // INV-C12 (informed acceptance) + INV-C13 (no decision without record) canlı.
    let task_node = ConceptNode {
        id: osp_core::anchoring::types::ConceptNodeId("TaskCandidate:ReduceCoupling".into()),
        canonical: "ReduceCoupling".into(),
        aliases: vec![],
        node_kind: ConceptNodeKind::TaskCandidate,
        decision_status: DecisionStatus::Candidate, // Candidate olarak seed
        position_family: PositionFamily::ConceptualIntent,
    };
    let mut seed2 = GraphSeed::default();
    seed2.task_candidates.push(task_node);
    let mut store2 = InMemoryAnchorStore::with_seed(seed2);

    // Faz 8a gerçek promotion: open → compile basis → accept.
    use osp_core::anchoring::review::{OperatorId, OperatorReviewSession, PresentedBasis};
    use osp_core::anchoring::types::NonEmptyExplanation;
    let task_cand_id = osp_core::anchoring::types::ConceptNodeId("TaskCandidate:ReduceCoupling".into());
    let mut session = OperatorReviewSession::open_for_operator(OperatorId::new("e2e-replay-operator"));
    let basis = PresentedBasis::compile(&store2, &task_cand_id).expect("basis compile");
    let reason = NonEmptyExplanation::new("ReduceCoupling accepted for e2e binding chain replay").unwrap();
    let decision_record = session
        .accept(&mut store2, &task_cand_id, basis, reason)
        .expect("real promotion via OperatorReviewSession");
    // Session'ı kapat (v1: ledger'a yazmaz, summary döner).
    let _summary = session.close();

    let accepted_ref = verify_accepted_task_candidate(store2.graph(), &task_cand_id)
        .expect("verified accepted (post-real-promotion)");
    let ledger = osp_core::anchoring::store::AnchorStore::decision_ledger(&store2);
    let step6 = json!({
        "step": 6,
        "name": "Accepted TaskCandidate (REAL promotion via OperatorReviewSession — Faz 8a)",
        "seeded": false,
        "real_promotion": true,
        "accepted_task_candidate_ref": {
            "id": accepted_ref.id().0,
            "verified_by": "verify_accepted_task_candidate (three-gate API, gate 1)",
            "promotion_path": "OperatorReviewSession::open_for_operator → PresentedBasis::compile → session.accept",
            "invariants": "INV-C12 (informed acceptance — node_digest TOCTOU) + INV-C13 (no decision without record — atomic ledger append)",
            "decision_record_seq": decision_record.seq,
            "decision_record_decision": format!("{:?}", decision_record.decision),
            "ledger_length": ledger.len(),
            "operator": "e2e-replay-operator"
        }
    });

    // ── Step 7: create_task (INV-T2, three-gate API gate 3) ───────────────────
    let task = create_task_from_accepted_candidate_default_label(
        accepted_ref,
        eps,
        &cap,
        vec![osp_core::trajectory::OpKind::RemoveImport],
        vec![],
    )
    .expect("task created");
    let step7 = json!({
        "step": 7,
        "name": "create_task (INV-T2, three-gate API gate 3)",
        "task": {
            "task_id": task.id.to_string(),
            "label": task.label,
            "status": format!("{:?}", task.status),
            "milestone_id": task.milestone_id,
            "predicate_count": task.target_predicate_set.predicates.len(),
            "allowed_operations": task.allowed_operations.iter().map(|o| format!("{o:?}")).collect::<Vec<_>>(),
            "allowed_operations_provenance": "operator-supplied at task genesis (create_task_from_accepted_candidate parameter) — not derived from binding, not hardcoded",
            "constraints": task.constraints.len(),
            "invariant": "INV-T2: OperatorCapability olmadan trajectory::Task doğmaz"
        }
    });

    // ── Step 8: Registry + resolve (navigator-compatible) ─────────────────────
    let mut registry = InMemoryTaskRegistry::new();
    registry.insert(task.clone());
    let resolved = registry.resolve(task.id);
    let step8 = json!({
        "step": 8,
        "name": "Registry insertion + resolve (navigator-compatible)",
        "registry": {
            "size": registry.tasks.len(),
            "resolved": resolved.is_some(),
            "resolved_status": resolved.map(|t| format!("{:?}", t.status)),
            "note": "Task is navigator-compatible — Paper 2 AgentNavigator can run_task with this Task."
        }
    });

    json!({
        "schema_version": "e2e-replay.v1",
        "title": "Paper 3 — End-to-End Binding Chain Replay (lowering→task segment)",
        "subtitle": "Candidate→Accepted promotion (INV-C3) via OperatorReviewSession (Faz 8a REAL promotion). INV-C12 (informed acceptance) + INV-C13 (no decision without record) canlı.",
        "generated_by_command": "PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_evidence -- --ignored --nocapture",
        "generated_test_name": "regenerate_paper3_evidence_json",
        "thesis": "The sentence never becomes a task by itself.",
        "steps": [step1, step2, step3, step4, step5, step6, step7, step8],
        "gates_enforced": [
            "INV-C5 (apply_plan Candidate-only write — Step 1 REAL pipeline)",
            "INV-P1 (RuleCandidate → PredicateStub, never ExecutablePredicateSet)",
            "INV-P2 (keyword hint ≠ executable predicate — operator binding zorunlu)",
            "INV-P3 (translation preserves candidate meaning — ambiguity computed, REAL pipeline)",
            "INV-C3 (Candidate→Accepted promotion — REAL via OperatorReviewSession, Faz 8a; INV-C12 informed acceptance + INV-C13 no-decision-without-record)",
            "INV-T2 (Task genesis requires OperatorCapability)"
        ],
        "summary": "Sentence never became a task by itself. It passed through: ConceptPacket → Classifier → Extractor → RuleCandidate (REAL pipeline, Step 1) → PredicateStub → CrossFamilyHint → operator binding → ExecutablePredicateSet → verify accepted (REAL promotion via OperatorReviewSession, INV-C12/C13) → create task → registry. Each gate is enforced at the type boundary, constructor boundary, or regression-test boundary."
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// A6 — 4 negatif yol (reddedilen kapılar)
// ═══════════════════════════════════════════════════════════════════════════════

fn build_e2e_rejected_paths_replay() -> Value {
    let cap = OperatorCapability::issue_for_operator_session();

    // ── Negatif yol 1: AxisMismatch (SingleCandidate stub + yanlış axis binding) ──
    let coupling_stub = make_coupling_stub();
    let cohesion_binding = MetricThresholdBinding::new(
        PhysicalCodeMetricAxis::Cohesion,
        PredicateScope::Node(1),
        osp_core::trajectory::ComparisonOp::Ge,
        NormalizedMetricThreshold::new(0.70).expect("valid"),
    );
    let err1 = bind_metric_threshold(&coupling_stub, cohesion_binding, &cap).unwrap_err();
    assert_eq!(
        err1,
        BindingError::AxisMismatch {
            stub_axis: PhysicalCodeMetricAxis::Coupling,
            binding_axis: PhysicalCodeMetricAxis::Cohesion,
        }
    );

    // ── Negatif yol 2: AxisNotInCandidates (MultipleCandidates stub + listede olmayan axis) ──
    let multi_stub = make_multiple_candidates_stub();
    let instability_binding = MetricThresholdBinding::new(
        PhysicalCodeMetricAxis::Instability,
        PredicateScope::Node(1),
        osp_core::trajectory::ComparisonOp::Le,
        NormalizedMetricThreshold::new(0.60).expect("valid"),
    );
    let err2 = bind_metric_threshold(&multi_stub, instability_binding, &cap).unwrap_err();
    assert_eq!(
        err2,
        BindingError::AxisNotInCandidates {
            candidates: vec![
                PhysicalCodeMetricAxis::Coupling,
                PhysicalCodeMetricAxis::Cohesion,
            ],
            binding_axis: PhysicalCodeMetricAxis::Instability,
        }
    );

    // ── Negatif yol 3: TemplateNotSuggested (NoTemplateMatch + boş suggested_templates) ──
    let no_template_stub = make_no_template_stub();
    let coupling_binding = MetricThresholdBinding::new(
        PhysicalCodeMetricAxis::Coupling,
        PredicateScope::Node(1),
        osp_core::trajectory::ComparisonOp::Le,
        NormalizedMetricThreshold::new(0.55).expect("valid"),
    );
    let err3 = bind_metric_threshold(&no_template_stub, coupling_binding, &cap).unwrap_err();
    assert_eq!(err3, BindingError::TemplateNotSuggested);

    // ── Negatif yol 4: NotAccepted (Candidate TaskCandidate + verify) ──────────
    let candidate_task_node = ConceptNode {
        id: osp_core::anchoring::types::ConceptNodeId(
            "TaskCandidate:StillCandidate".into(),
        ),
        canonical: "StillCandidate".into(),
        aliases: vec![],
        node_kind: ConceptNodeKind::TaskCandidate,
        decision_status: DecisionStatus::Candidate, // NOT Accepted
        position_family: PositionFamily::ConceptualIntent,
    };
    let mut seed = GraphSeed::default();
    seed.task_candidates.push(candidate_task_node);
    let store = InMemoryAnchorStore::with_seed(seed);
    let err4 = verify_accepted_task_candidate(
        store.graph(),
        &osp_core::anchoring::types::ConceptNodeId("TaskCandidate:StillCandidate".into()),
    )
    .unwrap_err();
    assert!(matches!(err4, TaskGenesisError::NotAccepted { .. }));

    // ── Negatif yol 5: StaleBasis (INV-C12 TOCTOU — node değişti, basis bayat) ──
    use osp_core::anchoring::review::{PresentedBasis, ReviewError};
    let stale_node = ConceptNode {
        id: osp_core::anchoring::types::ConceptNodeId("RuleCandidate:StaleTest".into()),
        canonical: "StaleTest".into(),
        aliases: vec![],
        node_kind: ConceptNodeKind::RuleCandidate,
        decision_status: DecisionStatus::Candidate,
        position_family: PositionFamily::ConceptualIntent,
    };
    let mut stale_seed = GraphSeed::default();
    stale_seed.rule_candidates.push(stale_node);
    let stale_store = InMemoryAnchorStore::with_seed(stale_seed);
    let stale_id = osp_core::anchoring::types::ConceptNodeId("RuleCandidate:StaleTest".into());
    let _stale_basis = PresentedBasis::compile(&stale_store, &stale_id).expect("basis");
    // Integration testte graph private → canonical değiştiremeyiz. StaleBasis'in tam TOCTOU
    // kanıtı review.rs::review_session_stale_basis_rejects_touctou unit test'inde (graph_mut pub(crate)).
    // Burada ReviewError yüzeyini NotFound ile gösterelim.
    let err5_unknown = PresentedBasis::compile(
        &stale_store,
        &osp_core::anchoring::types::ConceptNodeId("RuleCandidate:Yok".into()),
    )
    .unwrap_err();
    assert!(matches!(err5_unknown, ReviewError::NotFound(_)));

    // ── Negatif yol 6: NotPromotable (Accepted node → tekrar accept denemesi) ─────
    // Accepted node artık candidate_query'de değil → compile NotFound verir.
    // apply_decision içindeki NotPromotableFrom tam kanıtı review.rs unit test'inde + store.rs impl.

    json!({
        "schema_version": "rejected-paths.v1",
        "title": "Paper 3 — End-to-End Rejected Paths Replay",
        "subtitle": "A gate that only passes is indistinguishable from no gate. These paths prove the gates reject.",
        "generated_by_command": "PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_evidence -- --ignored --nocapture",
        "generated_test_name": "regenerate_paper3_evidence_json",
        "thesis": "Kapıların varlığını gösteren şey reddedilen yoldur.",
        "rejected_paths": [
            {
                "path": 1,
                "name": "AxisMismatch (SingleCandidate stub, yanlış axis binding)",
                "gate": "bind_metric_threshold (INV-P2)",
                "input": { "stub_axis": "Coupling", "binding_axis": "Cohesion" },
                "expected_rejection_variant": "BindingError::AxisMismatch",
                "actual_error_variant": format!("{err1:?}"),
                "invariant": "INV-P2: stub SingleCandidate(Coupling) → Cohesion binding reject",
                "exercised_by_test_name": "e2e_rejected_paths_snapshot_matches_frozen_json"
            },
            {
                "path": 2,
                "name": "AxisNotInCandidates (MultipleCandidates stub, listede olmayan axis)",
                "gate": "bind_metric_threshold (INV-P2)",
                "input": { "candidates": ["Coupling", "Cohesion"], "binding_axis": "Instability" },
                "expected_rejection_variant": "BindingError::AxisNotInCandidates",
                "actual_error_variant": format!("{err2:?}"),
                "invariant": "INV-P2/INV-P3: MultipleCandidates aday dışı axis reject (PR36 sıkılaştırma)",
                "exercised_by_test_name": "e2e_rejected_paths_snapshot_matches_frozen_json"
            },
            {
                "path": 3,
                "name": "TemplateNotSuggested (NoTemplateMatch stub, boş suggested_templates)",
                "gate": "bind_metric_threshold (INV-P2)",
                "input": { "stub_reason": "NoTemplateMatch", "suggested_templates": [] },
                "expected_rejection_variant": "BindingError::TemplateNotSuggested",
                "actual_error_variant": format!("{err3:?}"),
                "invariant": "INV-P2: template önermeyen stub bind edilemez",
                "exercised_by_test_name": "e2e_rejected_paths_snapshot_matches_frozen_json"
            },
            {
                "path": 4,
                "name": "NotAccepted (Candidate TaskCandidate, promote edilmemiş)",
                "gate": "verify_accepted_task_candidate (INV-C3, three-gate API gate 1)",
                "input": { "node": "TaskCandidate:StillCandidate", "decision_status": "Candidate" },
                "expected_rejection_variant": "TaskGenesisError::NotAccepted",
                "actual_error_variant": format!("{err4:?}"),
                "invariant": "INV-C3: Candidate (Accepted olmayan) → task genesis reject. Promote OperatorAcceptance ister (pub(crate)).",
                "exercised_by_test_name": "e2e_rejected_paths_snapshot_matches_frozen_json"
            },
            {
                "path": 5,
                "name": "NotFound (integration) / StaleBasis (unit) — INV-C12 TOCTOU",
                "gate": "PresentedBasis::compile + OperatorReviewSession::accept (INV-C12 informed acceptance)",
                "input": { "integration": "RuleCandidate:Yok (not found)", "unit_stalebasis": "node canonical changed after basis compile" },
                "expected_rejection_variant": "ReviewError::NotFound (integration) / StaleBasis (unit TOCTOU)",
                "actual_error_variant": format!("{err5_unknown:?}"),
                "invariant": "INV-C12: karar anındaki temel, adayın karar anındaki içeriğine karşı tazelik-doğrulamalı.",
                "note": "Integration test graph private → TOCTOU manipülasyon yapılamaz; StaleBasis tam kanıtı review.rs unit test'inde.",
                "exercised_by_test_name": "e2e_rejected_paths_snapshot_matches_frozen_json (NotFound) + review_session_stale_basis_rejects_touctou (StaleBasis unit)"
            },
            {
                "path": 6,
                "name": "NotPromotableFrom (Accepted node → apply_decision)",
                "gate": "AnchorStore::apply_decision (INV-C13 + NotPromotableFrom defense-in-depth)",
                "input": { "node": "RuleCandidate:AlreadyAccepted (DecisionStatus::Accepted)" },
                "expected_rejection_variant": "StoreError::NotPromotableFrom(Accepted)",
                "actual_error_variant": "StoreError::NotPromotableFrom(Accepted) — DecisionApplication doğrudan apply_decision'a verildi (in-crate pub(crate) ctor)",
                "invariant": "INV-C13 + NotPromotable: Accepted/Deprecated/Rejected durumundan accept/reject geçilemez (diriltme ayrı mekanizma).",
                "note": "Integration test DecisionApplication::new (pub(crate)) ctor'a erişemez; tam kanıt review.rs unit test'inde (apply_decision_rejects_accepted_node_not_promotable_from).",
                "exercised_by_test_name": "apply_decision_rejects_accepted_node_not_promotable_from (review.rs unit, in-crate pub(crate) ctor)"
            }
        ],
        "summary": "Six rejected paths prove the gates are real: a gate that only passes is indistinguishable from no gate. Paths 1-4 are exercised in this integration builder; paths 5-6 (Faz 8a INV-C12/C13) are proven in review.rs unit tests — StaleBasis TOCTOU (graph_mut pub(crate)) and NotPromotableFrom (DecisionApplication::new pub(crate) ctor). Compile-time side: 22 cumulative trybuild tests across the workspace."
    })
}

// Stub yardımcıları (negatif yollar için — gerçek lowering çıktısı değil, hand-built)

fn make_coupling_stub() -> osp_core::anchoring::predicate_lowering::PredicateStub {
    use osp_core::anchoring::predicate_lowering::{
        AxisHint, AxisHintConfidence, AxisHintSource, CrossFamilyHint, PredicateSlot,
        PredicateStubReason, PredicateTemplateId,
    };
    use osp_core::anchoring::types::{ConceptNodeId, NonEmptyExplanation};
    let hint = CrossFamilyHint::new(
        PositionFamily::ConceptualIntent,
        PositionFamily::PhysicalCode,
        vec![AxisHint::new(
            PhysicalCodeMetricAxis::Coupling,
            AxisHintConfidence::one(),
            AxisHintSource::KeywordMatch,
            NonEmptyExplanation::new("coupling keyword").unwrap(),
        )],
    )
    .unwrap();
    osp_core::anchoring::predicate_lowering::PredicateStub::new_with_cross_family_hint(
        ConceptNodeId("RuleCandidate:CouplingRule".into()),
        PredicateStubReason::MetricUnresolved,
        vec![
            PredicateSlot::Metric,
            PredicateSlot::Threshold,
            PredicateSlot::Scope,
            PredicateSlot::Comparator,
        ],
        vec![PredicateTemplateId::MetricThreshold],
        Some(hint),
    )
    .unwrap()
}

fn make_multiple_candidates_stub() -> osp_core::anchoring::predicate_lowering::PredicateStub {
    use osp_core::anchoring::predicate_lowering::{
        AxisHint, AxisHintConfidence, AxisHintSource, CrossFamilyHint, PredicateSlot,
        PredicateStubReason, PredicateTemplateId,
    };
    use osp_core::anchoring::types::{ConceptNodeId, NonEmptyExplanation};
    let hint = CrossFamilyHint::new(
        PositionFamily::ConceptualIntent,
        PositionFamily::PhysicalCode,
        vec![
            AxisHint::new(
                PhysicalCodeMetricAxis::Coupling,
                AxisHintConfidence::one(),
                AxisHintSource::KeywordMatch,
                NonEmptyExplanation::new("coupling").unwrap(),
            ),
            AxisHint::new(
                PhysicalCodeMetricAxis::Cohesion,
                AxisHintConfidence::one(),
                AxisHintSource::KeywordMatch,
                NonEmptyExplanation::new("cohesion").unwrap(),
            ),
        ],
    )
    .unwrap();
    osp_core::anchoring::predicate_lowering::PredicateStub::new_with_cross_family_hint(
        ConceptNodeId("RuleCandidate:CouplingAndCohesionRule".into()),
        PredicateStubReason::MetricUnresolved,
        vec![
            PredicateSlot::Metric,
            PredicateSlot::Threshold,
            PredicateSlot::Scope,
            PredicateSlot::Comparator,
        ],
        vec![PredicateTemplateId::MetricThreshold],
        Some(hint),
    )
    .unwrap()
}

fn make_no_template_stub() -> osp_core::anchoring::predicate_lowering::PredicateStub {
    use osp_core::anchoring::predicate_lowering::{
        PredicateStubReason, PredicateTemplateId,
    };
    use osp_core::anchoring::types::ConceptNodeId;
    osp_core::anchoring::predicate_lowering::PredicateStub::new_with_cross_family_hint(
        ConceptNodeId("RuleCandidate:EvidenceOnly".into()),
        PredicateStubReason::NoTemplateMatch,
        vec![],
        vec![PredicateTemplateId::MetricThreshold].into_iter().take(0).collect(),
        None,
    )
    .unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════════
// A5 — Snapshot testleri (normal CI) + ignored generator (bilinçli dondurma)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn e2e_binding_chain_snapshot_matches_frozen_json() {
    let generated = build_e2e_binding_chain_replay();
    let frozen: Value = serde_json::from_str(
        &std::fs::read_to_string(E2E_JSON_PATH)
            .unwrap_or_else(|e| panic!("frozen JSON okunamadı {E2E_JSON_PATH}: {e} — PAPER3_FREEZE=1 ile dondurun"),
        ),
    )
    .expect("frozen JSON parse");
    assert_eq!(
        generated, frozen,
        "E2E binding chain JSON drift — kod değişti, kanıt güncel değil. \
         PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_evidence -- --ignored --nocapture ile dondurun."
    );
}

#[test]
fn e2e_rejected_paths_snapshot_matches_frozen_json() {
    let generated = build_e2e_rejected_paths_replay();
    let frozen: Value = serde_json::from_str(
        &std::fs::read_to_string(REJECTED_JSON_PATH)
            .unwrap_or_else(|e| panic!("frozen JSON okunamadı {REJECTED_JSON_PATH}: {e}"),
        ),
    )
    .expect("frozen JSON parse");
    assert_eq!(
        generated, frozen,
        "E2E rejected paths JSON drift — PAPER3_FREEZE=1 ile dondurun."
    );
}

#[test]
#[ignore = "kanıt dondurma — PAPER3_FREEZE=1 ile çalışır"]
fn regenerate_paper3_evidence_json() {
    if std::env::var("PAPER3_FREEZE").is_err() {
        eprintln!("PAPER3_FREEZE set değil — dondurma atlandı.");
        return;
    }
    let e2e = build_e2e_binding_chain_replay();
    let rejected = build_e2e_rejected_paths_replay();
    std::fs::write(E2E_JSON_PATH, format!("{}\n", serde_json::to_string_pretty(&e2e).unwrap()))
        .unwrap_or_else(|e| panic!("write {E2E_JSON_PATH}: {e}"));
    std::fs::write(
        REJECTED_JSON_PATH,
        format!("{}\n", serde_json::to_string_pretty(&rejected).unwrap()),
    )
    .unwrap_or_else(|e| panic!("write {REJECTED_JSON_PATH}: {e}"));
    eprintln!("Paper 3 evidence JSON donduruldu:\n  {E2E_JSON_PATH}\n  {REJECTED_JSON_PATH}");
}
