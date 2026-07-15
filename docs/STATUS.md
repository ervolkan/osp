# OSP — Proje Durumu (STATUS)

> **Son güncelleme:** 2026-07-15 (Paper 3 v1.4 derive — Aşama A+B+C+D5+D6 (Zenodo v1.4 DOI + dist LaTeX+PDF) tamam; main `18ee7ef`; D7/D8 arXiv publication pending)
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
tightening + arXiv editorial pass + PR C axis-granular evidence + PR D evidence projection tamam. **Zenodo evidence pack hazır** (README + MANIFEST), DOI'ler
için draft deposit bekliyor. **~1150 workspace test (osp-desktop hariç; osp-core 653 lib, osp-cli 155 unit + 21 review_flow + 20 supersede_flow + 12 preview_flow + 13 analyze_bridge_flow + 9 resolution_flow + 2 architecture_guards, osp-mcp +2 INV-C11), 0 development marker, 367 kelime abstract.**

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
| **Faz 8b+** | ✅ PR48-57 + C/D/E/E2/F/G | PR #48-51 (epistemik çekirdek). PR #53-55 (CLI review/supersede/preview). PR #56-57 (analysis bridge + metric projection). **PR C** ✅ (axis-granular evidence model). **PR D** ✅ (evidence projection + wiring proof). **PR E** ✅ (entity resolution core + persistence contract — `CodeIdentityKey` + `ResolvesTo` + INV-C16 + snapshot v2). **PR E2** ✅ (CLI scheme adoption — binding seeding + `resolve-code-entity`). **PR F** ✅ (evidence identity migration — anti-corruption boundary: `CodeIdentityBindingLookup` + `CodeEvidenceSource` + adapter + EI1-EI8). **PR G** ✅ (lineage-aware effective projection — packet-level derived read model: `ResolvedImplementationExpectation` + pure projector + iki yönlü occurrence-aware ResolutionRecord triangulation + RP1-RP4). Sıradaki: arXiv v1.4. |
| Faz 8c | ✅ PR47 | promote_to_accepted kaldırma (legacy path migrate) |

### Invariant'lar (15 Paper 3'e özgü + INV-T2 boundary)

