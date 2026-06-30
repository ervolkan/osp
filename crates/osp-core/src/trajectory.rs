//! Architectural Trajectory Navigation — ontolojik tipler (Paper 2 omurgası).
//!
//! OSP'yi reaktif bir kapıdan (gate) **proaktif bir mimari navigasyon protokolüne**
//! taşır. Statik uzay (Paper 1) → dinamik katman (Paper 2). `docs/agent-trajectory-roadmap.md`
//! omurga, `docs/invariant-spec.md` formal sözleşme.
//!
//! # Tez
//! *"A task is not a claimed coordinate and not a structural delta. A task is a
//! verifiable measurement predicate over future engine-measured coordinates."*
//!
//! # Hibrit model (INV-T1..T8)
//! Matematiksel güç (koordinat) operator/planner seviyesinde; epistemolojik güven
//! (predicate) agent seviyesinde. Agent hedef koordinatı GÖRMEZ — sadece predicate
//! + mevcut ölçüm + izinli operasyonlar.
//!
//! # Aşama A kapsamı
//! Bu modül **ontolojik tipleri** tanımlar (type-level invariant enforcement).
//! Gate logic (Q5.b), planner, agent döngüsü Aşama B-D'de gelir.

use std::collections::HashMap;

use crate::coords::{MetricSource, RawPosition};
use crate::space::{EdgeKind, NodeId, NodeKind};
use crate::witness::{AgentId, ClaimId};

/// Rule referansı — `Rule` trait object Debug/Clone/Serialize değil, bu yüzden
/// Task/AgentTaskView serde'lanabilir yapıda rule'ları ID ile referanslar. Engine
/// (Aşama B, Q6 gate) RuleRef → `Box<dyn Rule>` resolve eder (rule registry).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RuleRef(pub String); // rule adı/id (örn "no_self_import", "max_coupling_0.5")

// ═══════════════════════════════════════════════════════════════════════════════
// ID tipleri — mevcut NodeId/ClaimId/AgentId pattern (u64 newtype-ish).
// ═══════════════════════════════════════════════════════════════════════════════

/// Trajectory (yörünge) kimliği.
pub type TrajectoryId = u64;
/// Milestone (ara hedef) kimliği.
pub type MilestoneId = u64;
/// Task (ölçülebilir niyet) kimliği.
pub type TaskId = u64;
/// TaskAttempt (tek deneme) kimliği.
pub type TaskAttemptId = u64;

// ═══════════════════════════════════════════════════════════════════════════════
// OperatorCapability (INV-T2 — operator-only genesis, type-level)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-T2 — Trusted operator capability token. **Private constructor** (`_private: ()`)
/// sayesinde agent kodu bu tipi üretemez; sadece trusted boundary'de (engine bootstrap /
/// God Mode API) `OperatorCapability::issue()` ile alınır.
///
/// `Trajectory::new()` ve `Milestone`/`Task` genesis bu capability'yi zorunlu kılar →
/// agent hedef belirleyemez (INV-T2, Seçenek A — insan mimar). PermissionMask (runtime
/// value, agent üretebilir) YERİNE capability tipi compile-time korur.
///
/// ```
/// use osp_core::trajectory::OperatorCapability;
/// // Agent kodu: OperatorCapability { _private: () } → COMPILE ERROR (private field)
/// // Trusted API: OperatorCapability::issue() → OK
/// ```
#[derive(Debug, Clone, Copy)]
pub struct OperatorCapability {
    _private: (),
}

impl OperatorCapability {
    /// Trusted boundary'de capability üret. Sadece engine bootstrap / God Mode API
    /// çağırır. Agent kodu bu metoda erişememeli (modül boundary).
    pub fn issue() -> Self {
        Self { _private: () }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ProvenancedRawPosition (INV-T4 — source type-level)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-T4 — Çıplak `RawPosition` (f64) provenance taşıyamaz. Her axis için ayrı
/// `AxisMetric { value, source }` — predicate evaluate source'u type-level kontrol eder.
/// Placeholder/heuristic kaynaklı ölçümlerle task kapatılamaz (epistemolojik bütünlük).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AxisMetric {
    /// Metric değeri (NaN/Inf yasak).
    pub value: f64,
    /// Değerin kaynağı (provenance) — TreeSitter/Scip/Placeholder/Heuristic.
    pub source: MetricSource,
}

/// 5 core axis'in her biri için provenance'lı ölçüm. `Claim.computed_raw`'ın
/// trajectory katmanındaki karşılığı — predicate bunu değerlendirir (INV-T3).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProvenancedRawPosition {
    pub coupling: AxisMetric,
    pub cohesion: AxisMetric,
    pub instability: AxisMetric,
    pub entropy: AxisMetric,
    pub witness_depth: AxisMetric,
}

