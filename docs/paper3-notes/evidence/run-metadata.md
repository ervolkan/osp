# Paper 3 — Run Metadata (Evidence Freeze)

> **Volatile metadata** — invariant/test sayıları protokolle birlikte evrilir;
> **frozen evidence JSON'lar immutable** (aynı kod → bayt-bayt aynı JSON).
> Bu dosya iki zaman katmanını ayırır: (1) frozen koşu anı, (2) güncel protokol envanteri.

## Frozen evidence snapshot (Aşama 1 freeze — değişmez)

| Parameter | Value |
|---|---|
| OSP commit (frozen evidence) | `481690d` (Aşama 1 freeze, PR #37) |
| Branch (evidence generated from) | `feat/paper3-faz8a-operator-review` (PR #40 packaging context) |
| Frozen date | 2026-07-05 |
| Rust toolchain | `rustc 1.95.0 (59807616e 2026-04-14)` |
| osp-core tests at freeze | 494 (Paper 1/2/3 birleşik; paper3_evidence + paper3_heldout + review.rs unit dahil) |
| Paper-3-specific invariants at evidence generation | 13 (INV-C1..C8, C12, C13, P1..P3) |
| Compile-fail tests at evidence generation | 22 |
| Golden fixtures | 13 (`anchoring.fixture.v1`) |
| Held-out fixtures | 5 (4 held_out + 1 regression_anchored) |
| E2E binding chain | Step 6 REAL promotion via `OperatorReviewSession` (Faz 8a) |
| Rejected paths | 6 (AxisMismatch, AxisNotInCandidates, TemplateNotSuggested, NotAccepted, NotFound/StaleBasis, NotPromotableFrom) |

## Current protocol metadata (INV-C14 sonrası — evrilir)

| Parameter | Value |
|---|---|
| Current Paper-3-specific invariants | 14 |
| ↳ type-enforced (INV-C1..C8, C12, C13) | 10 |
| ↳ runtime projection invariant (INV-C14, Faz 8b PR #48) | 1 |
| ↳ lowering/translation invariants (INV-P1..P3) | 3 |
| Compile-fail test count (unchanged) | 22 (INV-C14 runtime-asserted, type-level DEĞİL) |
| `DecisionStatus` variants | 5 (Candidate, Accepted, Deprecated, Rejected, SupersededAccepted) |

## Evidence strata (5 katman)

| Stratum | Amaç | Kanıt | Test |
|---|---|---|---|
| **(1) Type-level trybuild** | INV-C1..C8, INV-C12, INV-C13, INV-P1..P3 compile-time | 13 Paper 3'e özgü type-enforced invariant (22 cumulative compile-fail) | `tests/anchoring_typelevel.rs` |
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
