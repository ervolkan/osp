# INV-T9 Step 4b — Claim-specific EffectiveVisionGateContext (Handoff/Plan)

**Tarih:** 2026-07-17
**Branch:** `fix/inv-t9-witness-suspension`
**PR:** #69 (https://github.com/ontologicalspace/osp/pull/69)
**Repo:** P:/Work/SoftwarePhysics
**Base:** `c149107` (Step 4a complete, reviewer-onaylı, CI ✅)

---

## Yeni Oturumda İlk Komut

```bash
cd P:/Work/SoftwarePhysics
git checkout fix/inv-t9-witness-suspension
git pull  # c149107 ile senkron
git log --oneline -5  # c149107 head olmalı
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 771 test geçmeli
```

Sonra bu belgedeki planı uygula (aşağıda).

---

## Bağlam

INV-T9 PR #69, incremental reviewable. Steps 1-4a done + reviewer-onaylı. Step 4b = claim-specific effective vision binding. Merge blockers: #70 (EngineMeasurement), #71 remainder (4b/4c/5/6), #72 (evidence snapshot, Step 6'da açılacak).

**Commit zinciri (current head `c149107`):**
```
c149107  tighten captured-context test (4a P2 closure)
a640b08  reuse captured rule context in basis digest (4a closure)
de3dc18  step 4a CI — with_default_rules Result callers
6760b60  bind rule evaluation order to context digest (step 4a)
a1d1dfa  step 3 closure — core-only measurement context invariant
a635cbe  step 3 — real measurement context via validated axis descriptors
7af010e  closure review — full space-digest immutability + v1 schema wording
19feaaf  close INV-T9 canonical decision-basis gaps
9e560ec  Commit 1 amend — canonical integrity + engine-owned Held/Rejected context
```

---

## Onaylanan Plan (3 review turu sonrası, reviewer-onaylı)

Step 4b: `EffectiveVisionGateContext` — claim-specific effective vision binding. Captured-context pattern (4a gibi): vision_context bir kez üretilir, Q5 + build_authorization_context + digest paylaşır.

### Reviewer'ın 4 P0'ı (planın temeli)

#### P0-1 — Role inference + vision selection TEK fonksiyon
`vision_for_claim` zaten role infer ediyor. **Tek `effective_vision_selection(claim)`** tüm sonucu üretir (subject + effective vector + source aynı karar ağacından). `vision_for_claim` bu fonksiyona refactor.
- Alan adı: `subject: CanonicalVisionSubject` (`inferred_role` DEĞİL — Global bir inferred role değildir).

#### P0-2 — role_overrides commit sınırı çelişkisi düzeltildi
4b'de digest'ten kaldırılacaklar:
- `config.theta_bound`
- `config.role_overrides` (bütün map)
- global vision_vector (5-axis)

4b sonunda `compute(config, vision_context, rule_context)` — config yalnız: min_approvers, quorum_threshold, milestone_interval, abstractness, merge_ratio_observable (4c kaldırır).

#### P0-3 — Digest authority validation da yapar
```rust
impl EffectiveVisionGateContext {
    pub(crate) fn validate_for_authorization(&self) -> Result<(), VisionContextError> {
        self.validate_structure()?;
        self.validate_authority_for_mutation()
    }
}
```
Hem Q5 öncesinde hem digest başında. None/GlobalDefault → Q5'e ulaşamaz, digest üretilemez.

#### P0-4 — Tüm mutation yüzeyleri + terminal error mapping
3 Q5 yüzeyi: commit_task_claim, commit(), check_all_gates(). `EngineCommitError::VisionContextInvalid(VisionContextError)` terminal (AuthorizationContextFailed gibi) — maneuver budget tüketmez, yeni LLM attempt başlatmaz, witness'a ulaşmaz. `gate_decision_from_engine_error` → `GateDecision::Unknown`.

### İki aşamalı validate (onaylı karar)

