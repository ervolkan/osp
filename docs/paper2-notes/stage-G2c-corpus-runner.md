# Stage G2c — Corpus Experiment Runner Notes (Paper 2 evidence altyapısı)

> **Aşama:** G2c-1 (corpus experiment runner — harness MVP)
> **Tarih:** 2026-06-29
> **Tez:** "Navigator loop N repo × M task × {policy, feedback} matrisinde koşar,
> her hücreden G2cEvidenceRow üretilir. Paper 2 RQ6-9 evidence altyapısı."
> **Review entegrasyonu:** Arkadaş review 5 değerlendirmesinin 9 noktasının tamamı.

## Mimari

### 1. Runner: `crates/osp-analyzer/examples/g2c_corpus_matrix.rs`
Rust example (timing_bench.rs + token_benchmark.rs pattern). Dev-dependency: osp-llm-runtime + clap.
Çalıştırma: `cargo run --example g2c_corpus_matrix -- --llm mock|real [--out PATH]`

### 2. FeedbackSensitiveMock (review 5 #1)
```rust
enum MockBehavior {
    ScriptedFixed(Vec<DeltaProposal>),           // baseline — feedback'e bakmaz
    FeedbackSensitive {                           // RQ8 — feedback_history'ye göre branch
        without_feedback: Vec<DeltaProposal>,     // kötü → kötü → ... → limit
        with_feedback: Vec<DeltaProposal>,        // kötü → düzeltilmiş → başarılı
    },
}
```
RQ8 GERÇEK ölçülebilir: aynı task, aynı LLM ama feedback farkı.

### 3. NoFeedbackWrapper (review 5 #1)
```rust
struct NoFeedbackWrapper<L: LlmClient> { inner: L }
// complete(): view.feedback_history.clear() → inner.complete(&view)
```
RQ8 "without feedback" hücresi. osp-core'a dokunmaz — wrapper LlmClient'ı sarar.

### 4. Deterministik top-offender seçimi (review 5 #4)
Node(0) değil: `select_target_node()` highest coupling/instability score'lu Module/Concept node.
Tie-break: score desc, id asc (stable). Evidence row'a `target_node_id, target_node_role, selection_reason`.

### 5. Zengin evidence şeması (review 5 #6)
`G2cEvidenceRow`: run_id, git_commit, corpus_kind, target_node_id/role/reason, maneuver_limit,
final_outcome, final_mutation_decision, final_apply_target, total/prompt/completion_tokens,
feedback_count, rejected_by, loss_before/after, axis_regression, regression_axes,
max_regression_delta, duration_ms, per-attempt evidence ledger.

## Çalıştırma sonucu (G2c-1 harness MVP, mock)
- 24 cell (3 repo × 2 task × 2 policy × 2 feedback) — tamamı koştu
- Tümü `ExceededManeuverLimit` — 0 evidence entry
- **Kök neden:** mock proposals isolated node ekler, target node coupling'ini düşürmez
- Detay: `docs/paper2-notes/evidence/g2c-corpus-results.md`

## RQ etkisi
- **RQ6 (token cost):** mock=0. Gerçek LLM (G2c-4) ile dolacak.
- **RQ7 (task success):** mock 0/24 completed. Proposal realism fix (G2c-2) gerek.
- **RQ8 (calibration feedback):** mock MVP'de with/without farkı görünmüyor (ikisi de limit).
  G2c-2 fix: target-edge-aware proposals → Completed farkı.
- **RQ9 (policy):** mock MVP'de StrictReject/AcceptImprovement farkı görünmüyor (loss ↓ yok).
  G2c-3 fix: incremental coupling-dropping proposals → state accumulation farkı.

## Sınırlar (G2c-2/3/4/5'te gelecek)
- **G2c-2 fix:** Target-edge-aware mock proposals (delta node target'a edge remove/add ile
  bağlansın, coupling gerçek düşsün). RQ8 Completed farkı görünür.
- **G2c-3 fix:** Incremental proposals (0.82→0.71→0.63→0.53). RQ9 state accumulation.
- **G2c-4:** Gerçek LLM (GPT-4o-mini) küçük subset, cost-limited, manual. RQ6/RQ7.
- **G2c-5:** External corpus (chalk/click/cobra), paper-ready evidence.

## Statü etiketleri (review 5 #3, #8)
- **Mock matrix:** mechanism validation, controlled A/B, NOT external validity claim
- **Real LLM matrix (G2c-4):** preliminary empirical evidence, small subset, cost-controlled
- **Corpus:** "local-crate-subtree" (harness), "external-repo" (G2c-5)

## Doğrulama
- Build `-D warnings` temiz (CI uyumlu)
- 24 cell deterministik koştu, JSON üretildi
- INV-T1..T8 enforced (navigator loop koruması)
- navigator.run_task çalışıyor (attempts=5, ExceededManeuverLimit — proposal realism sorunu ayrı)

## Paper 2 için kritik bulgu
**Navigator loop altyapısı çalışıyor ama mock proposal'lar gerçek refactor etkisi yaratmıyor.**
Bu, Paper 2'ye dürüst bir bulgu: "altyapı hazır, proposal realism + gerçek LLM gerek."
Paper 2 yazımında threats/limitations bölümünde açıkça yazılacak.
