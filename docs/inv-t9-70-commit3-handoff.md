# INV-T9 #70 Commit 3 — Subject-Bound EngineMeasurement Tokens Handoff

**Tarih:** 2026-07-20
**Branch:** `fix/inv-t9-witness-suspension`
**PR:** #69 (https://github.com/ontologicalspace/osp/pull/69)
**Current head:** `eb9903b` (Commit 2 + review v1/v2 closure landed — APPROVED 10/10)
**Issue:** #70 (https://github.com/ontologicalspace/osp/issues/70)
**Plan approval:** Reviewer v4 APPROVED 9.7/10 (implementation plan doc)
**Commit 2 closure:** Reviewer APPROVED 10/10 — scoped tamamlandı

---

## Yeni oturumda ilk komut

```bash
cd P:/Work/SoftwarePhysics
git checkout fix/inv-t9-witness-suspension
git pull  # eb9903b head olmalı
git log --oneline -5
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 951 test geçmeli
```

Sonra bu belgeyi oku, Commit 3 implementation'a başla.

---

## Commit 2 durumu (tamamlandı — reviewer 10/10 APPROVED)

**3 commit ile landed:**
- `080009e` — Commit 2: `feat(coords): provenance-aware position measurement`
- `059ed04` — v1 closure: `bind defensive + descriptor contract`
- `eb9903b` — v2 closure: `error precedence + real drift tests`

### Commit 2 kazanımları (Commit 3'ün inşa edeceği zemin)
- `AxisMeasurementError::MixedDirectAxisSource` (axis output defensive re-validation)
- `AxisMeasurement::validate_direct_axis_output()` pub(crate)
- `CoordinateMeasurementError`: `EmptySourceSet`, `MissingCoreAxes { missing }`, `AxisMeasurementFailed { axis_id, source }`, `DuplicateCoreAxis`, `AxisIdentityMismatch`, `AxisDescriptorFailed`
- `aggregate_source()` pub(crate) free fn (heterojen aggregation semantics)
- `CoordinateSystem::bind_core_axes()` pub(crate) — iki-fazlı binding (structural completeness önce)
- `validate_bound_axis_identity()` pub(crate) + `measure_bound_axis()` pub(crate) helpers
- `CoreAxisRefs` pub(crate) struct (tek-pass bound 5 core &dyn Axis, manuel Debug impl)
- `CoordinateSystem::measured_position_of()` + `try_raw_position_of()` (single authority surface)
- 951 osp-core lib tests (923 → +28: Commit 2 +22, v1 closure +4, v2 closure +2 net)

