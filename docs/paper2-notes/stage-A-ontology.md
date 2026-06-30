# Stage A — Ontology Notes (Paper 2 evidence)

> **Aşama:** A (ontolojik tipler) — TAMAMLANDI
> **Tarih:** 2026-06-30
> **Dosya:** `crates/osp-core/src/trajectory.rs` (yeni modül)
> **Testler:** 13 test, INV-T1..T8 type-level enforcement (osp-core 232→245)
> **Spec:** `docs/invariant-spec.md` v3

---

## Karar 1: RuleRef — Rule trait object serde'lenemiyor

**Karar:** Task.constraints `Vec<RuleRef>` (string ID) oldu, `Vec<Box<dyn Rule>>` değil.
**Gerekçe:** `Rule` trait object Debug/Clone/Serialize değil. Task/AgentTaskView derive
Serialize — `dyn Rule` derive edemez. RuleRef (String) serde'lanabilir; engine Q6 gate
(Aşama B) RuleRef → `Box<dyn Rule>` resolve eder (rule registry).
**Kanıt:** Build önce `dyn Rule doesn't implement Debug/Clone/Serialize` (12 error) →
RuleRef ile temiz.
**Edge case:** RuleRef string naming convention kurulmalı (Aşama B) — "no_self_import",
"max_coupling:0.5" gibi.
**Paper materyali:** §1 Trajectory ontology — "Rule constraint serialization via indirection."

---

## Karar 2: is_improved Aşama A'da basit (refine Aşama C)

**Karar:** `is_improved` Aşama A'da basit Euclidean loss + max_axis_regression. cohesion
için "regression = azalma" mantığı placeholder (cohesion düşmek = kötü).
**Gerekçe:** Tam multi-axis WeightedPredicate loss Aşama C'nin konusu (F5). Aşama A'da
sadece loss direction doğrulandı (test: `trajectory_loss_decreases`).
**Kanıt:** `is_improved` fonksiyonu var ama testlerde sadece `trajectory_loss` doğrulandı;
`is_improved` Aşama C'de loss function tamamlanınca test edilir.
**Paper materyali:** §2 Task dematerialization — loss function matematiği Aşama C'de.

---

## Karar 3: PredicateMode::Weighted Aşama C'de loss

**Karar:** `evaluate_completion` Weighted modda All gibi davranıyor (source check).
Tam weight-based loss Aşama C'de.
**Gerekçe:** WeightedPredicate.weight Some(w) ile loss hesabı F5 axis oscillation matematiği
gerektirir — Aşama A ontoloji, Aşama C planner matematiği.
**Kanıt:** Weighted variantı var, predicate'ler taşınıyor; loss impl Aşama C.
**Paper materyali:** §7 Multi-axis oscillation — Weighted loss function.

---

## Kanıt 1: INV-T1 type-level enforce edildi

**Kanıt:** `AgentTaskView` serde çıktısında target coordinate alanları yok (test:
`agent_task_view_has_no_target_coordinate_fields`). `current_measurement` var (mevcut
durum, serbest), `preferred_vector`/`target_vector`/`milestone_target_vector`/`target_raw`/
`target_region` assertion ile reddedildi.
**InternalTaskPlan.to_agent_view()** preferred_vector'i düşürür (tek yönlü dönüşüm).
**Paper materyali:** §1 ontology — "AgentTaskView/InternalTaskPlan type separation (INV-T1)."

---

## Kanıt 2: INV-T2 OperatorCapability compile-time

**Kanıt:** `OperatorCapability { _private: () }` — agent kodu `OperatorCapability { _private: () }`
yazamaz (private field, compile error). Sadece `OperatorCapability::issue()` (trusted API).
`Trajectory::new(&cap, ...)` capability zorunlu.
**Test:** `operator_capability_can_be_issued_by_trusted_api` (Trajectory::new capability ile).
**Paper materyali:** §1 — "Capability-based operator boundary (INV-T2)."

---

## Kanıt 3: INV-T4 ProvenancedRawPosition source-level

**Kanıt:** `MetricPredicate::evaluate` `required_source` ile source karşılaştırır.
Placeholder source + required Scip → `PredicateResult::SourceInsufficient`.
**Test:** `placeholder_metric_cannot_close_task` (0.40 ≤ 0.55 ama placeholder → reddet).
**Paper materyali:** §4 Deterministic predicate gating — "Source-validated predicates (INV-T4)."

---

## Kanıt 4: INV-T8 ApplyTarget — Reject ≠ Sandbox

**Kanıt:** `MutationDecision::apply_target()` Reject → `NotApplied` (değil Sandbox).
`AcceptAsProgress` → `TrajectoryCheckpoint` (asla Mainline). `AcceptAsCompleted` → Mainline.
**Test:** 4 test (reject/progress/completed/operator_approval lane mapping).
**Paper materyali:** §4 — "Progress checkpoint isolation (INV-T8), ApplyTarget model."

---

## Aşama A'da YAPILMAYAN (Aşama B+)

- **Q5.b Predicate Gate** (engine.rs) — Aşama B
- **TaskAttempt Ledger** (TrajectoryEvidence kayıt) — Aşama B2
- **Planner** (TargetRegion → Task predicate set, Intent::from_task) — Aşama C
- **Weighted loss function** (F5 tam impl) — Aşama C
- **Agent döngüsü** (DeltaProposal → Claim → gate → trajectory update) — Aşama D
- **RuleRef → Box<dyn Rule> resolve** (rule registry) — Aşama B

---

## Test özeti (Aşama A)

| Invariant | Test | Durum |
|---|---|---|
| INV-T1 | agent_task_view_has_no_target_coordinate_fields | ✅ |
| INV-T2 | operator_capability_can_be_issued + private_field | ✅ (2 test) |
| INV-T3 | (measured_metric_satisfies — engine ölçer implicit) | ✅ |
| INV-T4 | placeholder_metric_cannot_close + measured/unsatisfied | ✅ (3 test) |
| INV-T5 | predicate_set_all/any_mode | ✅ (2 test) |
| INV-T6 | trajectory_loss_decreases | ✅ |
| INV-T8 | reject/progress/completed/operator_approval lane | ✅ (4 test) |

osp-core: 232 → **245 test** (+13 trajectory), 0 fail, -D warnings temiz.

---

*Bu not `docs/paper2-notes/README.md` disiplinine uyar: karar + gerekçe + kanıt + edge case + paper materyali.*
