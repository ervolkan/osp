//! Paper 3 held-out/adversarial fixture set — pipeline-level conformance.
//!
//! 5 cümle (4 held_out + 1 regression_anchored). Geliştirmede kullanılmamış
//! (held_out) veya explicitly regression-anchored. RQ1 (golden fixture conformance)
//! totoloji olmasın diye.
//!
//! # Snapshot disiplini (A5 — paper3_evidence.rs ile aynı)
//! Normal CI testi donmuş JSON ile `assert_eq!` — source tree'yi mutate etmez.
//! `PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_heldout -- --ignored --nocapture`
//! ile bilinçli dondurma.
//!
//! # 5-state conformance (Review 2 v4)
//! `Conform` / `PartialConform` / `KnownLimitation` / `RejectAsExpected` / `UnexpectedFailure`.
//! Non-conform/KnownLimitation çıkan fixture SİLİNMEZ — RQ1 inandırıcılığı oradan gelir.

use osp_core::anchoring::classifier::Classifier;
use osp_core::anchoring::gate::AnchorGateContext;
use osp_core::anchoring::pipeline::AnchorPipeline;
use osp_core::anchoring::predicate_lowering::{
    lower_rule_to_predicate_stub, PhysicalCodeMetricAxis, PredicateLoweringOutcome,
    TranslationAmbiguity,
};
use osp_core::anchoring::store::InMemoryAnchorStore;
use osp_core::anchoring::types::PacketSource;
use osp_core::anchoring::{ConceptEdgeKind, DecisionStatus};
use serde_json::{json, Value};

const HELDOUT_JSON_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/paper3-notes/evidence/held-out-adversarial-fixtures.json"
);

/// Tek held-out cümle için pipeline koşusu + lowering + beklenti karşılaştırması.
struct HeldOutCase {
    id: &'static str,
    category: &'static str,
    language: &'static str,
    sentence: &'static str,
    expected_canonical: &'static str,
    expected_packet_type: &'static str,
    expected_ambiguity: Option<TranslationAmbiguity>,
    expected_axes: &'static [PhysicalCodeMetricAxis],
    held_out_or_regression_reason: &'static str,
    expected_conformance_state: &'static str,
    /// extra context for known limitations / threats
    note: Option<&'static str>,
}

fn held_out_cases() -> Vec<HeldOutCase> {
    vec![
        HeldOutCase {
            id: "held_001",
            category: "held_out",
            language: "tr",
            sentence: "Modüller arası bağımlılık azaltılmalı.",
            expected_canonical: "ModüllerArasıBağımlılık",
            expected_packet_type: "Requirement", // "modül" REQUIREMENT_MARKERS'da; "malı" RULE_MARKERS'da (rule signal) ama ANTIGOAL değil
            expected_ambiguity: Some(TranslationAmbiguity::SingleCandidate),
            expected_axes: &[PhysicalCodeMetricAxis::Coupling],
            held_out_or_regression_reason:
                "Held-out sette sıfır Türkçe; RQ2 (iki dilli lowering) totoloji-olmayan kanıt. \
                 TR alias chain (bağımlılık → bagiml → Coupling) + TR rule marker (malı).",
            expected_conformance_state: "Conform",
            note: None,
        },
        HeldOutCase {
            id: "held_002",
            category: "held_out",
            language: "en",
            sentence: "The couplings in the pipe assembly must not be reused.",
            expected_canonical: "TheCouplingsIn",
            expected_packet_type: "Requirement",
            expected_ambiguity: Some(TranslationAmbiguity::SingleCandidate),
            expected_axes: &[PhysicalCodeMetricAxis::Coupling],
            held_out_or_regression_reason:
                "Semantik false-positive: 'couplings' fiziksel boru, yazılım metric değil. \
                 Matcher Coupling hint üretir (YANLIŞ).",
            expected_conformance_state: "KnownLimitation",
            note: Some(
                "The deterministic matcher can confuse lexical coupling with software coupling. \
                 The matcher is not the contribution; the binding protocol is.",
            ),
        },
        HeldOutCase {
            id: "held_003",
            category: "held_out",
            language: "en",
            sentence: "Coupling rule must not be enforced during tests.",
            expected_canonical: "CouplingRuleMust",
            expected_packet_type: "Requirement",
            expected_ambiguity: Some(TranslationAmbiguity::SingleCandidate),
            expected_axes: &[PhysicalCodeMetricAxis::Coupling],
            held_out_or_regression_reason:
                "Negasyon vakası: 'must not' rule marker + coupling axis, ama anlam NEGATİF \
                 (rule devre dışı). Coarse classifier negasyon yakalayamaz.",
            expected_conformance_state: "KnownLimitation",
            note: Some("Coarse classifier negation yakalayamaz; RuleCandidate yine üretilir. Negation-aware lowering Faz 6."),
        },
        HeldOutCase {
            id: "held_004",
            category: "held_out",
            language: "en",
            sentence: "Coupling and cohesion must not diverge.",
            expected_canonical: "CouplingAndCohesion",
            expected_packet_type: "Requirement",
            expected_ambiguity: Some(TranslationAmbiguity::MultipleCandidates),
            expected_axes: &[
                PhysicalCodeMetricAxis::Coupling,
                PhysicalCodeMetricAxis::Cohesion,
            ],
            held_out_or_regression_reason:
                "MultipleCandidates: canonical hem coupling hem cohesion → ambiguity Multiple. \
                 A6 negatif yol #2 (AxisNotInCandidates) ile ortak.",
            expected_conformance_state: "Conform",
            note: None,
        },
        HeldOutCase {
            id: "held_005",
            category: "regression_anchored",
            language: "en",
            sentence: "Witness count must not create metric evidence.",
            expected_canonical: "WitnessCountMust",
            expected_packet_type: "Requirement",
            // bare 'witness' → NoAxisCandidate (hint None, lowering cross_family_hint None)
            expected_ambiguity: None, // None = hint None (PR36 bare-witness exclusion)
            expected_axes: &[],
            held_out_or_regression_reason:
                "REGRESSION-ANCHORED (held_out DEĞİL): PR36 bare-witness testinin pipeline tekrarı. \
                 canonical WitnessCountMust → bare witness var, witnessdepth yok → NoAxisCandidate.",
            expected_conformance_state: "Conform",
            note: Some("PR36 davranışı: bare witness WitnessDepth axis üretmez (witnessdepth ayrımı)."),
        },
    ]
}

