# Faz 5b (PR33b) Evidence — Navigator Bridge: MetricThreshold Slot Binding

> **Tarih:** 2026-07-03
> **Durum:** ✅ Tek PR — review + commit
> **Tasarım:** [`docs/concept-anchoring-design.md`](../concept-anchoring-design.md) v0.2.1+, §11 Faz 5, INV-P2 (yeni), D17 (yeni)

## Özet

Faz 5 (Task/Predicate → Paper 2 navigator bridge) ikinci yarısı. PR33a PredicateStub
bridge kurdu (TaskCandidate + lowering, navigator'a bağlanmaz); **PR33b köprüyü tamamlar:**
PredicateStub → ExecutablePredicateSet (slot binding) + Accepted TaskCandidate → trajectory::Task
genesis (OperatorCapability). Navigator'a kayıt edilebilir Task üretir.

Ana tez (iki değerlendirmenin de vurguladığı):

```
Accepted intent is not executable work. Task genesis requires operator capability.
A conceptual rule may suggest a physical metric, but only bound slots can create an executable predicate.
Executable task = accepted intent + operator-bound predicate + operator capability.
```

## INV-P2 (yeni invariant, D17)

```
INV-P2 — Cross-family mapping may suggest metric slots, but cannot bind executable
predicates without operator/evidence binding.

INV-P1a (PR33a): RuleCandidate lowering PredicateStub üretir, ExecutablePredicateSet DEĞİL.
INV-P1b (PR33b): PredicateStub → ExecutablePredicateSet sadece slot binding (OperatorCapability) ile.
INV-P2 (PR33b): keyword hint ≠ executable predicate — operator binding zorunlu.
```

D17 modelleme kararı: `task_bridge.rs` modülü `anchoring/` ve `trajectory/` **arasında**
protocol boundary olarak yaşar — ikisini de görür ama birine aidiyet etmez.

## Üç kapılı temiz API (D1/D2 vurguladığı)

```rust
// 1. verify accepted intent (OperatorAcceptance ile promote edilmiş)
pub fn verify_accepted_task_candidate(graph, id) -> Result<AcceptedTaskCandidateRef, TaskGenesisError>;

// 2. bind executable predicate (OperatorCapability — slot binding, INV-P2)
pub fn bind_metric_threshold(stub, binding, cap) -> Result<ExecutablePredicateSet, BindingError>;

// 3. create executable task (OperatorCapability — Task genesis, INV-T2)
pub fn create_task_from_accepted_candidate(accepted, predicates, cap, label, ops, constraints)
    -> Result<Task, TaskGenesisError>;
```

Hiçbir kapı atlanamaz veya bypass edilemez. *"Candidate intent ≠ Accepted intent ≠ executable task."*

## 8 onay patch'i + 8 kontrol (review'dan, uygulandı)

| # | Patch / Kontrol |
|---|---|
| 1 | `create_task` raw PredicateSet değil **ExecutablePredicateSet** alır (blocker) |
| 2 | `bind_metric_threshold` **OperatorCapability** ister |
| 3 | `NormalizedMetricThreshold` [0,1] range-checked newtype (isim D1 öneri 3) |
| 4 | `MetricThresholdBinding` private + smart ctor |
| 5 | `PhysicalCodeMetricAxis` (aileyi açık eden isim, PredicateAxis değil) |
| 6 | `NotAccepted` verify'de, create_task ayrışır |
| 7 | `RequiresOperatorBinding` eklenmez (PR33a "lowering her zaman Stub") |
| 8 | TaskId deterministic candidate-derived (atomic counter değil) |
| K1 | `OperatorCapability` forge edilemez (private field; `issue()` public ama trusted boundary caller) |
| K2 | `ExecutablePredicateSet` non-empty by construction (tek üretim: bind_metric_threshold) |
| K3 | `NormalizedMetricThreshold` is_finite + [0,1] |
| K4 | `bind_metric_threshold` MetricThreshold template kontrolü (TemplateNotSuggested) |
| K5 | Axis hint mismatch reject (AxisMismatch) |
| K6 | `PredicateStub::new` backward-compat (new_with_axis_hint ayrı) |
| K7 | `AcceptedTaskCandidateRef` non-forgeable (private, verify-only) |
| K8 | TaskId deterministic normalize (FNV hash) |

## Teslim edilenler

### Yeni modül `task_bridge.rs` (D17 protocol boundary)
| Tip/Fonksiyon | Açıklama |
|---|---|
| `AcceptedTaskCandidateRef` | non-forgeable verify reference (private id, verify-only) |
| `verify_accepted_task_candidate` | 1. kapı — NodeNotFound/NotTaskCandidate/NotAccepted |
| `create_task_from_accepted_candidate` | 3. kapı — ExecutablePredicateSet + OperatorCapability → Task |
| `create_task_from_accepted_candidate_default_label` | convenience (deterministic label) |
| `TaskGenesisError` | NodeNotFound/NotTaskCandidate/NotAccepted |
| `deterministic_task_id` | FNV hash — candidate ID'den stable TaskId |

### `predicate_lowering.rs` (genişletme)
| Tip/Fonksiyon | Açıklama |
|---|---|
| `PhysicalCodeMetricAxis` | Coupling/Cohesion/Instability/Entropy/WitnessDepth + to_predicate_axis |
| `NormalizedMetricThreshold` | [0,1] + is_finite newtype (custom serde, EvidenceStrength paterni) |
| `MetricThresholdBinding` | private + smart ctor (axis/scope/comparator/threshold) |
| `ExecutablePredicateSet` | private inner + Serialize-only + non-empty by construction |
| `bind_metric_threshold` | 2. kapı — OperatorCapability-gated, template+axis kontrolü |
| `BindingError` | TemplateNotSuggested/AxisMismatch |
| `PredicateStub::new_with_axis_hint` | backward-compat (PR33a `new` delegate) |
| `lower_rule_to_predicate_stub` | axis hinting (coupling→Coupling, çoklu→None) |

### Testler
- `task_bridge.rs` unit: 7 test (verify ×4, deterministic TaskId, create_task, **E2E smoke**)
- `predicate_lowering.rs`: 19 test (PR33a 10 + Faz 5b 9: axis hint lowering ×3, bind ×4, threshold ×2)
- **2 trybuild compile-fail** (INV-P2): ExecutablePredicateSet literal + deserialize. Toplam **14 type-level invariant**
- E2E smoke: RuleCandidate → lower → PredicateStub → bind → ExecutablePredicateSet → verify → create_task → registry.insert → resolve

## Başarı kriterleri (D1 + arkadaşının listesi)
```
✅ Üç kapılı API: verify → bind → create
✅ create_task ExecutablePredicateSet alır (Patch 1)
✅ bind_metric_threshold OperatorCapability ister (Patch 2)
✅ NormalizedMetricThreshold [0,1] newtype (Patch 3)
✅ MetricThresholdBinding private + smart ctor (Patch 4)
✅ PhysicalCodeMetricAxis (Patch 5)
✅ NotAccepted verify'de (Patch 6)
✅ RequiresOperatorBinding eklenmez (Patch 7)
✅ TaskId deterministic (Patch 8)
✅ ExecutablePredicateSet non-empty by construction (K2)
✅ bind MetricThreshold template kontrolü (K4)
✅ Axis hint mismatch reject (K5)
✅ PredicateStub::new backward-compat (K6)
✅ AcceptedTaskCandidateRef non-forgeable (K7)
✅ keyword ≠ executable (INV-P2)
✅ MetricDelta/Evidence/Relation PR33b'de executable DEĞİL (Faz 5.1)
✅ E2E smoke: RuleCandidate → registry-resolvable Task
```

## Kapsam dışı (Faz 5.1 / ayrı PR)
- `OperatorCapability::issue()` → `pub(crate)` hardening (K1 — D1 önerisi, ayrı PR, Paper 2 callers'ı kırar)
- MetricDelta/EvidenceRequired/RelationExists executable logic
- Tam cross-family translation maturation (ConceptualIntent 6-axis → PhysicalCode 5-axis)
- Template-specific slot universe EvidenceRequired/RelationExists için
- `RequiresOperatorBinding(UnresolvedPredicateBinding)` variant
- navigator.run_task mock LLM smoke (PR33c — registry-resolvable kanıtı yeter)
- Datalog-like predicate evaluator
- Candidate→çok task allocator (deterministic ID yetersizse)

## Sırada
- **Faz 5.1:** Diğer 3 template (MetricDelta/Evidence/Relation) + cross-family translation maturation.
- OperatorCapability hardening (ayrı küçük PR).
- navigator.run_task smoke (PR33c — opsiyonel).
