# INV-T9 Commit 1 Amend — Tur 2 Handoff / Yol Haritası

**Tarih:** 2026-07-16
**Branch:** `fix/inv-t9-witness-suspension`
**PR:** #69 (https://github.com/ontologicalspace/osp/pull/69)
**Repo:** P:/Work/SoftwarePhysics (local)

---

## Git Durumu (KRİTİK — önce bunu oku)

| Commit | GitHub'da mı? | İçerik |
|--------|---------------|--------|
| `9e560ec` | ✅ Push edildi, review'a açık | Tur 1 amend — canonical primitives + engine-owned AuthorizationContext |
| **Çalışma ağacı (uncommitted)** | ❌ Commit edilmedi, push EDİLMEDİ | Tur 2'nin Adım 1-2'si (authorization.rs, canonical_tags.rs, engine.rs) |

**Tur 1 amend (`9e560ec`) GitHub'da ve review için hazır.** Tur 2'nin yaptığım kısmı (Adım 1-2) yerel çalışma ağacında — commit edilmedi, gönderilmedi. Bu doğru: Tur 2 henüz tamamlanmadı (Adım 3-6 kalıyor), yarım commit push etmemeliyiz.

```bash
# Mevcut durum:
git status --short
#  M crates/osp-core/src/authorization.rs
#  M crates/osp-core/src/canonical_tags.rs
#  M crates/osp-core/src/engine.rs
# (316 insertions, 114 deletions — Adım 1-2)
```

---

## İlk Komut (yeni oturumda)

```bash
cd P:/Work/SoftwarePhysics
git checkout fix/inv-t9-witness-suspension
git pull  # 9e560ec ile senkron
git status --short  # 3 dosya modified görmeli (Adım 1-2)
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib authorization::
# 51 test geçmeli — Adım 1-2 derlenir + test geçer
```

---

## Bağlam: INV-T9 External-Evidence Suspension Isolation

OSP `ontologicalspace/osp` reposunda **INV-T9 conformance fix**. Paper 2 model-implementation gap'inin düzeltilmesi — witness quorum eksikliğinin agent-correctable failure olarak yanlış sınıflandırılması.

Bu PR GOVERNANCE §3 **high-risk** (witness/quorum safety + evidence integrity). Merge edilmez; eligible independent review bekler.

---

## Review Turları Geçmişi

### Tur 1 Review → REQUEST CHANGES (4 P0 + 2 P1)
- P0-1: Option\<f64\> encoding collision
- P0-2: Infinity rejection + predicate sort canonical bytes
- P0-3: PersistedSpaceViewId pseudo-random (BLAKE3+timestamp+pid)
- P0-4: Navigator placeholder basis + fail-open zero-digest fallback
- P1: Raw u8 tags + duplicate structural identity

**Tur 1 amend (`9e560ec`) — TAMAMLANDI, push edildi.** Çözümler:
- `encode_optional_f64` presence tag
- `!is_finite()` rejection (NaN + ±Infinity)
- Predicate sort canonical byte dizisi (length-prefix)
- Validated newtype'lar (`canonical_tags.rs` modülü, exhaustive TryFrom)
- `CanonicalStructuralDelta::try_new` (duplicate/cross-list/non-finite)
- `PersistedSpaceViewId::generate()` getrandom 0.4.3 CSPRNG, fail-closed
- `SpaceDigest::compute` + `EvaluationContextDigest::compute` gerçek yüzeyler
- `RuleDescriptor { rule_id, semantics_version, canonical_parameters }`
- Engine-owned `AuthorizationContext` (Held/Rejected, witness_requirement from omega)
- Navigator placeholder kaldırıldı
- `SuspensionDurability` capability (Ephemeral + CrossProcess fail-closed)

### Tur 1 amend Review → REQUEST CHANGES (5 P1 + 1 P2) — Tur 2 başlattı
- P1-1: Deserialize-time tag validation + source collision
- P1-2: PredicateEvaluationBasis gerçek girdiler
- P1-3: MeasurementInputDigest placeholder
- P1-4: Rule duplicate/order
- P1-5: Structural delta deserialize bypass + edge identity
- P2: EvaluationContextDigest gereksiz alanlar

