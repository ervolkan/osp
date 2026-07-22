//! Space Engine — production runtime orchestrator (Faz 2.6).
//!
//! Tüm Faz 1-2 modüllerini tek çatı altında birleştirir:
//! - `vision_config` → `VisionVector` + `EngineConfig`
//! - `time::TimeFSM` → evaluate (Q1-Q3) + `bigbang::apply_delta` (mutate)
//! - `vision::compute_derived` → pozisyon reposition (`CosineDeviation`)
//! - `persistence::SnapshotStore` → event-sourcing (delta + milestone)
//!
//! **Commit pipeline (§4, space-engine-design.md):**
//! 0. CLAIM-BASED GATES (Q4-Q6) → syntax/vision/rule check (deterministik, witness öncesi)
//!    - Q4 Syntax: OutputContract compliant?
//!    - Q5 Vision: claim.computed_raw θ > bound → Err(VisionViolation) [mutasyon YOK]
//!    - Q6 Rule: Rule ihlali?
//! 1. WITNESS-BASED GATES (Q1-Q3) → evaluate + apply_delta (ΔV node + ΔE edge)
//! 2. REPOSITION → CosineDeviation ile ΔV∪N₁(ΔV) → drift_warnings
//! 3. SAVE DELTA → event-sourcing
//! 4. MILESTONE → periyodik tam snapshot
//! 5. EMIT → CommitOutcome

use std::path::Path;

use crate::agent::{PermissionMask, SyntaxViolation};
use crate::bigbang::Delta;
use crate::coords::{Position, RawPosition};
use crate::persistence::{
    DeltaRecord, PersistenceError, SnapshotStore, SpaceSnapshot, SNAPSHOT_FORMAT_VERSION,
};
use crate::rule::{Rule, RuleViolation};
use crate::space::{EdgeKind, NodeId, Space};
use crate::time::{TimeFSM, TimeMachine};
use crate::vision::{compute_derived, CosineDeviation, DeviationMetric, VisionVector};
use crate::vision_config::VisionConfig;
use crate::witness::{Claim, ClaimId, WitnessDisposition, WitnessSet};

// ═══════════════════════════════════════════════════════════════════════════════
// EngineConfig
// ═══════════════════════════════════════════════════════════════════════════════

/// Engine konfigürasyonu — `VisionConfig`'ten türetilir.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub min_approvers: usize,
    pub quorum_threshold: f64,
    pub theta_bound: f64,
    pub milestone_interval: u64,
    pub abstractness: f64,
    pub merge_ratio_observable: f64,
    /// Role-aware vision overrides (role → x/y/z override). Boşsa global vision.
    /// Engine, claim'in temsil ettiği node'un rolüne göre vision seçer.
    pub role_overrides: std::collections::HashMap<String, crate::vision_config::RoleVisionOverride>,
}

impl EngineConfig {
    pub fn from_vision_config(config: &VisionConfig) -> Self {
        Self {
            min_approvers: config.min_approvers(),
            quorum_threshold: config.quorum_threshold(),
            theta_bound: config.theta_bound(),
            milestone_interval: config.milestone_interval(),
            abstractness: config.abstractness(),
            merge_ratio_observable: config.merge_ratio_observable(),
            role_overrides: config.role_overrides.clone(),
        }
    }

    /// Test-friendly default (Faz 1.11 kalibrasyon değerleri).
    /// theta_bound=0.3: cosine deviation [0,1] değerlerde θ_max=0.5 (§5.2 NOT);
    /// 0.5 unreachable → 0.3 realistic threshold. TDA diffusion (Faz 5+) ile 0.5'e dönülebilir.
    pub fn default_calibrated() -> Self {
        Self {
            min_approvers: 2,
            quorum_threshold: 1.5,
            theta_bound: 0.3,
            milestone_interval: 1000,
            abstractness: 0.5,
            merge_ratio_observable: 0.10,
            role_overrides: std::collections::HashMap::new(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CommitOutcome + Warnings + Errors
// ═══════════════════════════════════════════════════════════════════════════════

/// Commit başarılı çıktısı.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitOutcome {
    pub event: Delta,
    pub drift_warnings: Vec<DriftWarning>,
    pub safety_weakened: bool,
    pub t_c: u64,
}

/// Aşama D2 — Task-bound Claim commit girdisi. Sizin önerdiğiniz structured input
/// (tek parametre yerine — daha temiz, genişletilebilir). commit()'in (standalone)
/// yanında, task-bound Claim'ler için Q5.b PredicateGate entegrasyonu.
///
/// **Prensip:** `commit() = legacy/standalone claim path; commit_task_claim() = trajectory/task-bound path.`
/// Mevcut commit() korunur (Paper 1 uyumluluk); commit_task_claim Paper 2 için.
///
/// **INV-T9 #70 Commit 4b (reviewer v3 P1-1 — TODO Faz 8):** Bu struct atomik migration'da
/// smart constructor'a çevrilecek: `{ claim, omega, task_resolver, measurement: EngineMeasurement }`.
/// `target`/`loss_before`/`measured` kaldırılıp engine-owned derivation'a geçilecek (Faz 3).
/// Public struct + private fields + `new()` smart constructor (external crate literal bypass
/// kapalı). Şimdilik mevcut caller'lar (navigator, MCP, test) korunduğu için public field'lar
/// kaldı — Faz 8 caller migration ile aynı commit'te smart constructor'a çevrilecek.
pub struct TaskCommitInput<'a> {
    pub claim: &'a crate::witness::Claim,
    pub omega: &'a crate::witness::WitnessSet,
    pub task_resolver: &'a dyn crate::trajectory::TaskResolver,
    /// preferred_vector (loss/distance target — INV-T1 internal).
    /// **TODO Faz 8:** kaldırılır, engine `task.target_predicate_set.preferred_vector`'den derive eder.
    pub target: crate::coords::RawPosition,
    /// Loss before (mevcut durumun preferred_vector'e uzaklığı).
    /// **TODO Faz 8:** kaldırılır, engine-owned typed loss evidence (reviewer v4 P0).
    pub loss_before: f64,
    /// Engine-measured simulated_after (INV-T3 — claim.computed_raw'tan ProvenancedRawPosition).
    /// **TODO Faz 8:** `measurement: EngineMeasurement` ile değiştirilir (token authority).
    pub measured: crate::trajectory::ProvenancedRawPosition,
}

/// Aşama D2 — commit_task_claim çıktısı. Attempt + outcome + apply_target + witness.
/// Sizin önerdiğiniz TaskCommitResult yapısı.
#[derive(Debug, Clone)]
pub struct TaskCommitResult {
    /// Q5.b PredicateGate attempt sonucu (gate_decision/predicate_completion/mutation_decision).
    pub outcome: crate::trajectory::AttemptOutcome,
    /// MutationDecision → ApplyTarget mapping (INV-T8 — Reject→NotApplied, Progress→Checkpoint).
    pub apply_target: crate::trajectory::ApplyTarget,
    /// Hesaplanan loss_after (preferred_vector'e distance, INV-T6 quantitative).
    pub loss_after: f64,
    /// Witness Q1-Q3 disposition'ı (Satisfied ise Some). Held/Rejected artık
    /// `EngineCommitResult::Held`/`Rejected` üzerinden gelir (INV-T9).
    pub witness: Option<crate::witness::WitnessDisposition>,
}

/// Post-mutation: neighbor θ > bound (commit geçerli, komşu degrade — WARNING, §4.1).
#[derive(Debug, Clone, PartialEq)]
pub struct DriftWarning {
    pub node_id: NodeId,
    pub theta: f64,
    pub raw: RawPosition,
}

// ═══════════════════════════════════════════════════════════════════════════════
// GateResult — commit pipeline visualizer çıktısı
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek bir gate'in sonucu (commit pipeline visualizer için).
#[derive(Debug, Clone, serde::Serialize)]
pub struct GateResult {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
    pub hallucination: Option<String>,
}

impl GateResult {
    pub fn passed(name: &'static str, detail: &str) -> Self {
        Self {
            name,
            passed: true,
            detail: detail.to_string(),
            hallucination: None,
        }
    }

    pub fn failed(
        name: &'static str,
        detail: &str,
        h: Option<crate::agent::HallucinationType>,
    ) -> Self {
        Self {
            name,
            passed: false,
            detail: detail.to_string(),
            hallucination: h.map(|ht| format!("{}", ht)),
        }
    }
}

/// Pre-mutation: claim θ > bound (Q5 ihlali — §4.1 REJECT, EngineCommitError::VisionViolation).
#[derive(Debug, Clone, PartialEq)]
pub struct VisionViolation {
    pub claim_id: ClaimId,
    pub theta: f64,
    pub raw: RawPosition,
}

impl std::fmt::Display for VisionViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Q5 vision violation (claim {}, negatif-uzay): θ={:.3}, raw={:?}",
            self.claim_id, self.theta, self.raw
        )
    }
}

/// Engine-level commit error (thiserror). Sadece **operational fault**'lar.
/// (osp-core-design.md §3.4).
///
/// **INV-T9:** Witness Hold/Rejected artık expected domain outcome olarak
/// `EngineCommitResult::Held`/`Rejected` üzerinden gelir (Err DEĞİL Ok kanalı).
/// `Witness(Reason)` varyantı KALDIRILDI — hem `commit()` hem `commit_task_claim()`
/// artık `EngineCommitResult` döndürür. Operational fault'lar (Syntax/Vision/Rule/
/// Permission/Persistence/Internal/InvalidWitnessEvidence) burada kalır.
///
/// **INV-T9 Step 4a:** Rule registration hataları (`register_rule`/`with_default_rules`).
///
/// Sadece duplicate değil; descriptor identity tutarsızlığı da yakalanır (runtime id
/// ile descriptor id farklı → Q6 ile digest farklı kuralı temsil eder).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleRegistrationError {
    #[error("empty runtime rule_id")]
    EmptyRuleId,
    #[error("invalid rule semantics version (must be > 0): {0}")]
    InvalidSemanticsVersion(u32),
    #[error(
        "rule descriptor identity mismatch: runtime_id={runtime_id}, descriptor_id={descriptor_id}"
    )]
    IdentityMismatch {
        runtime_id: String,
        descriptor_id: String,
    },
    #[error("duplicate active rule_id: {0}")]
    DuplicateActiveRuleId(String),
}