/// Tek case için pipeline koşusu + lowering + assert. JSON değerini döndürür.
fn run_held_out_case(case: &HeldOutCase) -> Value {
    let pipeline = AnchorPipeline::default_pipeline();
    let ctx = AnchorGateContext::no_authority();
    let classifier = Classifier::new();

    // (1) rule signal
    let rule_signal = classifier.has_rule_signal(case.sentence);
    assert!(
        rule_signal,
        "[{}] has_rule_signal false beklenmiyordu: {:?}",
        case.id, case.sentence
    );

    // (2) gerçek pipeline koşusu
    let mut store = InMemoryAnchorStore::new();
    let plan = pipeline
        .run_with_source(
            case.sentence,
            case.language,
            store.graph(),
            PacketSource::ExplicitUser,
            &ctx,
        )
        .unwrap_or_else(|e| panic!("[{}] pipeline failed: {e:?}", case.id));

    // (3) packet type
    let packet_type = classifier.classify(case.sentence, case.language);
    let packet_type_str = format!("{packet_type:?}");
    assert_eq!(
        packet_type_str, case.expected_packet_type,
        "[{}] packet type mismatch",
        case.id
    );

    // (4) DerivesRule + canonical
    let rule_cand = plan
        .candidates()
        .iter()
        .find(|c| c.edge_kind() == ConceptEdgeKind::DerivesRule)
        .unwrap_or_else(|| panic!("[{}] DerivesRule üretilmedi", case.id));
    let expected_node_id = format!("RuleCandidate:{}", case.expected_canonical);
    assert_eq!(
        rule_cand.target_node_id().0, expected_node_id,
        "[{}] canonical cross-check",
        case.id
    );

    // (5) apply_plan + node insert
    store.apply_plan(&plan).expect("apply_plan");
    let node = store
        .graph()
        .node(rule_cand.target_node_id())
        .expect("node inserted");
    assert_eq!(node.decision_status, DecisionStatus::Candidate);

    // (6) lowering + ambiguity/axes
    let outcome = lower_rule_to_predicate_stub(node).expect("lowering");
    let stub = match outcome {
        PredicateLoweringOutcome::Stub(s) => s,
    };
    let (ambiguity_str, axes_str, hint_present) = match case.expected_ambiguity {
        Some(exp_ambig) => {
            let hint = stub.cross_family_hint().unwrap_or_else(|| {
                panic!(
                    "[{}] hint bekleniyordu ama None: {:?}",
                    case.id, case.sentence
                )
            });
            assert_eq!(
                hint.ambiguity(),
                exp_ambig,
                "[{}] ambiguity mismatch",
                case.id
            );
            let axes: Vec<PhysicalCodeMetricAxis> =
                hint.axis_candidates().iter().map(|h| h.axis()).collect();
            assert_eq!(
                axes, case.expected_axes,
                "[{}] axes mismatch",
                case.id
            );
            (format!("{:?}", exp_ambig), format!("{:?}", axes), true)
        }
        None => {
            assert!(
                stub.cross_family_hint().is_none(),
                "[{}] hint None bekleniyordu ama Some geldi (bare-witness exclusion kırıldı): {:?}",
                case.id,
                case.sentence
            );
            ("NoAxisCandidate (hint None)".to_string(), "[]".to_string(), false)
        }
    };

    json!({
        "id": case.id,
        "category": case.category,
        "language": case.language,
        "sentence": case.sentence,
        "held_out_or_regression_reason": case.held_out_or_regression_reason,
        "invariants": ["INV-P3", "INV-P1"],
        "expected_pipeline_behavior": {
            "rule_signal": rule_signal,
            "rule_marker_expected": if case.sentence.to_lowercase().contains("must not") { "must not (RULE_MARKERS)" }
                else if case.sentence.to_lowercase().contains("malı") { "malı (RULE_MARKERS)" }
                else { "unknown" },
            "rule_marker_provenance": "test-side heuristic (classifier does not expose which marker matched; Faz 6 calibration)",
            "packet_type": packet_type_str,
            "produced_canonical": case.expected_canonical,
            "derives_rule": true
        },
        "expected_lowering_behavior": {
            "ambiguity": ambiguity_str,
            "axes": axes_str,
            "cross_family_hint_present": hint_present
        },
        "expected_conformance_state": case.expected_conformance_state,
        "note": case.note,
    })
}

