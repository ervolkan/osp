# INV-T9 #70 Commit 2 — Provenance-Aware Position Measurement Handoff

**Tarih:** 2026-07-20
**Branch:** `fix/inv-t9-witness-suspension`
**PR:** #69 (https://github.com/ontologicalspace/osp/pull/69)
**Current head:** `0d4eb51` (Commit 1 + v6/v7 closure landed)
**Issue:** #70 (https://github.com/ontologicalspace/osp/issues/70)
**Plan approval:** Reviewer v3 APPROVE WITH REQUIRED CLARIFICATIONS 9.7/10 — implementation-ready

---

## Yeni oturumda ilk komut

```bash
cd P:/Work/SoftwarePhysics
git checkout fix/inv-t9-witness-suspension
git pull  # 0d4eb51 head olmalı
git log --oneline -5
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 923 test geçmeli
```

Sonra bu belgeyi oku, Commit 2 implementation'a başla.

---

## Commit 1 durumu (tamamlandı — reviewer 9.9/10 APPROVED)

**3 commit ile landed:**
- `a300d75` — Commit 1: `feat(coords): provenance-native axis measurement contract`
- `6aaeb39` — v6 closure: `Mixed source fail-closed + SCIP wiring table test`
- `0d4eb51` — v7 closure: `set-level Mixed fail-closed (Any/Weighted bypass)`

