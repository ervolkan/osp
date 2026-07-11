# PR C Plan — Core Axis-Granular Evidence Model (yeni oturum için)

> **Dal:** `feat/core-axis-evidence-model` (main `798d18d` üstünde)
> **Scope:** osp-core only (CLI/guard/paper untouched)
> **4 tur plan review sonucu: implementation-ready**

## Özet

`ObservedCodeEvidence.physical_vector: PhysicalCodeVector` → axis-granular `observations: ObservedPhysicalMetrics`. Per-axis provenance, non-empty/unique/sorted collection, uniform [0,1] validation, zero-strength reject, normative minimum_observed_strength policy. `PhysicalCodeVector` + `PositionVector` unchanged.

## osp-core değişiklikleri (`crates/osp-core/src/anchoring/types.rs` + `code_evidence.rs`)

### A. Validated newtype'lar (uniform [0,1] — tüm 5 axis normalize, axes.rs)

```rust
pub struct PhysicalAxisValue(f64);  // [0,1] finite
impl PhysicalAxisValue { pub fn new(value: f64) -> Result<Self, MetricScalarViolation>; }

pub struct EvidenceCoverage(f64);  // [0,1] finite
impl EvidenceCoverage { pub fn new(value: f64) -> Result<Self, MetricScalarViolation>; }

pub enum MetricScalarViolation { NonFinite, BelowMinimum, AboveMaximum }
```

### B. PhysicalCodeMetricAxis reuse (predicate_lowering.rs canonical — ikinci enum YOK)

Mevcut `PhysicalCodeMetricAxis` (predicate_lowering.rs:113) reuse. 5 axis + `sort_order()`. types.rs yeni enum oluşturma.

### C. Per-axis observation + validated collection

```rust
pub struct ObservedPhysicalMetric { axis, value, source, strength, coverage } // private fields
impl ObservedPhysicalMetric {
    pub fn new(axis, value: f64, source, strength, coverage)
        -> Result<Self, ObservedPhysicalMetricError>;
    //   value [0,1] validation + strength > 0 (ZeroStrength { axis } reject)
}

pub enum ObservedPhysicalMetricError {
    InvalidValue { axis: PhysicalCodeMetricAxis, value: f64, violation: MetricScalarViolation },
    ZeroStrength { axis: PhysicalCodeMetricAxis },
}

pub struct ObservedPhysicalMetrics { values: Vec<ObservedPhysicalMetric> } // private
impl ObservedPhysicalMetrics {
    pub fn try_new(values) -> Result<Self, ObservedPhysicalMetricsError>;
    //   Empty → error; DuplicateAxis → error; sort by sort_order
    pub fn values(&self) -> &[ObservedPhysicalMetric];
    pub fn contains(&self, axis) -> bool;
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ObservedPhysicalMetric>;
    pub fn missing_axes(&self) -> Vec<PhysicalCodeMetricAxis>;
    pub fn minimum_observed_strength(&self) -> EvidenceStrength;
    //   min-over-axes. Coverage katılmaz (upstream confidence zaten coverage içerir).
    //   Missing axes are absent, not zero-strength observations.
    pub fn try_to_physical_vector(&self) -> Result<PhysicalCodeVector, IncompletePhysicalVector>;
    //   All 5 axes → Ok; missing → Err (zero-fill YOK)
}

pub enum ObservedPhysicalMetricsError { Empty, DuplicateAxis { axis: PhysicalCodeMetricAxis } }
pub struct IncompletePhysicalVector { missing: Vec<PhysicalCodeMetricAxis> } // sort_order, private + accessor
```

### D. ObservedCodeEvidence refactor

```rust
pub struct ObservedCodeEvidence {
    code_entity_id: ConceptNodeId,
    observations: ObservedPhysicalMetrics,  // was: physical_vector + metric_source + confidence
    measured_at: u64,
}
impl ObservedCodeEvidence {
    pub fn new(code_entity_id, observations, measured_at) -> Self;
    pub fn observations(&self) -> &ObservedPhysicalMetrics;
}
```

### E. Provider migration (code_evidence.rs)

```rust
fn evidence_strength(...) -> Result<EvidenceStrength, _> {
    Ok(match self.evidence.get(id) {
        Some(ev) => ev.observations().minimum_observed_strength(),
        None => EvidenceStrength::zero(),
    })
}
```

Gate unchanged (presence check). Scorer unchanged (scalar).

### F. "Not 5" doc güncelleme

Zero-strength reject + non-empty + min → "evidence var ama strength=0" artık temsil edilemez. Güçlenme — gate/scorer ayrım korunur ama korunan kenar durum yok. Üç dosyadaki "Not 5" doc comment'lerine cümle eklenir.

---

## Test migration

### 8 runtime construction (3 değer seti)

| Değer seti | Site'lar | Migration |
|---|---|---|
| `(0.42, 0.78, 0.30, 1.1, 5.0)` ×5 | code_evidence:151, scorer:398, gate:640, gate:672, anchoring_mvp:639 | entropy/witness **representative normalized** (0.52, 0.68) — eski raw hatası |
| `(0.1, 0.2, 0.3, 0.4, 1.0)` ×2 | code_evidence:211, scorer:371 | Zaten [0,1] — dokunma |
| `(0.9, 0.8, 0.7, 0.6, 9.0)` ×1 | code_evidence:218 | witness 9.0→**0.9** (soft-norm 9/(1+9)=0.9) |