### Tur 2 Plan Review → 4 yeni P0 eksiği
- **P0-1**: Per-axis measured provenance (yalnız coupling source, 5 ekseni bağlamalı)
- **P0-2**: Axis implementation/state identity (axis_names hash yetersiz)
- **P0-3**: Rule sırası semantiktir (first-match, order-independent DEĞİL)
- **P0-4**: Claim-specific effective vision (global vision bağlıyor, gerçek effective değil)

---

## Tur 2 — Tamamlanan Adımlar (Adım 1-2, uncommitted)

### Adım 1 — Validated canonical discriminants ✅

**Dosyalar:** authorization.rs, canonical_tags.rs, engine.rs

#### 1a. Tag newtype'ları deserialize'te reddet (P1-1)
`canonical_tags.rs` makrosuna `TryFrom<u8>` (range check via `VALID_TAGS`) + custom `Deserialize` eklendi. `CanonicalizationError::InvalidCanonicalTag { type_name, tag }`. Makro dışı `WitnessIndependencePolicyTag` için ayrı custom Deserialize. Artık diskten `255` gibi geçersiz tag deserialize edilemez.

#### 1b. EffectiveSourceRequirement enum (P1-1b, P0 collision)
```rust
pub enum EffectiveSourceRequirement {
    Any,                             // [0]
    Exact(CanonicalMetricSourceTag), // [1, src_tag]
}
```
`unwrap_or(0)` KALDIRILDI. `canonicalize_source_req` artık `Result` döner, fail-closed. Encoder: `push_effective_source` helper.

#### 1c. CanonicalPredicateScope enum (P1 — raw u8 scope_tag)
```rust
pub enum CanonicalPredicateScope {
    Node(u64),          // tag 0
    Module(String),     // tag 1
    Subgraph(Vec<u64>), // tag 2 (sorted)
}
```
`{ scope_tag: u8, identity_bytes: Vec<u8> }` yerine. `scope_tag()` + `identity_bytes()` metodları. `canonicalize_scope` (engine.rs) sadeleşti.

#### 1d. Per-axis measured provenance (P0-1 — bloklayıcı)
```rust
pub struct CanonicalAxisMeasurement {
    pub value: CanonicalF64,
    pub source: crate::canonical_tags::CanonicalMetricSourceTag,
}
pub struct ProvenancedMeasuredResult {
    pub coupling: CanonicalAxisMeasurement,
    pub cohesion: CanonicalAxisMeasurement,
    pub instability: CanonicalAxisMeasurement,
    pub entropy: CanonicalAxisMeasurement,
    pub witness_depth: CanonicalAxisMeasurement,
}
```
`metric_source: String` + `raw: RawPosition` KALDIRILDI. Engine 5 eksenin her birinin value+source'unu bağlar (`mk_axis` helper). `encode_axis_measurement` encoder helper.

### Adım 2 — PredicateEvaluationBasis gerçek girdiler (P1-2) ✅

```rust
pub struct PredicateEvaluationBasis {
    pub target_vector: CanonicalRawPosition,      // input.target (preferred_vector DEĞİL)
    pub loss_before: CanonicalF64,
    pub loss_after: CanonicalF64,
    pub failure_policy: PredicateFailurePolicyTag,
    pub min_improvement_delta: CanonicalF64,       // gerçek is_improved_loss girdisi
    pub allow_progress_checkpoint: bool,
    pub improvement_policy: EffectiveImprovementPolicy,
}

pub struct EffectiveImprovementPolicy {
    pub max_coupling: CanonicalF64,       // 0.85 (mevcut sabit)
    pub max_instability: CanonicalF64,    // 0.85
    pub min_cohesion: CanonicalF64,       // 0.15
    pub semantics_version: u32,
}

pub const IMPROVEMENT_SEMANTICS_VERSION: u32 = 1;
```

Engine.rs `build_authorization_context`: `target_vector = input.target` (preferred_vector kaldırıldı), `min_improvement_delta = task.policy.min_improvement_delta`, `tolerance` kaldırıldı, `improvement_policy: EffectiveImprovementPolicy::current_semantics()`.