**`validate_structure()`** — imkânsız kombinasyonlar:
| Subject | Source | Sonuç |
|---------|--------|-------|
| Global | UserLoaded | Geçerli |
| Global | GlobalDefault | Yapısal geçerli (authority'de reject) |
| Global | BuiltinRole/RoleProfile | **Geçersiz** (SubjectSourceMismatch) |
| Role | BuiltinRole/RoleProfile/UserLoaded | Geçerli |
| Role | GlobalDefault | Yapısal geçerli (authority'de reject) |
| Herhangi | None | **Geçersiz** (VisionUnavailable) |

**`validate_authority_for_mutation()`:** None→VisionUnavailable, GlobalDefault→VisionAuthorityInsufficient, diğerleri Ok.

### GlobalDefault reject (onaylı karar)
GlobalDefault evaluable ama authorization-gated mutation yolunda **authority yetersiz**. validate bunu reddeder. Engine VisionConfig olmadan kurulursa fail-closed.

### 4b vision-focused (onaylı karar)
4b: vision_context + propagation + global vision/role_overrides/config theta_bound digest'ten kaldır. 4c: kalan config fields + son EngineConfig parametresi.

---

## Uygulama Adımları (reviewer'ın revize sırası)

### Adım 1 — `CanonicalVisionSourceTag` + explicit subject encoding
**Dosya:** `crates/osp-core/src/canonical_tags.rs`
- Makro ile (CanonicalMetricSourceTag pattern, canonical_tags.rs:190-198):
```rust
canonical_tag_newtype! {
    pub struct CanonicalVisionSourceTag;
    domain: crate::vision::VisionSource;
    None => 0, GlobalDefault => 1, BuiltinRole => 2, RoleProfile => 3, UserLoaded => 4,
}
```
- Import ekle: `use crate::vision::VisionSource;` (canonical_tags.rs:18-20 civarı)
- **Explicit subject encoding:** `subject kind: Global → 0, Role → 1`. Global: `[0]` (dummy role tag YOK). Role(r): `[1, canonical_role_tag]`.

### Adım 2 — `EffectiveVisionSelection` tek karar ağacı
**Dosya:** `crates/osp-core/src/engine.rs`
- `vision_for_claim` → `effective_vision_selection(claim)` refactor (cascade korunur)
- Role yalnız burada infer edilir (delta_nodes.first → infer_role("", classification, None))
- Subject + effective vector + source birlikte döner
- Alan adı `subject` (`inferred_role` değil)

### Adım 3 — `EffectiveVisionGateContext` + validate
**Dosya:** `crates/osp-core/src/authorization.rs`
- `CanonicalVisionSubject { Global, Role(CanonicalNodeRole) }`
- `EffectiveVisionSelection { effective_vision, vision_source, subject, role_inference_semver, vision_selection_semver }`
- `EffectiveVisionGateContext { selection, theta_bound, deviation_semver }`
- `try_new` + `validate_structure` + `validate_authority_for_mutation` + `validate_for_authorization`
- `VisionContextError` (typed): VisionUnavailable, VisionAuthorityInsufficient{source}, SubjectSourceMismatch{subject,source}, NonFiniteVisionAxis{axis}, NonFiniteThetaBound, ThetaBoundOutOfRange(f64), InvalidSemanticsVersion{field,version}
- Constants: `ROLE_INFERENCE_SEMANTICS_VERSION`, `VISION_SELECTION_SEMANTICS_VERSION`, `DEVIATION_SEMANTICS_VERSION` (hep 1)
- **theta_bound aralığı:** `MIN_THETA_BOUND: f64 = 0.0`, `MAX_THETA_BOUND` CosineDeviation contract'ından
- Hepsi `pub(crate)` (runtime context, wire schema değil)

### Adım 4 — 3 Q5 yüzeyi migration
**Dosya:** `crates/osp-core/src/engine.rs`
- `effective_vision_gate_context(claim)` + `validate_for_authorization` Q5'ten ÖNCE
- `check_claim_vision_with_context(claim, &vision_context)`
- `commit_task_claim`: vision_context bir kez capture → Q5 + build_authorization_context + digest paylaşır (4a rule_context pattern)
- `commit()` (legacy): vision_context capture → Q5 (digest üretmez)
- `check_all_gates()`: aynı context üretimi/validation → typed Q5 failure

### Adım 5 — `VisionContextInvalid` terminal mapping
- `EngineCommitError::VisionContextInvalid(VisionContextError)` yeni variant
- `gate_decision_from_engine_error` (navigator.rs:198): → `GateDecision::Unknown` (terminal)
- Tüm exhaustive match'ler güncellenir (HallucinationType::from_engine_error, vs.)

### Adım 6 — Digest transition
**Dosya:** `crates/osp-core/src/authorization.rs`
- `compute(config, vision_context, rule_context)` yeni imza
- `vision_context.validate_for_authorization()` + `rule_context.validate()` defensive başta
- **Kaldır:** global vision_vector 5-axis, config theta_bound, tüm role_overrides map
- **Ekle (canonical, sabit field sırası):** effective_vision {x,y,z,w,v}, vision_source_tag, subject kind, optional role tag, role_inference_semver, vision_selection_semver, theta_bound, deviation_semver
- `current_evaluation_context_digest` accessor tamamen kaldırılır (recompute yüzeyi açma)

### Adım 7 — Caller/source audit + testler
- GlobalDefault caller audit: non-mutating fixture → GlobalDefault kalabilir; mutation fixture → UserLoaded/BuiltinRole/RoleProfile
- `from_vision_config()` → UserLoaded test
- Navigator test'leri güncellenir (accessor kaldırıldı)

### Adım 8 — Workspace validation + commit + push
```bash
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib
RUSTFLAGS="-D warnings" cargo build --workspace --examples --exclude osp-desktop
RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop
cargo fmt -p osp-core -- --check
cargo clippy -p osp-core --lib   # baseline 11; 0 yeni hedef
```
Commit: `fix(authorization): bind claim-specific effective vision context (INV-T9 step 4b)`
Normal push (force yok), `c149107` korunur.

---

## Test Matrisi

```
// Validation
effective_vision_context_rejects_non_finite_vector()
effective_vision_context_rejects_non_finite_theta_bound()
effective_vision_context_rejects_theta_bound_out_of_range()
vision_source_none_fails_closed_before_q5()
vision_source_global_default_rejected_for_mutation_authority()
vision_source_role_profile_with_role_subject_accepted()
vision_source_user_loaded_with_global_subject_accepted()
vision_source_builtin_role_with_global_subject_rejected()  // SubjectSourceMismatch
empty_delta_nodes_falls_to_global_subject()

// Digest binding
evaluation_context_binds_claim_specific_effective_vision()
evaluation_context_changes_when_effective_theta_bound_changes()
evaluation_context_changes_when_only_vision_source_changes()
evaluation_context_changes_when_only_subject_changes()
selected_role_override_changes_claim_context()
unrelated_role_override_does_not_invalidate_claim_context()
global_default_does_not_create_evaluation_context_digest()
vision_source_none_does_not_create_evaluation_context_digest()

// Captured context propagation
q5_and_evaluation_context_reuse_the_same_theta_bound()  // referans paylaşımı

// Terminal behavior (P0-4)
global_default_authority_failure_does_not_retry_or_consume_budget()

// Caller audit (P1)
vision_config_produces_user_loaded_authoritative_source()
cosine_deviation_none_fallback_remains_defensive_only()
```

---

## Önemli Dosya Lokasyonları (doğrulanmış)

- **vision_for_claim:** engine.rs:953-988 (3-tier cascade)
- **check_claim_vision:** engine.rs:922-943
- **commit_task_claim:** engine.rs:518-636 (vision 0c:545, rule_context 560, build_auth 590)
- **build_authorization_context:** engine.rs:646+ (digest 795)
- **current_evaluation_context_digest:** engine.rs:1274 (kaldırılacak)
- **EvaluationContextDigest::compute:** authorization.rs:957 (güncel imza 4a sonrası)
- **VisionSource enum:** vision.rs:32-80 (None/GlobalDefault/BuiltinRole/RoleProfile/UserLoaded)
- **VisionVector:** vision.rs:96-156 (raw + source)
- **CosineDeviation.theta:** vision.rs:186-195 (None → 1.0 defensive)
- **infer_role:** space.rs:231-289 (sadece Support/Runtime üretir — path="", metrics=None)
- **NodeRole enum:** space.rs:201-218 (6 variant)
- **role_overrides:** engine.rs:50 (HashMap<String, RoleVisionOverride>)
- **builtin_role_override:** vision_config.rs:254-269
- **canonical_tag_newtype! macro:** canonical_tags.rs:29-106
- **CanonicalMetricSourceTag (template):** canonical_tags.rs:190-198
- **canonical_tags imports:** canonical_tags.rs:18-20 (MetricSource, NodeClassification, NodeRole, ComparisonOp, PredicateAxis)
- **gate_decision_from_engine_error:** navigator.rs:198-216 (exhaustive)
- **EngineCommitError:** engine.rs:203+

## Önemli Nüanslar

- **infer_role sınırlaması:** `vision_for_claim` `infer_role("", classification, None)` — sadece Support/Runtime. TypeSurface/Core/Adapter/Utility builtin override ölü kod (4b dışı). validate yine exhaustive (Global/Role tüm kombinasyonlar).
- **CosineDeviation None fallback:** ikinci savunma katmanı (validate birinci). Korumalı.
- **#70/#72 bağımsız** — 4b onların API'sine dokunmaz.
- **Digest değişiyor:** testler `from_bytes` placeholder kullanır (etkilenmez); `compute` kullanan testler vision_context imzasına güncellenir.

## Risk / GOVERNANCE

- PR high-risk, merge edilmez. Eligible independent review gerekli.
- Force-push yok (incremental, normal push).
- Her commit bağımsız derlenir + `-D warnings` test geçer.
