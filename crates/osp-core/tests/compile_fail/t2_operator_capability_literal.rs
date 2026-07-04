// INV-T2 compile-fail: external crate OperatorCapability literal construct edemez.
// Private field `_private: ()` → struct literal engelli. Sadece trusted-boundary API
// (issue_for_operator_session) ile üretilebilir. "Agent kodu capability üretemez."
use osp_core::trajectory::OperatorCapability;

fn main() {
    // Bu satır derlenmemeli: field private.
    let _cap = OperatorCapability { _private: () };
}
