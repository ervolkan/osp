// EI1-a compile-fail (PR F): external crate CodeIdentityKey literal construct edemez.
//
// Field'lar private → harici `CodeIdentityKey { ... }` engelli. Sadece smart constructor
// `new(scheme, key)` ile üretilebilir (canonicalization + validation uygular). PR C
// `ObservedPhysicalMetrics` literal opacity pattern mirror.
//
// Bu compile-fail, identity dünyasının construction boundary'sini korur — anti-corruption
// boundary'nin identity tarafı. Dışarıdan forge edilmiş identity key ile evidence lookup
// yapılamaz; her key constructor'dan geçer (case policy + empty/control validation).
use osp_core::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};

fn main() {
    let scheme = CodeIdentityScheme::AnalysisPathV1 {
        case_policy: CodePathCasePolicy::CaseSensitive,
    };

    // Bu satır derlenmemeli: field'lar private.
    let _key = CodeIdentityKey {
        scheme,
        key: "CodeEntity:X".into(),
    };
}