**Doğrulama:** 51 authorization testi geçti. Workspace derlenir.

---

## Tur 2 — Kalan Adımlar (Adım 3-6)

### Adım 3 — Real measurement context (P1-3 + P0-2) ⏳

**Bu en kapsamlı adım. Axis trait + 5 axis + CoordinateSystem refactor.**

#### 3a. AxisDescriptor + Axis::descriptor() trait method
**Dosya:** coords.rs (Axis trait) + axes.rs (5 axis impl)

```rust
// coords.rs'de tanımla (authorization.rs import eder)
pub struct AxisDescriptor {
    pub axis_id: String,
    pub semantics_version: u32,
    pub canonical_parameters: Vec<u8>,
}

pub trait Axis: Send + Sync {
    fn name(&self) -> &'static str;
    fn descriptor(&self) -> AxisDescriptor;  // YENİ
    fn compute(&self, node: &Node, space: &Space) -> f64;
}
```

`canonical_parameters` her axis için gerçek ölçüm-affecting state:

| Axis | axis_id | canonical_parameters içeriği |
|------|---------|-------------------------------|
| `CouplingAxis` | "coupling" | formula `deg/(1+deg)` marker (parametresiz, boş veya semver) |
| `CohesionAxis` | "cohesion" | LCOM4 threshold + fallback strategy |
| `InstabilityAxis` | "instability" | Martin formula marker |
| `EntropyAxis` | "entropy" | **normalization denominator 13.0** + **configured value (h/13.0 clamped)** |
| `WitnessDepthAxis` | "witness_depth" | **normalization formula `raw/(1+raw)`** + **configured value** |

**Axis yapıları (axes.rs):**
- `CouplingAxis` (axes.rs:31) — parametresiz, per-node `deg/(1+deg)` (axes.rs:47)
- `EntropyAxis` (axes.rs:64) — `value: f64` private, `from_commit_entropy(h)` → `(h/13.0).clamp(0,1)` (axes.rs:69-73), `value()` accessor (axes.rs:76)
- `WitnessDepthAxis` (axes.rs:103) — `value: f64` private, `from_witness(ratio, distinct)` → `raw= ratio*ln(1+distinct)`, `raw/(1+raw)` (axes.rs:108-113)
- `CohesionAxis` (axes.rs:184) — `node.cohesion` + fallback
- `InstabilityAxis` (axes.rs:146) — `out/(out+in)`

#### 3b. CoordinateSystem::canonical_measurement_context
**Dosya:** coords.rs

```rust
impl CoordinateSystem {
    pub fn canonical_measurement_context(&self) -> Result<MeasurementInputContext, CanonicalizationError>;
}
```

- Axis descriptor'larından state üretir — **"herhangi bir node üzerinden w/v oku" YOK** (reviewer'ın kınaması).
- `config_tag` = axis descriptor set'inden (sorted by axis_id, hash).
- `repo_level_entropy/witness_depth` = EntropyAxis/WitnessDepthAxis descriptor'larının canonical_parameters'ından (gerçek configured value).
- Duplicate axis_id reddi (seçenek A — axis ID benzersiz).
- **MeasurementInputContext'ten kaldır:** `normalization_tag`, `measurement_adapters_version`, `metric_source_config` (core axes provenance taşımıyor — axis source artık Adım 1d'de per-axis).

**CoordinateSystem yapısı (coords.rs:202):**
```rust
pub struct CoordinateSystem {
    pub axes: Vec<Box<dyn Axis>>,
}
// Metodlar: empty(), dim(), axis_names(), position_of(), raw_position_of(), with_axis()
```

#### 3c. Duplicate axis policy
`CoordinateSystem::with_axis` duplicate axis_id reddeder. Seçenek A: axis ID benzersiz, descriptor'lar ID'ye göre sort.

**engine.rs güncellemesi:** `build_authorization_context` placeholder MeasurementInputContext kaldır, `self.coord_system.canonical_measurement_context()` çağır.

### Adım 4 — Evaluation semantics (P0-3 + P0-4 + P2) ⏳

