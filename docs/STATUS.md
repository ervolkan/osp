# OSP — Proje Durumu (STATUS)

> **Son güncelleme:** 2026-06-29 (Aşama G1 merge sonrası)
> **Detaylı roadmap:** [`agent-trajectory-roadmap.md`](agent-trajectory-roadmap.md)
> **Invariant spec:** [`invariant-spec.md`](invariant-spec.md)
> **MCP tasarım:** [`mcp-design.md`](mcp-design.md)
> **Paper 2 kanıtları:** [`paper2-notes/`](paper2-notes/)

OSP (Ontological Space Protocol / Software Physics) — software architecture analysis +
AI agent navigation. İki makale stratejisi: **Paper 1** (statik uzay, kanıtlandı) +
**Paper 2** (dinamik/agent, kanıt toplanıyor).

---

## Aşama Durumu

```
osp-core          ✅ A ontoloji + B predicate gate + C planner + D1 navigator + D2 gerçek measure
osp-llm-runtime   ✅ D3 gerçek LLM adapter + D4 calibration feedback
osp-cli           ✅ F1 truth surface (mock + gerçek LLM dispatch)
osp-mcp           ✅ G1 + G2 (operator tools + navigator loop, INV-T2 gate canlı)
osp-analyzer      ✅ Paper 1 (SCIP + tree-sitter, 5 dil)
osp-spike         ✅ Paper 1 korpuslar (svelte, 23 repo)
osp-desktop       ⬜ E (3D viewer donduruldu — Aşama 1-3 hover edge tamam)

G2c corpus runner ⚠️ G2c-1→4 ✅ (RQ9 kanıt + gerçek LLM 2/2 Completed); G2c-5 external corpus kaldı
osp-sdk (H)       ⬜ — TypeScript/Python/Rust bindings
osp-desktop/3D    ⬜ E opsiyonel — trajectory correction UI
Paper 2           ⬜ EN SON — tüm implementation bitince data-driven yazım
```

## Aşama Tablosu

