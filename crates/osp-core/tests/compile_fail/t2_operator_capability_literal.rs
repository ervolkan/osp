// INV-T2 compile-fail: external crate OperatorCapability literal construct edemez.
// Private field `_private: ()` → struct literal engelli.
//
// NOT (claim seviyesi — D1 review PR35): Bu test sadece struct literal forge'i kapatır.
// `issue_for_operator_session()` hâlâ public'tir. Tam type-level unforgeability için
// Faz 8 operator console core içinde trusted entrypoint getirecek.
use osp_core::trajectory::OperatorCapability;

fn main() {
    // Bu satır derlenmemeli: field private.
    let _cap = OperatorCapability { _private: () };
}
