# INV-T9 #70 Commit 4b Faz 3 — Engine Binding & Derivation Plan (v5 APPROVED)

**Tarih:** 2026-07-21 (ilk), 2026-07-22 (v5 closure sync)
**WIP branch:** `wip/inv-t9-70-commit4b` (head: `813a3e2`)
**Base:** `baa90a8` (Commit 4a APPROVED 9.9/10)
**Draft PR:** #81 (review-only, non-mergeable)
**Önceki:** Faz 1+2 TAMAM (scoped review #1→#4, 1067 osp-core test green)

---

## ⚠️ Truth-surface güncellemesi (reviewer v7 P1-3)

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

**Reviewer v2/v4/v6 kararları (tümü uygulandı):**

- `verify_measurement_binding` → `Result<VerifiedTaskMeasurementBinding, MeasurementBindingVerificationError>`
  (outer opaque proof — task/claim/measured-result kimliği taşır)
- `VerificationEpoch::with_epoch` — all-path drift-detected finalization (session başladıktan
  sonraki tüm yollar coord finalization + revision re-verify'dan geçer)
- `VerifiedTaskMeasurementBinding` — `Clone` YOK (cross-context substitution protection;
  same-context replay Faz 8 commit-ledger)
- Drift ≠ Derivation ayrımı — `MeasurementBindingDriftError` ayrı typed family
- `EngineMeasurement` origin invariant (Faz 1: Deserialize absent + `pub(crate)` new) +
  Faz 3 AST source-structure regression guard
- `commit_task_claim` task declaration guard Q5 öncesi (tag 7)

---

## Faz 3 implementation (3 WIP commit — `a3fdeb1..813a3e2`)

### Commit 1: `wip(4b-faz3-verifier)` (96ca02c)

**`verify_measurement_binding` standalone primitive:**

- **Encoder nötr primitif** (`canonical_encoding.rs`): `encode_axis_components` — auth tipi
  tanımaz, cycle kapalı. v1 `AuthorizationBasisDigest` byte contract korunur.
- **TaskClaimDigest + MeasurementDigest** (`measurement.rs`): `Serialize`-only (Deserialize
  absent). Stable canonical source tag, length-prefixed author (AgentId u64), normalized float.
- **Drift error ayrımı** (`measurement.rs`): `MeasurementBindingDriftError` (3 varyant:
  CoordinateContextChanged / SpaceRevisionChanged / BothChanged) +
  `MeasurementBindingDerivationError::RevisionRecheckFailed`.
- **EngineCommitError mapping** (reviewer v6 #1): tek kapsayıcı
  `MeasurementBindingVerification(#[from])` + üç alt `From`. Legacy varyantlar `#[from]` kaldırıldı.
- **VerificationEpoch** (`engine.rs`): `with_epoch` all-path finalization. Capture failure →
  Derivation; gözlenen değişim → Drift. Precedence: coord > revision recheck failed > revision
  changed > ordinary.
- **VerifiedTaskMeasurementBinding** (`engine.rs`): `Clone` YOK, `into_parts(self)` consuming
  projection (Faz 4 basis builder).
- **verify_measurement_binding** (`engine.rs`): 7 validation check (Task/Subject/Impact/
  StructuralDelta/Revision/ContextDigest/CurrentContext) + commitment derivation.
- **Single-producer AST guard** (`tests/engine_measurement_single_producer.rs`): syn source
  regression guard. **Reviewer v7 hardening:** exact cfg(test) ayrımı (substring false-positive
  kapandı), fail-closed scan (read/parse error → test fail), struct literal bypass detection
  (production `EngineMeasurement { ... }` count == 0).

### Commit 2: `wip(4b-faz3-guard)` (be14875)

**`commit_task_claim` declaration guard (tag 7):**

- `bound.task.validate_for_commit()?` — task bind sonrası, Q5 vision öncesi.
- `validate_for_commit` `#[allow(dead_code)]` kaldırıldı (artık wired).
- TaskBoundClaim semantic contract doc-comment: identity binding only, declaration validity ayrı.

### Commit 3: `wip(4b-faz3-contract)` (813a3e2)

**Faz 5 contract + non-forgeability + workspace closure:**

- TrajectoryLossEvidence Faz 5 completion-first contract notu (`trajectory.rs` doc-comment).
- `tests/measurement_binding_typelevel.rs` — trybuild harness. External crate
  VerifiedTaskMeasurementBinding tipini göremez (`pub(crate)`).
- Workspace closure: osp-core + downstream crates green (osp-desktop pre-existing breakage
  INV-T9 #80 — Faz 11 kapsamında).

---

## Test matrisi (reviewer v7 P1-1 closure)

**osp-core lib: 1067 → 1094 (+27 test) + integration harness'lar:**

- 1 pozitif (`succeeds_for_valid_token`)
- 6 mismatch check-order-aware (Task/Subject/Impact/StructuralDelta/Revision + no-state-mutation)
- SubjectDerivationFailed gerçek verify_measurement_binding çağrısı ile (module-scope predicate)
- 3 drift finalize_verification ile (CoordinateContextChanged / SpaceRevisionChanged / BothChanged)
- Derivation mapping pattern kanıtı (Subject test single-source-of-truth)
- ContextDigestMismatch / CurrentContextMismatch — constructor defensive verify yüzünden
  unreachable (doc-level assertion; Faz 12 test infra)
- ABA pinleme rename: `space_view_revision_changes_when_sequence_increments`
- 12 digest coverage (task_claim/measurement mutasyon + stable source tag + -0.0 + non-finite)
- 5 single-producer AST guard (production call + struct literal + exact cfg test + red-kanıt)
- 1 trybuild non-forgeability

---

## CI doğrulaması (yerel — remote GitHub CI görünmüyor)

```bash
cargo fmt --all -- --check  # clean
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 1094 passed
RUSTFLAGS="-D warnings" cargo test -p osp-core --test engine_measurement_single_producer  # 5 passed
RUSTFLAGS="-D warnings" cargo test -p osp-core --test measurement_binding_typelevel  # 1 passed
# Workspace: osp-mcp/osp-cli/osp-analyzer/osp-llm-runtime/osp-spike check green
# osp-desktop: PRE-EXISTING breakage (INV-T9 #80, Faz 11)
```

---

## Faz 3 closure iddiası (reviewer v6/v7 nihai metin)

Measurement-binding verifier, coordinate context'i ve monotonik space revision'ını
optimistic consistency validation altında gözlemleyen drift-detected verification epoch ile
oluşturuldu. Session başladıktan sonraki bütün yollar coordinate finalization'dan geçirildi;
revision baseline'ı başarıyla capture edilen yollar verification sonunda yeniden ölçülerek
karşılaştırıldı. Capture failure, derivation; gözlenen değişim, drift olarak modellendi. Mevcut
Faz-1 `VerifiedMeasurementBinding` modeli korunurken task, claim ve measured-result kimlikleri
clone edilemeyen outer opaque proof içinde bağlandı. Outer proof cross-context substitution'ı
engeller; same-context replay ve idempotency Faz 8 commit-ledger sorumluluğudur.
`EngineMeasurement` deserialize edilemez ve crate-private producer yüzeyi AST tabanlı
source-regression guard ile `measure_task_delta` call-site'ına pinlenmiştir. Commitment encoding
stable canonical tag'ler, length-prefixed author identity ve normalized float encoding kullanır.
Task declaration guard Q5 öncesine bağlanmış; ordering ve no-mutation yolları doğrulanmıştır.
Production enforcement Faz 8'e bırakılmıştır.

---

## Sonraki fazlar (Faz 3 sonrası — net sınırlar)

- **Faz 4:** `AuthorizationContextV2` + `build_authorization_context_v2` (outer proof
  `into_parts` consume) + custom Deserialize (untrusted → verify) + projection accessor'lar
- **Faz 5:** `TrajectoryLossEvidence::NotRequired` + completion-first PredicateGate refactor
  (atomik — producer + consumer + owned eşlenik + gate refactor + authorization wiring)
- **Faz 8:** Caller migration (navigator/MCP `measure_task_delta` infra) + `TaskCommitInput`
  smart constructor + `commit_task_claim` `verify_measurement_binding` production wiring +
  commit-ledger (idempotency/replay)
- **Faz 9:** General AST call-count suite
- **Faz 10:** trybuild type-suite genişletme
- **Faz 11:** osp-desktop fix (#80)
- **Faz 12:** Tests (tüm matrisler + EngineMeasurement test-only corrupt constructor ile
  tam mismatch/derivation/drift coverage)

---

*Bu belge INV-T9 #70 Commit 4b Faz 3 v5 APPROVED implementation planıdır. Reviewer turu #6
APPROVE sonrası, reviewer turu #7 REQUEST CHANGES (P1-3 truth-surface sync) ile güncellendi.
Gerçek implementation `a3fdeb1..813a3e2` aralığındadır.*
