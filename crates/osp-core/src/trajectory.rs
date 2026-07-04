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
/// sayesinde agent kodu bu tipi üretemez; sadece trusted boundary'de alınır.
///
/// `Trajectory::new()` ve `Milestone`/`Task` genesis (PR34 `task_bridge` dahil) bu
/// capability'yi zorunlu kılar → agent hedef belirleyemez (INV-T2, Seçenek A — insan
/// mimar). PermissionMask (runtime value, agent üretebilir) YERİNE capability tipi
/// compile-time korur.
///
/// # Hardening (PR35)
/// Eski `issue()` public'ti — dış crate kolayca capability mint edebiliyordu. Artık:
/// - **`pub(crate) issue()`** — osp-core içi (TCB) sadece.
/// - **`pub issue_for_operator_session()`** — downstream trusted boundary (osp-cli
///   operator mode, osp-mcp operator-mode startup). İsmi "trusted caller sorumluluğu"
///   anlatır — bu metodu çağıran kod operator authority boundary'sidir.
/// - **`#[cfg(test)] issue_for_tests()`** — osp-core testleri.
///
/// Type-level forge (struct literal) zaten engelliydi (private field). PR35 ile
/// *unforgeable capability convention* → gerçek type-level boundary'ye dönüşür.
///
/// ```
/// use osp_core::trajectory::OperatorCapability;
/// // Agent kodu: OperatorCapability { _private: () } → COMPILE ERROR (private field)
/// // Trusted boundary: OperatorCapability::issue_for_operator_session() → OK
/// ```
#[derive(Debug, Clone, Copy)]
pub struct OperatorCapability {
    _private: (),
}

impl OperatorCapability {
    /// osp-core içi (TCB + testler) capability üretimi. `pub(crate)` — downstream
    /// erişemez. osp-core testleri `#[cfg(test)]` modüllerinde bu metodu kullanır
    /// (aynı crate, pub(crate) erişilebilir).
    #[allow(dead_code)] // osp-core testleri dışında kullanılmıyor — downstream issue_for_operator_session
    pub(crate) fn issue() -> Self {
        Self { _private: () }
    }

    /// Downstream trusted-boundary capability (osp-cli operator mode, osp-mcp operator-
    /// mode startup). Bu metodu çağıran kod **operator authority boundary'sidir** —
    /// çağıran, operator olduğunu runtime'da doğrulamış olmalı (ServerMode, CLI flag, vb.).
    ///
    /// İsim kasıtlı: "trusted session" sorumluluğunu anlatır. Agent/automation kodu bu
    /// metodu doğrudan çağırmamalı; operator console / human-in-the-loop gerek.
    pub fn issue_for_operator_session() -> Self {
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
///
/// **G2c-1b:** `Unknown` (serde backward-compat default) + `RejectedByTaskBinding`
/// (Q5.b binding hatası) eklendi. navigator reject-evidence için her attempt hangi
/// gate'te kaldığını kaydeder (arkadaş review 6 #1, #2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GateDecision {
    /// Bilinmeyen / serde default (eski JSON backward-compat). Navigator hiçbir zaman
    /// aktif olarak Unknown üretmez — sadece deserialize sırasında görünebilir.
    #[default]
    Unknown,
    PassedAll,
    RejectedBySyntax,
    /// Q5 θ > bound.
    RejectedByVision,
    RejectedByRule,
    /// Q5.b binding hatası (claim task-bound değil / task resolver bulunamadı).
    /// `EngineCommitError::PermissionDenied` ile eşleşir (arkadaş review 6 #2).
    RejectedByTaskBinding,
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
    /// D4 — Calibration feedback history. Önceki attempt'lerin hata mesajları
    /// (HallucinationType::calibration_message). LLM bu feedback'ten öğrenir — aynı
    /// hatayı tekrarlamaz. INV-T1 uyumlu (hata mesajı, koordinat değil).
    #[serde(default)]
    pub feedback_history: Vec<String>,
    /// **G2c-4 (arkadaş review 10 #3):** Mevcut yapısal çevre — focus node + outgoing
    /// import edge'leri. INV-T1 uyumlu: hedef koordinat DEĞİL, sadece structural context.
    /// LLM bu bağlamı görüp removed_edges üretebilir (coupling düşürme).
    /// `None` = structural context yok (eski backward-compat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural_context: Option<AgentStructuralContext>,
}

/// **G2c-4 (arkadaş review 10 #3):** Agent'a verilen mevcut yapısal çevre.
/// `focus_node_id` (üzerinde çalışılan node) + `current_outgoing_imports` (mevcut
/// import edge'leri). INV-T1 uyumlu — hedef koordinat İÇERMEZ, sadece structural context.
///
/// LLM bu bağlamı görüp `removed_edges` üretir: "focus_node_id'nin şu import'larını kaldır."
/// `Vec<EdgeRef>` (Vec<NodeId> değil) — LLM gördüğü edge'i doğrudan removed_edges'a taşır.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentStructuralContext {
    /// Üzerinde çalışılan mevcut node (task'ın hedef node'u, ama koordinat değil — ID).
    pub focus_node_id: crate::space::NodeId,
    /// Focus node'un mevcut outgoing Imports edge'leri. LLM bunları removed_edges'a
    /// taşıyarak coupling düşürebilir.
    pub current_outgoing_imports: Vec<crate::agent::EdgeRef>,
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
        feedback_history: Vec<String>,
        structural_context: Option<AgentStructuralContext>,
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
            feedback_history,
            structural_context,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TrajectoryEvidence (Aşama B2 — Evidence Ledger, RQ6/RQ7/RQ8 ham veri)
// ═══════════════════════════════════════════════════════════════════════════════

/// Her TaskAttempt'in evidence kaydı (Aşama B2). Token cost + duration + outcome →
/// RQ6 (token), RQ7 (task success), RQ8 (correction değeri) için ham veri.
///
/// **G2c-1b (arkadaş review 6 #1):** `gate_decision` alanı eklendi — navigator'ın tüm
/// attempt'leri (empty/Q4-syntax/commit-error/success) evidence'a girer ve hangi gate'ta
/// kaldığını söyler. `#[serde(default)]` ile eski JSON backward-compat (Unknown default).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrajectoryEvidence {
    pub trajectory_id: TrajectoryId,
    pub milestone_id: MilestoneId,
    pub task_id: TaskId,
    pub attempt_id: TaskAttemptId,
    pub before: RawPosition,
    pub after: RawPosition,
    /// Hangi hard gate'ta kaldı (Q4/Q5/Q6/binding/maneuver-limit/passed).
    /// Reject attempt'lerde red nedeni; success'te PassedAll.
    #[serde(default)]
    pub gate_decision: GateDecision,
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

// ═══════════════════════════════════════════════════════════════════════════════
// Aşama B — TaskResolver + TaskBoundClaim + PredicateGate (Q5.b)
// ═══════════════════════════════════════════════════════════════════════════════

use crate::witness::Claim;

/// Task lookup abstraction (review v2 — planner'a bulaşmadan task resolve).
/// Production: gerçek registry (Aşama C planner). Test: `InMemoryTaskRegistry`.
pub trait TaskResolver {
    fn resolve(&self, task_id: TaskId) -> Option<&Task>;
}

/// Test/placeholder TaskResolver — `HashMap<TaskId, Task>`.
#[derive(Debug, Clone, Default)]
pub struct InMemoryTaskRegistry {
    pub tasks: HashMap<TaskId, Task>,
}

impl InMemoryTaskRegistry {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }
    pub fn insert(&mut self, task: Task) {
        self.tasks.insert(task.id, task);
    }
}