fn build_held_out_fixtures_json() -> Value {
    let cases = held_out_cases();
    let fixtures: Vec<Value> = cases.iter().map(run_held_out_case).collect();

    // 5-state dağılımı
    let mut conform = 0u32;
    let mut known_limitation = 0u32;
    for f in &fixtures {
        match f["expected_conformance_state"].as_str() {
            Some("Conform") => conform += 1,
            Some("KnownLimitation") => known_limitation += 1,
            _ => {}
        }
    }

    json!({
        "schema_version": "held-out.v1",
        "title": "Paper 3 — Held-out / Adversarial Fixture Set",
        "description": "Geliştirmede kullanılmamış (veya explicitly regression-anchored) cümleler. RQ1 totoloji olmasın diye. Beklentiler ikiye split: expected_pipeline_behavior + expected_lowering_behavior.",
        "generated_by_command": "PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_heldout -- --ignored --nocapture",
        "generated_test_name": "regenerate_paper3_heldout_json",
        "methodology_note": "Cümle seçimi 'canonical-kesme tuzağı' (derive_rule_name ilk 3 kelime → lowering canonical üzerinden axis tarar) ve 'marker-kaçırma tuzağı' (must vs must not) göz önünde tutularak yapıldı. Her cümle paper3_evidence.rs::preflight testinde gerçek pipeline koşusuyla pinlenmiştir.",
        "fixtures": fixtures,
        "summary": {
            "total": fixtures.len(),
            "held_out": fixtures.iter().filter(|f| f["category"] == "held_out").count(),
            "regression_anchored": fixtures.iter().filter(|f| f["category"] == "regression_anchored").count(),
            "languages": {
                "tr": fixtures.iter().filter(|f| f["language"] == "tr").count(),
                "en": fixtures.iter().filter(|f| f["language"] == "en").count(),
            },
            "expected_conformance_states": {
                "Conform": conform,
                "KnownLimitation": known_limitation,
                "PartialConform": 0,
                "RejectAsExpected": 0,
                "UnexpectedFailure": 0
            },
            "design_discipline": "Her cümle 'canonical-kesme' ve 'marker-kaçırma' tuzaklarından kaçacak şekilde seçildi. paper3_evidence.rs::preflight testinde pinlenmiştir."
        }
    })
}

#[test]
fn held_out_snapshot_matches_frozen_json() {
    let generated = build_held_out_fixtures_json();
    let frozen: Value = serde_json::from_str(
        &std::fs::read_to_string(HELDOUT_JSON_PATH).unwrap_or_else(|e| {
            panic!("frozen JSON okunamadı {HELDOUT_JSON_PATH}: {e} — PAPER3_FREEZE=1 ile dondurun")
        }),
    )
    .expect("frozen JSON parse");
    assert_eq!(
        generated, frozen,
        "held-out JSON drift — PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_heldout -- --ignored --nocapture ile dondurun."
    );
}

#[test]
#[ignore = "kanıt dondurma — PAPER3_FREEZE=1 ile çalışır"]
fn regenerate_paper3_heldout_json() {
    if std::env::var("PAPER3_FREEZE").is_err() {
        eprintln!("PAPER3_FREEZE set değil — dondurma atlandı.");
        return;
    }
    let v = build_held_out_fixtures_json();
    std::fs::write(
        HELDOUT_JSON_PATH,
        format!("{}\n", serde_json::to_string_pretty(&v).unwrap()),
    )
    .unwrap_or_else(|e| panic!("write {HELDOUT_JSON_PATH}: {e}"));
    eprintln!("Paper 3 held-out JSON donduruldu:\n  {HELDOUT_JSON_PATH}");
}
