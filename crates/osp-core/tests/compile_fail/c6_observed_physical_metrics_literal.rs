// INV-C6 compile-fail (PR C): external crate ObservedPhysicalMetrics literal construct edemez.
// `values` field private → harici `ObservedPhysicalMetrics { values: ... }` engelli.
// Sadece `try_new(...)` smart constructor ile üretilebilir (non-empty + unique-axis +
// deterministic-order invariant'ları constructor içinde korunur).
// "Axis-granular observation collection almak = try_new'den geçmiş olmak."
use osp_core::anchoring::types::ObservedPhysicalMetrics;

fn main() {
    // Bu satır derlenmemeli: `values` field private.
    let _metrics = ObservedPhysicalMetrics {
        values: Vec::new(),
    };
}