### Commit 1 kazanımları
- `MetricSource::Mixed` varyantı + `descriptor_id()` stable byte ID
- `AxisMeasurement { value, source }` + `try_new` validation + custom Deserialize (`deny_unknown_fields`)
- `MeasuredRawPosition` (5 core axis provenance'lı ölçüm, neutral)
- `AxisSourceError::MixedCannotBeDeclaredDirectly` + `validate_direct_source`
- `Axis` trait: `measure()` authoritative + `try_compute()` default + `compute()` legacy
- CouplingAxis/InstabilityAxis: `try_with_source` (Mixed reddeder)
- CohesionAxis: `effective_fallback` (observational equivalence), `try_from_normalized`, `try_with_observed_source`
- EntropyAxis/WitnessDepthAxis: sabit Heuristic
- Preset `default_raw_three/five(topology_source, ...)` — Coupling + Instability aynı graph topology
- `MetricPredicate::evaluate` predicate-level Mixed fail-closed guard (v6 closure)
- `PredicateSet::evaluate_completion` set-level Mixed fail-closed preflight (v7 closure — Any/Weighted bypass kapandı)
- `CanonicalMetricSourceTag::Mixed = 4`
- `MetricSourceRejection::MixedSourceNotAdmitted` (osp-cli evidence projection)
- Analyzer `node.cohesion` exhaustive source match (yalnız gerçek SCIP → `Some`)
- `MEASUREMENT_SEMANTICS_VERSION: 1 → 2` (schema v1 unchanged)
- `MEASUREMENT_SEMANTICS_V2_GOLDEN_HEX` real axis fixture
- `AUTHORIZATION_V1_GOLDEN_HEX` unchanged (sentinel `[0x11;32]` koruması)
- Workspace-wide migration: production (TreeSitter+Scip) + synthetic/test (Placeholder)

### CI doğrulaması (Commit 1 sonrası)
- **923 osp-core lib tests** (888 → +35)
- Workspace total (excl. osp-desktop): **1424 tests green**
- `cargo fmt` clean
- `cargo clippy -p osp-core --lib` — 13 warning (HEAD parity, Commit 1 yeni warning YOK)
- `cargo check -p osp-desktop --lib` — HEAD parity (2 mevcut #72-originated hata)

---

## Commit 2 — Sözleşme (reviewer v3 APPROVED 9.7/10)

### Authority zinciri

```
Axis::measure()
    ↓
CoordinateSystem::measured_position_of() → Result<MeasuredRawPosition, CoordinateMeasurementError>
    ↓ MeasuredRawPosition (5 AxisMeasurement, source preserved)
    ├── MeasuredRawPosition::to_raw()
    │   ↓
    └── CoordinateSystem::try_raw_position_of()  (delegasyon: measured.to_raw())
```

Tek authority yüzeyi — `try_raw_position_of` kendi axis traversal yazmaz.

### Error precedence (P1-1 — KRİTİK)

```text
1. Coordinate schema complete mi?     hayır → MissingCoreAxes { missing }
2. Beş axis'i measure et              hata → AxisMeasurementFailed { axis_id, source }
3. Sonuçları named fields'e yerleştir
```

**Missing-axis preflight ölçümden ÖNCE yapılır.** Partial coordinate system'de bir axis measure hatası dönerse bile structural olarak `MissingCoreAxes` önceliklidir. Custom `Axis::measure()` iç mutability kullanabilir; eksik schema belli olduğu halde axis'leri çağırmak fail-closed sınırını gereksiz yere measurement davranışına sokar.

### Düzenleme yüzeyi (yalnızca coords.rs — pure addition)

#### 1. `AxisMeasurementError::MixedDirectAxisSource` yeni varyant

```rust
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AxisMeasurementError {
    #[error("non-finite axis value (NaN/Inf rejected)")]
    NonFiniteValue,
    #[error("axis value out of range [0,1]: {0}")]
    OutOfRange(f64),
    /// **INV-T9 #70 Commit 2:** Mixed doğrudan axis output olarak red — yalnız aggregation.
    #[error("Mixed source cannot be returned by a single axis (only by aggregation)")]
    MixedDirectAxisSource,
}
```

#### 2. `AxisMeasurement::validate_direct_axis_output()` — `pub(crate)` (P2-3)

```rust
impl AxisMeasurement {
    // try_new + validate existing

    /// **INV-T9 #70 Commit 2:** Defensive re-validation — `measured_position_of` her axis
    /// output'ını çağırır. Mixed yalnız aggregation çıktısıdır; custom axis doğrudan
    /// üretemez. `pub(crate)` — Commit 3 internal kullanım; public migration yüzeyi değil.
    pub(crate) fn validate_direct_axis_output(&self) -> Result<(), AxisMeasurementError> {
        self.validate()?;
        if self.source == MetricSource::Mixed {
            return Err(AxisMeasurementError::MixedDirectAxisSource);
        }
        Ok(())
    }
}
```

#### 3. `CoordinateMeasurementError` (P1-1 + P1-2 düzeltmeli)

```rust
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CoordinateMeasurementError {
    /// `aggregate_source` boş iterator aldı.
    #[error("empty measurement source set")]
    EmptySourceSet,

    /// **P1-1:** Eksik core raw axis — sentetik (0.0, Placeholder) DEĞİL. measured_position_of
    /// tam 5-core-axis authority yüzeyidir; partial preset'ler legacy raw_position_of kullanır.
    /// Preflight ölçümden ÖNCE yapılır (error precedence).
    #[error("missing core raw axes: {missing:?}")]
    MissingCoreAxes { missing: Vec<&'static str> },

    /// **P1-2:** Per-axis measurement hatası — axis kimliği korunur (blanket #[from] YOK).
    #[error("axis `{axis_id}` measurement failed: {source}")]
    AxisMeasurementFailed {
        axis_id: &'static str,
        #[source]
        source: AxisMeasurementError,
    },
}
```

#### 4. `aggregate_source()` — `pub(crate)` (P2-3)

```rust
/// **INV-T9 #70 Commit 2** — Heterojen aggregation semantics (P2-2 doc):
///
/// `Mixed` doğrudan bir axis ölçümünün kaynağı olamaz. Aggregate input'ları daha önce
/// aggregate edilmiş ve dolayısıyla `Mixed` olabilir. Herhangi bir `Mixed` input içeren
/// üst aggregation da `Mixed` üretir; yalnız tamamen aynı non-Mixed source kümesi o
/// source'u korur.
///
/// Table:
/// ```text
/// [Scip]                → Scip
/// [Scip, Scip]          → Scip
/// [Scip, TreeSitter]    → Mixed
/// [Mixed]               → Mixed
/// [Mixed, Mixed]        → Mixed
/// [Mixed, Scip]         → Mixed
/// []                    → EmptySourceSet
/// ```
///
/// `pub(crate)` — Commit 3 `measure_task_delta` subject scope centroid aggregation için.
pub(crate) fn aggregate_source(
    sources: impl IntoIterator<Item = MetricSource>,
) -> Result<MetricSource, CoordinateMeasurementError> {
    let mut sources = sources.into_iter();
    let first = sources.next().ok_or(CoordinateMeasurementError::EmptySourceSet)?;
    if sources.all(|s| s == first) {
        Ok(first)
    } else {
        Ok(MetricSource::Mixed)
    }
}
```

#### 5. `CoordinateSystem::measured_position_of()` + `try_raw_position_of()` (P1-1 preflight)

```rust
impl CoordinateSystem {
    // raw_position_of — UNCHANGED (Commit 4'e kadar legacy infallible partial projection)
    // position_of — doc comment güncellenir, kod unchanged

    /// **INV-T9 #70 Commit 2:** Provenance-preserving tam 5-core-axis authority ölçümü.
    ///
    /// Error precedence:
    /// 1. Missing core axis preflight (structural) → `MissingCoreAxes`
    /// 2. Per-axis measure() / validate_direct_axis_output → `AxisMeasurementFailed`
    ///
    /// Authority/evidence yollarının tek ölçüm yüzeyi. Legacy partial preset'ler
    /// (default_raw_three) `raw_position_of` kullanmaya devam eder.
    pub fn measured_position_of(
        &self,
        node: &Node,
        space: &Space,
    ) -> Result<MeasuredRawPosition, CoordinateMeasurementError> {
        // P1-1: Missing-axis preflight ölçümden ÖNCE.
        let missing = CORE_RAW_AXIS_IDS
            .iter()
            .copied()
            .filter(|required| !self.axes.iter().any(|axis| axis.name() == *required))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(CoordinateMeasurementError::MissingCoreAxes { missing });
        }

        let mut coupling = None;
        let mut cohesion = None;
        let mut instability = None;
        let mut entropy = None;
        let mut witness_depth = None;
        for axis in &self.axes {
            let name = axis.name();
            if !is_core_raw_axis_id(name) {
                continue; // custom axis — MeasuredRawPosition'a dahil değil
            }
            // P1-2: axis kimliği error boundary'de korunur (blanket #[from] YOK).
            let measured = axis.measure(node, space).map_err(|source| {
                CoordinateMeasurementError::AxisMeasurementFailed { axis_id: name, source }
            })?;
            measured.validate_direct_axis_output().map_err(|source| {
                CoordinateMeasurementError::AxisMeasurementFailed { axis_id: name, source }
            })?;
            match name {
                "coupling" => coupling = Some(measured),
                "cohesion" => cohesion = Some(measured),
                "instability" => instability = Some(measured),
                "entropy" => entropy = Some(measured),
                "witness_depth" => witness_depth = Some(measured),
                _ => unreachable!("is_core_raw_axis_id guarantees known name"),
            }
        }
        // Safety: preflight all 5 core axes registered guaranteed.
        Ok(MeasuredRawPosition {
            coupling: coupling.unwrap(),
            cohesion: cohesion.unwrap(),
            instability: instability.unwrap(),
            entropy: entropy.unwrap(),
            witness_depth: witness_depth.unwrap(),
        })
    }

    /// **INV-T9 #70 Commit 2:** Fallible value-only projection — `measured_position_of`
    /// üzerinden `to_raw()`. Tek authority zinciri (kendi axis traversal yazmaz).
    /// Commit 4'te raw_position_of deprecated edilince caller'ların migrate edeceği yüzey.
    pub fn try_raw_position_of(
        &self,
        node: &Node,
        space: &Space,
    ) -> Result<RawPosition, CoordinateMeasurementError> {
        self.measured_position_of(node, space).map(|m| m.to_raw())
    }
}
```

#### 6. `position_of` doc comment (P2-1 düzeltmesi)

```rust
/// Legacy value-only projection for all registered axes, in registration order.
///
/// Calls `Axis::compute()` and therefore does not preserve provenance or propagate
/// measurement errors. It does not require or synthesize the five core raw axes.
///
/// Authority/evidence paths must use `measured_position_of()` or `try_raw_position_of()`.
/// Kept for generic/custom-axis compatibility; planned for deprecation in Commit 4.
pub fn position_of(&self, node: &Node, space: &Space) -> Vec<f64> {
    self.axes.iter().map(|a| a.compute(node, space)).collect()
}
```

#### 7. Commit 1 doc-comment placeholder'ların resolve edilmesi

`coords.rs:81-84, 215, 224, 248, 289, 1249` placeholder'ları resolve edilir.

---

## Test'ler (coords.rs test modülü) — P1-2 full-core fixture helper

### Yeni fixture'lar (5)

**SourcedConstantAxis** — per-axis source preservation:
```rust
struct SourcedConstantAxis { name: &'static str, value: f64, source: MetricSource }
impl Axis for SourcedConstantAxis {
    fn name(&self) -> &'static str { self.name }
    fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError> {
        let mut params = AxisParameterEncoder::new();
        params.push_u8(0);
        params.push_f64(self.value)?;
        params.push_bytes(self.source.descriptor_id())?;
        AxisDescriptor::try_new(self.name, 2, params)
    }
    fn measure(&self, _, _) -> Result<AxisMeasurement, AxisMeasurementError> {
        AxisMeasurement::try_new(self.value, self.source)
    }
    fn compute(&self, _, _) -> f64 { self.value }
}
```

**DivergentAxis** — `try_raw_position_of` measure() kullanır kanıtı (compute=0.1, measure=0.9).

**MixedReturningAxis** — direct Mixed rejection (MismatchedAxis template).

**ForgedValueAxis** — P2-1 defensive revalidation: parametric `Ok(AxisMeasurement { value: NAN/1.5, .. })`.

**FailingAxis** — axis'in kendi `Err(NonFiniteValue)` döndürmesi (P2-1 axis-own-error kategorisi).

### Full-core fixture helper (P1-2 — KRİTİK)

```rust
/// P1-2: AxisMeasurementFailed test'leri tam 5-core-axis system kurmalı.
/// Yalnız coupling değişkendir; diğer dört axis sabit sourced constant.
fn full_core_system_with_coupling<A: Axis + 'static>(coupling: A) -> CoordinateSystem {
    CoordinateSystem::empty()
        .try_with_axis(coupling).unwrap()
        .try_with_axis(SourcedConstantAxis { name: "cohesion", value: 0.2, source: MetricSource::Scip }).unwrap()
        .try_with_axis(SourcedConstantAxis { name: "instability", value: 0.3, source: MetricSource::TreeSitter }).unwrap()
        .try_with_axis(SourcedConstantAxis { name: "entropy", value: 0.4, source: MetricSource::Heuristic }).unwrap()
        .try_with_axis(SourcedConstantAxis { name: "witness_depth", value: 0.5, source: MetricSource::Heuristic }).unwrap()
}
```

Kullanım:
```rust
full_core_system_with_coupling(MixedReturningAxis)
full_core_system_with_coupling(ForgedNaNAxis)
full_core_system_with_coupling(FailingAxis)
```

`MissingCoreAxes` testleri ise kasıtlı olarak partial kalır (yalnız coupling register).

### Yeni test'ler (~22)

1. `measured_position_of_preserves_each_axis_source` — 5 SourcedConstantAxis farklı source
2. `measured_position_of_maps_by_axis_name_not_order` — registration sırası karışık
3. `measured_position_of_ignores_custom_axes` — custom axis dahil değil
4. `measured_position_of_rejects_axis_returning_mixed_directly` — `full_core_system_with_coupling(MixedReturningAxis)` → `AxisMeasurementFailed { coupling, MixedDirectAxisSource }`
5. `measured_position_of_rejects_forged_nan_axis_output` — P2-1 forged NaN
6. `measured_position_of_rejects_forged_out_of_range_axis_output` — P2-1 forged 1.5
7. `measured_position_of_propagates_axis_own_measurement_error` — P2-1 axis kendi Err(...)
8. `measured_position_of_rejects_missing_core_axes` — P1-1 only coupling registered → `MissingCoreAxes { missing: [cohesion, instability, entropy, witness_depth] }`
9. `measured_position_of_missing_axis_error_lists_all_absent` — P1-1 multiple missing listelenir
10. `measured_position_of_missing_axis_preflight_precedes_measurement_error` — P1-1 error precedence (partial + failing axis → MissingCoreAxes, AxisMeasurementFailed değil)
11. `try_raw_position_of_returns_measure_value_not_compute` — DivergentAxis: 0.9 (measure), 0.1 (compute) değil
12. `try_raw_position_of_propagates_measurement_error` — error propagation
13. `try_raw_position_of_rejects_missing_core_axes` — P1-1 parity
14. `try_raw_position_of_equals_measured_position_of_to_raw` — `try_raw_position_of(n,s) == measured_position_of(n,s)?.to_raw()`
15. `authoritative_value_projection_matches_legacy_raw_position_for_full_preset` — P2-2 full preset parity (`measured_position_of().to_raw() == raw_position_of()`)
16. `aggregate_source_homogeneous_returns_that_source`
17. `aggregate_source_heterogeneous_returns_mixed`
18. `aggregate_source_single_element_returns_that_source`
19. `aggregate_source_empty_returns_empty_source_set_error`
20. `aggregate_source_mixed_inputs_propagate_to_mixed` — P2-2 [Mixed, Mixed] / [Mixed, Scip]
21. `validate_direct_axis_output_rejects_mixed` — pub(crate) unit test
22. `validate_direct_axis_output_accepts_valid_sources` — TreeSitter/Scip/Placeholder/Heuristic

---

## CI simülasyonu (Commit 2 öncesi) — P2-3 truth-surface

```bash
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo build --workspace --examples --exclude osp-desktop
RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop
cargo clippy -p osp-core --lib
```

**osp-desktop targeted check (P2-3):** expected to fail with the same two pre-existing error fingerprints as parent `0d4eb51` (`compute_raw_from_delta` arity + `Claim` missing fields — #72-originated). Commit 2 desktop'a dokunmaz (yalnız coords.rs); **no additional error admitted**.

Daha güçlü minimum çıktı (P2-3):
```bash
cargo check -p osp-desktop --lib --message-format=short 2> commit2-desktop.err || true
grep '^crates/osp-desktop/.*error' commit2-desktop.err
```
Parent `0d4eb51` ve Commit 2 için normalize edilerek file path + error code + message category üçlüsü karşılaştırılır.

**Test count:** 923 → ~945 (+22 yeni). Final workspace output'tan alınır.

---

## Risk & governance

- **Runtime behavior is add-only for existing methods.** `AxisMeasurementError` yeni public enum varyantı alır — pre-release migration'da downstream exhaustive match'ler için intentionally source-breaking.
- **Single authority surface** — `try_raw_position_of` `measured_position_of().to_raw()` delegasyonu; validation tek yerde.
- **Mixed semantic integrity** — üç katmanlı kapsama:
  1. Commit 1 constructor guard (`validate_direct_source`)
  2. Commit 1 v7 closure predicate + set-level guard
  3. Commit 2 `validate_direct_axis_output` defensive revalidation
- **Ontolojik ayrım (P1-1)** — `raw_position_of` partial-zero legacy; `measured_position_of` tam 5-core-axis authority (eksik = typed error, preflight öncelikli). Sentetik ölçüm YOK.
- **Axis identity preservation (P1-2)** — measurement hatası `AxisMeasurementFailed { axis_id, source }` ile hangi axis'in failed olduğu korunur.
- **Internal helper encapsulation (P2-3)** — `aggregate_source` + `validate_direct_axis_output` `pub(crate)`; public migration yüzeyi yalnız `measured_position_of` + `try_raw_position_of`.
- **Commit 4 migration hazır** — `try_raw_position_of` + `measured_position_of` Commit 4'te `raw_position_of` deprecated edilince resmi migrate yüzeyi.

---

## Commit zinciri (kalan)

### Commit 2 — `feat(coords): provenance-aware position measurement` (BU COMMIT)
- `CoordinateMeasurementError` + `aggregate_source` (pub(crate)) + `validate_direct_axis_output` (pub(crate))
- `measured_position_of()` + `try_raw_position_of()` (add-only, P1-1 preflight)
- `raw_position_of` / `position_of` unchanged (Commit 4'e kadar)
- `AxisMeasurementError::MixedDirectAxisSource` yeni varyant
- Full-core fixture helper + 22 test

### Commit 3 — `feat(engine): subject-bound EngineMeasurement tokens` (add-only, non-breaking)
- Yeni `measurement.rs` neutral modül: `MeasurementMode`, `CanonicalMeasurementScope`, `MeasurementRequest`, `MeasurementRequestDigest`, `MeasurementBaseline`, `BaselineUnavailableReason`
- `EngineMeasurement` private-field token (before+after+context+revision+request, loss YOK)
- `measure_task_delta(task, ...)` + `measure_current_scope(scope)` planning
- `derive_task_subject_scope(task)` + `derive_impact_scope(structural_delta)` + hint validation
- Subject/impact aggregate invariant (subject_scope üyeleri only, partial→Unavailable)
- `try_compute_raw_from_delta` fallible; `compute_raw_from_delta` deprecated unchanged
- `MeasurementError` taxonomy
- `MeasurementRequestDigest` golden (v1 yeni)

### Commit 4 — `refactor(inv-t4): require EngineMeasurement across authority and evidence paths` (ATOMIK)
- `TaskCommitInput { claim, omega, task_resolver, measurement }` (subject_scope YOK)
- `commit_task_claim` migration + `claim.computed_raw` ignore + Mixed validation
- `AuthorizationBasis v2` (before+after single canonical + request digest + baseline/loss consistency)
- `PredicateGateInput` → token baseline/after
- `TrajectoryEvidenceBaseline` enum
- Tüm caller migration atomik: Navigator, MCP, CLI, g2c, test construction site'ları
- `provenanced_from_raw` production/evidence path'ten kaldır
- `raw_position_of` + `position_of` + `Axis::compute()` `#[deprecated]`
- Domain sep `osp.authorization-basis.v2\0`
- `TaskValidationError::InvalidRequiredMetricSource` (typed commit-time guard)
- AuthorizationBasis v2 golden + v1 strict-reject fixture
- Post-commit grep: `provenanced_from_raw(.*Scip` authority/evidence yolunda sonuç vermemeli

### Commit 5 — `test(inv-t4): adversarial measurement-binding regressions`
- 19 regression test

### Commit 6 — `docs(inv-t4): conformance + truth-surface`
- Conformance doc, #70 acceptance checklist, PR body sync

---

## Açılacak issue'lar (Commit 1 sonrası takip)

Yeni oturumda açılacak issue'lar (Commit 1 doc'lanan takip maddeleri):

1. **PredicateAxis derived/custom coupling fallback** — `_ => coupling` legacy behavior (Commit 1 P2-2). Uzun vadeli API: `raw_axis(PredicateAxis) -> Option<&AxisMeasurement>`.
2. **osp-desktop #72-originated errors** — `compute_raw_from_delta` arity + `Claim` missing fields. HEAD parity korundu ama desktop non-compiling.
3. **`Axis::compute()` `#[deprecated]` cleanup** — Commit 4'te eklenecek.

---

## Review pattern

INV-T9 #70 Commit 1 serisi incremental scoped review ile ilerledi (v5 plan + v6/v7 closure). Commit 2 için aynı yaklaşım:
1. Commit 2 implementation
2. Scoped review → REQUEST CHANGES/APPROVED
3. Closure commit'leri
4. Commit 3'e geçiş

---

*Bu belge INV-T9 #70 Commit 2 implementation'ına geçiş için handoff'tır. Commit 1 reviewer 9.9/10 APPROVED, Commit 2 planı reviewer 9.7/10 APPROVE WITH REQUIRED CLARIFICATIONS.*
