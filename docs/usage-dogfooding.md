# OSP Dogfooding Report — OSP Applied to Its Own Codebase

> **Tarih:** 2026-06-24 (Rust edge extraction 2026-06-25 güncellendi)
> **Repo:** osp-core (crates/osp-core/src/, 15 Rust source files)
> **SCIP:** scip-rust (Docker, 296s index time)
> **Amaç:** "OSP gerçekten kullanılıyor mu?" sorusuna somut cevap

---

## 1. Repo Analizi

### Tier 1 (Tree-sitter)

| Metrik | Değer | Yorum |
|---|---|---|
| Source files | 15 | osp-core/src/ altında |
| Nodes | 15 | Her dosya = 1 Module node |
| Edges | 34 | Rust `use` extraction (crate::/super::/self:: + grouped) |
| Abstractness (A) | 0.053 | 4 abstract / 75 total types (trait definitions) |
| Instability (I) | 0.52 | Gerçek Martin I (Ce/Ca edges'ten) |
| Main-seq dist (D) | 0.33 | |0.053 + 0.52 − 1| |

Rust edge extraction tamamlandı (2026-06-25) — coupling/instability artık gerçek değerler.

### Tier 2 (SCIP scip-rust)

| Metrik | Değer | Yorum |
|---|---|---|
| SCIP files indexed | 47 | (workspace gen + osp-core src) |
| SCIP classes detected | 98 | Struct/enum/trait definitions |
| Field accesses | 0 | scip-rust field-access detection pending |
| LCOM4 cohesion (y) | 0.50 | All classes LCOM4=1 (no method-field access data) |
| SCIP coverage | ~90% | 47 files / ~52 total (workspace gen dahil) |

**Not:** scip-rust `field_access` data üretmedi → tüm sınıflar LCOM4=1 → y=0.50 (placeholder-level).
Bu, scip-rust'ın Rust trait/impl field-access semantics'inin henüz tam desteklemediğini gösterir.
Python/TS/JS reposlarında gerçek field-access verisi aldık; Rust için bu bir known limitation.

### Token Benchmark

| Approach | Tokens (chars/4) | Savings vs Full |
|---|---|---|
| Full repo dump | 73.1K | — |
| 2-hop context | 4.9K | 93.3% |
| **OSP coordinate prompt** | **155** | **99.79% (1:472)** |

**Key finding:** OSP prompt'unun boyutu osp-core için de ~155 token — repo boyutundan bağımsız.

---

## 2. Vision Configuration

Architectural targets for osp-core (a foundational library):

```toml
[vision]
x = 0.30    # coupling target (low — library should be self-contained)
y = 0.70    # cohesion target (high — types should be cohesive)
z = 0.50    # instability target (balanced — stable foundation)
w = 0.60    # entropy target (moderate — active development)
v = 0.70    # witness-depth target (high — well-reviewed)

[thresholds]
theta_bound = 0.25
theta_quorum = 1.5
min_approvers = 2
```

---

## 3. Simulated Development Scenarios

10 geliştirme senaryosu — her biri osp-core'a uygulanabilir bir değişiklik:

### Senaryo Tablosu

| # | Senaryo | ΔS | Gate | Result | θ | Hallucination |
|---|---|---|---|---|---|---|
| 1 | Add `osp-server` module (isolated, no imports) | +1 node | Q4-Q6 + Q1-Q3 | **COMMIT** ✅ | 0.21 | — |
| 2 | Add module that imports itself | +1 node, self-loop edge | **Q4** | **REJECT** ❌ | — | Structural |
| 3 | Add module with NaN mass | +1 node (mass=NaN) | **Q4** | **REJECT** ❌ | — | Structural |
| 4 | Add high-coupling module (imports 5 existing) | +1 node, 5 edges | Q4 ✅ → **Q5** | **REJECT** ❌ | 0.68 | Vision |
| 5 | Duplicate existing node ID | +1 node (id=existing) | Q4 ✅, Q5 ✅ → **Q6** | **REJECT** ❌ | — | Rule |
| 6 | Add cohesive module (1 import, well-aligned) | +1 node, 1 edge | Q4-Q6 + Q1-Q3 | **COMMIT** ✅ | 0.18 | — |
| 7 | Zero-vector position (empty module) | +1 node (all zeros) | Q4 ✅ → **Q5** | **REJECT** ❌ | 1.00 | Vision |
| 8 | Add edge to non-existent node | +1 edge (to=999) | Q4 ✅, Q5 ✅ → **Q6** | **REJECT** ❌ | — | Rule |
| 9 | Add module with valid position + 1 witness | +1 node | Q4-Q6 ✅, Q1-Q3 → **HOLD** | **HOLD** ⏸ | 0.22 | Undersupported |
| 10 | Add module with valid position + 2 witnesses | +1 node | Q4-Q6 ✅ + Q1-Q3 ✅ | **COMMIT** ✅ | 0.19 | — |

### Özet İstatistikleri

| Metrik | Değer |
|---|---|
| Toplam senaryo | 10 |
| **COMMIT** | 3 (30%) |
| **REJECT** | 6 (60%) |
| **HOLD** | 1 (10%) |
| Q4 rejects (Structural) | 2 |
| Q5 rejects (Vision) | 2 |
| Q6 rejects (Rule) | 2 |
| Q1-Q3 Hold (Undersupported) | 1 |

### Gate Distribution

```
Q4 Syntax     ██░░░░░░░░  20%  (2/10)
Q5 Vision     ██░░░░░░░░  20%  (2/10)
Q6 Rule       ██░░░░░░░░  20%  (2/10)
Q1-Q3 Witness ░░░░░░░░░░  10%  (1/10 Hold)
PASS ALL      ███░░░░░░░  30%  (3/10 Commit)
```

---

## 4. Token Measurement (osp-core)

| Metric | OSP Prompt | Raw 2-hop | Raw Full |
|---|---|---|---|
| Characters | 620 | 19,600 | 292,400 |
| Tokens (chars/4) | **155** | 4,900 | 73,100 |
| Ratio vs OSP | 1× | 32× | 472× |

---

## 5. Bulgular

### Güçlü yönler
1. **Gate pipeline çalışıyor** — 10 senaryoda 7'si doğru şekilde reject edildi (Q4/Q5/Q6)
2. **Hallucination classification tutarlı** — her reject için doğru tür + calibration message
3. **Token savings dramatic** — 155 vs 73K (472× reduction)
4. **Vision gate anlamalı** — θ=0.68 high-coupling module doğru reject edildi

### Bilinen sınırlamalar
1. ~~**scip-rust field-access boş**~~ → **Çözüldü (2026-06-25):** Kök neden OSP loader bug'ıydı (`impl#[Type]` sembol deseni tanınmıyordu), toolchain limitation değil. Fix sonrası Rust struct method'ları class'lara bağlanıyor, gerçek LCOM4 hesaplanıyor. osp-core cohesion için rerun gerekir (önceki y=0.50 placeholder).
2. ~~**Rust edge extraction=0**~~ → **Çözüldü (2026-06-25):** osp-core artık 34 edges, I=0.52, D=0.33 veriyor.
3. **Mock LLM** — senaryolar manuel tanımlandı, gerçek LLM üretmedi. Adım 2 (real LLM) bunu çözecek.
4. **Tek repo** — n=1, istatistik değil gözlem. Daha fazla repo needed.

---

## 6. Paper'a Eklenecek

Bu veri paper §7.8 "Preliminary Usage Observations" olarak eklenecek:

> We applied OSP to its own codebase (osp-core, 15 Rust files) with a
> configured architectural vision (coupling ≤ 0.30, cohesion ≥ 0.70).
> Of 10 simulated development scenarios, 3 (30%) passed all gates and
> committed; 6 (60%) were rejected at deterministic gates (Q4 syntax,
> Q5 vision θ > 0.25, Q6 rule); and 1 (10%) was held for insufficient
> witnesses. The token benchmark on the same repository showed OSP's
> coordinate prompt at 155 tokens versus 73.1K for a full dump (472×
> reduction, chars/4 approximation).
