# INV-T9 #70 Commit 4b Faz 4 — Yeni Oturum Kickoff Mesajı

Aşağıdaki mesajı yeni oturumda bana paste et:

---

```
INV-T9 #70 Commit 4b Faz 4 (AuthorizationContextV2 — v1→v2 migration) implementation'a başlıyoruz.

Önce durum doğrula:

cd P:/Work/SoftwarePhysics
git fetch origin
git checkout wip/inv-t9-70-commit4b
git pull  # 9c02f98 head olmalı (Faz 3 closure)
git log --oneline -10
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 1102 test geçmeli
RUSTFLAGS="-D warnings" cargo test -p osp-core --test engine_measurement_single_producer  # 7 test
RUSTFLAGS="-D warnings" cargo test -p osp-core --test measurement_binding_typelevel  # 1 test

WIP branch: wip/inv-t9-70-commit4b (base: fix/inv-t9-witness-suspension @ baa90a8)
Draft PR #81 (review-only, non-mergeable): https://github.com/ontologicalspace/osp/pull/81

Faz 1+2+3 TAMAM (scoped review + reviewer v6→v9 turu kapandı, 1102 test green):
- Faz 1: tip tanımları (TaskValidationError, MeasurementBinding hata sistemi 10 derivation
  varyant, VerifiedMeasurementBinding, GateDecision v2 append-only tag'ler + v1 frozen encoder,
  EngineCommitError tek kapsayıcı mapping, CanonicalMeasurementRequestEvidence,
  TrajectoryEvidence baseline/loss enums, CanonicalTrajectoryEvidenceBaseline + try_from_reason,
  CanonicalTrajectoryLossEvidence)
- Faz 2: engine helper ayrımı (check_claim_structure / check_raw_position_finite /
  check_vision_raw_with_context — legacy backward-compat)
- Faz 3: standalone verify_measurement_binding primitive — drift-detected verification epoch
  (with_epoch all-path finalization), VerifiedTaskMeasurementBinding outer opaque proof
  (Clone YOK, cross-context substitution protection), TaskClaimDigest + MeasurementDigest
  (Serialize-only, stable canonical tag), MeasurementBindingDriftError 3 varyant,
  EngineCommitError MeasurementBindingVerification tek kapsayıcı, commit_task_claim
  validate_for_commit guard (tag 7, Q5 öncesi), single-producer AST guard (cfg_attr + modül-adı
  bypass kapandı, fail-closed, struct literal), TrajectoryLossEvidence Faz 5 contract notu,
  trybuild non-forgeability. combine_verification_results pure decision function.

Sonra Faz 4 planını oku: docs/inv-t9-70-commit4b-faz4-plan.md (v7 APPROVED — 7 turluk review)

Faz 4 kapsamı (AuthorizationContextV2 — v1→v2 migration, standalone):
- AuthorizationBasisV2 (canonical redesign — additive DEĞİL, duplicate field yok)
- 3 katman ayrımı: Basis (kanıtsal zemin) / GateEvaluation (snapshot+proof) / Context (proof-gated)
- VerifiedGateEvaluationV2 opaque proof (production build'de constructor YOK, Faz 5 evaluator)
- AuthorizationContextV2::new proof-gated (VerifiedGateEvaluationV2 tüketir, CanonicalGateEvaluationV2 reddeder)
- EngineMeasurementDigest (tam artifact: request+baseline+after+context — tek producer)
- TaskGoalDigest (task_id + predicate body + preferred_vector tek canonical temsil)
- CanonicalWitnessRequirementV2 (private repr, NotRequired/Required, TryFrom tek yol)
- VersionedAuthorizationBasis wire dispatch (legacy bare-v1 + strict versioned v1/v2, JSON-specific)
- Ayrı digest newtype (Basis/Context V2) + ayrı domain separator + canonical encoding (JSON DEĞİL) + hex wire
- measurement_context_digest + measurement_baseline_digest basis'te (reverify zinciri)
- build_authorization_context_v2 standalone builder (production wiring Faz 8)

Önemli notlar:
- WIP bazlı ilerleme — her alt-parça WIP commit + push (draft PR #81 incremental review)
- 4 WIP commit: 1a (types) → 1b (wire) → 2 (builder) → 3 (docs+closure)
- Atomiklik: final tüm fazlar bitince tek squashed commit → fix/inv-t9-witness-suspension
- reviewer scoped review için draft PR #81'den compare diff paylaş
- V1 frozen (serialization/digest/golden byte'ları HİÇ değişmez, type alias)
- Non-blocking notları: shared baseline encoder (tek fonksiyon), V1 digest golden pin,
  compile-fail testi opsiyonel (pub(crate) invariant yeterli)
- osp-desktop pre-existing breakage INV-T9 #80 (Faz 11 kapsamında, Faz 4'ten bağımsız)

Faz 4 ontolojik zincir:
  EngineMeasurement + Task/Claim
      ↓ verify_measurement_binding (Faz 3)
  VerifiedTaskMeasurementBinding (move-only proof)
      ↓ build_authorization_context_v2 (Faz 4 standalone)
  AuthorizationContextV2 (basis + verified gate eval + witness requirement)

Production wiring Faz 8'de atomik: verify_measurement_binding → build_authorization_context_v2 →
navigator/persistence V2 migration + omega receipt (AuthorizationReceiptV2).
```

---

## Yeni oturumda dikkat edilmesi gerekenler

### Doğrulama adımları
1. Branch head `9c02f98` olmalı (Faz 3 closure — reviewer v9 truth-surface final sync)
2. 1102 osp-core lib test green + 7 single-producer guard + 1 trybuild
3. Faz 3 plan doc (`docs/inv-t9-70-commit4b-faz3-plan.md`) Faz 3 closure'ı anlatıyor — referans

### Plan okuma
- `docs/inv-t9-70-commit4b-faz4-plan.md` — v7 APPROVED planın tamamı (13 mimari karar + implementation)
- Reviewer turları #1-7 her turda kapanan P0/P1/P2'ler doc'ta özetlendi

### Implementation sırası
1. **Commit 1a** (`wip(4b-faz4-types)`): V2 domain types + commitments + validators
   - EngineMeasurementDigest, TaskGoalDigest, AuthorizationBasisDigestV2, AuthorizationContextDigestV2
   - AuthorizationBasisV2 (validate_semantics + compute_digest)
   - CanonicalWitnessRequirementV2 (private repr), GateDispositionV2, CanonicalGateEvaluationV2, VerifiedGateEvaluationV2
   - AuthorizationContextV2 (proof-gated constructor)
   - Faz 3 extension: VerifiedTaskMeasurementBinding'e 3 yeni field
2. **Commit 1b** (`wip(4b-faz4-wire)`): VersionedAuthorizationBasis custom Serialize/Deserialize + legacy V1 strict
3. **Commit 2** (`wip(4b-faz4-builder)`): build_authorization_context_v2 standalone
4. **Commit 3** (`wip(4b-faz4-docs+closure)`): Faz 5/8 contract docs + workspace closure

### CI her commit
```bash
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo build -p osp-core --lib
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib
RUSTFLAGS="-D warnings" cargo test -p osp-core --test engine_measurement_single_producer
RUSTFLAGS="-D warnings" cargo test -p osp-core --test measurement_binding_typelevel
# Commit 3 closure: cargo check --workspace --all-targets
```

### Reviewer interaction
- Her WIP commit sonrası draft PR #81'e push
- Reviewer scoped review için compare diff paylaş (`9c02f98..HEAD`)
- Non-blocking notları implementasyon sırasında uygula (shared encoder, golden pin)
