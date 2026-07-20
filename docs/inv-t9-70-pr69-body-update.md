## ⚠️ HIGH-RISK — GOVERNANCE §3 (witness/quorum safety + evidence integrity)

> **Independent review is POLICY-REQUIRED.** This PR is **not merged** until an eligible
> independent reviewer is engaged. CI green → "ready for eligible independent review", not merge.

---

## Özet

Paper 2 model–implementation conformance fix. INV-T9 — External-Evidence Suspension Isolation (Model B).

## Kapsam durumu (Steps 1-6 done + #71 + #72 landed + #70 Commit 1-3 + Commit 4a implementation + v11/v12 closures landed — Commit 4a scoped review APPROVED 9.9/10)

Steps 1-6 + #71 (canonical decision-basis) + #72 (embedded attempt-evidence integrity) + #70 Commit 1-3 (provenance-aware axis measurement + position measurement + subject-bound EngineMeasurement tokens) + **#70 Commit 4a (BoundMeasurementSession — single-bind axis session integrity)** implementation + closure commit'leri tamamlandı. **#70 Commit 4a implementation landed (f72ed85) + v11 closure (5c271f5) + v12 closure (8c22d86) — scoped review APPROVED 9.9/10. Commit 4b-6 pending.**

### ✅ #72 — embedded attempt-evidence integrity (5 implementation commits + 5 closures)
- Commit 1-5: canonical evidence model, navigator factory, envelope binding, dangling id removal, persisted tamper matrix
- Closure 1-5: load/persist integrity, production mapper, conditional Held assertions, deterministic Held production-path, reviewer P2 cleanup

### ✅ #70 — provenance-aware axis measurement + subject-bound tokens (Commit 1-3 + Commit 4a + closures)
- **Commit 1** (a300d75): provenance-native axis measurement contract — MetricSource::Mixed, AxisMeasurement, MeasuredRawPosition, Axis::measure() authoritative, validate_direct_source, v6/v7 closure (6aaeb39 + 0d4eb51)
- **Commit 2** (080009e): provenance-aware position measurement — measured_position_of/try_raw_position_of authority surface, CoordinateMeasurementError, aggregate_source, bind_core_axes (P1-1 tek-pass reference binding)
  - Closure v1 (059ed04): Axis descriptor contract, bind_core_axes defensive
  - Closure v2 (eb9903b): MissingCoreAxes precedence iki-fazlı binding, gerçek mutable-state drift fixture'ları
- **Commit 3** (22e3d93): subject-bound EngineMeasurement tokens — canonical_encoding.rs neutral layer, measurement.rs subject-bound token + MeasurementDeltaDigest (shared CanonicalStructuralDelta producer) + cross-field validated MeasurementRequest + EngineMeasurement context↔digest defensive verify + MeasurementDigestError public boundary + v1 goldens
  - Closure v1 (650c620): reviewer v5 (REQUEST CHANGES 9.2/10) — measurement context fence (interior mutability), shared producer authorization yoluna uygulandı, predicate scope duplicate bypass kaldırıldı, gerçek v1 golden sabitleri, error taxonomy categorization, heterogeneous diagnostic scopes
  - Closure v2 (0d73801): reviewer v6 (REQUEST CHANGES 9.6/10) conditional approval — P1 BoundMeasurementSession Commit 4 merge-blocker, P2-1 source-level producer contract test (include_str!), P2-2 trybuild Deserialize guards carryover, P2-3 PR body truth-surface
