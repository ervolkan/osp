## ⚠️ HIGH-RISK — GOVERNANCE §3 (witness/quorum safety + evidence integrity)

> **Independent review is POLICY-REQUIRED.** This PR is **not merged** until an eligible
> independent reviewer is engaged. CI green → "ready for eligible independent review", not merge.

---

## Özet

Paper 2 model–implementation conformance fix. INV-T9 — External-Evidence Suspension Isolation (Model B).

## Kapsam durumu (Steps 1-6 done + #71 + #72 landed + #70 Commit 1-3 + closures landed — Commit 4-6 pending)

Steps 1-6 + #71 (canonical decision-basis) + #72 (embedded attempt-evidence integrity) + #70 Commit 1-3 (provenance-aware axis measurement + position measurement + subject-bound EngineMeasurement tokens) implementation + closure commit'leri tamamlandı. **#70 Commit 3 + review v5 closure landed (650c620); Commit 4-6 pending.**

### ✅ #72 — embedded attempt-evidence integrity (5 implementation commits + 5 closures)
- Commit 1-5: canonical evidence model, navigator factory, envelope binding, dangling id removal, persisted tamper matrix
- Closure 1-5: load/persist integrity, production mapper, conditional Held assertions, deterministic Held production-path, reviewer P2 cleanup

### ✅ #70 — provenance-aware axis measurement + subject-bound tokens (Commit 1-3 + closures)
- **Commit 1** (a300d75): provenance-native axis measurement contract — MetricSource::Mixed, AxisMeasurement, MeasuredRawPosition, Axis::measure() authoritative, validate_direct_source, v6/v7 closure (6aaeb39 + 0d4eb51)
- **Commit 2** (080009e): provenance-aware position measurement — measured_position_of/try_raw_position_of authority surface, CoordinateMeasurementError, aggregate_source, bind_core_axes (P1-1 tek-pass reference binding)
  - Closure v1 (059ed04): Axis descriptor contract, bind_core_axes defensive
  - Closure v2 (eb9903b): MissingCoreAxes precedence iki-fazlı binding, gerçek mutable-state drift fixture'ları
- **Commit 3** (22e3d93): subject-bound EngineMeasurement tokens — canonical_encoding.rs neutral layer, measurement.rs subject-bound token + MeasurementDeltaDigest (shared CanonicalStructuralDelta producer) + cross-field validated MeasurementRequest + EngineMeasurement context↔digest defensive verify + MeasurementDigestError public boundary + v1 goldens
  - Closure v1 (650c620): reviewer v5 (REQUEST CHANGES 9.2/10) — measurement context fence (interior mutability), shared producer authorization yoluna uygulandı, predicate scope duplicate bypass kaldırıldı, gerçek v1 golden sabitleri, error taxonomy categorization, heterogeneous diagnostic scopes

## Commits (current head 650c620)