#### 4a. Rule sequence binding (P0-3)
**Dosya:** engine.rs + authorization.rs

**Teyif edildi:** `check_claim_rules` (engine.rs:918-932) **first-match short-circuit**. Kayıt sırası semantiktir — hangi violation/calibration/reason geri döndüğünü belirler.

**Çözüm — sequence semantiğini koru:**
```rust
// EvaluationContextDigest::compute içinde:
for (ordinal, descriptor) in rules.iter().enumerate() {
    encode_u64(&mut hasher, ordinal as u64, "rule_ordinal");
    encode_rule_descriptor(&mut hasher, descriptor);
}
```
Mevcut sort-by-rule_id KALDIRILDI. `register_rule` → `Result<(), RuleRegistrationError>`, duplicate **active rule_id** reddeder (rule_id benzersiz; semantics_version zaman içi sürüm, aynı engine'de iki version değil).

**register_rule (engine.rs:309-311):** şu an sadece `self.rules.push(rule)`.

#### 4b. Claim-specific effective vision (P0-4 — bloklayıcı)
**Dosya:** engine.rs + authorization.rs

**Teyif edildi:** `vision_for_claim` (engine.rs:877-912) 3-tier:
1. User role override (`role_overrides.get(format!("{:?}", role))`) → `VisionSource::RoleProfile`
2. Builtin role override (`VisionConfig::builtin_role_override(role)`) → `VisionSource::BuiltinRole`
3. Engine global vision (`self.vision`)

Role çıkarımı: `claim.delta_nodes.first()` + `infer_role("", classification, None)` (classification-only).

```rust
pub struct CanonicalVisionEvaluationContext {
    pub effective_vision: CanonicalRawPosition,
    pub vision_source: CanonicalVisionSourceTag, // GlobalDefault/BuiltinRole/RoleProfile
    pub selected_role: CanonicalNodeRole,
    pub role_inference_semantics_version: u32,
    pub vision_selection_semantics_version: u32,
}
```

Accessor claim-specific:
```rust
pub fn evaluation_context_digest_for_claim(
    &self,
    claim: &Claim,
) -> Result<EvaluationContextDigest, CanonicalizationError>;
```
Engine `build_authorization_context` zaten claim'i parametre alıyor — `vision_for_claim(claim)` çağırıp gerçek effective vision'ı bağlar. Sadece ilgili role override; ilgisiz role override stale yapmaz.

**VisionSource enum (vision.rs:32-51):** None/GlobalDefault/BuiltinRole/RoleProfile/UserLoaded.

#### 4c. EvaluationContextDigest temizliği (P2)
**Dosya:** authorization.rs (`EvaluationContextDigest::compute`)

**Teyit edilen katman ayrımı:**
- Q5/Q6 evaluation kararını etkiler → EvaluationContextDigest: `theta_bound`, **effective_vision**, rule descriptors (ordinal ile). (Q4 syntax validation claim/structural içerikten çalışır; ayrı canonical context bağlanmaz.)
- Raw measurement üretimini etkiler → MeasurementInputDigest: axis descriptors (formül/config/normalizasyon).
- Witness → CanonicalWitnessPolicy (omega'da).
- Persistence cadence → hiçbir digest'e girmez.
- Post-apply derived position → `abstractness` (Martin D metric, `compute_derived`'da engine.rs:1176) — ayrı bir `ApplySemanticsDigest` gerekirse gelecekte.

**Kaldır:** `merge_ratio_observable` (HİÇBİR hesaplamada kullanılmıyor — digest filler), `min_approvers`/`quorum_threshold` (omega'da — authorization'a ait ama evaluation context'e değil), `milestone_interval` (persistence cadence), `abstractness` (digest'ten çıkarılır — Q5/Q6 authorization evaluation'ı etkilemiyor; `MeasurementInputDigest`'e taşınmaz çünkü axis tanımı değil, raw-axis measurement üretmez; post-apply derived-position etkisi gelecekte `ApplySemanticsDigest` bağlayabilir), tüm `role_overrides` (claim-specific effective vision ile değiştirildi, 4b).

### Adım 5 — Defensive structural integrity (P1-5) ⏳

**Dosya:** authorization.rs

#### 5a. Private fields + custom Deserialize
```rust
pub struct CanonicalStructuralDelta {
    new_nodes: Vec<CanonicalNode>,              // private
    new_edges: Vec<CanonicalEdge>,              // private
    removed_edges: Vec<CanonicalEdgeIdentity>,  // CanonicalEdgeIdentity (from,to,kind) — is_type_only HARİÇ
}
```
- Public constructor kaldırılır, `try_new` tek giriş.
- Custom Deserialize `try_new` üzerinden geçer.
- `validate()` defensive — `AuthorizationBasisDigest::compute` + `PendingAuthorizationEnvelope::verify` başında çağrılır.
- `removed_edges` artık `CanonicalEdgeIdentity` (from,to,kind) — removal identity'sinde is_type_only yok (reviewer önerisi; runtime remove `from+to+kind` üzerinden).

#### 5b. Edge identity düzeltimi
```rust
struct CanonicalEdgeIdentity { from: NodeId, to: NodeId, kind: CanonicalEdgeKind }
```
Cross-list/duplicate kontrolü bu identity üzerinden. `is_type_only` authorization içeriğinde kalır ama identity kontrolünde değil. Mevcut `CanonicalEdge` PartialEq `is_type_only` dahil — bu yüzden `(1,2,Imports,true)` add ile `(1,2,Imports,false)` remove conflict olarak yakalanmıyordu.

### Adım 6 — Validation + Commit ⏳

```bash
cd P:/Work/SoftwarePhysics
RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop
cargo fmt -p osp-core -p osp-mcp --check
```
`9e560ec` amend edilir (`git commit --amend`), force-push. INV-T9 spec hala `implementation in progress`.

**Commit mesajı (logical change) — Tur 2 eki:**
```
fix(authorization): INV-T9 Commit 1 amend tur 2 — validated discriminants + exact decision basis

Validated canonical discriminants (reviewer P1-1, P0-1):
- Tag newtype custom Deserialize + TryFrom<u8> (rejects invalid tags like 255)
- EffectiveSourceRequirement enum (Any/Exact) — None/TreeSitter collision fix
- CanonicalPredicateScope enum (Node/Module/Subgraph) — raw u8 scope_tag fix
- Per-axis measured provenance (CanonicalAxisMeasurement, 5-axis ProvenancedMeasuredResult)

Exact decision basis (reviewer P1-2):
- PredicateEvaluationBasis uses input.target (not preferred_vector)
- min_improvement_delta captured (real is_improved_loss input)
- EffectiveImprovementPolicy explicit thresholds (0.85/0.15) + IMPROVEMENT_SEMANTICS_VERSION
- tolerance (max_axis_regression mislabeled) removed

[Adım 3-6 tamamlandığında eklenir: AxisDescriptor, CoordinateSystem::canonical_measurement_context,
rule sequence binding, claim-specific effective vision, EvaluationContextDigest cleanup,
CanonicalStructuralDelta private + CanonicalEdgeIdentity]
```

---

## Test Matrisi (Adım 3-6 için)

### Adım 3
```
duplicate_axis_id_is_rejected()
measurement_digest_changes_when_axis_semantics_version_changes()
measurement_digest_changes_when_axis_parameters_change()
same_axis_name_with_different_implementation_changes_digest()
measurement_input_digest_reflects_real_coordinate_system()
```

### Adım 4
```
evaluation_context_changes_when_rule_registration_order_changes()  # order-dependent!
duplicate_active_rule_id_is_rejected()
same_rule_id_different_version_changes_context()
evaluation_context_binds_claim_specific_effective_vision()
evaluation_context_changes_when_builtin_role_profile_changes()
evaluation_context_changes_when_selected_vision_source_changes()
unrelated_role_override_does_not_invalidate_claim_context()
evaluation_context_digest_excludes_merge_ratio_observable()
```

### Adım 5
```
structural_delta_custom_deserialize_runs_validation()
cross_list_edge_conflict_ignores_is_type_only()
basis_digest_compute_validates_structural_delta()
removed_edge_uses_identity_without_is_type_only()
```

### Adım 1-2 (yeni testler — eklenecek)
```
tag_deserialization_rejects_invalid_node_kind_tag()
tag_deserialization_rejects_invalid_edge_kind_tag()
effective_source_requirement_any_vs_exact_distinct()
canonical_scope_deserialization_rejects_invalid_variant()
measured_result_binds_all_axis_sources()
authorization_basis_changes_when_entropy_source_changes()
authorization_basis_changes_when_witness_depth_source_changes()
authorization_basis_changes_when_min_improvement_delta_changes()
authorization_basis_records_the_exact_predicate_gate_target()
preferred_vector_does_not_replace_actual_gate_target()
improvement_semantics_version_golden_test_threshold_change_requires_version_bump()
```

---

## Önemli Dosya Lokasyonları

- **authorization.rs:** `crates/osp-core/src/authorization.rs` — tüm INV-T9 canonical tipler, digest, store
- **canonical_tags.rs:** `crates/osp-core/src/canonical_tags.rs` — validated tag newtype'lar (makro + WitnessIndependencePolicyTag)
- **engine.rs:** `crates/osp-core/src/engine.rs` — `commit_task_claim` (~438-519), `build_authorization_context` (~577+), accessor'lar (~924+), `register_rule` (309), `vision_for_claim` (877), `check_claim_rules` (918)
- **navigator.rs:** `crates/osp-core/src/navigator.rs` — `suspend_for_witness`, Held/Rejected arm'ları
- **coords.rs:** `crates/osp-core/src/coords.rs` — `Axis` trait (192), `CoordinateSystem` (202), `raw_position_of` (232)
- **axes.rs:** `crates/osp-core/src/axes.rs` — 5 axis (CouplingAxis:31, EntropyAxis:64, WitnessDepthAxis:103, InstabilityAxis:146, CohesionAxis:184)
- **rule.rs:** `crates/osp-core/src/rule.rs` — `Rule` trait (29), `RuleDescriptor` (auth.rs), `default_rules` (203)
- **trajectory.rs:** `crates/osp-core/src/trajectory.rs` — `PredicateGate` (979), `is_improved_loss` (1034), `TaskPolicy` (474)
- **vision.rs:** `crates/osp-core/src/vision.rs` — `VisionSource` enum (32), `VisionVector` (97)
- **vision_config.rs:** `crates/osp-core/src/vision_config.rs` — `builtin_role_override` (254), `role_overrides`

## Doğrulama

```bash
cd P:/Work/SoftwarePhysics
RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop
cargo fmt -p osp-core -p osp-mcp --check
cargo clippy -p osp-core --lib -- -D warnings  # pre-existing hatalar var, kendi kodun temiz olmalı
```

## Risk / GOVERNANCE

- Bu PR GOVERNANCE §3 high-risk. Merge edilmez; eligible independent review bekler.
- Force-push reviewer onaylı (amend doğası gereği).
- `getrandom 0.4.3` transitive grafide zaten var (Cargo.lock sabit).
- Rule/Vision accessor signature değişiklikleri (claim parametresi) — engine.rs iç çağrılar güncellenmeli.
- `merge_ratio_observable` digest'ten çıkınca conformance fixture digest'leri değişir — kabul edilebilir (schema bilinçli daralma).
- Mevcut clippy/fmt hataları pre-existing (repo zaten temiz değil) — kendi eklenen kod temiz olmalı.

## Adım 3'e Başlarken

1. `git status` ile Adım 1-2 değişikliklerinin durduğunu teyit et (3 dosya modified).
2. `coords.rs` Axis trait'ini oku (line 192). `AxisDescriptor`'ı orada tanımla.
3. `axes.rs` 5 axis'in her birine `descriptor()` impl ekle (canonical_parameters = gerçek state).
4. `CoordinateSystem::canonical_measurement_context` yaz (coords.rs).
5. `MeasurementInputContext`'ten gereksiz alanları kaldır (authorization.rs).
6. engine.rs placeholder MeasurementInputContext kaldır, yeni metodu çağır.
7. Derle + test et → Adım 4'e geç.
