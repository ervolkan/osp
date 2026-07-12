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
//
// PR F: ilk argüman artık CodeIdentityKey (ConceptNodeId değil) — anti-corruption boundary.
use osp_core::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};
use osp_core::anchoring::types::{ConceptualIntentVector, ObservedCodeEvidence};

fn main() {
    let identity_key = CodeIdentityKey::new(
        CodeIdentityScheme::AnalysisPathV1 {
            case_policy: CodePathCasePolicy::CaseSensitive,
        },
        "Concept:Auth",
    )
    .unwrap();
    let _evidence = ObservedCodeEvidence::new(
        identity_key,
        // HATA: ObservedPhysicalMetrics bekleniyor, ConceptualIntentVector verilmiş.
        // INV-C6 (kod metric ≠ niyet) + INV-C2 (family ayrımı).
        // Niyet ölçülmüş kod kanıtı oluşturamaz — type sistem reddeder.
        ConceptualIntentVector::new(0.5, 0.5, 0.5, 0.5, 0.5, 0.5),
        0,
    );
}
