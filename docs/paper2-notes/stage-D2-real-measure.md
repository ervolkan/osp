# Stage D2 — Gerçek engine measure + commit_task_claim Notes (Paper 2 evidence)

> **Aşama:** D2 (Gerçek engine measure + commit_task_claim Q5.b) — TAMAMLANDI
> **Tarih:** 2026-06-30
> **Tez:** "Navigator, analyzed repo'nun gerçek space'i üzerinde ölçüm yapar — mock 0 coupling
> değil. commit_task_claim metodu, task-bound Claim'ler için Q4→Q5→Q5.b→Q6→Q1-Q3 atomic pipeline."
> **Testler:** 5 yeni test (D2), osp-core 273→278, workspace 478→483
> **Kapsam:** commit_task_claim (sizin TaskCommitInput/Result yapınız), navigator mut engine.

---

## Karar 1: commit_task_claim — yeni metod (commit() korunur, sizin 3. seçenek)

**Karar:** `commit_task_claim(input: TaskCommitInput) -> Result<TaskCommitResult, EngineCommitError>`.
Mevcut `commit()` (standalone, Paper 1) korunur — backward compatible. Task-bound Claim'ler
için yeni metod (Paper 2). Sizin `TaskCommitInput`/`TaskCommitResult` structured yapınız.
**Gerekçe:** commit() içine Q5.b eklemek tüm caller'ları (tests, desktop, analyzer) kırar.
İki ayrı API: `commit() = legacy; commit_task_claim() = trajectory`.
**İç akış (sizin önerdiğiniz sıra):** Q4 → bind → Q5 → Q5.b(PredicateGate) → Q6 →
MutationDecision → ApplyTarget → Q1-Q3 → TaskCommitResult.
**Kanıt:** `commit_task_claim_runs_q5b_predicate_gate`, `commit_standalone_unchanged`,
`commit_task_claim_requires_task_bound_claim`.
**Paper materyali:** §4 Deterministic predicate gating — "Atomic commit_task_claim (Q5.b in transaction)."

---

## Karar 2: Gerçek measure — construction-site değişiklik (keşif raporu içgörüsü)

**Karar:** D1 mock 0 coupling'ler **construction** kaynaklıydı (boş space + boş axes), imza
değil. D2: navigator'a **analyzed space** + **5-axis CoordinateSystem** ile kurulmuş engine ver.
`compute_raw_from_delta` zaten gerçek ölçüm yapıyor — sadece doğru construction gerek.
**Gerekçe:** Keşif raporu — `compute_raw_from_delta` `self.space.clone()` + `coord_system`
kullanır. Boş space/axes → 0. osp-desktop'ta bu pattern zaten var (lib.rs:257-278).
**Kanıt:** `navigator_real_measure_nonzero_coupling` — gerçek space'de coupling > 0 (edge 0→1).
`navigator_delta_edges_affect_coupling` — proposed edge coupling'i artırır.
**Paper materyali:** §3 — "Real engine measure (construction-site, not signature change)."

---

## Karar 3: Navigator engine: &'a mut SpaceEngine

**Karar:** Navigator `engine: &'a SpaceEngine` (immutable) → `&'a mut SpaceEngine` (mutable).
commit_task_claim `&mut self` gerektirir.
**Gerekçe:** commit_task_claim engine'i mutasyona uğratır (apply_delta, t_c++).
**Kanıt:** 8 D1 testi `&mut engine` ile güncellendi, hepsi geçti.
**Paper materyali:** §3 — "Navigator mutates engine via commit_task_claim."

---

## Karar 4: delta_edges artık compute_raw_from_delta'ya geçiliyor

**Karar:** D1'de `compute_raw_from_delta(&delta_nodes, &[])` (boş edges). D2'de gerçek
`delta_edges` geçiriliyor — proposed edge'ler coupling'i etkiler.
**Gerekçe:** Keşif raporu — `compute_raw_edge_increases_coupling` testi edge'in coupling
artırdığını kanıtlar. D1'de `&[]` yanlıştı.
**Kanıt:** `navigator_delta_edges_affect_coupling` — edge ile coupling ≥ edge'siz.
**Paper materyali:** §3 — "Delta edges contribute to coupling measurement."

---

## Kanıt: 5 D2 done-criteria test

| # | Test | Sonuç |
|---|---|---|
| 1 | navigator_real_measure_nonzero_coupling | ✅ coupling > 0 (real space) |
| 2 | commit_task_claim_runs_q5b_predicate_gate | ✅ Q5.b çalıştı (witness fail OK) |
| 3 | commit_standalone_unchanged | ✅ mevcut commit() korunur |
| 4 | commit_task_claim_requires_task_bound_claim | ✅ standalone → reject |
| 5 | navigator_delta_edges_affect_coupling | ✅ edge coupling'i artırır |

---

## D1 limitation çözüldü

D1'de maneuver limit testi "loop ran" seviyesindeydi (mock engine 0 coupling → her predicate
satisfied). D2'de gerçek measure ile maneuver limit artık anlamlı — coupling düşmedikçe
reject. svelte corpus integration test (gerçek analyze_repo) bir sonraki adım (osp-cli ile,
çünkü osp-core analyzer'a bağımlı değil — integration test osp-analyzer crate'inde).

---

## Aşama D2'de YAPILMAYAN (D3+)

- **svelte corpus integration test** — osp-analyzer crate'inde (osp-core bağımlılık yok)
- **Gerçek LLM** (RuntimeLlmClient) — D3/osp-mcp
- **commit() içine permission gate** (INV #13) — D3+
- **Trajectory correction** — Aşama E
- **osp-cli / osp-mcp** — Aşama F/G

---

## Test özeti (Aşama D2)

osp-core: 273 → **278 test** (+5 D2), workspace 478 → **483**, -D warnings temiz, fmt temiz.

---

*Bu not `docs/paper2-notes/README.md` disiplinine uyar: karar + gerekçe + kanıt + edge case + paper materyali.*
