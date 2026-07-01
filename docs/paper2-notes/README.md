# Paper 2 Notes — Architectural Trajectory Navigation

> **Amaç:** Paper 2 ("Architectural Trajectory Navigation") için kanıt toplama.
> **Disiplin:** Paper en son yazılır. Bu dizin, implementasyon sırasında ortaya
> çıkan kararları, ölçümleri, edge case'leri toplar. Paper 2 bu notlardan
> **data-driven** yazılır (iddia değil, kanıt).
> **İlişki:** `docs/agent-trajectory-roadmap.md` (omurga) → bu dizin (kanıtlar)

## Dizin Yapısı

```
docs/paper2-notes/
  README.md                    ← bu dosya (indeks + disiplin)
  stage-A-ontology.md          — Aşama A: ontolojik kararlar, invariant ispatları
  stage-B-predicate-gate.md    — Aşama B: Q5.b deterministik reddin etkisi
  stage-C-planner.md           — Aşama C: task dematerialization, axis oscillation (F5)
  stage-D-agent-loop.md        — Aşama D: mock navigator loop
  stage-D2-real-measure.md     — Aşama D2: gerçek engine measure + commit_task_claim
  stage-D3-real-llm.md         — Aşama D3: gerçek LLM adapter (GPT-4o-mini)
  stage-D4-calibration.md      — Aşama D4: calibration feedback (LLM retry optimization)
  stage-F-osp-cli.md           — Aşama F1: CLI truth surface
  stage-G1-osp-mcp.md          — Aşama G1: MCP server (INV-T1 canlı doğrulama)
  stage-G2-osp-mcp-operator-navigator.md — Aşama G2: operator tools + navigator loop
  stage-G2c-corpus-runner.md   — Aşama G2c: corpus experiment runner (harness MVP)
  stage-G2c-1b-reject-evidence.md — Aşama G2c-1b: navigator reject-evidence (gate_decision)
  stage-G2c-2-remove-edges.md    — Aşama G2c-2: DeltaProposal +remove_edges (ontolojik dürüstlük)
  stage-G2c-3-incremental-accumulation.md — Aşama G2c-3: RQ9 policy accumulation (ilk kanıt)
  evidence/                    — ham ölçümler (JSON), corpus sonuçları
    g2c-corpus-results.md      — G2c-1 mock harness results + threats/limitations
```

## İlişkili Formal Spec

`docs/invariant-spec.md` — Aşama A kodlamadan ÖNCE INV-T1..T7 + mevcut INV #1..#15'in
formal tanımı. Her invariant: yapısal garanti (type-level) + test + ihlal örneği.
Review 1'in önerisi: kod yazarken epistemolojik sınırların bulanıklaşmasını önlemek.

## Not Yazma Disiplini

Her implementasyon aşaması bitiminde o aşamanın notu yazılır. Not şunları içerir:

1. **Karar:** Ne karar verildi (örn. "ε tolerance Trajectory parçası başına")
2. **Gerekçe:** Neden (örn. "tek commit'te coupling 0.82→0.55 inmez")
3. **Kanıt:** Ölçüm/test (örn. "svelte corpus'ta StoreRepository 7 commit'te indi")
4. **Edge case:** Karşılaşılan tuaf durum (örn. "multi-axis predicate margin sorunu")
5. **Paper materyali:** Bu not Paper 2'nin hangi bölümüne gider

## Paper 2 Bölüm Haritası (notlar → bölümler)

| Paper 2 bölümü | Not kaynağı |
|---|---|
| §1 Trajectory ontolojisi | stage-A-ontology.md |
| §2 Task dematerialization | stage-C-planner.md |
| §3 Adaptive control loop | stage-D-agent-loop.md, stage-D3-real-llm.md |
| §4 Deterministic predicate gating | stage-B-predicate-gate.md, stage-D2-real-measure.md |
| §5 Token cost (RQ6) | stage-D-agent-loop.md + stage-D4-calibration.md + evidence/ |
| §6 Task success (RQ7) | stage-D-agent-loop.md + evidence/ |
| §7 Epistemic projection (RQ5) | stage-G1-osp-mcp.md (INV-T1 canlı doğrulama) |

## Mevcut Durum (2026-06-29)

- **Aşama A-G1:** TAMAMLANDI (ontoloji → predicate gate → planner → navigator → gerçek
  measure → gerçek LLM → calibration feedback → CLI → MCP)
- **Aşama G2:** Operator-only tools, WorkspaceRegistry, navigator loop (gelecek)
- **3D viewer:** DURDURULDU (Aşama E için gerekli, ama agent işleri öncelik)
- **Paper 1:** Tamamlandı (statik uzay, kanıtlanmış)

## RQ Adayları (Paper 2 için)

- **RQ6:** OSP trajectory prompt, raw dump'a göre token maliyetini düşürür mü?
  (Paper 1 RQ5'in dinamik uzantısı — prompt-size değil, task-boyunca toplam)
- **RQ7:** Predicate gate'li agent, gatesiz agent'a göre task success'i artırır mı?
  (Kod kalitesi: coupling/cohesion ölçümü)
- **RQ8 (opsiyonel):** Trajectory correction, tek-shot planlamadan daha iyi mi?
  (Adaptive control loop'un değeri)

---

*Bu dizin `docs/agent-trajectory-roadmap.md` §10 ile uyumludur.*
