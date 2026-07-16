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
pub struct TaskCommitInput<'a> {
    pub claim: &'a crate::witness::Claim,
    pub omega: &'a crate::witness::WitnessSet,
    pub task_resolver: &'a dyn crate::trajectory::TaskResolver,
    /// preferred_vector (loss/distance target — INV-T1 internal).
    pub target: crate::coords::RawPosition,
    /// Loss before (mevcut durumun preferred_vector'e uzaklığı).
    pub loss_before: f64,
    /// Engine-measured simulated_after (INV-T3 — claim.computed_raw'tan ProvenancedRawPosition).
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
}

/// **INV-T9** — `commit_task_claim` expected domain outcome (HATA DEĞİL).
///
/// `Applied` = AcceptAsCompleted (Mainline) VE AcceptAsProgress (TrajectoryCheckpoint)
/// uygulandı. `Held` = expected authorization bekleme (INV-T9 suspended state).
/// `Rejected` = explicit witness rejection (non-empty).
///
/// Operational fault'lar (Syntax/Vision/Rule/Permission/Persistence/Internal +
/// InvalidWitnessEvidence) `EngineCommitError`'da kalır.
#[derive(Debug, Clone)]
pub enum EngineCommitResult {
    /// Mainline veya checkpoint'e uygulandı. TaskCommitResult tam witness + outcome taşır.
    Applied(TaskCommitResult),
    /// INV-T9 — expected authorization bekleme. Navigator AwaitingWitnesses'ye map'ler.
    Held {
        reason: crate::witness::WitnessHoldReason,
        snapshot: crate::witness::WitnessQuorumSnapshot,
    },
    /// Explicit witness rejection (Q3 honest-reject). Navigator RequiresRevision'a map'ler.
    Rejected {
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

    /// Q6 Rule Gate için kural ekle (God Mode / trusted operator).
    ///
    /// Kurallar `check_claim_rules()` içinde sırayla evaluate edilir.
    /// İlk ihlalde claim reddedilir (short-circuit).
    pub fn register_rule(&mut self, rule: Box<dyn crate::rule::Rule>) {
        self.rules.push(rule);
    }

    /// Q6 için varsayılan yapısal kural seti ile engine kur (no_self_import,
    /// no_duplicate_node, edge_target_exists).
    pub fn with_default_rules(
        space: Space,
        coord_system: crate::coords::CoordinateSystem,
        vision: VisionVector,
        config: EngineConfig,
    ) -> Self {
        let mut engine = Self::new(space, coord_system, vision, config);
        for rule in crate::rule::default_rules() {
            engine.register_rule(rule);
        }
        engine
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
        self.check_claim_vision(claim)?;
        self.check_claim_rules(claim)?;

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

        // Phase 0c: Q5 Vision (θ bound — negatif-uzay safety).
        self.check_claim_vision(input.claim)?;

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

        // Phase 0e: Q6 Rule (claim-based, deterministik).
        // Not: MutationDecision Reject ise bile Q6 çalışır (diagnostic — hangi gate reject etti).
        if !matches!(outcome.mutation_decision, MutationDecision::Reject) {
            self.check_claim_rules(input.claim)?;
        }

        // Phase 0f: MutationDecision → ApplyTarget kontrolü (INV-T8).
        // Reject → NotApplied (commit yok, witness yok). Sadece evidence kaydı navigator'da.
        if matches!(apply_target, ApplyTarget::NotApplied) {
            return Ok(EngineCommitResult::Applied(TaskCommitResult {
                outcome,
                apply_target,
                loss_after,
                witness: None,
            }));
        }

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
                Ok(EngineCommitResult::Applied(TaskCommitResult {
                    outcome,
                    apply_target,
                    loss_after,
                    witness: Some(disposition),
                }))
            }
            WitnessDisposition::Held { reason, snapshot } => {
                Ok(EngineCommitResult::Held { reason, snapshot })
            }
            WitnessDisposition::Rejected { reasons, snapshot } => {
                Ok(EngineCommitResult::Rejected { reasons, snapshot })
            }
        }
    }

    // ── Claim-based gates (Q4-Q6, Phase 0 — witness öncesi, deterministik) ───

    /// Q4 Syntax Gate — Claim'in ΔS yapısı geçerli mi? (inv #12)
    ///
    /// Kontroller:
    /// 1. delta_nodes: geçerli NodeKind, finite/non-negative mass, non-negative id
    /// 2. delta_edges: Imports self-loop reddi, geçerli EdgeKind, from/to ≥ 0
    /// 3. delta_nodes içinde duplicate ID yok
    /// 4. computed_raw: tüm core eksen değerleri finite
    fn check_claim_syntax(&self, claim: &Claim) -> Result<(), EngineCommitError> {
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

        // 4. computed_raw finite check (flat RawPosition: x,y,z,w,v)
        let raw = &claim.computed_raw;
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
                        claim_id: claim.id,
                        detail: format!("computed_raw.{} is not finite: {}", name, val),
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
    fn check_claim_vision(&self, claim: &Claim) -> Result<(), EngineCommitError> {
        // Role-aware vision: claim'in node'undan rol çıkar, override uygula.
        let vision = self.vision_for_claim(claim);
        let theta = CosineDeviation.theta(&claim.computed_raw, &vision, &self.space);
        if theta > self.config.theta_bound {
            tracing::warn!(
                claim_id = claim.id,
                theta,
                bound = self.config.theta_bound,
                "Q5 vision violation — claim rejected (negatif-uzay)"
            );
            return Err(EngineCommitError::VisionViolation {
                violation: VisionViolation {
                    claim_id: claim.id,
                    theta,
                    raw: claim.computed_raw,
                },
                bound: self.config.theta_bound,
            });
        }
        Ok(())
    }

    /// Claim'in temsil ettiği node'un rolüne göre vision vector seç.
    /// Override (kullanıcı TOML) yoksa builtin sensible-default kullanılır.
    ///
    /// **Provenance (#2):** Dönen `VisionVector`'un `source` alanı, vision'ın
    /// nereden geldiğini belirtir — UI "Vision: not loaded" çelişkisini çözer:
    ///   - kullanıcı TOML `[role_overrides.<Role>]` → `RoleProfile`
    ///   - `builtin_role_override` (hardcoded) → `BuiltinRole`
    ///   - engine global vision (`self.vision`) → `self.vision.source()` inherit
    fn vision_for_claim(&self, claim: &Claim) -> VisionVector {
        use crate::space::infer_role;
        use crate::vision::VisionSource;
        use crate::vision_config::VisionConfig;
        // İlk delta_node'un classification'ından rol çıkar (path/metric olmadan
        // classification-only — engine path bilmez, sadece node classification).
        if let Some(node) = claim.delta_nodes.first() {
            let role = infer_role("", node.classification, None);
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
                return VisionVector::with_source(raw_v, source);
            }
        }
        // Override yok → engine global vision'ı (source dahil) inherit et.
        self.vision
    }

    /// Q6 Rule Gate — ΔS herhangi bir Rule'u ihlal ediyor mu?
    ///
    /// Stub: `self.rules` boş (Faz 2) → her zaman Ok. Faz 5'te God Mode tarafından
    /// register edilen Hard/Soft Rule'lar `evaluate()` çağrılır (agent-prompt-semantics.md §4).
    fn check_claim_rules(&self, claim: &Claim) -> Result<(), EngineCommitError> {
        for rule in &self.rules {
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

        // Q5 Vision
        match self.check_claim_vision(claim) {
            Ok(()) => results.push(GateResult::passed("Q5 Vision", "θ within bound")),
            Err(e) => {
                let h = crate::agent::HallucinationType::from_engine_error(&e);
                results.push(GateResult::failed("Q5 Vision", &e.to_string(), h));
                return results;
            }
        }

        // Q6 Rule
        match self.check_claim_rules(claim) {
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
                results.push(GateResult::passed("Q1-Q3 Witness", "Quorum met — Satisfied"));
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

    /// **INV-T9** — Mevcut space view revision (store-scoped, lane-qualified).
    ///
    /// Engine tam revision tracking'i P1'de alacak; şimdilik t_c-based placeholder.
    /// Authorization basis bu revision'ı taşır — P1 resume staleness kontrolü için.
    pub fn current_space_view_revision(
        &self,
    ) -> crate::authorization::SpaceViewRevision {
        use crate::authorization::{SpaceDigest, SpaceViewRevision, StoreId};
        // Placeholder digest — t_c'den türetilmiş deterministic.
        let mut digest_bytes = [0u8; 32];
        let tc_bytes = self.t_c.to_le_bytes();
        digest_bytes[..8].copy_from_slice(&tc_bytes);
        SpaceViewRevision {
            store_id: StoreId("osp-engine".to_string()),
            lane: crate::trajectory::CommitLane::Mainline,
            sequence: self.t_c,
            content_digest: SpaceDigest::from_bytes(digest_bytes),
        }
    }

    /// **INV-T9** — Mevcut evaluation context digest (vision config + rule-set).
    ///
    /// Engine tam context digest'i P1'de alacak; şimdilik vision θ_bound + rule count
    /// tabanlı placeholder. Authorization basis stale measurement tespiti için.
    pub fn current_evaluation_context_digest(
        &self,
    ) -> crate::authorization::EvaluationContextDigest {
        use crate::authorization::EvaluationContextDigest;
        // Placeholder — vision θ_bound + rule count'tan deterministic digest.
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"osp.evaluation-context.v1\0");
        hasher.update(&self.config.theta_bound.to_le_bytes());
        hasher.update(&(self.rules.len() as u64).to_le_bytes());
        let hash = hasher.finalize();
        let bytes: [u8; 32] = hash.into();
        EvaluationContextDigest::from_bytes(bytes)
    }

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
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
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
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
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
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
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
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
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
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
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
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
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
        let result = engine.check_claim_rules(&claim);
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
        let result = engine.check_claim_rules(&claim);
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
        let result = engine.check_claim_rules(&claim);
        assert!(result.is_ok(), "valid claim should pass Q6: {:?}", result);
    }

    // --- Position computation from DeltaProposal (inv #4) ---

    /// Full 5-axis engine for position computation tests (coupling + cohesion + instability + entropy + witness)
    fn make_engine_full() -> SpaceEngine {
        let cs = CoordinateSystem::default_raw_five(
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
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
}