- **Commit 4a** (f72ed85): BoundMeasurementSession — single-bind axis session integrity. Reviewer v6 carryover kapatıldı: AxisStateEpoch (pub, monoton), MeasurementSessionPhase (pub, non_exhaustive), Axis::measurement_epoch() default ZERO, capture_bound_axis_state (epoch sandwich), bind_core_axis_refs → bind_core_axes_with_descriptors, BoundMeasurementSession (begin/measured_position_of/verify_unchanged/axis_descriptors — pre/post/final verify), CoordinateMeasurementError::AxisStateDrift (Box, typed phase) + AxisStateChangedDuringCapture, measured_centroid_in_session + wrapper, measure_task_delta manuel context-before/context-after digest fence KALKTI — tek session + verify_unchanged, context authorization layer'da. 1017 → 1033 osp-core lib tests (+16).
  - v11 closure (5c271f5): scoped review REQUEST CHANGES 9.6/10 (1 P1 + 4 P2) — P1 production-path `measure_task_delta → AxisStateDrift → token yok` testi, P2-1 gerçek `EngineMeasurement.context()`, P2-2 epoch-sandwich gerçek fixture (capture + begin), P2-3 stale `bind_core_axes` doc references, P2-4 truth-surface `f72ed85`.
  - v12 closure (8c22d86): scoped review APPROVED 9.9/10 (2 non-blocking P2) — P2-1 full descriptor equality (axis_id + semantics_version + canonical_parameters), P2-2 truth-surface `5c271f5` + `tests/compile_fail/` yolu düzeltmesi + gerçek GitHub PR body sync.

## Commits (repository head: see live PR head — milestone commits below)

```
8c22d86  test(inv-t9): #70 commit 4a review v2 closure — full descriptor equality + truth-surface sync
5c271f5  test(inv-t9): #70 commit 4a review v1 closure — production drift rejection + truth-surface
f72ed85  feat(coords): BoundMeasurementSession — single-bind axis session integrity (#70 commit 4a)
c6b25f6  docs(inv-t9): #70 commit 4a plan + Commit 3 handoff truth-surface update
389e7db  docs(inv-t9): #70 commit 3 review v3 closure — truth-surface head + producer contract hardening
0d73801  #70 commit 3 review v2 closure — producer contract test + Commit 4 P1 merge-blocker
650c620  #70 commit 3 review v1 closure — session fence + golden pin + producer parity
22e3d93  feat(engine): subject-bound EngineMeasurement tokens (#70 commit 3)
3b4231f  docs(inv-t9): #70 Commit 3 handoff (Commit 2 reviewer 10/10 APPROVED)
eb9903b  #70 commit 2 review v2 closure — error precedence + real drift tests
059ed04  #70 commit 2 review v1 closure — bind defensive + descriptor contract
080009e  feat(coords): provenance-aware position measurement (#70 commit 2)
7c0f8c8  docs(inv-t9): #70 Commit 2 handoff (reviewer v3 APPROVED 9.7/10)
0d4eb51  #70 commit 1 review v7 closure — set-level Mixed fail-closed (Any/Weighted bypass)
6aaeb39  #70 commit 1 review v6 closure — Mixed source fail-closed + SCIP wiring table test
a300d75  feat(coords): provenance-native axis measurement contract (#70 commit 1)
... (earlier #72 + Steps 1-6 commits)
```

## Doğrulama (repository head: see live PR head — Commit 4a implementation + v11/v12 closures landed)

- ✅ GitHub CI: Build & Test — pass
- ✅ RUSTFLAGS="-D warnings" cargo build --workspace --examples --exclude osp-desktop — temiz
- ✅ RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop — tüm testler geçer
- ✅ cargo fmt --all -- --check — temiz (workspace-wide)
- ✅ **1035 osp-core lib tests** (1017 → +16 Commit 4a, +2 v11 closure)
- ✅ cargo clippy -p osp-core --lib — 12 warnings (parent `3b4231f` parity)
- ✅ INV-T9 conformance test matrisi + v1 golden vectors (5: AuthorizationBasis, MeasurementInput, EvaluationContext, SuspendedAttemptEvidence, MeasurementDelta + MeasurementRequest)

## #70 Commit 3 — subject-bound EngineMeasurement tokens scope