impl TaskResolver for InMemoryTaskRegistry {
    fn resolve(&self, task_id: TaskId) -> Option<&Task> {
        self.tasks.get(&task_id)
    }
}

/// INV-T5 — Q5.b Predicate Gate'in girdisi. Çıplak `Claim` ile çalışmaz; `bind_task_claim`
/// ile üretilir. `claim.task_id` Some olmalı + resolver'da task bulunmalı.
///
/// **Backward-compat:** static Claim'ler (Paper 1) `task_id: None` ile çalışmaya devam
/// eder — sadece Q5.b yolu task-bound gerektirir.
#[derive(Debug, Clone)]
pub struct TaskBoundClaim<'a> {
    pub claim: &'a Claim,
    pub task: &'a Task,
}

/// `bind_task_claim` hatası — claim task'a bağlanamadı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingError {
    /// `claim.task_id` None — standalone claim, Q5.b için task-bound değil.
    MissingTaskId,
    /// `claim.task_id` var ama resolver'da bulunamadı.
    TaskNotFound(TaskId),
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindingError::MissingTaskId => {
                write!(
                    f,
                    "claim has no task_id (standalone — Q5.b requires TaskBoundClaim)"
                )
            }
            BindingError::TaskNotFound(id) => {
                write!(f, "task_id {id} not found in resolver")
            }
        }
    }
}

impl std::error::Error for BindingError {}

/// INV-T5 — Claim'i Task'a bağla. `claim.task_id` None → `MissingTaskId`;
/// resolver'da bulunamazsa → `TaskNotFound`. Başarılırsa `TaskBoundClaim`.
///
/// **Q5.b kuralı:** `PredicateGate::evaluate` sadece `TaskBoundClaim` kabul eder —
/// çıplak Claim ile çağrılamaz (type-level, INV-T5).
pub fn bind_task_claim<'a>(
    claim: &'a Claim,
    resolver: &'a impl TaskResolver,
) -> Result<TaskBoundClaim<'a>, BindingError> {
    let task_id = claim.task_id.ok_or(BindingError::MissingTaskId)?;
    let task = resolver
        .resolve(task_id)
        .ok_or(BindingError::TaskNotFound(task_id))?;
    Ok(TaskBoundClaim { claim, task })
}

/// Q5.b Predicate Gate — TaskBoundClaim'in predicate_set'ini değerlendirir ve
/// deterministic `AttemptOutcome` üretir (Aşama B tezi).
///
/// **Akış:**
/// 1. `measured` (engine ölçtü, INV-T3) → `PredicateSet::evaluate_completion` (INV-T4 source)
/// 2. `Completed` → `AcceptAsCompleted`; `SourceInsufficient` → `Reject` (INV-T4)
/// 3. `NotCompleted` → loss after hesapla, `is_improved` (INV-T6)
/// 4. `TaskPolicy.predicate_failure_policy` + improved → `MutationDecision`
/// 5. `AttemptOutcome { gate_decision: PassedAll, predicate_completion, mutation_decision, witness: None }`
///
/// **Not:** Hard gates (Q4/Q5/Q6) zaten geçti varsayılır (gate_decision: PassedAll).
/// Bu fonksiyon sadece soft gate Q5.b'yi değerlendirir.
#[derive(Debug, Clone, Copy, Default)]
pub struct PredicateGate;

/// Q5.b değerlendirme girdisi — engine'in ölçtüğü + loss context.
#[derive(Debug, Clone)]
pub struct PredicateGateInput<'a> {
    pub bound: TaskBoundClaim<'a>,
    /// Engine-measured simulated_after (INV-T3 — agent değiştiremez).
    pub measured: &'a ProvenancedRawPosition,
    /// Loss before (mevcut durumun preferred_vector'e uzaklığı).
    pub loss_before: f64,
    /// Preferred/target vector (loss & is_improved için).
    pub target: &'a RawPosition,
}

/// Q5.b çıktısı — AttemptOutcome + hesaplanan loss_after.
#[derive(Debug, Clone, PartialEq)]
pub struct PredicateGateOutput {
    pub outcome: AttemptOutcome,
    pub loss_after: f64,
}