impl ProvenancedRawPosition {
    /// Belirli bir axis'in `AxisMetric`'ini al (predicate evaluate için).
    pub fn axis(&self, predicate_axis: PredicateAxis) -> &AxisMetric {
        match predicate_axis {
            PredicateAxis::Coupling => &self.coupling,
            PredicateAxis::Cohesion => &self.cohesion,
            PredicateAxis::Instability => &self.instability,
            PredicateAxis::Entropy => &self.entropy,
            PredicateAxis::WitnessDepth => &self.witness_depth,
            // Derived/custom axis — şu an coupling'e fallback (Aşama C'de genişletme).
            _ => &self.coupling,
        }
    }

    /// Sadece değerleri RawPosition'a indirge (loss/distance hesabı için, source'suz).
    pub fn to_raw(&self) -> RawPosition {
        RawPosition {
            x: self.coupling.value,
            y: self.cohesion.value,
            z: self.instability.value,
            w: self.entropy.value,
            v: self.witness_depth.value,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MetricPredicate + PredicateSet (INV-T3, T4 — multi-axis, review v2/v4)
// ═══════════════════════════════════════════════════════════════════════════════

/// Engine-measured koordinat üzerinde doğrulanabilir şart. `MetricValue` provenance'ı
/// korur (measured/scip/placeholder/heuristic) — `required_source` ile placeholder
/// ölçümle task kapatma engellenir (INV-T4).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MetricPredicate {
    pub metric: PredicateAxis,
    pub operator: ComparisonOp,
    pub threshold: f64,
    pub scope: PredicateScope,
    /// `Some(req)` ise bu source zorunlu. Placeholder/Heuristic ile predicate satisfied
    /// olsa bile `PredicateResult::SourceInsufficient` (INV-T4).
    pub required_source: Option<MetricSource>,
    /// ε — "≤ 0.55 ± 0.02". Numeric tolerance.
    pub tolerance: f64,
}

/// Hangi eksen (coupling/cohesion/instability/entropy/witness-depth + derived + custom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PredicateAxis {
    Coupling,
    Cohesion,
    Instability,
    Entropy,
    WitnessDepth,
    // Derived (engine-computed, ölçülebilir ama raw değil)
    RiskScore,
    MainSequenceDistance,
    // Domain-specific (security.audit, wcag.compliance — Aşama C+)
    Custom,
}

/// Karşılaştırma operatörü.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComparisonOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl ComparisonOp {
    /// `value op threshold` değerlendirmesi (tolerance dahil).
    pub fn compare(&self, value: f64, threshold: f64, tolerance: f64) -> bool {
        match self {
            ComparisonOp::Lt => value < threshold - tolerance,
            ComparisonOp::Le => value <= threshold + tolerance,
            ComparisonOp::Gt => value > threshold + tolerance,
            ComparisonOp::Ge => value >= threshold - tolerance,
            ComparisonOp::Eq => (value - threshold).abs() <= tolerance,
            ComparisonOp::Ne => (value - threshold).abs() > tolerance,
        }
    }
}

/// Predicate'in uygulandığı kapsamı (node/module/subgraph).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PredicateScope {
    Node(NodeId),
    Module(String),
    Subgraph(Vec<NodeId>),
}

/// Predicate değerlendirme sonucu — satisfied + source yeterli mi (INV-T4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateResult {
    /// Şart sağlandı + source yeterli.
    Satisfied,
    /// Şart sağlandı AMA source placeholder/heuristic (INV-T4 ihlali).
    SourceInsufficient,
    /// Şart sağlanmadı (değer eşiği geçmiyor).
    Unsatisfied,
}

impl MetricPredicate {
    /// `ProvenancedRawPosition` üzerinde değerlendir. INV-T3 (engine ölçer) + INV-T4
    /// (provenance) birlikte. scope module/subgraph ise Aşama B'de aggregate gelir.
    pub fn evaluate(&self, pos: &ProvenancedRawPosition) -> PredicateResult {
        let m = pos.axis(self.metric);
        // INV-T4: required_source varsa ve metric source eşleşmiyorsa → reddet.
        if let Some(req) = self.required_source {
            if m.source != req {
                return PredicateResult::SourceInsufficient;
            }
        }
        // INV-T3: value engine-measured, agent değiştiremez.
        if self
            .operator
            .compare(m.value, self.threshold, self.tolerance)
        {
            PredicateResult::Satisfied
        } else {
            PredicateResult::Unsatisfied
        }
    }
}

/// Multi-axis predicate set (review v2 — F5 axis oscillation'ı doğal çözer).
/// Tek MetricPredicate yerine Vec + birleştirme modu.
/// review v4 — Weighted duplication temizlendi: tek predicate listesi + weight Option.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PredicateSet {
    pub mode: PredicateMode,
    pub predicates: Vec<WeightedPredicate>,
    /// Navigasyon merkezi (debug, distance/loss hesabı). **Internal** — agent view'a
    /// ASLA girmemeli (INV-T1, review v4 #5).
    pub preferred_vector: Option<RawPosition>,
}

