//! Concept Anchoring — golden fixture loader + format/teorik-doğrulama testleri.
//!
//! **Faz 0** (Paper 3, [`docs/concept-anchoring-design.md`] §11). Bu testler hiçbir
//! resolver/classifier çağırmaz — sadece fixture JSON formatının ve teorik referanslarının
//! (INV-C1..C8, D1-D13, §8.x) geçerliliğini doğrular. Faz 1 in-memory MVP geldiğinde
//! aynı loader kalır, `expected` assert'leri eklenir.
//!
//! ## Disiplin
//! Paper 1/2 "önce spec, sonra kod" — mekanizmayı kodlamadan önce teoriyi testlerle
//! dondur. Bu 10 golden fixture, Paper 3 eval metodolojisinin çekirdeği (Q11).
//!
//! ## Fixture struct'ları neden burada (core'da değil)?
//! `AnchoringFixture`/`FixtureGiven`/`FixtureExpected` runtime domain modeli değil,
//! test schema modelidir. Core API'yi fixture detaylarıyla şişirmemek için private
//! tutulur (değerlendirme "Seçenek B"). Runtime domain enum'ları [`osp_core::anchoring`]'da.

#![allow(dead_code)] // fixture struct alanları bazı fixture'larda boş kalır (örn. fix_010 edges)