impl PredicateGate {
    /// Q5.b — soft gate. Hard gates (Q4/Q5/Q6) zaten geçti (gate_decision: PassedAll).
    pub fn evaluate(&self, input: PredicateGateInput<'_>) -> PredicateGateOutput {
        let policy = &input.bound.task.policy;
        let loss_after = trajectory_loss(input.measured, input.target);

        // 1. PredicateSet completion (INV-T4 source check dahil).
        let (predicate_completion, mutation_decision) = match input
            .bound
            .task
            .target_predicate_set
            .evaluate_completion(input.measured)
        {
            PredicateSetResult::Completed => (
                PredicateCompletion::Completed,
                MutationDecision::AcceptAsCompleted,
            ),
            PredicateSetResult::SourceInsufficient => {
                // INV-T4 — placeholder/heuristic ile task kapatılamaz. Her zaman Reject.
                (PredicateCompletion::NotCompleted, MutationDecision::Reject)
            }
            PredicateSetResult::NotCompleted => {
                // 2. INV-T6 — policy'ye göre: improved mı, regressed mi?
                let improved =
                    is_improved_loss(input.loss_before, loss_after, input.measured, policy);
                let completion = PredicateCompletion::NotCompleted;
                let decision = match policy.predicate_failure_policy {
                    PredicateFailurePolicy::StrictReject => MutationDecision::Reject,
                    PredicateFailurePolicy::AcceptImprovement => {
                        if policy.allow_progress_checkpoint && improved {
                            MutationDecision::AcceptAsProgress
                        } else {
                            MutationDecision::Reject
                        }
                    }
                    PredicateFailurePolicy::OperatorApproval => {
                        MutationDecision::RequireOperatorApproval
                    }
                };
                (completion, decision)
            }
        };

        PredicateGateOutput {
            outcome: AttemptOutcome {
                gate_decision: GateDecision::PassedAll,
                predicate_completion,
                mutation_decision,
                witness_status: None,
            },
            loss_after,
        }
    }
}

/// INV-T6 — loss-based improved kontrolü. `loss_after < loss_before - min_delta`
/// AND max_axis_regression aşılmadı. (Aşama A'daki `is_improved`'un loss-input versiyonu.)
fn is_improved_loss(
    loss_before: f64,
    loss_after: f64,
    measured: &ProvenancedRawPosition,
    policy: &TaskPolicy,
) -> bool {
    if loss_after >= loss_before - policy.min_improvement_delta {
        return false;
    }
    // max_axis_regression: kritik eksenlerde regresyon kontrolü.
    // measured'da her axis 0..1; regresyon = değerin threshold'u aşması (basit Aşama B;
    // Aşama C'de before/after karşılaştırması + WeightedPredicate loss).
    // Şimdilik: hiçbir axis 0.85'i aşmasın (hard cap, refine Aşama C).
    measured.coupling.value < 0.85
        && measured.instability.value < 0.85
        && measured.cohesion.value > 0.15
}

// ═══════════════════════════════════════════════════════════════════════════════
// Aşama C — Planner / Milestone Decomposition (deterministic, INV-T2)
// ═══════════════════════════════════════════════════════════════════════════════

use crate::space::NodeRole;

/// Planner/operator seviyesi decomposition kısıtları. Agent bunları **değiştiremez**
/// (INV-T2 — operator only). Decomposition policy milestone scope + loss dağılımına
/// göre kaç task üretileceğini belirler.
///
/// **Prensip:** Task, bir agent'ın 1-3 attempt içinde anlamlı progress üretebileceği
/// kadar küçük, ama mimari bağlamı kaybettirmeyecek kadar büyük olmalı.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecompositionPolicy {
    /// Milestone başına maksimum task sayısı (task patlaması önü).
    pub max_tasks_per_milestone: usize,
    /// Task başına maksimum node sayısı (geniş task önü).
    pub max_nodes_per_task: usize,
    /// Top-offender için loss katkısı oranı (örn 0.10 = top %10).
    pub top_offender_ratio: f64,
    /// Bir node'un task'a girmesi için min loss katkısı.
    pub min_loss_contribution: f64,
    /// Role bazlı decomposition aktif mi.
    pub split_by_role: bool,
    /// Axis bazlı decomposition aktif mi.
    pub split_by_axis: bool,
}

impl Default for DecompositionPolicy {
    fn default() -> Self {
        Self {
            max_tasks_per_milestone: 8,
            max_nodes_per_task: 15,
            top_offender_ratio: 0.10,
            min_loss_contribution: 0.05,
            split_by_role: true,
            split_by_axis: false,
        }
    }
}

/// Decomposition stratejisi (deterministic). Planner seçer; agent görmez/yapamaz.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DecompositionStrategy {
    /// Scope tek node / küçük module → tek task.
    OneTask,
    /// Top K high-loss node → her biri ayrı task + aggregate cleanup (kalan node'lar).
    SplitByNodeTopK(usize),
    /// Role bazlı (Core task, Runtime task, Support task) — her role için ayrı task.
    SplitByRole,
    /// Axis bazlı (coupling task, cohesion task, instability task).
    SplitByAxis,
}

/// Planner'ın ihtiyaç duyduğu measured space snapshot. **Agent'a verilmez.**
/// Aşama C'de minimal (node id + measured position + role + loss). Aşama D'de engine'den beslenir.
#[derive(Debug, Clone, Default)]
pub struct DecompositionSpace {
    pub nodes: Vec<DecompositionNode>,
    /// preferred_vector (milestone merkezi) — loss_contribution hesabı için.
    pub preferred_vector: RawPosition,
}