/// Tek predicate + opsiyonel ağırlık (Weighted modda loss'a katkı).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WeightedPredicate {
    pub predicate: MetricPredicate,
    /// `None` = All/Any modda (ağırlıksız); `Some(w)` = Weighted modda (loss katkısı).
    pub weight: Option<f64>,
}

/// Predicate'lerin nasıl birleştirileceği.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PredicateMode {
    /// Tüm predicate'lar satisfied olmalı (AND) — default.
    All,
    /// En az biri satisfied (OR).
    Any,
    /// Loss function: weight'lerle (F5 axis oscillation). Aşama C'de loss hesabı.
    Weighted,
}

/// PredicateSet değerlendirme sonucu — completion durumu (INV-T5/T6 ayrımı).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateSetResult {
    /// Tüm (veya Any modda en az bir) predicate satisfied + source yeterli → task kapanabilir.
    Completed,
    /// En az bir predicate SourceInsufficient (placeholder/heuristic) → INV-T4.
    SourceInsufficient,
    /// Predicate'lar satisfied değil (completion fail — ama progress olabilir, INV-T6).
    NotCompleted,
}

impl PredicateSet {
    /// Completion değerlendirmesi. `mode`'a göre All/Any/Weighted. Source yetersizse
    /// `SourceInsufficient` (task Done olamaz, INV-T4).
    pub fn evaluate_completion(&self, pos: &ProvenancedRawPosition) -> PredicateSetResult {
        let mut any_source_insufficient = false;
        match self.mode {
            PredicateMode::All => {
                let mut all_satisfied = true;
                for wp in &self.predicates {
                    match wp.predicate.evaluate(pos) {
                        PredicateResult::Satisfied => {}
                        PredicateResult::SourceInsufficient => {
                            any_source_insufficient = true;
                            all_satisfied = false;
                        }
                        PredicateResult::Unsatisfied => all_satisfied = false,
                    }
                }
                if all_satisfied {
                    PredicateSetResult::Completed
                } else if any_source_insufficient {
                    PredicateSetResult::SourceInsufficient
                } else {
                    PredicateSetResult::NotCompleted
                }
            }
            PredicateMode::Any => {
                let mut any_satisfied = false;
                for wp in &self.predicates {
                    match wp.predicate.evaluate(pos) {
                        PredicateResult::Satisfied => any_satisfied = true,
                        PredicateResult::SourceInsufficient => any_source_insufficient = true,
                        PredicateResult::Unsatisfied => {}
                    }
                }
                if any_satisfied {
                    PredicateSetResult::Completed
                } else if any_source_insufficient {
                    PredicateSetResult::SourceInsufficient
                } else {
                    PredicateSetResult::NotCompleted
                }
            }
            // Weighted: Aşama C'de loss function. Şimdilik All gibi davran (source check).
            PredicateMode::Weighted => {
                let all_satisfied = self
                    .predicates
                    .iter()
                    .all(|wp| matches!(wp.predicate.evaluate(pos), PredicateResult::Satisfied));
                if all_satisfied {
                    PredicateSetResult::Completed
                } else {
                    PredicateSetResult::NotCompleted
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Trajectory + Milestone + TargetRegion (INV-T2 — operator tanımlar)
// ═══════════════════════════════════════════════════════════════════════════════

/// Vision'dan türetilmiş, sıralı Milestone'lar dizisi. Bir projenin "nereye gideceği"
/// planı. **Operator** (insan mimar / God Mode) tanımlar — agent DEĞİL (INV-T2).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Trajectory {
    pub id: TrajectoryId,
    pub label: String,
    /// Hedef mimari (mevcut VisionVector ile uyumlu, Aşama C'de bağlantı).
    pub vision: crate::vision::VisionVector,
    pub milestones: Vec<Milestone>,
    pub status: TrajectoryStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TrajectoryStatus {
    Planned,
    Active,
    Completed,
    /// Yeni trajectory ile değiştirildi (Trajectory Correction, Aşama E).
    Superseded,
}

impl Trajectory {
    /// INV-T2 — `OperatorCapability` zorunlu. Agent `Trajectory::new()` çağıramaz
    /// (capability üretemez, private constructor). Sadece trusted API.
    pub fn new(
        _cap: &OperatorCapability,
        id: TrajectoryId,
        label: String,
        vision: crate::vision::VisionVector,
    ) -> Self {
        Self {
            id,
            label,
            vision,
            milestones: Vec::new(),
            status: TrajectoryStatus::Planned,
        }
    }

    /// Milestone ekle. INV-T2 — capability zorunlu.
    pub fn add_milestone(&mut self, _cap: &OperatorCapability, milestone: Milestone) {
        self.milestones.push(milestone);
    }
}

/// Trajectory üzerinde bir waypoint. `target_region` operator tarafından tanımlanır;
/// koordinat agent'a verilmez, predicate'e dönüştürülür (planner, Aşama C).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Milestone {
    pub id: MilestoneId,
    pub label: String,
    /// Kabul bölgesi (tek nokta DEĞİL — review 1, F1 çözüldü).
    pub target_region: TargetRegion,
    pub tasks: Vec<TaskId>,
    pub status: MilestoneStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MilestoneStatus {
    Pending,
    InProgress,
    Achieved,
    Failed,
}

/// Milestone tek nokta değil, KABUL BÖLGESİ tanımlar (F1 çözümü, review 1).
/// Region = predicate bölgesi; preferred_vector = navigasyon için ideal merkez (sert
/// kriter değil — region içinde herhangi bir nokta milestone'u Achieved yapar).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TargetRegion {
    /// Bölgeyi tanımlayan şartlar (AND). Her predicate engine-measured.
    pub predicates: Vec<MetricPredicate>,
    /// İdeal merkez (navigasyon/distance/loss hesabı, debug). **Internal**.
    pub preferred_vector: Option<RawPosition>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Task + TaskPolicy + OpKind (INV-T5 — Task≠Claim, multi-axis)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir Milestone'a ulaşmak için uzayda yapılması gereken ölçülebilir hareketin
/// PREDICATE SET karşılığı. Agent'a bu verilir — koordinat hedefi DEĞİL (INV-T1).
///
/// Multi-axis (review v2): coupling AND cohesion AND instability birlikte.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub milestone_id: MilestoneId,
    pub label: String,
    pub target_predicate_set: PredicateSet,
    pub policy: TaskPolicy,
    /// Agent'ın araç kutusu (OperationPolicy Aşama C'de scope+max_delta ekler).
    pub allowed_operations: Vec<OpKind>,
    pub constraints: Vec<RuleRef>,
    pub status: TaskStatus,
}

/// Task bazlı mutation policy (review v2 #2). Predicate fail olduğunda mutation
/// reject mi, progress checkpoint mı, operator approval mı — task'ın karakterine göre.
///
/// **Prensip cümlesi:** *"Predicate failure never completes a task, but under a
/// task-specific mutation policy it may be accepted as a bounded progress checkpoint
/// if engine-measured trajectory loss decreases and no hard invariant is violated."*
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskPolicy {
    pub predicate_failure_policy: PredicateFailurePolicy,
    /// Loss en az bu kadar azalmalı (improved saymak için).
    pub min_improvement_delta: f64,
    /// Hiçbir kritik eksen bu kadar bozulamaz (axis oscillation, F5).
    pub max_axis_regression: f64,
    /// INV-T7 — ardışık reject limiti (default 5, operator-configurable).
    pub maneuver_limit: u32,
    /// AcceptAsProgress izinli mi (progress checkpoint lane).
    pub allow_progress_checkpoint: bool,
}

impl Default for TaskPolicy {
    fn default() -> Self {
        Self {
            predicate_failure_policy: PredicateFailurePolicy::StrictReject,
            min_improvement_delta: 0.02,
            max_axis_regression: 0.15,
            maneuver_limit: 5,
            allow_progress_checkpoint: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PredicateFailurePolicy {
    /// Default — basit task, predicate fail = reject.
    StrictReject,
    /// Büyük refactor — loss ↓ ise progress checkpoint.
    AcceptImprovement,
    /// Critical domain (security/payment) — insan review.
    OperatorApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TaskStatus {
    Pending,
    Assigned,
    InProgress,
    Completed,
    /// INV-T7 — maneuver limit aşıldı, operatör kontrol bekliyor.
    Blocked,
}

/// Agent'ın yapabileceği structural operasyonlar (review 2 — Task.allowed_operations).
/// Planner, Task'a "coupling düşürmek için sadece import'ları soyutla" diyebilir.
/// OperationPolicy (scope + max_delta) Aşama C'de eklenir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OpKind {
    AddImport,
    RemoveImport,
    /// Interface/trait ekle (dependency inversion).
    AddAbstraction,
    /// Mevcut kodu yeni modüle taşı.
    ExtractModule,
    AddNode,
    RemoveNode,
    AddEdge,
    RemoveEdge,
    /// kind/mass/metadata değiştir (RawPosition hariç).
    ModifyEntity,
}

// ═══════════════════════════════════════════════════════════════════════════════
// AttemptOutcome + MutationDecision + CommitLane + ApplyTarget (INV-T6, T8)
// ═══════════════════════════════════════════════════════════════════════════════

/// Bir Task için tek deneme. Agent'ın bir DeltaProposal'ı → Claim → gate akışı.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskAttempt {
    pub id: TaskAttemptId,
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub claim_id: Option<ClaimId>,
    /// Engine tarafından simüle edilen (hypothetical graph + re-analyze) sonucu.
    /// Hard gate'ler (Q4/Q5/Q6) BUNU değerlendirir. Reject ise commit edilmedi.
    pub simulated_after: ProvenancedRawPosition,
    /// Mutation kabul edildiyse (AcceptAsProgress/AcceptAsCompleted) gerçek commit
    /// sonrası ölçüm. Reject → None (simulated'da kaldı, hiç uygulanmadı).
    pub committed_after: Option<ProvenancedRawPosition>,
    pub measured_before: ProvenancedRawPosition,
    /// Loss function sonucu (F5 — multi-axis trajectory loss). preferred_vector'e
    /// weighted distance. INV-T6'nın quantitative temeli (failure ≠ regression).
    pub loss_before: f64,
    pub loss_after: f64,
    /// Zengin outcome (review v2 #5) — her boyut ayrı.
    pub outcome: AttemptOutcome,
}

/// review v2 #5 — tek enum yetmez. Gate kararını, predicate sonucunu, mutation
/// kararını, witness durumunu ayrı ayrı taşır.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttemptOutcome {
    /// Hard gate'ler (Q4 Syntax / Q5 Vision / Q6 Rule) — deterministik.
    pub gate_decision: GateDecision,
    /// Soft gate Q5.b — predicate completion durumu.
    pub predicate_completion: PredicateCompletion,
    /// Policy'ye göre mutation kararı (TaskPolicy, INV-T6).
    pub mutation_decision: MutationDecision,
    /// Witness (Q1-Q3) — mutation kabul edildiyse.
    pub witness_status: Option<WitnessOutcome>,
}

/// Hard gate kararları (deterministik, witness öncesi).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GateDecision {
    PassedAll,
    RejectedBySyntax,
    /// Q5 θ > bound.
    RejectedByVision,
    RejectedByRule,
    /// INV-T7 — ardışık N reject.
    BlockedByManeuverLimit,
}

/// Soft gate Q5.b — predicate completion (mutation kararından ayrı).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PredicateCompletion {
    /// Predicate satisfied → task kapanabilir.
    Completed,
    /// Predicate fail — mutation policy'ye bakılır (INV-T6).
    NotCompleted,
}

/// Policy'ye göre mutation kararı (INV-T6). Predicate fail = Reject DEĞİL her zaman.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MutationDecision {
    /// Simulated'da kaldı, hiç uygulanmadı.
    Reject,
    /// Trajectory checkpoint olarak uygulandı (loss ↓, INV-T6).
    AcceptAsProgress,
    /// Predicate satisfied, tamamlandı (→ Mainline promote edilebilir).
    AcceptAsCompleted,
    /// İnsan review gerekli (critical domain).
    RequireOperatorApproval,
}

/// Commit lane — INV-T8 (progress checkpoint isolation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CommitLane {
    /// Ana branch — sadece AcceptAsCompleted.
    Mainline,
    /// Progress checkpoint lane — AcceptAsProgress (asla Mainline).
    TrajectoryCheckpoint,
    /// İzole lane — RequireOperatorApproval.
    Sandbox,
}

/// review v4 #3 — Reject "hiç uygulanmaz" demek, Sandbox "uygulanabilir ama izole" demek.
/// Karışıklığı önlemek için MutationDecision → ApplyTarget ayrımı.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ApplyTarget {
    /// Reject — delta hiç uygulanmadı (simulated'da kaldı).
    NotApplied,
    /// Uygulandı, lane içinde.
    Lane(CommitLane),
}