```
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

## Doğrulama (current head 650c620 — #70 Commit 3 + review v1 closure landed)

- ✅ GitHub CI: Build & Test — pass
- ✅ RUSTFLAGS="-D warnings" cargo build --workspace --examples --exclude osp-desktop — temiz
- ✅ RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop — tüm testler geçer
- ✅ cargo fmt --all -- --check — temiz (workspace-wide)
- ✅ **1017 osp-core lib tests** + full workspace tests (951 → +66: #70 Commit 3 +62, review v1 closure +4)
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

### #70 — EngineMeasurement pipeline (Commit 4-6 pending)

- **Commit 4 — `refactor(inv-t4): require EngineMeasurement across authority and evidence paths` (ATOMIK)**
  - `TaskCommitInput { claim, omega, task_resolver, measurement }` (subject_scope YOK — token'a taşındı)
  - `commit_task_claim` migration + `claim.computed_raw` ignore + Mixed validation
  - `AuthorizationBasis v2` (before+after single canonical + request digest + baseline/loss consistency)
  - `PredicateGateInput` → token baseline/after
  - `TrajectoryEvidenceBaseline` enum
  - Tüm caller migration atomik: Navigator, MCP, CLI, g2c, test construction site'ları
  - `provenanced_from_raw` production/evidence path'ten kaldır
  - `raw_position_of` + `position_of` + `Axis::compute()` `#[deprecated]`
  - Domain sep `osp.authorization-basis.v2\0`

  **⚠️ P1 MERGE-BLOCKER — BoundMeasurementSession (reviewer v6 carryover):**

  Commit 4, EngineMeasurement'ı herhangi bir authority/evidence caller'a bağlamadan önce:

  1. Core axis referanslarını yalnız bir kez bind etmelidir (`BoundMeasurementSession`).
  2. Before ve after ölçümlerinin tamamı aynı bound referanslarla yapılmalıdır.
  3. Session başlangıcındaki measurement context token'a bağlanmalıdır.
  4. Session sonunda descriptor/context drift fail-closed doğrulanmalıdır.
  5. **A→B→A ABA descriptor fixture'ı token üretimini reddetmelidir** —
     `bound_measurement_session_rejects_aba_descriptor_drift` blocking test.
  6. Bu koşullar sağlanmadan legacy measured input migration'ı yapılamaz.

  Commit 3 context-before/context-after fence kalıcı descriptor değişikliğini yakalar
  ama ABA senaryosunu (node ölçümleri arası geçici descriptor drift) yakalamaz. Commit 3
  add-only olduğu için (EngineMeasurement henüz production authority yoluna bağlanmıyor)
  bu koşul Commit 4 migration ön koşulu olarak taşınır — "P1-1 tamamen kapandı" denmez.

  **P2 carryover — compile-fail Deserialize guards (reviewer v6):**

  `tests/ui/engine_measurement_deserialize_forbidden.rs` ve
  `tests/ui/measurement_request_deserialize_forbidden.rs` `trybuild` fixture'ları
  Commit 4'te eklenmeli. Commit 3'te `Serialize`-only invariant manuel review ile korunur.

- **Commit 5 — `test(inv-t4): adversarial measurement-binding regressions`** (19 regression test)
- **Commit 6 — `docs(inv-t4): conformance + truth-surface`** (Conformance doc, #70 acceptance checklist, PR body sync)

### #72 — embedded attempt-evidence integrity — closure landed, scoped review pending
### #73 — witness Q3 honest-reject production wiring — PR #69 merge decision requires governance call

## Truth-surface (current head 650c620)

```
Current head: 650c620
osp-core lib tests: 1017 (951 → +66: Commit 3 +62, review v1 closure +4)
workspace tests (excl. osp-desktop): green
cargo check -p osp-desktop --lib: parent parity (2 #80-originated errors, yeni hata yok)
cargo clippy -p osp-core --lib: 12 warnings (parent `3b4231f` parity)

#70 Commit 3: landed (22e3d93) — subject-bound EngineMeasurement tokens
#70 Commit 3 review v5 closure: landed (650c620) — session fence + golden pin + producer parity
#70 Commit 1 + v6/v7 closure: landed (0d4eb51)
#70 Commit 2 + review v1/v2 closure: landed (eb9903b)
#72 implementation + 5 closures: landed (920a1dc), scoped review pending

Commit 4 carryover (P1 merge-blocker before authority migration):
  - BoundMeasurementSession (tek core-axis binding, descriptor drift fail-closed, ABA fixture)
Commit 4 carryover (P2):
  - trybuild compile-fail Deserialize guards

#70: Commit 4-6 pending
#73: Q3 wiring — PR #69 merge decision governance call required
eligible independent review: still required (GOVERNANCE §3 high-risk)
```

## Conformance evidence

Tam dokümantasyon: [`docs/paper2-notes/conformance/inv-t9-external-evidence-suspension.md`](docs/paper2-notes/conformance/inv-t9-external-evidence-suspension.md)

🤖 Generated with [ZCode](https://github.com/ervolkan/zai-coding-plan)