Her `ObservedCodeEvidence::new(id, PhysicalCodeVector::new(...), source, strength, time)` → `ObservedCodeEvidence::new(id, observations, time)`. Migration örneği `PhysicalCodeVector::new` İÇERMEZ.

Test helper:
```rust
fn auth_service_observations() -> ObservedPhysicalMetrics {
    ObservedPhysicalMetrics::try_new(vec![
        ObservedPhysicalMetric::new(Coupling, 0.42, Scip, EvidenceStrength::new(0.85).unwrap(), EvidenceCoverage::new(1.0).unwrap()).unwrap(),
        ObservedPhysicalMetric::new(Cohesion, 0.78, Scip, EvidenceStrength::new(0.85).unwrap(), EvidenceCoverage::new(1.0).unwrap()).unwrap(),
        // ... Instability 0.30, Entropy 0.52, WitnessDepth 0.68
    ]).unwrap()
}
```

### Compile-fail (24 → 26, .stderr lifecycle)

- `c6_observed_evidence_literal.rs` — field rename (`physical_vector` → `observations`), ad korunur + `.stderr` update
- `c6_intent_carries_physical_vector.rs` → rename `c6_intent_cannot_form_observed_code_evidence.rs` + `.stderr` rename + delete orphan
- **Yeni:** `c6_observed_physical_metrics_literal.rs` + `.stderr`
- **Yeni:** `c6_observed_physical_metrics_deserialize.rs` + `.stderr`

### Runtime unit testler

- `try_new` rejects empty / duplicate axis / sorts by sort_order
- `minimum_observed_strength` heterojen pin ([0.9, 0.6, 0.8] → 0.6) + single axis (0.8 → 0.8)
- `try_to_physical_vector` succeeds (5 axes) / fails (3 → missing [Entropy, WitnessDepth])
- **Uniform [0,1]:** entropy 1.1 → AboveMaximum; entropy 0.52 → Ok
- **Zero-strength reject:** strength=0 → ZeroStrength { axis } error
- **Shape-compatibility:** 3-axis mixed provenance (Coupling TreeSitter + Cohesion Scip + Instability TreeSitter → sort_order + missing [Entropy, WitnessDepth])
- **EvidenceStrength::new(0.0).is_ok()** boundary test dokunulmaz (0 newtype aralıkta geçerli)

---

## Kabul kriterleri

1. `ObservedCodeEvidence` axis-granular observation taşır (PhysicalCodeVector değil)
2. `PhysicalCodeMetricAxis` reuse (ikinci enum YOK)
3. `ObservedPhysicalMetrics::try_new` non-empty + unique + deterministic ordering
4. Uniform [0,1] `PhysicalAxisValue::new(value)` (axis parametresi YOK)
5. Zero-strength reject (`ZeroStrength { axis }`)
6. `minimum_observed_strength()` normative min (coverage katılmaz doc)
7. `try_to_physical_vector` all-5-axes (zero-fill YOK; missing deterministik)
8. `PhysicalCodeVector` + `PositionVector` unchanged
9. Gate/scorer API unchanged
10. Per-axis source/strength/coverage
11. Compile-fail 24 → 26 (literal + intent rename + collection literal + deserialize)
12. Paper untouched; guard untouched
13. Shape-compatibility fixture
14. `ObservedPhysicalMetricError` axis/value context; newtype `MetricScalarViolation`
15. "Not 5" doc güçlenme cümlesi
16. `.stderr` lifecycle (rename/create/delete)

---

## Uygulama sırası (compile-fail erken — Review 1 önerisi)

1. `PhysicalAxisValue` (uniform [0,1]) + `EvidenceCoverage` + `MetricScalarViolation` (types.rs)
2. `ObservedPhysicalMetric` (new → Result, zero-strength reject, `ObservedPhysicalMetricError`)
3. `ObservedPhysicalMetrics` (try_new: non-empty + unique + sort + minimum_observed_strength + contains + iter + missing_axes + try_to_physical_vector)
4. `ObservedPhysicalMetricsError` + `IncompletePhysicalVector` (private + accessor)
5. `ObservedCodeEvidence` refactor (observations field, new constructor, accessors)
6. Compile-fail fixture'ları + .stderr (literal update + intent rename + collection literal + deserialize; 24→26)
7. Provider migration (code_evidence.rs — minimum_observed_strength)
8. Runtime fixture/test migration (8 construction sites, 3 değer seti)
9. Runtime unit testler (invariants + uniform [0,1] + zero-strength + min pin + shape-compat)
10. "Not 5" doc güncelleme (3 dosya)
11. Doküman/count updates (HANDOFF/STATUS/run-metadata; paper untouched; guard untouched)
12. Workspace validation (`RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop`)

## HANDOFF bullet'ler (PR C sonrası)

- PR D dedup listesi: PhysicalCodeMetricAxis reuse, AxisSet/MetricAxisValue/MetricCoverage CLI→core adopt. PR D indivisible (conversion + provider + wiring + guard update + stderr flip).
- `minimum_observed_strength` policy doc.
- `PhysicalCodeVector` unvalidated debt: raw pub fields (NaN coupling enjekte edilebilir) — PR C kapsamı dışı.
- `measured_at` PR D interface: wall-clock kaynağı.
- **v1.4 pending paper edits:** Table C6 fixture adları + trybuild 24→26.
- `PhysicalCodeMetricAxis` placement note: canonical predicate_lowering.rs'te; neutral modüle taşıma future cleanup.

## run-metadata: current protocol 24→26; frozen snapshot 22 unchanged.
