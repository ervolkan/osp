# INV-T9 #70 Commit 3 Final Handoff — Subject-Bound EngineMeasurement Tokens

**Tarih:** 2026-07-21
**Branch:** `fix/inv-t9-witness-suspension`
**PR:** #69 (https://github.com/ontologicalspace/osp/pull/69)
**Current head:** `389e7db` (Commit 3 implementation + review v5/v6/v7 closure landed — reviewer v7 CONDITIONAL APPROVE 9.8/10)
**Commit 4 split:** 4a (BoundMeasurementSession, P1 merge-blocker — plan reviewer v10 APPROVED 9.9/10) + 4b (atomik authority migration). 4a plan: [`docs/inv-t9-70-commit4a-plan.md`](inv-t9-70-commit4a-plan.md)
**Issue:** #70 (https://github.com/ontologicalspace/osp/issues/70)
**Commit 3 review status:** Reviewer v6 REQUEST CHANGES 9.6/10 — **conditional approval** (P1 carryover to Commit 4)
**Commit 2 closure:** Reviewer APPROVED 10/10 — scoped tamamlandı

---

## Yeni oturumda ilk komut

```bash
cd P:/Work/SoftwarePhysics
git checkout fix/inv-t9-witness-suspension
git pull  # 389e7db head olmalı
git log --oneline -5
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 1017 test geçmeli
```

Sonra Commit 4 planını uygula (aşağıda).

---

## Commit 3 durumu (landed — reviewer v7 CONDITIONAL APPROVE 9.8/10)

**4 commit ile landed:**
- `22e3d93` — Commit 3: `feat(engine): subject-bound EngineMeasurement tokens`
- `650c620` — review v5 closure: `test(inv-t9): session fence + golden pin + producer parity`
- `0d73801` — review v6 closure: `docs(inv-t9): producer contract test + Commit 4 P1 merge-blocker`
- `389e7db` — review v7 closure: `docs(inv-t9): truth-surface head + producer contract hardening`

### Commit 3 kazanımları

- `canonical_encoding.rs` (private modül) — neutral BLAKE3 framing primitives
- `measurement.rs` (public) — `CanonicalSubjectScope`, `CanonicalImpactScope`, `MeasurementDeltaDigest`, `MeasurementRequest`, `MeasurementRequestDigest`, `MeasurementBaseline`, `EngineMeasurement` (private-field token), `MeasurementError`, `MeasurementDigestError`, `SubjectScopeResolutionError`
- `authorization.rs` — encoding primitives taşındı, `canonical_structural_delta_from_claim` shared producer, `encode_space_view_id` pub(crate) infallible, `canonicalize_node` CanonicalizationError döner, canonical encoder'lar pub(crate)
- `engine.rs` — `measure_task_delta`, `measure_current_scope` (KALDIRILDI), `derive_task_subject_scope`, `derive_impact_scope`, `measured_centroid_of`, `try_compute_raw_from_delta`
- 1017 osp-core lib tests (951 → +66)
- 4 existing golden test byte-for-byte unchanged (AuthorizationBasis, MeasurementInput, EvaluationContext, SuspendedAttemptEvidence)
- 2 yeni v1 golden: MeasurementDeltaDigest (`071b94001b33e714...`), MeasurementRequestDigest (`bcc98fc016a15062...`)

### Reviewer v1→v7 turu (Commit 3 implementation + closures: 8.9 → 9.3 → 9.6 → 9.7 → 9.2 → 9.6 → 9.8)

Commit 3 implementation + 3 closure commit ile landed. Reviewer v7 CONDITIONAL APPROVE 9.8/10 — "Commit 3 için tekrar kapsamlı review gerekmiyor".

### Commit 4a plan reviewer v6→v10 turu (8.9 → 9.3 → 9.6 → 9.7 → 9.6 → 9.9)

Commit 4a planı (BoundMeasurementSession P1 merge-blocker) reviewer v10 APPROVED 9.9/10 — implementation-ready. Plan: [`docs/inv-t9-70-commit4a-plan.md`](inv-t9-70-commit4a-plan.md).

24 P0/P1/P2 bulgusu kapatıldı. Tek açık konu: **measurement-session atomikliği (P1)**.

---

## ✅ Commit 4a P1 MERGE-BLOCKER — BoundMeasurementSession (plan reviewer v10 APPROVED 9.9/10)

**Reviewer v6 (9.6/10) carryover → Commit 4a planı:** Commit 3 context-before/context-after fence kalıcı descriptor değişikliğini yakalar ama **ABA senaryosunu yakalamaz**:

