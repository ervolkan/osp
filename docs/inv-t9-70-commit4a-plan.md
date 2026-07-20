# INV-T9 #70 Commit 4a — BoundMeasurementSession Implementation Plan

**Tarih:** 2026-07-21
**Branch:** `fix/inv-t9-witness-suspension`
**PR:** #69 (https://github.com/ontologicalspace/osp/pull/69)
**Current head:** `389e7db` (Commit 3 + review v5/v6/v7 closure landed — reviewer v7 CONDITIONAL APPROVE 9.8/10)
**Issue:** #70 (https://github.com/ontologicalspace/osp/issues/70)
**Plan approval:** Reviewer v10 APPROVED 9.9/10 (implementation-ready)
**Commit 4 split:** 4a (session integrity) + 4b (atomik authority migration). 4a additive, authority caller'lara dokunmaz.

---

## Commit 4a sözleşmesi (reviewer v6→v10 turu)

**Reviewer v6 carryover:** Commit 3 context-before/context-after fence ABA senaryosunu (A→B→A) yakalayamıyordu. Commit 4'ten ÖNCE `BoundMeasurementSession` ile kapatılmalı.

**Reviewer v10 APPROVED 9.9/10 — kapanan P1/P2'ler (v6→v10 turu):**
- **v6 P1-1:** Measurement session atomikliği — interior mutability threat model
- **v8 P1-1:** Gerçek ABA — her ölçümde pre/post descriptor verify
- **v8 P1-2:** Refs + descriptors aynı bind işleminden atomik
- **v8 P1-3:** `coords.rs` authorization tiplerinden bağımsız (neutral layer)
- **v8 P1-4:** `measured_centroid_in_session` + wrapper (try_compute_raw_from_delta compat)
- **v9 P1-1:** AxisStateEpoch (monoton) — gerçek transient ABA (descriptor equality A→B→A'yı kaçırdı)
- **v9 P1-2:** Delegasyon yönü — `bind_core_axis_refs` (refs only) → `bind_core_axes_with_descriptors` → `bind_core_axes` (compat)
- **v10 P1-1:** `MeasurementSessionPhase` public visibility (private_interfaces warning)
- **v10 P1-2:** `AxisStateEpoch` external construct (pub const fn new/get, From<u64>, ZERO, Default)

**Additive — production authority/evidence caller'lara dokunmaz.** TaskCommitInput, commit_task_claim, PredicateGateInput, navigator, MCP, CLI unchanged. Commit 4b atomik authority migration'unun ön koşulu.

---

## Zemin teyit edildi

- **1017 osp-core lib test** (head `389e7db`).
- **`CoordinateSystem`** (coords.rs:764): `axes: Vec<Box<dyn Axis>>`, field private.
- **`Axis` trait** (coords.rs:710): `name/descriptor/measure/try_compute/compute`. `measurement_epoch` YOK — default impl ile eklenecek (sabit 0, backward-compat).
- **`bind_core_axes`** (coords.rs:926): Faz 1 (name binding) + Faz 2.1 (completeness) + Faz 2.2 (duplicate) + Faz 2.3 (descriptor validate). Refactor: Faz 1+2.1+2.2 `bind_core_axis_refs`'e, Faz 2.3 `bind_core_axes_with_descriptors`'a taşınır.
- **`validate_bound_axis_identity`** (coords.rs:443) → `capture_bound_axis_state` generalize.
- **`AxisDescriptor`** (coords.rs:505) — Eq + Clone, Box'lama çalışır.
- **`MeasurementInputContext::try_new(Vec<AxisDescriptor>)`** (auth:575).
- **engine.rs `try_compute_raw_from_delta`** (1862) → `measured_centroid_of` çağırır — wrapper pattern gerekli (P1-4).
- **engine.rs `measure_task_delta`** (1497) — manuel context-before/context-after fence (1608-1629) KALKACAK.

---

## Dosya düzenlemeleri

### 1. `crates/osp-core/src/coords.rs` — Neutral session + AxisStateEpoch

#### AxisStateEpoch (public, external construct — v10 P1-2)

```rust
/// **Commit 4a P1-1 (reviewer v9):** Monoton session epoch — her behaviorally-relevant
/// interior mutation'da artar. Immutable axis'ler sabit 0. Descriptor equality A→B→A
/// revert'te A'yı görür; epoch monoton arttığı için revert yakalanır.
///
/// **TCB sınırı (reviewer v10 P2-1):** BoundMeasurementSession, immutable axis'leri
/// descriptor equality ile; `measurement_epoch` kontratına uyan versioned mutable
/// axis'leri descriptor + epoch ile doğrular. Trait kontratını ihlal ederek mutation
/// yapan ve epoch'u güncellemeyen axis, TCB ihlalidir — fail-closed tarafından
/// yakalanamaz (untrusted axis implementasyonlarına karşı defense-in-depth DEĞİL).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct AxisStateEpoch(u64);

impl AxisStateEpoch {
    pub const ZERO: Self = Self(0);
    pub const fn new(value: u64) -> Self { Self(value) }
    pub const fn get(self) -> u64 { self.0 }
}

impl From<u64> for AxisStateEpoch {
    fn from(value: u64) -> Self { Self::new(value) }
}
```

#### Axis trait + measurement_epoch default (v9 P1-1)

```rust
pub trait Axis: Send + Sync {
    fn name(&self) -> &'static str;
    fn descriptor(&self) -> Result<AxisDescriptor, AxisDescriptorError>;
    fn measure(&self, node: &Node, space: &Space) -> Result<AxisMeasurement, AxisMeasurementError>;
    fn try_compute(&self, node: &Node, space: &Space) -> Result<f64, AxisMeasurementError> {
        Ok(self.measure(node, space)?.value)
    }
    fn compute(&self, node: &Node, space: &Space) -> f64;

    /// **Commit 4a P1-1:** Session epoch — her interior mutation'da artar.
    /// Default impl sabit ZERO (immutable axis'ler backward-compat).
    /// Interior-mutable axis'ler override eder. Session captured epoch ile
    /// pre/post/final epoch karşılaştırır; descriptor eşit olsa bile epoch
    /// farkı ABA revert'u yakalar.
    fn measurement_epoch(&self) -> AxisStateEpoch {
        AxisStateEpoch::ZERO
    }
}
```

#### MeasurementSessionPhase (public, non_exhaustive — v10 P1-1)

```rust
/// **Commit 4a P2-2 (reviewer v8):** Session drift check fazı — typed enum.
/// **Commit 4a P1-1 (reviewer v10):** public — CoordinateMeasurementError varyantında
/// kullanıldığı için private_interfaces warning önlenir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MeasurementSessionPhase {
    PreMeasure,
    PostMeasure,
    Capture,
    SessionFinal,
}
```

#### Bound refs / states / session

```rust
/// 5 core axis referansları — name binding + completeness + duplicate check.
/// Descriptor ÜRETMEZ (v9 P1-2 — descriptor yalnız bind_core_axes_with_descriptors'da).
pub(crate) struct CoreAxisRefs<'a> {
    coupling: &'a dyn Axis,
    cohesion: &'a dyn Axis,
    instability: &'a dyn Axis,
    entropy: &'a dyn Axis,
    witness_depth: &'a dyn Axis,
}

/// **Commit 4a P1-2 (v9):** Captured descriptor + epoch snapshot.
#[derive(Clone)]
pub(crate) struct BoundAxisState {
    descriptor: AxisDescriptor,
    epoch: AxisStateEpoch,
}

/// **Commit 4a P1-2 (v9):** Bound refs + captured state — atomik.
pub(crate) struct BoundCoreAxes<'a> {
    refs: CoreAxisRefs<'a>,
    states: CoreAxisStates,
}

#[derive(Clone)]
pub(crate) struct CoreAxisStates {
    coupling: BoundAxisState,
    cohesion: BoundAxisState,
    instability: BoundAxisState,
    entropy: BoundAxisState,
    witness_depth: BoundAxisState,
}

impl CoreAxisStates {
    /// Engine için context construction — authorization layer descriptor listesi alır.
    pub(crate) fn descriptors(&self) -> Vec<AxisDescriptor> {
        vec![
            self.coupling.descriptor.clone(),
            self.cohesion.descriptor.clone(),
            self.instability.descriptor.clone(),
            self.entropy.descriptor.clone(),
            self.witness_depth.descriptor.clone(),
        ]
    }
}
```

#### capture_bound_axis_state (v10 P2-2 epoch sandwich)

```rust
/// **Commit 4a P1-2 (v9):** Bound axis için descriptor + epoch consistent epoch-fenced
/// capture + identity verify. "Atomik" yerine "consistent epoch-fenced capture" —
/// descriptor üretimi sırasında concurrent mutation epoch sandwich ile yakalanır.
pub(crate) fn capture_bound_axis_state(
    axis_id: &'static str,
    axis: &dyn Axis,
) -> Result<BoundAxisState, CoordinateMeasurementError> {
    let epoch_before = axis.measurement_epoch();
    let descriptor = axis
        .descriptor()
        .map_err(|source| CoordinateMeasurementError::AxisDescriptorFailed { axis_id, source })?;
    let epoch_after = axis.measurement_epoch();
    if epoch_before != epoch_after {
        return Err(CoordinateMeasurementError::AxisStateChangedDuringCapture {
            axis_id,
            epoch_before,
            epoch_after,
        });
    }
    if descriptor.axis_id() != axis_id {
        return Err(CoordinateMeasurementError::AxisIdentityMismatch {
            runtime_name: axis_id,
            descriptor_id: descriptor.axis_id().to_owned(),
        });
    }
    Ok(BoundAxisState { descriptor, epoch: epoch_after })
}
```

#### Binding refactor (v9 P1-2 delegasyon yönü)

```rust
/// **Commit 4a P1-2 (v9):** Faz 1 + 2.1 + 2.2 — name binding + completeness + duplicate.
/// Descriptor ÜRETMEZ. Ortak düşük seviye binding katmanı.
fn bind_core_axis_refs(&self) -> Result<CoreAxisRefs<'_>, CoordinateMeasurementError> {
    // Faz 1: tek-pass name() binding + duplicate kaydet
    // Faz 2.1: structural completeness (MissingCoreAxes önce)
    // Faz 2.2: DuplicateCoreAxis
    // Faz 2.3 YOK — descriptor çağrısı yok
}

/// **Commit 4a P1-2 (v9):** Refs + her axis için descriptor + epoch atomik capture.
pub(crate) fn bind_core_axes_with_descriptors(&self) -> Result<BoundCoreAxes<'_>, CoordinateMeasurementError> {
    let refs = self.bind_core_axis_refs()?;
    let states = CoreAxisStates {
        coupling: capture_bound_axis_state("coupling", refs.coupling)?,
        cohesion: capture_bound_axis_state("cohesion", refs.cohesion)?,
        instability: capture_bound_axis_state("instability", refs.instability)?,
        entropy: capture_bound_axis_state("entropy", refs.entropy)?,
        witness_depth: capture_bound_axis_state("witness_depth", refs.witness_depth)?,
    };
    Ok(BoundCoreAxes { refs, states })
}

/// **Commit 4a P1-2 (v9):** Compatibility projection — bind_core_axes_with_descriptors'tan
/// refs alır. Backward-compat: mevcut caller'lar kırılmaz. Descriptor'lar drop edilir.
pub(crate) fn bind_core_axes(&self) -> Result<CoreAxisRefs<'_>, CoordinateMeasurementError> {
    Ok(self.bind_core_axes_with_descriptors()?.refs)
}
```

#### BoundMeasurementSession (pre/post/final verify — v9 P1-1 gerçek ABA)

```rust
/// **INV-T9 #70 Commit 4a (reviewer v6/v8/v9/v10 P1):** Measurement session.
///
/// **P1-1 (v9) gerçek transient ABA:** Her measured_position_of çağrısında pre ve post
/// descriptor + epoch verify. A→B→A revert'te descriptor A görülür ama epoch artar
/// (monoton) → fail-closed. Yalnız descriptor equality veya session-sonu digest yakalayamıyordu.
///
/// **P1-3 (v8) neutral layer:** coords-layer authorization tipleri içermez.
pub(crate) struct BoundMeasurementSession<'a> {
    axes: BoundCoreAxes<'a>,
}

impl<'a> BoundMeasurementSession<'a> {
    pub(crate) fn begin(coord_system: &'a CoordinateSystem)
        -> Result<Self, CoordinateMeasurementError>
    {
        Ok(Self { axes: coord_system.bind_core_axes_with_descriptors()? })
    }

    /// Pre-measure + ölçüm + post-measure state verify (descriptor + epoch).
    pub(crate) fn measured_position_of(&self, node: &Node, space: &Space)
        -> Result<MeasuredRawPosition, CoordinateMeasurementError>
    {
        self.verify_bound_states(MeasurementSessionPhase::PreMeasure)?;
        let measured = MeasuredRawPosition {
            coupling: measure_bound_axis("coupling", self.axes.refs.coupling, node, space)?,
            cohesion: measure_bound_axis("cohesion", self.axes.refs.cohesion, node, space)?,
            instability: measure_bound_axis("instability", self.axes.refs.instability, node, space)?,
            entropy: measure_bound_axis("entropy", self.axes.refs.entropy, node, space)?,
            witness_depth: measure_bound_axis("witness_depth", self.axes.refs.witness_depth, node, space)?,
        };
        self.verify_bound_states(MeasurementSessionPhase::PostMeasure)?;
        Ok(measured)
    }

    fn verify_bound_states(&self, phase: MeasurementSessionPhase)
        -> Result<(), CoordinateMeasurementError>
    {
        self.verify_one("coupling", &self.axes.states.coupling, self.axes.refs.coupling, phase)?;
        self.verify_one("cohesion", &self.axes.states.cohesion, self.axes.refs.cohesion, phase)?;
        self.verify_one("instability", &self.axes.states.instability, self.axes.refs.instability, phase)?;
        self.verify_one("entropy", &self.axes.states.entropy, self.axes.refs.entropy, phase)?;
        self.verify_one("witness_depth", &self.axes.states.witness_depth, self.axes.refs.witness_depth, phase)?;
        Ok(())
    }

    fn verify_one(&self, axis_id: &'static str, expected: &BoundAxisState,
                  axis: &dyn Axis, phase: MeasurementSessionPhase)
        -> Result<(), CoordinateMeasurementError>
    {
        let actual = capture_bound_axis_state(axis_id, axis)?;
        if actual.descriptor != expected.descriptor || actual.epoch != expected.epoch {
            return Err(CoordinateMeasurementError::AxisStateDrift {
                axis_id, phase,
                expected_descriptor: Box::new(expected.descriptor.clone()),
                actual_descriptor: Box::new(actual.descriptor),
                expected_epoch: expected.epoch, actual_epoch: actual.epoch,
            });
        }
        Ok(())
    }

    /// Engine descriptor listesi — authorization layer context kurar.
    pub(crate) fn axis_descriptors(&self) -> Vec<AxisDescriptor> { self.axes.states.descriptors() }

    /// Session-sonu defensive verify.
    pub(crate) fn verify_unchanged(&self) -> Result<(), CoordinateMeasurementError> {
        self.verify_bound_states(MeasurementSessionPhase::SessionFinal)
    }
}
```

#### CoordinateMeasurementError yeni varyantlar

```rust
/// **Commit 4a P1-1:** Axis state drift — descriptor ve/veya epoch değişti.
/// Box: AxisDescriptor String + Vec<u8> içerir, large_err önlenir (v8 P2-1).
#[error("axis `{axis_id}` state drift at {phase:?}: expected_descriptor={expected_descriptor:?}, actual_descriptor={actual_descriptor:?}, expected_epoch={expected_epoch:?}, actual_epoch={actual_epoch:?}")]
AxisStateDrift {
    axis_id: &'static str,
    phase: MeasurementSessionPhase,
    expected_descriptor: Box<AxisDescriptor>,
    actual_descriptor: Box<AxisDescriptor>,
    expected_epoch: AxisStateEpoch,
    actual_epoch: AxisStateEpoch,
},

/// **Commit 4a P2-2 (v10):** Capture sırasında epoch sandwich drift.
#[error("axis `{axis_id}` state changed during capture: epoch_before={epoch_before:?}, epoch_after={epoch_after:?}")]
AxisStateChangedDuringCapture {
    axis_id: &'static str,
    epoch_before: AxisStateEpoch,
    epoch_after: AxisStateEpoch,
},
```

#### Mevcut CoordinateSystem::measured_position_of delegate

```rust
pub fn measured_position_of(&self, node: &Node, space: &Space)
    -> Result<MeasuredRawPosition, CoordinateMeasurementError>
{
    let session = BoundMeasurementSession::begin(self)?;
    session.measured_position_of(node, space)
    // verify_unchanged measured_position_of içinde pre/post yapıyor.
}
```

### 2. `crates/osp-core/src/engine.rs` — measured_centroid migration (v8 P1-4 compat)

```rust
/// **Commit 4a P1-4 (v8):** Session-bound centroid — tüm node'lar aynı bound refs üzerinden.
pub(crate) fn measured_centroid_in_session(
    &self,
    session: &crate::coords::BoundMeasurementSession<'_>,
    space: &crate::space::Space,
    member_ids: &[crate::space::NodeId],
) -> Result<crate::coords::MeasuredRawPosition, crate::measurement::MeasurementError> {
    // Aynı centroid logic ama session.measured_position_of kullanır (pre/post verify dahil).
    // Mass validation + aggregate_axis_measurement (Commit 3 unchanged).
}

/// **Commit 4a P1-4 (v8):** Backward-compat wrapper — try_compute_raw_from_delta unchanged.
pub(crate) fn measured_centroid_of(&self, space: &crate::space::Space, member_ids: &[crate::space::NodeId])
    -> Result<crate::coords::MeasuredRawPosition, crate::measurement::MeasurementError>
{
    let session = crate::coords::BoundMeasurementSession::begin(&self.coord_system)
        .map_err(crate::measurement::MeasurementError::CoordinateMeasurement)?;
    let measured = self.measured_centroid_in_session(&session, space, member_ids)?;
    session.verify_unchanged()
        .map_err(crate::measurement::MeasurementError::CoordinateMeasurement)?;
    Ok(measured)
}
```

#### measure_task_delta — tek session, manuel fence KALKAR

```rust
// ESKİ (engine.rs:1608-1629): context_before + ... + context_after + digest equality
// YENİ:
let session = crate::coords::BoundMeasurementSession::begin(&self.coord_system)
    .map_err(MeasurementError::CoordinateMeasurement)?;

let before_centroid = self.measured_centroid_in_session(&session, &self.space, &existing)?;
let after = self.measured_centroid_in_session(&session, &hypothetical, subject.member_ids())?;
session.verify_unchanged()
    .map_err(MeasurementError::CoordinateMeasurement)?;

// P1-3 (v8): Context authorization layer'da kurulur (coords neutral).
let context = crate::authorization::MeasurementInputContext::try_new(session.axis_descriptors())
    .map_err(MeasurementError::MeasurementContext)?;
```

**MeasurementError::MeasurementContextDrift varyantı retained** — artık üretilmez (session `AxisStateDrift` üretir), ama API stability için korunur (Commit 4b'de kaldırılabilir).

### 3. Test'ler (+13 → 1017 → ~1030)

#### coords.rs — BoundMeasurementSession birim test'leri (9)

1. `bound_measurement_session_begin_captures_descriptors`
2. `coordinate_system_measured_position_delegates_to_session` (v8 P2-3 delegasyon)
3. `bound_measurement_session_measured_position_of_fixed_values` (v8 P2-3 sabit değer: coupling=0.2/Scip vb.)
4. `bound_measurement_session_verify_unchanged_no_drift`
5. **★ `bound_measurement_session_rejects_persistent_descriptor_drift`** (v10 P2-4 — descriptor A→B, B kalır → mismatch)
6. **★ `bound_measurement_session_rejects_transient_aba_via_epoch`** (v10 P2-4 + v9 P1-1 — EpochDriftingAxis: descriptor sabit A, epoch her measure()'da artar → post-measure epoch mismatch)
7. **★ `session_begin_captures_each_descriptor_once`** (v9 P1-2 — DescriptorCallCounterAxis: her core axis için tam 1 descriptor çağrısı)
8. **★ `external_axis_can_produce_nonzero_epoch`** (v10 P1-2 — AtomicU64 mutable axis, measurement_epoch override, non-zero epoch)
9. `bound_measurement_session_begin_propagates_missing_core_axes` + `_duplicate_core_axis` (ayrı test'ler)

**Fixture'lar:**
- `EpochDriftingAxis` — `name()` sabit "coupling", `descriptor()` sabit (A), `measurement_epoch()` her `measure()` çağrısında `AtomicU64::fetch_add(1)` ile artar
- `DescriptorCallCounterAxis` — `Arc<AtomicU64>` descriptor çağrı sayacı
- `ExternalMutableAxis` — `AtomicU64` epoch, `measurement_epoch()` override

#### engine.rs — session migration test'leri (4)

10. `measured_centroid_in_session_uses_bound_refs`
11. `measured_centroid_of_wrapper_creates_session` (v8 P1-4 backward-compat)
12. `measure_task_delta_session_rejects_axis_drift` (engine seviyesi drift)
13. **★ `measurement_token_context_equals_session_captured_descriptors`** (v9 — token context session snapshot'tan, yeniden CoordinateSystem traversal değil)

★ = reviewer explicit blocking test'ler (v6/v8/v9/v10).

---

## CI doğrulaması

```bash
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo build --workspace --examples --exclude osp-desktop
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib    # 1017 → ~1030 (+13)
RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop
cargo clippy -p osp-core --lib    # parent parity (12 warning) — yeni warning YOK
cargo check -p osp-desktop --lib    # parent parity (Commit 4a osp-desktop'a dokunmaz)
```

osp-desktop Commit 4a'dan etkilenmez.

---

## Risk & governance

- **Additive, authority caller'lara dokunmaz:** TaskCommitInput, commit_task_claim, PredicateGateInput, navigator, MCP, CLI — unchanged. Commit 4b ön koşulu.
- **P1-1 gerçek transient ABA:** `AxisStateEpoch` monoton. A→B→A revert'te descriptor A görülür ama epoch artar → fail-closed. `measurement_epoch()` default impl sabit ZERO (immutable axis'ler backward-compat). TCB sınırı doc'ta açık.
- **P1-2 atomik bind+capture:** `bind_core_axis_refs` (refs only) → `bind_core_axes_with_descriptors` (refs + state) → `bind_core_axes` (compat projection). Descriptor + epoch consistent epoch-fenced capture. `session_begin_captures_each_descriptor_once` blocking test.
- **P1-3 neutral layer:** `coords.rs` authorization tipleri içermez. Session yalnız `AxisDescriptor` + `AxisStateEpoch` (neutral) taşır. Engine context'i authorization layer'da kurar.
- **P1-4 compatibility:** `measured_centroid_in_session` + wrapper. `try_compute_raw_from_delta` unchanged.
- **P1-1 (v10) public visibility:** `MeasurementSessionPhase` pub + non_exhaustive.
- **P1-2 (v10) external construct:** `AxisStateEpoch` pub const fn new/get, From<u64>, ZERO, Default.
- **P2-1 Box:** `AxisStateDrift` `Box<AxisDescriptor>` — large_err önlenir.
- **P2-2 typed phase + epoch sandwich:** `MeasurementSessionPhase` enum, `AxisStateChangedDuringCapture`.
- **P2-3 gerçek parity:** Sabit beklenen değerler + delegasyon testi ayrı.
- **P2-4 persistent/transient ayrımı:** İki ABA test ayrı savunma katmanı.

---

## Commit planı

Tek implementation commit:
```
feat(coords): BoundMeasurementSession — single-bind axis session integrity (#70 commit 4a)

Reviewer v6/v8/v9/v10 P1 merge-blocker closure. Atomik authority migration'dan (Commit 4b)
önce measurement session integrity altyapısı.

P1-1 (v10) public visibility: MeasurementSessionPhase pub + non_exhaustive.
P1-2 (v10) external construct: AxisStateEpoch pub const fn new/get, From<u64>, ZERO, Default.
P1-1 (v9) gerçek transient ABA: AxisStateEpoch monoton.
P1-2 (v9) atomik bind+capture: bind_core_axis_refs → bind_core_axes_with_descriptors →
  bind_core_axes (compat projection). Descriptor + epoch consistent epoch-fenced capture.
P1-3 (v8) neutral layer: coords.rs authorization tipleri içermez.
P1-4 (v8) compat: measured_centroid_in_session + measured_centroid_of wrapper.
P2-1 Box (large_err), P2-2 typed phase + epoch sandwich, P2-3 sabit parity test,
P2-4 persistent/transient ABA ayrımı.

- coords.rs: AxisStateEpoch (pub), Axis::measurement_epoch() default ZERO,
  MeasurementSessionPhase (pub, non_exhaustive), CoreAxisStates/BoundAxisState/BoundCoreAxes,
  capture_bound_axis_state (epoch sandwich), bind_core_axis_refs/
  bind_core_axes_with_descriptors/bind_core_axes (compat), BoundMeasurementSession
  (pre/post/final verify), CoordinateMeasurementError::AxisStateDrift (Box, typed phase) +
  AxisStateChangedDuringCapture
- engine.rs: measured_centroid_in_session (session-bound), measured_centroid_of (wrapper),
  measure_task_delta manuel fence KALKAR — BoundMeasurementSession + verify_unchanged,
  context authorization layer'da (MeasurementInputContext::try_new(session.axis_descriptors()))
- coords.rs test: bound_measurement_session_rejects_persistent_descriptor_drift (★),
  bound_measurement_session_rejects_transient_aba_via_epoch (★),
  session_begin_captures_each_descriptor_once (★),
  external_axis_can_produce_nonzero_epoch (★) + 5 session test
- engine.rs test: measured_centroid session migration + measure_task_delta drift rejection
  + measurement_token_context_equals_session_captured_descriptors (★)

Additive — TaskCommitInput/commit_task_claim/PredicateGateInput/navigator/MCP/CLI unchanged.
Commit 4b atomik authority migration'unun ön koşulu.

Test: 1017 → ~1030 osp-core lib tests (+13).
CI: fmt clean, build clean, clippy parent parity (12 warning), osp-desktop parent parity.
```

Review pattern (Commit 1/2/3 gibi): implementation → scoped review → closure commit'leri → Commit 4b'ye geçiş.

---

## Açık issue'lar (Commit 4a sonrası)

- **Commit 4b:** atomik authority migration (TaskCommitInput, AuthorizationBasis v2, tüm caller'lar, deprecation, trybuild, #80 fix, v2 golden).
- **#79** (PredicateAxis fallback) — Commit 4b kapsamı dışı.
- **#80** (osp-desktop) — Commit 4b'de çözülür.
- **Module scope resolution** — Commit 4b'de graph-aware SubjectScopeResolver trait.

---

*Bu belge INV-T9 #70 Commit 4a implementation planıdır. Reviewer v10 APPROVED 9.9/10 (implementation-ready). Commit 3 reviewer v7 CONDITIONAL APPROVE 9.8/10 carryover'ı kapatır.*
