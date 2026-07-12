//! Faz 2/4/5a type-level invariant compile-fail testleri.
//!
//! [`trybuild`] ile type-enforced INV'lerin compile-time garanti'lerini doğrular. Bu testler
//! IHMAL edilemez: "invariant korunuyor" iddiasının en güçlü kanıtı — runtime testler
//! invariant'ın çalıştığını gösterir, compile-fail testler ihlalin İMKANSIZ olduğunu kanıtlar.
//!
//! # INV'ler
//! - `c3_graph_private.rs` — INV-C3: ConceptGraph.nodes private
//! - `c8_anchorplan_literal.rs` — INV-C8: AnchorPlan literal construct
//! - `c2_family_incompatible.rs` — INV-C2: PositionVector family ayrımı
//! - `c3_operator_acceptance_construct.rs` — INV-C3: OperatorAcceptance external construct
//! - `c4_supersede_authority_construct.rs` — INV-C4: SupersedeAuthority external construct
//! - `c8_anchorplan_deserialize.rs` — INV-C8: AnchorPlan Deserialize (Faz 3 serde boundary)
//! - `c3_conceptgraph_deserialize.rs` — INV-C3: ConceptGraph Deserialize (Faz 3 serde boundary)
//! - `c6_observed_evidence_literal.rs` — INV-C6: ObservedCodeEvidence literal construct (Faz 4, PR C)
//! - `c6_intent_cannot_form_observed_code_evidence.rs` — INV-C6+C2: intent observed evidence oluşturamaz (Faz 4, PR C rename)
//! - `c6_observed_evidence_deserialize.rs` — INV-C6: ObservedCodeEvidence Deserialize (Faz 4)
//! - `c6_observed_physical_metrics_literal.rs` — INV-C6: ObservedPhysicalMetrics literal construct (PR C)
//! - `c6_observed_physical_metrics_deserialize.rs` — INV-C6: ObservedPhysicalMetrics Deserialize (PR C serde boundary)
//! - `cP1_predicate_stub_literal.rs` — INV-P1: PredicateStub literal construct (Faz 5a)
//! - `cP1_predicate_stub_deserialize.rs` — INV-P1: PredicateStub Deserialize (Faz 5a serde boundary)

#[test]
fn type_level_invariants_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/c3_graph_private.rs");
    t.compile_fail("tests/compile_fail/c8_anchorplan_literal.rs");
    t.compile_fail("tests/compile_fail/c2_family_incompatible.rs");
    t.compile_fail("tests/compile_fail/c3_operator_acceptance_construct.rs");
    t.compile_fail("tests/compile_fail/c4_supersede_authority_construct.rs");
    t.compile_fail("tests/compile_fail/c8_anchorplan_deserialize.rs");
    t.compile_fail("tests/compile_fail/c3_conceptgraph_deserialize.rs");
    // Faz 4 — INV-C6 (code evidence type-level). PR C: axis-granular model.
    t.compile_fail("tests/compile_fail/c6_observed_evidence_literal.rs");
    t.compile_fail("tests/compile_fail/c6_intent_cannot_form_observed_code_evidence.rs");
    t.compile_fail("tests/compile_fail/c6_observed_evidence_deserialize.rs");
    // PR C — INV-C6 axis-granular collection (literal + Deserialize engelli)
    t.compile_fail("tests/compile_fail/c6_observed_physical_metrics_literal.rs");
    t.compile_fail("tests/compile_fail/c6_observed_physical_metrics_deserialize.rs");
    // Faz 5a — INV-P1 (predicate stub type-level)
    t.compile_fail("tests/compile_fail/cP1_predicate_stub_literal.rs");
    t.compile_fail("tests/compile_fail/cP1_predicate_stub_deserialize.rs");
    // Faz 5b — INV-P2 (executable predicate set type-level)
    t.compile_fail("tests/compile_fail/cP2_executable_predicate_set_literal.rs");
    t.compile_fail("tests/compile_fail/cP2_executable_predicate_set_deserialize.rs");
    // PR35 — INV-T2 (OperatorCapability hardening, type-level)
    t.compile_fail("tests/compile_fail/t2_operator_capability_issue_external.rs");
    t.compile_fail("tests/compile_fail/t2_operator_capability_literal.rs");
    // Faz 5.1 — INV-P3 (cross-family hint type-level)
    t.compile_fail("tests/compile_fail/cP3_cross_family_hint_literal.rs");
    t.compile_fail("tests/compile_fail/cP3_cross_family_hint_deserialize.rs");

    // Faz 8a — INV-C12 (PresentedBasis: literal + Deserialize engelli)
    t.compile_fail("tests/compile_fail/c12_presented_basis_literal.rs");
    t.compile_fail("tests/compile_fail/c12_presented_basis_deserialize.rs");

    // Faz 8a — INV-C13 (DecisionApplication: literal + Deserialize engelli)
    t.compile_fail("tests/compile_fail/c13_decision_application_literal.rs");
    t.compile_fail("tests/compile_fail/c13_decision_application_deserialize.rs");

    // Faz 8b (PR #49) — SupersedeApplication opacity boundary (C13-paralel; C15 runtime
    // semantiğini değil construction boundary'yi korur).
    t.compile_fail("tests/compile_fail/supersede_application_opacity_literal.rs");
    t.compile_fail("tests/compile_fail/supersede_application_opacity_deserialize.rs");

    // PR E — INV-C16 (ResolutionApplication: literal + Deserialize engelli)
    t.compile_fail("tests/compile_fail/c16_resolution_application_literal.rs");
    t.compile_fail("tests/compile_fail/c16_resolution_application_deserialize.rs");

    // PR F — EI1-a evidence identity (ResolvedCodeIdentity + CodeIdentityKey literal opacity)
    t.compile_fail("tests/compile_fail/cF1_resolved_code_identity_literal.rs");
    t.compile_fail("tests/compile_fail/cF1_code_identity_key_literal.rs");
}