- **canonical_encoding.rs (private):** Neutral BLAKE3 framing primitives — `encode_u64/u32/u8`, `encode_bytes`, `encode_f64`, `canonical_f64_bytes`, `CanonicalTag` trait, `CanonicalEncodingError`. Authorization ve measurement stable `From` mapping ile sarmalar.
- **measurement.rs (public):** `CanonicalSubjectScope` (sort + duplicate reject), `CanonicalImpactScope` (CanonicalEdgeIdentity), `MeasurementDeltaDigest` (shared CanonicalStructuralDelta producer, defensive validate, AS-IS encode), `MeasurementRequest` (try_new digest'leri üretir), `MeasurementRequestDigest`, `MeasurementBaseline` + `BaselineUnavailableReason`, `EngineMeasurement` (private-field token, cross-field defensive verify), `MeasurementError` + `MeasurementDigestError` + `SubjectScopeResolutionError`.
- **authorization.rs:** Encoding primitives taşındı (4 existing golden byte-for-byte unchanged), `canonical_structural_delta_from_claim` shared producer, `encode_space_view_id` pub(crate) infallible, `canonicalize_node` CanonicalizationError döner, canonical encoder'lar pub(crate).
- **engine.rs:** `measure_task_delta(TaskBoundClaim, expected_base_revision, hint)`, `derive_task_subject_scope/derive_impact_scope` (canonical return), `measured_centroid_of` (axis identity + mass + total mass validation), `try_compute_raw_from_delta` (add-only).

### Reviewer v1→v5 turu (8.9 → 9.3 → 9.6 → 9.7 → 9.2 → 9.6)

24 P0/P1/P2 bulgusu kapatıldı. En kritik olanlar: delta binding (P0-1), subject/impact bağımsız kümeler, revision reachability, TaskBoundClaim defensive, heterojen scope fail-closed, baseline matrix, current-scope kaldırıldı, neutral encoding, public error boundary, canonical impact identity, shared structural producer, centroid axis identity + mass validation, digest framing, canonical derivation, defensive validate, single canonicalization, cross-field digest integrity, blanket From kaldırıldı, serialize-only request, hex error varyantları, nested Serialize, measurement context fence, golden pin, dedup bypass kaldırıldı.

## Kalan işler (merge-blocking)

### #70 — EngineMeasurement pipeline (Commit 4a APPROVED 9.9/10 → 4b-6 pending)

- **Commit 4a — `feat(coords): BoundMeasurementSession — single-bind axis session integrity` (LANDED f72ed85 + v11 closure 5c271f5 + v12 closure 8c22d86)**
  - Reviewer v6/v8/v9/v10 P1 merge-blocker closure: AxisStateEpoch + MeasurementSessionPhase + BoundMeasurementSession (pre/post/final verify — gerçek transient ABA epoch ile).
  - v11 closure (5c271f5) — scoped review REQUEST CHANGES 9.6/10 (1 P1 + 4 P2):
    - **P1 (KAPANDI):** gerçek `measure_task_delta → AxisStateDrift → token yok` production-path testi (`DriftDuringMeasurementAxis` fixture + gerçek producer).
    - **P2-1 (KAPANDI v11):** token-context testi gerçek `EngineMeasurement.context()` ile.
    - **P2-2 (KAPANDI):** epoch-sandwich gerçek fixture (`EpochChangesDuringDescriptorAxis` → `AxisStateChangedDuringCapture` capture + begin yolundan).
    - **P2-3 (KAPANDI):** kod yorumları `bind_core_axes` (compat, kaldırıldı) referanslarını günceller — `bind_core_axis_refs → bind_core_axes_with_descriptors → BoundMeasurementSession` zinciri.
    - **P2-4 (KAPANDI):** PR body truth-surface `f72ed85` durumuna güncellendi.
  - v12 closure (8c22d86) — scoped review APPROVED 9.9/10 (2 non-blocking P2):
    - **P2-1 (HARDENED v12):** token-context testinde full `AxisDescriptor` equality (axis_id + semantics_version + canonical_parameters byte-for-byte) — axis_id-only karşılaştırma version/parameters farkını kaçırırdı.
    - **P2-2 (TRUTH-SURFACE v12):** body-update doc `5c271f5`/APPROVED durumuna + `tests/ui/` → `tests/compile_fail/` yolu düzeltmesi + gerçek GitHub PR body sync.
  - **P1 BoundMeasurementSession merge-blocker: CLOSED** — Commit 4b authority migration'unu bloke etmiyor.

- **Commit 4b — `refactor(inv-t4): require EngineMeasurement across authority and evidence paths` (ATOMIK)**
  - `TaskCommitInput { claim, omega, task_resolver, measurement }` (subject_scope YOK — token'a taşındı)
  - `commit_task_claim` migration + `claim.computed_raw` ignore + Mixed validation
  - `AuthorizationBasis v2` (before+after single canonical + request digest + baseline/loss consistency)
  - `PredicateGateInput` → token baseline/after
  - `TrajectoryEvidenceBaseline` enum
  - Tüm caller migration atomik: Navigator, MCP, CLI, g2c, test construction site'ları
  - `provenanced_from_raw` production/evidence path'ten kaldır
  - `raw_position_of` + `position_of` + `Axis::compute()` `#[deprecated]`
  - Domain sep `osp.authorization-basis.v2\0`
  - Issue #80 (osp-desktop Claim struct field migration) çözümü

  **P2 carryover — compile-fail Deserialize guards (reviewer v6):**

  `tests/compile_fail/engine_measurement_deserialize.rs` ve
  `tests/compile_fail/measurement_request_deserialize.rs` `trybuild` fixture'ları
  Commit 4b'de eklenmeli. Repo konvansiyonu `tests/compile_fail/` (anchoring_typelevel.rs
  orchestrator `cN_*` prefix pattern'i).

- **Commit 5 — `test(inv-t4): adversarial measurement-binding regressions`** (19 regression test)
- **Commit 6 — `docs(inv-t4): conformance + truth-surface`** (Conformance doc, #70 acceptance checklist, PR body sync)

### #72 — embedded attempt-evidence integrity — closure landed, scoped review pending
### #73 — witness Q3 honest-reject production wiring — PR #69 merge decision requires governance call

## Truth-surface

```
Repository head: see live PR head (https://github.com/ontologicalspace/osp/pull/69)
  — doc'ta "current head X" tutmak her closure commit'inde stale yaratır; canlı head
    yalnız GitHub PR body'sinde. Bu doc yalnız milestone commit'lerini taşır (stable).

osp-core lib tests: 1035 (1017 → +16 Commit 4a, +2 v11 closure)
workspace tests (excl. osp-desktop): green
cargo check -p osp-desktop --lib: parent parity (2 #80-originated errors, Commit 4a'dan değil — Issue #80 Commit 4b)
cargo clippy -p osp-core --lib: 12 warnings (parent `3b4231f` parity)

#70 Commit 4a milestone commits (stable — tarihsel):
  implementation: f72ed85 (BoundMeasurementSession)
  v11 closure:    5c271f5 (1 P1 production drift rejection + 4 P2)
  v12 closure:    8c22d86 (full descriptor equality + truth-surface sync)
  scoped review:  APPROVED 9.9/10
  P1 BoundMeasurementSession merge-blocker: CLOSED — Commit 4b'i bloke etmiyor
#70 Commit 3: landed (22e3d93) — subject-bound EngineMeasurement tokens
#70 Commit 3 review v5 closure: landed (650c620) — session fence + golden pin + producer parity
#70 Commit 3 review v6 closure: landed (0d73801) — producer contract test + Commit 4 P1 merge-blocker
#70 Commit 1 + v6/v7 closure: landed (0d4eb51)
#70 Commit 2 + review v1/v2 closure: landed (eb9903b)
#72 implementation + 5 closures: landed (920a1dc), scoped review pending

Commit 4 carryover (P1 merge-blocker): CLOSED (Commit 4a implementation + v11/v12 closures landed)
Commit 4b carryover (P2): trybuild compile-fail Deserialize guards (tests/compile_fail/)

#70: Commit 4a APPROVED 9.9/10 → Commit 4b-6 pending
#73: Q3 wiring — PR #69 merge decision governance call required
eligible independent review: still required (GOVERNANCE §3 high-risk)
```

## Conformance evidence

Tam dokümantasyon: [`docs/paper2-notes/conformance/inv-t9-external-evidence-suspension.md`](docs/paper2-notes/conformance/inv-t9-external-evidence-suspension.md)

🤖 Generated with [ZCode](https://github.com/ervolkan/zai-coding-plan)