> INV-C14 (PR #48) + INV-C15 (PR #49) eklendi.
> 10 type-enforced genesis + 3 type-enforced lowering/translation (P1-P3) + 2 runtime (C14 projection + C15 transition) = 15.
> Toplam type-enforced = 13 (10 genesis + 3 lowering); 2 runtime-asserted (C14, C15).
> Compile-fail count 28 (PR E: 26→28 — c16_resolution_application literal + deserialize; C14/C15 runtime-asserted).

- **INV-C1..C8** (anchoring): embedding proposes/C2 family/C3 candidate isolation/C4 supersede authority/C5 inferred not accepted/C6 code intent hypothesis/C7 explainable/C8 canonicalized
- **INV-C12** (informed acceptance): basis karar anındaki içeriğe karşı node_digest tazelik-doğrulamalı (TOCTOU)
- **INV-C13** (no reviewed operator decision without record): Accepted/Rejected geçişi DecisionRecord ile atomik
- **INV-C14** (acceptance-provenance projection, runtime-asserted — Faz 8b PR #48): `mainline_query ⊆ mainline_history`, SupersededAccepted current projection'da değil, history'de
- **INV-C15** (atomic supersession transition, runtime-asserted — Faz 8b PR #49): `apply_supersede` Accepted→SupersededAccepted + successor edge atomik; incoming committed cardinality; lane-sensitive Supersedes
- **INV-P1** (predicate lowering): RuleCandidate → PredicateStub, never ExecutablePredicateSet
- **INV-P2** (binding): keyword hint ≠ executable predicate — operator binding zorunlu
- **INV-P3** (translation): ambiguity-preserving — translation proposes candidate meaning, binding creates commitment

### Paper 3 kanıtları (Aşama 1 evidence freeze sertleştirildi)

- **Frozen evidence snapshot (Aşama 1):** 18 type-level trybuild compile-fail (11 Paper 3'e özgü: INV-C + INV-P) — **current protocol envanteri 30** (PR F evidence identity trybuild'leri sonrası; bkz. run-metadata.md current protocol tablosu)
- 653 osp-core lib testi, 13 golden fixture + **5 held-out adversarial** (4 held_out + 1 regression_anchored)
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

**Paper 3 arXiv v1.4 — Aşama A+B+C+D5+D6 (Zenodo v1.4 + dist derive) tamam; D7/D8 (arXiv publication) pending.**

v1.4 derive ilerlemesi (main `18ee7ef`; dist derive `docs/paper3-v1.4-dist` dalında):
- **Aşama A (evidence authority) ✅:** run-metadata.json v2 frozen/current ayrımı; invariant-evidence-matrix.json (37 entry, machine-readable source of truth); scripts/verify_invariant_evidence.py (34 verified, 0 failed, 0 gaps); EI3-a ARCH-GUARD (`resolution_api_evidence_isolation_guard.rs`); EI4-b gap closure (defense-in-depth regression test); C6 stale fixture rename; DOI kalıcı ifade.
- **Aşama B (ontolojik çekirdek) ✅:** §3.1–3.6 başlık yapısı; §3.4 INV-C16 (4 yüzey + R6/N:1/R7 cardinality); §3.5 Evidence-Identity + Table EI (8 satır); §3.6 Derived-Projection + Table RP (4+1 satır, RP1 admitted-domain); Abstract + Contribution 1 minimum taxonomy.
- **Aşama C (tam manuscript propagation) ✅:** Contributions 5/6 ("six"); §1.3 Terminology; §7.1 (Thirty/16/13/3 + C16 ayrı cümle); §7.5b (4 family evidence scope); §7.6 (üç yüzey preview ayrımı); §9.5 (üç boundary); §10 (3 yeni threat); §11 (rule/risk/ImplementedBy ayrımı).
- **Aşama D6 (dist LaTeX derive) ✅:** `docs/dist/paper3.tex` v1.4 (deterministik converter `scripts/md_to_latex.py --paper-version v1.4` + repo-relative provenance header); `docs/dist/osp-paper3-v1.4.pdf` (27 sayfa, Tectonic 0.16.9); `docs/dist/paper3-build.log` (Tectonic `--print`, missing-glyph=0, undefined-ref=0); v1.3 PDF `old_version_pdf/` arşivlendi. Release-claim validator `scripts/validate_paper3_v14_dist.py` (source/tex/pdf üç kapı: marker manifest + per-layer canonicalization + structural row-key + golden column-spec pattern).
- **Aşama D5 (Zenodo DOI reserve) ✅:** Paper 3 v1.4 New Version — concept DOI `10.5281/zenodo.21220992` (korunur), v1.4 version DOI `10.5281/zenodo.21376820` (v1.3 `21251821` arşivde). License CC-BY-4.0 (değişmedi).
- **Aşama D7/D8 (publication transaction) 🔶:** D7 arXiv upload pending (ertelendi); D8 publish + receipt pending.

Test envanteri (v1.4 Aşama A sonrası): osp-core lib 654, workspace 1153, 30 compile-fail, 0 regression. Validator: `py scripts/verify_invariant_evidence.py` → All verifications passed.

Paper 2 v1.2 review ile arXiv adayı (docs/papers/paper2-agent-trajectory.md).
Paper 3 v1.4 — ontolojik çekirdek + evidence authority tamam; publication transaction sırada.

## Test Durumu

```
cargo test --workspace --exclude osp-desktop
```
- osp-core: 654 lib unit (PR G 604→653 +49 lineage projection; v1.4 Aşama A +1 EI4-b duplicate-live-identity regression) + integration (anchoring_mvp/fixtures/evidence/heldout/typelevel + resolution_api_evidence_isolation_guard EI3-a architecture guard); 30 type-level compile-fail (trybuild) = 28 Paper-3-specific + 2 INV-T2
- osp-analyzer: ~148 + 4 smoke
- osp-llm-runtime: ~12
- osp-cli: 155 unit + 21 review_flow + 20 supersede_flow + 12 preview_flow + 13 analyze_bridge_flow + 9 resolution_flow + 2 architecture_guards integration
- osp-mcp: 8 unit + 7 INV-T1 integration + 2 INV-C11 agent-surface regression
- osp-spike: ~32
- **Toplam: 1153 workspace test (osp-desktop hariç)**, hepsi yeşil. CI warning-only clippy (`|| true`); v1.4 Aşama A 0 yeni uyarı.

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