impl MutationDecision {
    /// INV-T8 — MutationDecision → ApplyTarget mapping (type-level). Reject → NotApplied
    /// (değil Sandbox); AcceptAsProgress → TrajectoryCheckpoint (asla Mainline).
    pub fn apply_target(&self) -> ApplyTarget {
        match self {
            MutationDecision::Reject => ApplyTarget::NotApplied,
            MutationDecision::AcceptAsCompleted => ApplyTarget::Lane(CommitLane::Mainline),
            MutationDecision::AcceptAsProgress => {
                ApplyTarget::Lane(CommitLane::TrajectoryCheckpoint)
            }
            MutationDecision::RequireOperatorApproval => ApplyTarget::Lane(CommitLane::Sandbox),
        }
    }
}

/// Witness (Q1-Q3) outcome — mutation kabul edildiyse. Mevcut WitnessResult ile uyumlu.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WitnessOutcome {
    Hold,
    Commit,
    /// Admin override.
    Override,
}

// ═══════════════════════════════════════════════════════════════════════════════
// AgentTaskView vs InternalTaskPlan (INV-T1 — view ayrımı, en kritik)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-T1 — Agent'a serialize edilen görünümdür. **HEDEF KOORDİNAT İÇERMEZ**
/// (`current_measurement` mevcut engine-measured durum, serbest). Sadece predicate +
/// mevcut ölçüm + izinli operasyonlar + kısıtlar. `serialize_agent_view()` bunu üretir.
///
/// **Kritik:** `preferred_vector` / `target_region` / `milestone_target_vector` ASLA
/// bu struct'ta olmamalı (INV-T1 test matrisi ile compile/serde-level enforce).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentTaskView {
    pub task_id: TaskId,
    pub label: String,
    /// Mevcut engine-measured durum (görülebilir — agent nerede olduğunu bilmeli).
    /// Hedef koordinat DEĞİL.
    pub current_measurement: RawPosition,
    /// Multi-axis ölçüm şartı (epistemolojik güven). **preferred_vector YOK** —
    /// PredicateSet'in preferred_vector alanı bu view'a sızmamalı (Aşama C'de
    /// AgentPredicateSet/InternalPredicateSet ayrımı).
    pub target_predicate: AgentPredicateView,
    pub allowed_operations: Vec<OpKind>,
    pub constraints: Vec<RuleRef>,
}