impl DecompositionSpace {
    /// Top K high-loss node (loss_contribution'a göre descending, deterministic sort).
    pub fn top_offenders(&self, k: usize, min_loss: f64) -> Vec<&DecompositionNode> {
        let mut sorted: Vec<&DecompositionNode> = self
            .nodes
            .iter()
            .filter(|n| n.loss_contribution >= min_loss)
            .collect();
        // Deterministic: loss descending, tie-break id ascending.
        sorted.sort_by(|a, b| {
            b.loss_contribution
                .partial_cmp(&a.loss_contribution)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        sorted.into_iter().take(k).collect()
    }

    /// Belirli bir role'deki node'lar.
    pub fn by_role(&self, role: NodeRole) -> Vec<&DecompositionNode> {
        self.nodes.iter().filter(|n| n.role == role).collect()
    }
}

/// Decomposition için tek node snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct DecompositionNode {
    pub id: NodeId,
    pub role: NodeRole,
    pub measured: ProvenancedRawPosition,
    /// preferred_vector'e uzaklık (trajectory_loss). Top-offender için.
    pub loss_contribution: f64,
}

/// INV-T2 — MilestoneDecomposer. Deterministic kurallarla Milestone → Task[].
/// `OperatorCapability` zorunlu (operator/planner only; agent decomposition yapamaz).
pub fn decompose_milestone(
    milestone: &Milestone,
    space: &DecompositionSpace,
    policy: &DecompositionPolicy,
    strategy: &DecompositionStrategy,
    _cap: &OperatorCapability, // INV-T2 — operator only
) -> Vec<Task> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TASK_ID: AtomicU64 = AtomicU64::new(1);
    let next_id = || TaskId::from(TASK_ID.fetch_add(1, Ordering::SeqCst));

    let make_task = |label: String, predicate_set: PredicateSet, scope_nodes: Vec<NodeId>| {
        // Scope'u ilk node'a bağla (multi-node scope Aşama D'de Subgraph predicate).
        let scope = scope_nodes
            .first()
            .copied()
            .map(PredicateScope::Node)
            .unwrap_or(PredicateScope::Subgraph(scope_nodes));
        // Predicate'ların scope'unu override et (milestone predicate'leri + task scope).
        let mut ps = predicate_set;
        ps.predicates = ps
            .predicates
            .into_iter()
            .map(|wp| {
                let mut p = wp.predicate;
                p.scope = scope.clone();
                WeightedPredicate {
                    predicate: p,
                    weight: wp.weight,
                }
            })
            .collect();
        Task {
            id: next_id(),
            milestone_id: milestone.id,
            label,
            target_predicate_set: ps,
            policy: TaskPolicy::default(),
            allowed_operations: vec![
                OpKind::RemoveImport,
                OpKind::AddAbstraction,
                OpKind::ExtractModule,
            ],
            constraints: vec![],
            status: TaskStatus::Pending,
        }
    };

    // Milestone predicate'lerini PredicateSet'e çevir (All mode, weight None).
    let base_predicate_set = PredicateSet {
        mode: PredicateMode::All,
        predicates: milestone
            .target_region
            .predicates
            .iter()
            .map(|p| WeightedPredicate {
                predicate: p.clone(),
                weight: None,
            })
            .collect(),
        preferred_vector: milestone.target_region.preferred_vector,
    };

    match strategy {
        DecompositionStrategy::OneTask => {
            let all_nodes: Vec<NodeId> = space.nodes.iter().map(|n| n.id).collect();
            vec![make_task(
                milestone.label.clone(),
                base_predicate_set,
                all_nodes,
            )]
        }
        DecompositionStrategy::SplitByNodeTopK(k) => {
            let k = (*k)
                .min(policy.max_tasks_per_milestone.saturating_sub(1))
                .max(1);
            let offenders = space.top_offenders(k, policy.min_loss_contribution);
            let offender_ids: Vec<NodeId> = offenders.iter().map(|n| n.id).collect();
            let mut tasks: Vec<Task> = offenders
                .into_iter()
                .map(|n| {
                    let mut ps = base_predicate_set.clone();
                    ps.predicates = ps
                        .predicates
                        .into_iter()
                        .map(|wp| {
                            let mut p = wp.predicate;
                            p.scope = PredicateScope::Node(n.id);
                            WeightedPredicate {
                                predicate: p,
                                weight: wp.weight,
                            }
                        })
                        .collect();
                    Task {
                        id: next_id(),
                        milestone_id: milestone.id,
                        label: format!(
                            "Reduce loss for node {} ({:.2})",
                            n.id, n.loss_contribution
                        ),
                        target_predicate_set: ps,
                        policy: TaskPolicy::default(),
                        allowed_operations: vec![OpKind::RemoveImport, OpKind::AddAbstraction],
                        constraints: vec![],
                        status: TaskStatus::Pending,
                    }
                })
                .collect();
            // Aggregate cleanup task — kalan node'lar.
            let remaining: Vec<NodeId> = space
                .nodes
                .iter()
                .map(|n| n.id)
                .filter(|id| !offender_ids.contains(id))
                .collect();
            if !remaining.is_empty() {
                tasks.push(make_task(
                    format!("Aggregate cleanup ({} remaining nodes)", remaining.len()),
                    base_predicate_set,
                    remaining,
                ));
            }
            tasks.truncate(policy.max_tasks_per_milestone);
            tasks
        }
        DecompositionStrategy::SplitByRole => {
            use NodeRole::*;
            let mut tasks = Vec::new();
            for role in [Core, Runtime, Support, Adapter, Utility, TypeSurface] {
                let nodes: Vec<&DecompositionNode> = space.by_role(role);
                if nodes.is_empty() {
                    continue;
                }
                let ids: Vec<NodeId> = nodes.iter().map(|n| n.id).collect();
                let mut ps = base_predicate_set.clone();
                ps.predicates = ps
                    .predicates
                    .into_iter()
                    .map(|wp| {
                        let mut p = wp.predicate;
                        p.scope = PredicateScope::Subgraph(ids.clone());
                        WeightedPredicate {
                            predicate: p,
                            weight: wp.weight,
                        }
                    })
                    .collect();
                tasks.push(Task {
                    id: next_id(),
                    milestone_id: milestone.id,
                    label: format!("{role:?} role cleanup ({} nodes)", ids.len()),
                    target_predicate_set: ps,
                    policy: TaskPolicy::default(),
                    allowed_operations: vec![OpKind::RemoveImport, OpKind::AddAbstraction],
                    constraints: vec![],
                    status: TaskStatus::Pending,
                });
                if tasks.len() >= policy.max_tasks_per_milestone {
                    break;
                }
            }
            tasks
        }
        DecompositionStrategy::SplitByAxis => {
            // Axis bazlı: her predicate → ayrı task. coupling task, cohesion task, vb.
            let mut tasks = Vec::new();
            for wp in &base_predicate_set.predicates {
                let all_nodes: Vec<NodeId> = space.nodes.iter().map(|n| n.id).collect();
                let axis_label = format!("{:?}", wp.predicate.metric);
                let mut ps = PredicateSet {
                    mode: PredicateMode::All,
                    predicates: vec![{
                        let mut p = wp.predicate.clone();
                        p.scope = PredicateScope::Subgraph(all_nodes.clone());
                        WeightedPredicate {
                            predicate: p,
                            weight: wp.weight,
                        }
                    }],
                    preferred_vector: base_predicate_set.preferred_vector,
                };
                let _ = &mut ps; // satisfy
                tasks.push(Task {
                    id: next_id(),
                    milestone_id: milestone.id,
                    label: format!("{axis_label} axis improvement"),
                    target_predicate_set: ps,
                    policy: TaskPolicy::default(),
                    allowed_operations: vec![OpKind::RemoveImport, OpKind::AddAbstraction],
                    constraints: vec![],
                    status: TaskStatus::Pending,
                });
                if tasks.len() >= policy.max_tasks_per_milestone {
                    break;
                }
            }
            tasks
        }
    }
}

