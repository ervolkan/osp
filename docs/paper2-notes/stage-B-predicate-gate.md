# Stage B — Predicate Gate Integration Notes (Paper 2 evidence)

> **Aşama:** B (Predicate Gate Integration) — TAMAMLANDI
> **Tarih:** 2026-06-30
> **Tez:** "Verilmiş bir Task ve Claim için engine, task predicate'ini provenance-aware
> şekilde değerlendirir ve deterministic AttemptOutcome üretir."
> **Testler:** 11 yeni test (10 done-criteria + 1 ek), osp-core 245→256
> **Spec:** `docs/invariant-spec.md` INV-T5 güncellendi

---

## Karar 1: Claim.task_id = Option<TaskId> (review v2 — backward-compat)

**Karar:** Mevcut Claim'e `task_id: Option<TaskId>` eklendi (TaskBoundClaim ayrı struct değil).
**Gerekçe:** `Claim` OSP'nin merkezi epistemik nesnesi. Paper 1 static claim'ler, legacy
snapshot'lar taskless olmalı. Ayrı `TrajectoryClaim` iki paralel claim dünyası yaratırdı
(witness hangisini kabul? Q4-Q6 hangi struct?).
**Kanıt:** `#[serde(default)]` ile backward-compat — eski snapshot'lar None ile deserialize.
11 Claim literal (engine/time/witness test helper + faz5_e2e + integration) `task_id: None`
ile güncellendi, hepsi standalone.
**Edge case:** Q5.b çıplak Claim ile çalışmaz — `bind_task_claim()` zorunlu (type-level).
**Paper materyali:** §1 ontology — "Claim-task binding via Option + TaskBoundClaim view (INV-T5)."

---

## Karar 2: TaskBoundClaim + bind_task_claim (Q5.b girişi tip-güvenli)

**Karar:** `TaskBoundClaim<'a> { claim: &'a Claim, task: &'a Task }` validated view.
`bind_task_claim(claim, resolver)` üretir — None → `MissingTaskId`, bulunamaz → `TaskNotFound`.
**Gerekçe:** Q5.b'in çıplak Claim ile çağrılmasını type-level engellemek. Task lookup
`TaskResolver` trait (test: `InMemoryTaskRegistry`) — planner'a bulaşmadan.
**Kanıt:** `PredicateGate::evaluate(PredicateGateInput { bound: TaskBoundClaim, ... })` —
bound olmadan çağrılamaz (compile error).
**Paper materyali:** §4 Deterministic predicate gating — "Type-safe task binding."

---

## Karar 3: PredicateGate.evaluate — AttemptOutcome üretimi

**Karar:** `PredicateGate::evaluate` deterministic AttemptOutcome üretir:
1. `PredicateSet::evaluate_completion` (INV-T4 source check dahil)
2. Completed → AcceptAsCompleted; SourceInsufficient → Reject
3. NotCompleted → loss after + is_improved (INV-T6) + TaskPolicy → MutationDecision
**Gerekçe:** Soft gate Q5.b, hard gates (Q4/Q5/Q6) zaten geçti varsayılır (gate_decision: PassedAll).
**Kanıt:** 10 done-criteria test + 1 ek (operator_approval) — hepsi geçti.
**Paper materyali:** §4 — "Predicate gate → AttemptOutcome deterministic mapping."

---

## Karar 4: is_improved_loss — Aşama B basit, Aşama C Weighted

**Karar:** `is_improved_loss` loss_before/after + hard cap (axis < 0.85). Aşama C'de
WeightedPredicate loss + before/after karşılaştırması gelir.
**Gerekçe:** Aşama B ontoloji + gate integration; tam multi-axis loss Aşama C planner.
**Test:** `regression_rejected_even_if_one_axis_improved` — coupling OK ama instability 0.90
(> 0.85) → is_improved false → Reject.
**Paper materyali:** §7 Multi-axis oscillation — Aşama C'de Weighted loss.

---

## Kanıt 1: 10 done-criteria test (sizin listeniz) — hepsi geçti

| # | Test | Sonuç |
|---|---|---|
| 1 | predicate_satisfied_completes_task | ✅ Completed → AcceptAsCompleted |
| 2 | placeholder_metric_cannot_close_task_gate (INV-T4) | ✅ SourceInsufficient → Reject |
| 3 | predicate_uses_computed_raw_not_hint (INV-T3) | ✅ measured authoritative |
| 4 | missing_task_id_rejects_claim (binding) | ✅ MissingTaskId |
| 5 | strict_policy_rejects_unsatisfied_predicate | ✅ Reject |
| 6 | accept_improvement_policy_accepts_progress (INV-T6) | ✅ AcceptAsProgress |
| 7 | regression_rejected_even_if_one_axis_improved (F5) | ✅ Reject |
| 8 | progress_checkpoint_cannot_promote_to_mainline (INV-T8) | ✅ TrajectoryCheckpoint |
| 9 | reject_produces_not_applied (review v4 #3) | ✅ NotApplied |
| 10 | task_not_found_rejects_claim (binding) | ✅ TaskNotFound |
| Ek | operator_approval_policy_requires_human_review | ✅ RequireOperatorApproval |

---

## Aşama B'de YAPILMAYAN (Aşama C+)

- **Planner** (TargetRegion → Task predicate set üretimi) — Aşama C
- **Intent::from_task** — Aşama C
- **Milestone decomposition** (Trajectory → Milestone → Task scheduling) — Aşama C
- **WeightedPredicate loss** (F5 tam impl, multi-axis before/after) — Aşama C
- **RuleRef → Box<dyn Rule> registry** (Q6 extension) — Aşama C (stub B'de)
- **Agent Navigator loop** (DeltaProposal → Claim → gate → trajectory update) — Aşama D

---

## Test özeti (Aşama B)

osp-core: 245 → **256 test** (+11 Aşama B), 0 fail, -D warnings temiz.
Workspace: 450 → **461 test**.

---

*Bu not `docs/paper2-notes/README.md` disiplinine uyar: karar + gerekçe + kanıt + edge case + paper materyali.*
