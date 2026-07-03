// INV-P2 compile-fail: external crate ExecutablePredicateSet literal construct edemez.
// Private inner field → harici `ExecutablePredicateSet { ... }` engelli.
// Sadece bind_metric_threshold() ile üretilebilir (non-empty by construction).
// "ExecutablePredicateSet operator capability olmadan doğmaz."
use osp_core::anchoring::ExecutablePredicateSet;
use osp_core::trajectory::PredicateSet;

fn main() {
    // Bu satır derlenmemeli: inner field private.
    let _eps = ExecutablePredicateSet {
        predicate_set: PredicateSet {
            mode: osp_core::trajectory::PredicateMode::All,
            predicates: vec![],
            preferred_vector: None,
        },
    };
}
