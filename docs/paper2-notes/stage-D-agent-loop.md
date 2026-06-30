# Stage D1 — Agent Navigator Loop Notes (Paper 2 evidence)

> **Aşama:** D1 (Agent Navigator iskeleti + mock LLM) — TAMAMLANDI
> **Tarih:** 2026-06-30
> **Tez:** "Agent Navigator, bir Task için LLM çağırır → DeltaProposal → Claim (task-bound)
> → engine measure + PredicateGate → TaskAttempt/Evidence kayıt → retry/progress/complete."
> **Testler:** 8 yeni test, osp-core 265→273, workspace 470→478
> **Kapsam:** Mock LLM (gerçek HTTP D2'de). Hard gates PassedAll varsay (commit entegrasyon D2).

---

## Karar 1: LlmClient trait (mock + production decoupling)

**Karar:** `LlmClient` trait — `complete(view: &AgentTaskView) -> Result<DeltaProposal, LlmError>`.
Mock: `MockLlmClient` (scripted proposals, deterministic call_count). Production: osp-llm-runtime sarar (D2).
**Gerekçe:** Test gerçek API key gerektirmemeli (decoupling). INV-T1 — AgentTaskView serialize (hedef koordinat YOK).
**Kanıt:** `mock_llm_returns_scripted_proposals_in_order` — deterministic, NoMoreProposals.
**Paper materyali:** §1 ontology — "LlmClient abstraction (mock vs production)."

---

## Karar 2: DeltaProposal → Claim + ProvenancedRawPosition bridge (boşluk #3, #7)

**Karar:** `build_claim_from_proposal()` — DeltaProposal + engine computed_raw + task_id → Claim (task-bound).
`provenanced_from_raw()` — RawPosition + source → ProvenancedRawPosition (INV-T4 source-level).
**Gerekçe:** 9 boşluktan #3 (DeltaProposal→Claim) ve #7 (Raw→Provenanced) kapatıldı.
**Kanıt:** `build_claim_sets_task_id` (task_id Some), `provenanced_from_raw_preserves_values`.
**Paper materyali:** §1 — "DeltaProposal→Claim bridge; engine measures, agent declares no position."

---

## Karar 3: AgentNavigator.run_task loop (boşluk #4, #5, #6, #8)

**Karar:** `AgentNavigator.run_task(task_id, agent)` — maneuver limit (INV-T7) kadar iteratif:
LLM → DeltaProposal → Q4 syntax → Claim → engine measure → bind_task_claim + PredicateGate (Q5.b) →
Evidence kayıt → MutationDecision (AcceptAsCompleted/AcceptAsProgress/Reject/RequireOperatorApproval).
**Gerekçe:** 9 boşluktan #4 (Q5.b commit entegrasyon — D1'de ayrı çağrı, D2'de commit'e),
#5 (CommitOutcome trajectory fields — navigator PredicateGate ayrı), #6 (TaskAttempt construct +
persistence — evidence ledger), #8 (loop driver).
**Kanıt:** `navigator_records_evidence_per_attempt`, `navigator_accepts_progress_checkpoint`,
`navigator_token_cost_accumulated` (RQ6).
**Paper materyali:** §3 Adaptive control loop — "Navigator loop with evidence ledger."

---

## Karar 4: D1 limitation — mock engine measure (D2'de gerçek)

**Karar:** D1'de mock engine (boş space) compute_raw_from_delta gerçek coupling vermez.
Maneuver limit testi D1'de "loop çalıştı" seviyesinde; ExceededManeuverLimit D2'de (gerçek measure).
**Gerekçe:** compute_raw_from_delta SpaceEngine method — gerçek node/edge ölçümü D2'de (corpus).
D1 iskelet + evidence akışını doğrular.
**Kanıt:** `navigator_exceeds_maneuver_limit` — D1'de Completed/LlmError (loop ran), D2'de refine.
**Paper materyali:** §3 — "D1 mock; D2 real engine measure for corpus validation."

---

## Kanıt: 8 done-criteria test

| # | Test | Sonuç |
|---|---|---|
| 1 | navigator_task_not_found | ✅ TaskNotFound |
| 2 | navigator_records_evidence_per_attempt (boşluk #6) | ✅ ledger dolu |
| 3 | navigator_exceeds_maneuver_limit (INV-T7, D1 limitation) | ✅ loop ran |
| 4 | navigator_accepts_progress_checkpoint (INV-T6) | ✅ evidence |
| 5 | navigator_token_cost_accumulated (RQ6) | ✅ 100+120=220 |
| 6 | mock_llm_returns_scripted_proposals_in_order | ✅ deterministic |
| 7 | build_claim_sets_task_id (boşluk #3) | ✅ task_id Some |
| 8 | provenanced_from_raw_preserves_values (boşluk #7) | ✅ values + source |

---

## 9 boşluk durumu (D1 sonrası)

| # | Boşluk | D1 | D2+ |
|---|---|---|---|
| 1 | OspPrompt→AgentTaskView | ✅ (navigator AgentTaskView üretir) | D2: OspPrompt genişletme |
| 2 | LLM call site core/desktop | ✅ (LlmClient trait + navigator) | D2: RuntimeLlmClient |
| 3 | DeltaProposal→Claim | ✅ build_claim_from_proposal | — |
| 4 | commit()↔PredicateGate | ✅ (navigator ayrı çağırır) | D2: commit() içine |
| 5 | CommitOutcome trajectory fields | ✅ (navigator PredicateGate ayrı) | D2: commit() extension |
| 6 | TaskAttempt construct + persistence | ✅ (evidence ledger in-memory) | E: persistent store |
| 7 | Raw→Provenanced bridge | ✅ provenanced_from_raw | — |
| 8 | Loop driver | ✅ run_task (maneuver, retry, complete) | — |
| 9 | Permission gate | ⬜ (stub) | D2+: commit() mask |

D1: 7/9 boşluk kapatıldı. #1, #2 kısmen (D2'de tam). #9 permission D2+.

---

## Aşama D1'de YAPILMAYAN (D2+)

- **Gerçek LLM** (osp-llm-runtime RuntimeLlmClient) — D2
- **OspPrompt genişletme** (space_slice, calibration feedback) — D2
- **commit() içine Q5.b + permission entegrasyon** — D2
- **Real engine feeding** (DecompositionSpace engine'den, gerçek measure) — D2
- **Calibration feedback** (HallucinationType → retry message) — D2
- **Trajectory correction** (commit sonrası replan) — Aşama E
- **Multi-task scheduling** — Aşama E
- **Persistent evidence ledger** (in-memory Vec D1) — E

---

## Test özeti (Aşama D1)

osp-core: 265 → **273 test** (+8), workspace 470 → **478**, -D warnings temiz, fmt temiz.

---

*Bu not `docs/paper2-notes/README.md` disiplinine uyar: karar + gerekçe + kanıt + edge case + paper materyali.*