use osp_core::anchoring::{
    AnchorDecisionKind, ConceptEdgeKind, ConceptPacketType, PositionFamily, ThresholdBand,
};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Fixture şema struct'ları (private, Serialize+Deserialize+PartialEq — roundtrip için)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct AnchoringFixture {
    schema_version: String,
    id: String,
    invariants: Vec<String>,
    given: FixtureGiven,
    input: FixtureInput,
    expected: FixtureExpected,
    score: FixtureScore,
    notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FixtureGiven {
    #[serde(default)]
    concepts: Vec<GivenConcept>,
    #[serde(default)]
    decisions: Vec<GivenDecision>,
    #[serde(default)]
    code_entities: Vec<GivenCodeEntity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct GivenConcept {
    id: String,
    canonical: String,
    aliases: Vec<String>,
    decision_status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct GivenDecision {
    id: String,
    status: String,
    authority: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct GivenCodeEntity {
    id: String,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FixtureInput {
    text: String,
    language: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FixtureExpected {
    packet_type: ConceptPacketType,
    concepts: Vec<String>,
    edges: Vec<FixtureEdge>,
    decision_status: String,
    position_family: PositionFamily,
    requires_operator_review: bool,
    anchor_decision: AnchorDecisionKind,
    threshold_band: ThresholdBand,
    #[serde(default)]
    negative_assertions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FixtureEdge {
    kind: ConceptEdgeKind,
    target: String,
    requires_explanation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FixtureScore {
    /// Yorum alanı — serde skip değil, deserialize edilir ama test edilmez.
    _comment: Option<String>,
    semantic_similarity: Option<f64>,
    ontology_type_compatibility: Option<f64>,
    graph_context_score: Option<f64>,
    domain_term_match: Option<f64>,
    code_evidence_score: Option<f64>,
    temporal_trust_score: Option<f64>,
    decision_status_score: Option<f64>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Glossary şema struct'ları
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Glossary {
    schema_version: String,
    entries: Vec<GlossaryEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct GlossaryEntry {
    canonical: String,
    aliases: Vec<String>,
    language: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Loader — include_str! (runtime I/O yok, projenin fixture I/O'dan kaçınma kuralı)
// ═══════════════════════════════════════════════════════════════════════════════

/// 10 golden fixture — sabit liste (fixture ekle/çıkarma bilinçli görünür).
const FIXTURE_FILES: &[(&str, &str)] = &[
    (
        "fix_001_payment_trust_vision",
        include_str!("fixtures/anchoring/fix_001_payment_trust_vision.json"),
    ),
    (
        "fix_002_requirement_rule",
        include_str!("fixtures/anchoring/fix_002_requirement_rule.json"),
    ),
    (
        "fix_003_arch_decision",
        include_str!("fixtures/anchoring/fix_003_arch_decision.json"),
    ),
    (
        "fix_004_antigoal",
        include_str!("fixtures/anchoring/fix_004_antigoal.json"),
    ),
    (
        "fix_005_assumption_only",
        include_str!("fixtures/anchoring/fix_005_assumption_only.json"),
    ),
    (
        "fix_006_direct_code_ref",
        include_str!("fixtures/anchoring/fix_006_direct_code_ref.json"),
    ),
    (
        "fix_007_contradiction",
        include_str!("fixtures/anchoring/fix_007_contradiction.json"),
    ),
    (
        "fix_008_dedup_alias",
        include_str!("fixtures/anchoring/fix_008_dedup_alias.json"),
    ),
    (
        "fix_009_weak_anchor_canon",
        include_str!("fixtures/anchoring/fix_009_weak_anchor_canon.json"),
    ),
    (
        "fix_010_unanchored",
        include_str!("fixtures/anchoring/fix_010_unanchored.json"),
    ),
];

const GLOSSARY_JSON: &str = include_str!("fixtures/anchoring/_glossary.json");

fn load_all_fixtures() -> Vec<AnchoringFixture> {
    FIXTURE_FILES
        .iter()
        .map(|(_, raw)| serde_json::from_str(raw).expect("fixture parse"))
        .collect()
}

/// High-stake edge kümesi — INV-C7/§8.4 (10 edge, v0.2.1 DerivesRisk dahil).
fn high_stake_kinds() -> Vec<ConceptEdgeKind> {
    use ConceptEdgeKind::*;
    vec![
        DerivesRule,
        DerivesTask,
        DerivesRisk,
        Constrains,
        ExpectedImplementation,
        ImplementedBy,
        EvidencedBy,
        Contradicts,
        Supersedes,
        AntiGoalOf,
    ]
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 1 — tüm fixture'lar parse
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_all_fixtures_parse_successfully() {
    let fixtures = load_all_fixtures();
    assert_eq!(fixtures.len(), 10, "Faz 0 spec: 10 golden fixture");
    for f in &fixtures {
        assert_eq!(
            f.schema_version, "anchoring.fixture.v1",
            "fixture {} schema_version uyumsuz",
            f.id
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 2 — glossary parse + validasyon
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_glossary_parse_successfully() {
    let g: Glossary = serde_json::from_str(GLOSSARY_JSON).expect("glossary parse");
    assert_eq!(g.schema_version, "anchoring.glossary.v1");
    assert!(!g.entries.is_empty(), "glossary boş olamaz");

    let mut canonicals = std::collections::HashSet::new();
    for e in &g.entries {
        assert!(
            canonicals.insert(&e.canonical),
            "glossary canonical tekrarlı: {}",
            e.canonical
        );
        assert!(
            !e.aliases.is_empty(),
            "glossary {} alias'ları boş",
            e.canonical
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 3 — id benzersiz
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_ids_unique() {
    let fixtures = load_all_fixtures();
    let mut ids = std::collections::HashSet::new();
    for f in &fixtures {
        assert!(ids.insert(&f.id), "fixture id tekrarlı: {}", f.id);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 4 — invariants sadece INV-C1..C8
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_invariants_reference_valid() {
    let valid: std::collections::HashSet<&str> = (1..=8)
        .map(|i| match i {
            1 => "INV-C1",
            2 => "INV-C2",
            3 => "INV-C3",
            4 => "INV-C4",
            5 => "INV-C5",
            6 => "INV-C6",
            7 => "INV-C7",
            _ => "INV-C8",
        })
        .collect();
    for f in load_all_fixtures() {
        for inv in &f.invariants {
            assert!(
                valid.contains(inv.as_str()),
                "fixture {} geçersiz invariant referansı: {} (sadece INV-C1..C8)",
                f.id,
                inv
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 5 — edge kind'leri ConceptEdgeKind 15 varyantından
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_edges_valid_kinds() {
    for f in load_all_fixtures() {
        for e in &f.expected.edges {
            // kind deserialize edildiğinde zaten ConceptEdgeKind olmuş; ekstra kontrol:
            assert!(
                matches!(
                    e.kind,
                    ConceptEdgeKind::Mentions
                        | ConceptEdgeKind::Refines
                        | ConceptEdgeKind::DerivesRule
                        | ConceptEdgeKind::DerivesTask
                        | ConceptEdgeKind::DerivesRisk
                        | ConceptEdgeKind::Constrains
                        | ConceptEdgeKind::ExpectedImplementation
                        | ConceptEdgeKind::ImplementedBy
                        | ConceptEdgeKind::EvidencedBy
                        | ConceptEdgeKind::Contradicts
                        | ConceptEdgeKind::Supersedes
                        | ConceptEdgeKind::RelatedTo
                        | ConceptEdgeKind::AntiGoalOf
                        | ConceptEdgeKind::DependsOnDecision
                        | ConceptEdgeKind::HasPosition
                ),
                "fixture {} bilinmeyen edge kind",
                f.id
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 6 — packet_type ConceptPacketType 7 varyantından (AntiGoal dahil)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_packet_types_valid() {
    for f in load_all_fixtures() {
        assert!(
            matches!(
                f.expected.packet_type,
                ConceptPacketType::UserVision
                    | ConceptPacketType::Requirement
                    | ConceptPacketType::RuleCandidate
                    | ConceptPacketType::Risk
                    | ConceptPacketType::Decision
                    | ConceptPacketType::Assumption
                    | ConceptPacketType::AntiGoal
            ),
            "fixture {} geçersiz packet_type",
            f.id
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 7 — threshold_band ThresholdBand 4 değerden
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_threshold_bands_valid() {
    for f in load_all_fixtures() {
        assert!(
            matches!(
                f.expected.threshold_band,
                ThresholdBand::Strong
                    | ThresholdBand::Tentative
                    | ThresholdBand::Weak
                    | ThresholdBand::Unanchored
            ),
            "fixture {} geçersiz threshold_band",
            f.id
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 8 — decision status consistency (D6 high-stake + MarkContradiction)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_decision_status_consistency() {
    for f in load_all_fixtures() {
        if f.expected.requires_operator_review {
            assert!(
                matches!(
                    f.expected.anchor_decision,
                    AnchorDecisionKind::RequireOperatorReview
                        | AnchorDecisionKind::TentativeLink
                        | AnchorDecisionKind::MarkContradiction
                ),
                "fixture {}: requires_operator_review=true ama anchor_decision={:?} \
                 (∈ {{RequireOperatorReview, TentativeLink, MarkContradiction}} olmalı, D6)",
                f.id,
                f.expected.anchor_decision
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 9 — high-stake edge'ler explanation ister + review gerektirir (INV-C7 ↔ D6)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_high_stake_edges_require_explanation() {
    let hs = high_stake_kinds();
    for f in load_all_fixtures() {
        let has_high_stake_edge = f.expected.edges.iter().any(|e| hs.contains(&e.kind));
        for e in &f.expected.edges {
            if hs.contains(&e.kind) {
                assert!(
                    e.requires_explanation,
                    "fixture {}: high-stake edge {:?} requires_explanation=true olmalı (INV-C7)",
                    f.id, e.kind
                );
            }
        }
        // INV-C7 ↔ D6 korelasyonu: high-stake edge varsa review gerekir
        if has_high_stake_edge {
            assert!(
                f.expected.requires_operator_review,
                "fixture {}: high-stake edge var ama requires_operator_review=false (D6)",
                f.id
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 10 — pre-state (given) gereken fixture'larda given non-empty
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_pre_state_required_when_needed() {
    let fixtures = load_all_fixtures();
    let by_id = |id: &str| -> &AnchoringFixture {
        fixtures
            .iter()
            .find(|f| f.id == id)
            .expect("fixture bulunamadı")
    };

    // fix_003 → decisions non-empty (accepted Decision:AdoptEventSourcing)
    let f3 = by_id("fix_003_arch_decision");
    assert!(
        !f3.given.decisions.is_empty(),
        "fix_003 given.decisions gerekli"
    );

    // fix_007 → decisions + code_entities non-empty (accepted Decision + Observed CodeEntity)
    let f7 = by_id("fix_007_contradiction");
    assert!(
        !f7.given.decisions.is_empty(),
        "fix_007 given.decisions gerekli"
    );
    assert!(
        !f7.given.code_entities.is_empty(),
        "fix_007 given.code_entities gerekli"
    );

    // fix_008 → concepts non-empty (pre-existing Concept:Payment)
    let f8 = by_id("fix_008_dedup_alias");
    assert!(
        !f8.given.concepts.is_empty(),
        "fix_008 given.concepts gerekli"
    );

    // fix_009 → concepts non-empty (pre-existing Concept:Notification + alias)
    let f9 = by_id("fix_009_weak_anchor_canon");
    assert!(
        !f9.given.concepts.is_empty(),
        "fix_009 given.concepts gerekli"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 11 — semantic roundtrip (structural equality, byte-level değil)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn anchor_fixture_semantic_roundtrip() {
    for (_, raw) in FIXTURE_FILES {
        let f1: AnchoringFixture = serde_json::from_str(raw).expect("parse f1");
        let serialized = serde_json::to_string(&f1).expect("serialize");
        let f2: AnchoringFixture = serde_json::from_str(&serialized).expect("parse f2");
        assert_eq!(f1, f2, "semantic roundtrip başarısız (structural equality)");
    }
}