impl Milestone {
    /// Milestone achieved = `TargetRegion.predicates` satisfied (engine-measured).
    /// **Task'lar Done OLMASA bile** milestone achieved olabilir — task'lar araç,
    /// asıl otorite engine measurement (sizin kuralınız, review v2).
    pub fn is_achieved(&self, measured: &ProvenancedRawPosition) -> bool {
        self.target_region
            .predicates
            .iter()
            .all(|p| matches!(p.evaluate(measured), PredicateResult::Satisfied))
    }
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
            vec![],
            None, // G2c-4: structural context (bu test INV-T1 leak check)
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

    /// G2c-4 (arkadaş review 10): INV-T1 — structural context SERBEST, hedef koordinat YASAK.
    /// Agent structural context görebilir (focus_node_id, current_outgoing_imports) ama
    /// hedef koordinatı GÖREMEZ. Bu ayrım Paper 2 reviewer'ı için kritik.
    #[test]
    fn g2c4_structural_context_allowed_but_target_coordinate_forbidden() {
        use crate::agent::EdgeRef;
        use crate::space::{EdgeKind, NodeId};
        let plan = InternalTaskPlan {
            task_id: 1,
            milestone_target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            task_predicate: PredicateSet {
                mode: PredicateMode::All,
                predicates: vec![],
                preferred_vector: Some(RawPosition {
                    x: 0.55,
                    y: 0.6,
                    z: 0.4,
                    w: 0.5,
                    v: 0.3,
                }),
            },
            tolerance: 0.02,
        };
        let sc = AgentStructuralContext {
            focus_node_id: 0 as NodeId,
            current_outgoing_imports: vec![EdgeRef {
                from: 0,
                to: 1,
                kind: EdgeKind::Imports,
            }],
        };
        let view = plan.to_agent_view(
            "Reduce coupling",
            RawPosition {
                x: 0.80,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            vec![],
            vec![],
            vec![],
            Some(sc),
        );
        let json = serde_json::to_string(&view).unwrap();
        // Structural context SERBEST (G2c-4) — agent mevcut yapısal çevreyi görür.
        assert!(
            json.contains("structural_context"),
            "structural context allowed"
        );
        assert!(json.contains("focus_node_id"), "focus_node_id allowed");
        assert!(
            json.contains("current_outgoing_imports"),
            "current_outgoing_imports allowed"
        );
        // Hedef koordinat hâlâ YASAK (INV-T1 korunur).
        assert!(
            !json.contains("preferred_vector"),
            "target coordinate forbidden"
        );
        assert!(
            !json.contains("milestone_target_vector"),
            "target coordinate forbidden"
        );
        assert!(
            !json.contains("target_region"),
            "target region forbidden (structural context != target)"
        );
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

    // ─────────────────────────────────────────────────────────────────────────
    // Aşama B — Q5.b Predicate Gate done-criteria (10 test)
    // ─────────────────────────────────────────────────────────────────────────

    use crate::coords::RawPosition;
    use crate::witness::{Claim, Intent};

    fn coupling_pred_le(threshold: f64, req_source: Option<MetricSource>) -> MetricPredicate {
        MetricPredicate {
            metric: PredicateAxis::Coupling,
            operator: ComparisonOp::Le,
            threshold,
            scope: PredicateScope::Node(1),
            required_source: req_source,
            tolerance: 0.0,
        }
    }

    /// Test için task üret — coupling ≤ threshold predicate + policy. Target vector da döner
    /// (task move edilmeden önce preferred_vector alınmış olur).
    fn test_task(id: TaskId, threshold: f64, policy: TaskPolicy) -> (Task, RawPosition) {
        let target = RawPosition {
            x: threshold,
            y: 0.7,
            z: 0.3,
            w: 0.5,
            v: 0.3,
        };
        let task = Task {
            id,
            milestone_id: 1,
            label: format!("Reduce coupling to {threshold}"),
            target_predicate_set: PredicateSet {
                mode: PredicateMode::All,
                predicates: vec![WeightedPredicate {
                    predicate: coupling_pred_le(threshold, Some(MetricSource::Scip)),
                    weight: None,
                }],
                preferred_vector: Some(target),
            },
            policy,
            allowed_operations: vec![OpKind::RemoveImport],
            constraints: vec![],
            status: TaskStatus::Pending,
        };
        (task, target)
    }

    /// Test için claim üret — task_id ile veya None (standalone).
    fn test_claim(id: u64, task_id: Option<TaskId>, measured: ProvenancedRawPosition) -> Claim {
        Claim {
            id,
            intent: Intent::new(42, measured.to_raw()),
            author: 42,
            computed_raw: measured.to_raw(),
            delta_nodes: vec![],
            delta_edges: vec![],
            task_id,
            removed_edges: vec![], // G2c-2
        }
    }

    fn gate_eval<'a>(
        bound: TaskBoundClaim<'a>,
        measured: &'a ProvenancedRawPosition,
        loss_before: f64,
        target: &'a RawPosition,
    ) -> PredicateGateOutput {
        PredicateGate.evaluate(PredicateGateInput {
            bound,
            measured,
            loss_before,
            target,
        })
    }

    // 1. predicate_satisfied_completes_task
    #[test]
    fn predicate_satisfied_completes_task() {
        let (task, target) = test_task(1, 0.55, TaskPolicy::default());
        let mut reg = InMemoryTaskRegistry::new();
        reg.insert(task);
        // coupling 0.40 ≤ 0.55 (measured/scip) → Completed.
        let measured = measured_pos(0.40, 0.7, 0.3);
        let claim = test_claim(1, Some(1), measured.clone());
        let bound = bind_task_claim(&claim, &reg).unwrap();
        let out = gate_eval(bound, &measured, 1.0, &target);
        assert_eq!(
            out.outcome.predicate_completion,
            PredicateCompletion::Completed
        );
        assert_eq!(
            out.outcome.mutation_decision,
            MutationDecision::AcceptAsCompleted
        );
    }

    // 2. placeholder_metric_cannot_close_task_gate (INV-T4 — gate seviyesi)
    #[test]
    fn placeholder_metric_cannot_close_task_gate() {
        let (task, target) = test_task(1, 0.55, TaskPolicy::default());
        let mut reg = InMemoryTaskRegistry::new();
        reg.insert(task);
        // coupling 0.40 ≤ 0.55 ama placeholder → SourceInsufficient → Reject.
        let measured = placeholder_pos(0.40);
        let claim = test_claim(1, Some(1), measured.clone());
        let bound = bind_task_claim(&claim, &reg).unwrap();
        let out = gate_eval(bound, &measured, 1.0, &target);
        assert_eq!(
            out.outcome.predicate_completion,
            PredicateCompletion::NotCompleted
        );
        assert_eq!(out.outcome.mutation_decision, MutationDecision::Reject);
    }

    // 3. predicate_uses_computed_raw_not_hint (INV-T3)
    #[test]
    fn predicate_uses_computed_raw_not_hint() {
        // PositionHint "coupling 0.30" dese bile, computed_raw (measured) 0.70 → Unsatisfied.
        // (Hint Aşama B'de yok; bu test measured'ın authoritative olduğunu doğrular.)
        let (task, target) = test_task(1, 0.55, TaskPolicy::default());
        let mut reg = InMemoryTaskRegistry::new();
        reg.insert(task);
        let measured = measured_pos(0.70, 0.7, 0.3); // 0.70 > 0.55 → Unsatisfied
        let claim = test_claim(1, Some(1), measured.clone());
        let bound = bind_task_claim(&claim, &reg).unwrap();
        let out = gate_eval(bound, &measured, 0.5, &target);
        assert_eq!(
            out.outcome.predicate_completion,
            PredicateCompletion::NotCompleted
        );
    }

    // 4. missing_task_id_rejects_claim (binding)
    #[test]
    fn missing_task_id_rejects_claim() {
        let reg = InMemoryTaskRegistry::new();
        let measured = measured_pos(0.40, 0.7, 0.3);
        let claim = test_claim(1, None, measured); // standalone — task_id None
        let result = bind_task_claim(&claim, &reg);
        assert_eq!(result.unwrap_err(), BindingError::MissingTaskId);
    }

    // 5. strict_policy_rejects_unsatisfied_predicate
    #[test]
    fn strict_policy_rejects_unsatisfied_predicate() {
        let (task, target) = test_task(1, 0.55, TaskPolicy::default()); // StrictReject default
        let mut reg = InMemoryTaskRegistry::new();
        reg.insert(task);
        let measured = measured_pos(0.70, 0.7, 0.3); // > 0.55 → NotCompleted
        let claim = test_claim(1, Some(1), measured.clone());
        let bound = bind_task_claim(&claim, &reg).unwrap();
        let out = gate_eval(bound, &measured, 1.0, &target);
        assert_eq!(out.outcome.mutation_decision, MutationDecision::Reject);
    }

    // 6. accept_improvement_policy_accepts_progress (INV-T6)
    #[test]
    fn accept_improvement_policy_accepts_progress() {
        let mut policy = TaskPolicy::default();
        policy.predicate_failure_policy = PredicateFailurePolicy::AcceptImprovement;
        policy.allow_progress_checkpoint = true;
        let (task, target) = test_task(1, 0.55, policy);
        let mut reg = InMemoryTaskRegistry::new();
        reg.insert(task);
        // coupling 0.65 > 0.55 → NotCompleted, ama loss_before'dan az (improved).
        let measured = measured_pos(0.65, 0.6, 0.4);
        let claim = test_claim(1, Some(1), measured.clone());
        let bound = bind_task_claim(&claim, &reg).unwrap();
        let loss_before = 0.9; // büyük — measured loss_after'dan çok daha büyük
        let out = gate_eval(bound, &measured, loss_before, &target);
        assert_eq!(
            out.outcome.predicate_completion,
            PredicateCompletion::NotCompleted
        );
        assert_eq!(
            out.outcome.mutation_decision,
            MutationDecision::AcceptAsProgress
        );
    }

    // 7. regression_rejected_even_if_one_axis_improved (F5)
    #[test]
    fn regression_rejected_even_if_one_axis_improved() {
        // coupling improved ama instability 0.90 (> 0.85 hard cap) → is_improved false → Reject.
        let mut policy = TaskPolicy::default();
        policy.predicate_failure_policy = PredicateFailurePolicy::AcceptImprovement;
        policy.allow_progress_checkpoint = true;
        let (task, target) = test_task(1, 0.55, policy);
        let mut reg = InMemoryTaskRegistry::new();
        reg.insert(task);
        let measured = measured_pos(0.60, 0.6, 0.90); // coupling OK ama instability patladı
        let claim = test_claim(1, Some(1), measured.clone());
        let bound = bind_task_claim(&claim, &reg).unwrap();
        let out = gate_eval(bound, &measured, 0.9, &target);
        assert_eq!(out.outcome.mutation_decision, MutationDecision::Reject);
    }

    // 8. progress_checkpoint_cannot_promote_to_mainline (INV-T8)
    #[test]
    fn progress_checkpoint_cannot_promote_to_mainline() {
        // AcceptAsProgress → ApplyTarget::Lane(TrajectoryCheckpoint), asla Mainline.
        assert_eq!(
            MutationDecision::AcceptAsProgress.apply_target(),
            ApplyTarget::Lane(CommitLane::TrajectoryCheckpoint)
        );
        assert_ne!(
            MutationDecision::AcceptAsProgress.apply_target(),
            ApplyTarget::Lane(CommitLane::Mainline)
        );
    }

    // 9. reject_produces_not_applied (review v4 #3)
    #[test]
    fn reject_produces_not_applied() {
        assert_eq!(
            MutationDecision::Reject.apply_target(),
            ApplyTarget::NotApplied
        );
    }

    // 10. task_not_found_rejects_claim (binding)
    #[test]
    fn task_not_found_rejects_claim() {
        let reg = InMemoryTaskRegistry::new(); // boş — task 999 yok
        let measured = measured_pos(0.40, 0.7, 0.3);
        let claim = test_claim(1, Some(999), measured);
        let result = bind_task_claim(&claim, &reg);
        assert_eq!(result.unwrap_err(), BindingError::TaskNotFound(999));
    }

    // Ek: operator_approval_policy (review v2 — critical domain)
    #[test]
    fn operator_approval_policy_requires_human_review() {
        let mut policy = TaskPolicy::default();
        policy.predicate_failure_policy = PredicateFailurePolicy::OperatorApproval;
        let (task, target) = test_task(1, 0.55, policy);
        let mut reg = InMemoryTaskRegistry::new();
        reg.insert(task);
        let measured = measured_pos(0.70, 0.7, 0.3); // NotCompleted
        let claim = test_claim(1, Some(1), measured.clone());
        let bound = bind_task_claim(&claim, &reg).unwrap();
        let out = gate_eval(bound, &measured, 1.0, &target);
        assert_eq!(
            out.outcome.mutation_decision,
            MutationDecision::RequireOperatorApproval
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Aşama C — Planner / Milestone Decomposition done-criteria
    // ─────────────────────────────────────────────────────────────────────────

    use crate::space::NodeRole;

    fn dec_node(id: NodeId, role: NodeRole, coupling: f64, loss: f64) -> DecompositionNode {
        DecompositionNode {
            id,
            role,
            measured: measured_pos(coupling, 0.6, 0.4),
            loss_contribution: loss,
        }
    }

    fn dec_space(nodes: Vec<DecompositionNode>, target_coupling: f64) -> DecompositionSpace {
        DecompositionSpace {
            nodes,
            preferred_vector: RawPosition {
                x: target_coupling,
                y: 0.7,
                z: 0.3,
                w: 0.5,
                v: 0.3,
            },
        }
    }

    fn coupling_milestone(id: MilestoneId, threshold: f64) -> Milestone {
        Milestone {
            id,
            label: format!("Coupling ≤ {threshold}"),
            target_region: TargetRegion {
                predicates: vec![coupling_pred_le(threshold, Some(MetricSource::Scip))],
                preferred_vector: Some(RawPosition {
                    x: threshold,
                    y: 0.7,
                    z: 0.3,
                    w: 0.5,
                    v: 0.3,
                }),
            },
            tasks: vec![],
            status: MilestoneStatus::Pending,
        }
    }

    // 1. one_task_decomposition_for_single_node_scope
    #[test]
    fn one_task_decomposition_for_single_node_scope() {
        let cap = OperatorCapability::issue();
        let milestone = coupling_milestone(1, 0.55);
        let space = dec_space(vec![dec_node(10, NodeRole::Core, 0.82, 0.3)], 0.55);
        let policy = DecompositionPolicy::default();
        let tasks = decompose_milestone(
            &milestone,
            &space,
            &policy,
            &DecompositionStrategy::OneTask,
            &cap,
        );
        assert_eq!(tasks.len(), 1, "OneTask → tek task");
        assert_eq!(tasks[0].milestone_id, 1);
    }

    // 2. split_by_node_topk_produces_offender_tasks
    #[test]
    fn split_by_node_topk_produces_offender_tasks() {
        let cap = OperatorCapability::issue();
        let milestone = coupling_milestone(1, 0.55);
        // 5 node, farklı loss katkıları.
        let space = dec_space(
            vec![
                dec_node(1, NodeRole::Core, 0.90, 0.50),
                dec_node(2, NodeRole::Core, 0.85, 0.40),
                dec_node(3, NodeRole::Runtime, 0.60, 0.10),
                dec_node(4, NodeRole::Runtime, 0.55, 0.05),
                dec_node(5, NodeRole::Support, 0.50, 0.02),
            ],
            0.55,
        );
        let policy = DecompositionPolicy::default();
        let tasks = decompose_milestone(
            &milestone,
            &space,
            &policy,
            &DecompositionStrategy::SplitByNodeTopK(2),
            &cap,
        );
        // 2 offender task + 1 aggregate cleanup (3 remaining nodes) = 3 task.
        assert!(tasks.len() >= 2, "top-k offender task + cleanup");
        // İlk task en yüksek loss node (id 1, loss 0.50).
        assert!(
            tasks[0].label.contains("node 1"),
            "highest loss first: {}",
            tasks[0].label
        );
    }

    // 3. split_by_role_groups_by_architectural_role
    #[test]
    fn split_by_role_groups_by_architectural_role() {
        let cap = OperatorCapability::issue();
        let milestone = coupling_milestone(1, 0.55);
        let space = dec_space(
            vec![
                dec_node(1, NodeRole::Core, 0.80, 0.3),
                dec_node(2, NodeRole::Core, 0.75, 0.25),
                dec_node(3, NodeRole::Runtime, 0.70, 0.2),
                dec_node(4, NodeRole::Support, 0.65, 0.15),
            ],
            0.55,
        );
        let policy = DecompositionPolicy::default();
        let tasks = decompose_milestone(
            &milestone,
            &space,
            &policy,
            &DecompositionStrategy::SplitByRole,
            &cap,
        );
        // 3 role var (Core, Runtime, Support) → 3 task.
        assert_eq!(tasks.len(), 3, "3 distinct roles → 3 tasks");
        assert!(tasks.iter().any(|t| t.label.contains("Core")));
        assert!(tasks.iter().any(|t| t.label.contains("Runtime")));
        assert!(tasks.iter().any(|t| t.label.contains("Support")));
    }

    // 4. max_tasks_per_milestone_enforced
    #[test]
    fn max_tasks_per_milestone_enforced() {
        let cap = OperatorCapability::issue();
        let milestone = coupling_milestone(1, 0.55);
        // 10 node, hepsi farklı role'de (SplitByRole → 6 role task).
        let mut nodes = Vec::new();
        for i in 1..=10 {
            let role = match i % 3 {
                0 => NodeRole::Core,
                1 => NodeRole::Runtime,
                _ => NodeRole::Support,
            };
            nodes.push(dec_node(i, role, 0.80, 0.1 * i as f64));
        }
        let space = dec_space(nodes, 0.55);
        let mut policy = DecompositionPolicy::default();
        policy.max_tasks_per_milestone = 2; // strict cap
        let tasks = decompose_milestone(
            &milestone,
            &space,
            &policy,
            &DecompositionStrategy::SplitByRole,
            &cap,
        );
        assert!(
            tasks.len() <= 2,
            "max_tasks_per_milestone enforced: got {}",
            tasks.len()
        );
    }

    // 5. decomposer_requires_operator_capability (INV-T2 — signature)
    #[test]
    fn decomposer_requires_operator_capability() {
        // decompose_milestone imzası &OperatorCapability zorunlu — agent üretemez.
        // Bu test sadece capability ile çağrılabildiğini doğrular (compile-time invariant).
        let cap = OperatorCapability::issue();
        let milestone = coupling_milestone(1, 0.55);
        let space = dec_space(vec![dec_node(1, NodeRole::Core, 0.80, 0.3)], 0.55);
        let tasks = decompose_milestone(
            &milestone,
            &space,
            &DecompositionPolicy::default(),
            &DecompositionStrategy::OneTask,
            &cap,
        );
        assert!(!tasks.is_empty());
    }

    // 6. intent_from_task_uses_preferred_vector (INV-T1)
    #[test]
    fn intent_from_task_uses_preferred_vector() {
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
                    predicate: coupling_pred_le(0.55, None),
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
        let intent = Intent::from_task(42, &plan);
        // target_raw = milestone_target_vector (preferred_vector), INV-T1.
        assert_eq!(intent.target_raw, plan.milestone_target_vector);
    }

    // 7. milestone_achieved_when_region_satisfied_not_all_tasks_done
    #[test]
    fn milestone_achieved_when_region_satisfied_not_all_tasks_done() {
        // Sizin kuralınız: milestone achieved = TargetRegion satisfied (engine-measured),
        // task'lar Done olmasa bile.
        let milestone = coupling_milestone(1, 0.55);
        // coupling 0.40 ≤ 0.55 (measured/scip) → region satisfied.
        let measured = measured_pos(0.40, 0.7, 0.3);
        assert!(
            milestone.is_achieved(&measured),
            "region satisfied → achieved"
        );
        // coupling 0.70 > 0.55 → region not satisfied.
        let measured_fail = measured_pos(0.70, 0.7, 0.3);
        assert!(
            !milestone.is_achieved(&measured_fail),
            "region not satisfied → not achieved"
        );
    }

    // 8. decomposition_deterministic_same_input_same_output
    #[test]
    fn decomposition_deterministic_same_input_same_output() {
        let cap = OperatorCapability::issue();
        let milestone = coupling_milestone(1, 0.55);
        let space = dec_space(
            vec![
                dec_node(1, NodeRole::Core, 0.90, 0.50),
                dec_node(2, NodeRole::Core, 0.85, 0.40),
                dec_node(3, NodeRole::Runtime, 0.60, 0.10),
            ],
            0.55,
        );
        let policy = DecompositionPolicy::default();
        let tasks1 = decompose_milestone(
            &milestone,
            &space,
            &policy,
            &DecompositionStrategy::SplitByNodeTopK(2),
            &cap,
        );
        let tasks2 = decompose_milestone(
            &milestone,
            &space,
            &policy,
            &DecompositionStrategy::SplitByNodeTopK(2),
            &cap,
        );
        // Aynı input → aynı task labels (id'ler atomic artar, label deterministic).
        let labels1: Vec<String> = tasks1.iter().map(|t| t.label.clone()).collect();
        let labels2: Vec<String> = tasks2.iter().map(|t| t.label.clone()).collect();
        assert_eq!(
            labels1, labels2,
            "deterministic: same labels for same input"
        );
    }

    // 9. aggregate_cleanup_task_for_remaining_nodes
    #[test]
    fn aggregate_cleanup_task_for_remaining_nodes() {
        let cap = OperatorCapability::issue();
        let milestone = coupling_milestone(1, 0.55);
        let space = dec_space(
            vec![
                dec_node(1, NodeRole::Core, 0.90, 0.50),
                dec_node(2, NodeRole::Core, 0.85, 0.40),
                dec_node(3, NodeRole::Runtime, 0.60, 0.10),
                dec_node(4, NodeRole::Support, 0.55, 0.03),
            ],
            0.55,
        );
        let policy = DecompositionPolicy::default();
        let tasks = decompose_milestone(
            &milestone,
            &space,
            &policy,
            &DecompositionStrategy::SplitByNodeTopK(2),
            &cap,
        );
        // Aggregate cleanup task var (remaining nodes 3,4).
        assert!(
            tasks.iter().any(|t| t.label.contains("Aggregate cleanup")),
            "cleanup task for remaining nodes: {:?}",
            tasks.iter().map(|t| &t.label).collect::<Vec<_>>()
        );
    }
}
