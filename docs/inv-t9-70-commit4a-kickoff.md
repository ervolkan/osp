# INV-T9 #70 Commit 4a Implementation — Yeni Oturum Kickoff Mesajı

Aşağıdaki mesajı yeni oturumda bana paste et:

---

```
INV-T9 #70 Commit 4a (BoundMeasurementSession) implementation'a başlıyoruz.

Önce plan belgesini oku:
docs/inv-t9-70-commit4a-plan.md

Bu belge reviewer v10 APPROVED 9.9/10 (implementation-ready) Commit 4a planı içerir.
Commit 3 (22e3d93 + 650c620 + 0d73801 + 389e7db, reviewer v7 CONDITIONAL APPROVE
9.8/10) tamamen bitti. Commit 4 ikiye bölündü:
- Commit 4a (BU): BoundMeasurementSession P1 merge-blocker — additive, authority
  caller'lara dokunmaz. Reviewer v6 carryover'ı kapatır.
- Commit 4b (sonra): atomik authority migration (TaskCommitInput, AuthorizationBasis
  v2, tüm caller'lar, deprecation, trybuild, #80 fix, v2 golden).

Önce durumu doğrula:

cd P:/Work/SoftwarePhysics
git checkout fix/inv-t9-witness-suspension
git pull  # 389e7db head olmalı (doc closure commit ile 41a2b3c olabilir)
git log --oneline -6
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 1017 test geçmeli

Sonra Commit 4a planını uygula (docs/inv-t9-70-commit4a-plan.md):
- crates/osp-core/src/coords.rs: AxisStateEpoch (pub, external construct),
  Axis::measurement_epoch() default ZERO, MeasurementSessionPhase (pub,
  non_exhaustive), CoreAxisStates/BoundAxisState/BoundCoreAxes,
  capture_bound_axis_state (epoch sandwich), bind_core_axis_refs →
  bind_core_axes_with_descriptors → bind_core_axes (compat projection),
  BoundMeasurementSession (pre/post/final verify — descriptor + epoch),
  CoordinateMeasurementError::AxisStateDrift (Box, typed phase) +
  AxisStateChangedDuringCapture
- crates/osp-core/src/engine.rs: measured_centroid_in_session (session-bound),
  measured_centroid_of (wrapper, P1-4 compat), measure_task_delta manuel fence
  KALKAR — BoundMeasurementSession + verify_unchanged, context authorization
  layer'da (MeasurementInputContext::try_new(session.axis_descriptors()))
- ~13 yeni test (1017 → ~1030):
  ★ bound_measurement_session_rejects_persistent_descriptor_drift
  ★ bound_measurement_session_rejects_transient_aba_via_epoch (EpochDriftingAxis)
  ★ session_begin_captures_each_descriptor_once (DescriptorCallCounterAxis)
  ★ external_axis_can_produce_nonzero_epoch (AtomicU64 mutable axis)
  ★ measurement_token_context_equals_session_captured_descriptors
  + 8 session birim/parity test

Additive, non-breaking — TaskCommitInput/commit_task_claim/PredicateGateInput/
navigator/MCP/CLI unchanged. Commit 4b atomik authority migration'unun ön koşulu.

Review pattern: Commit 1/2/3 gibi — implementation commit + closure commit'leri
ayrı review turu.

Önemli notlar:
- P1-1 (v9) gerçek transient ABA: AxisStateEpoch monoton. A→B→A revert'te
  descriptor A görülür ama epoch artar → fail-closed.
- P1-2 (v9) delegasyon yönü: bind_core_axis_refs (refs only, descriptor YOK) →
  bind_core_axes_with_descriptors (refs + state) → bind_core_axes (compat).
  Descriptor tam bir kez capture.
- P1-1 (v10) public visibility: MeasurementSessionPhase pub + non_exhaustive
  (CoordinateMeasurementError varyantında kullanılıyor — private_interfaces
  warning).
- P1-2 (v10) external construct: AxisStateEpoch pub const fn new/get, From<u64>,
  ZERO, Default. External Axis implementor'lar non-zero epoch üretebilir.
- P1-3 (v8) neutral layer: coords.rs authorization tipleri içermez
  (MeasurementInputContext/Digest YOK).
- P1-4 (v8) compat: measured_centroid_in_session + measured_centroid_of wrapper.
  try_compute_raw_from_delta unchanged.

CI doğrulaması:
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo build --workspace --examples --exclude osp-desktop
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 1017 → ~1030
RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop
cargo clippy -p osp-core --lib  # parent parity (12 warning), yeni warning YOK
cargo check -p osp-desktop --lib  # parent parity

Issue'lar: #79 (PredicateAxis fallback), #80 (osp-desktop) Commit 4b kapsamında.
```

---

## Oturum başlatma sonrası

Bu mesajı paste ettiğinde şu adımları izleyeceğim:

1. **Plan belgesini oku** (`docs/inv-t9-70-commit4a-plan.md`)
2. **Durumu doğrula** — `git pull`, `git log`, 1017 test
3. **Zemin keşfi** — `coords.rs` mevcut `bind_core_axes`, `CoreAxisRefs`, `validate_bound_axis_identity`; `engine.rs` `measured_centroid_of`, `measure_task_delta` manuel fence
4. **Implementation** — plan belgesindeki sırayla:
   - coords.rs: AxisStateEpoch + Axis trait + MeasurementSessionPhase + BoundCoreAxes + capture + binding refactor + BoundMeasurementSession + error varyantlar
   - engine.rs: measured_centroid_in_session + wrapper + measure_task_delta session migration
   - Test'ler (13 yeni)
5. **CI doğrulaması** — fmt, build, test (1017→~1030), clippy parity, osp-desktop parity
6. **Commit** — `feat(coords): BoundMeasurementSession — single-bind axis session integrity (#70 commit 4a)`
7. **Push** + PR body truth-surface güncelle

Review pattern: implementation → scoped review (v1) → closure commit'leri → Commit 4b'ye geçiş.