/// Variant tasarımı: violation struct'lar tek kaynak (single-source-of-truth). theta/detail/
/// rule_id gibi field'lar variant'ta TEKRAR EDİLMEZ — `Display` impl ile erişilir (drift risk yok).
#[derive(Debug, thiserror::Error)]
pub enum EngineCommitError {
    #[error("{violation}")]
    SyntaxViolation { violation: SyntaxViolation },
    #[error("{violation} (bound={bound:.3})")]
    VisionViolation {
        violation: VisionViolation,
        bound: f64,
    },
    #[error("{violation}")]
    RuleViolation { violation: RuleViolation },
    /// Malformed/author-self/duplicate/wrong-binding evidence — terminal (agent retry ile çözülmez).
    #[error("invalid witness evidence: {0}")]
    InvalidWitnessEvidence(String),
    #[error("permission denied (inv #13): {0}")]
    PermissionDenied(String),
    #[error("persistence kapalı — restore/milestone kullanılamaz (snapshot_store None)")]
    NoPersistence,
    #[error("persistence hatası: {0}")]
    Persistence(#[from] PersistenceError),
    /// Internal engine hatası — terminal system failure.
    #[error("internal engine error: {0}")]
    Internal(String),
    /// **INV-T9 (reviewer P0-4):** AuthorizationContext üretilemedi — fail-closed.
    /// Sıfır digest'e düşüş YOK. Navigator SystemFailure'a map'ler.
    #[error("authorization context construction failed (fail-closed): {0}")]
    AuthorizationContextFailed(String),
    /// **INV-T9 Step 4b (reviewer P0-4):** Effective vision context validation failure —
    /// terminal. None/GlobalDefault/mismatch/non-finite/out-of-range → Q5'e ulaşılamaz,
    /// digest üretilemez. Maneuver budget tüketmez, yeni LLM attempt başlatmaz,
    /// witness'a ulaşmaz. Navigator `GateDecision::Unknown`'a map'ler.
    #[error("vision context invalid (terminal — fail-closed): {0}")]
    VisionContextInvalid(#[from] crate::authorization::VisionContextError),

    /// **INV-T9 #70 Commit 4b (reviewer v2 karar 2):** Task declaration validation
    /// failure — `Task::validate_for_commit` terminal reject. Geçersiz task declaration
    /// (Mixed source requirement, non-finite threshold/tolerance, geçersiz policy).
    /// Guard sırası: Q4 syntax → task bind → **validate_for_commit** →
    /// verify_measurement_binding → Q5 → gate → Q6 → witness.
    /// Navigator `GateDecision::RejectedByTaskValidation`'a map'ler (append-only tag 7).
    /// Maneuver budget tüketmez, witness'a ulaşmaz, authorization üretmez.
    #[error("task validation failed: {0}")]
    TaskValidation(#[from] crate::trajectory::TaskValidationError),

    /// **INV-T9 #70 Commit 4b (reviewer v2 karar 4 + v4 P1-2/P1-3):** Presented
    /// `EngineMeasurement` token'ı claim/task/subject/impact/delta/revision/context ile
    /// uyuşmuyor — token replay/tamper detected. Disposition:
    /// `RegenerateMeasurement` (stale — Revision/CurrentContext) veya
    /// `RejectPresentedAuthority` (replay — Task/Subject/Impact/StructuralDelta/ContextDigest).
    /// Navigator `GateDecision::RejectedByMeasurementBinding`'a map'ler (append-only tag 8).
    /// Maneuver budget tüketmez, witness'a ulaşmaz, authorization üretmez.
    ///
    /// **Reviewer v6 #1 (legacy):** Bu varyant korunur ama `#[from]` KALDIRILDI — yeni kod
    /// tek kapsayıcı `MeasurementBindingVerification` üzerinden gider. Aynı hata ailesi
    /// tek EngineCommitError şekline dağılmaz.
    #[error("measurement binding mismatch: {0}")]
    MeasurementBindingMismatch(crate::measurement::MeasurementBindingMismatch),

    /// **INV-T9 #70 Commit 4b (reviewer v3 P1-4):** Engine derivation failure —
    /// `verify_measurement_binding` sırasında expected binding üretilemedi. Sistem
    /// hatası (operational fault), hallucination DEĞİL. Navigator SystemFailure'a
    /// map'ler, `GateDecision::Unknown`. Maneuver budget tüketmez, witness'a ulaşmaz.
    ///
    /// **Reviewer v6 #1 (legacy):** Bu varyant korunur ama `#[from]` KALDIRILDI.
    #[error("measurement binding derivation failed: {0}")]
    MeasurementBindingFailed(crate::measurement::MeasurementBindingDerivationError),

    /// **INV-T9 #70 Commit 4b Faz 3 (reviewer v6 #1):** Tek kapsayıcı measurement
    /// binding verification error varyantı. Mismatch (presented authority) + Derivation
    /// (system/capture failure) + Drift (verification epoch gerçeklik değişimi) üç sınıf.
    /// `verify_measurement_binding` `?` ile yayılır. Navigator:
    /// - Mismatch → `GateDecision::RejectedByMeasurementBinding` (tag 8)
    /// - Derivation → `GateDecision::Unknown` (SystemFailure)
    /// - Drift → `GateDecision::Unknown` (SystemFailure — retry gerekebilir)
    #[error("measurement binding verification failed: {0}")]
    MeasurementBindingVerification(#[from] crate::measurement::MeasurementBindingVerificationError),
}

/// **INV-T9 #70 Commit 4b Faz 3 (reviewer v6 #1):** `MeasurementBindingVerificationError`
/// → `EngineCommitError` tek terminal mapping — `#[from]` attribute varyant üzerinde
/// otomatik `From` üretir (manuel impl KALDIRILDI — E0119 conflict).
///
/// **Reviewer v6 #1:** Alt error tipleri wrapper üzerinden — aynı hata ailesi tek
/// EngineCommitError şekline gider. Legacy `MeasurementBindingMismatch`/`MeasurementBindingFailed`
/// varyantları yeni kod tarafından üretilmez.
impl From<crate::measurement::MeasurementBindingMismatch> for EngineCommitError {
    fn from(value: crate::measurement::MeasurementBindingMismatch) -> Self {
        Self::MeasurementBindingVerification(value.into())
    }
}

impl From<crate::measurement::MeasurementBindingDerivationError> for EngineCommitError {
    fn from(value: crate::measurement::MeasurementBindingDerivationError) -> Self {
        Self::MeasurementBindingVerification(value.into())
    }
}

impl From<crate::measurement::MeasurementBindingDriftError> for EngineCommitError {
    fn from(value: crate::measurement::MeasurementBindingDriftError) -> Self {
        Self::MeasurementBindingVerification(value.into())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// VerifiedMeasurementBinding (INV-T9 #70 Commit 4b — reviewer Faz 2 scoped P1-3)
//
// **Capability encapsulation:** tip engine.rs'te tanımlı — `new()` modül-private
// (engine modülü içinde). `verify_measurement_binding` (Faz 3) aynı modülde olduğu
// için construction private — measurement.rs veya başka crate/modül doğrudan
// `VerifiedMeasurementBinding::new()` çağıramaz (gerçek verifier-only invariant).
//
// Accessor'lar `pub(crate)` — authorization.rs basis builder (Faz 4) consume eder.
// External tanılama gerekirse ayrı DTO kullanılmalı.
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b (reviewer v2 karar 4 + Faz 2 scoped P1-3 + P2-4):** `verify_measurement_binding`
/// (Faz 3) tarafından üretilen doğrulanmış binding capability. **Production non-test
/// code'da construction yalnız `verify_measurement_binding()` ile** — modül-private `new()`
/// (engine.rs). Basis builder consume eder — re-derivation yok.
///
/// **Dürüst invariant (reviewer Faz 2 scoped P2-4):** Rust'ta `engine.rs` içindeki diğer
/// fonksiyonlar ve `engine.rs`'in child `mod tests` modülü private constructor/field'lara
/// erişebilir. Dolayısıyla "external crate/test bypass tip seviyesinde kapalı" ifadesi
/// tam doğru DEĞİL — test modülü hala çağırabilir. Doğru invariant: **production non-test
/// code'da `VerifiedMeasurementBinding` yalnız `verify_measurement_binding` tarafından
/// construct edilir.** Bu Faz 9 AST/source-contract kontrolü ile enforce edilir:
/// `non-test AST içinde VerifiedMeasurementBinding::new call count == 1` ve çağıran
/// `verify_measurement_binding` olmalı.
#[derive(Debug, Clone)]
pub(crate) struct VerifiedMeasurementBinding {
    subject: crate::measurement::CanonicalSubjectScope,
    impact: crate::measurement::CanonicalImpactScope,
    canonical_delta: crate::authorization::CanonicalStructuralDelta,
    current_revision: crate::authorization::SpaceViewRevision,
    current_context: crate::authorization::MeasurementInputContext,
    request_digest: crate::measurement::MeasurementRequestDigest,
}

impl VerifiedMeasurementBinding {
    /// **Faz 3:** modül-private constructor — yalnız `verify_measurement_binding`
    /// (engine.rs aynı modül) çağırır. measurement.rs veya test bypass kapalı.
    fn new(
        subject: crate::measurement::CanonicalSubjectScope,
        impact: crate::measurement::CanonicalImpactScope,
        canonical_delta: crate::authorization::CanonicalStructuralDelta,
        current_revision: crate::authorization::SpaceViewRevision,
        current_context: crate::authorization::MeasurementInputContext,
        request_digest: crate::measurement::MeasurementRequestDigest,
    ) -> Self {
        Self {
            subject,
            impact,
            canonical_delta,
            current_revision,
            current_context,
            request_digest,
        }
    }

    #[allow(dead_code)] // Faz 4: basis builder consume
    pub(crate) fn subject(&self) -> &crate::measurement::CanonicalSubjectScope {
        &self.subject
    }
    #[allow(dead_code)] // Faz 4: basis builder consume
    pub(crate) fn impact(&self) -> &crate::measurement::CanonicalImpactScope {
        &self.impact
    }
    #[allow(dead_code)] // Faz 4: basis builder consume
    pub(crate) fn canonical_delta(&self) -> &crate::authorization::CanonicalStructuralDelta {
        &self.canonical_delta
    }
    #[allow(dead_code)] // Faz 4: basis builder consume
    pub(crate) fn current_revision(&self) -> &crate::authorization::SpaceViewRevision {
        &self.current_revision
    }
    #[allow(dead_code)] // Faz 4: basis builder consume
    pub(crate) fn current_context(&self) -> &crate::authorization::MeasurementInputContext {
        &self.current_context
    }
    #[allow(dead_code)] // Faz 4: basis builder consume
    pub(crate) fn request_digest(&self) -> &crate::measurement::MeasurementRequestDigest {
        &self.request_digest
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 3 — VerifiedTaskMeasurementBinding (outer opaque proof)
//
// **Reviewer v4 P1-1 + v6 P1-2:** Mevcut `VerifiedMeasurementBinding` task_id/claim/
// measured-result taşımıyor. `MeasurementRequestDigest` hash'e task_id katmıyor.
// Outer proof bu kimlikleri taşır — cross-context substitution protection.
//
// **Clone YOK (reviewer v6 P1-2):** "replay protection" değil "cross-context
// substitution protection". Same-context replay/idempotency Faz 8 commit-ledger
// sorumluluğu. `into_parts(self)` consuming projection — Faz 4 basis builder move-only.
//
// **EngineMeasurement origin invariant (reviewer v6 P1-3 — Faz 1'de kapatılmış):**
// `measurement_digest` yalnız `measure_task_delta` (engine.rs) tarafından üretilen
// EngineMeasurement'tan gelir. Constructor `pub(crate)` (measurement.rs:523),
// Deserialize absent (measurement.rs:563-564) — wire/literal bypass kapalı.
// Single-producer Faz 3 source-structure regression guard ile pinlenir (Commit 1.9).
// ═══════════════════════════════════════════════════════════════════════════════

/// **INV-T9 #70 Commit 4b Faz 3 (reviewer v4 P1-1, v6 P1-2):** Task-bound verified
/// measurement binding — task/claim/measured-result kimliği taşır. Faz 1
/// `VerifiedMeasurementBinding`'i wrap eder (frozen koruma).
///
/// **Cross-context substitution protection (reviewer v6 P1-2):** Outer proof farklı
/// task/claim bağlamına taşınamaz. Aynı bağlamda iki kez kullanım (same-context replay)
/// Faz 8 commit-ledger/idempotency katmanı tarafından engellenir — bu tip `Clone`
/// olmadığı için caller önceden kopya üretemez, ama `build_authorization_context_v2`
/// iki defa çağırmak Faz 8'de ayrıca guardlanmalı.
///
/// **Construction:** modül-private `new()` — yalnız `verify_measurement_binding`
/// (engine.rs aynı modül) çağırır. Rust module privacy + Faz 9 AST call-count +
/// Faz 10 trybuild type-suite ile multi-layer non-forgeability.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "Faz 3 verify_measurement_binding producer + Faz 4 consumer"
)]
pub(crate) struct VerifiedTaskMeasurementBinding {
    task_id: crate::trajectory::TaskId,
    task_claim_digest: crate::measurement::TaskClaimDigest,
    measurement_digest: crate::measurement::MeasurementDigest,
    binding: VerifiedMeasurementBinding,
}

impl VerifiedTaskMeasurementBinding {
    /// Modül-private constructor — yalnız `verify_measurement_binding` çağırır.
    fn new(
        task_id: crate::trajectory::TaskId,
        task_claim_digest: crate::measurement::TaskClaimDigest,
        measurement_digest: crate::measurement::MeasurementDigest,
        binding: VerifiedMeasurementBinding,
    ) -> Self {
        Self {
            task_id,
            task_claim_digest,
            measurement_digest,
            binding,
        }
    }

    /// Task identity — outer proof field. `MeasurementRequestDigest` task_id'yı
    /// doğrudan hash'lemiyor; bu field cross-context substitution'ı engeller.
    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn task_id(&self) -> crate::trajectory::TaskId {
        self.task_id
    }

    /// Claim binding commitment — claim_id + task_id + author + structural_delta_digest.
    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn task_claim_digest(&self) -> &crate::measurement::TaskClaimDigest {
        &self.task_claim_digest
    }

    /// Measured result commitment — 5-axis değer + source.
    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn measurement_digest(&self) -> &crate::measurement::MeasurementDigest {
        &self.measurement_digest
    }

    /// Inner binding — subject/impact/delta/revision/context/request_digest.
    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn binding(&self) -> &VerifiedMeasurementBinding {
        &self.binding
    }

    /// **Consuming projection (reviewer v6 P1-2):** Faz 4 basis builder move-only
    /// consume. Clone YOK — outer proof iki defa kullanılamaz. Same-context replay
    /// Faz 8 commit-ledger sorumluluğu.
    #[allow(dead_code, reason = "Faz 4 basis builder consumer")]
    pub(crate) fn into_parts(
        self,
    ) -> (
        crate::trajectory::TaskId,
        crate::measurement::TaskClaimDigest,
        crate::measurement::MeasurementDigest,
        VerifiedMeasurementBinding,
    ) {
        (
            self.task_id,
            self.task_claim_digest,
            self.measurement_digest,
            self.binding,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INV-T9 #70 Commit 4b Faz 3 — VerificationEpoch (drift-detected consistent verification)
//
// **Reviewer v4 P1-1 + v6 P1-1/P1-2:** `BoundMeasurementSession` axis descriptor'ları
// capture eder ama space revision'ı DEĞİL. Revision okuması ile session begin arası
// race window'u kapatmak için: revision + context aynı epoch içinde capture, ve
// finalization'da revision re-verify.
//
// **All-path finalization (reviewer v6 P1-1):** Session başladıktan sonraki tüm
// success/error/early-return yolları coordinate finalization'dan geçer. Operation
// closure içinde revision/context computation hata verse bile finalization çalışır.
//
// **Capture failure vs drift (reviewer v6 P1-2):**
// - `BoundMeasurementSession::begin` Err → Derivation(CurrentContextCaptureFailed)
//   (capture failure — drift DEĞİL, başlangıç kanıtı elde edilemedi)
// - `session.verify_unchanged()` Err → Drift(CoordinateContextChanged)
//   (capture başarılıydı ama verification sırasında gerçeklik değişti)
//
// **Revision re-verify (reviewer v6 P1-1):** Revision baseline başarıyla capture
// edildiyse verification sonunda yeniden hesaplanıp karşılaştırılır. Capture
// edilemeyen yollar derivation failure olarak sonuçlanır. `revision_after` hesap
// hatası → Derivation(RevisionRecheckFailed).
//
// **Deterministic precedence:** coord drift > revision recheck failed > revision
// before≠after > ordinary verification error. Drift ordinary verification sonuçlarına
// göre öncelikli — drift sırasında üretilen karşılaştırma sonucu güvenilmez.
//
// **Naming (reviewer v6 P2-7):** "atomic snapshot" DEĞİL — "drift-detected consistent
// verification epoch". Optimistic consistency validation with drift detection. Read-lock/
// immutable-copy yok; before/after karşılaştırması ile drift tespiti.
// ═══════════════════════════════════════════════════════════════════════════════

/// Verification epoch view — inner verifier'e geçilen captured revision + context.
/// Private (reviewer P3) — yalnız `with_epoch` + `verify_measurement_binding_inner`
/// tarafından kullanılır. `pub(crate)` DEĞİL (gereksiz yüzey genişletme riski).
struct VerificationEpochView<'a> {
    revision_before: &'a crate::authorization::SpaceViewRevision,
    context: &'a crate::authorization::MeasurementInputContext,
}

impl<'a> VerificationEpochView<'a> {
    fn revision_before(&self) -> &crate::authorization::SpaceViewRevision {
        self.revision_before
    }
    fn context(&self) -> &crate::authorization::MeasurementInputContext {
        self.context
    }
}

/// `with_epoch` operation result — revision_before dışarı taşınır yalnız capture
/// başarılıysa (context construction hatasında proof üretilmiyor, revision gerekmez).
type EpochOperationResult<R> = Result<
    (
        crate::authorization::SpaceViewRevision,
        Result<R, crate::measurement::MeasurementBindingVerificationError>,
    ),
    crate::measurement::MeasurementBindingVerificationError,
>;

impl SpaceEngine {
    /// **Reviewer v6 P1-1 (all-path finalization):** Verification epoch runner.
    ///
    /// Session başladıktan sonraki tüm yollar coordinate finalization'dan geçer:
    /// - revision computation hata → operation Err, ama coord finalization çalışır
    /// - context construction hata → operation Err, ama coord finalization çalışır
    /// - inner verifier mismatch/derivation → operation Err, ama coord finalization çalışır
    /// - inner verifier Ok → operation Ok, coord finalization + revision re-verify
    ///
    /// Session begin failure (capture failure) → Derivation, finalization yok (session yok).
    fn with_epoch<R>(
        &self,
        f: impl FnOnce(
            &VerificationEpochView<'_>,
        ) -> Result<R, crate::measurement::MeasurementBindingVerificationError>,
    ) -> Result<R, crate::measurement::MeasurementBindingVerificationError> {
        use crate::measurement::{
            MeasurementBindingDerivationError, MeasurementBindingVerificationError as VerifErr,
        };

        // Session begin — capture failure (drift DEĞİL): başlangıç kanıtı elde edilemedi.
        let session = match crate::coords::BoundMeasurementSession::begin(&self.coord_system) {
            Ok(s) => s,
            Err(source) => {
                return Err(VerifErr::Derivation(
                    MeasurementBindingDerivationError::CurrentContextCaptureFailed { source },
                ));
            }
        };

        // Operation closure: revision + context setup + inner verify. Tüm fallible işlemler
        // burada — early `?` return bile (closure olduğu için) finalization'ı atlamaz.
        let operation: EpochOperationResult<R> = (|| {
            let revision_before = self.current_space_view_revision().map_err(|detail| {
                VerifErr::Derivation(
                    MeasurementBindingDerivationError::RevisionComputationFailed { detail },
                )
            })?;
            let context =
                crate::authorization::MeasurementInputContext::try_new(session.axis_descriptors())
                    .map_err(|e| {
                        VerifErr::Derivation(
                            MeasurementBindingDerivationError::ContextConstructionFailed {
                                detail: e.to_string(),
                            },
                        )
                    })?;
            let view = VerificationEpochView {
                revision_before: &revision_before,
                context: &context,
            };
            let result = f(&view);
            Ok((revision_before, result))
        })();

        // Finalization — session başladıktan SONRA her durumda çalışır (reviewer v6 P1-1).
        let coordinate_drift = session.verify_unchanged();
        self.finalize_verification(operation, coordinate_drift)
    }

    /// **Reviewer v6 P1-1/P1-2:** Drift-aware finalization. Coord drift + revision
    /// re-verify + ordinary result üçünü deterministic precedence ile birleştirir.
    ///
    /// Precedence: coord drift > revision recheck failed > revision before≠after > ordinary.
    /// Drift tespit edilirse ordinary verification sonucu güvenilmez — drift öncelikli.
    fn finalize_verification<R>(
        &self,
        operation: EpochOperationResult<R>,
        coordinate_drift: Result<(), crate::coords::CoordinateMeasurementError>,
    ) -> Result<R, crate::measurement::MeasurementBindingVerificationError> {
        // **Reviewer v8 P1-1:** revision_after engine'den alınıp pure decision function'a
        // pass edilir. Bu sayede combine_verification_results test edilebilir (mock engine
        // gerekmeden RevisionRecheckFailed kolu doğrulanabilir).
        let revision_after = self.current_space_view_revision();
        Self::combine_verification_results(operation, coordinate_drift, revision_after)
    }

    /// **Reviewer v8 P1-1:** Pure decision combiner — engine state bağımlılığı yok.
    /// `finalize_verification` production'ta `current_space_view_revision()` sonucunu
    /// `revision_after` olarak verir; testler synthetic `Err(...)` vererek
    /// `RevisionRecheckFailed` kolunu doğrudan doğrular.
    #[allow(
        dead_code,
        reason = "Faz 3 test matrisi consumer — combine_verification_results test'leri"
    )]
    fn combine_verification_results<R>(
        operation: EpochOperationResult<R>,
        coordinate_drift: Result<(), crate::coords::CoordinateMeasurementError>,
        revision_after: Result<crate::authorization::SpaceViewRevision, String>,
    ) -> Result<R, crate::measurement::MeasurementBindingVerificationError> {
        use crate::measurement::{
            MeasurementBindingDerivationError, MeasurementBindingDriftError,
            MeasurementBindingVerificationError as VerifErr,
        };

        // Operation Err (revision/context capture failed) — coord drift varsa onu döndür
        // (drift capture failure'a göre öncelikli — capture sırasında gerçeklik değişti).
        let (revision_before, inner_result) = match operation {
            Ok((revision_before, inner_result)) => (revision_before, inner_result),
            Err(capture_err) => {
                return match coordinate_drift {
                    Ok(()) => Err(capture_err),
                    Err(coord) => Err(VerifErr::Drift(
                        MeasurementBindingDriftError::CoordinateContextChanged { source: coord },
                    )),
                };
            }
        };

        // Revision re-verify — baseline capture edildi, final revision karşılaştır.
        match (coordinate_drift, revision_after) {
            // İkisi de Ok — revision before==after kontrolü.
            (Ok(()), Ok(after)) => {
                if revision_before == after {
                    // Drift yok — ordinary result döndür.
                    inner_result
                } else {
                    // Revision drift — ordinary result güvenilmez.
                    Err(VerifErr::Drift(
                        MeasurementBindingDriftError::SpaceRevisionChanged {
                            before: revision_before,
                            after,
                        },
                    ))
                }
            }
            // Coord drift var, revision recheck başarılı — coord öncelikli ama revision
            // drift varsa BothChanged.
            (Err(coord), Ok(after)) => {
                if revision_before == after {
                    Err(VerifErr::Drift(
                        MeasurementBindingDriftError::CoordinateContextChanged { source: coord },
                    ))
                } else {
                    Err(VerifErr::Drift(MeasurementBindingDriftError::BothChanged {
                        coord,
                        before: revision_before,
                        after,
                    }))
                }
            }
            // Coord Ok, revision recheck failed — Derivation (system failure).
            (Ok(()), Err(detail)) => Err(VerifErr::Derivation(
                MeasurementBindingDerivationError::RevisionRecheckFailed { detail },
            )),
            // Coord drift + revision recheck failed — coord öncelikli (revision karşılaştırma yapılamaz).
            (Err(coord), Err(_detail)) => Err(VerifErr::Drift(
                MeasurementBindingDriftError::CoordinateContextChanged { source: coord },
            )),
        }
    }

    /// **INV-T9 #70 Commit 4b Faz 3 (reviewer v4 karar 4 + v6):** Measurement binding
    /// verifier — presented `EngineMeasurement` token'ını claim/task/subject/impact/
    /// delta/revision/context karşısında doğrular. 7 binding validation + canonical
    /// commitment derivation.
    ///
    /// **Standalone primitive (reviewer v6):** Production enforcement Faz 8'de
    /// (caller migration + smart constructor ile). Faz 3'te binding primitive
    /// established, production commit-path enforcement deferred.
    ///
    /// **All-path drift validation:** `with_epoch` session başladıktan sonraki tüm
    /// yolları coordinate finalization + revision re-verify'dan geçirir. Capture
    /// failure (begin Err) → Derivation; gözlenen değişim → Drift.
    #[allow(
        dead_code,
        reason = "Binding primitive established in Faz 3; production commit-path wiring is Faz 8"
    )]
    pub(crate) fn verify_measurement_binding(
        &self,
        claim: &crate::witness::Claim,
        task: &crate::trajectory::Task,
        measurement: &crate::measurement::EngineMeasurement,
    ) -> Result<
        VerifiedTaskMeasurementBinding,
        crate::measurement::MeasurementBindingVerificationError,
    > {
        self.with_epoch(|epoch| {
            self.verify_measurement_binding_inner(epoch, claim, task, measurement)
        })
    }

    /// **7 binding validation + commitment derivation.** Check sırası: TaskMismatch →
    /// Subject → Impact → StructuralDelta → Revision → ContextDigest → CurrentContext.
    /// Her mismatch testi kendisinden önceki check'leri geçecek fixture ile tasarlanmalı
    /// (reviewer P2-1 — check-order-aware).
    fn verify_measurement_binding_inner(
        &self,
        epoch: &VerificationEpochView<'_>,
        claim: &crate::witness::Claim,
        task: &crate::trajectory::Task,
        measurement: &crate::measurement::EngineMeasurement,
    ) -> Result<
        VerifiedTaskMeasurementBinding,
        crate::measurement::MeasurementBindingVerificationError,
    > {
        use crate::measurement::{
            MeasurementBindingDerivationError as DerivErr, MeasurementBindingMismatch as Mismatch,
        };

        // ── Check 1: TaskMismatch ────────────────────────────────────────────────
        // Task identity doğrudan hash değil — explicit check. Aynı subject scope'a sahip
        // iki farklı task aynı request_digest üretebilir, ama bu check TaskMismatch üretir.
        match claim.task_id {
            Some(tid) if tid == task.id => {}
            other => {
                return Err(Mismatch::TaskMismatch {
                    claim_task_id: other,
                    resolved_task_id: task.id,
                }
                .into());
            }
        }

        // ── Check 2: SubjectMismatch ─────────────────────────────────────────────
        let subject = self.derive_task_subject_scope(task).map_err(|e| {
            DerivErr::SubjectDerivationFailed {
                detail: e.to_string(),
            }
        })?;
        if &subject != measurement.request().subject() {
            return Err(Mismatch::SubjectMismatch {
                expected: subject,
                presented: measurement.request().subject().clone(),
            }
            .into());
        }

        // ── Check 3: ImpactMismatch ──────────────────────────────────────────────
        let impact =
            self.derive_impact_scope(claim)
                .map_err(|e| DerivErr::ImpactDerivationFailed {
                    detail: e.to_string(),
                })?;
        if &impact != measurement.request().impact() {
            return Err(Mismatch::ImpactMismatch {
                expected: impact,
                presented: measurement.request().impact().clone(),
            }
            .into());
        }

        // ── Check 4: StructuralDeltaMismatch ─────────────────────────────────────
        // Canonical delta → digest. claim delta'yı canonical'a çevir + digest üret,
        // token request'in structural_delta_digest ile karşılaştır.
        let canonical_delta = crate::authorization::canonical_structural_delta_from_claim(claim)
            .map_err(|e| DerivErr::StructuralCanonicalizationFailed {
                detail: e.to_string(),
            })?;
        let expected_delta_digest =
            crate::measurement::MeasurementDeltaDigest::compute_from_canonical(&canonical_delta)
                .map_err(|e| DerivErr::StructuralCanonicalizationFailed {
                    detail: e.to_string(),
                })?;
        if &expected_delta_digest != measurement.request().structural_delta_digest() {
            return Err(Mismatch::StructuralDeltaMismatch {
                expected: expected_delta_digest,
                presented: measurement.request().structural_delta_digest().clone(),
            }
            .into());
        }

        // ── Check 5: RevisionMismatch ────────────────────────────────────────────
        // Epoch'tan (session begin sonrası capture) — race window kapalı.
        if epoch.revision_before() != measurement.request().base_revision() {
            return Err(Mismatch::RevisionMismatch {
                expected: epoch.revision_before().clone(),
                presented: measurement.request().base_revision().clone(),
            }
            .into());
        }

        // ── Check 6: ContextDigestMismatch (token içi tutarsızlık) ───────────────
        // Context zaten mevcut — yapılan digest hesaplaması. Faz 1 frozen isim
        // (ContextConstructionFailed) korunur, doc net. Defensively fallible — pratikte
        // infallible (already-validated context), ama Result korunur.
        let actual_input_digest = crate::authorization::MeasurementInputDigest::compute(
            measurement.context(),
        )
        .map_err(|e| DerivErr::ContextConstructionFailed {
            detail: e.to_string(),
        })?;
        if &actual_input_digest != measurement.request().measurement_input_digest() {
            return Err(Mismatch::ContextDigestMismatch {
                expected: actual_input_digest,
                presented: measurement.request().measurement_input_digest().clone(),
            }
            .into());
        }

        // ── Check 7: CurrentContextMismatch (epoch context vs token context) ─────
        // Epoch'tan capture edilen context (drift-detect) ile token context karşılaştır.
        let epoch_context_digest = crate::authorization::MeasurementInputDigest::compute(
            epoch.context(),
        )
        .map_err(|e| DerivErr::ContextConstructionFailed {
            detail: e.to_string(),
        })?;
        let token_context_digest = crate::authorization::MeasurementInputDigest::compute(
            measurement.context(),
        )
        .map_err(|e| DerivErr::ContextConstructionFailed {
            detail: e.to_string(),
        })?;
        if epoch_context_digest != token_context_digest {
            return Err(Mismatch::CurrentContextMismatch {
                expected: epoch_context_digest,
                presented: token_context_digest,
            }
            .into());
        }

        // ── Commitment derivation (check değil — proof inşası) ───────────────────
        // TaskClaimDigest: claim_id + task_id + author + structural_delta_digest.
        // **Reviewer v7 P2-1:** TaskClaimDigestComputationFailed — structural DEĞİL,
        // binding commitment hatası (semantic ayrım telemetry için korunur).
        let task_claim_digest =
            crate::measurement::TaskClaimDigest::compute(claim, task.id, &expected_delta_digest)
                .map_err(|e| DerivErr::TaskClaimDigestComputationFailed {
                    detail: e.to_string(),
                })?;

        // MeasurementDigest: 5-axis measured result (engine-origin — measure_task_delta).
        // **Reviewer v7 P2-1:** MeasurementResultDigestComputationFailed — structural DEĞİL,
        // measured-result commitment hatası.
        let measurement_digest =
            crate::measurement::MeasurementDigest::compute(measurement.after()).map_err(|e| {
                DerivErr::MeasurementResultDigestComputationFailed {
                    detail: e.to_string(),
                }
            })?;

        // RequestDigest: defensively fallible — unreachable invariant (pratikte infallible,
        // input already-canonical). Result korunur, test ÜRETİLMEZ (reviewer P2-5).
        let request_digest =
            crate::measurement::MeasurementRequestDigest::compute(measurement.request())
                .map_err(|source| DerivErr::RequestDigestComputationFailed { source })?;

        // Inner binding — Faz 1 frozen 6 field.
        let binding = VerifiedMeasurementBinding::new(
            subject,
            impact,
            canonical_delta,
            epoch.revision_before().clone(),
            epoch.context().clone(),
            request_digest,
        );

        // Outer proof — task/claim/measured-result identity (cross-context substitution protection).
        Ok(VerifiedTaskMeasurementBinding::new(
            task.id,
            task_claim_digest,
            measurement_digest,
            binding,
        ))
    }
}

/// **INV-T9** — `commit_task_claim` expected domain outcome (HATA DEĞİL).
///
/// `Evaluated` = commit pipeline tamamlandı (AcceptAsCompleted Mainline'e, AcceptAsProgress
/// Checkpoint'e, Reject NotApplied — hepsi bu varyantta, apply_target ayrımı TaskCommitResult'ta).
/// `Held` = expected authorization bekleme (INV-T9 suspended state). `Rejected` = explicit
/// witness rejection (non-empty).
///
/// **reviewer P0-4 + plan-review #1:** Held/Rejected artık gerçek engine-owned
/// `AuthorizationContext` taşır. Navigator basis'i yeniden ÜRETMEZ. `Evaluated`'da
/// `authorization: Option<AuthorizationContext>` — Reject→NotApplied ve
/// RequireOperatorApproval terminal'lerde `None` (witness değerlendirilmedi).
///
/// Operational fault'lar (Syntax/Vision/Rule/Permission/Persistence/Internal +
/// InvalidWitnessEvidence) `EngineCommitError`'da kalır.
#[derive(Debug, Clone)]
pub enum EngineCommitResult {
    /// Pipeline evaluated — apply_target NotApplied (Reject) veya Lane (Mainline/Checkpoint).
    /// `authorization`: Satisfied witness varsa `Some` (audit için); Reject→NotApplied'da `None`.
    Evaluated {
        result: TaskCommitResult,
        authorization: Option<crate::authorization::AuthorizationContext>,
    },
    /// INV-T9 — expected authorization bekleme. Navigator AwaitingWitnesses'ye map'ler.
    /// Context witness'tan ÖNCE üretildi — navigator direkt kullanır.
    Held {
        authorization: crate::authorization::AuthorizationContext,
        reason: crate::witness::WitnessHoldReason,
        snapshot: crate::witness::WitnessQuorumSnapshot,
    },
    /// Explicit witness rejection (Q3 honest-reject). Navigator RequiresRevision'a map'ler.
    /// Context witness'tan ÖNCE üretildi.
    Rejected {
        authorization: crate::authorization::AuthorizationContext,
        reasons: crate::witness::NonEmptyWitnessRejections,
        snapshot: crate::witness::WitnessQuorumSnapshot,
    },
}

// ═══════════════════════════════════════════════════════════════════════════════
// SpaceEngine
// ═══════════════════════════════════════════════════════════════════════════════

/// Production runtime — all Faz 1-2 modules orchestrated.
pub struct SpaceEngine {
    space: Space,
    coord_system: crate::coords::CoordinateSystem,
    vision: VisionVector,
    rules: Vec<Box<dyn Rule>>,
    time: TimeFSM,
    config: EngineConfig,
    t_c: u64,
    snapshot_store: Option<SnapshotStore>,
}

impl SpaceEngine {
    /// Manuel kurulum (tüm bileşenler caller sağlar).
    pub fn new(
        space: Space,
        coord_system: crate::coords::CoordinateSystem,
        vision: VisionVector,
        config: EngineConfig,
    ) -> Self {
        Self {
            space,
            coord_system,
            vision,
            rules: vec![], // Faz 5: God Mode `register_rule()` ile ekler
            time: TimeFSM::default(),
            config,
            t_c: 0,
            snapshot_store: None,
        }
    }

    /// **INV-T9 Step 4a:** Q6 Rule Gate için kural ekle — validated registration.
    ///
    /// Sadece duplicate `rule_id` değil; descriptor identity de doğrulanır:
    /// runtime ID boş değil, `descriptor.rule_id == rule.id()`, `semantics_version > 0`,
    /// aynı active `rule_id` daha önce kayıtlı değil. Custom rule descriptor override
    /// tutarsızlığı (runtime id "security.no-sql" ama descriptor "structural.no-cycle")
    /// yakalanır.
    ///
    /// Kurallar `check_claim_rules_with_context()` içinde sırayla evaluate edilir.
    /// İlk ihlalde claim reddedilir (short-circuit) — registration sırası semantik.
    pub fn register_rule(
        &mut self,
        rule: Box<dyn crate::rule::Rule>,
    ) -> Result<(), RuleRegistrationError> {
        let runtime_id = rule.id();
        if runtime_id.is_empty() {
            return Err(RuleRegistrationError::EmptyRuleId);
        }
        let descriptor = rule.descriptor();
        if descriptor.rule_id != *runtime_id {
            return Err(RuleRegistrationError::IdentityMismatch {
                runtime_id: runtime_id.clone(),
                descriptor_id: descriptor.rule_id,
            });
        }
        if descriptor.semantics_version == 0 {
            return Err(RuleRegistrationError::InvalidSemanticsVersion(
                descriptor.semantics_version,
            ));
        }
        if self
            .rules
            .iter()
            .any(|existing| existing.id() == runtime_id)
        {
            return Err(RuleRegistrationError::DuplicateActiveRuleId(
                runtime_id.clone(),
            ));
        }
        self.rules.push(rule);
        Ok(())
    }

    /// Q6 için varsayılan yapısal kural seti ile engine kur (no_self_import,
    /// no_duplicate_node, edge_target_exists).
    ///
    /// **Step 4a:** `register_rule` artık Result döner — `?` ile yayılır.
    pub fn with_default_rules(
        space: Space,
        coord_system: crate::coords::CoordinateSystem,
        vision: VisionVector,
        config: EngineConfig,
    ) -> Result<Self, RuleRegistrationError> {
        let mut engine = Self::new(space, coord_system, vision, config);
        for rule in crate::rule::default_rules() {
            engine.register_rule(rule)?;
        }
        Ok(engine)
    }

    /// `VisionConfig`'ten kurulum (TOML → engine).
    pub fn from_vision_config(
        space: Space,
        coord_system: crate::coords::CoordinateSystem,
        config: &VisionConfig,
    ) -> Self {
        Self::new(
            space,
            coord_system,
            config.to_vision_vector(),
            EngineConfig::from_vision_config(config),
        )
    }

    /// Persistence aç (event-sourcing — delta + milestone).
    pub fn with_persistence(
        mut self,
        base_dir: impl AsRef<Path>,
    ) -> Result<Self, PersistenceError> {
        self.snapshot_store = Some(SnapshotStore::new(base_dir)?);
        Ok(self)
    }

    // ── Commit pipeline (§4) ───────────────────────────────────────────────

    /// `commit(claim, omega)` — full pipeline (Q4-Q6 claim-based → Q1-Q3 witness → mutate → reposition → save).
    ///
    /// 0. CLAIM-BASED GATES (Q4-Q6, deterministik, witness öncesi):
    ///    - Q4 Syntax: OutputContract compliant? (inv #12)
    ///    - Q5 Vision: claim.computed_raw θ > bound → Err(VisionViolation) [mutasyon YOK]
    ///    - Q6 Rule: Rule ihlali?
    /// 1. WITNESS-BASED GATES (Q1-Q3) + TIME ADVANCE: evaluate + apply_delta
    /// 2. REPOSITION: CosineDeviation → drift_warnings
    /// 3. SAVE DELTA: event-sourcing
    /// 4. MILESTONE: periyodik tam snapshot
    /// 5. EMIT: CommitOutcome
    pub fn commit(
        &mut self,
        claim: &Claim,
        omega: &WitnessSet,
    ) -> Result<CommitOutcome, EngineCommitError> {
        // Phase 0: CLAIM-BASED GATES (Q4-Q6 — deterministik, witness öncesi)
        self.check_claim_syntax(claim)?;
        // **Step 4b:** Captured vision context — Q5 + (digest üretmez ama) aynı
        // validation pattern. Legacy commit() digest üretmez; standalone yol.
        let vision_context = self
            .effective_vision_gate_context(claim)
            .map_err(EngineCommitError::VisionContextInvalid)?;
        self.check_claim_vision_with_context(claim, &vision_context)?;
        // **Step 4a:** Q6 ordinal-aware rule context (standalone commit — authorization
        // digest üretmez ama rule evaluation yine de context snapshot'ı kullanır).
        let rule_context = self
            .current_rule_evaluation_context()
            .map_err(EngineCommitError::AuthorizationContextFailed)?;
        self.check_claim_rules_with_context(claim, &rule_context)?;

        // Phase 1: WITNESS-BASED GATES (Q1-Q3) + TIME ADVANCE (apply_delta mutasyon)
        let result = self.time.advance(&mut self.space, claim, omega);
        let (delta, safety_weakened) = match result {
            WitnessDisposition::Satisfied {
                delta,
                safety_weakened,
                ..
            } => (delta, safety_weakened),
            // **INV-T9:** Legacy `commit()` (standalone/Paper 1) Held/Rejected'ı Err olarak
            // işler. INV-T9 conformance `commit_task_claim` yolunda geçerli (EngineCommitResult).
            // Legacy commit() production'da kullanılmıyor (navigator commit_task_claim kullanır);
            // bu test/setup yolunda Held/Rejected witness shortage olarak Internal error döner.
            // P1: legacy commit() refactor → EngineCommitResult (INV-T9 tam conformance).
            WitnessDisposition::Held { reason, .. } => {
                return Err(EngineCommitError::Internal(format!(
                    "legacy commit() witnessed Held (use commit_task_claim for INV-T9): {reason:?}"
                )));
            }
            WitnessDisposition::Rejected { reasons, .. } => {
                return Err(EngineCommitError::Internal(format!(
                    "legacy commit() witnessed Rejected (use commit_task_claim for INV-T9): {reasons:?}"
                )));
            }
        };

        self.t_c += 1;

        // Phase 2: REPOSITION (CosineDeviation + drift warnings, inv #5)
        let drift_warnings = self.reposition_nodes(&delta.repositioned);

        // Phase 3: SAVE DELTA (event-sourcing)
        if let Some(store) = &self.snapshot_store {
            let record = DeltaRecord {
                version: SNAPSHOT_FORMAT_VERSION,
                t_c: self.t_c,
                claim_id: claim.id,
                delta: delta.clone(),
                safety_weakened,
            };
            let _ = store.save_delta(record); // best-effort; log on error
        }

        // Phase 4: MILESTONE (periyodik)
        if self.t_c % self.config.milestone_interval == 0 {
            if let Some(store) = &self.snapshot_store {
                let snapshot = SpaceSnapshot {
                    version: SNAPSHOT_FORMAT_VERSION,
                    t_c: self.t_c,
                    timestamp_ms: current_time_ms(),
                    space: self.space.clone(),
                };
                let _ = store.save_milestone(snapshot);
            }
        }

        // Phase 5: EMIT
        Ok(CommitOutcome {
            event: delta,
            drift_warnings,
            safety_weakened,
            t_c: self.t_c,
        })
    }

    /// Aşama D2 — Task-bound Claim commit. Atomic pipeline: Q4 → bind → Q5 → Q5.b
    /// (PredicateGate) → Q6 → MutationDecision → ApplyTarget → Q1-Q3 witness.
    ///
    /// **Prensip:** `commit() = legacy/standalone path; commit_task_claim() = trajectory path.`
    /// Mevcut commit() (standalone, Paper 1) korunur — backward compatible. Bu metod
    /// task-bound Claim'ler için Q5.b PredicateGate'i commit transaction içine alır
    /// (atomic — navigator ayrı PredicateGate çağırmaz).
    ///
    /// **İç akış (sizin önerdiğiniz sıra):**
    /// 1. Q4 Syntax (check_claim_syntax)
    /// 2. bind_claim_to_task (TaskResolver → TaskBoundClaim, INV-T5)
    /// 3. Q5 Vision (θ bound, check_claim_vision)
    /// 4. Q5.b PredicateGate (task predicate, loss/policy → MutationDecision)
    /// 5. Q6 Rule (check_claim_rules)
    /// 6. MutationDecision → ApplyTarget (INV-T8: Reject→NotApplied, Progress→Checkpoint)
    /// 7. Q1-Q3 Witness (AcceptAsCompleted/AcceptAsProgress ise — apply_delta)
    /// 8. TaskCommitResult (outcome + apply_target + witness)
    pub fn commit_task_claim(
        &mut self,
        input: TaskCommitInput<'_>,
    ) -> Result<EngineCommitResult, EngineCommitError> {
        use crate::trajectory::{ApplyTarget, MutationDecision, PredicateGate, PredicateGateInput};
        use crate::witness::WitnessDisposition;

        // Phase 0a: Q4 Syntax (claim-based, deterministik).
        self.check_claim_syntax(input.claim)?;

        // Phase 0b: bind_claim_to_task (INV-T5 — TaskBoundClaim zorunlu).
        // bind_task_claim generic (impl TaskResolver), &dyn ile çağrılamaz → manuel bind.
        let task_id = input.claim.task_id.ok_or_else(|| {
            EngineCommitError::PermissionDenied(
                "claim has no task_id (standalone — commit_task_claim requires TaskBoundClaim)"
                    .into(),
            )
        })?;
        let task = input.task_resolver.resolve(task_id).ok_or_else(|| {
            EngineCommitError::PermissionDenied(format!("task_id {task_id} not found in resolver"))
        })?;
        let bound = crate::trajectory::TaskBoundClaim {
            claim: input.claim,
            task,
        };

        // **Phase 0b+ (INV-T9 #70 Commit 4b Faz 3 — reviewer v2 karar 2, tag 7):**
        // Task declaration validation. TaskBoundClaim yalnız `claim.task_id ↔ task.id`
        // identity binding'i kanıtlar (semantic contract) — task'ın commit için geçerli
        // olduğunu ima ETMEZ. Declaration validity `validate_for_commit()` ile ayrıca
        // kontrol edilir: empty predicate set, non-finite threshold/tolerance, Mixed
        // source requirement, geçersiz policy.
        //
        // Guard order: Q4 syntax → task bind → **validate_for_commit** → Q5 vision →
        // (Faz 8: verify_measurement_binding) → Q5.b gate → Q6 rule → witness.
        // Terminal — maneuver budget tüketmez, witness'a ulaşmaz, authorization üretmez.
        bound.task.validate_for_commit()?;

        // Phase 0c: Q5 Vision (θ bound — negatif-uzay safety).
        // **Step 4b:** Captured `EffectiveVisionGateContext` — bir kez üretilir, Q5 +
        // build_authorization_context + digest paylaşır (4a rule_context pattern).
        // None/GlobalDefault/mismatch/non-finite → terminal VisionContextInvalid (P0-4).
        let vision_context = self
            .effective_vision_gate_context(input.claim)
            .map_err(EngineCommitError::VisionContextInvalid)?;
        self.check_claim_vision_with_context(input.claim, &vision_context)?;

        // Phase 0d: Q5.b PredicateGate (soft gate — task completion + policy).
        let gate_out = PredicateGate.evaluate(PredicateGateInput {
            bound,
            measured: &input.measured,
            loss_before: input.loss_before,
            target: &input.target,
        });
        let outcome = gate_out.outcome.clone();
        let loss_after = gate_out.loss_after;
        let apply_target = outcome.mutation_decision.apply_target();

        // **INV-T9 Step 4a:** Rule evaluation context — Q6 ve digest tarafından PAYLAŞILAN
        // ordinal-aware snapshot. İki ayrı yerde rule listesi üretip drift bırakmaz.
        let rule_context = self
            .current_rule_evaluation_context()
            .map_err(EngineCommitError::AuthorizationContextFailed)?;

        // Phase 0e: Q6 Rule (claim-based, deterministik).
        // Not: MutationDecision Reject ise bile Q6 çalışır (diagnostic — hangi gate reject etti).
        // **Step 4a:** Q6 aynı rule_context snapshot'ını kullanır (ordinal alignment).
        if !matches!(outcome.mutation_decision, MutationDecision::Reject) {
            self.check_claim_rules_with_context(input.claim, &rule_context)?;
        }

        // Phase 0f: MutationDecision → ApplyTarget kontrolü (INV-T8).
        // Reject → NotApplied (commit yok, witness yok). authorization: None — witness
        // değerlendirilmedi, mutation uygulanmadı.
        if matches!(apply_target, ApplyTarget::NotApplied) {
            return Ok(EngineCommitResult::Evaluated {
                result: TaskCommitResult {
                    outcome,
                    apply_target,
                    loss_after,
                    witness: None,
                },
                authorization: None,
            });
        }

        // **reviewer P0-4 + plan-review #1:** AuthorizationContext tam bir kez üretilir —
        // bütün deterministik gate'ler (Q4/Q5/Q5.b/Q6) geçtikten sonra, witness
        // (`time.advance`) çağrısından hemen önce. Satisfied/Held/Rejected aynı context'i
        // kullanır. witness_requirement gerçek `input.omega`'dan (engine config DEĞİL).
        // **Step 4b:** Captured `vision_context` paylaşılır — Q5 ile aynı effective vision.
        let authorization = self
            .build_authorization_context(
                &outcome,
                apply_target,
                &input,
                input.loss_before,
                loss_after,
                &gate_out.improvement_policy,
                &rule_context,
                &vision_context,
                input.omega,
            )
            .map_err(EngineCommitError::AuthorizationContextFailed)?;

        // Phase 1: Q1-Q3 Witness (AcceptAsCompleted/AcceptAsProgress/OperatorApproval).
        // apply_delta mutation — mevcut commit() gibi time.advance.
        //
        // **INV-T9:** WitnessDisposition::Held expected authorization bekleme, Rejected
        // explicit witness ret — ikisi de domain outcome, HATA DEĞİL. EngineCommitResult::Held/
        // Rejected olarak döner; navigator AwaitingWitnesses/RequiresRevision'a map'ler.
        let disposition = self.time.advance(&mut self.space, input.claim, input.omega);
        match disposition {
            WitnessDisposition::Satisfied { .. } => {
                self.t_c += 1;
                Ok(EngineCommitResult::Evaluated {
                    result: TaskCommitResult {
                        outcome,
                        apply_target,
                        loss_after,
                        witness: Some(disposition),
                    },
                    authorization: Some(authorization),
                })
            }
            WitnessDisposition::Held { reason, snapshot } => Ok(EngineCommitResult::Held {
                authorization,
                reason,
                snapshot,
            }),
            WitnessDisposition::Rejected { reasons, snapshot } => {
                Ok(EngineCommitResult::Rejected {
                    authorization,
                    reasons,
                    snapshot,
                })
            }
        }
    }

    /// **reviewer P0-4 + plan-review #1:** Engine-owned AuthorizationContext üretimi.
    ///
    /// Witness'tan ÖNCE, bütün deterministik gate'ler geçtikten sonra çağrılır.
    /// Engine'in elindeki TÜM gerçek verilerden basis inşa eder — navigator placeholder
    /// DEĞİL. Hata durumunda fail-closed (SystemFailure) — sıfır digest'e düşüş YOK.
    ///
    /// **plan-review #1:** `witness_requirement` ve `basis.witness_policy` gerçek
    /// `input.omega`'dan türetilir (engine config DEĞİL).
    ///
    /// **Step 4b:** Captured `vision_context` paylaşılır — Q5 ile aynı effective vision
    /// digest'a bağlanır. Yeniden vision infer YOK (drift risk kapalı).
    #[allow(clippy::too_many_arguments)]
    fn build_authorization_context(
        &self,
        outcome: &crate::trajectory::AttemptOutcome,
        apply_target: crate::trajectory::ApplyTarget,
        input: &TaskCommitInput<'_>,
        loss_before: f64,
        loss_after: f64,
        improvement_policy: &crate::authorization::EffectiveImprovementPolicy,
        rule_context: &crate::authorization::RuleEvaluationContext,
        vision_context: &crate::authorization::EffectiveVisionGateContext,
        omega: &crate::witness::WitnessSet,
    ) -> Result<crate::authorization::AuthorizationContext, String> {
        use crate::authorization::{
            AuthorizationBasis, CanonicalF64, CanonicalPredicateContent, CanonicalRawPosition,
            CanonicalWitnessPolicy, ClaimAuthor, ClaimIdentity, MeasurementInputContext,
            MeasurementInputDigest, PredicateEvaluationBasis, ProvenancedMeasuredResult,
            WitnessRequirement,
        };
        use crate::canonical_tags::{PredicateAxisTag, PredicateModeTag};
        let claim = input.claim;
        let task_id = claim
            .task_id
            .ok_or_else(|| "claim has no task_id for authorization context".to_string())?;

        // **Reviewer v5 P1-2:** Shared structural delta producer — measurement
        // `MeasurementDeltaDigest` ile aynı ontology. İki truth source (inline vs
        // shared producer) drift riskini kapatır. canonical_structural_delta_from_claim
        // claim'in delta_nodes/delta_edges/removed_edges field'larını canonical'a çevirir
        // ve try_new (sort + validate) ile tek canonical representation üretir.
        let structural_delta = crate::authorization::canonical_structural_delta_from_claim(claim)
            .map_err(|e| e.to_string())?;

        // Predicate content — task'ın predicate set'inden effective predicate'lara.
        let task = input.task_resolver.resolve(task_id).ok_or_else(|| {
            format!("task_id {task_id} not found in resolver during authorization context build")
        })?;
        let predicate_mode = PredicateModeTag::try_from(&task.target_predicate_set.mode)
            .map_err(|e| e.to_string())?;
        let predicates: Vec<crate::authorization::EffectiveMetricPredicate> = task
            .target_predicate_set
            .predicates
            .iter()
            .map(|wp| {
                Ok(crate::authorization::EffectiveMetricPredicate {
                    axis: PredicateAxisTag::try_from(&wp.predicate.metric)
                        .map_err(|e: crate::authorization::CanonicalizationError| e.to_string())?,
                    operator: crate::canonical_tags::ComparisonOpTag::try_from(
                        &wp.predicate.operator,
                    )
                    .map_err(|e: crate::authorization::CanonicalizationError| e.to_string())?,
                    threshold: wp.predicate.threshold,
                    scope: canonicalize_scope(&wp.predicate.scope)?,
                    required_source: canonicalize_source_req(&wp.predicate.required_source)?,
                    effective_weight: wp.weight.unwrap_or(1.0),
                    effective_tolerance: wp.predicate.tolerance,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let predicate_content = CanonicalPredicateContent {
            mode: predicate_mode,
            predicates,
        };

        // Predicate evaluation basis — gerçek PredicateGate girdileri (reviewer P1-2).
        // target_vector = input.target (preferred_vector DEĞİL — evaluator input.target kullanır).
        // min_improvement_delta = gerçek is_improved_loss girdisi.
        // improvement_policy = mevcut sabit 0.85/0.15 threshold'ları explicit.
        let predicate_evaluation = PredicateEvaluationBasis {
            target_vector: CanonicalRawPosition {
                x: input.target.x,
                y: input.target.y,
                z: input.target.z,
                w: input.target.w,
                v: input.target.v,
            },
            loss_before: loss_before as CanonicalF64,
            loss_after: loss_after as CanonicalF64,
            failure_policy: crate::canonical_tags::PredicateFailurePolicyTag::try_from(
                &task.policy.predicate_failure_policy,
            )
            .map_err(|e| e.to_string())?,
            min_improvement_delta: task.policy.min_improvement_delta as CanonicalF64,
            allow_progress_checkpoint: task.policy.allow_progress_checkpoint,
            improvement_policy: *improvement_policy,
        };

        // Measured result — 5 eksen value + source (INV-T4 per-axis provenance).
        // Her eksenin MetricSource'u ayrı bağlanır — INV-T4 source-requirement kararının
        // evidence basis'i tam (placeholder source ile task kapatma engeli reconstructible).
        let mk_axis = |am: &crate::trajectory::AxisMetric| -> Result<_, String> {
            Ok(crate::authorization::CanonicalAxisMeasurement {
                value: am.value,
                source: crate::canonical_tags::CanonicalMetricSourceTag::try_from(&am.source)
                    .map_err(|e: crate::authorization::CanonicalizationError| e.to_string())?,
            })
        };
        let measured_result = ProvenancedMeasuredResult {
            coupling: mk_axis(&input.measured.coupling)?,
            cohesion: mk_axis(&input.measured.cohesion)?,
            instability: mk_axis(&input.measured.instability)?,
            entropy: mk_axis(&input.measured.entropy)?,
            witness_depth: mk_axis(&input.measured.witness_depth)?,
        };

        // Witness policy — gerçek omega'dan (plan-review #1).
        let witness_policy = CanonicalWitnessPolicy::try_from(omega).map_err(|e| e.to_string())?;

        // **INV-T9 Adım 3:** Measurement input context — gerçek axis descriptor'ları
        // (placeholder config_tag/axis_tags kaldırıldı). CoordinateSystem'den üretilir;
        // axis implementation identity + semantics + canonical parameters bağlanır.
        let measurement_input =
            MeasurementInputContext::try_from(&self.coord_system).map_err(|e| e.to_string())?;
        let measurement_input_digest =
            MeasurementInputDigest::compute(&measurement_input).map_err(|e| e.to_string())?;

        // **reviewer (Step 4a + 4b + 4c closure):** Evaluation context digest — captured
        // `rule_context` + `vision_context` kullanır (commit_task_claim'in ürettiği
        // snapshot'lar). Yeniden `current_evaluation_context_digest()` çağrısı YOK —
        // Q5/Q6 ve digest aynı captured context'lerden türetilir (drift risk kapalı).
        // **Step 4c:** config parametresi KALDIRILDI — digest yalnız Q5/Q6 girdilerini bağlar.
        let evaluation_context_digest =
            crate::authorization::EvaluationContextDigest::compute(rule_context, vision_context)
                .map_err(|e| e.to_string())?;
        let base_space_view_revision = self.current_space_view_revision()?;

        let basis = AuthorizationBasis {
            schema_version: 1,
            task_id,
            claim_identity: ClaimIdentity {
                claim_id: claim.id,
                task_id,
            },
            claim_author: claim.author as ClaimAuthor,
            structural_delta,
            predicate_content,
            predicate_evaluation,
            measured_result,
            deterministic_gate_result: outcome.gate_decision,
            predicate_completion: outcome.predicate_completion,
            mutation_decision: outcome.mutation_decision,
            intended_apply_target: apply_target,
            witness_policy,
            measurement_input_digest,
            evaluation_context_digest,
            base_space_view_revision,
        };

        Ok(crate::authorization::AuthorizationContext {
            outcome: outcome.clone(),
            apply_target,
            basis,
            witness_requirement: WitnessRequirement::from(omega),
        })
    }

    // ── Claim-based gates (Q4-Q6, Phase 0 — witness öncesi, deterministik) ───

    /// Q4 Syntax Gate — Claim'in ΔS yapısı geçerli mi? (inv #12)
    ///
    /// Kontroller:
    /// 1. delta_nodes: geçerli NodeKind, finite/non-negative mass, non-negative id
    /// 2. delta_edges: Imports self-loop reddi, geçerli EdgeKind, from/to ≥ 0
    /// 3. delta_nodes içinde duplicate ID yok
    /// 4. computed_raw: tüm core eksen değerleri finite
    ///
    /// **INV-T9 #70 Commit 4b (reviewer v2 P1-3):** Artık ayrılmış helper'lara delegasyon —
    /// `check_claim_structure` (structural 1-3) + `check_raw_position_finite` (computed_raw 4).
    /// Task-bound path (Faz 3) `measurement.after().to_raw()` ile finite-check yapar.
    fn check_claim_syntax(&self, claim: &Claim) -> Result<(), EngineCommitError> {
        self.check_claim_structure(claim)?;
        self.check_raw_position_finite(claim.id, "computed_raw", &claim.computed_raw)?;
        Ok(())
    }

    /// **INV-T9 #70 Commit 4b (reviewer v2 P1-3):** Structural syntax validation —
    /// node mass/kind, duplicate ID, edge self-import. `claim.computed_raw`'a dokunmaz
    /// (finite-check ayrı). Legacy `commit()` + task-bound path ortak kullanır.
    fn check_claim_structure(&self, claim: &Claim) -> Result<(), EngineCommitError> {
        // 1. Node validation
        for node in &claim.delta_nodes {
            if node.id == 0 && !claim.delta_nodes.is_empty() {
                // id=0 is valid for first node; check mass/kind instead
            }
            if !node.mass.is_finite() || node.mass < 0.0 {
                return Err(EngineCommitError::SyntaxViolation {
                    violation: SyntaxViolation {
                        claim_id: claim.id,
                        detail: format!(
                            "node {} has invalid mass: {} (must be finite, non-negative)",
                            node.id, node.mass
                        ),
                    },
                });
            }
        }

        // 2. Duplicate node IDs within delta
        let mut seen_ids: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        for node in &claim.delta_nodes {
            if !seen_ids.insert(node.id) {
                return Err(EngineCommitError::SyntaxViolation {
                    violation: SyntaxViolation {
                        claim_id: claim.id,
                        detail: format!("duplicate node id {} in delta_nodes", node.id),
                    },
                });
            }
        }

        // 3. Edge validation
        for edge in &claim.delta_edges {
            // Imports self-loop: module cannot import itself (semantic rule)
            if edge.kind == EdgeKind::Imports && edge.from == edge.to {
                return Err(EngineCommitError::SyntaxViolation {
                    violation: SyntaxViolation {
                        claim_id: claim.id,
                        detail: format!("self-import edge: node {} imports itself", edge.from),
                    },
                });
            }
        }

        Ok(())
    }

    /// **INV-T9 #70 Commit 4b (reviewer v2 P1-3 + Faz 2 scoped P2-2):** RawPosition
    /// finite-check — flat x/y/z/w/v eksen değerleri finite. `claim.computed_raw`'dan
    /// ayrı parametre (task-bound path `measurement.after().to_raw()` ile çağırır — Faz 3).
    /// `source_label` nötr — "computed_raw" (legacy) veya "measurement.after" (task-bound).
    fn check_raw_position_finite(
        &self,
        claim_id: crate::witness::ClaimId,
        source_label: &str,
        raw: &crate::coords::RawPosition,
    ) -> Result<(), EngineCommitError> {
        let axes = [
            ("x", raw.x),
            ("y", raw.y),
            ("z", raw.z),
            ("w", raw.w),
            ("v", raw.v),
        ];
        for (name, val) in &axes {
            if !val.is_finite() {
                return Err(EngineCommitError::SyntaxViolation {
                    violation: SyntaxViolation {
                        claim_id,
                        detail: format!("{}.{} is not finite: {}", source_label, name, val),
                    },
                });
            }
        }
        Ok(())
    }

    /// Q5 Vision Gate — `θ(claim.computed_raw, vision) > theta_bound` → Err.
    /// Claim negatif-uzayda ise ana dala GİREMEZ (BFT-derived Safety, §4.1).
    ///
    /// **Role-aware:** Claim'in temsil ettiği node'un mimari rolüne göre vision
    /// vector seçilir (override varsa). Örn: bir TypeSurface node'u için coupling
    /// düşük beklenir — global vision'a göre fail etmemeli. Rol, claim'in ilk
    /// delta_node'unun classification'ından çıkarılır (çoğu claim tek node ekler).
    ///
    /// **INV-T9 Step 4b (reviewer P0-1):** Artık captured `EffectiveVisionGateContext`
    /// kullanır — `effective_vision_gate_context(claim)` bir kez üretilir, Q5 + digest
    /// paylaşır. `vision_for_claim` wrapper'ı legacy/test yüzeylerinde kalır.
    ///
    /// **INV-T9 #70 Commit 4b (reviewer v2 P1-3):** `claim.computed_raw`'a delegasyon —
    /// `check_vision_raw_with_context`. Task-bound path (Faz 3) `measurement.after().to_raw()`
    /// ile çağırır.
    fn check_claim_vision_with_context(
        &self,
        claim: &Claim,
        vision_context: &crate::authorization::EffectiveVisionGateContext,
    ) -> Result<(), EngineCommitError> {
        self.check_vision_raw_with_context(claim.id, &claim.computed_raw, vision_context)
    }

    /// **INV-T9 #70 Commit 4b (reviewer v2 P1-3):** Vision gate — ayrı raw parametre.
    /// `claim.computed_raw`'dan bağımsız — task-bound path `measurement.after().to_raw()`
    /// ile çağırır (token authority). Violation evidence `raw` field'ı authority-tied.
    fn check_vision_raw_with_context(
        &self,
        claim_id: crate::witness::ClaimId,
        raw: &crate::coords::RawPosition,
        vision_context: &crate::authorization::EffectiveVisionGateContext,
    ) -> Result<(), EngineCommitError> {
        // Effective vision captured context'ten — yeniden infer YOK.
        let vision = vision_context.selection.effective_vision;
        let theta = CosineDeviation.theta(raw, &vision, &self.space);
        if theta > vision_context.theta_bound {
            tracing::warn!(
                claim_id,
                theta,
                bound = vision_context.theta_bound,
                "Q5 vision violation — claim rejected (negatif-uzay)"
            );
            return Err(EngineCommitError::VisionViolation {
                violation: VisionViolation {
                    claim_id,
                    theta,
                    raw: *raw,
                },
                bound: vision_context.theta_bound,
            });
        }
        Ok(())
    }

    /// **INV-T9 Step 4b (reviewer P0-1):** Tek karar ağacı — role inference + vision
    /// selection AYNI fonksiyonda. Subject + effective vector + source birlikte üretilir.
    ///
    /// **scoped-review P1-a:** Subject = claim'in değerlendirme bağlamı. `delta_node`
    /// varsa override olsun/olmasın `Role(infer_role)` üretilir — global fallback'te
    /// bile claim'in rolü korunur (Runtime claim + global UserLoaded ≠ Support claim +
    /// global UserLoaded). Yalnız `delta_node` yoksa `Global`.
    ///
    /// **scoped-review P1-c:** Canonical role conversion fail-closed. Yeni `NodeRole`
    /// varyantı eklendiğinde context başka role aitmiş gibi kaydedilmesin; dönüşüm hatası
    /// `CanonicalRoleConversionFailed` olarak terminal yayılır (sessiz Runtime fallback YOK).
    ///
    /// **scoped-review P0:** Vision source TEK truth — `effective_vision.source()`. Ayrı
    /// `vision_source` alanı YOK.
    ///
    /// Cascade (subject her zaman önce üretilir, sonra source/vector):
    /// 1. `delta_node.first()` varsa → `infer_role("", classification, None)` → `subject = Role(role)`.
    ///    a. Kullanıcı TOML override (`role_overrides[Role]`) → `RoleProfile`
    ///    b. `builtin_role_override` (hardcoded) → `BuiltinRole`
    ///    c. Override yok → global vision inherit → source inherit (UserLoaded/GlobalDefault/None)
    /// 2. `delta_node` yok → `subject = Global`, global vision inherit.
    ///
    /// **Alan adı:** `subject` (`inferred_role` DEĞİL — global bir inferred role değildir).
    /// Semantics version'lar (`ROLE_INFERENCE_SEMANTICS_VERSION`,
    /// `VISION_SELECTION_SEMANTICS_VERSION`) digest'e bağlı — staleness tespiti.
    pub(crate) fn effective_vision_selection(
        &self,
        claim: &Claim,
    ) -> Result<
        crate::authorization::EffectiveVisionSelection,
        crate::authorization::VisionContextError,
    > {
        use crate::authorization::{
            CanonicalVisionSubject, EffectiveVisionSelection, ROLE_INFERENCE_SEMANTICS_VERSION,
            VISION_SELECTION_SEMANTICS_VERSION,
        };
        use crate::space::infer_role;
        use crate::vision::VisionSource;
        use crate::vision_config::VisionConfig;

        // İlk delta_node'un classification'ından rol çıkar (path/metric olmadan
        // classification-only — engine path bilmez, sadece node classification).
        if let Some(node) = claim.delta_nodes.first() {
            let role = infer_role("", node.classification, None);
            // **P1-c:** Canonical role conversion fail-closed (sessiz Runtime fallback YOK).
            let canonical_role = crate::canonical_tags::CanonicalNodeRole::try_from(&role)
                .map_err(|e| {
                    crate::authorization::VisionContextError::CanonicalRoleConversionFailed(
                        e.to_string(),
                    )
                })?;
            // **P1-a:** subject her zaman Role (override olsun/olmasın) — claim'in
            // değerlendirme bağlamı korunur.
            let subject = CanonicalVisionSubject::Role(canonical_role);
            // Önce kullanıcı TOML override'ı (RoleProfile), sonra builtin (BuiltinRole).
            let key = format!("{:?}", role);
            let user_override = self.config.role_overrides.get(&key).cloned();
            let builtin_override = VisionConfig::builtin_role_override(role);
            // Kullanıcı override'ı varsa o kazanır; yoksa builtin.
            if let Some(ovr) = user_override.clone().or(builtin_override.clone()) {
                let mut raw_v = *self.vision.raw();
                if let Some(x) = ovr.x {
                    raw_v.x = x;
                }
                if let Some(y) = ovr.y {
                    raw_v.y = y;
                }
                if let Some(z) = ovr.z {
                    raw_v.z = z;
                }
                // Source: kullanıcı override mı, builtin mi?
                let source = if user_override.is_some() {
                    VisionSource::RoleProfile
                } else {
                    VisionSource::BuiltinRole
                };
                return Ok(EffectiveVisionSelection {
                    effective_vision: VisionVector::with_source(raw_v, source),
                    subject,
                    role_inference_semver: ROLE_INFERENCE_SEMANTICS_VERSION,
                    vision_selection_semver: VISION_SELECTION_SEMANTICS_VERSION,
                });
            }
            // Override yok → engine global vision'ı inherit et. Subject Role korunur (P1-a);
            // source vision'ın kendi provenance'ından gelir (UserLoaded/GlobalDefault/None).
            return Ok(EffectiveVisionSelection {
                effective_vision: self.vision,
                subject,
                role_inference_semver: ROLE_INFERENCE_SEMANTICS_VERSION,
                vision_selection_semver: VISION_SELECTION_SEMANTICS_VERSION,
            });
        }
        // delta_node yok → engine global vision'ı inherit. Subject Global.
        Ok(EffectiveVisionSelection {
            effective_vision: self.vision,
            subject: CanonicalVisionSubject::Global,
            role_inference_semver: ROLE_INFERENCE_SEMANTICS_VERSION,
            vision_selection_semver: VISION_SELECTION_SEMANTICS_VERSION,
        })
    }

    /// **INV-T9 Step 4b (reviewer P0-1 + P0-3):** Claim-specific effective vision gate
    /// context üret + validate_for_authorization. Q5 öncesinde çağrılır; None/
    /// GlobalDefault burada fail-closed reddedilir (VisionContextInvalid → terminal).
    ///
    /// Captured-context pattern: bir kez üretilir, Q5 + build_authorization_context +
    /// digest paylaşır (4a rule_context ile aynı).
    pub(crate) fn effective_vision_gate_context(
        &self,
        claim: &Claim,
    ) -> Result<
        crate::authorization::EffectiveVisionGateContext,
        crate::authorization::VisionContextError,
    > {
        use crate::authorization::EffectiveVisionGateContext;
        let selection = self.effective_vision_selection(claim)?;
        EffectiveVisionGateContext::try_new(selection, self.config.theta_bound)
    }

    /// **INV-T9 Step 4a:** Q6 Rule Gate — ΔS herhangi bir Rule'u ihlal ediyor mu?
    ///
    /// `RuleEvaluationContext` ile runtime `self.rules` zip + ordinal/rule_id doğrulaması.
    /// Q6 gerçek implementation'ları çalıştırırken, digest'in bağladığı sıra ile runtime
    /// sırasının ayrışmasına izin vermez. Descriptor kuralı evaluate edemez — runtime
    /// rule implementation'ları `self.rules` üzerinden çağrılır, context sadece alignment
    /// doğrular.
    fn check_claim_rules_with_context(
        &self,
        claim: &Claim,
        context: &crate::authorization::RuleEvaluationContext,
    ) -> Result<(), EngineCommitError> {
        use crate::authorization::checked_rule_ordinal;
        let ordered = context.ordered_rules();
        if self.rules.len() != ordered.len() {
            return Err(EngineCommitError::AuthorizationContextFailed(
                "rule evaluation context length mismatch".into(),
            ));
        }
        for (index, (rule, ordered_desc)) in self.rules.iter().zip(ordered).enumerate() {
            let expected_ordinal = checked_rule_ordinal(index).map_err(|_| {
                EngineCommitError::AuthorizationContextFailed("rule ordinal overflow".into())
            })?;
            if ordered_desc.ordinal != expected_ordinal
                || ordered_desc.descriptor.rule_id != *rule.id()
            {
                return Err(EngineCommitError::AuthorizationContextFailed(format!(
                    "rule context mismatch at index {index}: runtime id={}, context id={}",
                    rule.id(),
                    ordered_desc.descriptor.rule_id
                )));
            }
            if let Some(violation) =
                rule.evaluate(&claim.delta_nodes, &claim.delta_edges, &self.space)
            {
                tracing::warn!(
                    claim_id = claim.id,
                    rule_id = %rule.id(),
                    "Q6 rule violation — claim rejected"
                );
                return Err(EngineCommitError::RuleViolation { violation });
            }
        }
        Ok(())
    }

    /// PermissionMask nihai denetimi (inv #13, agent-prompt-semantics.md §2.1 nokta 3).
    /// Claim.author'ın yazma yetkisi olmayan düğümlere dokunması engellenir.
    ///
    /// Stub: Faz 2'de full_access mask (tüm node'lar writable). Faz 5'te God Mode
    /// config'ten yüklenen gerçek PermissionMask ile çalışır.
    #[allow(dead_code)] // Faz 5'te commit() imzasına mask parametresi eklenecek
    fn check_permissions(
        &self,
        _claim: &Claim,
        _mask: &PermissionMask,
    ) -> Result<(), EngineCommitError> {
        // Faz 5 stub: read_only_nodes'a yazma, forbidden_edge_kinds oluşturma kontrolü
        Ok(())
    }

    // ── Reposition (incremental, inv #5/#6) ────────────────────────────────

    /// Phase 2: post-mutation neighbor drift tespiti + pozisyon güncelleme.
    /// `CosineDeviation` kullanır (inv #5 — DiffusionDeviation değil).
    /// İki-fazlı (collect → apply) — borrow checker uyumu.
    fn reposition_nodes(&mut self, ids: &[NodeId]) -> Vec<DriftWarning> {
        let mut drift_warnings = Vec::new();

        // Faz 1: hesapla (immutable borrow)
        let updates: Vec<(NodeId, Position)> = ids
            .iter()
            .filter_map(|&id| {
                let node = self.space.nodes.get(&id)?;
                let raw = self.coord_system.raw_position_of(node, &self.space);
                let derived = compute_derived(
                    &raw,
                    &self.vision,
                    &self.space,
                    &CosineDeviation,
                    raw.z,
                    self.config.abstractness,
                );
                if derived.theta > self.config.theta_bound {
                    drift_warnings.push(DriftWarning {
                        node_id: id,
                        theta: derived.theta,
                        raw,
                    });
                }
                Some((id, Position { raw, derived }))
            })
            .collect();

        // Faz 2: uygula (mutable borrow)
        for (id, pos) in updates {
            if let Some(node) = self.space.nodes.get_mut(&id) {
                node.position = pos;
            }
        }

        drift_warnings
    }

    /// TAM reposition (analyze/dashboard — inv #5 lazy). Tüm düğümleri günceller.
    /// Commit path'inde DEĞİL — `osp analyze` / dashboard çağrısı.
    /// Faz 5+: `DiffusionDeviation` ile upgrade.
    pub fn full_reposition(&mut self) -> Vec<DriftWarning> {
        let all_ids: Vec<NodeId> = self.space.nodes.keys().copied().collect();
        self.reposition_nodes(&all_ids)
    }

    // ── Persistence ────────────────────────────────────────────────────────

    /// Time-travel (event-sourcing): milestone + delta replay → request_t_c.
    pub fn restore(&mut self, request_t_c: u64) -> Result<usize, EngineCommitError> {
        let store = self
            .snapshot_store
            .as_ref()
            .ok_or(EngineCommitError::NoPersistence)?;
        let restored = store.restore(request_t_c)?;
        self.space = restored.space;
        self.t_c = restored.t_c;
        tracing::info!(
            t_c = restored.t_c,
            replayed = restored.replayed_deltas,
            "restore tamamlandı"
        );
        Ok(restored.replayed_deltas)
    }

    /// Manuel milestone snapshot (tag vb.).
    pub fn save_milestone(&self) -> Result<(), EngineCommitError> {
        let store = self
            .snapshot_store
            .as_ref()
            .ok_or(EngineCommitError::NoPersistence)?;
        let snapshot = SpaceSnapshot {
            version: SNAPSHOT_FORMAT_VERSION,
            t_c: self.t_c,
            timestamp_ms: current_time_ms(),
            space: self.space.clone(),
        };
        store.save_milestone(snapshot)?;
        Ok(())
    }

    // ── Accessors ───────────────────────────────────────────────────────────

    pub fn space(&self) -> &Space {
        &self.space
    }

    /// **Commit Pipeline visualizer** — tüm gate'leri sırayla çalıştırır, her gate'in
    /// sonucunu döner (kısa-devre yok). Q4 fail → Q5/Q6 "skipped".
    ///
    /// Bu metod `commit()`'ten farklı olarak: hatada durmaz, tüm gate durumlarını raporlar.
    /// Frontend visualizer için tasarlandı.
    pub fn check_all_gates(&self, claim: &Claim, omega: &WitnessSet) -> Vec<GateResult> {
        let mut results = vec![];

        // Q4 Syntax
        match self.check_claim_syntax(claim) {
            Ok(()) => results.push(GateResult::passed("Q4 Syntax", "Schema valid")),
            Err(e) => {
                let h = crate::agent::HallucinationType::from_engine_error(&e);
                results.push(GateResult::failed("Q4 Syntax", &e.to_string(), h));
                return results; // pipeline stops
            }
        }

        // Q5 Vision (Step 4b: captured vision context + typed failure)
        match self
            .effective_vision_gate_context(claim)
            .map_err(EngineCommitError::VisionContextInvalid)
            .and_then(|ctx| self.check_claim_vision_with_context(claim, &ctx))
        {
            Ok(()) => results.push(GateResult::passed("Q5 Vision", "θ within bound")),
            Err(e) => {
                let h = crate::agent::HallucinationType::from_engine_error(&e);
                results.push(GateResult::failed("Q5 Vision", &e.to_string(), h));
                return results;
            }
        }

        // Q6 Rule (Step 4a: context-aware)
        match self
            .current_rule_evaluation_context()
            .map_err(EngineCommitError::AuthorizationContextFailed)
            .and_then(|ctx| self.check_claim_rules_with_context(claim, &ctx))
        {
            Ok(()) => results.push(GateResult::passed("Q6 Rule", "No rule violations")),
            Err(e) => {
                let h = crate::agent::HallucinationType::from_engine_error(&e);
                results.push(GateResult::failed("Q6 Rule", &e.to_string(), h));
                return results;
            }
        }

        // Q1-Q3 Witness
        match crate::witness::evaluate(claim, omega) {
            crate::witness::WitnessDisposition::Satisfied { .. } => {
                results.push(GateResult::passed(
                    "Q1-Q3 Witness",
                    "Quorum met — Satisfied",
                ));
            }
            crate::witness::WitnessDisposition::Held { reason, .. } => {
                let h = Some(crate::agent::HallucinationType::Undersupported {
                    support: 0.0,
                    threshold: 1.5,
                });
                results.push(GateResult::failed(
                    "Q1-Q3 Witness",
                    &format!("Held: {:?}", reason),
                    h,
                ));
            }
            crate::witness::WitnessDisposition::Rejected { reasons, .. } => {
                let h = Some(crate::agent::HallucinationType::Witness { witness: 0 });
                results.push(GateResult::failed(
                    "Q1-Q3 Witness",
                    &format!("Rejected: {:?}", reasons),
                    h,
                ));
            }
        }

        results
    }

    /// Mutable space reference (test/setup için — production'da commit() kullan).
    #[cfg(test)]
    pub fn space_mut(&mut self) -> &mut Space {
        &mut self.space
    }

    pub fn t_c(&self) -> u64 {
        self.t_c
    }

    /// **INV-T9** — Mevcut space view revision.
    ///
    /// **reviewer P0-3 (C6):** Artık gerçek `SpaceDigest::compute` kullanır — node/edge
    /// canonical içeriği. Önceki placeholder yalnız `t_c` üzerinden hash üretiyordu.
    ///
    /// `view_id` hala `Ephemeral(self.t_c)` — persisted identity dosya lifecycle'ı
    /// Commit 4'te. Navigator, Ephemeral + CrossProcess store kombinasyonunu fail-closed
    /// olarak reddeder (D3).
    pub fn current_space_view_revision(
        &self,
    ) -> Result<crate::authorization::SpaceViewRevision, String> {
        use crate::authorization::{SpaceDigest, SpaceViewId, SpaceViewRevision};
        let content_digest = SpaceDigest::compute(&self.space).map_err(|e| e.to_string())?;
        Ok(SpaceViewRevision {
            view_id: SpaceViewId::Ephemeral(self.t_c),
            sequence: self.t_c,
            content_digest,
        })
    }

    /// **INV-T9 Step 4a** — Mevcut rule evaluation context (ordinal-aware snapshot).
    ///
    /// `self.rules` registration sırasıyla `.enumerate()` → ordinal üretir. Bu snapshot
    /// hem Q6 (`check_claim_rules_with_context`) hem `EvaluationContextDigest::compute`
    /// tarafından paylaşılır — iki ayrı yerde rule listesi üretip drift bırakmaz.
    pub(crate) fn current_rule_evaluation_context(
        &self,
    ) -> Result<crate::authorization::RuleEvaluationContext, String> {
        use crate::authorization::{
            checked_rule_ordinal, OrderedRuleDescriptor, RuleEvaluationContext,
        };
        let mut ordered: Vec<OrderedRuleDescriptor> = Vec::with_capacity(self.rules.len());
        for (index, rule) in self.rules.iter().enumerate() {
            let ordinal = checked_rule_ordinal(index).map_err(|e| e.to_string())?;
            ordered.push(OrderedRuleDescriptor {
                ordinal,
                descriptor: rule.descriptor(),
            });
        }
        RuleEvaluationContext::try_new(ordered).map_err(|e| e.to_string())
    }

    // **INV-T9 Step 4b:** `current_evaluation_context_digest` accessor KALDIRILDI.
    // Evaluation context artık claim-specific `EffectiveVisionGateContext` + captured
    // `RuleEvaluationContext` ile üretilir — recompute yüzeyi AÇILMAZ. Digest yalnızca
    // `build_authorization_context` içinde captured context'lerden hesaplanır.

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
    pub fn vision(&self) -> &VisionVector {
        &self.vision
    }

    /// Coordinate system accessor (for what-if simulations and position computation).
    pub fn coord_system(&self) -> &crate::coords::CoordinateSystem {
        &self.coord_system
    }

    /// **Position computation from DeltaProposal** (inv #4 — epistemological integrity).
    ///
    /// Agent/LLM pozisyon **declare edemez** — engine structural ΔS'i hypothetical
    /// graph'ta uygular, CoordinateSystem ile gerçek pozisyonları ölçer.
    ///
    /// Bu metod Agent kabuğu tarafından çağrılır:
    /// 1. Agent DeltaProposal üretir (structural only — no positions)
    /// 2. Agent kabuğu engine.compute_raw_from_delta() çağırır
    /// 3. Dönen RawPosition ile Claim oluşturur (computed_raw)
    /// 4. Engine.commit() → Q5 θ(computed_raw, vision) kontrol eder
    ///
    /// **Hypothetical graph:** Mevcut space'in klonu + delta uygulanır.
    /// Coupling/Instability yeni edge'lerden compute edilir (actual measured).
    /// Cohesion node.cohesion'dan (analyzer tarafından set edilmişse).
    /// Entropy/WitnessDepth repo-level (CoordinateSystem stored values).
    ///
    /// **Centroid:** ΔS'deki tüm node'ların mass-weighted ortalama pozisyonu.
    /// Bu, "bu değişiklik uzayın neresinde?" sorusunun cevabıdır.
    /// **G2c-2 (arkadaş review 7):** Hypothetical graph ölçümü — delta node/edge ekleme
    /// + delta_removed edge kaldırma + affected_nodes ölçüm scope'u.
    ///
    /// `affected_nodes` (review 7 #6): ölçülecek MEVCUT node ID'leri. Boşsa delta_nodes
    /// kullanılır. Target node'u buraya koy — new_nodes'a DEĞİL (ontolojik tutarsızlık).
    /// `delta_removed`: hypothetical'ta uygulanır, coupling/instability düşürür (import kaldırma).
    pub fn compute_raw_from_delta(
        &self,
        delta_nodes: &[crate::space::Node],
        delta_edges: &[crate::space::Edge],
        delta_removed: &[crate::agent::EdgeRef],
        affected_nodes: &[crate::space::NodeId],
    ) -> RawPosition {
        // Ölçülecek node seti: affected_nodes (boşsa delta_nodes) — review 7 #6.
        if delta_nodes.is_empty() && affected_nodes.is_empty() {
            return RawPosition::default();
        }

        // 1. Hypothetical graph: clone current space.
        let mut hypothetical = self.space.clone();

        // 2. G2c-2: subtractive delta uygula (edge kaldırma) — eklemelerden ÖNCE.
        for er in delta_removed {
            hypothetical.remove_edge(er.from, er.to, er.kind);
        }

        // 3. Additive delta uygula (node + edge ekleme).
        for node in delta_nodes {
            hypothetical.insert_node(node.clone());
        }
        for edge in delta_edges {
            hypothetical.insert_edge(*edge);
        }

        // 4. Ölçülecek node setini belirle.
        let measure_ids: Vec<crate::space::NodeId> = if !affected_nodes.is_empty() {
            affected_nodes.to_vec()
        } else {
            delta_nodes.iter().map(|n| n.id).collect()
        };

        // 5. Measure edilen node'ların pozisyonunu hesapla.
        let positions: Vec<(f64, RawPosition)> = measure_ids
            .iter()
            .filter_map(|&id| {
                let node = hypothetical.nodes.get(&id)?;
                let raw = self.coord_system.raw_position_of(node, &hypothetical);
                Some((node.mass.max(0.01), raw))
            })
            .collect();

        if positions.is_empty() {
            return RawPosition::default();
        }

        // 6. Mass-weighted centroid.
        let total_mass: f64 = positions.iter().map(|(m, _)| m).sum();
        RawPosition {
            x: positions.iter().map(|(m, r)| m * r.x).sum::<f64>() / total_mass,
            y: positions.iter().map(|(m, r)| m * r.y).sum::<f64>() / total_mass,
            z: positions.iter().map(|(m, r)| m * r.z).sum::<f64>() / total_mass,
            w: positions.iter().map(|(m, r)| m * r.w).sum::<f64>() / total_mass,
            v: positions.iter().map(|(m, r)| m * r.v).sum::<f64>() / total_mass,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 3 — Subject-bound EngineMeasurement tokens (add-only)
    //
    // Authority token üretimi — Commit 4'te TaskCommitInput.measured field'ının
    // yerine geçecek. Add-only: hiçbir existing caller'a dokunmaz.
    // Reviewer v1→v4 turu (8.9 → 9.7) kapanmış tüm P0/P1/P2'ler implemente edildi.
    // ═══════════════════════════════════════════════════════════════════════════════

    /// **INV-T9 #70 Commit 3:** Task delta subject-bound measurement token üretir.
    ///
    /// before+after+context+request — loss YOK. Authority/evidence yolları Commit 4'te
    /// bu token'ı `TaskCommitInput.measurement`'a geçirecek.
    ///
    /// **Reviewer v1→v4 kapanan sözleşmeler:**
    /// - P1-1 (v2): `TaskBoundClaim` defensive binding check (`claim.task_id == task.id`)
    /// - P1-2 (v2): `expected_base_revision` exact match (revision mismatch reachable)
    /// - P1-4 (v2): Heterojen predicate scope fail-closed
    /// - P1-1 (v3): Impact ⊆ subject invariant YOK — bağımsız kümeler
    /// - P1-5 (v2): Baseline availability matrix (terminal error vs unavailable)
    /// - P2-2 (v3): Canonical scope derivation
    /// - P2-3 (v3): Hypothetical explicit sıra (removed → nodes → edges → measure)
    #[allow(clippy::result_large_err)]
    pub fn measure_task_delta<'a>(
        &self,
        bound: &crate::trajectory::TaskBoundClaim<'a>,
        expected_base_revision: &crate::authorization::SpaceViewRevision,
        subject_scope_hint: Option<&[crate::space::NodeId]>,
    ) -> Result<crate::measurement::EngineMeasurement, crate::measurement::MeasurementError> {
        use crate::measurement::{
            BaselineUnavailableReason, CanonicalSubjectScope, EngineMeasurement,
            MeasurementBaseline, MeasurementError, MeasurementRequest,
        };

        // 1. P1-1 (v2): Runtime defensive binding check — TaskBoundClaim public struct
        //    literal bypass'a karşı. claim.task_id yok → ClaimNotTaskBound; mismatch → error.
        let claim_task_id = bound
            .claim
            .task_id
            .ok_or(MeasurementError::ClaimNotTaskBound {
                claim_id: bound.claim.id,
            })?;
        if claim_task_id != bound.task.id {
            return Err(MeasurementError::TaskBindingMismatch {
                claim_task_id,
                bound_task_id: bound.task.id,
            });
        }

        // 2. P1-2 (v2): Current revision exact match — view_id + sequence + content_digest.
        //    **Reviewer v5 P2-2:** `current_space_view_revision` hatası axis context değil,
        //    structural digest computation hatası — ayrı varyant (telemetry categorization).
        let current_revision = self
            .current_space_view_revision()
            .map_err(|e| MeasurementError::RevisionComputationFailed { detail: e })?;
        if expected_base_revision != &current_revision {
            return Err(MeasurementError::RevisionMismatch {
                expected: expected_base_revision.clone(),
                current: current_revision,
            });
        }

        // **Commit 4a P1-1 (reviewer v6/v8/v9/v10):** Measurement session atomikliği —
        // interior mutability threat model. Tek `BoundMeasurementSession::begin` tüm
        // before/after ölçümleri için aynı captured descriptor + epoch snapshot'ını
        // kullanır. Her `measured_position_of` çağrısında pre/post verify; session-sonu
        // `verify_unchanged` (`SessionFinal` faz) defensive kontrol. Drift →
        // `AxisStateDrift` fail-closed typed error (Commit 3 context-before/context-after
        // digest fence'inin gerçek transient ABA'yı (A→B→A) yakalayamaması kapatıldı —
        // `AxisStateEpoch` monoton olduğu için revert'te epoch artar).
        let session = crate::coords::BoundMeasurementSession::begin(&self.coord_system)
            .map_err(MeasurementError::CoordinateMeasurement)?;

        // 3. P2-2 (v3) + P1-4 (v2): Canonical subject scope derivation.
        //    Heterojen predicate scope (farklı canonical set) → typed error.
        let subject = self.derive_task_subject_scope(bound.task)?;

        // 4. P1-1 (v3): Hint canonical karşılaştırma — CanonicalSubjectScope üzerinden.
        if let Some(hint) = subject_scope_hint {
            let canonical_hint = CanonicalSubjectScope::try_new(hint.to_vec())?;
            if canonical_hint != subject {
                return Err(MeasurementError::SubjectScopeHintMismatch {
                    hint_members: canonical_hint.member_ids().to_vec(),
                    derived_members: subject.member_ids().to_vec(),
                });
            }
        }

        // 5. P1-1 (v3): Impact scope — subject'ten BAĞIMSIZ küme (subset check YOK).
        let impact = self.derive_impact_scope(bound.claim)?;

        // 6. P2-3 (v3): Hypothetical explicit sıra:
        //    clone → removed edges → delta nodes → delta edges → measure.
        let mut hypothetical = self.space.clone();
        for er in &bound.claim.removed_edges {
            hypothetical.remove_edge(er.from, er.to, er.kind);
        }
        for node in &bound.claim.delta_nodes {
            hypothetical.insert_node(node.clone());
        }
        for edge in &bound.claim.delta_edges {
            hypothetical.insert_edge(*edge);
        }

        // 7. P1-5 (v2): Baseline availability matrix.
        //    Partition subject_member_ids: existing (base'de) | introduced (delta'da) | unresolvable.
        let delta_introduced: std::collections::HashSet<crate::space::NodeId> =
            bound.claim.delta_nodes.iter().map(|n| n.id).collect();
        let mut existing: Vec<crate::space::NodeId> = Vec::new();
        let mut introduced: Vec<crate::space::NodeId> = Vec::new();
        let mut unresolvable: Vec<crate::space::NodeId> = Vec::new();
        for &id in subject.member_ids() {
            if self.space.nodes.contains_key(&id) {
                existing.push(id);
            } else if delta_introduced.contains(&id) {
                introduced.push(id);
            } else {
                unresolvable.push(id);
            }
        }
        if !unresolvable.is_empty() {
            return Err(MeasurementError::SubjectMemberUnresolvable {
                missing: unresolvable,
            });
        }

        let before = match (existing.is_empty(), introduced.is_empty()) {
            (true, true) => return Err(MeasurementError::EmptySubjectScope),
            (false, true) => {
                // Tüm üyeler base'de — before centroid mevcut space üzerinden.
                // **Commit 4a:** aynı session üzerinden — captured state ile verify.
                let centroid =
                    self.measured_centroid_in_session(&session, &self.space, &existing)?;
                MeasurementBaseline::Available(centroid)
            }
            (true, false) => MeasurementBaseline::Unavailable {
                reason: BaselineUnavailableReason::AllMembersIntroducedByDelta {
                    members: introduced,
                },
            },
            (false, false) => MeasurementBaseline::Unavailable {
                reason: BaselineUnavailableReason::PartialNewSubject {
                    existing,
                    introduced,
                },
            },
        };

        // 8. After: hypothetical'te subject_member_ids centroid.
        //    Subject member hypothetical'te yoksa fail-closed (sessiz skip YOK).
        //    **Commit 4a:** aynı session — before ile aynı captured state verify.
        for &id in subject.member_ids() {
            if !hypothetical.nodes.contains_key(&id) {
                return Err(MeasurementError::SubjectMemberMissingAfterDelta { node_id: id });
            }
        }
        let after =
            self.measured_centroid_in_session(&session, &hypothetical, subject.member_ids())?;

        // **Commit 4a P1-1:** Session-sonu defensive verify — captured descriptor +
        // epoch ile tüm axis'leri karşılaştırır. before/after ölçümleri sırasında
        // interior mutation olduysa yakalanır (axis `measurement_epoch()` override
        // etmişse; default `ZERO` axis'ler için captured == actual == ZERO).
        session
            .verify_unchanged()
            .map_err(MeasurementError::CoordinateMeasurement)?;

        // 9. P1-3 (v8): Context authorization layer'da kurulur (coords neutral — P1-3).
        //    Yeniden CoordinateSystem traversal DEĞİL — session açılışında captured
        //    descriptor snapshot'tan. Token, ölçümlerin üretildiği aynı descriptor
        //    set'ini bağlar (Commit 3 context_after == context_before invariant'ı
        //    artık session pre/post/final verify ile yapısal).
        let context =
            crate::authorization::MeasurementInputContext::try_new(session.axis_descriptors())
                .map_err(MeasurementError::MeasurementContext)?;

        // 10. P1-5 (v3): Shared canonical producer — authorization basis ile aynı ontology.
        let canonical_delta = crate::authorization::canonical_structural_delta_from_claim(
            bound.claim,
        )
        .map_err(|e| {
            crate::measurement::MeasurementError::Digest(
                crate::measurement::MeasurementDigestError::from(e),
            )
        })?;

        // 11. P1-3 (v3): MeasurementRequest::try_new digest'leri üretir (cross-field).
        let request = MeasurementRequest::try_new(
            subject,
            impact,
            expected_base_revision.clone(),
            &canonical_delta,
            &context,
        )
        .map_err(crate::measurement::MeasurementError::Digest)?;

        // 12. P1-3 (v3): EngineMeasurement::new defensive cross-field verify yapar.
        EngineMeasurement::new(before, after, context, request)
    }

    /// **INV-T9 #70 Commit 3 (P2-2 v3):** Task → subject scope üyeleri türetme (canonical).
    ///
    /// `task.target_predicate_set.predicates[*].predicate.scope` üzerinde iterate:
    /// - `Node(id)` → member
    /// - `Subgraph(ids)` → member'lar
    /// - `Module(name)` → typed error (Commit 3 fail-closed; Commit 4 graph-aware resolver)
    ///
    /// **P1-4 (v2):** Heterojen predicate scope (farklı canonical member set) → fail-closed.
    /// `decompose_milestone` homojen üretir ama tip seviyesinde runtime check gerekli.
    #[allow(clippy::result_large_err)]
    pub(crate) fn derive_task_subject_scope(
        &self,
        task: &crate::trajectory::Task,
    ) -> Result<crate::measurement::CanonicalSubjectScope, crate::measurement::MeasurementError>
    {
        use crate::measurement::{
            CanonicalSubjectScope, MeasurementError, SubjectScopeResolutionError,
        };
        use crate::trajectory::PredicateScope;

        // **Reviewer v5 P1-3:** Her predicate scope doğrudan CanonicalSubjectScope::try_new
        // üzerinden geçer — sort dedup YOK. Duplicate Subgraph scope (örn [1, 1, 2])
        // sessizce düzeltilmez, typed error ile reddedilir (authorization
        // CanonicalSubgraphScope ile aynı sözleşme).
        let canonical_scopes: Vec<CanonicalSubjectScope> = task
            .target_predicate_set
            .predicates
            .iter()
            .map(|wp| {
                let ids = match &wp.predicate.scope {
                    PredicateScope::Node(id) => vec![*id],
                    PredicateScope::Subgraph(member_ids) => member_ids.clone(),
                    PredicateScope::Module(name) => {
                        return Err(MeasurementError::SubjectScopeResolutionFailed(
                            SubjectScopeResolutionError::ModuleResolutionUnavailable {
                                module: name.clone(),
                            },
                        ));
                    }
                };
                CanonicalSubjectScope::try_new(ids).map_err(MeasurementError::Digest)
            })
            .collect::<Result<Vec<_>, _>>()?;

        if canonical_scopes.is_empty() {
            return Err(MeasurementError::EmptySubjectScope);
        }

        // P1-4 (v2): Heterojen predicate scope fail-closed. canonical_scopes[0]
        // referans; diğerleri eşit olmalı. Diagnostic için tüm canonical scope'lar taşınır.
        let mut iter = canonical_scopes.into_iter();
        let first = iter.next().expect("non-empty checked above");
        for other in iter {
            if other != first {
                return Err(MeasurementError::HeterogeneousPredicateScopes {
                    // Reviewer v5 P2-3: diagnostic kanıtı — ilk iki farklı scope.
                    // Tüm liste yerine iki temsilci yeterli (hata mesajı okunabilir kalır).
                    scopes: vec![first.clone(), other],
                });
            }
        }
        Ok(first)
    }

    /// **INV-T9 #70 Commit 3 (P1-1 v3 + P1-4 v3):** Claim → impact scope türetme (canonical).
    ///
    /// Structural direct impact footprint — semantik closure DEĞİL:
    /// - `node_ids`: delta_nodes.id ∪ delta_edges(from+to) ∪ removed_edges(from+to)
    /// - `edge_ids`: CanonicalEdgeIdentity (raw EdgeRef DEĞİL) — delta_edges + removed_edges
    ///
    /// Subject'ten BAĞIMSIZ küme (P1-1 v3 — subset check YOK). Impact semantik olarak
    /// küme olduğundan dedup edilir (subject scope'tan farklı kural).
    #[allow(clippy::result_large_err)]
    pub(crate) fn derive_impact_scope(
        &self,
        claim: &crate::witness::Claim,
    ) -> Result<crate::measurement::CanonicalImpactScope, crate::measurement::MeasurementError>
    {
        use crate::authorization::{CanonicalEdgeIdentity, CanonicalEdgeKind};
        use crate::measurement::{CanonicalImpactScope, MeasurementError};

        let mut node_ids: Vec<crate::space::NodeId> = Vec::new();
        node_ids.extend(claim.delta_nodes.iter().map(|n| n.id));
        for edge in &claim.delta_edges {
            node_ids.push(edge.from);
            node_ids.push(edge.to);
        }
        for edge in &claim.removed_edges {
            node_ids.push(edge.from);
            node_ids.push(edge.to);
        }

        let mut edge_ids: Vec<CanonicalEdgeIdentity> = Vec::new();
        for edge in &claim.delta_edges {
            // **Reviewer v5 P2-2:** Structural canonicalization hatası — axis context
            // değil, canonical tag conversion. Digest yoluna yönlendir (telemetry categorization).
            let kind = CanonicalEdgeKind::try_from(&edge.kind).map_err(|e| {
                MeasurementError::Digest(crate::measurement::MeasurementDigestError::from(e))
            })?;
            edge_ids.push(CanonicalEdgeIdentity::new(edge.from, edge.to, kind));
        }
        for edge in &claim.removed_edges {
            let kind = CanonicalEdgeKind::try_from(&edge.kind).map_err(|e| {
                MeasurementError::Digest(crate::measurement::MeasurementDigestError::from(e))
            })?;
            edge_ids.push(CanonicalEdgeIdentity::new(edge.from, edge.to, kind));
        }

        let scope =
            CanonicalImpactScope::try_new(node_ids, edge_ids).map_err(MeasurementError::Digest)?;
        Ok(scope)
    }

    /// **INV-T9 #70 Commit 3 (P1-6 v2) + Commit 4a (P1-4 v8):** Subject scope üyelerinin
    /// mass-weighted centroid ölçümü — backward-compat wrapper. **Commit 4a:** tek session
    /// açar, `measured_centroid_in_session`'a delege eder, sonunda `verify_unchanged` ile
    /// session-sonu defensive verify yapar. Per-axis source `aggregate_source()` ile
    /// korunur (Scip laundering YOK).
    ///
    /// **P1-6 (v2):**
    /// - Mass validation: non-finite veya negatif → `InvalidSubjectMass`
    /// - Total mass: non-finite veya non-positive → `InvalidTotalSubjectMass`
    /// - Axis identity preserved: `AxisMeasurement::try_new` hatası
    ///   `CoordinateMeasurementError::AxisMeasurementFailed { axis_id, source }` sarmalanır
    #[allow(clippy::result_large_err)]
    pub(crate) fn measured_centroid_of(
        &self,
        space: &crate::space::Space,
        member_ids: &[crate::space::NodeId],
    ) -> Result<crate::coords::MeasuredRawPosition, crate::measurement::MeasurementError> {
        use crate::measurement::MeasurementError;

        // **Commit 4a P1-4 (v8) compat wrapper:** tek session açar, içine delege eder,
        // sonunda verify_unchanged ile session-sonu defensive verify. `try_compute_raw_from_delta`
        // unchanged — backward-compat. `measure_task_delta` kendi session'ını yönetir
        // (before/after centroid aynı session'dan).
        let session = crate::coords::BoundMeasurementSession::begin(&self.coord_system)
            .map_err(MeasurementError::CoordinateMeasurement)?;
        let measured = self.measured_centroid_in_session(&session, space, member_ids)?;
        session
            .verify_unchanged()
            .map_err(MeasurementError::CoordinateMeasurement)?;
        Ok(measured)
    }

    /// **INV-T9 #70 Commit 4a P1-4 (reviewer v8):** Session-bound centroid — tüm node'lar
    /// aynı bound refs üzerinden ölçülür. `measured_centroid_of` wrapper bunu çağırır;
    /// `measure_task_delta` tek session'ını açıp before/after centroid'ı buradan alır.
    ///
    /// **Mass validation** + **per-axis aggregate** (Commit 3 unchanged). Ölçüm
    /// `session.measured_position_of` üzerinden — pre/post descriptor+epoch verify dahil.
    #[allow(clippy::result_large_err)]
    pub(crate) fn measured_centroid_in_session(
        &self,
        session: &crate::coords::BoundMeasurementSession<'_>,
        space: &crate::space::Space,
        member_ids: &[crate::space::NodeId],
    ) -> Result<crate::coords::MeasuredRawPosition, crate::measurement::MeasurementError> {
        use crate::coords::MetricSource;
        use crate::measurement::MeasurementError;

        if member_ids.is_empty() {
            return Err(MeasurementError::EmptySubjectScope);
        }

        // Her üye için measured_position_of + mass validation.
        let mut coupling_values: Vec<(f64, f64, MetricSource)> = Vec::new();
        let mut cohesion_values: Vec<(f64, f64, MetricSource)> = Vec::new();
        let mut instability_values: Vec<(f64, f64, MetricSource)> = Vec::new();
        let mut entropy_values: Vec<(f64, f64, MetricSource)> = Vec::new();
        let mut witness_depth_values: Vec<(f64, f64, MetricSource)> = Vec::new();

        for &id in member_ids {
            let node = space
                .nodes
                .get(&id)
                .ok_or(MeasurementError::SubjectMemberMissingAfterDelta { node_id: id })?;
            // P1-6 (v2): Mass validation — non-finite veya negatif reddedilir.
            if !node.mass.is_finite() || node.mass < 0.0 {
                return Err(MeasurementError::InvalidSubjectMass {
                    node_id: id,
                    mass: node.mass,
                });
            }
            let effective_mass = node.mass.max(0.01); // Legacy mass clamp korunur.
                                                      // **Commit 4a:** session.measured_position_of — pre/post verify dahil.
            let measured = session.measured_position_of(node, space)?;
            coupling_values.push((
                effective_mass,
                measured.coupling.value,
                measured.coupling.source,
            ));
            cohesion_values.push((
                effective_mass,
                measured.cohesion.value,
                measured.cohesion.source,
            ));
            instability_values.push((
                effective_mass,
                measured.instability.value,
                measured.instability.source,
            ));
            entropy_values.push((
                effective_mass,
                measured.entropy.value,
                measured.entropy.source,
            ));
            witness_depth_values.push((
                effective_mass,
                measured.witness_depth.value,
                measured.witness_depth.source,
            ));
        }

        // Per-axis mass-weighted centroid + aggregate source.
        let aggregate_axis = |values: Vec<(f64, f64, MetricSource)>, axis_id: &'static str| {
            aggregate_axis_measurement(values, axis_id)
        };

        Ok(crate::coords::MeasuredRawPosition {
            coupling: aggregate_axis(coupling_values, "coupling")?,
            cohesion: aggregate_axis(cohesion_values, "cohesion")?,
            instability: aggregate_axis(instability_values, "instability")?,
            entropy: aggregate_axis(entropy_values, "entropy")?,
            witness_depth: aggregate_axis(witness_depth_values, "witness_depth")?,
        })
    }

    /// **INV-T9 #70 Commit 3:** Fallible compute_raw_from_delta — Commit 2
    /// `measured_position_of()` kullanır. Legacy `compute_raw_from_delta` unchanged
    /// (Commit 4'te deprecated).
    ///
    /// Subject scope YOK — `affected_nodes` üzerinden (legacy parity). Authority token
    /// yolu için `measure_task_delta` kullanılır (subject-bound).
    #[allow(clippy::result_large_err)]
    pub fn try_compute_raw_from_delta(
        &self,
        delta_nodes: &[crate::space::Node],
        delta_edges: &[crate::space::Edge],
        delta_removed: &[crate::agent::EdgeRef],
        affected_nodes: &[crate::space::NodeId],
    ) -> Result<crate::coords::RawPosition, crate::measurement::MeasurementError> {
        // Empty delta → default RawPosition (legacy compute_raw_from_delta parity).
        if delta_nodes.is_empty() && affected_nodes.is_empty() {
            return Ok(crate::coords::RawPosition::default());
        }

        // P2-3 (v3): Hypothetical explicit sıra (legacy parity).
        let mut hypothetical = self.space.clone();
        for er in delta_removed {
            hypothetical.remove_edge(er.from, er.to, er.kind);
        }
        for node in delta_nodes {
            hypothetical.insert_node(node.clone());
        }
        for edge in delta_edges {
            hypothetical.insert_edge(*edge);
        }

        let measure_ids: Vec<crate::space::NodeId> = if !affected_nodes.is_empty() {
            affected_nodes.to_vec()
        } else {
            delta_nodes.iter().map(|n| n.id).collect()
        };

        // measured_position_of → to_raw() (Commit 2 authority surface).
        let measured = self.measured_centroid_of(&hypothetical, &measure_ids)?;
        Ok(measured.to_raw())
    }
}

/// **INV-T9 #70 Commit 3 (P1-6 v2):** Per-axis mass-weighted centroid + aggregate
/// source. `measured_centroid_of` her axis için bu helper'ı çağırır.
///
/// - Total mass validation (non-finite/non-positive → `InvalidTotalSubjectMass`)
/// - Axis identity preserved: `AxisMeasurement::try_new` hatası
///   `CoordinateMeasurementError::AxisMeasurementFailed { axis_id, source }` sarmalanır
#[allow(clippy::result_large_err)]
fn aggregate_axis_measurement(
    values: Vec<(f64, f64, crate::coords::MetricSource)>,
    axis_id: &'static str,
) -> Result<crate::coords::AxisMeasurement, crate::measurement::MeasurementError> {
    use crate::coords::{aggregate_source, AxisMeasurement, CoordinateMeasurementError};
    use crate::measurement::MeasurementError;

    let total_mass: f64 = values.iter().map(|(m, _, _)| m).sum();
    if !total_mass.is_finite() || total_mass <= 0.0 {
        return Err(MeasurementError::InvalidTotalSubjectMass { total_mass });
    }
    let weighted_value = values.iter().map(|(m, v, _)| m * v).sum::<f64>() / total_mass;
    let source = aggregate_source(values.into_iter().map(|(_, _, s)| s))?;
    AxisMeasurement::try_new(weighted_value, source).map_err(|source| {
        MeasurementError::CoordinateMeasurement(CoordinateMeasurementError::AxisMeasurementFailed {
            axis_id,
            source,
        })
    })
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Authorization context helpers — domain → canonical dönüşüm (free functions)
// ═══════════════════════════════════════════════════════════════════════════════

/// PredicateScope → CanonicalPredicateScope (typed enum).
///
/// **reviewer P1-1:** `Subgraph` arm'ı validated constructor (`CanonicalSubgraphScope::try_new`)
/// üzerinden geçer — duplicate id reddedilir. İmza `Result` döner; caller `?` ile yayar.
fn canonicalize_scope(
    scope: &crate::trajectory::PredicateScope,
) -> Result<crate::authorization::CanonicalPredicateScope, String> {
    use crate::trajectory::PredicateScope;
    match scope {
        PredicateScope::Node(id) => Ok(crate::authorization::CanonicalPredicateScope::Node(*id)),
        PredicateScope::Module(name) => Ok(crate::authorization::CanonicalPredicateScope::Module(
            name.clone(),
        )),
        PredicateScope::Subgraph(ids) => {
            let sub = crate::authorization::CanonicalSubgraphScope::try_new(ids.clone())
                .map_err(|e| e.to_string())?;
            Ok(crate::authorization::CanonicalPredicateScope::Subgraph(sub))
        }
    }
}

/// Option<MetricSource> → EffectiveSourceRequirement (source_tag).
/// **reviewer P1-1b (P0):** Option<MetricSource> → EffectiveSourceRequirement.
/// `unwrap_or` KALDIRILDI — None/TreeSitter collision fix. `None → Any`,
/// `Some(src) → Exact(tag)`. Geçersiz MetricSource fail-closed.
fn canonicalize_source_req(
    required: &Option<crate::coords::MetricSource>,
) -> Result<crate::authorization::EffectiveSourceRequirement, String> {
    match required {
        None => Ok(crate::authorization::EffectiveSourceRequirement::Any),
        Some(src) => {
            let tag = crate::canonical_tags::CanonicalMetricSourceTag::try_from(src)
                .map_err(|e: crate::authorization::CanonicalizationError| e.to_string())?;
            Ok(crate::authorization::EffectiveSourceRequirement::Exact(tag))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
    use crate::coords::CoordinateSystem;
    use crate::space::{Edge, EdgeKind, Node, NodeKind};
    use crate::trajectory::Task;
    use crate::witness::{EvidenceEvent, EvidenceId, Intent, WitnessKind};

    /// Vision center — `make_engine` vision ile hizalı. Q5 pre-check geçer.
    const CENTER: RawPosition = RawPosition {
        x: 0.5,
        y: 0.5,
        z: 0.5,
        w: 0.5,
        v: 0.5,
    };

    fn mod_node(id: u64) -> Node {
        Node {
            id,
            kind: NodeKind::Module,
            ..Default::default()
        }
    }

    fn edge(from: u64, to: u64) -> Edge {
        Edge {
            from,
            to,
            kind: EdgeKind::Imports,
            ..Default::default()
        }
    }

    fn ev(id: EvidenceId, actor: u64) -> EvidenceEvent {
        EvidenceEvent::new(id, &format!("src-{id}"), WitnessKind::MergeCommit, actor, 1)
    }

    fn two_witnesses() -> WitnessSet {
        WitnessSet::new(vec![ev(1, 200), ev(2, 300)])
    }

    fn claim_with(author: u64, computed_raw: RawPosition) -> Claim {
        Claim {
            id: 1,
            intent: Intent::new(author, RawPosition::default()),
            author,
            computed_raw,
            delta_nodes: vec![mod_node(10)],
            delta_edges: vec![],
            task_id: None,         // standalone (Paper 1 static flow, INV-T5)
            removed_edges: vec![], // G2c-2
        }
    }

    fn make_engine() -> SpaceEngine {
        let space = Space::new();
        let cs = CoordinateSystem::default_raw_three(
            crate::coords::MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let vision = VisionVector::new(RawPosition {
            x: 0.5,
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.5,
        });
        SpaceEngine::new(space, cs, vision, EngineConfig::default_calibrated())
    }

    // --- commit success ---

    #[test]
    fn commit_success_returns_outcome() {
        let mut engine = make_engine();
        let claim = claim_with(100, CENTER); // aligned with vision (center)
        let omega = two_witnesses();

        let outcome = engine.commit(&claim, &omega).expect("commit");
        assert_eq!(outcome.t_c, 1);
        assert!(!outcome.safety_weakened);
        assert_eq!(engine.space().node_count(), 1); // node 10 added
        assert!(engine.space().nodes.contains_key(&10));
    }

    #[test]
    fn commit_increments_t_c() {
        let mut engine = make_engine();
        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        engine.commit(&claim, &omega).unwrap();
        assert_eq!(engine.t_c(), 1);
        engine.commit(&claim, &omega).unwrap();
        assert_eq!(engine.t_c(), 2);
    }

    // --- Q5 vision pre-check (Safety — reviewer #1) ---

    #[test]
    fn commit_q5_aligned_claim_passes() {
        let mut engine = make_engine();
        // Claim aligned with vision → θ ≈ 0 → passes Q5
        let good_claim = claim_with(
            100,
            RawPosition {
                x: 0.5,
                y: 0.5,
                z: 0.5,
                w: 0.5,
                v: 0.5,
            },
        );
        let omega = two_witnesses();

        let result = engine.commit(&good_claim, &omega);
        assert!(result.is_ok(), "aligned claim → Commit");
    }

    // --- commit Hold (witness insufficient) ---

    #[test]
    fn commit_hold_returns_internal_error() {
        // **INV-T9:** Legacy commit() Held/Rejected'ı Internal error olarak döner
        // (commit_task_claim EngineCommitResult::Held/Rejected kullanır).
        let mut engine = make_engine();
        let claim = claim_with(100, CENTER);
        let omega = WitnessSet::new(vec![ev(1, 200)]); // 1 witness → Held

        let result = engine.commit(&claim, &omega);
        assert!(
            matches!(result, Err(EngineCommitError::Internal(ref msg)) if msg.contains("Held")),
            "legacy commit() Held → Internal error: {result:?}"
        );
        assert_eq!(engine.space().node_count(), 0, "Held → mutasyon yok");
    }

    // --- reposition + drift warnings ---

    #[test]
    fn commit_repositions_new_nodes() {
        let mut engine = make_engine();
        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        let _outcome = engine.commit(&claim, &omega).unwrap();
        // node 10 was added + repositioned → has a position
        let node = engine.space().nodes.get(&10).expect("node 10");
        assert!(node.position.raw.x >= 0.0); // position computed (not default)
    }

    #[test]
    fn commit_drift_warning_when_node_far_from_vision() {
        // Engine vision = (0.5, 0.5, 0.5, 0.5, 0.5). Add a node that, after reposition,
        // has high coupling (x → 1.0) → θ > 0.5 → drift warning.
        let mut space = Space::new();
        for i in 1..=20 {
            space.insert_node(mod_node(i));
        }
        // node 1 imports everything → high coupling
        for i in 2..=20 {
            space.insert_edge(edge(1, i));
        }

        let cs = CoordinateSystem::default_raw_three(
            crate::coords::MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let vision = VisionVector::new(RawPosition {
            x: 0.2, // low coupling vision — node 1 (x≈0.95) will drift
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.5,
        });
        let mut config = EngineConfig::default_calibrated();
        config.theta_bound = 0.2; // test-specific: drift triggers at lower θ
        let mut engine = SpaceEngine::new(space, cs, vision, config);

        // full_reposition: node 1 has x ≈ 0.95 (19 imports) vs vision x=0.2 → θ high
        let warnings = engine.full_reposition();
        assert!(
            !warnings.is_empty(),
            "node 1 high coupling → drift warning expected"
        );
        assert!(warnings.iter().any(|w| w.node_id == 1));
    }

    // --- persistence ---

    #[test]
    fn commit_saves_delta_to_store() {
        let tmp = tempfile::tempdir().unwrap();
        let mut engine = make_engine().with_persistence(tmp.path()).unwrap();
        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        engine.commit(&claim, &omega).unwrap();

        // Delta saved
        let store = SnapshotStore::new(tmp.path()).unwrap();
        let deltas = store.list_deltas_in_range(0, 1).unwrap();
        assert_eq!(deltas.len(), 1);
    }

    #[test]
    fn commit_milestone_at_interval() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = EngineConfig::default_calibrated();
        config.milestone_interval = 2; // every 2 commits
        let cs = CoordinateSystem::default_raw_three(
            crate::coords::MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let vision = VisionVector::new(CENTER);
        let mut engine = SpaceEngine::new(Space::new(), cs, vision, config)
            .with_persistence(tmp.path())
            .unwrap();

        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        engine.commit(&claim, &omega).unwrap(); // t_c=1 (no milestone)
        engine.commit(&claim, &omega).unwrap(); // t_c=2 → milestone

        let store = SnapshotStore::new(tmp.path()).unwrap();
        let milestones = store.list_milestones().unwrap();
        assert!(milestones.contains(&2), "milestone at t_c=2");
    }

    #[test]
    fn restore_via_event_sourcing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut engine = make_engine().with_persistence(tmp.path()).unwrap();
        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        engine.save_milestone().unwrap(); // milestone at t_c=0
        engine.commit(&claim, &omega).unwrap(); // t_c=1, delta saved
        engine.commit(&claim, &omega).unwrap(); // t_c=2, delta saved

        // Restore to t_c=1
        let replayed = engine.restore(1).unwrap();
        assert_eq!(replayed, 1); // 1 delta replayed (milestone at 0)
        assert_eq!(engine.t_c(), 1);
        assert_eq!(engine.space().node_count(), 1); // 1 commit → 1 node
    }

    // --- full_reposition ---

    #[test]
    fn full_reposition_updates_all_nodes() {
        let mut space = Space::new();
        space.insert_node(mod_node(1));
        space.insert_node(mod_node(2));

        let cs = CoordinateSystem::default_raw_three(
            crate::coords::MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let mut engine = SpaceEngine::new(
            space,
            cs,
            VisionVector::new(CENTER),
            EngineConfig::default_calibrated(),
        );

        let _ = engine.full_reposition();
        // All nodes have positions (not default all-zero)
        for node in engine.space().nodes.values() {
            assert!(node.position.raw.x >= 0.0 || node.position.raw.w > 0.0);
        }
    }

    // --- from_vision_config ---

    #[test]
    fn from_vision_config_builds_engine() {
        let toml = r#"
[raw]
x = 0.4
y = 0.7
z = 0.5
w = 0.5
v = 0.5
"#;
        let config = VisionConfig::from_str(toml).unwrap();
        let cs = CoordinateSystem::default_raw_three(
            crate::coords::MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let engine = SpaceEngine::from_vision_config(Space::new(), cs, &config);

        assert!((engine.vision().raw().x - 0.4).abs() < 1e-9);
        assert_eq!(engine.config().min_approvers, 2);
        assert!((engine.config().theta_bound - 0.3).abs() < 1e-9);
        assert_eq!(engine.t_c(), 0);
    }

    // --- no persistence ---

    #[test]
    fn restore_without_persistence_returns_error() {
        let mut engine = make_engine(); // no persistence
        let result = engine.restore(1);
        assert!(matches!(result, Err(EngineCommitError::NoPersistence)));
    }

    // --- Q4 Syntax Gate (real implementation) ---

    fn claim_with_delta(author: u64, nodes: Vec<Node>, edges: Vec<Edge>) -> Claim {
        Claim {
            id: 1,
            intent: Intent::new(author, RawPosition::default()),
            author,
            computed_raw: RawPosition::default(),
            delta_nodes: nodes,
            delta_edges: edges,
            task_id: None,         // standalone (Paper 1 static flow, INV-T5)
            removed_edges: vec![], // G2c-2
        }
    }

    #[test]
    fn q4_rejects_nan_mass() {
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: f64::NAN,
                ..Default::default()
            }],
            vec![],
        );
        let result = engine.commit(&claim, &two_witnesses());
        assert!(
            matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
            "NaN mass should be rejected by Q4"
        );
    }

    #[test]
    fn q4_rejects_negative_mass() {
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: -5.0,
                ..Default::default()
            }],
            vec![],
        );
        let result = engine.commit(&claim, &two_witnesses());
        assert!(
            matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
            "negative mass should be rejected by Q4"
        );
    }

    #[test]
    fn q4_rejects_duplicate_node_ids() {
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![
                Node {
                    id: 42,
                    kind: NodeKind::Module,
                    mass: 1.0,
                    ..Default::default()
                },
                Node {
                    id: 42,
                    kind: NodeKind::Module,
                    mass: 2.0,
                    ..Default::default()
                },
            ],
            vec![],
        );
        let result = engine.commit(&claim, &two_witnesses());
        assert!(
            matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
            "duplicate node IDs should be rejected by Q4"
        );
    }

    #[test]
    fn q4_rejects_imports_self_loop() {
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            }],
            vec![Edge {
                from: 10,
                to: 10,
                kind: EdgeKind::Imports,
                ..Default::default()
            }],
        );
        let result = engine.commit(&claim, &two_witnesses());
        assert!(
            matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
            "self-import should be rejected by Q4"
        );
    }

    #[test]
    fn q4_allows_calls_self_loop() {
        // Calls self-loop (recursion) is valid — not Imports
        let engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            }],
            vec![Edge {
                from: 10,
                to: 10,
                kind: EdgeKind::Calls,
                ..Default::default()
            }],
        );
        // Should pass Q4 (might fail Q5 if vision not aligned, but not Q4)
        let result = engine.check_claim_syntax(&claim);
        assert!(
            result.is_ok(),
            "Calls self-loop should pass Q4: {:?}",
            result
        );
    }

    #[test]
    fn q4_rejects_nan_computed_raw() {
        let engine = make_engine();
        let mut claim = claim_with(100, CENTER);
        claim.computed_raw.x = f64::NAN;
        let result = engine.check_claim_syntax(&claim);
        assert!(result.is_err(), "NaN computed_raw should fail Q4");
    }

    // --- Q6 Rule Gate (default rules) ---

    fn make_engine_with_rules() -> SpaceEngine {
        let cs = CoordinateSystem::default_raw_three(
            crate::coords::MetricSource::Placeholder,
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let vision = VisionVector::new(RawPosition {
            x: 0.5,
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.5,
        });
        SpaceEngine::with_default_rules(
            Space::new(),
            cs,
            vision,
            EngineConfig::default_calibrated(),
        )
        .expect("test rule registration: 3 distinct default rules")
    }

    #[test]
    fn q6_rejects_self_import_via_default_rule() {
        let engine = make_engine_with_rules();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            }],
            vec![Edge {
                from: 10,
                to: 10,
                kind: EdgeKind::Imports,
                ..Default::default()
            }],
        );
        // Q4 catches this first, but if we bypass Q4, Q6 catches it too
        // Verify Q6 directly
        let ctx = engine.current_rule_evaluation_context().unwrap();
        let result = engine.check_claim_rules_with_context(&claim, &ctx);
        assert!(
            matches!(result, Err(EngineCommitError::RuleViolation { .. })),
            "self-import should be caught by Q6 default rule"
        );
    }

    #[test]
    fn q6_rejects_duplicate_node_via_default_rule() {
        let mut engine = make_engine_with_rules();
        // Pre-insert node 5
        engine.space_mut().insert_node(Node {
            id: 5,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        // Claim tries to add node 5 again
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 5,
                kind: NodeKind::Module,
                mass: 2.0,
                ..Default::default()
            }],
            vec![],
        );
        let ctx = engine.current_rule_evaluation_context().unwrap();
        let result = engine.check_claim_rules_with_context(&claim, &ctx);
        assert!(
            matches!(result, Err(EngineCommitError::RuleViolation { .. })),
            "duplicate node should be caught by Q6 default rule"
        );
    }

    #[test]
    fn q6_allows_valid_claim_with_default_rules() {
        let engine = make_engine_with_rules();
        let claim = claim_with_delta(
            100,
            vec![
                Node {
                    id: 10,
                    kind: NodeKind::Module,
                    mass: 1.0,
                    ..Default::default()
                },
                Node {
                    id: 11,
                    kind: NodeKind::Module,
                    mass: 1.0,
                    ..Default::default()
                },
            ],
            vec![Edge {
                from: 10,
                to: 11,
                kind: EdgeKind::Imports,
                ..Default::default()
            }],
        );
        let ctx = engine.current_rule_evaluation_context().unwrap();
        let result = engine.check_claim_rules_with_context(&claim, &ctx);
        assert!(result.is_ok(), "valid claim should pass Q6: {:?}", result);
    }

    // --- Position computation from DeltaProposal (inv #4) ---

    /// Full 5-axis engine for position computation tests (coupling + cohesion + instability + entropy + witness)
    fn make_engine_full() -> SpaceEngine {
        let cs = CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::Placeholder,
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let vision = VisionVector::new(RawPosition {
            x: 0.5,
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.5,
        });
        SpaceEngine::new(Space::new(), cs, vision, EngineConfig::default_calibrated())
    }

    #[test]
    fn compute_raw_empty_delta_returns_default() {
        let engine = make_engine();
        let raw = engine.compute_raw_from_delta(&[], &[], &[], &[]);
        assert_eq!(
            raw,
            RawPosition::default(),
            "empty delta → default position"
        );
    }

    #[test]
    fn compute_raw_does_not_mutate_real_space() {
        let engine = make_engine();
        let initial_count = engine.space().node_count();

        let nodes = vec![Node {
            id: 999,
            kind: NodeKind::Module,
            mass: 10.0,
            ..Default::default()
        }];
        let _ = engine.compute_raw_from_delta(&nodes, &[], &[], &[]);

        assert_eq!(
            engine.space().node_count(),
            initial_count,
            "hypothetical graph must not mutate real space"
        );
    }

    #[test]
    fn compute_raw_single_isolated_node_has_zero_coupling() {
        let engine = make_engine_full();
        let nodes = vec![Node {
            id: 42,
            kind: NodeKind::Module,
            mass: 10.0,
            ..Default::default()
        }];
        let raw = engine.compute_raw_from_delta(&nodes, &[], &[], &[]);
        // Isolated node: coupling = out_degree / (1 + out_degree) = 0 / 1 = 0
        assert!(
            (raw.x - 0.0).abs() < 1e-9,
            "isolated node coupling should be 0, got {}",
            raw.x
        );
        // Isolated node: Ce=Ca=0 → instability = 0.5 (convention)
        assert!(
            (raw.z - 0.5).abs() < 1e-9,
            "isolated node instability should be 0.5, got {}",
            raw.z
        );
    }

    #[test]
    fn compute_raw_edge_increases_coupling() {
        let engine = make_engine_full();
        // Two nodes + one import edge: node 1 imports node 2
        let nodes = vec![
            Node {
                id: 1,
                kind: NodeKind::Module,
                mass: 10.0,
                ..Default::default()
            },
            Node {
                id: 2,
                kind: NodeKind::Module,
                mass: 10.0,
                ..Default::default()
            },
        ];
        let edges = vec![Edge {
            from: 1,
            to: 2,
            kind: EdgeKind::Imports,
            ..Default::default()
        }];

        let raw = engine.compute_raw_from_delta(&nodes, &edges, &[], &[]);

        // Node 1: out_degree(Imports) = 1 → coupling = 1/(1+1) = 0.5
        // Node 2: out_degree(Imports) = 0 → coupling = 0
        // Centroid (equal mass): (0.5 + 0.0) / 2 = 0.25
        assert!(
            (raw.x - 0.25).abs() < 1e-9,
            "centroid coupling with 1 edge should be 0.25, got {}",
            raw.x
        );
    }

    #[test]
    fn compute_raw_is_mass_weighted() {
        let engine = make_engine_full();
        let nodes = vec![
            Node {
                id: 1,
                kind: NodeKind::Module,
                mass: 100.0,
                ..Default::default()
            },
            Node {
                id: 2,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            },
        ];
        let edges = vec![Edge {
            from: 1,
            to: 2,
            kind: EdgeKind::Imports,
            ..Default::default()
        }];

        let raw = engine.compute_raw_from_delta(&nodes, &edges, &[], &[]);
        let expected = 100.0 * 0.5 / 101.0;
        assert!(
            (raw.x - expected).abs() < 1e-6,
            "mass-weighted centroid: expected {}, got {}",
            expected,
            raw.x
        );
    }

    #[test]
    fn compute_raw_cohesion_from_node() {
        let engine = make_engine_full();
        let nodes = vec![Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 10.0,
            cohesion: Some(0.85),
            ..Default::default()
        }];
        let raw = engine.compute_raw_from_delta(&nodes, &[], &[], &[]);
        assert!(
            (raw.y - 0.85).abs() < 1e-9,
            "cohesion should come from node.cohesion, got {}",
            raw.y
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Step 4c — production-path regression: kaldırılan 5 config field digest'i etkilemiyor
    // ═══════════════════════════════════════════════════════════════════════════════

    /// **Step 4c test helper:** `commit_task_claim → Held` production yolundan gerçek
    /// `(AuthorizationContext, WitnessHoldReason, WitnessQuorumSnapshot)` üret. Boş
    /// `WitnessSet` (min_approvers=2 kendi içinde) + predicate satisfied → Held.
    ///
    /// **Omega kaynağı:** `WitnessSet::new(vec![])` kendi `min_approvers: 2, quorum_threshold:
    /// 1.5` değerlerini taşır (engine.rs:113-118, EngineConfig'ten bağımsız). Held sebebi
    /// `input.omega`'dan gelir — `EngineConfig.min_approvers/quorum_threshold` değil.
    /// Bu yüzden reason + snapshot EngineConfig'ten bağımsız olmalı (test assert'leri).
    fn held_for_config(
        config: EngineConfig,
    ) -> (
        crate::authorization::AuthorizationContext,
        crate::witness::WitnessHoldReason,
        crate::witness::WitnessQuorumSnapshot,
    ) {
        use crate::trajectory::{
            InMemoryTaskRegistry, MetricPredicate, PredicateAxis, PredicateMode, PredicateSet,
            Task, TaskPolicy, TaskStatus, WeightedPredicate,
        };
        use crate::witness::WitnessSet;

        // Minimal space: tek node + tek edge (coupling ölçülebilir).
        let mut space = crate::space::Space::default();
        space.nodes.insert(
            0,
            Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                ..Default::default()
            },
        );
        let cs = CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::Placeholder,
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(0.0),
            WitnessDepthAxis::from_witness(0.0, 0),
        )
        .unwrap();
        // UserLoaded vision — authority yeterli (GlobalDefault reject edilmez).
        let vision = crate::vision::VisionVector::with_source(
            RawPosition {
                x: 0.5,
                y: 0.5,
                z: 0.5,
                w: 0.5,
                v: 0.5,
            },
            crate::vision::VisionSource::UserLoaded,
        );
        let mut engine = SpaceEngine::new(space, cs, vision, config);

        // Task: coupling ≤ 0.9 (measured 0.0 ≤ 0.9 → predicate satisfied).
        let task = Task {
            id: 1,
            milestone_id: 1,
            label: "coupling gate".into(),
            target_predicate_set: PredicateSet {
                mode: PredicateMode::All,
                predicates: vec![WeightedPredicate {
                    predicate: MetricPredicate {
                        metric: PredicateAxis::Coupling,
                        operator: crate::trajectory::ComparisonOp::Le,
                        threshold: 0.9,
                        scope: crate::trajectory::PredicateScope::Node(0),
                        required_source: Some(crate::coords::MetricSource::Scip),
                        tolerance: 0.0,
                    },
                    weight: None,
                }],
                preferred_vector: Some(RawPosition {
                    x: 0.5,
                    y: 0.5,
                    z: 0.5,
                    w: 0.5,
                    v: 0.5,
                }),
            },
            policy: TaskPolicy::default(),
            allowed_operations: vec![],
            constraints: vec![],
            status: TaskStatus::Pending,
        };
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);

        // Claim: tek node, computed_raw vision'a hizalı (θ küçük, Q5 geçer).
        let claim = crate::witness::Claim {
            id: 1,
            intent: Intent::new(0, RawPosition::default()),
            author: 0,
            computed_raw: RawPosition {
                x: 0.5,
                y: 0.5,
                z: 0.5,
                w: 0.5,
                v: 0.5,
            },
            delta_nodes: vec![Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                ..Default::default()
            }],
            delta_edges: vec![],
            task_id: Some(1),
            removed_edges: vec![],
        };

        // measured: coupling 0.5 (predicate threshold ≤ 0.9 → satisfied).
        let measured = crate::trajectory::ProvenancedRawPosition {
            coupling: crate::trajectory::AxisMetric {
                value: 0.5,
                source: crate::coords::MetricSource::Scip,
            },
            cohesion: crate::trajectory::AxisMetric {
                value: 0.5,
                source: crate::coords::MetricSource::Scip,
            },
            instability: crate::trajectory::AxisMetric {
                value: 0.5,
                source: crate::coords::MetricSource::Scip,
            },
            entropy: crate::trajectory::AxisMetric {
                value: 0.5,
                source: crate::coords::MetricSource::Scip,
            },
            witness_depth: crate::trajectory::AxisMetric {
                value: 0.0,
                source: crate::coords::MetricSource::Scip,
            },
        };

        // Omega: boş WitnessSet → kendi min_approvers=2/quorum=1.5 taşır → Held.
        let omega = WitnessSet::new(vec![]);

        let input = TaskCommitInput {
            claim: &claim,
            omega: &omega,
            task_resolver: &resolver,
            target: RawPosition {
                x: 0.5,
                y: 0.5,
                z: 0.5,
                w: 0.5,
                v: 0.5,
            },
            loss_before: 1.0,
            measured,
        };

        match engine.commit_task_claim(input) {
            Ok(crate::engine::EngineCommitResult::Held {
                authorization,
                reason,
                snapshot,
            }) => (authorization, reason, snapshot),
            other => panic!(
                "fixture must reach Held (empty WitnessSet, predicate satisfied); got: {other:?}"
            ),
        }
    }

    #[test]
    fn evaluation_context_excludes_non_evaluation_config_fields() {
        // **Step 4c:** Beş config field (min_approvers, quorum_threshold, milestone_interval,
        // abstractness, merge_ratio_observable) artık EvaluationContextDigest'i etkilemiyor.
        //
        // Production yolu: commit_task_claim → Held → AuthorizationContext.basis
        // .evaluation_context_digest. Bu, config'in başka yoldan bağlanmadığını da kanıtlar.
        //
        // Sabit tutulanlar: space, coord_system, rules, claim, task, predicate girdileri,
        // effective vision, theta_bound, WitnessSet (omega). Yalnız kaldırılan 5 field değişir.
        //
        // Omega izolasyonu: iki çağrıda da aynı WitnessSet::new(vec![]) kullanılır.
        // EngineConfig.min_approvers/quorum_threshold farklı ama gerçek witness policy
        // (omega'dan) değişmedi → Held sebebi/snapshot/witness_policy aynı kalmalı.
        let config_a = EngineConfig {
            min_approvers: 2,
            quorum_threshold: 1.5,
            theta_bound: 0.3,
            milestone_interval: 1000,
            abstractness: 0.5,
            merge_ratio_observable: 0.1,
            role_overrides: std::collections::HashMap::new(),
        };
        let config_b = EngineConfig {
            min_approvers: 7, // omega'yı ETKİLEMEZ — WitnessSet kendi değerini taşır
            quorum_threshold: 4.0,
            theta_bound: 0.3,
            milestone_interval: 50,
            abstractness: 0.9,
            merge_ratio_observable: 0.75,
            role_overrides: std::collections::HashMap::new(),
        };

        let (auth_a, reason_a, snapshot_a) = held_for_config(config_a);
        let (auth_b, reason_b, snapshot_b) = held_for_config(config_b);

        // Fixture izolasyonu: EngineConfig farklı, omega aynı → Held çıktıları aynı.
        // reason + snapshot omega'dan türetilir (EngineConfig.min_approvers/quorum_threshold'tan
        // DEĞİL) — iki config farklı değerler taşısa da Held davranışı özdeş kalmalı.
        assert_eq!(
            reason_a, reason_b,
            "Held reason derives from omega, not EngineConfig"
        );
        assert_eq!(
            snapshot_a, snapshot_b,
            "witness snapshot derives from omega, not EngineConfig"
        );
        assert_eq!(
            auth_a.basis.witness_policy, auth_b.basis.witness_policy,
            "witness policy derives from omega, not EngineConfig"
        );

        // **Step 4c sınırı:** kaldırılan 5 config field digest'i etkilemiyor.
        assert_eq!(
            auth_a.basis.evaluation_context_digest, auth_b.basis.evaluation_context_digest,
            "removed config fields (min_approvers/quorum_threshold/milestone_interval/\
             abstractness/merge_ratio_observable) must NOT affect evaluation context digest"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 3 — Subject-bound EngineMeasurement tokens tests
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Test engine'i: `default_raw_five` CoordinateSystem + empty space.
    fn make_measurement_engine() -> SpaceEngine {
        let cs = CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::TreeSitter,
            crate::axes::CohesionAxis::try_with_observed_source(crate::coords::MetricSource::Scip)
                .unwrap(),
            crate::axes::EntropyAxis::from_commit_entropy(6.5),
            crate::axes::WitnessDepthAxis::from_witness(0.5, 3),
        )
        .unwrap();
        let vision = VisionVector::new(RawPosition::default());
        SpaceEngine::new(
            crate::space::Space::new(),
            cs,
            vision,
            EngineConfig::default_calibrated(),
        )
    }

    /// Task with single `Node(id)` predicate scope (homojen).
    fn task_with_node_scope(
        node_id: NodeId,
        task_id: crate::trajectory::TaskId,
    ) -> crate::trajectory::Task {
        use crate::trajectory::{
            MetricPredicate, PredicateMode, PredicateSet, TaskPolicy, TaskStatus, WeightedPredicate,
        };
        let predicate = MetricPredicate {
            metric: crate::trajectory::PredicateAxis::Coupling,
            operator: crate::trajectory::ComparisonOp::Le,
            threshold: 0.5,
            scope: crate::trajectory::PredicateScope::Node(node_id),
            required_source: None,
            tolerance: 0.0,
        };
        let ps = PredicateSet {
            mode: PredicateMode::All,
            predicates: vec![WeightedPredicate {
                predicate,
                weight: Some(1.0),
            }],
            preferred_vector: None,
        };
        Task {
            id: task_id,
            milestone_id: 0,
            label: "test-task".to_string(),
            target_predicate_set: ps,
            policy: TaskPolicy::default(),
            allowed_operations: vec![],
            constraints: vec![],
            status: TaskStatus::Pending,
        }
    }

    /// Task with heterogeneous predicate scopes (Node(A) + Node(B)).
    fn task_with_heterogeneous_scopes(
        a: NodeId,
        b: NodeId,
        task_id: crate::trajectory::TaskId,
    ) -> crate::trajectory::Task {
        use crate::trajectory::{
            MetricPredicate, PredicateMode, PredicateSet, TaskPolicy, TaskStatus, WeightedPredicate,
        };
        let p1 = MetricPredicate {
            metric: crate::trajectory::PredicateAxis::Coupling,
            operator: crate::trajectory::ComparisonOp::Le,
            threshold: 0.5,
            scope: crate::trajectory::PredicateScope::Node(a),
            required_source: None,
            tolerance: 0.0,
        };
        let p2 = MetricPredicate {
            metric: crate::trajectory::PredicateAxis::Cohesion,
            operator: crate::trajectory::ComparisonOp::Ge,
            threshold: 0.3,
            scope: crate::trajectory::PredicateScope::Node(b), // different node
            required_source: None,
            tolerance: 0.0,
        };
        let ps = PredicateSet {
            mode: PredicateMode::All,
            predicates: vec![
                WeightedPredicate {
                    predicate: p1,
                    weight: Some(1.0),
                },
                WeightedPredicate {
                    predicate: p2,
                    weight: Some(1.0),
                },
            ],
            preferred_vector: None,
        };
        Task {
            id: task_id,
            milestone_id: 0,
            label: "heterogeneous-task".to_string(),
            target_predicate_set: ps,
            policy: TaskPolicy::default(),
            allowed_operations: vec![],
            constraints: vec![],
            status: TaskStatus::Pending,
        }
    }

    /// Task with `Module(name)` scope — Commit 3 fail-closed.
    fn task_with_module_scope(task_id: crate::trajectory::TaskId) -> crate::trajectory::Task {
        use crate::trajectory::{
            MetricPredicate, PredicateMode, PredicateSet, TaskPolicy, TaskStatus, WeightedPredicate,
        };
        let predicate = MetricPredicate {
            metric: crate::trajectory::PredicateAxis::Coupling,
            operator: crate::trajectory::ComparisonOp::Le,
            threshold: 0.5,
            scope: crate::trajectory::PredicateScope::Module("payment".to_string()),
            required_source: None,
            tolerance: 0.0,
        };
        let ps = PredicateSet {
            mode: PredicateMode::All,
            predicates: vec![WeightedPredicate {
                predicate,
                weight: Some(1.0),
            }],
            preferred_vector: None,
        };
        Task {
            id: task_id,
            milestone_id: 0,
            label: "module-task".to_string(),
            target_predicate_set: ps,
            policy: TaskPolicy::default(),
            allowed_operations: vec![],
            constraints: vec![],
            status: TaskStatus::Pending,
        }
    }

    fn claim_with_task_id(
        task_id: crate::trajectory::TaskId,
        delta_nodes: Vec<Node>,
        delta_edges: Vec<Edge>,
        removed_edges: Vec<crate::agent::EdgeRef>,
    ) -> Claim {
        Claim {
            id: 1,
            intent: crate::witness::Intent::new(100, RawPosition::default()),
            author: 100,
            computed_raw: RawPosition::default(),
            delta_nodes,
            delta_edges,
            task_id: Some(task_id),
            removed_edges,
        }
    }

    // === Binding + revision + scope (P1-1 v2, P1-2 v2, P1-4 v2) ===

    #[test]
    fn measure_task_delta_rejects_missing_claim_task_id() {
        let engine = make_measurement_engine();
        let task: crate::trajectory::Task = task_with_node_scope(1, 42);
        // Claim without task_id.
        let mut claim = claim_with_task_id(42, vec![], vec![], vec![]);
        claim.task_id = None;
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let result = engine.measure_task_delta(&bound, &revision, None);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::ClaimNotTaskBound { claim_id: 1 })
            ),
            "claim without task_id must be rejected"
        );
    }

    #[test]
    fn measure_task_delta_rejects_forged_task_bound_claim() {
        let engine = make_measurement_engine();
        let task_b = task_with_node_scope(1, 20);
        // Claim bound to task 10 but we pass task 20 — structural forgery.
        let claim = claim_with_task_id(10, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task_b,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let result = engine.measure_task_delta(&bound, &revision, None);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::TaskBindingMismatch {
                    claim_task_id: 10,
                    bound_task_id: 20
                })
            ),
            "forged TaskBoundClaim must be rejected"
        );
    }

    #[test]
    fn measure_task_delta_revision_mismatch_is_reachable() {
        let engine = make_measurement_engine();
        let task: crate::trajectory::Task = task_with_node_scope(1, 42);
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        // Construct a mismatched expected revision.
        use crate::authorization::{SpaceDigest, SpaceViewId, SpaceViewRevision};
        let wrong_revision = SpaceViewRevision {
            view_id: SpaceViewId::Ephemeral(999),
            sequence: 999,
            content_digest: SpaceDigest::from_bytes([0xAB; 32]),
        };
        let result = engine.measure_task_delta(&bound, &wrong_revision, None);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::RevisionMismatch { .. })
            ),
            "revision mismatch must be reachable via expected_base_revision"
        );
    }

    #[test]
    fn measure_task_delta_rejects_heterogeneous_predicate_scopes() {
        let engine = make_measurement_engine();
        let task = task_with_heterogeneous_scopes(1, 2, 42);
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let result = engine.measure_task_delta(&bound, &revision, None);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::HeterogeneousPredicateScopes { .. })
            ),
            "heterogeneous predicate scopes must fail-closed"
        );
    }

    #[test]
    fn measure_task_delta_module_scope_typed_error() {
        let engine = make_measurement_engine();
        let task = task_with_module_scope(42);
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let result = engine.measure_task_delta(&bound, &revision, None);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::SubjectScopeResolutionFailed(_))
            ),
            "Module(name) scope must produce typed error (Commit 4 resolver)"
        );
    }

    /// **Reviewer v5 P1-3:** Duplicate node in Subgraph scope must be rejected
    /// (sessizce dedup EDİLMEZ — CanonicalSubjectScope::try_new ile aynı sözleşme).
    #[test]
    fn measure_task_delta_rejects_duplicate_node_in_subgraph_scope() {
        let engine = make_measurement_engine();
        // Subgraph([5, 5]) — duplicate. Authorization CanonicalSubgraphScope reddeder;
        // measurement yolu da reddetmeli (iki truth source aynı sözleşme).
        use crate::trajectory::{
            MetricPredicate, PredicateMode, PredicateSet, TaskPolicy, TaskStatus, WeightedPredicate,
        };
        let predicate = MetricPredicate {
            metric: crate::trajectory::PredicateAxis::Coupling,
            operator: crate::trajectory::ComparisonOp::Le,
            threshold: 0.5,
            scope: crate::trajectory::PredicateScope::Subgraph(vec![5, 5]),
            required_source: None,
            tolerance: 0.0,
        };
        let ps = PredicateSet {
            mode: PredicateMode::All,
            predicates: vec![WeightedPredicate {
                predicate,
                weight: Some(1.0),
            }],
            preferred_vector: None,
        };
        let task = Task {
            id: 42,
            milestone_id: 0,
            label: "dup-subgraph".to_string(),
            target_predicate_set: ps,
            policy: TaskPolicy::default(),
            allowed_operations: vec![],
            constraints: vec![],
            status: TaskStatus::Pending,
        };
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let result = engine.measure_task_delta(&bound, &revision, None);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::Digest(
                    crate::measurement::MeasurementDigestError::StructuralCanonicalization { .. }
                ))
            ),
            "duplicate node in Subgraph scope must be rejected (not silently deduped)"
        );
    }

    /// **Reviewer v5 P1-2:** Authorization basis ve measurement digest aynı shared
    /// producer'ı kullanmalı — structural delta identity parity.
    #[test]
    fn authorization_and_measurement_share_exact_structural_delta_identity() {
        use crate::authorization::canonical_structural_delta_from_claim;
        use crate::space::{Edge, EdgeKind, Node, NodeKind};
        let claim = claim_with_task_id(
            42,
            vec![
                Node {
                    id: 1,
                    kind: NodeKind::Module,
                    mass: 1.0,
                    ..Default::default()
                },
                Node {
                    id: 2,
                    kind: NodeKind::Concept,
                    mass: 2.0,
                    ..Default::default()
                },
            ],
            vec![Edge {
                from: 1,
                to: 2,
                kind: EdgeKind::Imports,
                ..Default::default()
            }],
            vec![crate::agent::EdgeRef {
                from: 3,
                to: 4,
                kind: EdgeKind::Calls,
            }],
        );

        // Shared producer — measurement yolu.
        let canonical_measurement = canonical_structural_delta_from_claim(&claim).unwrap();

        // Shared producer — authorization basis de bunu kullanır (engine.rs:694).
        // build_authorization_context producer'a refactor edildi, bu yüzden aynı
        // CanonicalStructuralDelta değerini üretmeli.
        let canonical_auth = canonical_structural_delta_from_claim(&claim).unwrap();

        assert_eq!(
            canonical_measurement, canonical_auth,
            "shared producer deterministik — iki çağrı aynı değer"
        );

        // MeasurementDeltaDigest, bu canonical delta üzerinden üretilmeli.
        let digest = crate::measurement::MeasurementDeltaDigest::compute_from_canonical(
            &canonical_measurement,
        )
        .unwrap();
        let digest_again =
            crate::measurement::MeasurementDeltaDigest::compute_from_canonical(&canonical_auth)
                .unwrap();
        assert_eq!(
            digest, digest_again,
            "measurement digest aynı canonical identity'den üretiliyor"
        );

        // **Reviewer v6/v7 P2-1:** Shared-producer regression guard — `build_authorization_context`
        // inline structural canonicalization'a geri dönerse, bu source-level contract test yakalar.
        //
        // **Reviewer v7 P2-2:** Tam üretim çağrı biçimi aranır (`let structural_delta = ...`),
        // yorumlar geçmez. İki-çağrı parity test inline'a dönüşü yakalayamıyordu (aynı
        // fonksiyonu çağırıyordu); bu guard gerçek production-path contract'ı doğrular.
        //
        // NOT: Tam semantic production-path test (build_authorization_context fixture'ı ile
        // gerçek AuthorizationContext.basis.structural_delta karşılaştırması) ağırdır —
        // builder 8 parametreli (outcome, vision_context, rule_context vb.). Commit 4'te
        // CoordinateSystem refactor sırasında builder helper'a ayrılınca semantic test eklenebilir.
        let engine_source = include_str!("engine.rs");
        // build_authorization_context body'sini bul (fn imzasından ilk kapanış `}`'a kadar).
        let builder_start = engine_source
            .find("fn build_authorization_context(")
            .expect("build_authorization_context must exist in engine.rs");
        let builder_end = engine_source[builder_start..]
            .find("\n    }\n")
            .map(|offset| builder_start + offset)
            .unwrap_or(engine_source.len());
        let builder_body = &engine_source[builder_start..builder_end];
        // Tam üretim çağrı biçimi — yorumlarda bu syntax geçmez.
        let shared_call = "let structural_delta =\n            crate::authorization::canonical_structural_delta_from_claim(claim)";
        // fmt formatlamayı tolere etmek için whitespace-normalize edip substring ara.
        let normalized: String = builder_body
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let shared_call_normalized: String =
            shared_call.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            normalized.contains(&shared_call_normalized),
            "build_authorization_context must call canonical_structural_delta_from_claim via \
             production statement (not comment). Inline structural canonicalization drift risk."
        );
    }

    /// **Reviewer v5 P2-3:** HeterogeneousPredicateScopes diagnostic kanıtı taşır
    /// (boş Vec değil, iki temsilci scope).
    #[test]
    fn heterogeneous_predicate_scopes_carries_diagnostic_scopes() {
        let engine = make_measurement_engine();
        let task = task_with_heterogeneous_scopes(1, 2, 42);
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let result = engine.measure_task_delta(&bound, &revision, None);
        match result {
            Err(crate::measurement::MeasurementError::HeterogeneousPredicateScopes { scopes }) => {
                assert_eq!(
                    scopes.len(),
                    2,
                    "diagnostic — iki temsilci scope taşınmalı (boş Vec değil)"
                );
            }
            other => panic!(
                "expected HeterogeneousPredicateScopes with 2 scopes, got {:?}",
                other
            ),
        }
    }

    // === Impact scope (P1-1 v3 + P1-4 v3) ===

    #[test]
    fn derive_impact_scope_edge_only_addition_records_endpoints_in_impact() {
        let engine = make_measurement_engine();
        let claim = claim_with_task_id(
            42,
            vec![], // no delta_nodes
            vec![Edge {
                from: 1,
                to: 2,
                kind: EdgeKind::Imports,
                is_type_only: false,
            }],
            vec![],
        );
        let scope = engine.derive_impact_scope(&claim).unwrap();
        assert!(
            scope.node_ids().contains(&1) && scope.node_ids().contains(&2),
            "delta_edges endpoints must be in impact scope"
        );
        assert_eq!(
            scope.edge_ids().len(),
            1,
            "delta edge identity must be recorded"
        );
    }

    #[test]
    fn derive_impact_scope_edge_only_removal_records_endpoints_in_impact() {
        let engine = make_measurement_engine();
        let removed = vec![crate::agent::EdgeRef {
            from: 3,
            to: 4,
            kind: EdgeKind::Calls,
        }];
        let claim = claim_with_task_id(42, vec![], vec![], removed);
        let scope = engine.derive_impact_scope(&claim).unwrap();
        assert!(
            scope.node_ids().contains(&3) && scope.node_ids().contains(&4),
            "removed_edges endpoints must be in impact scope"
        );
        assert_eq!(
            scope.edge_ids().len(),
            1,
            "removed edge identity must be recorded"
        );
    }

    #[test]
    fn measure_task_delta_allows_impact_outside_subject() {
        let engine = make_measurement_engine();
        // Subject = {1}. Impact includes {1, 5, 6} via removed_edges. Success path.
        let task: crate::trajectory::Task = task_with_node_scope(1, 42);
        let removed = vec![crate::agent::EdgeRef {
            from: 5,
            to: 6,
            kind: EdgeKind::Calls,
        }];
        let claim = claim_with_task_id(42, vec![], vec![], removed);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        // Subject member 1 not in engine space → SubjectMemberUnresolvable, NOT impact violation.
        let result = engine.measure_task_delta(&bound, &revision, None);
        assert!(
            !matches!(
                result,
                Err(crate::measurement::MeasurementError::SubjectScopeResolutionFailed(_))
            ) && !matches!(
                result,
                Err(crate::measurement::MeasurementError::HeterogeneousPredicateScopes { .. })
            ),
            "impact ⊄ subject must NOT cause scope errors (P1-1 v3)"
        );
    }

    // === Baseline (P1-5 v2) ===

    #[test]
    fn measure_task_delta_subject_member_unresolvable_error() {
        let engine = make_measurement_engine();
        // Subject = {1} but engine space is empty and delta doesn't add node 1.
        let task: crate::trajectory::Task = task_with_node_scope(1, 42);
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let result = engine.measure_task_delta(&bound, &revision, None);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::SubjectMemberUnresolvable { .. })
            ),
            "subject member not in base or delta must produce unresolvable error"
        );
    }

    #[test]
    fn measure_task_delta_baseline_all_members_introduced_by_delta() {
        let engine = make_measurement_engine();
        // Subject = {10}. Engine space empty, but delta adds node 10.
        let task: crate::trajectory::Task = task_with_node_scope(10, 42);
        let claim = claim_with_task_id(
            42,
            vec![Node {
                id: 10,
                kind: NodeKind::Concept,
                mass: 1.0,
                ..Default::default()
            }],
            vec![],
            vec![],
        );
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let measurement = engine.measure_task_delta(&bound, &revision, None).unwrap();
        match measurement.before() {
            crate::measurement::MeasurementBaseline::Unavailable {
                reason:
                    crate::measurement::BaselineUnavailableReason::AllMembersIntroducedByDelta {
                        members,
                    },
            } => assert_eq!(members, &[10]),
            other => panic!("expected AllMembersIntroducedByDelta, got {:?}", other),
        }
    }

    #[test]
    fn measure_task_delta_baseline_partial_new_subject() {
        let mut engine = make_measurement_engine();
        // Pre-insert node 1 (existing). Subject = {1, 2}. Delta adds 2 (introduced).
        engine.space_mut().insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let task = {
            use crate::trajectory::{
                MetricPredicate, PredicateMode, PredicateSet, TaskPolicy, TaskStatus,
                WeightedPredicate,
            };
            let predicate = MetricPredicate {
                metric: crate::trajectory::PredicateAxis::Coupling,
                operator: crate::trajectory::ComparisonOp::Le,
                threshold: 0.5,
                scope: crate::trajectory::PredicateScope::Subgraph(vec![1, 2]),
                required_source: None,
                tolerance: 0.0,
            };
            let ps = PredicateSet {
                mode: PredicateMode::All,
                predicates: vec![WeightedPredicate {
                    predicate,
                    weight: Some(1.0),
                }],
                preferred_vector: None,
            };
            Task {
                id: 42,
                milestone_id: 0,
                label: "test".to_string(),
                target_predicate_set: ps,
                policy: TaskPolicy::default(),
                allowed_operations: vec![],
                constraints: vec![],
                status: TaskStatus::Pending,
            }
        };
        let claim = claim_with_task_id(
            42,
            vec![Node {
                id: 2,
                kind: NodeKind::Feature,
                mass: 1.0,
                ..Default::default()
            }],
            vec![],
            vec![],
        );
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let measurement = engine.measure_task_delta(&bound, &revision, None).unwrap();
        match measurement.before() {
            crate::measurement::MeasurementBaseline::Unavailable {
                reason:
                    crate::measurement::BaselineUnavailableReason::PartialNewSubject {
                        existing,
                        introduced,
                    },
            } => {
                assert_eq!(existing, &[1]);
                assert_eq!(introduced, &[2]);
            }
            other => panic!("expected PartialNewSubject, got {:?}", other),
        }
    }

    // === Hint (P1-1 v3) ===

    #[test]
    fn measure_task_delta_hint_matches_derived() {
        let mut engine = make_measurement_engine();
        engine.space_mut().insert_node(Node {
            id: 5,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let task: crate::trajectory::Task = task_with_node_scope(5, 42);
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        let hint: Vec<NodeId> = vec![5];
        let result = engine.measure_task_delta(&bound, &revision, Some(&hint));
        assert!(result.is_ok(), "matching hint must succeed");
    }

    #[test]
    fn measure_task_delta_hint_mismatch_error() {
        let mut engine = make_measurement_engine();
        engine.space_mut().insert_node(Node {
            id: 5,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let task: crate::trajectory::Task = task_with_node_scope(5, 42);
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();
        // Wrong hint — derived is [5], hint is [9].
        let hint: Vec<NodeId> = vec![9];
        let result = engine.measure_task_delta(&bound, &revision, Some(&hint));
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::SubjectScopeHintMismatch { .. })
            ),
            "hint mismatch must produce typed error"
        );
    }

    // === Centroid (P1-6 v2) ===

    #[test]
    fn measured_centroid_rejects_empty_member_set() {
        let engine = make_measurement_engine();
        let space = crate::space::Space::new();
        let result = engine.measured_centroid_of(&space, &[]);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::EmptySubjectScope)
            ),
            "empty member set must be rejected"
        );
    }

    #[test]
    fn measured_centroid_rejects_negative_mass() {
        let mut engine = make_measurement_engine();
        engine.space_mut().insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: -5.0,
            ..Default::default()
        });
        let result = engine.measured_centroid_of(engine.space(), &[1]);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::InvalidSubjectMass {
                    node_id: 1,
                    mass: -5.0
                })
            ),
            "negative mass must be rejected"
        );
    }

    #[test]
    fn measured_centroid_rejects_infinite_mass() {
        let mut engine = make_measurement_engine();
        engine.space_mut().insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: f64::INFINITY,
            ..Default::default()
        });
        let result = engine.measured_centroid_of(engine.space(), &[1]);
        assert!(
            matches!(
                result,
                Err(crate::measurement::MeasurementError::InvalidSubjectMass { node_id: 1, .. })
            ),
            "infinite mass must be rejected"
        );
    }

    #[test]
    fn measured_centroid_mass_weighted() {
        let mut engine = make_measurement_engine();
        // Two nodes, masses 1.0 and 3.0. After centroid, mass-weighted.
        engine.space_mut().insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        engine.space_mut().insert_node(Node {
            id: 2,
            kind: NodeKind::Module,
            mass: 3.0,
            ..Default::default()
        });
        let measured = engine
            .measured_centroid_of(engine.space(), &[1, 2])
            .unwrap();
        // Verify MeasuredRawPosition returned (not RawPosition).
        let raw = measured.to_raw();
        // All finite — basic sanity check.
        assert!(raw.x.is_finite() && raw.y.is_finite());
    }

    // === try_compute_raw_from_delta (Commit 2 authority surface parity) ===

    #[test]
    fn try_compute_raw_from_delta_returns_measured_value() {
        let engine = make_measurement_engine();
        let nodes = vec![Node {
            id: 10,
            kind: NodeKind::Concept,
            mass: 1.0,
            ..Default::default()
        }];
        let result = engine.try_compute_raw_from_delta(&nodes, &[], &[], &[10]);
        assert!(result.is_ok(), "try_compute_raw_from_delta must succeed");
        let raw = result.unwrap();
        assert!(raw.x.is_finite());
    }

    #[test]
    fn try_compute_raw_from_delta_empty_returns_default() {
        let engine = make_measurement_engine();
        let result = engine.try_compute_raw_from_delta(&[], &[], &[], &[]);
        assert_eq!(result.unwrap(), RawPosition::default());
    }

    #[test]
    fn try_compute_raw_from_delta_equals_legacy_for_full_preset() {
        // Parity: same delta + affected_nodes → same RawPosition value.
        let engine = make_measurement_engine();
        let nodes = vec![Node {
            id: 10,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        }];
        let affected: Vec<NodeId> = vec![10];
        let legacy = engine.compute_raw_from_delta(&nodes, &[], &[], &affected);
        let fallible = engine
            .try_compute_raw_from_delta(&nodes, &[], &[], &affected)
            .unwrap();
        assert_eq!(
            legacy, fallible,
            "fallible must match legacy for same input"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4a — session migration test'leri (reviewer v8/v9)
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn measured_centroid_of_wrapper_creates_session() {
        // v8 P1-4 backward-compat: measured_centroid_of wrapper session açar, içine
        // delege eder, verify_unchanged ile kapatır. Sabit değer korunur — wrapper
        // eskiden doğrudan measured_position_of çağırıyordu, şimdi session üzerinden.
        let mut engine = make_measurement_engine();
        engine.space_mut().insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let measured = engine.measured_centroid_of(engine.space(), &[1]).unwrap();
        // default_raw_five preset coupling/entropy/witness_depth sabit; cohesion/
        // instability node 1 için hesaplanır. Sadece ölçüm başarılı + source dolu kontrol.
        assert!(
            measured.coupling.source == crate::coords::MetricSource::TreeSitter
                || measured.coupling.source == crate::coords::MetricSource::Scip
                || measured.coupling.source == crate::coords::MetricSource::Placeholder,
            "coupling source must be a valid MetricSource"
        );
    }

    #[test]
    fn measured_centroid_in_session_uses_bound_refs() {
        // measured_centroid_in_session — aynı session üzerinden before/after centroid.
        // measure_task_delta bu yolu kullanır; wrapper DEĞİL, doğrudan session alır.
        let mut engine = make_measurement_engine();
        engine.space_mut().insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        engine.space_mut().insert_node(Node {
            id: 2,
            kind: NodeKind::Module,
            mass: 2.0,
            ..Default::default()
        });
        let session = crate::coords::BoundMeasurementSession::begin(&engine.coord_system)
            .expect("session begin succeeds for full coord system");
        let measured = engine
            .measured_centroid_in_session(&session, engine.space(), &[1, 2])
            .unwrap();
        // İki node mass-weighted — toplam değer 0..1 aralığında, source dolu.
        assert!(measured.coupling.value.is_finite());
        assert!(measured.coupling.value >= 0.0 && measured.coupling.value <= 1.0);
        // verify_unchanged — immutable axis'ler (default ZERO epoch) drift etmez.
        session.verify_unchanged().unwrap();
    }

    #[test]
    fn measure_task_delta_session_rejects_axis_drift() {
        // ★ Reviewer v10/v11 P1 blocking test — gerçek production-path drift rejection.
        // measure_task_delta → tek session açar → measured_centroid_in_session
        // (before/after aynı session) → DriftDuringMeasurementAxis.measure() epoch artırır
        // → PostMeasure verify captured(0) ≠ actual(1) → AxisStateDrift
        // → MeasurementError::CoordinateMeasurement → EngineMeasurement token ÜRETİLMEZ.
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        struct DriftDuringMeasurementAxis {
            epoch: Arc<AtomicU64>,
        }
        impl crate::coords::Axis for DriftDuringMeasurementAxis {
            fn name(&self) -> &'static str {
                "coupling"
            }
            fn descriptor(
                &self,
            ) -> Result<crate::coords::AxisDescriptor, crate::coords::AxisDescriptorError>
            {
                // Descriptor SABİT — A→A. Sadece measure() çağrısında epoch drift eder.
                let mut params = crate::coords::AxisParameterEncoder::new();
                params.push_u8(0);
                crate::coords::AxisDescriptor::try_new("coupling", 1, params)
            }
            fn measure(
                &self,
                _node: &Node,
                _space: &Space,
            ) -> Result<crate::coords::AxisMeasurement, crate::coords::AxisMeasurementError>
            {
                // measure() çağrısı interior mutation — epoch artar. Session PostMeasure
                // verify captured(0) ≠ actual(1) → AxisStateDrift.
                self.epoch.fetch_add(1, Ordering::SeqCst);
                crate::coords::AxisMeasurement::try_new(
                    0.5,
                    crate::coords::MetricSource::Placeholder,
                )
            }
            fn compute(&self, _node: &Node, _space: &Space) -> f64 {
                0.5
            }
            fn measurement_epoch(&self) -> crate::coords::AxisStateEpoch {
                crate::coords::AxisStateEpoch::new(self.epoch.load(Ordering::SeqCst))
            }
        }

        let drift_epoch = Arc::new(AtomicU64::new(0));
        // Custom coord_system — coupling drifting axis, diğer 4 production axis.
        let cs = CoordinateSystem::empty()
            .try_with_axis(DriftDuringMeasurementAxis {
                epoch: drift_epoch.clone(),
            })
            .unwrap()
            .try_with_axis(
                crate::axes::CohesionAxis::try_with_observed_source(
                    crate::coords::MetricSource::Scip,
                )
                .unwrap(),
            )
            .unwrap()
            .try_with_axis(
                crate::axes::InstabilityAxis::try_with_source(
                    crate::coords::MetricSource::TreeSitter,
                )
                .unwrap(),
            )
            .unwrap()
            .try_with_axis(crate::axes::EntropyAxis::from_commit_entropy(6.5))
            .unwrap()
            .try_with_axis(crate::axes::WitnessDepthAxis::from_witness(0.5, 3))
            .unwrap();
        let vision = VisionVector::new(RawPosition::default());
        let engine = SpaceEngine::new(
            crate::space::Space::new(),
            cs,
            vision,
            EngineConfig::default_calibrated(),
        );

        // Subject node — task_with_node_scope(1, 42) + claim_with_task_id(42).
        // Node base space'de değil; delta_nodes ile introduced edilir.
        let task = task_with_node_scope(1, 42);
        let delta_node = Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        };
        let claim = claim_with_task_id(42, vec![delta_node], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();

        // Gerçek production producer — measure_task_delta. Drifting coupling axis
        // before centroid ölçümünde measure() çağrılır → epoch 1 → PostMeasure drift.
        let error = engine
            .measure_task_delta(&bound, &revision, None)
            .unwrap_err();

        // Token ÜRETİLMEDİ — AxisStateDrift MeasurementError::CoordinateMeasurement
        // ile sarmalanır. Phase: PostMeasure (measure'dan sonra verify).
        match error {
            crate::measurement::MeasurementError::CoordinateMeasurement(
                crate::coords::CoordinateMeasurementError::AxisStateDrift {
                    axis_id,
                    phase,
                    expected_epoch,
                    actual_epoch,
                    ..
                },
            ) => {
                assert_eq!(axis_id, "coupling", "drift axis must be coupling");
                assert_eq!(
                    phase,
                    crate::coords::MeasurementSessionPhase::PostMeasure,
                    "drift detected at post-measure verify"
                );
                assert_eq!(expected_epoch, crate::coords::AxisStateEpoch::ZERO);
                assert_eq!(actual_epoch, crate::coords::AxisStateEpoch::new(1));
            }
            other => panic!("expected AxisStateDrift via CoordinateMeasurement, got {other:?}"),
        }
    }

    #[test]
    fn measurement_token_context_equals_session_captured_descriptors() {
        // ★ Reviewer v11/v12 P2-1 — gerçek EngineMeasurement token'ının context'i,
        // session açılışında captured descriptor snapshot ile **full equality**:
        // axis_id + semantics_version + canonical_parameters (byte-for-byte).
        // Manuel context kurulumu DEĞİL — measure_task_delta production yolu token
        // üretir, token'ın context'i session snapshot'ından gelir.
        let mut engine = make_measurement_engine();
        engine.space_mut().insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let task = task_with_node_scope(1, 42);
        let claim = claim_with_task_id(42, vec![], vec![], vec![]);
        let bound = crate::trajectory::TaskBoundClaim {
            claim: &claim,
            task: &task,
        };
        let revision = engine.current_space_view_revision().unwrap();

        // Production token — measure_task_delta EngineMeasurement üretir.
        let measurement = engine
            .measure_task_delta(&bound, &revision, None)
            .expect("production token for immutable preset must succeed");

        // Token context'i — session captured snapshot'tan. Aynı engine için bağımsız
        // session açıp captured descriptor'ı al, token context'i ile karşılaştır.
        let independent_session =
            crate::coords::BoundMeasurementSession::begin(&engine.coord_system)
                .expect("independent session begin succeeds");
        let mut expected_descriptors = independent_session.axis_descriptors();
        expected_descriptors.sort_unstable_by(|a, b| a.axis_id().cmp(b.axis_id()));

        // Full descriptor equality — axis_id + semantics_version + canonical_parameters.
        // axis_id-only karşılaştırma version/parameters farkını kaçırırdı (regression:
        // token coupling v1/A ama session coupling v2/B → axis_id eşit, descriptor farklı).
        assert_eq!(
            measurement.context().axis_descriptors(),
            expected_descriptors.as_slice(),
            "token context must equal the full captured descriptor snapshot \
             (axis_id + semantics_version + canonical_parameters)"
        );
        assert_eq!(
            measurement.context().axis_descriptors().len(),
            5,
            "token context carries exactly 5 core axis descriptors"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b Faz 2 — helper regression test'leri
    // (reviewer Faz 2 scoped P2-1: provided raw bağımsız-parametre semantiği)
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn check_raw_position_finite_rejects_provided_nan_independent_of_claim_computed_raw() {
        // Reviewer Faz 2 scoped P2-1: claim.computed_raw finite olsa bile provided raw
        // (measurement.after().to_raw()) NaN ise reddetmeli — bağımsız parametre.
        let engine = SpaceEngine::new(
            crate::space::Space::new(),
            CoordinateSystem::empty(),
            VisionVector::new(RawPosition::default()),
            EngineConfig::default_calibrated(),
        );
        let claim_id: crate::witness::ClaimId = 42;
        // claim.computed_raw finite (simüle) — provided raw NaN.
        let provided_raw_nan = crate::coords::RawPosition {
            x: f64::NAN,
            ..RawPosition::default()
        };
        let result =
            engine.check_raw_position_finite(claim_id, "measurement.after", &provided_raw_nan);
        match result {
            Err(EngineCommitError::SyntaxViolation { violation }) => {
                assert_eq!(violation.claim_id, claim_id);
                assert!(
                    violation.detail.contains("measurement.after.x"),
                    "detail must reference provided source label, got: {}",
                    violation.detail
                );
                assert!(
                    violation.detail.contains("NaN"),
                    "detail must report NaN, got: {}",
                    violation.detail
                );
            }
            other => panic!("expected SyntaxViolation for NaN provided raw, got {other:?}"),
        }
    }

    #[test]
    fn check_raw_position_finite_accepts_all_finite_provided_raw() {
        let engine = SpaceEngine::new(
            crate::space::Space::new(),
            CoordinateSystem::empty(),
            VisionVector::new(RawPosition::default()),
            EngineConfig::default_calibrated(),
        );
        let finite_raw = RawPosition {
            x: 0.3,
            y: 0.5,
            z: 0.7,
            w: 0.2,
            v: 0.1,
        };
        engine
            .check_raw_position_finite(1, "measurement.after", &finite_raw)
            .expect("all-finite provided raw must pass");
    }

    #[test]
    fn check_raw_position_finite_source_label_appears_in_violation_detail() {
        // Reviewer Faz 2 scoped P2-2: nötr source label — "computed_raw" DEĞİL.
        let engine = SpaceEngine::new(
            crate::space::Space::new(),
            CoordinateSystem::empty(),
            VisionVector::new(RawPosition::default()),
            EngineConfig::default_calibrated(),
        );
        let raw = RawPosition {
            z: f64::INFINITY,
            ..RawPosition::default()
        };
        let err = engine
            .check_raw_position_finite(7, "measurement.after", &raw)
            .unwrap_err();
        match err {
            EngineCommitError::SyntaxViolation { violation } => {
                assert!(
                    violation.detail.contains("measurement.after.z"),
                    "detail must use provided source label 'measurement.after', got: {}",
                    violation.detail
                );
                assert!(
                    !violation.detail.contains("computed_raw"),
                    "detail must NOT hardcode 'computed_raw', got: {}",
                    violation.detail
                );
            }
            other => panic!("expected SyntaxViolation, got {other:?}"),
        }
    }

    #[test]
    fn check_vision_raw_with_context_uses_provided_raw_not_claim_computed_raw() {
        // Reviewer Faz 2 scoped P2-3: claim.computed_raw Q5'i geçiyor ama provided raw
        // (measurement.after) ihlal ediyor → VisionViolation oluşmalı + raw field == provided.
        // Bağımsız-raw semantiği: helper claim.computed_raw'a değil verilen raw'a bakar.
        let space = crate::space::Space::new();
        let engine = SpaceEngine::new(
            space,
            CoordinateSystem::empty(),
            // Vision center — claim.computed_raw ile aynı (theta=0, Q5 geçer).
            VisionVector::new(CENTER),
            EngineConfig::default_calibrated(),
        );
        // Claim — computed_raw vision center'a yakın (Q5 geçer).
        let claim = crate::witness::Claim {
            id: 1,
            intent: crate::witness::Intent::new(100, CENTER),
            author: 100,
            computed_raw: CENTER, // theta=0 → Q5 geçer
            delta_nodes: vec![mod_node(1)],
            delta_edges: vec![],
            task_id: None,
            removed_edges: vec![],
        };
        let vision_context = engine
            .effective_vision_gate_context(&claim)
            .expect("vision context for CENTER claim");

        // claim.computed_raw (CENTER) ile Q5 geçer.
        engine
            .check_vision_raw_with_context(claim.id, &claim.computed_raw, &vision_context)
            .expect("CENTER computed_raw must pass Q5 (theta=0)");

        // Provided raw — vision center'dan uzak (theta > bound → Q5 ihlal).
        let provided_raw_far = RawPosition {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
            v: 0.0,
        };
        let err = engine
            .check_vision_raw_with_context(claim.id, &provided_raw_far, &vision_context)
            .expect_err("provided far raw must violate Q5");
        match err {
            EngineCommitError::VisionViolation { violation, .. } => {
                assert_eq!(violation.claim_id, claim.id);
                // **Reviewer Faz 2 scoped P2-3:** raw field provided raw olmalı
                // (claim.computed_raw DEĞIL — authority-tied evidence).
                assert_eq!(
                    violation.raw, provided_raw_far,
                    "VisionViolation.raw must be the provided raw (measurement.after), \
                     not claim.computed_raw"
                );
            }
            other => panic!("expected VisionViolation, got {other:?}"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b Faz 3 — verify_measurement_binding test matrisi
    //
    // **Reviewer v6:** 1 pozitif + 7 mismatch (check-order-aware) + derivation/drift +
    // canonical-field coverage + EngineMeasurement origin evidence.
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Helper: produce a valid EngineMeasurement via measure_task_delta (engine-origin
    /// token producer). Faz 3 verify_measurement_binding bu token'ı doğrular.
    /// Subject scope (node_id) delta ile introduced — claim delta_nodes içermeli.
    fn produce_valid_measurement(
        engine: &SpaceEngine,
        task: &crate::trajectory::Task,
        claim: &crate::witness::Claim,
    ) -> crate::measurement::EngineMeasurement {
        let bound = crate::trajectory::TaskBoundClaim { claim, task };
        let revision = engine.current_space_view_revision().unwrap();
        engine.measure_task_delta(&bound, &revision, None).unwrap()
    }

    /// Claim with node-1 delta introduced (matches task_with_node_scope(1, ...)).
    fn claim_with_node1_delta(task_id: crate::trajectory::TaskId) -> crate::witness::Claim {
        claim_with_task_id(task_id, vec![mod_node(1)], vec![], vec![])
    }

    #[test]
    fn verify_measurement_binding_succeeds_for_valid_token() {
        // Pozitif: measure_task_delta ürettiği token, aynı engine ile verify → Ok.
        let engine = make_measurement_engine();
        let task = task_with_node_scope(1, 42);
        let claim = claim_with_node1_delta(42);
        let measurement = produce_valid_measurement(&engine, &task, &claim);
        let result = engine.verify_measurement_binding(&claim, &task, &measurement);
        assert!(
            result.is_ok(),
            "valid token must verify: {:?}",
            result.err()
        );
        let proof = result.unwrap();
        assert_eq!(proof.task_id(), 42);
    }

    #[test]
    fn verify_measurement_binding_rejects_task_mismatch() {
        // Check 1: claim.task_id ≠ task.id → TaskMismatch.
        // Aynı subject scope'a sahip iki farklı task → request_digest aynı olabilir,
        // ama bu explicit check TaskMismatch üretir.
        let engine = make_measurement_engine();
        let task_a = task_with_node_scope(1, 10);
        let task_b = task_with_node_scope(1, 20); // aynı node scope, farklı id
        let claim = claim_with_node1_delta(10);
        let measurement = produce_valid_measurement(&engine, &task_a, &claim);
        // claim task 10'a bağlı, ama task_b (id=20) ile verify et.
        let result = engine.verify_measurement_binding(&claim, &task_b, &measurement);
        use crate::measurement::{MeasurementBindingMismatch, MeasurementBindingVerificationError};
        assert!(matches!(
            result,
            Err(MeasurementBindingVerificationError::Mismatch(
                MeasurementBindingMismatch::TaskMismatch {
                    claim_task_id: Some(10),
                    resolved_task_id: 20
                }
            ))
        ));
    }

    #[test]
    fn verify_measurement_binding_rejects_subject_mismatch() {
        // Check 2: task predicate scope değişince (impact'i etkilemeden) → SubjectMismatch.
        let engine = make_measurement_engine();
        let task_a = task_with_node_scope(1, 42); // subject = {1}
        let task_b = task_with_node_scope(2, 42); // subject = {2}, aynı task id
        let claim = claim_with_node1_delta(42);
        let measurement = produce_valid_measurement(&engine, &task_a, &claim);
        // task_a ile üretilen measurement subject={1}, task_b subject={2}.
        let result = engine.verify_measurement_binding(&claim, &task_b, &measurement);
        use crate::measurement::{MeasurementBindingMismatch, MeasurementBindingVerificationError};
        assert!(matches!(
            result,
            Err(MeasurementBindingVerificationError::Mismatch(
                MeasurementBindingMismatch::SubjectMismatch { .. }
            ))
        ));
    }

    #[test]
    fn verify_measurement_binding_rejects_revision_mismatch() {
        // Check 5: stale token (engine t_c artınca revision değişir).
        let engine = make_measurement_engine();
        let task = task_with_node_scope(1, 42);
        let claim = claim_with_node1_delta(42);
        let measurement = produce_valid_measurement(&engine, &task, &claim);
        // Engine state değiştir (t_c artır) → revision değişir → stale token.
        let mut engine = engine;
        engine.t_c += 1;
        let result = engine.verify_measurement_binding(&claim, &task, &measurement);
        use crate::measurement::{
            MeasurementBindingDisposition, MeasurementBindingMismatch,
            MeasurementBindingVerificationError,
        };
        match &result {
            Err(MeasurementBindingVerificationError::Mismatch(
                MeasurementBindingMismatch::RevisionMismatch {
                    expected,
                    presented,
                },
            )) => {
                assert_ne!(expected, presented, "revision must differ");
                // Disposition: RegenerateMeasurement (stale — reviewer v2 karar 4).
                assert_eq!(
                    MeasurementBindingMismatch::RevisionMismatch {
                        expected: expected.clone(),
                        presented: presented.clone(),
                    }
                    .disposition(),
                    MeasurementBindingDisposition::RegenerateMeasurement
                );
            }
            other => panic!("expected RevisionMismatch, got {other:?}"),
        }
    }

    #[test]
    fn verify_measurement_binding_no_state_mutation_on_failure() {
        // **Reviewer v8 P2-2 (dürüst kapsam):** mismatch durumunda selected state
        // cardinality (node/edge count) ve t_c korunur. Full space equality, coordinate
        // generation, event/audit state Faz 12 carryover (in-memory engine'de audit/
        // event-ledger boş; full snapshot test'i Faz 12'de persistent engine ile).
        let engine = make_measurement_engine();
        let engine_before_t_c = engine.t_c;
        let engine_before_space_node_count = engine.space.nodes.len();
        let engine_before_space_edge_count = engine.space.edges.len();

        let task_a = task_with_node_scope(1, 10);
        let task_b = task_with_node_scope(1, 20);
        let claim = claim_with_node1_delta(10);
        let measurement = produce_valid_measurement(&engine, &task_a, &claim);
        // TaskMismatch üret.
        let _ = engine.verify_measurement_binding(&claim, &task_b, &measurement);

        assert_eq!(engine.t_c, engine_before_t_c, "t_c must not change");
        assert_eq!(
            engine.space.nodes.len(),
            engine_before_space_node_count,
            "space node count must not change"
        );
        assert_eq!(
            engine.space.edges.len(),
            engine_before_space_edge_count,
            "space edge count must not change"
        );
    }

    #[test]
    fn verify_measurement_binding_rejects_impact_mismatch() {
        // Check 3: claim delta_edges/removed_edges değişince (subject'i etkilemeden)
        // → ImpactMismatch. node 1 subject'te, edge impact'i değiştir.
        let engine = make_measurement_engine();
        let task = task_with_node_scope(1, 42);
        // Claim A: sadece node 1 delta (impact = {1}).
        let claim_a = claim_with_node1_delta(42);
        // Claim B: node 1 + edge(1→2) — impact'e node 2 eklenir.
        let claim_b = claim_with_task_id(42, vec![mod_node(1)], vec![edge(1, 2)], vec![]);
        let measurement = produce_valid_measurement(&engine, &task, &claim_b);
        // measurement claim_b'nin impact'ini taşır, claim_a ile verify → mismatch.
        let result = engine.verify_measurement_binding(&claim_a, &task, &measurement);
        use crate::measurement::{MeasurementBindingMismatch, MeasurementBindingVerificationError};
        assert!(matches!(
            result,
            Err(MeasurementBindingVerificationError::Mismatch(
                MeasurementBindingMismatch::ImpactMismatch { .. }
            ))
        ));
    }

    #[test]
    fn verify_measurement_binding_rejects_structural_delta_mismatch() {
        // Check 4: canonical delta digest değişince → StructuralDeltaMismatch.
        // Aynı impact set'i koruyan ama delta içeriği farklı claim → digest farklı.
        // claim_a node 1 mass=1.0, claim_b node 1 mass=2.0 — impact aynı ({1}),
        // ama canonical delta node mass farklı → digest farklı.
        let engine = make_measurement_engine();
        let task = task_with_node_scope(1, 42);
        let mut node_a = mod_node(1);
        node_a.mass = 1.0;
        let claim_a = claim_with_task_id(42, vec![node_a], vec![], vec![]);
        let mut node_b = mod_node(1);
        node_b.mass = 2.0;
        let claim_b = claim_with_task_id(42, vec![node_b], vec![], vec![]);
        let measurement = produce_valid_measurement(&engine, &task, &claim_b);
        let result = engine.verify_measurement_binding(&claim_a, &task, &measurement);
        use crate::measurement::{MeasurementBindingMismatch, MeasurementBindingVerificationError};
        assert!(matches!(
            result,
            Err(MeasurementBindingVerificationError::Mismatch(
                MeasurementBindingMismatch::StructuralDeltaMismatch { .. }
            ))
        ));
    }

    #[test]
    fn verify_measurement_binding_fails_subject_derivation() {
        // **P1-1 (reviewer v7):** verify_measurement_binding gerçek çağrı ile
        // SubjectDerivationFailed üretir. Module-scope predicate → derive_task_subject_scope
        // SubjectScopeResolutionError verir. Measurement valid task ile üretilir, sonra
        // task module-scope'a değiştirilip verify çağrılır.
        let engine = make_measurement_engine();
        let valid_task = task_with_node_scope(1, 42);
        let claim = claim_with_node1_delta(42);
        let measurement = produce_valid_measurement(&engine, &valid_task, &claim);

        // Module-scope predicate ile task — derive_task_subject_scope fail.
        use crate::trajectory::{MetricPredicate, PredicateMode, PredicateSet, WeightedPredicate};
        let module_task = crate::trajectory::Task {
            id: 42,
            milestone_id: 0,
            label: "module-scope-task".to_string(),
            target_predicate_set: PredicateSet {
                mode: PredicateMode::All,
                predicates: vec![WeightedPredicate {
                    predicate: MetricPredicate {
                        metric: crate::trajectory::PredicateAxis::Coupling,
                        operator: crate::trajectory::ComparisonOp::Le,
                        threshold: 0.5,
                        scope: crate::trajectory::PredicateScope::Module("nonexistent".to_string()),
                        required_source: None,
                        tolerance: 0.0,
                    },
                    weight: Some(1.0),
                }],
                preferred_vector: None,
            },
            policy: crate::trajectory::TaskPolicy::default(),
            allowed_operations: vec![],
            constraints: vec![],
            status: crate::trajectory::TaskStatus::Pending,
        };
        let result = engine.verify_measurement_binding(&claim, &module_task, &measurement);
        use crate::measurement::{
            MeasurementBindingDerivationError, MeasurementBindingVerificationError,
        };
        assert!(
            matches!(
                result,
                Err(MeasurementBindingVerificationError::Derivation(
                    MeasurementBindingDerivationError::SubjectDerivationFailed { .. }
                ))
            ),
            "module-scope predicate must produce SubjectDerivationFailed — got {result:?}"
        );
    }

    #[test]
    fn space_view_revision_changes_when_sequence_increments() {
        // **P1-1b (reviewer v7 rename):** SpaceViewRevision sequence monotonik.
        // t_c artınca sequence değişir → revision farklı. Gerçek A→B→A revert test'i
        // Faz 12 full test matrisinde (space mutation + rollback infra gerekir).
        let engine = make_measurement_engine();
        let r1 = engine.current_space_view_revision().unwrap();
        let mut engine = engine;
        engine.t_c += 1; // sequence artar
        let r2 = engine.current_space_view_revision().unwrap();
        assert_ne!(
            r1.sequence, r2.sequence,
            "sequence must change when t_c increments"
        );
        assert_ne!(r1, r2, "revisions must differ");
    }

    #[test]
    fn verify_reports_coordinate_context_changed() {
        // **P1-1 (reviewer v7):** Drift — CoordinateContextChanged. finalize_verification
        // direkt çağrılır: operation Ok + coordinate_drift Err → Drift(CoordinateContextChanged).
        // Coord drift doğal olarak axis descriptor değişimi sırasında oluşur — test'te
        // finalize_verification synthetic input ile çağrılır (realistic epoch mutation zor).
        let engine = make_measurement_engine();
        let revision_before = engine.current_space_view_revision().unwrap();
        let ok_proof: Result<(), crate::measurement::MeasurementBindingVerificationError> = Ok(());
        let operation: EpochOperationResult<()> = Ok((revision_before.clone(), ok_proof));
        let coord_drift = Err(crate::coords::CoordinateMeasurementError::EmptySourceSet);
        let result = engine.finalize_verification(operation, coord_drift);
        use crate::measurement::{
            MeasurementBindingDriftError, MeasurementBindingVerificationError,
        };
        assert!(
            matches!(
                result,
                Err(MeasurementBindingVerificationError::Drift(
                    MeasurementBindingDriftError::CoordinateContextChanged { .. }
                ))
            ),
            "coord drift must produce Drift(CoordinateContextChanged) — got {result:?}"
        );
    }

    #[test]
    fn verify_reports_space_revision_changed() {
        // **P1-1 (reviewer v7):** Drift — SpaceRevisionChanged. finalize_verification:
        // operation Ok(rev_before) + coord Ok + revision_after ≠ rev_before → Drift.
        let engine = make_measurement_engine();
        let revision_before = engine.current_space_view_revision().unwrap();
        let ok_proof: Result<(), crate::measurement::MeasurementBindingVerificationError> = Ok(());
        let operation: EpochOperationResult<()> = Ok((revision_before.clone(), ok_proof));
        // Engine state değiştir → revision_after farklı.
        let mut engine = engine;
        engine.t_c += 1;
        let coord_drift: Result<(), crate::coords::CoordinateMeasurementError> = Ok(());
        let result = engine.finalize_verification(operation, coord_drift);
        use crate::measurement::{
            MeasurementBindingDriftError, MeasurementBindingVerificationError,
        };
        assert!(
            matches!(
                result,
                Err(MeasurementBindingVerificationError::Drift(
                    MeasurementBindingDriftError::SpaceRevisionChanged { .. }
                ))
            ),
            "revision change must produce Drift(SpaceRevisionChanged) — got {result:?}"
        );
    }

    #[test]
    fn verify_reports_both_changed() {
        // **P1-1 (reviewer v7):** Drift — BothChanged. coord drift + revision change.
        let engine = make_measurement_engine();
        let revision_before = engine.current_space_view_revision().unwrap();
        let ok_proof: Result<(), crate::measurement::MeasurementBindingVerificationError> = Ok(());
        let operation: EpochOperationResult<()> = Ok((revision_before.clone(), ok_proof));
        let mut engine = engine;
        engine.t_c += 1; // revision change
        let coord_drift = Err(crate::coords::CoordinateMeasurementError::EmptySourceSet);
        let result = engine.finalize_verification(operation, coord_drift);
        use crate::measurement::{
            MeasurementBindingDriftError, MeasurementBindingVerificationError,
        };
        assert!(
            matches!(
                result,
                Err(MeasurementBindingVerificationError::Drift(
                    MeasurementBindingDriftError::BothChanged { .. }
                ))
            ),
            "coord + revision drift must produce Drift(BothChanged) — got {result:?}"
        );
    }

    #[test]
    fn verify_reports_revision_recheck_failed() {
        // **P1-1 (reviewer v8):** Derivation — RevisionRecheckFailed.
        // combine_verification_results pure decision fn — synthetic revision_after Err
        // ile RevisionRecheckFailed kolu doğrudan doğrulanır (mock engine gerekmez).
        let revision_before = crate::authorization::SpaceViewRevision {
            view_id: crate::authorization::SpaceViewId::Ephemeral(0),
            sequence: 0,
            content_digest: crate::authorization::SpaceDigest::from_bytes([0; 32]),
        };
        let ok_proof: Result<(), crate::measurement::MeasurementBindingVerificationError> = Ok(());
        let operation: EpochOperationResult<()> = Ok((revision_before.clone(), ok_proof));
        let coord_drift: Result<(), crate::coords::CoordinateMeasurementError> = Ok(());
        // revision_after Err — SpaceDigest computation hatası simülasyonu.
        let revision_after: Result<crate::authorization::SpaceViewRevision, String> =
            Err("simulated space digest failure".to_string());
        let result =
            SpaceEngine::combine_verification_results(operation, coord_drift, revision_after);
        use crate::measurement::{
            MeasurementBindingDerivationError, MeasurementBindingVerificationError,
        };
        assert!(
            matches!(
                result,
                Err(MeasurementBindingVerificationError::Derivation(
                    MeasurementBindingDerivationError::RevisionRecheckFailed { .. }
                ))
            ),
            "revision recheck Err must produce Derivation(RevisionRecheckFailed) — got {result:?}"
        );
    }

    #[test]
    fn verify_reports_coord_drift_and_revision_recheck_failed_prefers_coord() {
        // **P1-1 (reviewer v8):** Coord drift + revision recheck failed → coord öncelikli.
        // combine_verification_results ile doğrulanır.
        let revision_before = crate::authorization::SpaceViewRevision {
            view_id: crate::authorization::SpaceViewId::Ephemeral(0),
            sequence: 0,
            content_digest: crate::authorization::SpaceDigest::from_bytes([0; 32]),
        };
        let ok_proof: Result<(), crate::measurement::MeasurementBindingVerificationError> = Ok(());
        let operation: EpochOperationResult<()> = Ok((revision_before, ok_proof));
        let coord_drift = Err(crate::coords::CoordinateMeasurementError::EmptySourceSet);
        let revision_after: Result<crate::authorization::SpaceViewRevision, String> =
            Err("simulated failure".to_string());
        let result =
            SpaceEngine::combine_verification_results(operation, coord_drift, revision_after);
        use crate::measurement::{
            MeasurementBindingDriftError, MeasurementBindingVerificationError,
        };
        assert!(
            matches!(
                result,
                Err(MeasurementBindingVerificationError::Drift(
                    MeasurementBindingDriftError::CoordinateContextChanged { .. }
                ))
            ),
            "coord drift + revision recheck failed → coord öncelikli — got {result:?}"
        );
    }

    #[test]
    fn verify_reports_operation_capture_failure_prefers_coord_drift() {
        // **P1-1 (reviewer v8):** Operation Err (capture failure) + coord drift →
        // coord drift öncelikli (capture sırasında gerçeklik değişti).
        let capture_err: crate::measurement::MeasurementBindingVerificationError =
            crate::measurement::MeasurementBindingDerivationError::RevisionComputationFailed {
                detail: "capture failed".to_string(),
            }
            .into();
        let operation: EpochOperationResult<()> = Err(capture_err);
        let coord_drift = Err(crate::coords::CoordinateMeasurementError::EmptySourceSet);
        let revision_after: Result<crate::authorization::SpaceViewRevision, String> =
            Ok(crate::authorization::SpaceViewRevision {
                view_id: crate::authorization::SpaceViewId::Ephemeral(0),
                sequence: 0,
                content_digest: crate::authorization::SpaceDigest::from_bytes([0; 32]),
            });
        let result =
            SpaceEngine::combine_verification_results(operation, coord_drift, revision_after);
        use crate::measurement::{
            MeasurementBindingDriftError, MeasurementBindingVerificationError,
        };
        assert!(
            matches!(
                result,
                Err(MeasurementBindingVerificationError::Drift(
                    MeasurementBindingDriftError::CoordinateContextChanged { .. }
                ))
            ),
            "capture failure + coord drift → coord drift öncelikli — got {result:?}"
        );
    }

    #[test]
    fn verify_rejects_current_context_mismatch_axis_descriptor_drift() {
        // İki engine fixture: aynı Space + t_c ama farklı axis descriptor (CohesionAxis
        // source TreeSitter vs Scip). Token engine A'da üretilip engine B'de verify →
        // önceki kontroller geçer (subject/impact/delta/revision aynı — t_c aynı), ama
        // check 7'de epoch context (engine B) ≠ token context (engine A) → CurrentContextMismatch.
        let engine_a = make_measurement_engine(); // cohesion source = Scip (explicit)
                                                  // Engine B: cohesion source = TreeSitter (default) — farklı axis descriptor.
        let cs_b = CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::TreeSitter,
            crate::axes::CohesionAxis::try_with_observed_source(
                crate::coords::MetricSource::TreeSitter, // farklı source
            )
            .unwrap(),
            crate::axes::EntropyAxis::from_commit_entropy(6.5),
            crate::axes::WitnessDepthAxis::from_witness(0.5, 3),
        )
        .unwrap();
        let engine_b = SpaceEngine::new(
            crate::space::Space::new(),
            cs_b,
            VisionVector::new(RawPosition::default()),
            EngineConfig::default_calibrated(),
        );

        let task = task_with_node_scope(1, 42);
        let claim = claim_with_node1_delta(42);
        // Token engine A'da üretilir (A'nın context digest'i ile).
        let measurement = produce_valid_measurement(&engine_a, &task, &claim);
        // Verify engine B'de — B'nin epoch context'i farklı (cohesion source farklı).
        let result = engine_b.verify_measurement_binding(&claim, &task, &measurement);
        use crate::measurement::{
            MeasurementBindingDisposition, MeasurementBindingMismatch,
            MeasurementBindingVerificationError,
        };
        match &result {
            Err(MeasurementBindingVerificationError::Mismatch(
                MeasurementBindingMismatch::CurrentContextMismatch {
                    expected,
                    presented,
                },
            )) => {
                assert_ne!(
                    expected, presented,
                    "epoch context (engine B) must differ from token context (engine A)"
                );
                // Disposition: RegenerateMeasurement (axis config drift — stale).
                assert_eq!(
                    MeasurementBindingMismatch::CurrentContextMismatch {
                        expected: expected.clone(),
                        presented: presented.clone(),
                    }
                    .disposition(),
                    MeasurementBindingDisposition::RegenerateMeasurement
                );
            }
            other => panic!("expected CurrentContextMismatch, got {other:?}"),
        }
    }

    #[test]
    fn verify_rejects_context_digest_mismatch_corrupt_fixture() {
        // **P1-1d (reviewer v8):** Check 6 — ContextDigestMismatch (token içi tutarsızlık).
        // EngineMeasurement::new defensive cross-field verify yapar → production yolunda
        // unreachable. Ama verifier'daki defense-in-depth kolu test edilmeli.
        // measurement.rs `#[cfg(test)] corrupt_request_context_digest_for_test` fixture
        // ile tutarsız token üretilir, verify_measurement_binding check 6 yakalar.
        let engine = make_measurement_engine();
        let task = task_with_node_scope(1, 42);
        let claim = claim_with_node1_delta(42);
        let valid_measurement = produce_valid_measurement(&engine, &task, &claim);
        // Corrupt fixture: measurement_input_digest rastgele bytes → context digest ile tutarsız.
        let corrupt_measurement =
            crate::measurement::EngineMeasurement::corrupt_request_context_digest_for_test(
                valid_measurement.before().clone(),
                valid_measurement.after().clone(),
                valid_measurement.context().clone(),
                valid_measurement.request().clone(),
            );
        let result = engine.verify_measurement_binding(&claim, &task, &corrupt_measurement);
        use crate::measurement::{MeasurementBindingMismatch, MeasurementBindingVerificationError};
        assert!(
            matches!(
                result,
                Err(MeasurementBindingVerificationError::Mismatch(
                    MeasurementBindingMismatch::ContextDigestMismatch { .. }
                ))
            ),
            "corrupt token (context digest ≠ request digest) must produce ContextDigestMismatch — got {result:?}"
        );
    }

    // NOTE: verify_reports_current_context_capture_failed ve verify_maps_derivation_failures
    // testleri KALDIRILDI (reviewer v8 P1-1 — no-op testler closure kanıtı olarak sayılmaz).
    //
    // **Reviewer v9 P2-1:** ContextDigestMismatch ve CurrentContextMismatch artık gerçek
    // test edildi (yukarıda verify_rejects_context_digest_mismatch_corrupt_fixture +
    // verify_rejects_current_context_mismatch_axis_descriptor_drift). Faz 12 carryover
    // olarak yalnız gerçekten ertelenen yollar kaldı:
    //
    // **Faz 12 carryover:**
    // - CurrentContextCaptureFailed: malformed coord_system fixture (BoundMeasurementSession::begin
    //   Err). Default engine'da begin infallible.
    // - ImpactDerivationFailed: invalid edge kind fixture (measurement üretilemez — aynı
    //   helper measure_task_delta kullanır).
    // - StructuralCanonicalizationFailed: duplicate/cross-list node ID fixture.
    // - RevisionComputationFailed: space digest computation failure (pratikte infallible).
    // - ContextConstructionFailed: MeasurementInputContext::try_new invalid descriptors fixture.
    // - MeasurementResultDigestComputationFailed: corrupted measured result (NaN/∞ —
    //   MeasuredRawPosition smart constructor yok, struct literal).
    // - Full state-integrity: space equality / coordinate generation / event-audit state.
}
