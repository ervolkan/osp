// INV-C6 compile-fail (PR C, serde boundary): ObservedPhysicalMetrics Deserialize YOK.
// Private `values` field serde ile reconstruct edilemesin (axis-granular evidence bypass engeli).
// Serialize-only (audit için); ObservedCodeEvidence + ObservedPhysicalMetric ile aynı patern.
// "Diskten observation collection reconstruct edip INV-C6 boundary'yi bypass etmek imkansız."
use osp_core::anchoring::types::ObservedPhysicalMetrics;

fn main() {
    // Bu satır derlenmemeli: ObservedPhysicalMetrics Deserialize impl'i yok.
    let _metrics: ObservedPhysicalMetrics = serde_json::from_str("{}").unwrap();
}
