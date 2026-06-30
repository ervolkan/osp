# Stage D3 — Gerçek LLM adapter (RuntimeLlmClient) Notes (Paper 2 evidence)

> **Aşama:** D3 (RuntimeLlmClient + özel prompt) — TAMAMLANDI
> **Tarih:** 2026-07-01
> **Tez:** "RuntimeLlmClient, gerçek GPT-4o-mini'yi navigator'a bağlar. Özel prompt
> (osp_system_prompt + AgentTaskView JSON) ile LLM gerçek DeltaProposal üretir."
> **Testler:** 3 unit test (osp-llm-runtime 9→12) + gerçek API integration (svelte)
> **Milestone:** OSP'nin dinamik çekirdeği GERÇEK LLM ile çalışıyor.

---

## Milestone: Gerçek GPT-4o-mini çağrısı başarılı

```
osp trajectory attempt 1 --repo P:/repos/osp-spike/svelte --llm real --maneuver-limit 3
→ GPT-4o-mini DeltaProposal üretti:
  - modified_entities: [{entity: "Import", operation: "RemoveImport"}]
  - reasoning: "coupling 0.7 > 0.55, RemoveImport ile düşürelim"
→ D2 navigator measure + PredicateGate çalıştı
→ Maneuver limit exceeded after 3 attempts (INV-T7 aktif)
```

**Bu OSP'nin Paper 2 vizyonunun nihai kanıtı:**
- ✅ Gerçek LLM çağrısı (GPT-4o-mini, OpenAI API)
- ✅ AgentTaskView → system prompt → LLM (INV-T1 — hedef koordinat YOK)
- ✅ LLM DeltaProposal üretti (structural change + reasoning)
- ✅ D2 navigator gerçek measure + commit_task_claim (Q5.b)
- ✅ Maneuver limit aktif (INV-T7)

---

## Karar 1: Özel prompt (3. seçenek — OspPrompt değişmez)

**Karar:** RuntimeLlmClient `complete_raw`'ı custom CompletionRequest ile çağırır.
`system` = osp_system_prompt + trajectory task context, `user` = AgentTaskView JSON.
OspPrompt DEĞİŞMEZ (Paper 1 stub alanları korunur).
**Gerekçe:** OspPrompt ile AgentTaskView arasında sıfır ortak alan. complete_raw bypass
en temiz — OspPrompt'u değiştirmez, AgentTaskView doğrudan serialize.
**Kanıt:** trajectory_system_prompt task_id/coupling/INV-T4 uyarısı içeriyor (unit test).
**Paper materyali:** §1 — "Custom prompt bypass (AgentTaskView, not OspPrompt)."

---

## Karar 2: RuntimeLlmClient — navigator::LlmClient adapter

**Karar:** `RuntimeLlmClient { runtime: Runtime, last_usage: Cell<TokenUsage> }`.
impl LlmClient: complete(view) → complete_raw(custom prompt) → DeltaProposal + token.
**Gerekçe:** Runtime::complete OspPrompt alır, navigator AgentTaskView üretir. Adapter
köprü. last_token_cost TokenUsage → TokenCost map.
**Kanıt:** 3 unit test (trait impl compile, prompt context, error mapping 5 variant).
**Paper materyali:** §3 — "RuntimeLlmClient adapter (real GPT-4o-mini)."

---

## Karar 3: Error mapping (Runtime → navigator LlmError)

**Karar:** Http/Status/MissingApiKey → Network; BadResponse/ProposalParse → ProposalParse.
**Kanıt:** error_mapping_runtime_to_navigator (5 variant test).

---

## Format uyumluluğu (LLM output)

İlk çağrıda GPT-4o-mini `modified_entities`'i yanlış format üretti (`entity`/`operation`
yerine `node_id`). System prompt'a strict JSON format + örnekler eklendi.
**Ders:** LLM'e DeltaProposal şeması net verilmeli — sadece "JSON üret" değil, field
önekleri ve değer enum'ları ile. Bu calibration feedback (D4) için materyal.

---

## osp-cli --llm flag

`osp trajectory attempt --llm mock` (FileMockLlm, D1/F1) veya `--llm real`
(RuntimeLlmClient, GPT-4o-mini). Generic dispatch (run_navigator<L: LlmClient>).

---

## RQ6/RQ7 Evidence (ilk gerçek veri)

- **RQ6 (token cost):** RuntimeLlmClient.last_token_cost() gerçek prompt/completion tokens.
  D1 mock 0'dı; D3 gerçek maliyet.
- **RQ7 (task success):** Maneuver limit exceeded (3 reject) — gerçek task success ratio
  ölçülebilir (Completed vs ExceededManeuverLimit).

**Not:** Evidence kayıt (commit_task_claim hata yolunda evidence kaydetmeme) bir
iyileştirme noktası — navigator reject'leri evidence'a yazmalı. D4'te düzeltme.

---

## Test özeti (D3)

osp-llm-runtime: 9 → **12 test** (+3 adapter), workspace build temiz.
Gerçek API integration: GPT-4o-mini svelte corpus'ta çalıştı.

---

*Bu not `docs/paper2-notes/README.md` disiplinine uyar.*
