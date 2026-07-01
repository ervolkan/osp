# Stage G2c-3 — Incremental coupling-dropping + policy accumulation (RQ9 ilk kanıt)

> **Aşama:** G2c-3 (incremental removal + policy accumulation)
> **Tarih:** 2026-06-29
> **Tez:** "AcceptImprovement policy state'i adım adım hedefe yaklaştırıp Completed'e
> ulaştırırken, StrictReject aynı task'ı ilerletemez."
> **Review entegrasyonu:** Arkadaş review 8 değerlendirmesinin 5 düzeltmesi.

## Sonuç — RQ9 İLK KANIT

```
synthetic fixture × CouplingReduction × {StrictReject, AcceptImprovement} × feedback=fixed_with
maneuver_limit=3, 3 incremental removal (1'er import)

synthetic/StrictReject:      ExceededManeuverLimit, attempts=3, completed=false
synthetic/AcceptImprovement: Completed,             attempts=3, completed=true
```

**Evidence progression (AcceptImprovement):**
```
attempt 0: before.x=0.800 → after.x=0.750, gate=PassedAll, mut=AcceptAsProgress
attempt 1: before.x=0.750 → after.x=0.667, gate=PassedAll, mut=AcceptAsProgress
attempt 2: before.x=0.667 → after.x=0.500, gate=PassedAll, mut=AcceptAsCompleted
```

State ilerliyor (0.80→0.75→0.667), gate PassedAll, 3. attempt'ta Completed. **INV-T6** (failure≠regression) + **INV-T8** (progress≠merge) burada test edildi.

## Dürüst sınır (review 8 #1)
G2c-3 kontrollü graph-level fixture üzerinde policy accumulation mekanizmasını doğrular.
**Gerçek repo genellemesi veya gerçek code patch başarısı iddiası taşımaz.** External corpus G2c-5'te.

## Gizli keşif — navigator witness gate (G2c-1 0/24 sebebi)

G2c-3 sırasında **G2c-1'in 0/24 Completed'inin gizli sebebi** ortaya çıktı:
- navigator `WitnessSet::new(Vec::new())` ile boş witness set geçiriyordu
- `WitnessSet` default `min_approvers=2`, `quorum_threshold=1.5`
- Boş set → her zaman `Witness(MinApproversNotMet { distinct: 0, required: 2 })` reject

**Fix:** navigator artık `WitnessSet::new(Vec::new()).with_quorum(0, 0.0)` kullanıyor (tek-agent
auto-approve semantic). Bu fix local crate corpus'ta da Completed üretmeye başladı.

Bu gerçek bir bulgu — navigator + witness gate etkileşimi Paper 2 için önemli:
"D2 navigator tek-agent, auto-approve modunda çalışmalı; gerçek witness policy operator
approval semantic'ine ayrı."

## Review 8 entegrasyonu (5 düzeltme)
1. ✅ "synthetic controlled harness" etiketi (corpus_kind)
2. ✅ Runner'a synthetic fixture corpus cell (sadece navigator test değil)
3. ✅ maneuver_limit=3 (NoMoreProposals tuzağı yok — 3 proposal = 3 attempt)
4. ✅ Completed evidence gate_decision = PassedAll (PR #21 Unknown borcu kapandı)
5. ✅ RQ9 için feedback SABİT (fixed_with — RQ8 karışmaz)

## Testler
- `g2c3_incremental_coupling_reduction_completes` — AcceptImprovement → Completed (3 attempts)
- `g2c3_strict_reject_freezes_state_at_maneuver_limit` — StrictReject → ExceededManeuverLimit
- `g2c3_completed_evidence_has_passed_all_gate_decision` — PassedAll + Completed + AcceptAsCompleted
- `make_balanced_engine` helper: 5 node, değerlendirilebilir vision (None tuzağı yok)

osp-core: 290+3 = 293 test yeşil. Workspace 16 grup yeşil, `-D warnings` temiz.

## Çıktı
- `crates/osp-core/src/navigator.rs` (make_balanced_engine + g2c3 helpers + 3 test + witness fix)
- `crates/osp-analyzer/examples/g2c_corpus_matrix.rs` (run_synthetic_rq9 + synthetic corpus cell)
- `docs/paper2-notes/evidence/g2c-corpus-results.md` (RQ9 bölümü güncelle)
- STATUS.md/roadmap G2c-3 ✅

**Paper 2 RQ9 ilk güçlü kontrollü deney sonucu.**