| Aşama | Crate/Dosya | Durum | Açıklama |
|---|---|---|---|
| **A** Ontolojik Tipler | `osp-core/trajectory.rs` | ✅ | Trajectory/Milestone/Task/MetricPredicate, INV-T1..T8 type-level |
| **B** Predicate Gate (Q5.b) | `osp-core/trajectory.rs` | ✅ | `PredicateGate::evaluate`, TaskBoundClaim |
| **B2** TaskAttempt Ledger | `osp-core/trajectory.rs` | ✅ | AttemptOutcome (B'ye entegre) |
| **C** Planner | `osp-core/trajectory.rs` | ✅ | MilestoneDecomposer, deterministic decomposition |
| **D1** Navigator (mock LLM) | `osp-core/navigator.rs` | ✅ | AgentNavigator.run_task, maneuver limit loop |
| **D2** Gerçek measure | `osp-core/engine.rs` | ✅ | `commit_task_claim` atomic Q5.b pipeline |
| **D3** Gerçek LLM adapter | `osp-llm-runtime/adapter.rs` | ✅ | RuntimeLlmClient (GPT-4o-mini) |
| **D4** Calibration feedback | `osp-core/agent.rs` | ✅ | feedback_history + HallucinationType |
| **F1** osp-cli | `osp-cli/` | ✅ | truth surface, mock + gerçek LLM |
| **G1** osp-mcp | `osp-mcp/` | ✅ | rmcp 0.8, 4 tool, INV-T1 canlı doğrulandı |
| **G2** MCP operator tools + navigator loop | `osp-mcp/` | ✅ | trajectory_init, task_add (INV-T2 gate), osp_run_task navigator loop, --llm mock|real |
| **D5** OspPrompt unification | `osp-llm-runtime` | ⬜ | prompt debt giderme (D3 complete_raw → OspPrompt.task_view) |
| **H** osp-sdk | — | ⬜ | TS/Py/Rust bindings (sona bırakıldı) |
| **E** 3D UI + trajectory correction | `osp-desktop/` | ⬜ | donduruldu, opsiyonel |

## Paper 2 Readiness Matrix

Paper 2 yazımı için katman bazında hazırlık durumu (review 4):

| Layer | Status | Paper 2 readiness | Not |
|---|---|---|---|
| osp-core (ontology) | ✅ done | **high** | INV-T1..T8 type-level |
| Predicate gate (Q5.b) | ✅ done | **high** | deterministik, kanıtlandı |
| Planner / decomposition | ✅ done | **medium-high** | deterministic |
| Navigator (mock LLM loop) | ✅ done | **medium** | loop çalışıyor |
| Real LLM adapter (D3) | ✅ done | **medium** | ⚠ prompt debt (D5) |
| Calibration feedback (D4) | ✅ done | **medium** | RQ8 için ölçülecek |
| osp-cli (truth surface) | ✅ done | **high** | evidence export |
| osp-mcp G1 (agent access) | ✅ done | **medium-high** | INV-T1 canlı |
| osp-mcp G2 (operator + loop) | ✅ done | **high** | INV-T2 gate canlı, navigator loop |
| **G2c-1 Corpus runner (harness)** | ✅ done | **required** | RQ6-9 altyapı; 24 cell deterministik |
| **G2c-1b Reject-evidence** | ✅ done | **required** | navigator tüm attempt'ler evidence'a girer (gate_decision) |
| **G2c-2 remove_edges** | ✅ done | **required** | DeltaProposal +removed_edges, OpKind::RemoveImport onurlandırılır |
| **G2c-3 incremental + accumulation** | ✅ done | **high** | RQ9 kanıtlandı: AcceptImprovement→Completed, StrictReject→LimitExceeded |
| **G2c-4 real LLM smoke** | ✅ done | **high** | RQ6/RQ7 preliminary: GPT-4o-mini 2/2 Completed, ~1160-1180 tokens |
| Evidence JSON + failure notes | ⬜ pending | **required** | data-driven yazım |
| osp-sdk (H) | ⬜ pending | **optional** | ürünleşme, paper'ı geciktirmez |
| 3D UI / trajectory correction (E) | ⏸ paused | **optional** | sunum katmanı |

**Paper 2 minimum gate:** G2 + gerçek LLM corpus deneyleri + evidence JSON + failure notes.
H (SDK) ve E (3D) beklenmez — opsiyonel ürünleşme/sunum katmanıdır.

## Çekirdek Disiplinler

1. **Task = measurement predicate** (koordinat değil) — Paper 2 ana tezi.
2. **Hibrit model:** predicate (epistemolojik güven, INV-T1) + coordinate (matematiksel güç, operator-only).
3. **INV-T1..T8** type-level enforced — agent hedef koordinatı göremez, OperatorCapability request'ten gelemaz.
4. **CLI/MCP = truth surface** — osp-core'u bypass etmez, typed access sağlar.
5. **Paper en son** — kanıt önce, data-driven yazım (iddia değil, kanıt).

## Sonraki Adım Önerisi

**G2c-5 — External corpus.** G2c-4 tamam: gerçek LLM (GPT-4o-mini) prompt enhancement ile
2/2 Completed (RQ6 ~1160-1180 tokens, RQ7 smoke outcome). Sıradaki: external cloneable corpus
(chalk/click/cobra) ile paper-ready evidence. Paper 2 minimum gate doluyor (G2c ✅ + corpus +
evidence + failures).

H (SDK) ve E (3D) opsiyonel — paper'ı geciktirmez.

## Test Durumu

```
cargo test --workspace --exclude osp-desktop
```
- osp-core: ~278 unit + 11 integration
- osp-analyzer: ~148 + 4 smoke
- osp-llm-runtime: ~12
- osp-cli: smoke
- osp-mcp: 8 unit + 7 INV-T1 integration
- osp-spike: ~32
- Toplam: 16 test grubu, hepsi yeşil

## Önemli Commit'ler

```
6d57388 G1 osp-mcp (INV-T1 canlı doğrulandı)
ed9dd2f D4 calibration feedback
5fe2ea8 D3 gerçek LLM adapter (GPT-4o-mini)
1f842c1 D2 gerçek engine measure (commit_task_claim)
dea053c D1 navigator loop (mock LLM)
```

## Hızlı Başlangıç

```bash
# Build (osp-desktop Linux CI eksikliği nedeniyle exclude)
cargo build --workspace --exclude osp-desktop

# Test
cargo test --workspace --exclude osp-desktop

# MCP server'ı çalıştır (Claude/Cursor için)
cargo run -p osp-mcp -- --workspace /path/to/repo

# CLI analyze
cargo run -p osp-cli -- analyze --repo /path/to/repo
```
