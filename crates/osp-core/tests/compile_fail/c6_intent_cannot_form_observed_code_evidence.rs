// INV-C6 compile-fail: ConceptualIntentVector (niyet) observed code evidence oluşturamaz.
//
// PR C öncesi: ObservedCodeEvidence::new 2. argümanı PhysicalCodeVector beklerdi;
// ConceptualIntentVector verince type-mismatch (INV-C6 + INV-C2 birleşimi).
//
// PR C: ObservedCodeEvidence::new artık (id, observations: ObservedPhysicalMetrics, time)
// imzasına sahip. ConceptualIntentVector — ne PhysicalCodeVector olarak ne de
// ObservedPhysicalMetrics olarak bu konuma geçemez. "Kod metric'leri (PhysicalCode) ile
// niyet (ConceptualIntent) karıştırılamaz" invariant'ı korunur; axis-granular modelle
// daha da güçlenir (intent'ten per-axis observation üretilemez).
use osp_core::anchoring::types::{ConceptualIntentVector, ObservedCodeEvidence};
use osp_core::anchoring::ConceptNodeId;

fn main() {
    let _evidence = ObservedCodeEvidence::new(
        ConceptNodeId("Concept:Auth".into()),
        // HATA: ObservedPhysicalMetrics bekleniyor, ConceptualIntentVector verilmiş.
        // INV-C6 (kod metric ≠ niyet) + INV-C2 (family ayrımı).
        // Niyet ölçülmüş kod kanıtı oluşturamaz — type sistem reddeder.
        ConceptualIntentVector::new(0.5, 0.5, 0.5, 0.5, 0.5, 0.5),
        0,
    );
}
