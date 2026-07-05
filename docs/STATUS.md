# OSP — Proje Durumu (STATUS)

> **Son güncelleme:** 2026-07-05 (Paper 3 Faz 5.1 tamamlandı, Paper 3 taslak yazımında)
> **Detaylı roadmap:** [`agent-trajectory-roadmap.md`](agent-trajectory-roadmap.md)
> **Invariant spec:** [`invariant-spec.md`](invariant-spec.md)
> **MCP tasarım:** [`mcp-design.md`](mcp-design.md)
> **Paper 2 kanıtları:** [`paper2-notes/`](paper2-notes/)
> **Paper 3 tasarım + kanıtları:** [`concept-anchoring-design.md`](concept-anchoring-design.md) + [`paper3-notes/`](paper3-notes/)

OSP (Ontological Space Protocol / Software Physics) — software architecture analysis +
AI agent navigation. Üç makale stratejisi: **Paper 1** (statik uzay, kanıtlandı v2.6) +
**Paper 2** (dinamik/agent, draft v1.2 yazıldı) + **Paper 3** (Genesis Layer/Concept
Anchoring, **omurga tamam — taslak yazımında**).

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

G2c corpus runner ✅ G2c-1→5 TAMAM (RQ9 kanıt + gerçek LLM + external corpus 26/26 Completed)
osp-sdk (H)       ⬜ — TypeScript/Python/Rust bindings (opsiyonel)
osp-desktop/3D    ⬜ E opsiyonel — trajectory correction UI
Paper 2           ✍️ DRAFT v1.2 yazıldı (docs/paper2-draft-v1.md) — review sonrası arXiv adayı
Paper 3           ✍️ OMRGA TAMAM — taslak yazımında (docs/paper3-draft dalı)
```

## Paper 3 (Genesis Layer / Concept Anchoring) Durumu

Paper 3 omurgası Faz 0-5.1 + 3 hardening PR ile tamamlandı. **Aşama 1 evidence freeze 4 review
turu sonunda sertleştirildi** (canonical-kesme + marker-kaçırma tuzakları yapısal imkânsız).
**Aşama 2 iskelet hazır** (`paper3-draft-v1.md`). Aşama 3 (bölüm dolgu) bekliyor.

### Fazlar

| Faz | Durum | İçerik |
|---|---|---|
| **Faz 0** | ✅ | Spec + 13 golden fixture + `anchoring.fixture.v1` şema |
| **Faz 1** | ✅ | In-memory deterministic MVP — 5 bileşen pipeline |
| **Faz 2** | ✅ | INV-C1..C8 type-level enforcement (compile-time garantiler) |
| **Faz 3a** | ✅ PR30 | AnchorStore trait + serde boundary |
| Faz 3b-c | ⏸ Ertelendi | KuzuDB arşivlendi (Ekim 2025) — successor projeler bekleniyor |
| **Faz 4** | ✅ | Code evidence — CodeEvidenceProvider + evidence-gated ImplementedBy + INV-C6 |
| **Faz 5a** | ✅ PR33a | PredicateStub bridge — TaskCandidate lane + RuleCandidate→PredicateStub + INV-P1 |
| **Faz 5b** | ✅ PR33b | Navigator bridge — MetricThreshold slot binding + Accepted TaskCandidate→Task + INV-P2 |
| **PR35** | ✅ | OperatorCapability hardening (INV-T2 trusted-boundary) |
| **Faz 5.1** | ✅ PR36 | Cross-family translation semantics — CrossFamilyHint + INV-P3 |
| Faz 5.2 | Planlandı | MetricDelta executable + glossary genişletme |
| Faz 5.3 | Planlandı | EvidenceRequired + RelationExists executable |
| Faz 6 | Planlandı | Concept Synthesis (code repo → concept hipotezleri) |
| Faz 7 | Planlandı | Embedding + LLM-assisted candidate generation |
| Faz 8 | Planlandı | Desktop integration (Project Reality Cockpit) |

### Invariant'lar (11 Paper 3'e özgü)

- **INV-C1..C8** (anchoring): embedding proposes/C2 family/C3 candidate isolation/C4 supersede authority/C5 inferred not accepted/C6 code intent hypothesis/C7 explainable/C8 canonicalized
- **INV-P1** (predicate lowering): RuleCandidate → PredicateStub, never ExecutablePredicateSet
- **INV-P2** (binding): keyword hint ≠ executable predicate — operator binding zorunlu
- **INV-P3** (translation): ambiguity-preserving — translation proposes candidate meaning, binding creates commitment

### Paper 3 kanıtları (Aşama 1 evidence freeze sertleştirildi)

- 18 type-level trybuild compile-fail (11 Paper 3'e özgü: INV-C + INV-P)
- 450+ osp-core testi, 13 golden fixture + **5 held-out adversarial** (4 held_out + 1 regression_anchored)
- **E2E binding chain replay** (Adım 1 gerçek pipeline koşusu) — `e2e-binding-chain-replay.json`
- **E2E rejected paths replay** (4 negatif yol: AxisMismatch, AxisNotInCandidates, TemplateNotSuggested, NotAccepted) — `e2e-rejected-paths-replay.json`
- **§0 pre-flight canonical + marker tablosu** (6 cümle × 4 sütun, gerçek pipeline koşusu) — `paper3_evidence.rs::preflight`
- **5-state conformance** (18 cümle: Conform 12, PartialConform 2, KnownLimitation 2, RejectAsExpected 2) — `conformance-results.json`
- **Run-metadata** (volatile'lerin tek evi: commit hash, sha256) — `run-metadata.json`
- 7 faz evidence dosyası (`paper3-notes/faz*.md`)
- **Aşama 2 iskelet:** `docs/paper3-draft-v1.md`

#### Snapshot disiplini (A5)
- Normal CI: `cargo test -p osp-core --test paper3_evidence --test paper3_heldout` (drift yakalar)
- Dondurma: `PAPER3_FREEZE=1 cargo test -p osp-core --test {target} -- --ignored --nocapture`
- *"Test altına alınmayan invariant ihlal edilir."*

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
| **G2c-5 external corpus** | ✅ done | **required** | RQ6-9 external: chalk/click/cobra 3 dil, 26/26 Completed, ~1100 tok/cell |
| Evidence JSON + failure notes | ✅ done | **required** | data-driven yazım — g2c-external-corpus-results.md + JSON |
| osp-sdk (H) | ⬜ pending | **optional** | ürünleşme, paper'ı geciktirmez |
| 3D UI / trajectory correction (E) | ⏸ paused | **optional** | sunum katmanı |

**Paper 2 minimum gate:** G2 + gerçek LLM corpus deneyleri + evidence JSON + failure notes.
✅ **DOLDU (G2c-5).** H (SDK) ve E (3D) beklenmez — opsiyonel ürünleşme/sunum katmanıdır.

## Çekirdek Disiplinler

1. **Task = measurement predicate** (koordinat değil) — Paper 2 ana tezi.
2. **Hibrit model:** predicate (epistemolojik güven, INV-T1) + coordinate (matematiksel güç, operator-only).
3. **INV-T1..T8** type-level enforced — agent hedef koordinatı göremez, OperatorCapability request'ten gelemaz.
4. **CLI/MCP = truth surface** — osp-core'u bypass etmez, typed access sağlar.
5. **Paper en son** — kanıt önce, data-driven yazım (iddia değil, kanıt).

## Sonraki Adım Önerisi

**Paper 3 Aşama 3 (bölüm dolgu).** `docs/paper3-draft-v1.md` iskeleti hazır.
- Aşama 1 (evidence freeze hardening + held-out + metadata + conformance) TAMAM (4 review turu)
- Aşama 2 (iskelet) TAMAM
- Aşama 3 (bölüm dolgu: abstract ~350 kelime + §1-§12 + Appendix + References) bekliyor
- Detaylı handoff: [`paper3-notes/HANDOFF.md`](paper3-notes/HANDOFF.md)

Paper 2 v1.2 review ile arXiv adayı (docs/paper2-draft-v1.md).
Paper 3 omurga tamam — Faz 6/7 paper sonrasına ertelendi.

## Test Durumu

```
cargo test --workspace --exclude osp-desktop
```
- osp-core: 447 unit + integration (Paper 1/2/3 birleşik, 18 type-level trybuild)
- osp-analyzer: ~148 + 4 smoke
- osp-llm-runtime: ~12
- osp-cli: smoke
- osp-mcp: 8 unit + 7 INV-T1 integration
- osp-spike: ~32
- Toplam: 16+ test grubu, hepsi yeşil

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