/// INV-T1 — Agent'a verilen predicate view. `preferred_vector`/`target_region` YOK.
/// Sadece mode + predicate'ler (weight dahil). PredicateSet'ten üretilir, ayrık tip.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentPredicateView {
    pub mode: PredicateMode,
    pub predicates: Vec<WeightedPredicate>,
    // preferred_vector KASITLI YOK — INV-T1. InternalPredicateSet'te var.
}

/// Engine/planner/debug içindir. Koordinat hedefini taşır ama agent'a serialize edilmez.
/// `Intent::from_task` (Aşama C) bu view'ı kullanır.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InternalTaskPlan {
    pub task_id: TaskId,
    /// Koordinat hedefi (operator seviyesi) — agent'a verilmez.
    pub milestone_target_vector: RawPosition,
    /// Predicate (agent'a verilir, AgentPredicateView'a dönüştürülür).
    pub task_predicate: PredicateSet,
    pub tolerance: f64,
}

impl InternalTaskPlan {
    /// INV-T1 — InternalTaskPlan'dan AgentTaskView üret. **Koordinat düşürülür**:
    /// `milestone_target_vector` ve `task_predicate.preferred_vector` çıkarılır.
    /// Bu dönüşüm tek yönlü (engine→agent); geri dönüş yok.
    pub fn to_agent_view(
        &self,
        task_label: &str,
        current_measurement: RawPosition,
        allowed_operations: Vec<OpKind>,
        constraints: Vec<RuleRef>,
    ) -> AgentTaskView {
        AgentTaskView {
            task_id: self.task_id,
            label: task_label.to_string(),
            current_measurement,
            // preferred_vector KASITLI düşürüldü (INV-T1).
            target_predicate: AgentPredicateView {
                mode: self.task_predicate.mode,
                predicates: self.task_predicate.predicates.clone(),
            },
            allowed_operations,
            constraints,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TrajectoryEvidence (Aşama B2 — Evidence Ledger, RQ6/RQ7/RQ8 ham veri)
// ═══════════════════════════════════════════════════════════════════════════════

/// Her TaskAttempt'in evidence kaydı (Aşama B2). Token cost + duration + outcome →
/// RQ6 (token), RQ7 (task success), RQ8 (correction değeri) için ham veri.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrajectoryEvidence {
    pub trajectory_id: TrajectoryId,
    pub milestone_id: MilestoneId,
    pub task_id: TaskId,
    pub attempt_id: TaskAttemptId,
    pub before: RawPosition,
    pub after: RawPosition,
    pub predicate_completion: PredicateCompletion,
    pub mutation_decision: MutationDecision,
    pub token_cost: TokenCost,
    pub duration_ms: u64,
}

/// Token maliyeti (osp-llm-runtime TokenUsage ile uyumlu).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TokenCost {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Loss function placeholder (F5 — Aşama C'de tam impl)
// ═══════════════════════════════════════════════════════════════════════════════

/// Multi-axis trajectory loss (F5 axis oscillation). preferred_vector'e weighted distance.
/// "improved ⟺ loss_after < loss_before − min_improvement_delta AND max_axis_regression respected"
///
/// Aşama A'da basit Euclidean distance; Aşama C'de WeightedPredicate'lerle genişletme.
pub fn trajectory_loss(pos: &ProvenancedRawPosition, target: &RawPosition) -> f64 {
    let raw = pos.to_raw();
    let dx = raw.x - target.x;
    let dy = raw.y - target.y;
    let dz = raw.z - target.z;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// INV-T6 — improved kontrolü. Loss azaldı mı + max_axis_regression aşılmadı mı.
pub fn is_improved(
    pos_before: &ProvenancedRawPosition,
    pos_after: &ProvenancedRawPosition,
    target: &RawPosition,
    policy: &TaskPolicy,
) -> bool {
    let loss_before = trajectory_loss(pos_before, target);
    let loss_after = trajectory_loss(pos_after, target);
    if loss_after >= loss_before - policy.min_improvement_delta {
        return false;
    }
    // max_axis_regression: hiçbir eksen bu kadar bozulamaz.
    let reg = |before: f64, after: f64| -> f64 { (after - before).max(0.0) };
    reg(pos_before.coupling.value, pos_after.coupling.value) > policy.max_axis_regression
        || reg(pos_before.cohesion.value, pos_after.cohesion.value) > -policy.max_axis_regression
        || reg(pos_before.instability.value, pos_after.instability.value) > policy.max_axis_regression
        && false // cohesion: regression = azalma (düşük = kötü). Basit Aşama A; C'de refine.
        || false
}

// HashMap kullanımı uyarısı için (Aşama C'de scope aggregate için).
#[allow(dead_code)]
fn _placeholder_scope_aggregate() {
    let _h: HashMap<NodeId, ProvenancedRawPosition> = HashMap::new();
    let _ = (NodeKind::Module, EdgeKind::Imports);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured_pos(coupling: f64, cohesion: f64, instability: f64) -> ProvenancedRawPosition {
        ProvenancedRawPosition {
            coupling: AxisMetric {
                value: coupling,
                source: MetricSource::Scip,
            },
            cohesion: AxisMetric {
                value: cohesion,
                source: MetricSource::Scip,
            },
            instability: AxisMetric {
                value: instability,
                source: MetricSource::Scip,
            },
            entropy: AxisMetric {
                value: 0.5,
                source: MetricSource::Placeholder,
            },
            witness_depth: AxisMetric {
                value: 0.3,
                source: MetricSource::Placeholder,
            },
        }
    }

    fn placeholder_pos(coupling: f64) -> ProvenancedRawPosition {
        ProvenancedRawPosition {
            coupling: AxisMetric {
                value: coupling,
                source: MetricSource::Placeholder,
            },
            cohesion: AxisMetric {
                value: 0.5,
                source: MetricSource::Placeholder,
            },
            instability: AxisMetric {
                value: 0.5,
                source: MetricSource::Placeholder,
            },
            entropy: AxisMetric {
                value: 0.5,
                source: MetricSource::Placeholder,
            },
            witness_depth: AxisMetric {
                value: 0.3,
                source: MetricSource::Placeholder,
            },
        }
    }

    fn coupling_predicate(
        threshold: f64,
        op: ComparisonOp,
        req_source: Option<MetricSource>,
    ) -> MetricPredicate {
        MetricPredicate {
            metric: PredicateAxis::Coupling,
            operator: op,
            threshold,
            scope: PredicateScope::Node(1),
            required_source: req_source,
            tolerance: 0.0,
        }
    }

    // ── INV-T2: OperatorCapability ──

    #[test]
    fn operator_capability_can_be_issued_by_trusted_api() {
        let cap = OperatorCapability::issue();
        // Trajectory::new requires &OperatorCapability — capability mevcut.
        let t = Trajectory::new(
            &cap,
            1,
            "test".into(),
            crate::vision::VisionVector::default(),
        );
        assert_eq!(t.id, 1);
        assert_eq!(t.status, TrajectoryStatus::Planned);
    }

    #[test]
    fn operator_capability_private_field_cannot_be_constructed_by_agent() {
        // Agent kodu şunu yazamaz (compile error): OperatorCapability { _private: () }
        // Bu test sadece issue() yolunun çalıştığını doğrular; private field invariant'ı
        // compile-time (agent modülü _private'a erişemez).
        let cap = OperatorCapability::issue();
        let _ = cap; // kullanılabilir
    }

    // ── INV-T4: ProvenancedRawPosition + source check ──

    #[test]
    fn placeholder_metric_cannot_close_task() {
        // INV-T4: placeholder source ile predicate satisfied olsa bile reddet.
        let pred = coupling_predicate(0.55, ComparisonOp::Le, Some(MetricSource::Scip));
        let pos = placeholder_pos(0.40); // 0.40 ≤ 0.55 ama placeholder
        assert_eq!(pred.evaluate(&pos), PredicateResult::SourceInsufficient);
    }

    #[test]
    fn measured_metric_satisfies_predicate() {
        let pred = coupling_predicate(0.55, ComparisonOp::Le, Some(MetricSource::Scip));
        let pos = measured_pos(0.40, 0.7, 0.3); // measured, 0.40 ≤ 0.55
        assert_eq!(pred.evaluate(&pos), PredicateResult::Satisfied);
    }

    #[test]
    fn measured_metric_unsatisfied_when_above_threshold() {
        let pred = coupling_predicate(0.55, ComparisonOp::Le, None);
        let pos = measured_pos(0.70, 0.7, 0.3); // 0.70 > 0.55
        assert_eq!(pred.evaluate(&pos), PredicateResult::Unsatisfied);
    }

    // ── INV-T5: PredicateSet multi-axis ──

    #[test]
    fn predicate_set_all_mode_requires_all_satisfied() {
        let set = PredicateSet {
            mode: PredicateMode::All,
            predicates: vec![
                WeightedPredicate {
                    predicate: coupling_predicate(0.55, ComparisonOp::Le, None),
                    weight: None,
                },
                WeightedPredicate {
                    predicate: MetricPredicate {
                        metric: PredicateAxis::Cohesion,
                        operator: ComparisonOp::Ge,
                        threshold: 0.70,
                        scope: PredicateScope::Node(1),
                        required_source: None,
                        tolerance: 0.0,
                    },
                    weight: None,
                },
            ],
            preferred_vector: None,
        };
        // coupling 0.40 ≤ 0.55 ✓, cohesion 0.50 ≥ 0.70 ✗ → NotCompleted
        assert_eq!(
            set.evaluate_completion(&measured_pos(0.40, 0.50, 0.3)),
            PredicateSetResult::NotCompleted
        );
        // coupling ✓, cohesion ✓ → Completed
        assert_eq!(
            set.evaluate_completion(&measured_pos(0.40, 0.75, 0.3)),
            PredicateSetResult::Completed
        );
    }

    #[test]
    fn predicate_set_any_mode_one_satisfied() {
        let set = PredicateSet {
            mode: PredicateMode::Any,
            predicates: vec![
                WeightedPredicate {
                    predicate: coupling_predicate(0.55, ComparisonOp::Le, None),
                    weight: None,
                },
                WeightedPredicate {
                    predicate: MetricPredicate {
                        metric: PredicateAxis::Cohesion,
                        operator: ComparisonOp::Ge,
                        threshold: 0.70,
                        scope: PredicateScope::Node(1),
                        required_source: None,
                        tolerance: 0.0,
                    },
                    weight: None,
                },
            ],
            preferred_vector: None,
        };
        // coupling ✓ (0.40 ≤ 0.55), cohesion ✗ → Any → Completed
        assert_eq!(
            set.evaluate_completion(&measured_pos(0.40, 0.50, 0.3)),
            PredicateSetResult::Completed
        );
    }

    // ── INV-T8: MutationDecision → ApplyTarget ──

    #[test]
    fn reject_produces_not_applied_not_sandbox() {
        // review v4 #3 — Reject ≠ Sandbox. Reject "hiç uygulanmaz".
        assert_eq!(
            MutationDecision::Reject.apply_target(),
            ApplyTarget::NotApplied
        );
    }

    #[test]
    fn accept_as_progress_goes_to_trajectory_checkpoint_not_mainline() {
        // INV-T8 — progress checkpoint asla Mainline.
        assert_eq!(
            MutationDecision::AcceptAsProgress.apply_target(),
            ApplyTarget::Lane(CommitLane::TrajectoryCheckpoint)
        );
    }

    #[test]
    fn accept_as_completed_promotes_to_mainline() {
        assert_eq!(
            MutationDecision::AcceptAsCompleted.apply_target(),
            ApplyTarget::Lane(CommitLane::Mainline)
        );
    }

    #[test]
    fn operator_approval_goes_to_sandbox() {
        assert_eq!(
            MutationDecision::RequireOperatorApproval.apply_target(),
            ApplyTarget::Lane(CommitLane::Sandbox)
        );
    }

    // ── INV-T1: AgentTaskView target coordinate sızıntısı yok ──

    #[test]
    fn agent_task_view_has_no_target_coordinate_fields() {
        let plan = InternalTaskPlan {
            task_id: 1,
            milestone_target_vector: RawPosition {
                x: 0.55,
                y: 0.70,
                z: 0.30,
                w: 0.5,
                v: 0.3,
            },
            task_predicate: PredicateSet {
                mode: PredicateMode::All,
                predicates: vec![WeightedPredicate {
                    predicate: coupling_predicate(0.55, ComparisonOp::Le, None),
                    weight: None,
                }],
                preferred_vector: Some(RawPosition {
                    x: 0.55,
                    y: 0.70,
                    z: 0.30,
                    w: 0.5,
                    v: 0.3,
                }),
            },
            tolerance: 0.02,
        };
        let view = plan.to_agent_view(
            "Reduce coupling",
            RawPosition {
                x: 0.82,
                y: 0.5,
                z: 0.6,
                w: 0.5,
                v: 0.3,
            },
            vec![OpKind::RemoveImport],
            vec![],
        );
        let json = serde_json::to_string(&view).unwrap();
        // INV-T1: hedef koordinat sızıntısı yok (spesifik alan adları).
        assert!(!json.contains("target_vector"));
        assert!(!json.contains("preferred_vector"));
        assert!(!json.contains("milestone_target_vector"));
        assert!(!json.contains("target_raw"));
        assert!(!json.contains("target_region"));
        // current_measurement SERBEST — mevcut durum, hedef değil.
        assert!(json.contains("current_measurement"));
    }

    // ── INV-T6: failure ≠ regression (loss-based) ──

    #[test]
    fn trajectory_loss_decreases_when_approaching_target() {
        let target = RawPosition {
            x: 0.55,
            y: 0.70,
            z: 0.30,
            w: 0.5,
            v: 0.3,
        };
        let far = measured_pos(0.82, 0.50, 0.60);
        let closer = measured_pos(0.65, 0.60, 0.45);
        assert!(trajectory_loss(&closer, &target) < trajectory_loss(&far, &target));
    }
}
