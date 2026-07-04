// INV-T2 compile-fail (PR35): external crate OperatorCapability::issue() çağıramaz.
// issue() pub(crate) — sadece osp-core içi (TCB + testler).
//
// NOT (claim seviyesi — D1 review PR35): Bu test sadece `issue()` external erişimini
// kapatır. `issue_for_operator_session()` hâlâ public'tir (trusted-boundary API).
// Tam type-level unforgeability DEĞİL — runtime enforcement operator-session boundary'de.
use osp_core::trajectory::OperatorCapability;

fn main() {
    // Bu satır derlenmemeli: issue() pub(crate) — external crate erişemez.
    let _cap = OperatorCapability::issue();
}
