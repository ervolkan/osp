# INV-T9 #70 Commit 4b Faz 4 — AuthorizationContextV2 Plan (v7 APPROVED)

**Tarih:** 2026-07-22 (plan 7 turluk review sonrası APPROVED)
**WIP branch:** `wip/inv-t9-70-commit4b` (PR head: GitHub PR #81 metadata authoritative)
**Base:** `baa90a8` (Commit 4a APPROVED 9.9/10)
**Draft PR:** #81 (review-only, non-mergeable)
**Önceki:** Faz 1+2+3 TAMAM (scoped review #1→#9, 1102 osp-core test green)

---

## Faz 4 sözleşmesi (v7 APPROVED — reviewer turu #7)

Faz 4, Commit 4b'nin authorization context v1→v2 migration'ıdır — `VerifiedTaskMeasurementBinding`
(Faz 3 standalone verifier output) consume eden **proof-gated standalone v2 builder** + ayrı V1/V2
domain struct'ları + tam commitment zinciri + strict wire dispatch.

**Ontolojik zincir (3 katman — reviewer v6/v7):**

```
EngineMeasurement + Task/Claim
    ↓ verify_measurement_binding (Faz 3)
VerifiedTaskMeasurementBinding (move-only proof)
    ↓ build_authorization_context_v2 (Faz 4 standalone)
AuthorizationContextV2 (basis + verified gate eval + witness requirement)
```

**Katman ayrımı (duplicate field YOK — reviewer v7 P1-1):**

- **Basis** (`AuthorizationBasisV2`) = kanıtsal zemin — identity + evidence + artifact commitments + delta/goal digests. Gate/witness YOK.
- **GateEvaluation** (`CanonicalGateEvaluationV2` persisted snapshot + `VerifiedGateEvaluationV2` opaque producer proof) — gate disposition + evaluation context. Faz 4 structural consistency only; Faz 5 gerçek evaluator producer.
- **Context** (`AuthorizationContextV2`) = basis + verified gate snapshot + canonical witness requirement — checked constructor proof-gated.

---

## Mimari kararlar (7 turluk review — hepsi APPROVED)

### 1. Standalone v2 builder, atomik Faz 8 production wiring
- Faz 4 builder **"implemented and verified, but not production-wired"**
- Production wiring Faz 8'de atomik: verify_measurement_binding → build_authorization_context_v2 → navigator/persistence V2 migration + omega receipt
- Faz 4 standalone builder test edilir, production path v1 korur

### 2. V1/V2 ayrı domain struct + versioned envelope dispatch
- `AuthorizationBasisV1` (type alias — mevcut `AuthorizationBasis`, Faz 1-3 frozen) + `AuthorizationBasisV2` (canonical redesign)
- Serialization boundary: `VersionedAuthorizationBasis::{V1, V2}` dispatch enum (wire only, domain enum DEĞİL)
- V1 serialization/digest/golden byte'ları HİÇ değişmez

### 3. V2 canonical redesign (additive DEĞİL)
- V2, V1'in superset'i DEĞİL — duplicate field yok
- `loss_before/after` → `CanonicalTrajectoryLossEvidence`
- `measurement_input_digest` → `CanonicalMeasurementRequestEvidence + MeasurementRequestDigest`
- `measured_result` → `measurement_digest + canonical evidence`
- Backward compat = V1'i okuyabilmek (V1 field'larını V2'ye kopyalamak DEĞİL)

### 4. Ayrı digest newtype + domain separator + canonical encoding
- `AuthorizationBasisDigestV1` (mevcut) + `AuthorizationBasisDigestV2` + `AuthorizationContextDigestV2` — ayrı newtype'lar (compile-time ayrım)
- Domain: `OSP/AUTHORIZATION-BASIS/V1`, `OSP/AUTHORIZATION-BASIS/V2`, `OSP/AUTHORIZATION-CONTEXT/V2`
- `EngineMeasurementDigest` (`OSP/ENGINE-MEASUREMENT/V1`), `TaskGoalDigest` (`OSP/TASK-GOAL/V1`), `MeasurementBaselineDigest`, `MeasurementContextDigest`
- Canonical encoding (JSON serialization DEĞİL — `serde_json::to_vec` YASAK) + hex wire format (64 lowercase)
- `DigestBytes` private; constructor `pub(crate)`, sadece `as_bytes()`/`to_hex()` public

### 5. apply_target field DEĞİL, witness_status context'te DEĞİL
- `apply_target` `mutation_decision`'dan deterministic türetilir (INV-T8): Reject→NotApplied, Progress→Checkpoint, Completed→Mainline, OperatorApproval→Sandbox
- `witness_status` context'te YOK — `AuthorizationReceiptV2`'ye (Faz 8) ait

### 6. VerifiedTaskMeasurementBinding Clone YOK (move-only)
- Cross-context substitution protection (same-context replay Faz 8 commit-ledger)

### 7. Proof-gated context constructor (reviewer v6 P0 → v7 closure)
- `AuthorizationContextV2::new(basis, gate_evaluation: VerifiedGateEvaluationV2, witness_requirement)` — **VerifiedGateEvaluationV2 tüketir**
- `CanonicalGateEvaluationV2` (persisted snapshot) → `new` reddedilir (compile error)
- `CanonicalGateEvaluationV2::try_from_parts` `pub(crate)` ama context constructor proof-gated → bypass imkânsız
- Invariant: "AuthorizationContextV2 yalnızca VerifiedGateEvaluationV2 tüketilerek doğabilir"

### 8. VerifiedGateEvaluationV2 opaque proof
- `pub(crate) struct VerifiedGateEvaluationV2 { canonical: CanonicalGateEvaluationV2 }` — field private
- Serialize/Deserialize/Clone YOK
- Production build'de constructor YOK — Faz 5 gerçek evaluator producer
- `into_canonical(self) -> CanonicalGateEvaluationV2` pub(crate)
- `#[cfg(test)] impl { pub(crate) fn fixture(canonical) -> Self }` — authorization.rs'te (field privacy)

### 9. EngineMeasurementDigest — tam artifact commitment (reviewer v5 P0)
- `MeasurementDigest` sadece `after`'ı bağlıyor — `before`/`request`/`context` açık DEĞİL
- Aynı request + same after + farklı before karışımı mümkün → `EngineMeasurementDigest` çözüm
- Preimage: `OSP/ENGINE-MEASUREMENT/V1 || request_digest || baseline_digest || MeasurementDigest(after) || context_digest`
- Tek producer: `EngineMeasurement::compute_digest()` + `EngineMeasurementDigest::compute_from_commitments()` shared canonical encoder
- Builder: `measurement.compute_digest() == binding.engine_measurement_digest()` kontrolü
- Basis'te: `measurement_context_digest` + `measurement_baseline_digest` (reverify zinciri)

### 10. TaskGoalDigest — task goal commitment (reviewer v4 P2-1)
- preferred_vector proof identity'ye bağlı — `task_claim_digest` preimage'ında yok
- Preimage: `OSP/TASK-GOAL/V1 || task_id || canonical predicate mode || canonical weighted predicates (preferred_vector HARIÇ) || preferred_vector option tag || preferred_vector canonical value`
- Tek canonical temsil (preferred_vector iki kez encode YOK — `PredicateSet` preferred_vector içeriyor)
- Verifier: task tek okuma → snapshot + digest (TOCTOU yok)
- Basis'te: `task_goal_digest` field

### 11. CanonicalWitnessRequirementV2 private repr (reviewer v4 P1-1)
- `pub struct CanonicalWitnessRequirementV2 { repr: CanonicalWitnessRequirementRepr }` — repr private
- `enum CanonicalWitnessRequirementRepr { NotRequired { reason }, Required { min_approvers, quorum_threshold, independence_policy } }`
- Tek creation yolu: `impl TryFrom<(&CanonicalWitnessPolicy, ApplyTarget)>` (direct construct edilemez)
- `WitnessNotRequiredReason::RejectedBeforeWitness` (Reject→NotApplied witness aşaması çalışmaz)
- Wire serde adı digest girdisi DEĞİL; pinned numeric tag (`WITNESS_REQUIREMENT_NOT_REQUIRED_V2: u8 = 0`, etc.)
- `validate_for(apply_target)` — lane/witness requirement uyumu

### 12. Wire migration güvenli (reviewer v5/v6)
- `schema_version` yok → legacy bare-V1 (mevcut permissive parser KORUR)
- `schema_version=1` → strict versioned V1 (`RawAuthorizationBasisV1Envelope` + `RawAuthorizationBasisV1Strict` nested deny_unknown_fields)
- `schema_version=2` → strict versioned V2 (`RawAuthorizationBasisV2Envelope` + `RawAuthorizationBasisV2` deny_unknown_fields) + validate_semantics + digest recompute + constant_time compare
- Bilinmeyen/malformed → reject (**V2→V1 fallback YOK**)
- V1/V2 envelope borrowing (clone yok) — `AuthorizationBasisV1Envelope<'a>` + `AuthorizationBasisV2Envelope<'a>`
- `parse_schema_version` strict numeric (`Value::Number(n) if n.is_u64()` → u16; 1.0/"2"/-1/null/u16 dışı reject)
- Typed constants: `AUTHORIZATION_BASIS_SCHEMA_V1/V2: u16`
- JSON-specific wire contract (doc pinli)

### 13. Canonical tag'ler pinli (reviewer v5 P2-2/v6)
- `PredicateCompletion`, `MutationDecision`, `GateDecision`, `WitnessIndependencePolicy`, witness requirement varyant/reason — pinned numeric tag (`canonical_tag_newtype!` macro)
- Enum ordinal/Debug/Serde adı DEĞİL
- apply_target digest'e YAZILMAZ (mutation_decision'dan türetilir)

---

## Faz 3 extension (engine.rs)

`VerifiedTaskMeasurementBinding`'e 3 yeni field:
- `task_goal_digest: TaskGoalDigest` — preferred_vector + predicate commitment
- `engine_measurement_digest: EngineMeasurementDigest` — tam artifact commitment
- `preferred_vector_snapshot: Option<RawPosition>` — trusted task'tan snapshot

`into_parts` güncellenir (5 tuple döner). Verifier sırasında trusted task'tan alınır, measurement request ile bağlanır.

---

## Faz 4 implementation (4 WIP commit)

### Commit 1a: `wip(4b-faz4-types)` — V2 domain types + commitments + validators

**Konum:** `authorization.rs` + `measurement.rs` + `engine.rs`

**Commitment tipleri:**
- `EngineMeasurementDigest` + `impl EngineMeasurement { compute_digest() }` + `compute_from_commitments` (shared canonical)
- `impl MeasurementBaseline { compute_digest() }` + `impl CanonicalTrajectoryEvidenceBaseline { compute_measurement_baseline_digest() }` (shared encoder — non-blocking notu)
- `TaskGoalDigest` + `compute(task)`
- `AuthorizationBasisDigestV2` + `AuthorizationContextDigestV2`
- `MeasurementContextDigest`, `MeasurementBaselineDigest`

**V2 basis:**
```rust
pub struct AuthorizationBasisV2 {
    task_id, claim_id,
    task_claim_digest, task_goal_digest,
    measurement_digest, engine_measurement_digest,
    trajectory_baseline, measurement_baseline_digest,  // reverify için
    trajectory_loss,
    measurement_request, measurement_request_digest,
    measurement_context_digest,
    canonical_delta_digest,
}
// pub(crate) fn new(...) -> Result<Self, AuthorizationBasisV2Error>
// fn validate_semantics: nested evidence + baseline digest reverify + engine_measurement_digest reverify
// pub fn compute_digest
```

**CanonicalWitnessRequirementV2 + GateSnapshot/Proof + AuthorizationContextV2:** (yukarıdaki kararlar #11, #7, #8)

**Errors:** `AuthorizationContextV2BuildError` (orchestration) / `AuthorizationContextV2Error` (context invariant) / `AuthorizationBasisV2Error` (basis + MeasurementBaselineDigestMismatch + EngineMeasurementDigestMismatch) / `EngineMeasurementDigestError` / `GateDispositionError`.

**Test matrisi:** digest deterministic + golden; V1/V2 ayrımı; validate_semantics (baseline + engine digest reverify); GateDispositionV2 structural matrisi; VerifiedGateEvaluationV2 production build'de construct edilemez + fixture cfg(test); AuthorizationContextV2::new CanonicalGateEvaluationV2 reddeder (compile-fail); TaskGoalDigest; EngineMeasurementDigest mutation matrisi (request/before/after/context); baseline evidence değiştir → MeasurementBaselineDigestMismatch/EngineMeasurementDigestMismatch; CanonicalWitnessRequirementV2 private repr + TryFrom + NotRequired/Required.

### Commit 1b: `wip(4b-faz4-wire)` — envelope + custom Serialize/Deserialize + legacy V1 strict

**Konum:** `authorization.rs`

- Typed constants, borrowing envelopes (V1 + V2)
- `VersionedAuthorizationBasis` custom Serialize (borrow, clone yok) + Deserialize (serde_json::Value peek + parse_schema_version strict)
- `RawAuthorizationBasisV2`, `RawAuthorizationBasisV1Strict`, `RawAuthorizationBasisV1Envelope`, `RawAuthorizationBasisV2Envelope` (deny_unknown_fields)
- Digest wire: 64 lowercase hex

**Test matrisi:** versioned v1 strict → V1; versioned v2 → V2 + integrity; legacy bare-v1 → V1; V1/V2 custom serialize → strict deserialize round-trip (clone yok); unknown version reject; 1.0/"2"/-1/null/u16 dışı reject; schema_version=2 + malformed → reject (fallback YOK); schema_version=2 + V1-shaped → reject; schema_version=1 + V2 nested → reject (strict); V2 tampering reject; digest hex validation; V1 digest wire mevcut golden ile aynı (non-blocking notu).

### Commit 2: `wip(4b-faz4-builder)` — build_authorization_context_v2 standalone

**Konum:** `engine.rs`

```rust
pub(crate) fn build_authorization_context_v2(
    &self,
    binding: VerifiedTaskMeasurementBinding,
    gate_evaluation: VerifiedGateEvaluationV2,
    witness_requirement: CanonicalWitnessRequirementV2,
    measurement: &EngineMeasurement,
) -> Result<AuthorizationContextV2, AuthorizationContextV2BuildError>
```

**Body:** `binding.into_parts()` → `measurement.compute_digest()` equality kontrolü → canonical evidence (baseline/loss/request) → basis (validate_semantics + compute_digest) → `AuthorizationContextV2::new(basis, gate_evaluation, witness_requirement)` (proof gate).

**Test matrisi:** pozitif; same request+after+farklı before → EngineMeasurementBindingMismatch; EngineMeasurementDigest mutation matrisi; binding consume; preferred_vector Some/None; deterministic digest; identity preservation; witness_requirement apply_target uyumsuz → reject; VerifiedGateEvaluationV2 fixture (cfg(test)).

### Commit 3: `wip(4b-faz4-docs+closure)` — Faz 5/8 contract + workspace closure

**Docs:** Faz 5 (gate evaluator → VerifiedGateEvaluationV2 gerçek producer + NotRequired loss); Faz 8 (production wiring + AuthorizationReceiptV2 + omega receipt); Clone semantiği; digest recompute; VersionedAuthorizationBasis JSON-specific; VerifiedGateEvaluationV2 non-test constructor yok; proof-gated context; baseline digest shared encoder invariant.

**Workspace closure:** `cargo check --workspace --all-targets` (osp-desktop pre-existing Faz 11).

---

## Non-blocking implementation notları (reviewer v7)

1. **Shared baseline encoder:** `MeasurementBaseline::compute_digest()` ve `CanonicalTrajectoryEvidenceBaseline::compute_measurement_baseline_digest()` aynı internal `write_measurement_baseline_commitment` fonksiyonunu çağırsın (drift risk kapalı). Test: Available/Unavailable raw digest == canonical evidence digest.

2. **V1 digest wire golden pin:** `AuthorizationBasisDigest::Serialize` mevcut hex string üretiyor — versioned V1 envelope digest representation ile aynı. Test: existing V1 digest wire == versioned V1 envelope digest (V1 istemeden V2 hex politikasına migrate edilmedi).

3. **Compile-fail testi (opsiyonel):** `pub(crate)` tipler için integration trybuild zor. `VerifiedGateEvaluationV2` field private + normal build'de constructor yok + context `VerifiedGateEvaluationV2` istiyor + test constructor cfg(test) → mimari invariant yeterli. Crate-içi UI fixture mümkünse ekle; değilse implementation blocker sayılmaz.

---

## CI doğrulaması (her commit)

```bash
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo build -p osp-core --lib
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 1102 → ~1195+
RUSTFLAGS="-D warnings" cargo test -p osp-core --test engine_measurement_single_producer
RUSTFLAGS="-D warnings" cargo test -p osp-core --test measurement_binding_typelevel
# Commit 3 closure: cargo check --workspace --all-targets
```

---

## Faz 4 closure iddiası (reviewer v7 APPROVE metni)

AuthorizationContextV2, verified measurement binding'den türetilen canonical redesign. Üç katman: Basis (kanıtsal zemin — identity + evidence + delta + goal + tam artifact digest + context digest + baseline digest), GateEvaluation (CanonicalGateEvaluationV2 persisted snapshot + VerifiedGateEvaluationV2 opaque proof — production build'de constructor YOK, Faz 5 evaluator), Context (checked constructor proof-gated — VerifiedGateEvaluationV2 tüketir, CanonicalGateEvaluationV2 reddeder). Builder tam EngineMeasurement artifact commitment doğrular (EngineMeasurementDigest tek producer — compute_from_commitments shared; request+baseline+after+context; MeasurementDigest sadece after YETMEZ). Basis validate_semantics engine_measurement_digest'i compute_from_commitments ile + baseline digest'i compute_measurement_baseline_digest (shared encoder) ile reverify eder. TaskGoalDigest (task_id + predicate body + preferred_vector tek canonical temsil). CanonicalWitnessRequirementV2 private repr (wire serde adı digest girdisi DEĞİL, pinned numeric tag). Witness requirement varyant/reason + PredicateCompletion canonical tag'leri pinli. witness_policy/omega pre-witness builder'da DEĞİL. preferred_vector trusted proof'ta. Duplicate field yok. Ayrı V1/V2 domain struct + versioned envelope custom Serialize/Deserialize (her branch borrow envelope — clone yok, serde_json::Value peek + strict dispatch + parse_schema_version numeric kontrol, digest zorunlu, JSON-specific) + RawAuthorizationBasisV1Strict (nested deny_unknown_fields) + AuthorizationBasisV1/V2Envelope (borrow) + typed schema constants + ayrı digest newtype (Basis/Context V2) + ayrı domain separator + canonical encoding (JSON DEĞİL) + hex wire format. apply_target field DEĞİL. witness_status context'te DEĞİL. Standalone — production wiring Faz 8. V1 frozen.

---

## Nihai faz sınırı

```
Faz 4
├─ canonical V2 basis
├─ tam measurement/task commitments
├─ proof-gated context constructor
├─ strict V1/V2 wire dispatch
├─ standalone builder
└─ production wiring yok

Faz 5
├─ gerçek gate evaluator
├─ VerifiedGateEvaluationV2 producer
└─ CanonicalTrajectoryLossEvidence::NotRequired

Faz 8
├─ verify measurement binding
├─ verified gate evaluation
├─ canonical witness requirement
├─ build AuthorizationContextV2
├─ witness evaluation + receipt
├─ navigator V2
└─ persistence write V2
```

---

## Kapsam dışı

- **Faz 5:** gate evaluator → VerifiedGateEvaluationV2 gerçek producer + CanonicalTrajectoryLossEvidence::NotRequired
- **Faz 8:** production wiring + AuthorizationReceiptV2 + navigator V2 + persistence V2 write + omega receipt
- **Faz 9:** V1 production path deprecate/remove
- **Faz 11:** osp-desktop fix (#80)
- **Faz 13:** format-agnostic Serde (JSON-specific wire şu an)

---

*Bu belge INV-T9 #70 Commit 4b Faz 4 v7 APPROVED implementation planıdır. Reviewer turları #1-7 (her tur mimari/type-safety açıklarını kapatarak ilerledi). Gerçek implementation Commit 1a→1b→2→3 sırasıyla uygulanacak.*
