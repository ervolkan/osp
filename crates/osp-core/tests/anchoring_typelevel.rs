//! Faz 2/4/5a type-level invariant compile-fail testleri.
//!
//! [`trybuild`] ile 12 INV compile-time garantiyi doğrular. Bu testler IHMAL
//! edilemez: "invariant korunuyor" iddiasının en güçlü kanıtı — runtime testler
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
//! - `c6_observed_evidence_literal.rs` — INV-C6: ObservedCodeEvidence literal construct (Faz 4)
//! - `c6_intent_carries_physical_vector.rs` — INV-C6+C2: intent vector observed evidence'a (Faz 4)
//! - `c6_observed_evidence_deserialize.rs` — INV-C6: ObservedCodeEvidence Deserialize (Faz 4)
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
    // Faz 4 — INV-C6 (code evidence type-level)
    t.compile_fail("tests/compile_fail/c6_observed_evidence_literal.rs");
    t.compile_fail("tests/compile_fail/c6_intent_carries_physical_vector.rs");
    t.compile_fail("tests/compile_fail/c6_observed_evidence_deserialize.rs");
    // Faz 5a — INV-P1 (predicate stub type-level)
    t.compile_fail("tests/compile_fail/cP1_predicate_stub_literal.rs");
    t.compile_fail("tests/compile_fail/cP1_predicate_stub_deserialize.rs");
    // Faz 5b — INV-P2 (executable predicate set type-level)
    t.compile_fail("tests/compile_fail/cP2_executable_predicate_set_literal.rs");
    t.compile_fail("tests/compile_fail/cP2_executable_predicate_set_deserialize.rs");
}