```text
context_before descriptor = A
node 1 ölçümünde descriptor/state = B (interior mutability)
node 2 ölçümünde descriptor/state = C
context_after descriptor = A
→ digest(A) == digest(A) → fence geçer
→ ama centroid farklı axis semantikleri altında üretilmiş olabilir
```

**Commit 4 split (kullanıcı kararı):** 4a (BoundMeasurementSession P1 merge-blocker) + 4b (atomik authority migration). 4a additive, authority caller'lara dokunmaz.

**Commit 4a plan reviewer v6→v10 turu (8.9 → 9.3 → 9.6 → 9.7 → 9.6 → 9.9):** Tam plan [`docs/inv-t9-70-commit4a-plan.md`](inv-t9-70-commit4a-plan.md). Kapanan P1/P2'ler:
- **v8 P1-1:** Her ölçümde pre/post descriptor verify
- **v8 P1-2:** Refs + descriptors aynı bind işleminden atomik
- **v8 P1-3:** `coords.rs` authorization tiplerinden bağımsız (neutral layer)
- **v8 P1-4:** `measured_centroid_in_session` + wrapper (try_compute_raw_from_delta compat)
- **v9 P1-1:** `AxisStateEpoch` (monoton) — gerçek transient ABA (descriptor equality A→B→A'yı kaçırdı)
- **v9 P1-2:** Delegasyon yönü — `bind_core_axis_refs` → `bind_core_axes_with_descriptors` → `bind_core_axes` (compat)
- **v10 P1-1:** `MeasurementSessionPhase` public visibility (private_interfaces warning)
- **v10 P1-2:** `AxisStateEpoch` external construct (pub const fn new/get, From<u64>, ZERO, Default)

### Commit 4a acceptance conditions (EngineMeasurement herhangi bir authority/evidence caller'a bağlanmadan önce — reviewer v10 APPROVED)

1. **Core axis referanslarını yalnız bir kez bind et** — `BoundMeasurementSession` + `BoundCoreAxes` (refs + states)
2. **Before ve after ölçümlerinin tamamı aynı bound referanslarla** — `measured_centroid_in_session`
3. **Session başlangıcındaki captured descriptors token'a bağlanmalı** — `MeasurementInputContext::try_new(session.axis_descriptors())`
4. **Her ölçümde pre/post + session-sonunda descriptor/epoch drift fail-closed** — `AxisStateDrift` (descriptor + monoton epoch)
5. **ABA fixture'ları token üretimini reddetmeli** — blocking test'ler:
   - `bound_measurement_session_rejects_persistent_descriptor_drift` (descriptor A→B, B kalır)
   - `bound_measurement_session_rejects_transient_aba_via_epoch` (descriptor A→B→A, epoch artar)
   - `session_begin_captures_each_descriptor_once` (descriptor call-count)
   - `external_axis_can_produce_nonzero_epoch` (AxisStateEpoch external construct)
6. **Bu koşullar sağlanmadan legacy measured input migration'ı yapılamaz** (Commit 4b).

### Doğal migration yüzeyi

Commit 4'te `CoordinateSystem` ölçüm yüzeyi yeniden düzenleneceği için `BoundMeasurementSession` bu aşamada doğal olarak uygulanır:
- `CoordinateSystem::begin_measurement_session() -> Result<BoundMeasurementSession, ...>`
- `BoundMeasurementSession::measured_position_of(node, space) -> Result<MeasuredRawPosition, ...>`
- `BoundMeasurementSession::verify_unchanged(&self) -> Result<(), MeasurementContextDrift>`

---

## Commit 4b P2 carryover — compile-fail Deserialize guards

**Reviewer v6/v7 P2-2:** Commit 3'te `EngineMeasurement` ve `MeasurementRequest` `Serialize`-only (Deserialize intentionally absent). Commit 3 test'leri yalnız `Serialize` trait bound'unu doğrular — `Deserialize` eklenirse bile geçer (manuel review ile korunur).

Commit 4b'de `trybuild` compile-fail fixture'ları eklenmeli (osp-core Cargo.toml:35 `trybuild = "1"` zaten mevcut):

```text
tests/compile_fail/engine_measurement_deserialize.rs + .stderr
tests/compile_fail/measurement_request_deserialize.rs + .stderr
```

Mevcut konvensiyon `tests/compile_fail/` (NOT `tests/ui/` — handoff doc'un eski önerisi yanlıştı). Orchestrator: `tests/anchoring_typelevel.rs` — `t.compile_fail(...)` çağrıları.

Beklenen compile error: `the trait Deserialize is not implemented for EngineMeasurement`.

---

## Commit 4b — Sözleşme (reviewer v4 plan APPROVED 9.7/10 + v6 carryover + Commit 4a sonrası)

**Commit 4 split:** 4a (BoundMeasurementSession P1 merge-blocker — plan v10 APPROVED 9.9/10) + 4b (atomik authority migration). 4b, 4a scoped approval sonrası başlar.

### Atomik migration (Commit 4b)

```text
TaskCommitInput { claim, omega, task_resolver, measurement }   ← Commit 4b
    ↓ measurement: EngineMeasurement
EngineMeasurement (private-field token)
    ↓ before: MeasuredRawPosition, after: MeasuredRawPosition,
    ↓ context: MeasurementInputContext, request: MeasurementRequest
    ↓ (revision artık request içinde — P2-1 v2 tek truth source)
measure_task_delta(TaskBoundClaim, expected_base_revision, hint) → EngineMeasurement
    ↓ BoundMeasurementSession (Commit 4a — P1 merge-blocker kapandı) ile tek core-axis binding
    ↓ uses CoordinateSystem::measured_position_of() (Commit 2)
    ↓ derive_task_subject_scope + derive_impact_scope
Subject/impact aggregate invariant
    ↓ subject_scope üyeleri only, partial → Unavailable
```

### Düzenleme yüzeyi

1. `TaskCommitInput { claim, omega, task_resolver, measurement: EngineMeasurement }` (subject_scope YOK — token'a taşındı)
2. `commit_task_claim` migration + `claim.computed_raw` ignore + Mixed validation
3. `AuthorizationBasis v2` (before+after single canonical + request digest + baseline/loss consistency)
4. `PredicateGateInput` → token baseline/after
5. `TrajectoryEvidenceBaseline` enum
6. Tüm caller migration atomik: Navigator (832), MCP (867), CLI (313), g2c (491/782/594/904), test construction site'ları
7. `provenanced_from_raw` production/evidence path'ten kaldır
8. `raw_position_of` + `position_of` + `Axis::compute()` `#[deprecated]`
9. Domain sep `osp.authorization-basis.v2\0`
10. `TaskValidationError::InvalidRequiredMetricSource` (typed commit-time guard)
11. AuthorizationBasis v2 golden + v1 strict-reject fixture
12. **BoundMeasurementSession** (P1 merge-blocker — yukarıda)
13. **trybuild compile-fail Deserialize guards** (P2 carryover — yukarıda)
14. Post-commit grep: `provenanced_from_raw(.*Scip` authority/evidence yolunda sonuç vermemeli

### Caller envanteri

**`commit_task_claim` / `TaskCommitInput` caller'ları:**
- Production: navigator.rs:845 (AgentNavigator::run_task), osp-mcp/server.rs:878
- Test: navigator.rs:1485/1570, engine.rs:2412/2427

**`provenanced_from_raw` caller'ları (Commit 4'te kaldırılacak):**
- Production: navigator.rs:169 (def) + 832, osp-mcp/server.rs:768/867, osp-cli/commands/mod.rs:313, g2c_corpus_matrix.rs:491/782/594/904
- Test: navigator.rs:1043/1162/1483/1549/1671/1726/3004, engine.rs:2379

**`compute_raw_from_delta` caller'ları (Commit 4 migration):**
- Production: navigator.rs:790, osp-mcp/server.rs:842, osp-desktop/lib.rs:347 (2-arg #80 hatası)
- Test: engine.rs:2131/2150/2168/2208/2244/2264, navigator.rs:1457/1752/1753/2103/2115, faz5_e2e.rs:191/239/322

---

## Açık issue'lar (Commit 4 sonrası takip)

- **#79** (PredicateAxis fallback) — Commit 4 kapsamı dışı.
- **#80** (osp-desktop #72-originated errors) — Commit 4 atomik migration'da ele alınacak.
- **Module scope resolution** — Commit 4b'de graph-aware `SubjectScopeResolver` trait.

---

*Bu belge INV-T9 #70 Commit 3 final handoff'tur. Commit 3 reviewer v7 CONDITIONAL APPROVE 9.8/10 (4 commit ile landed: 22e3d93 + 650c620 + 0d73801 + 389e7db). Commit 4a planı (BoundMeasurementSession P1 merge-blocker) reviewer v10 APPROVED 9.9/10 — implementation-ready. Commit 4b atomik authority migration Commit 4a scoped approval sonrası başlar.*

**Güncelleme (2026-07-21):** Head `389e7db`, Commit 4 split (4a + 4b) belgelendi. Commit 4a planı: [`docs/inv-t9-70-commit4a-plan.md`](inv-t9-70-commit4a-plan.md).
