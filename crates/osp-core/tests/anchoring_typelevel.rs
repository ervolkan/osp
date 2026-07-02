//! Faz 2 type-level invariant compile-fail testleri.
//!
//! [`trybuild`] ile 5 INV compile-time garantiyi doğrular. Bu testler IHMAL
//! edilemez: "invariant korunuyor" iddiasının en güçlü kanıtı — runtime testler
//! invariant'ın çalıştığını gösterir, compile-fail testler ihlalin İMKANSIZ olduğunu kanıtlar.
//!
//! # INV'ler
//! - `c3_graph_private.rs` — INV-C3: ConceptGraph.nodes private
//! - `c8_anchorplan_literal.rs` — INV-C8: AnchorPlan literal construct
//! - `c2_family_incompatible.rs` — INV-C2: PositionVector family ayrımı
//! - `c3_operator_acceptance_construct.rs` — INV-C3: OperatorAcceptance external construct
//! - `c4_supersede_authority_construct.rs` — INV-C4: SupersedeAuthority external construct

#[test]
fn type_level_invariants_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/c3_graph_private.rs");
    t.compile_fail("tests/compile_fail/c8_anchorplan_literal.rs");
    t.compile_fail("tests/compile_fail/c2_family_incompatible.rs");
    t.compile_fail("tests/compile_fail/c3_operator_acceptance_construct.rs");
    t.compile_fail("tests/compile_fail/c4_supersede_authority_construct.rs");
    t.compile_fail("tests/compile_fail/c8_anchorplan_deserialize.rs");
}
