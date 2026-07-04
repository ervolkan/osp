# Faz 5.1 — Cross-Family Translation Semantics (PR36) Evidence

> **Tarih:** 2026-07-03
> **Durum:** ✅ Tek PR — review (3 tur) + commit
> **Tasarım:** [`docs/concept-anchoring-design.md`](../concept-anchoring-design.md) v0.2.1+, §11 Faz 5, INV-P3 (yeni), D18 (yeni)

## Özet

Faz 5.1, ConceptualIntent → PhysicalCode cross-family translation'ın epistemik modelini
sağlamlaştırır. **Executable template eklenmez** — mekanizma (MetricThreshold) PR33b'de
kanıtlandı; bu faz anlam geçişini zenginleştirir. Faz 6 (Concept Synthesis) zeminini hazırlar.

Ana tez (3 tur review'un koruduğu):
> *Translation preserves candidate meaning; binding alone creates commitment.*
> *Çeviri aday anlamı korur; yalnızca bağlama taahhüt yaratır.*

## INV-P3 (yeni invariant, D18)

```
INV-P3 — Translation preserves candidate meaning; binding alone creates commitment.
Cross-family mapping aday anlam üretir; operator/evidence binding olmadan belirsizlik
korunur. Ambiguity candidate sayısından türetilir (computed, stored değil) — yapısal
olarak imkansız invariant. Confidence sıralama/açıklama içindir, aggregate edilmez
(pseudo-probability değil).
```

## 3 tur review patch'leri (özet)

### Tur 1-2 patch'leri
- `Certain` → `SingleCandidate` (epistemik isim — ontolojik kesinlik değil)
- `template_candidates` çıkarıldı (tek truth source — PredicateStub.suggested_templates)
- `suggested_axis` field kaldırıldı → computed legacy accessor (D1-3)
- ambiguity computed (D2-1) — stored değil, yapısal imkansız invariant
- kazanan-hint bütün merge (R1-2) — frankenstein yok
- witness-depth canonical (R1-4) — bare "witness" false-positive kapalı
- LanguageAlias isim (R2-2)
- default_confidence tek yerde (R1-1)
- merge saf fonksiyon + sentetik tie-break unit test (R2-1)
- normalize tam tablo büyük+küçük (R2-2)
- AxisMismatch kesin tip (R2-4)
- sort_order explicit (R1-2)
- multi-axis sıkılaştırma doc (R1-3)

### Tur 3 mikro notları (uygulama sırasında)
- **NFC decomposed test** (Mikro-1): `"I\u{0307}"` decomposed girdi → NFC → precomposed İ → fold
- **to_ascii_lowercase** (Mikro-2): Unicode-aware değil, deterministic eşleşme uzayı
- **witness_depth pattern** (Mikro-3): snake_case form da canonical
- **Option\<CrossFamilyHint\> davranışı** (Mikro-4): None = no metadata; Some(NoAxis) = translation ran, no candidate

## Teslim edilenler

### Yeni tipler (`predicate_lowering.rs`)
| Tip | Açıklama |
|---|---|
| `AxisHintConfidence` | [0,1] + is_finite newtype (custom serde, `one()` + `language_alias_default()`) |
| `AxisHintConfidenceError` | range error |
| `AxisHintSource` | KeywordMatch / LanguageAlias / LegacyDirect + `default_confidence()` |
| `AxisHint` | private + smart ctor: axis, confidence, source, reason |
| `TranslationAmbiguity` | SingleCandidate / MultipleCandidates / NoAxisCandidate (**computed**) |
| `CrossFamilyHint` | private + smart ctor: from/to_family, axis_candidates; ambiguity derived; Serialize-only |
| `CrossFamilyHintError` | InvalidFamilyPair / DuplicateAxis |

### Helper'lar
- `merge_axis_hints(Vec<AxisHint>) -> Vec<AxisHint>` — saf fonksiyon (R2-1), kazanan-hint bütün,
  deterministic sort (confidence desc total_cmp + sort_order asc)
- `normalize_for_axis_match` — NFC + Türkçe fold tablosu (büyük+küçük) + `to_ascii_lowercase`

### PredicateStub genişletme
- `cross_family_hint: Option<CrossFamilyHint>` (source of truth)
- `suggested_axis` field YOK — computed legacy accessor
- `new_with_cross_family_hint` (ana constructor)
- `new_with_axis_hint` deprecated redirect → `single_candidate_legacy`

### Lowering
- KeywordMatch: coupling/cohesion/instability/entropy/witness-depth/witness depth/witness_depth
- LanguageAlias: bağıml/bağımlılık (fold edilmiş: bagiml)
- Kazanan-hint merge + deterministic sort

### bind_metric_threshold
- Tek membership kuralı (R2-2): empty→serbest, contains→OK, else reject
- AxisMismatch (len==1, kesin tip) / AxisNotInCandidates (len≥2)

### Testler
- **40 predicate_lowering testi** (19 eski + 21 yeni Faz 5.1)
- AxisHintConfidence range + serde + defaults
- CrossFamilyHint ambiguity computed, family pair reject, duplicate reject
- merge_axis_hints: kazanan-hint bütün, tie-break source priority, deterministic sort
- lowering: coupling→Single KeywordMatch, bagimlilik→Single LanguageAlias, coupling+bagimlilik→
  Single (collapse), coupling+cohesion→Multiple, witness-depth→Single, bare witness→NoAxis,
  accounting→NoAxis (trivially, iddia değil)
- **Türkçe Unicode:** BAĞIMLILIK/Bağımlılık/bağımlılık → Coupling
- **Decomposed İ** (Mikro-1)
- NoAxis≠NoTemplate (azaltılmalı)
- bind: multi-candidate AxisNotInCandidates, candidate axis OK
- suggested_axis computed legacy
- legacy redirect
- **2 trybuild compile-fail** (INV-P3): CrossFamilyHint literal + deserialize. Toplam **18 type-level invariant**

## Başarı kriterleri (R2'nin kilit listesi)
```
✅ CrossFamilyHint source of truth
✅ suggested_axis computed legacy accessor
✅ ambiguity derived, stored değil
✅ SingleCandidate ontolojik kesinlik taşımaz
✅ duplicate same-axis collapse (kazanan-hint bütün)
✅ merge tie-break saf fonksiyonla testli (sentetik, lowering'de erişilemez)
✅ NoAxisCandidate ≠ NoTemplateMatch
✅ witness-depth/witness depth/witness_depth canonical, bare witness değil
✅ AxisHintConfidence tanımlı tek yerde (language_alias_default)
✅ normalize_for_axis_match deterministic (NFC + tam tablo + to_ascii_lowercase)
✅ explicit sort_order
✅ bind membership kuralı sade (AxisMismatch kesin tip)
✅ executable predicate/template eklenmez
✅ CrossFamilyHint Serialize-only + literal/deserialize trybuild
✅ multi-axis sıkılaştırma doc notu (R1-3)
```

## Kapsam dışı (Faz 5.2/5.3/6/7'ye)
- MetricDelta/EvidenceRequired/RelationExists executable logic
- Operator-override yolu (strict reject'i aşma — Faz 5.2/5.3)
- LLM-assisted axis hint inference (Faz 7)
- Alias/glossary genişletme + ayraç normalizasyonu olgunlaştırma (Faz 5.2)
- Tam ConceptualIntent 6-axis → PhysicalCode 5-axis matematiksel translation
- `unicode-normalization` crate ile gerçek NFC (Mikro-1 decomposed — şu an fold tablosu precomposed yakalar)
- Datalog-like predicate evaluator

## Sırada
- **Faz 5.2:** MetricDelta executable (Paper 2 progress checkpoint ile bağlantı) + glossary genişletme
- **Faz 5.3:** EvidenceRequired + RelationExists executable
- **Faz 6:** Concept Synthesis (code repo → concept hipotezleri)
