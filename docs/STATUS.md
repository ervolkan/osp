# OSP — Proje Durumu (STATUS)

> **Son güncelleme:** 2026-07-11 (Paper 3 v1.3 public manuscript + PR C core axis-granular evidence model — arXiv editorial pass tamam, Zenodo yolunda)
> **Detaylı roadmap:** [`roadmap/paper2-roadmap.md`](roadmap/paper2-roadmap.md)
> **Invariant spec:** [`spec/invariants.md`](spec/invariants.md)
> **MCP tasarım:** [`spec/mcp-design.md`](spec/mcp-design.md)
> **Paper 2 kanıtları:** [`paper2-notes/`](paper2-notes/)
> **Paper 3 tasarım + kanıtları:** [`roadmap/paper3-design.md`](roadmap/paper3-design.md) + [`paper3-notes/`](paper3-notes/)

OSP (Ontological Space Protocol / Software Physics) — software architecture analysis +
AI agent navigation. Üç makale stratejisi: **Paper 1** (statik uzay, kanıtlandı v2.6) +
**Paper 2** (dinamik/agent, draft v1.2 yazıldı) + **Paper 3** (Genesis Layer/Concept
Anchoring, **v1.3 public manuscript — arXiv adayı**).

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
Paper 2           ✍️ DRAFT v1.2 yazıldı (docs/papers/paper2-agent-trajectory.md) — review sonrası arXiv adayı
Paper 3           ✅ v1.3 PUBLIC MANUSCRIPT — arXiv editorial pass tamam (52cc9c9)
```

## Paper 3 (Genesis Layer / Concept Anchoring) Durumu

Paper 3 **v1.3 public manuscript** — first-complete draft + Faz 8a real promotion + threat/limitations
tightening + arXiv editorial pass + PR C axis-granular evidence model tamam. **Zenodo evidence pack hazır** (README + MANIFEST), DOI'ler
için draft deposit bekliyor. **~987 workspace test (osp-desktop hariç, CLI: osp-core 552 lib, osp-cli 108 unit + 21 review_flow + 20 supersede_flow + 12 preview_flow + 9 analyze_bridge_flow + 1 architecture_guards, osp-mcp +2 INV-C11), 0 development marker, 367 kelime abstract.**

### Bu oturumda yapılanlar (PR #37-#42)

| PR | İçerik |
|---|---|
| #37 | Aşama 1 evidence freeze hardening + held-out + metadata + conformance (4 review turu) |
| #38 | Aşama 3 kalp bölümler: Abstract + §2 Motivating Example + §5 Cross-Family Translation |
| #39 | Aşama 4 kalan bölümler: first-complete draft (§1, §3, §4, §6, §7, §8, §9-§12, Appendix, References) |
| #40 | Faz 8a: OperatorReviewSession (INV-C12/C13, real promotion — protokolün eksik organı) |
| #41 | Threat/limitations tightening (InReview sil, promote_to_accepted deprecated, 11 madde) |
| #42 | arXiv editorial pass (Zenodo evidence pack, development markers temiz, public manuscript yüzeyi) |

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
| **Faz 8a** | ✅ PR40-41 | OperatorReviewSession (INV-C12/C13 real promotion) + threat tightening |
| **Faz 8b** | ✅ PR48-51 + CLI | PR #48 ✅ (varyant + INV-C14). PR #49 ✅ (`apply_supersede` + INV-C15 atomic). PR #50 ✅ (`SupersedeSession` + crate-private authority issuer, INV-C15 production invocation). PR #51 ✅ (`mainline_query` deterministic ordering). **CLI accept/reject** ✅ (PR #53): persistent `AnchorStoreSnapshot`, Candidate-only seed, one-shot + interactive, basis-freshness. **CLI supersession** ✅ (PR #54): `osp review supersede`, `node_digest_hex` unconditional, named `SupersedeDigests`, endpoint-specific stale, store-level typed errors (E1 downcast), yön-açık confirmation. **Rich SupersedePreview** ✅ (PR #55). **Analysis bridge** ✅ (PR #56). **Metric projection** ✅ (PR #57). **PR C** ✅ (axis-granular evidence model): `ObservedPhysicalMetrics` per-axis provenance/strength/coverage, zero-strength reject, compile-fail 24→26. Sıradaki: PR D (provider + gate/scorer wiring) |
| Faz 8c | ✅ PR47 | promote_to_accepted kaldırma (legacy path migrate) |

### Invariant'lar (15 Paper 3'e özgü + INV-T2 boundary)

> INV-C14 (PR #48) + INV-C15 (PR #49) eklendi.
> 10 type-enforced genesis + 3 type-enforced lowering/translation (P1-P3) + 2 runtime (C14 projection + C15 transition) = 15.
> Toplam type-enforced = 13 (10 genesis + 3 lowering); 2 runtime-asserted (C14, C15).
> Compile-fail count 26 (PR C: 24→26 — `c6_observed_physical_metrics_literal` + `c6_observed_physical_metrics_deserialize` + `c6_intent` rename; C14/C15 runtime-asserted).

- **INV-C1..C8** (anchoring): embedding proposes/C2 family/C3 candidate isolation/C4 supersede authority/C5 inferred not accepted/C6 code intent hypothesis/C7 explainable/C8 canonicalized
- **INV-C12** (informed acceptance): basis karar anındaki içeriğe karşı node_digest tazelik-doğrulamalı (TOCTOU)
- **INV-C13** (no reviewed operator decision without record): Accepted/Rejected geçişi DecisionRecord ile atomik
- **INV-C14** (acceptance-provenance projection, runtime-asserted — Faz 8b PR #48): `mainline_query ⊆ mainline_history`, SupersededAccepted current projection'da değil, history'de
- **INV-C15** (atomic supersession transition, runtime-asserted — Faz 8b PR #49): `apply_supersede` Accepted→SupersededAccepted + successor edge atomik; incoming committed cardinality; lane-sensitive Supersedes
- **INV-P1** (predicate lowering): RuleCandidate → PredicateStub, never ExecutablePredicateSet
- **INV-P2** (binding): keyword hint ≠ executable predicate — operator binding zorunlu
- **INV-P3** (translation): ambiguity-preserving — translation proposes candidate meaning, binding creates commitment

### Paper 3 kanıtları (Aşama 1 evidence freeze sertleştirildi)

- **Frozen evidence snapshot (Aşama 1):** 18 type-level trybuild compile-fail (11 Paper 3'e özgü: INV-C + INV-P) — **current protocol envanteri 26** (PR C axis-granular collection trybuild'leri sonrası; bkz. run-metadata.md current protocol tablosu)
- 552 osp-core lib testi, 13 golden fixture + **5 held-out adversarial** (4 held_out + 1 regression_anchored)
- **E2E binding chain replay** (Adım 1 gerçek pipeline koşusu) — `e2e-binding-chain-replay.json`
- **E2E rejected paths replay** (4 negatif yol: AxisMismatch, AxisNotInCandidates, TemplateNotSuggested, NotAccepted) — `e2e-rejected-paths-replay.json`
- **§0 pre-flight canonical + marker tablosu** (6 cümle × 4 sütun, gerçek pipeline koşusu) — `paper3_evidence.rs::preflight`
- **5-state conformance** (18 cümle: Conform 12, PartialConform 2, KnownLimitation 2, RejectAsExpected 2) — `conformance-results.json`
- **Run-metadata** (volatile'lerin tek evi: commit hash, sha256) — `run-metadata.json`
- 7 faz evidence dosyası (`paper3-notes/faz*.md`)
- **Aşama 2 iskelet:** `docs/papers/paper3-concept-anchoring.md`

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

**Paper 3 Zenodo → endorsement → arXiv.** Paper 3 v1.3 public manuscript hazır (`52cc9c9`).
- v1.3 first-complete + Faz 8a real promotion + threat tightening + arXiv editorial + PR C axis-granular evidence model TAMAM
- **Sıradaki:** PR D (provider + gate/scorer wiring), Zenodo evidence pack + P1/P2 deposit → 3 DOI → References doldur → endorsement → arXiv
- Evidence pack hazır: `docs/paper3-notes/evidence-pack/` (README + MANIFEST)
- Detaylı handoff: [`paper3-notes/HANDOFF.md`](paper3-notes/HANDOFF.md)

Paper 2 v1.2 review ile arXiv adayı (docs/papers/paper2-agent-trajectory.md).
Paper 3 v1.3 public manuscript — Zenodo yolunda.

## Test Durumu

```
cargo test --workspace --exclude osp-desktop
```
- osp-core: 552 lib unit (503 + 21 AnchorStoreSnapshot/restore + 12 supersede-preview predicates + 14 PR C axis-granular evidence + 2 misc) + 30 integration (anchoring_mvp/fixtures/evidence/heldout/typelevel) = 582; 26 type-level compile-fail (trybuild)
- osp-analyzer: ~148 + 4 smoke
- osp-llm-runtime: ~12
- osp-cli: 108 unit (store_io/repository/seed_file/review_session/mapper/preview-builder/canonical-identity/analysis-bridge/graph-seed-builder/metric-projection) + 21 review_flow + 20 supersede_flow + 12 preview_flow + 9 analyze_bridge_flow + 1 architecture_guards integration
- osp-mcp: 8 unit + 7 INV-T1 integration + 2 INV-C11 agent-surface regression
- osp-spike: ~32
- **Toplam: ~987 workspace test (osp-desktop hariç)**, hepsi yeşil. CI warning-only clippy (`|| true`); bu PR 0 yeni uyarı.

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
