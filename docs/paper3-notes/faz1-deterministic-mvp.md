# Faz 1 Evidence — In-Memory Deterministic MVP

> **Tarih:** 2026-07-02
> **Durum:** ✅ Tamamlandı — review pass + commit
> **Tasarım:** [`docs/concept-anchoring-design.md`](../concept-anchoring-design.md) v0.2.1, §11 Faz 1

## Özet

Paper 3 Genesis Layer'ın **ilk çalışan deterministic proof-of-mechanism**'i.
Concept Anchoring artık sadece dokümanda duran bir fikir değil — gerçek bir pipeline.

```
input text
  → classifier
  → extractor
  → scorer
  → gate
  → anchor plan
  → store
```

**Üstelik LLM, embedding ve KuzuDB olmadan** — OSP disiplini (§11) korundu.

## Teslim edilenler

### 5 bileşen pipeline (`crates/osp-core/src/anchoring/`)

| Bileşen | Dosya | Sorumluluk |
|---|---|---|
| **Classifier** | `classifier.rs` | Rule-based packet type sınıflandırma (7 precedence), runtime Glossary |
| **Extractor** | `extractor.rs` | Glossary/typed-prefix/rule/risk → `ExtractedAnchorCandidate` |
| **Scorer** | `scorer.rs` | Lexical 7+2 bileşenli skor (INV-C1: skalar similarity, vector görmez) |
| **Gate** | `gate.rs` | §8.2 threshold + INV-C7 explanation + INV-C8 canon gate + §6.4.1/§8.6 |
| **Store** | `store.rs` | `InMemoryAnchorStore` + `OperatorAcceptance` (INV-C3 capability) |
| Pipeline | `pipeline.rs` | Stateless facade (store dışında) |
| Types | `types.rs` | Runtime domain tipleri |
| Helpers | `typed_ref.rs`, `edit_distance.rs` | Merkezî parser, Levenshtein ≤2 |

### 10 golden fixture gerçek pipeline'dan geçiyor

`crates/osp-core/tests/anchoring_mvp.rs` — her fixture: seed → classify → extract → score → gate → apply_plan.

### Test kapsamı

```
387 test yeşil, 0 başarısız
├── 64 anchoring unit testi (her modülde)
├── 11 Faz 0 fixture format testi (API stabilitesi korundu)
├── 10 Faz 1 MVP integration testi
└── 290+302 mevcut Paper 1/2 + osp-core testi (regresyon yok)
```

## INV doğrulamaları (Faz 1 runtime-level)

| INV | Durum | Kanıt |
|---|---|---|
| **INV-C1** (embedding proposes, never decides) | ✅ Type shape | Scorer `semantic_similarity: f64` skalar görür; embedding vector erişim alanı dışında. Faz 1: placeholder 0.0 |
| **INV-C3** (candidate isolation) | ✅ Runtime | `apply_plan` hep `Candidate` yazar; `promote_to_accepted` `OperatorAcceptance` ister; `mainline_query` Accepted filtre |
| **INV-C7** (high-stake explainable) | ✅ Runtime | Gate high-stake edge'lerde `explanation` zorunlu → `GateError::MissingExplanation` |
| **INV-C8** (concept canonicalized) | ✅ Runtime | Canon gate 3 katman: exact canonical + glossary alias + edit distance ≤2 → `CanonicalRedirect` (error değil) |

## Epistemik çizgiler (korundu)

```
Beklenti ≠ kanıt          → ExpectedImplementation ≠ ImplementedBy
Yorum ≠ ölçüm             → INV-C6 (Faz 1'de Concept Synthesis yok)
Candidate ≠ Accepted      → INV-C3 (OperatorAcceptance capability gate)
Embedding ≠ ontolojik konum → INV-C1 (scorer vector görmez)
```

### `ExpectedImplementation` ≠ `ImplementedBy`

En kritik ayrım: Faz 1'de code analizi yok, bu yüzden:
- `ExpectedImplementation` → `CodeEntityCandidate` **ALLOWED**
- `ImplementedBy` → `CodeEntity` **DISALLOWED** (`GateError::ImplementedByRequiresCodeEvidence`)

Code evidence Faz 4'te gelecek; o zamana kadar "implemented" iddiası üretilemez.

### `OperatorAcceptance` capability boundary

```rust
pub struct OperatorAcceptance { _private: () }
impl OperatorAcceptance { pub(crate) fn issue_for_tests() -> Self { ... } }
```

- `_private: ()` field → dış crate struct literal ile üretemez
- `issue_for_tests` `pub(crate)` → sadece osp-core içi (unit testler)
- Integration test (`tests/`) ve downstream (osp-cli/mcp) **üretemez**
- Faz 8 operator console bu gate'i gerçek API ile açar

## Review pass doğrulamaları (commit öncesi)

| # | Kontrol | Sonuç |
|---|---|---|
| 1 | Public API yüzeyi minimal | ✅ `ensure_node_exists` kaldırıldı (dead code) |
| 2 | OperatorAcceptance downstream-proof | ✅ `_private` + `pub(crate)` |
| 3 | Fixture expected = spec (classifier'a uydurulmuş değil) | ✅ fix_007 Contradicts sertleştirildi |
| 4 | AnchorPlan okunabilir rapor | ✅ `summary()` multi-line rapor |
| 5 | CanonicalRedirect = successful redirect (error değil) | ✅ `AnchorPlan.redirects` |
| 6 | ImplementedBy/Supersedes gate seviyesinde kesin | ✅ `validate_edge_kind` tüm candidate'ler |
| 7 | `apply_plan` hiçbir koşulda Accepted yazmıyor | ✅ hep `DecisionStatus::Candidate` |

## Faz 1 → Faz 2 geçişi

Faz 1 "çalışıyor" ama INV'ler runtime-level. Faz 2 = **invariant enforcement hardening**:

1. INV-C1 type-level (EmbeddingVector scorer'a ulaşamaz)
2. INV-C2 PositionVector family enforcement (karışmaz)
3. INV-C3 Candidate → Accepted capability sertleştirme
4. INV-C4 SupersedeAuthority capability-gated edge
5. INV-C7 NonEmptyExplanation (high-stake path)
6. INV-C8 CreateNode path CanonGate'ten geçmeden çalışamaz (type-level)
7. AnchorPlan audit output (JSON/Markdown explainable rapor)

**Faz 2 tamamlanmadan demo/oyuna geçilmemesi** önerisi (değerlendirme) kabul edildi.

## Çıktı komutu

```bash
cargo test -p osp-core anchoring
# 64 unit + 11 fixture + 10 mvp = 85 anchoring testi yeşil
```
