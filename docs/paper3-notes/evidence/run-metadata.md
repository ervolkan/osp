# Paper 3 — Run Metadata (Evidence Freeze)

> **Volatile metadata** — invariant/test sayıları protokolle birlikte evrilir;
> **frozen evidence JSON'lar immutable** (aynı kod → bayt-bayt aynı JSON).
> Bu dosya iki zaman katmanını ayırır: (1) frozen koşu anı, (2) güncel protokol envanteri.

## Frozen evidence snapshot (değişmez — provenance zinciri)

| Parameter | Value |
|---|---|
| Evidence generation commit | `ef022a9` (PR #40 — Faz 8a, her iki JSON'ı **gerçekten üreten** commit) |
| Evidence baseline commit | `481690d` (PR #37 — Aşama 1 freeze hardening; corpus + snapshot-compare altyapısı) |
| Packaging branch | `feat/paper3-faz8a-operator-review` (PR #40) |
| Frozen date | 2026-07-05 |
| Rust toolchain | `rustc 1.95.0 (59807616e 2026-04-14)` |
| osp-core tests at generation | 494 (Paper 1/2/3 birleşik; paper3_evidence + paper3_heldout + review.rs unit dahil) |
| Paper-3-specific invariants at generation | 13 (INV-C1..C8, C12, C13, P1..P3) |
| Compile-fail tests at generation | 22 |
| Golden fixtures | 13 (`anchoring.fixture.v1`) |
| Held-out fixtures | 5 (4 held_out + 1 regression_anchored) |
| E2E binding chain | Step 6 REAL promotion via `OperatorReviewSession` (Faz 8a — `ef022a9`) |
| Rejected paths | 6 (AxisMismatch, AxisNotInCandidates, TemplateNotSuggested, NotAccepted, NotFound/StaleBasis, NotPromotableFrom) |

> **Provenance notu (Review PR #48 P1):** `481690d` (PR #37) evidence'ı *üretmedi* — o aşamada
> Step 6 henüz seed edilmişti (commit mesajı: "OperatorAcceptance pub(crate), promotion in-crate
> enforced"). Gerçek promotion yolu, iki yeni invariant (INV-C12/C13) ve evidence'ın yeniden
> dondurulması PR #40 (`ef022a9`) ile geldi. `481690d`, baseline (corpus + altyapı) olarak korunur;
> `ef022a9`, evidence generation commit olarak kaydedilir. Doğrula:
> `git log -1 --format=%H -- docs/paper3-notes/evidence/e2e-binding-chain-replay.json`

## Current protocol metadata (PR G lineage-aware effective projection — feat/resolved-implementation-expectation)

İki eksen: **kapsam** (genesis / lowering / projection / transition / persistence / evidence-identity) × **enforcement** (type-level / runtime-asserted / restore-validated).

| Parameter | Value |
|---|---|
| Current Paper-3-specific invariants | **16** (INV-C1..C8, C12, C13, C14, C15, C16 + P1..P3) |
| ↳ type-enforced genesis (INV-C1..C8, C12, C13) | 10 |
| ↳ type-enforced lowering/translation (INV-P1..P3) | 3 |
| ↳ runtime projection invariant (INV-C14, Faz 8b PR #48) | 1 |
| ↳ runtime atomic transition invariant (INV-C15, Faz 8b PR #49 atomik + PR #50 production invocation) | 1 |
| ↳ runtime atomic transition invariant (INV-C16, PR E entity-resolution transition) | 1 |
| **Toplam type-enforced** (genesis + lowering) | **13** |
| **Toplam runtime-asserted** | **3** (C14 projection + C15 supersession transition + C16 entity-resolution transition) |
| **PR F evidence identity invariantları (EI1-EI8)** | **8 clause-bazlı** (EI1-a TYPE, EI1-b RUNTIME, EI2 RUNTIME, EI3-a TYPE/API, EI3-b RUNTIME, EI4-a/b/c RUNTIME, EI5-a/b TYPE, EI6 RUNTIME, EI7 RUNTIME, EI8-V1 RUNTIME) — ayrı invariant ailesi; INV-Cx sayımına eklenmez (evidence identity layer, concept anchoring layer ile paralel) |
| **PR G projection invariantları (RP1-RP4)** | **5 clause-bazlı** (RP1 soundness RUNTIME, RP2 TYPE/API+RUNTIME/TRUST, RP3 TYPE+serde, RP4-a TYPE/API structural, RP4-b RUNTIME snapshot equality) — ayrı invariant ailesi; INV-Cx/EI sayımına eklenmez (lineage projection layer) |
| Compile-fail test count | 30 (PR F: cF1_resolved_code_identity_literal + cF1_code_identity_key_literal eklendi) |
| `DecisionStatus` variants | 5 (Candidate, Accepted, Deprecated, Rejected, SupersededAccepted) |
| INV-C15 production invocation | `SupersedeSession` (PR #50) — crate-private authority issuer + parametresiz `supersede()` + token içeride mint |
| **PR F evidence identity layer** | `CodeIdentityBindingLookup` (dar public capability) + `CodeEvidenceSource` (key-facing) + `ResolvedCodeEvidenceProvider` adapter + `InMemoryCodeEvidenceSource` (fail-closed builders) + `ResolvedCodeIdentity` (pub ctor) — anti-corruption boundary: graph dünyası ↔ identity dünyası ayrı; tek truth source `HashMap<CodeIdentityKey, ObservedCodeEvidence>` |
| **Restore-validated persistence (CLI)** | `AnchorStoreSnapshot::restore_snapshot` — graph schema + node uniqueness + edge endpoints + record→node/status forward integrity + dense audit_seq (union unique + {1..N} + ==N) + INV-C15 üç yönlü triangulation. paper3 "known gap" cümlesi evaluated path için kapatıldı. |
| **INV-C11 surface classification (CLI)** | MCP = agent-facing (review/supersede authority yok, static regression test); `osp review` CLI = operator-facing (session expose eder — INV-T2 attribution, auth deployment boundary). |
| **Operator review testleri** | osp-core lib 652 (PR G: 604→652 +48: ConceptPacketId Ord/round-trip + DerivedEdgeReference/Lineage/Expectation nested public ctor + ResolvedImplementationBasis + project_resolved_implementations pure projector fail-closed + RP1-RP4 lineage fold + review tur 1: ResolutionRecord triangulation + ExpectedImplementation fail-closed + error dalı fixture + store integration RP4-b); osp-cli 155 unit (PR G untouched) + 21 review_flow + 20 supersede_flow + 12 preview_flow + 13 analyze_bridge_flow + 9 resolution_flow + 2 architecture_guards integration; osp-mcp +2 INV-C11 |

> **Taksonomi notu (Review PR #48/#49 + PR F):** P1-P3 lowering invariant'ları da type-enforced'dur
> (trybuild katmanında, strata tablosu (1) ile tutarlı). "13 type-enforced = 10 genesis + 3 lowering";
> INV-C14 (projection), INV-C15 (supersession transition) ve INV-C16 (entity-resolution transition) runtime-asserted.
> Toplam 16 = 13 type-enforced + 3 runtime. INV-C16'nın iki compile-fail testi yalnız
> `ResolutionApplication` construction-opacity boundary'sini kanıtlar (type-enforced invariant sayısını artırmaz).
> **PR F EI1-EI8** ayrı bir evidence identity invariant ailesidir (concept anchoring INV-Cx ile paralel);
> INV-Cx sayımına dahil edilmez — evidence identity layer'ın kendi clause-bazlı enforcement matrisi.

## Evidence strata (5 katman)

| Stratum | Amaç | Kanıt | Test |
|---|---|---|---|
| **(1) Type-level trybuild** | INV-C1..C8, INV-C12, INV-C13, INV-C16, INV-P1..P3 + supersede opacity + EI1-a evidence identity (genesis + lowering + evidence-identity, type-enforced) | 13 Paper 3'e özgü type-enforced invariant + 2 supersede opacity boundary + 2 EI1-a evidence identity boundary (30 cumulative compile-fail — PR F evidence identity collection) | `tests/anchoring_typelevel.rs` |
| **(2) Golden fixture conformance** | 13 fixture pipeline davranışı | `anchoring_mvp.rs` + `anchoring_fixtures.rs` | `cargo test -p osp-core --test anchoring_mvp` |
| **(3) Held-out adversarial** | 5 cümle totoloji-olmayan RQ1 | `held-out-adversarial-fixtures.json` | `paper3_heldout.rs` |
| **(4) E2E binding chain replay** | Uçtan uca zincir; Step 6 REAL promotion (Faz 8a) | `e2e-binding-chain-replay.json` | `paper3_evidence.rs` |
| **(5) E2E rejected paths replay** | 6 reddedilen kapı (paths 5-6 unit test'lerde) | `e2e-rejected-paths-replay.json` | `paper3_evidence.rs` + `review.rs` unit |

## Evidence JSON dosyaları + sha256

| Dosya | sha256 |
|---|---|
| `e2e-binding-chain-replay.json` | `be733f384a2d443d81243042b6f58362bfc6e847296d74c10e01a3336ebd62f3` |
| `e2e-rejected-paths-replay.json` | `66a9a892e5d67e4a0d5d1fbbd80a99c901616a6396532b0bfc369879a08d334e` |
| `held-out-adversarial-fixtures.json` | `12babf65966e89d99d4d98460369695a3a54a4e52f2deceacbbb40d43fad7a41` |

## Üretim komutları

```bash
# Drift yakalar (normal CI — source tree'yi mutate ETMEZ):
cargo test -p osp-core --test paper3_evidence
cargo test -p osp-core --test paper3_heldout

# Bilinçli dondurma (kod değiştiğinde, evidence güncellenmeli):
PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_evidence -- --ignored --nocapture
PAPER3_FREEZE=1 cargo test -p osp-core --test paper3_heldout -- --ignored --nocapture
```

## Not

- Evidence JSON'lar **saf deterministik builder çıktısıdır** — aynı kod → bayt-bayt aynı JSON.
- Volatile alanlar (commit/tarih/toolchain) burada yaşar, JSON'larda DEĞİL.
- Snapshot testleri `assert_eq!(generated, frozen)` ile drift yakalar; `PAPER3_FREEZE=1` ile
  bilinçli yeniden dondurma. *"Test altına alınmayan invariant ihlal edilir."*
