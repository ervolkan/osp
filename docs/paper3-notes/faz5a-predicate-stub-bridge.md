# Faz 5a (PR33a) Evidence — PredicateStub Bridge

> **Tarih:** 2026-07-03
> **Durum:** ✅ Tek PR — review + commit
> **Tasarım:** [`docs/concept-anchoring-design.md`](../concept-anchoring-design.md) v0.2.1+, §11 Faz 5, INV-P1 (yeni), D16 (yeni)

## Özet

Faz 5 (Task/Predicate → Paper 2 navigator bridge) iki PR'a bölündü. **PR33a (bu):**
TaskCandidate üretim yolu + RuleCandidate → PredicateStub lowering + Candidate→Accepted
promotion. **Navigator'a bağlanmaz** — INV-T2 ihlal yok.

Ana tez (iki değerlendirmenin de vurguladığı):

```
A rule is not a predicate. A predicate is a rule whose measurable slots have been bound.
A PredicateStub is not absence of knowledge; it is structured uncertainty.
A task may be born from intent, but a predicate is born only after measurement slots are bound.
```

## INV-P1 (yeni invariant, D16)

```
INV-P1 — RuleCandidate is not PredicateSet.
Ölçülebilir slotları bağlanmamış RuleCandidate, ExecutablePredicateSet üretemez.

INV-P1a (PR33a) — RuleCandidate lowering PredicateStub üretir, ExecutablePredicateSet DEĞİL.
INV-P1b (PR33b) — PredicateStub → ExecutablePredicateSet sadece slot binding (operator/
                  evidence-backed) ile.
```

D16 modelleme kararı: `PredicateStub` boş bir "bilmiyorum" DEĞİL — neyi bilmediğini
(`unresolved_slots`), neden bilmediğini (`reason`), hangi kalıplara uyabileceğini
(`suggested_templates`) ölçülü şekilde temsil eder. **Structured uncertainty.**

## 8 onay patch'i (review'dan, uygulandı)

| Patch | Karar |
|---|---|
| 1 | `PredicateStub` private fields + public smart constructor (Faz 4 ObservedCodeEvidence paterni) |
| 2 | Smart constructor non-empty consistency (structured uncertainty type-level) |
| 3 | `PredicateLoweringOutcome` PR33a'da sadece `Stub` (RequiresOperatorBinding PR33b) |
| 4 | `Accepted TaskCandidate ≠ trajectory::Task` ayrımı test ile sabit (INV-T2) |
| 5 | Mevcut `promote_to_accepted` kullanılır (yeni method yok) |
| 6 | GraphSeed yeni bucket'lar backward-compat (Default, deterministik sıra) |
| 7 | TaskCandidate extraction: task signal + typed `TaskCandidate:` ref only (no NLP) |
| 8 | INV-P1 / D16 dokümana eklendi |

## D1'in son 5 patch'i (uygulandı)

| Patch | Karar |
|---|---|
| S1 | `lower_rule_to_predicate_stub` Result döner (PredicateLoweringError) |
| S2 | Non-RuleCandidate input reject testi (NotRuleCandidate) |
| S3 | PredicateStub consistency testleri (3: empty/no-template-with-suggestions/no-template-allowed) |
| S4 | `completeness()` sabit formül (ALL_SLOTS const, NoTemplateMatch → 0.0) |
| S5 | Serde politikası: PredicateStub Serialize-only, PredicateSlot/TemplateId Serialize+Deserialize |

## D2'nin 3 önerisi (uygulandı)

