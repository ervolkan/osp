// EI1-a compile-fail (PR F): external crate ResolvedCodeIdentity literal construct edemez.
//
// Field'lar private → harici `ResolvedCodeIdentity { ... }` engelli. Sadece public smart
// constructor `new(node_id, identity_key)` ile üretilebilir. "Resolved identity almak =
// lookup'tan (CodeIdentityBindingLookup) gelmiş olmak" — sahte resolved value kurulamaz.
//
// EI1-a (TYPE): resolved value exactly one key taşır (private fields + fixed struct shape).
// Struct literal dışarıdan kurulamaz → evidence identity invariant'ın type-enforced parçası.
use osp_core::anchoring::code_evidence::ResolvedCodeIdentity;
use osp_core::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};
use osp_core::anchoring::ConceptNodeId;

fn main() {
    let identity_key = CodeIdentityKey::new(
        CodeIdentityScheme::AnalysisPathV1 {
            case_policy: CodePathCasePolicy::CaseSensitive,
        },
        "CodeEntity:X",
    )
    .unwrap();

    // Bu satır derlenmemeli: field'lar private.
    let _resolved = ResolvedCodeIdentity {
        node_id: ConceptNodeId("CodeEntity:X".into()),
        identity_key,
    };
}
