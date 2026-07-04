// INV-T2 compile-fail (PR35 hardening): external crate OperatorCapability::issue()
// çağıramaz. issue() pub(crate) — sadece osp-core içi. Downstream trusted-boundary
// issue_for_operator_session() kullanmalı (ismi "trusted caller sorumluluğu" anlatır).
// "OperatorCapability olmadan executable predicate / task doğmaz" artık type-level.
use osp_core::trajectory::OperatorCapability;

fn main() {
    // Bu satır derlenmemeli: issue() pub(crate) — external crate erişemez.
    let _cap = OperatorCapability::issue();
}