### CI doğrulaması (Commit 2 sonrası)
- **951 osp-core lib tests** (workspace green excl. osp-desktop)
- `cargo fmt` clean
- `cargo clippy -p osp-core --lib` — 12 warning (HEAD parity, coords.rs kaynaklı warning YOK)
- `cargo check -p osp-desktop --lib` — HEAD parity (2 #72-originated hata: `compute_raw_from_delta` arity + `Claim` missing fields)

---

## Commit 3 — Sözleşme (reviewer v4 plan APPROVED 9.7/10)

### Authority zinciri (Commit 3 sonrası)

```
TaskCommitInput { claim, omega, task_resolver, measurement }   ← Commit 4
    ↓ measurement: EngineMeasurement
EngineMeasurement (private-field token)
    ↓ before: MeasuredRawPosition, after: MeasuredRawPosition,
    ↓ context: MeasurementInputContext, revision: SpaceViewRevision,
    ↓ request: MeasurementRequest
measure_task_delta(task, ...) → EngineMeasurement   ← Commit 3
    ↓ uses CoordinateSystem::measured_position_of() (Commit 2)
    ↓ derive_task_subject_scope + derive_impact_scope
Subject/impact aggregate invariant
    ↓ subject_scope üyeleri only, partial → Unavailable
```

Commit 3 add-only, non-breaking — hiçbir existing caller'a dokunmaz. `TaskCommitInput` `measured` field'ı Commit 4'te `measurement: EngineMeasurement`'a dönüşecek.

### Düzenleme yüzeyi

**Yeni dosya:** `crates/osp-core/src/measurement.rs` — neutral modül (coords/authorization/trajectory arasında).
**lib.rs'e ekleme:** `pub mod measurement;` (alfabetik: `persistence` sonrası, `navigator` öncesi).

#### 1. `MeasurementMode` enum

```rust
/// INV-T9 #70 Commit 3 — Measurement authority mode (subject scope vs current scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MeasurementMode {
    /// Task delta subject scope (derive_task_subject_scope centroid).
    SubjectScope,
    /// Current scope (measure_current_scope — mevcut state measurement).
    CurrentScope,
}
```

#### 2. `CanonicalMeasurementScope` struct

```rust
/// INV-T9 #70 Commit 3 — Subject/impact scope canonical representation.
/// subject_scope üyeleri NodeId listesi; impact_scope structural delta field'ları.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanonicalMeasurementScope {
    pub mode: MeasurementMode,
    pub subject_member_ids: Vec<NodeId>,      // subject_scope üyeleri (sorted, unique)
    pub impact_node_ids: Vec<NodeId>,         // impact scope (structural delta)
    pub impact_edge_ids: Vec<EdgeRef>,        // impact scope removed edges
}
```

#### 3. `MeasurementRequest` struct

```rust
/// INV-T9 #70 Commit 3 — Measurement authority request (caller'ın ne ölçtüğünü beyan eder).
/// EngineMeasurement token'ın "request" field'ı — kanıt zinciri için gerekli.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeasurementRequest {
    pub mode: MeasurementMode,
    pub subject_scope: CanonicalMeasurementScope,
    pub impact_scope: CanonicalMeasurementScope,
    pub space_view_revision: SpaceViewRevision,
}
```

#### 4. `MeasurementRequestDigest` (BLAKE3)

```rust
/// INV-T9 #70 Commit 3 — Measurement request canonical digest.
/// AuthorizationBasis v2 digest zincirinde yer alacak (Commit 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasurementRequestDigest([u8; 32]);

impl MeasurementRequestDigest {
    pub const DOMAIN_SEPARATOR: &'static [u8] = b"osp.measurement-request.v1\0";

    pub fn compute(request: &MeasurementRequest) -> Result<Self, MeasurementDigestError> {
        // BLAKE3: domain separator + canonical encoding (sorted node IDs, deterministic)
    }

    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
}
```

**Golden:** `MEASUREMENT_REQUEST_V1_GOLDEN_HEX` — real axis fixture (Commit 1 pattern).

#### 5. `MeasurementBaseline` + `BaselineUnavailableReason`

```rust
/// INV-T9 #70 Commit 3 — Before-state measurement (subject scope'da zaten ölçülmüş mü?).
/// Partial measurement → Unavailable (sentetik (0.0, Placeholder) DEĞİL).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MeasurementBaseline {
    /// Before-state measured (subject scope üyeleri tamam).
    Available(MeasuredRawPosition),
    /// Before-state unavailable — partial subject scope veya revision mismatch.
    Unavailable { reason: BaselineUnavailableReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize, serde::Deserialize)]
pub enum BaselineUnavailableReason {
    #[error("partial subject scope — not all members previously measured")]
    PartialSubjectScope,
    #[error("space view revision mismatch — baseline from different revision")]
    RevisionMismatch,
    #[error("no prior measurement at subject scope")]
    NoPriorMeasurement,
}
```

#### 6. `EngineMeasurement` private-field token

```rust
/// INV-T9 #70 Commit 3 — Subject-bound measurement token. Private-field: yalnız
/// constructor + accessor'lar üzerinden erişilebilir. Loss YOK — before+after+context+
/// revision+request hepsi preserved.
///
/// Authority/evidence yollarının tek measurement yüzeyi (Commit 4 migration).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngineMeasurement {
    before: MeasurementBaseline,
    after: MeasuredRawPosition,
    context: crate::authorization::MeasurementInputContext,
    revision: SpaceViewRevision,
    request: MeasurementRequest,
}

impl EngineMeasurement {
    /// Private-field constructor — yalnız engine内部的 (Commit 4 migration öncesi pub(crate)).
    pub(crate) fn new(
        before: MeasurementBaseline,
        after: MeasuredRawPosition,
        context: crate::authorization::MeasurementInputContext,
        revision: SpaceViewRevision,
        request: MeasurementRequest,
    ) -> Self { Self { before, after, context, revision, request } }

    pub fn before(&self) -> &MeasurementBaseline { &self.before }
    pub fn after(&self) -> &MeasuredRawPosition { &self.after }
    pub fn context(&self) -> &crate::authorization::MeasurementInputContext { &self.context }
    pub fn revision(&self) -> SpaceViewRevision { self.revision }
    pub fn request(&self) -> &MeasurementRequest { &self.request }

    /// Request digest — authorization zinciri için (Commit 4).
    pub fn request_digest(&self) -> Result<MeasurementRequestDigest, MeasurementDigestError> {
        MeasurementRequestDigest::compute(&self.request)
    }
}
```

#### 7. `MeasurementError` taxonomy

```rust
/// INV-T9 #70 Commit 3 — Measurement pipeline hatası (subject/impact scope + baseline).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MeasurementError {
    #[error(transparent)]
    CoordinateMeasurement(#[from] crate::coords::CoordinateMeasurementError),

    #[error(transparent)]
    MeasurementInput(#[from] crate::authorization::CanonicalizationError),

    /// Subject scope boş — task hiçbir node'a binding değil.
    #[error("empty subject scope — task not bound to any node")]
    EmptySubjectScope,

    /// Impact scope subject scope üyeleri dışında node'lar içeriyor.
    #[error("impact scope contains nodes outside subject scope")]
    ImpactScopeNotSubsetOfSubject,

    /// Subject scope üyeleri partial — bazı node'lar space'de yok.
    #[error("partial subject scope — missing members: {missing:?}")]
    PartialSubjectScope { missing: Vec<NodeId> },

    /// Hint validation: caller hint subject_scope ile derived subject_scope mismatch.
    #[error("subject scope hint mismatch: hint={hint_members:?}, derived={derived_members:?}")]
    SubjectScopeHintMismatch {
        hint_members: Vec<NodeId>,
        derived_members: Vec<NodeId>,
    },

    /// Digest computation hatası.
    #[error(transparent)]
    Digest(#[from] MeasurementDigestError),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MeasurementDigestError {
    #[error("BLAKE3 digest computation failed")]
    Blakes3Failure,
}
```

#### 8. `SpaceEngine` measurement metotları (add-only)

```rust
impl SpaceEngine {
    /// **INV-T9 #70 Commit 3:** Task delta subject-bound measurement token üretir.
    /// before+after+context+revision+request — loss YOK. Authority/evidence yolları
    /// Commit 4'te bu token'ı TaskCommitInput'a geçirecek.
    pub fn measure_task_delta(
        &self,
        task: &dyn crate::trajectory::TaskResolver,
        claim: &crate::witness::Claim,
        structural_delta: &CanonicalStructuralDelta,
        subject_scope_hint: Option<&[NodeId]>,
    ) -> Result<EngineMeasurement, MeasurementError> {
        // 1. derive_task_subject_scope(task) → subject_scope üyeleri
        // 2. derive_impact_scope(structural_delta) → impact_node_ids + impact_edge_ids
        // 3. Subject/impact invariant: impact ⊆ subject (partial → error)
        // 4. Hint validation: hint sağlandıysa derived ile karşılaştır
        // 5. before: subject_scope üyelerinin prior measurement'ı (revision aware)
        //    - Eğer tüm üyeler prior revision'da ölçülmüşse Available
        //    - Aksi halde Unavailable { reason }
        // 6. after: try_compute_raw_from_delta fallible (affected centroid)
        //    - CoordinateSystem::measured_position_of() kullanır (Commit 2)
        // 7. context: MeasurementInputContext (CoordinateSystem'den)
        // 8. request: MeasurementRequest { mode: SubjectScope, subject_scope, impact_scope, revision }
        // 9. EngineMeasurement::new(...)
    }

    /// **INV-T9 #70 Commit 3:** Current scope measurement (mevcut state).
    /// measure_task_delta'nın current-scope karşılığı — diagnostic/reporting.
    pub fn measure_current_scope(
        &self,
        scope: &CanonicalMeasurementScope,
    ) -> Result<EngineMeasurement, MeasurementError> {
        // Current mode: before = Unavailable (no prior), after = current measured
    }

    /// **INV-T9 #70 Commit 3:** Task → subject scope üyeleri türetme.
    /// Task binding'inden etkilenen node'lar. Hint sağlandıysa validation.
    pub(crate) fn derive_task_subject_scope(
        &self,
        task: &dyn crate::trajectory::TaskResolver,
    ) -> Result<Vec<NodeId>, MeasurementError> {
        // task.bound_node_ids() veya task intent → subject members
    }

    /// **INV-T9 #70 Commit 3:** Structural delta → impact scope türetme.
    /// delta_nodes + delta_edges + delta_removed → impact scope.
    pub(crate) fn derive_impact_scope(
        &self,
        structural_delta: &CanonicalStructuralDelta,
    ) -> Result<(Vec<NodeId>, Vec<EdgeRef>), MeasurementError> {
        // structural_delta field'larından impact scope üret
    }

    /// **INV-T9 #70 Commit 3:** Fallible compute_raw_from_delta — Commit 2
    /// measured_position_of() kullanır. Legacy compute_raw_from_delta unchanged
    /// (Commit 4'te deprecated).
    pub fn try_compute_raw_from_delta(
        &self,
        delta_nodes: &[crate::space::Node],
        delta_edges: &[crate::space::Edge],
        delta_removed: &[crate::agent::EdgeRef],
        affected_nodes: &[crate::space::NodeId],
    ) -> Result<crate::coords::RawPosition, MeasurementError> {
        // CoordinateSystem::try_raw_position_of() kullanır (Commit 2 authority surface)
        // Mass-weighted centroid — mevcut compute_raw_from_delta logic'i
    }
}
```

#### 9. `TaskCommitInput` (Commit 4 migration için hazır — Commit 3 unchanged)

`TaskCommitInput` (engine.rs:101-111) Commit 3'te unchanged. Commit 4'te `measured` field `measurement: EngineMeasurement`'a dönüşecek. Commit 3 `EngineMeasurement` token'ı sadece tanımlar + `SpaceEngine` metotlarını ekler — caller migration yok.

---

## Test'ler (measurement.rs test modülü + engine.rs test extension)

### Yeni test'ler (~25-30)

**measurement.rs neutral layer:**
1. `measurement_mode_serialization_roundtrip` — SubjectScope/CurrentScope serde
2. `canonical_measurement_scope_sorted_unique_member_ids` — duplicate reject + sort
3. `measurement_request_serialization_roundtrip` — all fields
4. `measurement_request_digest_deterministic` — same request → same digest
5. `measurement_request_digest_distinct_for_distinct_requests` — farklı mode/scope/revision → farklı digest
6. `measurement_request_digest_v1_golden` — real axis fixture golden hex
7. `measurement_baseline_available_preserves_measured` — Available variant
8. `measurement_baseline_unavailable_preserves_reason` — 3 reason varyantı
9. `engine_measurement_private_field_no_construction` — struct literal compile-time reject (constructor pub(crate))
10. `engine_measurement_accessors_preserve_all_fields` — before/after/context/revision/request
11. `engine_measurement_request_digest_chain` — token → digest → authorization zinciri

**SpaceEngine measurement metotları (engine.rs test):**
12. `measure_task_delta_subject_scope_full_core_system` — full subject scope + impact ⊆ subject
13. `measure_task_delta_empty_subject_scope_error` — task binding yok → EmptySubjectScope
14. `measure_task_delta_impact_not_subset_of_subject_error` — impact subject dışı → ImpactScopeNotSubsetOfSubject
15. `measure_task_delta_partial_subject_scope_error` — subject üyeleri partial → PartialSubjectScope
16. `measure_task_delta_hint_matches_derived` — hint = derived → OK
17. `measure_task_delta_hint_mismatch_error` — hint ≠ derived → SubjectScopeHintMismatch
18. `measure_task_delta_before_available` — prior measurement var → Available
19. `measure_task_delta_before_unavailable_no_prior` — prior yok → NoPriorMeasurement
20. `measure_task_delta_before_unavailable_revision_mismatch` — farklı revision → RevisionMismatch
21. `measure_task_delta_after_uses_measured_position_of` — after Commit 2 authority surface
22. `measure_current_scope_before_unavailable` — current mode → before = Unavailable
23. `try_compute_raw_from_delta_returns_measured_value` — Commit 2 measured_position_of() değer
24. `try_compute_raw_from_delta_propagates_missing_core_axes` — partial coordinate system
25. `try_compute_raw_from_delta_equals_legacy_for_full_preset` — default_raw_five parity

**Subject/impact aggregate invariant:**
26. `derive_task_subject_scope_task_bound_nodes` — task binding → subject members
27. `derive_impact_scope_structural_delta_fields` — delta_nodes/edges/removed → impact scope
28. `subject_impact_aggregate_invariant_violation_detection` — impact ⊄ subject → error

---

## CI simülasyonu (Commit 3 öncesi)

```bash
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo build --workspace --examples --exclude osp-desktop
RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop
cargo clippy -p osp-core --lib
cargo check -p osp-desktop --lib --message-format=short  # parent 7c0f8c8 parity (2 #72-originated hata, yeni hata YOK)
```

**osp-desktop targeted check:** Commit 3 osp-desktop'a dokunmaz (yalnız yeni measurement.rs + engine.rs add-only metotlar). **No additional error admitted** — parent `eb9903b` ile aynı 2 #72-originated hata.

**Test count:** 951 → ~980 (+25-30 yeni).

---

## Risk & governance

- **Add-only, non-breaking:** `TaskCommitInput` unchanged (Commit 4 migration). Hiçbir existing caller'a dokunmaz.
- **Private-field token:** `EngineMeasurement` field'ları private, constructor pub(crate). Struct literal bypass kapalı. Loss YOK — before+after+context+revision+request hepsi preserved.
- **Subject/impact invariant:** impact ⊆ subject (partial → typed error). Sentetik ölçüm YOK.
- **Baseline honesty:** `MeasurementBaseline::Unavailable` — partial subject scope veya revision mismatch'ta sentetik (0.0, Placeholder) DEĞİL typed reason.
- **Digest zinciri:** `MeasurementRequestDigest` BLAKE3 + v1 domain separator (`osp.measurement-request.v1\0`). Commit 4 AuthorizationBasis v2 digest zincirinde yer alacak.
- **Commit 2 authority surface:** `try_compute_raw_from_delta` `CoordinateSystem::measured_position_of()` kullanır — tek authority zinciri korunur.
- **Hint validation:** caller hint subject_scope → derived karşılaştırma (fail-closed). Subject scope gizli/gönderimsel değil; hint sadece defensive teyit.

---

## Commit zinciri (kalan)

### Commit 3 — `feat(engine): subject-bound EngineMeasurement tokens` (BU COMMIT)
- Yeni `measurement.rs` neutral modül: `MeasurementMode`, `CanonicalMeasurementScope`, `MeasurementRequest`, `MeasurementRequestDigest`, `MeasurementBaseline`, `BaselineUnavailableReason`, `EngineMeasurement`, `MeasurementError`, `MeasurementDigestError`
- `SpaceEngine`: `measure_task_delta`, `measure_current_scope`, `derive_task_subject_scope` pub(crate), `derive_impact_scope` pub(crate), `try_compute_raw_from_delta` (add-only)
- `lib.rs`: `pub mod measurement;`
- `TaskCommitInput` unchanged (Commit 4 migration yüzeyi)
- `compute_raw_from_delta` unchanged (Commit 4'te deprecated)
- ~25-30 test + MeasurementRequestDigest v1 golden

### Commit 4 — `refactor(inv-t4): require EngineMeasurement across authority and evidence paths` (ATOMIK)
- `TaskCommitInput { claim, omega, task_resolver, measurement }` (subject_scope YOK — token'a taşındı)
- `commit_task_claim` migration + `claim.computed_raw` ignore + Mixed validation
- `AuthorizationBasis v2` (before+after single canonical + request digest + baseline/loss consistency)
- `PredicateGateInput` → token baseline/after
- `TrajectoryEvidenceBaseline` enum
- Tüm caller migration atomik: Navigator (832), MCP (867), CLI (313), g2c (491/782/594/904), test construction site'ları (navigator.rs:1043 + 7 test sites, engine.rs:2412)
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

## Commit 3 etki yüzeyi envanteri (Explore agent ile doğrulandı)

### engine.rs mevcut measurement yüzeyi (Commit 3'ün inşa edeceği zemin)
- `compute_raw_from_delta` (engine.rs:1416-1474) — 4 argüman, `RawPosition` döner, `raw_position_of` çağırır (Commit 4'te `try_compute_raw_from_delta`'a migration)
- `reposition_nodes` (engine.rs:1158-1194) — `raw_position_of` 1166 (analyze path, Commit 4 değil)
- `SpaceEngine::new/with_default_rules/from_vision_config` — CoordinateSystem by-value
- `commit_task_claim` (engine.rs:529-655) — INV-T9 task-bound path
- `TaskCommitInput` (engine.rs:101-111) — `measured: ProvenancedRawPosition` (Commit 4'te `measurement: EngineMeasurement`)

### authorization.rs authority yüzeyi
- `AuthorizationBasis` v1 (authorization.rs:1550-1568) — 17 field, v1 byte contract locked
- `AuthorizationBasisDigest` (1601-1633) — `DOMAIN_SEPARATOR = "osp.authorization-basis.v1\0"` (v2 Commit 4)
- `ProvenancedMeasuredResult` (1586-1593) — 5 axis `CanonicalAxisMeasurement`
- `MeasurementInputContext` (authorization.rs:553-653) — `CoordinateSystem → MeasurementInputContext` köprüsü (678-690)
- `MeasurementInputDigest` (693-741) — `DOMAIN_SEPARATOR = "osp.measurement-input.v1\0"` (v1 locked)
- `TaskValidationError` — **HENÜZ TANIMLI DEĞİL** (Commit 4'te eklenecek, `InvalidRequiredMetricSource` varyantı)

### trajectory.rs evidence yüzeyi
- `TrajectoryEvidence` (trajectory.rs:846-862) — `before: RawPosition` plain field (Commit 4'te `TrajectoryEvidenceBaseline` enum)
- `TrajectoryEvidenceBaseline` — **HENÜZ TANIMLI DEĞİL** (Commit 4)
- `MeasuredRawPosition::axis()` inherent impl (trajectory.rs:136-149) — Commit 1 eklemişti
- `provenanced_from_raw` — **navigator.rs:169-192** (trajectory.rs DEĞİL) — authority/evidence yolunda Commit 4'te kaldırılacak

### osp-desktop #72-originated (Commit 3 kapsamı dışı)
- `compute_raw_from_delta` arity (lib.rs:347 — 2 argüman, 4 gerekli)
- `Claim` missing fields (lib.rs:350-357 — `removed_edges`, `task_id` eksik)
- Commit 3 osp-desktop'a dokunmaz; Commit 4 atomik migration'da ele alınacak

### Commit 4 caller envanteri (Commit 3 hazır olması için)
**`commit_task_claim` / `TaskCommitInput` caller'ları:**
- Production: navigator.rs:845 (AgentNavigator::run_task), osp-mcp/server.rs:878
- Test: navigator.rs:1485/1570, engine.rs:2412/2427

**`provenanced_from_raw` caller'ları (Commit 4'te kaldırılacak):**
- Production: navigator.rs:169 (def) + 832, osp-mcp/server.rs:768/867, osp-cli/commands/mod.rs:313, g2c_corpus_matrix.rs:491/782/594/904
- Test: navigator.rs:1043/1162/1483/1549/1671/1726/3004, engine.rs:2379

**`compute_raw_from_delta` caller'ları (Commit 4 migration):**
- Production: navigator.rs:790, osp-mcp/server.rs:842, osp-desktop/lib.rs:347 (2-arg #72 hatası)
- Test: engine.rs:2131/2150/2168/2208/2244/2264, navigator.rs:1457/1752/1753/2103/2115, faz5_e2e.rs:191/239/322

### Modül yapısı (Commit 3 measurement.rs yeri)
- lib.rs:16-33 modül listesi (alfabetik): `agent, anchoring, authorization, axes, bigbang, canonical_tags, coords, engine, navigator, persistence, rule, space, task_bridge, time, trajectory, vision, vision_config, witness`
- **`pub mod measurement;`** — `persistence` sonrası, `navigator` öncesi (alfabetik) veya `coords` sonrası (neutral layer sırası)
- Bağımlılık: `measurement` → `coords` + `authorization` + `space` + `agent` (EdgeRef). `engine`/`navigator` → `measurement`.

---

## Review pattern

INV-T9 #70 Commit 1 (v6/v7 closure) ve Commit 2 (v1/v2 closure) incremental scoped review ile ilerledi. Commit 3 için aynı yaklaşım:
1. Commit 3 implementation
2. Scoped review → REQUEST CHANGES/APPROVED
3. Closure commit'leri
4. Commit 4'e geçiş

---

## Açık issue'lar (Commit 3 sonrası takip)

Commit 1 sonrası açılan issue'lar:
1. **#79 (PredicateAxis fallback)** — `_ => coupling` legacy behavior. Uzun vadeli API: `raw_axis(PredicateAxis) -> Option<&AxisMeasurement>`.
2. **#80 (osp-desktop #72-originated errors)** — `compute_raw_from_delta` arity + `Claim` missing fields. Commit 4 atomik migration'da ele alınacak.

Commit 3 yeni issue açmaz — add-only, non-breaking.

---

## Yeni oturumda söylemen gerekenler (text olarak)

Yeni oturumu açtığında şu mesajı yapıştır:

```
INV-T9 #70 Commit 3 implementation'a başlıyoruz.

Önce handoff belgesini oku:
docs/inv-t9-70-commit3-handoff.md

Bu belge reviewer v4 APPROVED (9.7/10) Commit 3 planı içerir. Commit 2
(080009e + 059ed04 + eb9903b, reviewer 10/10 APPROVED) tamamen bitti,
#70 PR #69'un kalan tek merge-blocker'ı.

Önce durumu doğrula:

cd P:/Work/SoftwarePhysics
git checkout fix/inv-t9-witness-suspension
git pull  # eb9903b head olmalı
git log --oneline -5
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 951 test geçmeli

Sonra Commit 3 planını uygula:
- Yeni crates/osp-core/src/measurement.rs neutral modül
- lib.rs'e pub mod measurement; ekle
- MeasurementMode, CanonicalMeasurementScope, MeasurementRequest,
  MeasurementRequestDigest (BLAKE3 + v1 golden), MeasurementBaseline,
  BaselineUnavailableReason, EngineMeasurement (private-field token),
  MeasurementError, MeasurementDigestError
- SpaceEngine: measure_task_delta, measure_current_scope,
  derive_task_subject_scope pub(crate), derive_impact_scope pub(crate),
  try_compute_raw_from_delta (add-only)
- TaskCommitInput unchanged (Commit 4 migration yüzeyi)
- compute_raw_from_delta unchanged (Commit 4'te deprecated)
- ~25-30 yeni test + MeasurementRequestDigest v1 golden

Add-only, non-breaking — hiçbir existing caller'a dokunmaz.

Review pattern: Commit 1/2 gibi — implementation commit + closure commit'leri
ayrı review turu.

Issue'lar: #79 (PredicateAxis fallback), #80 (osp-desktop errors) Commit 3
kapsamı dışı.
```

---

*Bu belge INV-T9 #70 Commit 3 implementation'ına geçiş için handoff'tır. Commit 2 reviewer 10/10 APPROVED, Commit 3 planı reviewer v4 9.7/10 APPROVED.*
