# Stage D4 — Calibration Feedback Notes (Paper 2 evidence)

> **Aşama:** D4 (Calibration feedback — LLM retry optimization) — TAMAMLANDI
> **Tarih:** 2026-07-01
> **Tez:** "Reject edilen DeltaProposal'ın nedenini LLM'e geri besle. LLM aynı hatayı
> tekrarlamaz — token maliyeti düşer (RQ6), task success artar (RQ7)."

## Mimari

### 1. AgentTaskView'a feedback_history alanı eklendi
`feedback_history: Vec<String>` — önceki attempt'lerin calibration mesajları. INV-T1
uyumlu (hata mesajı, koordinat değil). `#[serde(default)]` backward compat.

### 2. Navigator loop'te calibration feedback accumulation
İki reject yolunda feedback üret + feedback_history'ye push:
- **Q4 syntax reject:** "Structural hallucination — {detail}. Fix schema and retry."
- **commit_task_claim error:** `HallucinationType::from_engine_error` + `calibration_message()`
  → "Vision hallucination: θ=0.8 > bound=0.3..." gibi.

### 3. RuntimeLlmClient — system prompt'a feedback ekle
trajectory_system_prompt feedback_history'yi "PREVIOUS ATTEMPTS FAILED" bölümü olarak
ekler: "learn from these errors, do NOT repeat these mistakes."

## RQ6/RQ7 etkisi (Paper 2)
- **RQ6 (token cost):** LLM aynı hatayı tekrarlamaz → daha az wasted attempt → token düşer.
- **RQ7 (task success):** LLM hatadan öğrenir → success ratio artar.
- **INV-T7 (maneuver limit):** Daha az reject → maneuver limit'e çarpma azalır.

## Doğrulama
- workspace 483 test, 0 fail
- gerçek API test (svelte, GPT-4o-mini): navigator 3 attempt — D4 feedback aktif

## HallucinationType (mevcut agent.rs'den reuse)
- `from_engine_error(EngineCommitError) -> Option<HallucinationType>` — 5 variant
- `calibration_message() -> String` — LLM-okur hata mesajı
- D4 bu mekanizmayı navigator loop'a bağladı (daha önce bağlanmamıştı).
