# INV-T9 #70 Commit 4b Faz 3 — Engine Binding & Derivation Plan (v5 APPROVED)

**Tarih:** 2026-07-21 (ilk), 2026-07-22 (v9 closure sync)
**WIP branch:** `wip/inv-t9-70-commit4b` (PR head: GitHub PR #81 metadata authoritative)
**Implementation commits:** `96ca02c..c0bc206` (commit 1-5)
**Docs sync commit:** `25e7984` (truth-surface — implementation commit DEĞİL)
**Base:** `baa90a8` (Commit 4a APPROVED 9.9/10)
**Draft PR:** #81 (review-only, non-mergeable)
**Önceki:** Faz 1+2 TAMAM (scoped review #1→#4, 1067 osp-core test green)

---

## ⚠️ Truth-surface güncellemesi (reviewer v7/v8 P1-3)

**Bu belge v5 APPROVED planını yansıtır.** Önceki sürüm (v5 öncesi) şunları içeriyordu ama
**gerçekleşmedi** (Faz 3 scope'undan çıkarıldı, sonraki fazlara taşındı):

- ~~engine-owned target/loss derivation~~ → Faz 5 (completion-first PredicateGate refactor)
- ~~`commit_task_claim` measurement wiring~~ → Faz 8 (caller migration + smart constructor)
- ~~`build_authorization_context_v2`~~ → Faz 4 (AuthorizationContextV2 consumer)
- ~~`TaskCommitInput` içine `measurement` alanı~~ → Faz 8
- ~~`Result<VerifiedMeasurementBinding, ...>`~~ → **gerçek dönüş outer proof**
  `Result<VerifiedTaskMeasurementBinding, ...>`

**Gerçek Faz 3 kapsamı:** standalone `verify_measurement_binding` primitive + drift-detected
verification epoch + outer opaque proof + digest commitments + task declaration guard +
type/source regression evidence. Production commit-path enforcement Faz 8'e bırakıldı.

---

## Faz 3 sözleşmesi (v5 APPROVED — reviewer turu #6)

Faz 3, Commit 4b'nin binding primitive çekirdeğidir — presented `EngineMeasurement` token'ını
claim/task/subject/impact/delta/revision/context karşısında doğrulayan **standalone verifier**.

**Reviewer v2/v4/v6/v7/v8 kararları (tümü uygulandı):**

- `verify_measurement_binding` → `Result<VerifiedTaskMeasurementBinding, MeasurementBindingVerificationError>`
  (outer opaque proof — task/claim/measured-result kimliği taşır)
- `combine_verification_results` (reviewer v8) — pure decision function, finalize_verification
  içinden ayrıldı. RevisionRecheckFailed test edilebilir (mock engine gerekmeden).
- `VerificationEpoch::with_epoch` — all-path drift-detected finalization
- `VerifiedTaskMeasurementBinding` — `Clone` YOK (cross-context substitution protection)
- Drift ≠ Derivation ayrımı — `MeasurementBindingDriftError` ayrı typed family
- `EngineMeasurement` origin invariant (Faz 1: Deserialize absent + `pub(crate)` new) +
  Faz 3 AST source-structure regression guard (reviewer v8: cfg_attr + modül-adı bypass kapandı)
- `commit_task_claim` task declaration guard Q5 öncesi (tag 7)

---

## Faz 3 implementation (5 WIP commit)

### Commit 1: `wip(4b-faz3-verifier)` (96ca02c)

**`verify_measurement_binding` standalone primitive:**

- **Encoder nötr primitif** (`canonical_encoding.rs`): `encode_axis_components`
- **TaskClaimDigest + MeasurementDigest** (`measurement.rs`): `Serialize`-only. Stable
  canonical source tag, **fixed-width numeric author identity encoding** (`AgentId` as
  canonical little-endian `u64`), normalized float.
- **Drift error ayrımı** (`measurement.rs`): `MeasurementBindingDriftError` (3 varyant) +
  `MeasurementBindingDerivationError::RevisionRecheckFailed`.
- **EngineCommitError mapping** (reviewer v6 #1): tek kapsayıcı varyant.
- **VerificationEpoch** (`engine.rs`): `with_epoch` all-path finalization.
- **VerifiedTaskMeasurementBinding** (`engine.rs`): `Clone` YOK, `into_parts(self)`.
- **verify_measurement_binding** (`engine.rs`): 7 validation check + commitment derivation.
- **Single-producer AST guard** (`tests/engine_measurement_single_producer.rs`).

### Commit 2: `wip(4b-faz3-guard)` (be14875)

**`commit_task_claim` declaration guard (tag 7)** — `bound.task.validate_for_commit()?`.

### Commit 3: `wip(4b-faz3-contract)` (813a3e2)

Faz 5 contract docs + trybuild non-forgeability + workspace closure.

### Commit 4: `wip(4b-faz3-review-fix)` (ccdc55c)

Reviewer v7 REQUEST CHANGES closure: error taxonomy (MeasurementResultDigestComputationFailed),
real SubjectDerivationFailed test, drift combiner tests, AST fail-closed + struct literal.

### Commit 5: `wip(4b-faz3-review-fix-v2)` (c0bc206)

Reviewer v8 REQUEST CHANGES closure:
- **combine_verification_results** refactor — pure decision function (finalize_verification
  içinden ayrı). RevisionRecheckFailed test edilebilir.
- **4 no-op test kaldırıldı** — gerçek testlere dönüştürüldü veya Faz 12 carryover doc:
  - RevisionRecheckFailed → combine_verification_results synthetic Err (gerçek test)
  - CurrentContextMismatch → iki engine fixture farklı axis descriptor (gerçek test)
  - ContextDigestMismatch → `#[cfg(test)] corrupt_request_context_digest_for_test` fixture (gerçek test)
  - CurrentContextCaptureFailed → Faz 12 carryover (malformed coord_system)
  - verify_maps_derivation_failures → kaldırıldı (SubjectDerivationFailed pattern kanıtı yeterli)
- **AST guard cfg_attr bypass kapandı** — cfg_attr production kodunu dışlamaz.
- **AST guard modül-adı heuristic kaldırıldı** — sadece `#[cfg(test)]` syntax kesin.
- 2 yeni red-kanıt test (cfg_attr production + test-named module production).

---

## Test matrisi (reviewer v8 — semantik categori ayrımı)

**osp-core lib: 1067 → 1102 (+35 test)** + integration harness'lar:

**Positive verification: 1**
- `verify_measurement_binding_succeeds_for_valid_token`

**Mismatch variants exercised: 7/7**
- Task / Subject / Impact / StructuralDelta / Revision / ContextDigest / CurrentContext
- ContextDigestMismatch: `#[cfg(test)] corrupt_request_context_digest_for_test` fixture
- CurrentContextMismatch: iki engine fixture farklı axis descriptor (cohesion source)

**Derivation variants directly exercised: 2/10** (reviewer v9 — enum yüzeyi 10 varyant)
- `SubjectDerivationFailed` — gerçek verify_measurement_binding çağrısı (module-scope predicate)
- `RevisionRecheckFailed` — `combine_verification_results` synthetic `revision_after: Err`

**Deferred with missing fixture (Faz 12 carryover):**
- `ImpactDerivationFailed` — invalid edge kind fixture (measurement üretilemez — aynı helper
  measure_task_delta kullanır)
- `StructuralCanonicalizationFailed` — duplicate/cross-list node ID fixture
- `RevisionComputationFailed` — space digest computation failure (pratikte infallible)
- `CurrentContextCaptureFailed` — malformed coord_system (BoundMeasurementSession::begin Err)
- `ContextConstructionFailed` — MeasurementInputContext::try_new failure (reachable via
  invalid descriptors, fixture gerek)

**Reachable through corrupted measured result but not yet exercised (Faz 12 carryover):**
- `MeasurementResultDigestComputationFailed` — verify_measurement_binding_inner commitment
  derivation, NaN/∞ measured result (MeasuredRawPosition smart constructor yok — struct literal)

**Defensively fallible / currently unreachable:**
- `TaskClaimDigestComputationFailed` — verify_measurement_binding_inner, claim canonicalization
  (context-dependent; Faz 12)
- `RequestDigestComputationFailed` — defensively fallible, unreachable invariant (pratikte
  infallible — input already-canonical; reviewer P2-5)

**NOT:** `combine_verification_results` yalnız `RevisionRecheckFailed` üretir (`revision_after: Err`).
`MeasurementResultDigestComputationFailed` ve `TaskClaimDigestComputationFailed` bu fonksiyonun
çıktısı DEĞİL — `verify_measurement_binding_inner` commitment derivation'da üretilir.

**Drift variants exercised: 3/3**
- `CoordinateContextChanged` / `SpaceRevisionChanged` / `BothChanged`
- `combine_verification_results` ile + coord drift + revision recheck failed precedence

**State-integrity tests: 2**
- verify_measurement_binding_no_state_mutation_on_failure (selected state: t_c + node/edge count)
- invalid_task_declaration_no_state_mutation
- **Dürüst kapsam:** "selected state cardinality and t_c remained unchanged on tested failure
  paths" — full space equality / coordinate generation / event-audit state Faz 12 carryover.

**Digest tests: 12**
- task_claim_digest (claim_id/task_id/author mutasyon + stable)
- measurement_digest (axis value/source mutasyon + stable + -0.0 + non-finite)
- stable canonical source tags

**Architecture guard tests: 7**
- engine_measurement_new production call count == 1, enclosing fn == measure_task_delta
- struct literal bypass detection (production count == 0)
- exact cfg(test) ayrımı (cfg(not(test)) production; cfg(test) test)
- cfg_attr bypass kapandı (production exclude edilmez)
- test-named module bypass kapandı (modül adı heuristic yok)
- 5 red-kanıt test

**Compile-fail tests: 1**
- trybuild: external crate VerifiedTaskMeasurementBinding göremez (pub(crate))

**Deferred/no-fixture paths (Faz 12 carryover):**
- CurrentContextCaptureFailed (malformed coord_system fixture)
- ImpactDerivationFailed / StructuralCanonicalizationFailed / RevisionComputationFailed
  (invalid edge kind / duplicate node ID fixture — measurement üretilemez)
- full state-integrity (space equality / coordinate generation / event-audit)

---

## CI doğrulaması (yerel — remote GitHub CI görünmüyor)

```bash
cargo fmt --all -- --check  # clean
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 1102 passed
RUSTFLAGS="-D warnings" cargo test -p osp-core --test engine_measurement_single_producer  # 7 passed
RUSTFLAGS="-D warnings" cargo test -p osp-core --test measurement_binding_typelevel  # 1 passed
# Workspace: osp-mcp/osp-cli/osp-analyzer/osp-llm-runtime/osp-spike check green
# osp-desktop: PRE-EXISTING breakage (INV-T9 #80, Faz 11)
```

---

## Faz 3 closure iddiası (reviewer v8 dürüst kapsam)

Measurement-binding verifier, coordinate context'i ve monotonik space revision'ını
optimistic consistency validation altında gözlemleyen drift-detected verification epoch ile
oluşturuldu. Session başladıktan sonraki bütün yollar coordinate finalization'dan geçer;
revision ve context capture işlemleri başarıyla tamamlanıp epoch view oluşturulan yolların
revision değeri verification sonunda yeniden hesaplanarak karşılaştırılır (context construction
failure durumunda revision_before dışarı taşınmaz — proof üretimi engellenir; combine_verification_results
pure decision function ile test edilebilir). Capture failure, derivation; gözlenen değişim,
drift olarak modellendi. Mevcut Faz-1 `VerifiedMeasurementBinding` modeli korunurken task,
claim ve measured-result kimlikleri clone edilemeyen outer opaque proof içinde bağlandı. Outer
proof cross-context substitution'ı engeller; same-context replay ve idempotency Faz 8
commit-ledger sorumluluğudur. `EngineMeasurement` deserialize edilemez ve crate-private
producer yüzeyi AST tabanlı source-regression guard ile `measure_task_delta` call-site'ına
pinlenmiştir (cfg_attr + modül-adı bypass kapandı). Commitment encoding stable canonical tag'ler,
fixed-width numeric
author identity encoding (`AgentId` as canonical little-endian `u64`) ve normalized float
encoding kullanır. Task declaration guard Q5 öncesine bağlanmış; selected state cardinality
ve `t_c` test edilen failure path'lerde korunmuştur (full state-integrity Faz 12 carryover).
Production enforcement Faz 8'e bırakılmıştır.

---

## Sonraki fazlar (Faz 3 sonrası — net sınırlar)

- **Faz 4:** `AuthorizationContextV2` + `build_authorization_context_v2` (outer proof
  `into_parts` consume) + custom Deserialize (untrusted → verify)
- **Faz 5:** `TrajectoryLossEvidence::NotRequired` + completion-first PredicateGate refactor
- **Faz 8:** Caller migration + `TaskCommitInput` smart constructor + production wiring
- **Faz 9:** General AST call-count suite
- **Faz 10:** trybuild type-suite genişletme
- **Faz 11:** osp-desktop fix (#80)
- **Faz 12:** Tests (tam matrisler — CurrentContextCaptureFailed, Impact/Structural/Revision
  derivation, full state-integrity, EngineMeasurement corrupt fixture genişletme)

---

*Bu belge INV-T9 #70 Commit 4b Faz 3 v5 APPROVED implementation planıdır. Reviewer turları
#6 APPROVE, #7 REQUEST CHANGES, #8 REQUEST CHANGES ile güncellendi. Gerçek implementation
commit 1-5 (`96ca02c`..`c0bc206`) aralığındadır.*