- `completeness()` skoru (S4) — operator önceliklendirme
- Cross-family translation spike PR33b öncesi (PR33a sadece suggested_templates)
- TaskCandidate ↔ PredicateStub ilişkisi net (PR33a'da stub bağımsız yaşar, attach PR33b)

## Teslim edilenler

### Yeni modül (`predicate_lowering.rs`)
| Tip | Açıklama |
|---|---|
| `PredicateSlot` | Metric/Threshold/Scope/Comparator (Serialize+Deserialize) |
| `ALL_SLOTS` | const [4] — completeness() için sabit |
| `PredicateTemplateId` | MetricThreshold/MetricDelta/EvidenceRequired/RelationExists (stub ID) |
| `PredicateStubReason` | MetricUnresolved/ThresholdUnresolved/ScopeUnresolved/ComparatorUnresolved/NoTemplateMatch |
| `PredicateStub` | private fields + smart ctor + Serialize-only (Deserialize YOK) |
| `PredicateStubError` | EmptyUnresolvedSlots/NoTemplateMatchCannotSuggestTemplate |
| `PredicateLoweringOutcome` | Stub(PredicateStub) — PR33a'da tek variant |
| `PredicateLoweringError` | NotRuleCandidate/InvalidStub |
| `lower_rule_to_predicate_stub()` | Result döner, kind kontrolü, deterministic keyword → suggested_templates |
| `PredicateStub::completeness()` | [0,1] sabit formül |

### Classifier (`classifier.rs`)
- `has_task_signal()` + TASK_SIGNAL_MARKERS

### Extractor (`extractor.rs`)
- DerivesTask üretimi: `has_task_signal` + `find_typed_task_ref` (typed `TaskCandidate:` ref)
- `derive_task_name` YOK (Patch 7 — NLP dışı)

### GraphSeed (`types.rs`)
- 3 yeni bucket: `rule_candidates`, `task_candidates`, `risk_candidates`
- `all_nodes()` deterministik sıra: concepts → decisions → code_entities → rule_candidates → task_candidates → risk_candidates
- Backward-compat: **runtime `GraphSeed` serde derive'a sahip DEĞİL** — `Default` ile uyumlu (yeni field'lar `Vec::default()` = boş başlar). Eski kod `GraphSeed { concepts, decisions, code_entities }` ile çalışmaya devam eder. **`#[serde(default)]` fixture tarafında** (`FixtureGiven` struct'ları, anchoring_fixtures.rs/anchoring_mvp.rs) — eski fixture JSON'ları yeni candidate bucket olmadan parse edilir (D2 non-blocking not netleştirme).

### Store (`store.rs`)
- `seed_trusted` 6 bucket chain (yeni candidate bucket'lar dahil)
- Mevcut `promote_to_accepted` kullanılır (yeni method yok — Patch 5)

### Testler
- `predicate_lowering.rs` unit: 10 test (consistency ×3, non-RuleCandidate reject, completeness ×3, lowering ×3)
- `extractor.rs`: DerivesTask extraction (signal+ref, no-ref, no-both)
- `classifier.rs`: has_task_signal (detect ×1, absent ×1)
- `store.rs`: promote_task_candidate_does_not_create_trajectory_task, graph_seed_candidate_buckets_backward_compatible
- **2 trybuild compile-fail** (INV-P1): literal construct, deserialize. Toplam 12 type-level invariant
- `fix_012_task_candidate_derivation` + `fix_013_rule_to_predicate_stub` fixture'ları

### Fixture şeması
- `FixtureGiven` 3 yeni candidate bucket (`#[serde(default)]` backward-compat) — hem anchoring_fixtures.rs hem anchoring_mvp.rs

## Başarı kriterleri (D1'in listesi)

```
✅ TaskCandidate typed-ref lane canlı (DerivesTask üretimi)
✅ RuleCandidate → PredicateStub üretilebilir (Result, non-RuleCandidate reject)
✅ PredicateStub non-empty structured uncertainty (3 consistency testi)
✅ PredicateStub Serialize-only, Deserialize yok
✅ PredicateStub literal construct edilemez (trybuild)
✅ GraphSeed candidate bucket'ları backward-compatible
✅ TaskCandidate Candidate→Accepted promotion OperatorAcceptance ile
✅ Accepted TaskCandidate trajectory::Task yaratmaz (test ile sabit)
✅ ExecutablePredicateSet PR33a'da üretilemez (INV-P1a)
✅ Navigator'a bağlanılmaz (INV-T2 ihlal yok)
✅ completeness() sabit formül (ALL_SLOTS const)
✅ PredicateSlot/TemplateId Serialize+Deserialize, PredicateStub Serialize-only
```

## Kapsam dışı (PR33b / Faz 5.1'e)

- Gerçek predicate template executable logic (MetricThreshold/Delta/Evidence/Relation)
- Stub → ExecutablePredicateSet lowering (slot binding)
- TaskCandidate → `trajectory::Task` converter
- `PredicateLoweringOutcome::RequiresOperatorBinding(UnresolvedPredicateBinding)`
- OperatorCapability bridge (INV-T2 Task genesis)
- Navigator besleme (`osp_task_add`/`run_task`)
- E2E navigator testi
- Cross-family translation spike
- Deterministic keyword mapping (executable seviye)
- Datalog-like predicate evaluator

## Sırada

- **PR33b:** Navigator bridge + executable predicate template'leri + Task genesis.
- Faz 5.1: Cross-family translation (ConceptualIntent → PhysicalCode) olgunlaştırma.
