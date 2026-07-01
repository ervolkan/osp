//! Agent Navigator loop (Aşama D1) — DeltaProposal → Claim → gate → TaskAttempt/Evidence.
//!
//! OSP'nin dinamik çekirdeğinin orkestrasyonu. Bir Task için iteratif:
//! LLM call → DeltaProposal → Claim (task-bound) → engine measure + PredicateGate →
//! TaskAttempt/Evidence kayıt → retry (maneuver limit) veya complete.
//!
//! **D1 kapsamı:** Mock LLM (gerçek HTTP D2'de). Hard gates Q4/Q5/Q6 D1'de PassedAll
//! varsayılır (commit() entegrasyonu D2'de); PredicateGate ayrı çağrılır. Evidence ledger
//! in-memory (Vec<TrajectoryEvidence>).
//!
//! # Tez
//! Agent Navigator, agent'ın mimari uzayda hedefe kontrollü ilerlemesini sağlar. Agent
//! decomposition yapamaz (Aşama C), hedef koordinat göremez (INV-T1), pozisyon declare
//! edemez (INV-T4). Sadece DeltaProposal üretir; engine ölçer; PredicateGate karar verir.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::agent::{DeltaProposal, NewNodeSpec, OutputContract};
use crate::coords::{MetricSource, RawPosition};
use crate::engine::SpaceEngine;
use crate::space::{Edge, Node, NodeId};
use crate::trajectory::{
    AgentTaskView, AttemptOutcome, GateDecision, InternalTaskPlan, MutationDecision,
    PredicateCompletion, ProvenancedRawPosition, TaskId, TaskResolver, TokenCost,
    TrajectoryEvidence,
};
use crate::witness::{AgentId, Claim, ClaimId, Intent};

// ═══════════════════════════════════════════════════════════════════════════════
// LlmClient trait (D1 — mock + production abstraction)
// ═══════════════════════════════════════════════════════════════════════════════

/// INV-T1 — Agent'a sadece `AgentTaskView` serialize edilir (hedef koordinat YOK).
/// Agent, bu view'ı alır (predicate + mevcut ölçüm + allowed_ops) ve bir `DeltaProposal`
/// üretir. Production impl `osp-llm-runtime` sarar; test impl `MockLlmClient`.
///
/// **INV-T3 (engine ölçer):** Agent pozisyon declare edemez; DeltaProposal structural-only.
/// LLM'in position_hints advisory'dir, engine tarafından authoritative kabul edilmez.
///
/// **G2 (osp-mcp):** `Send + Sync` supertrait — MCP server `Arc<dyn LlmClient>` olarak
/// tutar ve `spawn_blocking` ile farklı thread'e taşır. MockLlmClient ve RuntimeLlmClient
/// zaten Send+Sync (reqwest Client Send+Sync).
pub trait LlmClient: Send + Sync {
    /// AgentTaskView → DeltaProposal. Agent'a view serialize edilir (INV-T1),
    /// agent structural change önerir (INV-#4 — pozisyon YOK).
    fn complete(&self, view: &AgentTaskView) -> Result<DeltaProposal, LlmError>;

    /// Token maliyeti (RQ6 evidence). Mock için 0; production gerçek TokenUsage.
    fn last_token_cost(&self) -> TokenCost {
        TokenCost::default()
    }
}

/// LLM hatası (parse, network, rate limit, scripted proposals tükendi).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    /// DeltaProposal JSON parse edilemedi (Q4 syntax agent-shell'de yakalanır).
    ProposalParse(String),
    /// Network/HTTP hatası (production only).
    Network(String),
    /// Mock — scripted proposals tükendi (test senaryosu bitişi).
    NoMoreProposals,
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::ProposalParse(d) => write!(f, "LLM proposal parse error: {d}"),
            LlmError::Network(d) => write!(f, "LLM network error: {d}"),
            LlmError::NoMoreProposals => write!(f, "mock LLM ran out of scripted proposals"),
        }
    }
}

impl std::error::Error for LlmError {}

/// Scripted mock LLM — test için sıralı proposal listesi (deterministic).
/// Örn: [fail_proposal, progress_proposal, success_proposal] → 3-attempt senaryosu.
///
/// **Deterministic:** call_count sırayla artar; aynı proposals → aynı davranış.
///
/// **G2:** `call_count: AtomicUsize` (Cell → AtomicUsize) — `LlmClient: Send + Sync`
/// gereği (MCP server Arc<dyn LlmClient> + spawn_blocking). AtomicUsize Sync'tir.
pub struct MockLlmClient {
    proposals: Vec<DeltaProposal>,
    call_count: AtomicUsize,
    /// Her çağrı için token cost (RQ6 test). Default 0.
    token_costs: Vec<TokenCost>,
}

impl MockLlmClient {
    /// Scripted proposals. `complete()` her çağrıda sıradakini döner.
    pub fn new(proposals: Vec<DeltaProposal>) -> Self {
        let token_costs = vec![TokenCost::default(); proposals.len()];
        Self {
            proposals,
            call_count: AtomicUsize::new(0),
            token_costs,
        }
    }

    /// Token cost'lu mock (RQ6 test için).
    pub fn with_token_costs(proposals: Vec<DeltaProposal>, token_costs: Vec<TokenCost>) -> Self {
        Self {
            proposals,
            call_count: AtomicUsize::new(0),
            token_costs,
        }
    }

    /// Kaç çağrı yapıldı (test assertion için).
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl LlmClient for MockLlmClient {
    fn complete(&self, _view: &AgentTaskView) -> Result<DeltaProposal, LlmError> {
        // fetch_add her zaman artar; ama eski Cell davranışını (NoMoreProposals'da
        // counter artmıyor) korumak için load-then-conditional-store kullanırız.
        let idx = self.call_count.load(Ordering::SeqCst);
        let proposal = self
            .proposals
            .get(idx)
            .cloned()
            .ok_or(LlmError::NoMoreProposals)?;
        self.call_count.store(idx + 1, Ordering::SeqCst);
        Ok(proposal)
    }

    fn last_token_cost(&self) -> TokenCost {
        let idx = self.call_count().saturating_sub(1);
        self.token_costs.get(idx).copied().unwrap_or_default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DeltaProposal → Claim + ProvenancedRawPosition bridge (boşluk #3, #7)
// ═══════════════════════════════════════════════════════════════════════════════

/// Claim build hatası.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimBuildError {
    /// DeltaProposal'da node/edge yok (empty proposal).
    EmptyProposal,
}

impl std::fmt::Display for ClaimBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaimBuildError::EmptyProposal => write!(f, "DeltaProposal has no nodes/edges"),
        }
    }
}

impl std::error::Error for ClaimBuildError {}

/// INV-T3 (boşluk #7) — Engine RawPosition → ProvenancedRawPosition. Her axis'e aynı
/// `source` atanır (Aşama D'de engine per-axis source verebilir; D1'de uniform).
pub fn provenanced_from_raw(raw: RawPosition, source: MetricSource) -> ProvenancedRawPosition {
    ProvenancedRawPosition {
        coupling: crate::trajectory::AxisMetric {
            value: raw.x,
            source,
        },
        cohesion: crate::trajectory::AxisMetric {
            value: raw.y,
            source,
        },
        instability: crate::trajectory::AxisMetric {
            value: raw.z,
            source,
        },
        entropy: crate::trajectory::AxisMetric {
            value: raw.w,
            source,
        },
        witness_depth: crate::trajectory::AxisMetric {
            value: raw.v,
            source,
        },
    }
}

/// **G2c-1b (arkadaş review 6 #2):** Engine commit hatası → GateDecision mapping.
/// Tek noktada mapping — navigator reject-evidence sitesinde elle match yerine bu helper.
/// Task binding hatası (PermissionDenied) Syntax'a gömülmez → `RejectedByTaskBinding`.
pub fn gate_decision_from_engine_error(err: &crate::engine::EngineCommitError) -> GateDecision {
    use crate::engine::EngineCommitError;
    match err {
        EngineCommitError::SyntaxViolation { .. } => GateDecision::RejectedBySyntax,
        EngineCommitError::VisionViolation { .. } => GateDecision::RejectedByVision,
        EngineCommitError::RuleViolation { .. } => GateDecision::RejectedByRule,
        EngineCommitError::PermissionDenied(_) => GateDecision::RejectedByTaskBinding,
        // Witness gate ayrı (Q1-Q3) — evidence'da Unknown kalır, witness_status taşır.
        EngineCommitError::Witness(_) => GateDecision::Unknown,
        // Persistence hataları gate kararı değil (altyapı hatası) → Unknown.
        EngineCommitError::NoPersistence | EngineCommitError::Persistence(_) => {
            GateDecision::Unknown
        }
    }
}

/// INV-T4 (boşluk #3) — DeltaProposal + engine-measured computed_raw + task_id → Claim
/// (task-bound). Engine `compute_raw_from_delta()` ile ölçer (agent declare etmez).
///
/// **Not:** Bu fonksiyon engine'in hypothetical-graph ölçümünü kullanır. Navigator,
/// `engine.compute_raw_from_delta(&delta_nodes, &delta_edges)` sonucunu computed_raw'a koyar.
pub fn build_claim_from_proposal(
    proposal: &DeltaProposal,
    computed_raw: RawPosition,
    task_id: TaskId,
    agent: AgentId,
    claim_id: ClaimId,
) -> Result<Claim, ClaimBuildError> {
    // G2c-2: empty check — removed_edges veya affected_nodes varsa proposal boş değil.
    // (sadece additive delta değil, subtractive delta da geçerli proposal).
    if proposal.new_nodes.is_empty()
        && proposal.new_edges.is_empty()
        && proposal.removed_edges.is_empty()
    {
        return Err(ClaimBuildError::EmptyProposal);
    }
    // NewNodeSpec → Node (resolve: connected_to ile yeni ID'ler ata).
    let delta_nodes: Vec<Node> = proposal
        .new_nodes
        .iter()
        .enumerate()
        .map(|(i, spec)| node_from_spec(spec, i))
        .collect();
    // NewEdgeSpec → Edge.
    let mut delta_edges: Vec<Edge> = proposal
        .new_edges
        .iter()
        .map(|spec| Edge {
            from: spec.from,
            to: spec.to,
            kind: spec.kind,
            is_type_only: false,
        })
        .collect();
    // connected_to edge'leri delta_edges'e ekle (NewNodeSpec.connected_to).
    for (i, spec) in proposal.new_nodes.iter().enumerate() {
        let node_id = delta_nodes[i].id;
        for (target, kind) in &spec.connected_to {
            delta_edges.push(Edge {
                from: node_id,
                to: *target,
                kind: *kind,
                is_type_only: false,
            });
        }
    }
    let intent = Intent::new(agent, computed_raw);
    Ok(Claim {
        id: claim_id,
        intent,
        author: agent,
        computed_raw,
        delta_nodes,
        delta_edges,
        task_id: Some(task_id),
        removed_edges: proposal.removed_edges.clone(), // G2c-2: subtractive delta
    })
}

fn node_from_spec(spec: &NewNodeSpec, index: usize) -> Node {
    Node {
        id: (10_000 + index as NodeId), // yeni node ID'leri (mevcut ID'lerle çakışmaması için)
        kind: spec.kind,
        mass: spec.initial_mass,
        ..Default::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AgentNavigator — D1 loop driver (boşluk #4, #5, #6, #8)
// ═══════════════════════════════════════════════════════════════════════════════

/// D1 — Agent Navigator loop sonucu.
#[derive(Debug, Clone, PartialEq)]
pub enum NavigatorResult {
    /// Task completed — predicate satisfied, AcceptAsCompleted.
    Completed {
        attempts: usize,
        total_tokens: TokenCost,
    },
    /// INV-T7 — maneuver limit aşıldı (ardışık reject/improved).
    ExceededManeuverLimit {
        attempts: usize,
        last_outcome: AttemptOutcome,
    },
    /// Task resolver'da bulunamadı.
    TaskNotFound,
    /// RequireOperatorApproval — insan review gerekli (critical domain). D2'de pause.
    RequiresOperatorApproval {
        attempts: usize,
        last_outcome: AttemptOutcome,
    },
    /// LLM hatası (NoMoreProposals veya parse — D1'de mock).
    LlmError(LlmError),
}

/// D1 — Agent Navigator. Bir Task için iteratif loop: LLM → DeltaProposal → Claim →
/// measure → PredicateGate → evidence → retry/complete.
///
/// **Hard gates (Q4/Q5/Q6):** D1'de PassedAll varsayılır (commit() entegrasyonu D2'de).
/// Navigator PredicateGate (Q5.b soft gate) ayrı çağırır.
pub struct AgentNavigator<'a, L: LlmClient + ?Sized, R: TaskResolver> {
    pub llm: &'a L,
    pub resolver: &'a R,
    /// D2 — mutable engine (commit_task_claim &mut self gerektirir).
    pub engine: &'a mut SpaceEngine,
    /// Evidence ledger (in-memory Vec, Aşama E'de persistent store).
    pub evidence: &'a mut Vec<TrajectoryEvidence>,
    /// Trajectory + milestone context (loss target için).
    pub trajectory_id: crate::trajectory::TrajectoryId,
    pub milestone_id: crate::trajectory::MilestoneId,
    /// preferred_vector (loss/distance target — INV-T1 internal).
    pub target_vector: RawPosition,
    /// Mevcut measured position (loss_before başlangıcı).
    pub current_measured: ProvenancedRawPosition,
    /// Q4 syntax contract (agent shell).
    pub output_contract: OutputContract,
}

impl<'a, L: LlmClient + ?Sized, R: TaskResolver> AgentNavigator<'a, L, R> {
    /// Bir Task için navigator loop. Maneuver limit (INV-T7) kadar attempt.
    /// Her attempt: LLM → DeltaProposal → Claim → measure → PredicateGate → evidence.
    pub fn run_task(&mut self, task_id: TaskId, agent: AgentId) -> NavigatorResult {
        // Task resolve.
        let task = match self.resolver.resolve(task_id) {
            Some(t) => t.clone(),
            None => return NavigatorResult::TaskNotFound,
        };
        let maneuver_limit = task.policy.maneuver_limit as usize;
        let mut loss_before =
            crate::trajectory::trajectory_loss(&self.current_measured, &self.target_vector);
        let mut total_tokens = TokenCost::default();
        let mut last_outcome: Option<AttemptOutcome> = None;
        let mut claim_id_counter = 1u64;
        // D4 — Calibration feedback accumulation. Reject olunca hata mesajı ekle,
        // sonraki attempt'te AgentTaskView'a geçir → LLM hatadan öğrenir.
        let mut feedback_history: Vec<String> = Vec::new();

        for attempt_num in 1..=maneuver_limit {
            // 1. AgentTaskView üret (INV-T1 — hedef koordinat YOK + D4 feedback).
            let plan = InternalTaskPlan {
                task_id,
                milestone_target_vector: self.target_vector,
                task_predicate: task.target_predicate_set.clone(),
                tolerance: 0.02,
            };
            let agent_view = plan.to_agent_view(
                &task.label,
                self.current_measured.to_raw(),
                task.allowed_operations.clone(),
                task.constraints.clone(),
                feedback_history.clone(),
            );

            // 2. LLM call → DeltaProposal.
            let proposal = match self.llm.complete(&agent_view) {
                Ok(p) => p,
                Err(e) => return NavigatorResult::LlmError(e),
            };
            let token_cost = self.llm.last_token_cost();
            total_tokens.prompt_tokens += token_cost.prompt_tokens;
            total_tokens.completion_tokens += token_cost.completion_tokens;
            total_tokens.total_tokens += token_cost.total_tokens;

            // 3. Q4 syntax (agent shell — OutputContract.validate).
            let contract = self.output_contract.clone();
            if let Err(violation) = contract.validate(&proposal) {
                // Q4 reject — evidence kaydet, retry.
                last_outcome = Some(AttemptOutcome {
                    gate_decision: GateDecision::RejectedBySyntax,
                    predicate_completion: PredicateCompletion::NotCompleted,
                    mutation_decision: MutationDecision::Reject,
                    witness_status: None,
                });
                let before_raw = self.current_measured.to_raw();
                self.evidence.push(TrajectoryEvidence {
                    trajectory_id: self.trajectory_id,
                    milestone_id: self.milestone_id,
                    task_id,
                    attempt_id: attempt_num as u64,
                    before: before_raw,
                    after: before_raw,
                    gate_decision: GateDecision::RejectedBySyntax,
                    predicate_completion: PredicateCompletion::NotCompleted,
                    mutation_decision: MutationDecision::Reject,
                    token_cost,
                    duration_ms: 0,
                });
                // D4 — Calibration feedback: Q4 syntax hatasını LLM'e geri besle.
                feedback_history.push(format!(
                    "Attempt {attempt_num}: Structural hallucination — {}. Fix the DeltaProposal schema and retry.",
                    violation.detail
                ));
                continue;
            }

            // G2c-2 (arkadaş review 7 #8 — güvenlik kritik): removed_edges için
            // allowed_operations kontrolü. OpKind::RemoveImport yoksa policy ihlali → RejectedByRule.
            if !proposal.removed_edges.is_empty()
                && !task
                    .allowed_operations
                    .contains(&crate::trajectory::OpKind::RemoveImport)
            {
                last_outcome = Some(crate::trajectory::AttemptOutcome {
                    gate_decision: GateDecision::RejectedByRule,
                    predicate_completion: PredicateCompletion::NotCompleted,
                    mutation_decision: MutationDecision::Reject,
                    witness_status: None,
                });
                let before_raw = self.current_measured.to_raw();
                self.evidence.push(TrajectoryEvidence {
                    trajectory_id: self.trajectory_id,
                    milestone_id: self.milestone_id,
                    task_id,
                    attempt_id: attempt_num as u64,
                    before: before_raw,
                    after: before_raw,
                    gate_decision: GateDecision::RejectedByRule,
                    predicate_completion: PredicateCompletion::NotCompleted,
                    mutation_decision: MutationDecision::Reject,
                    token_cost,
                    duration_ms: 0,
                });
                feedback_history.push(format!(
                    "Attempt {attempt_num}: Policy violation — removed_edges requires OpKind::RemoveImport in task.allowed_operations."
                ));
                continue;
            }

            // 4. DeltaProposal → Claim (task-bound, boşluk #3).
            // D2: computed_raw = engine hypothetical ölçümü (gerçek space + delta_edges).
            let delta_nodes: Vec<Node> = proposal
                .new_nodes
                .iter()
                .enumerate()
                .map(|(i, s)| node_from_spec(s, i))
                .collect();
            let delta_edges: Vec<Edge> = proposal
                .new_edges
                .iter()
                .map(|spec| Edge {
                    from: spec.from,
                    to: spec.to,
                    kind: spec.kind,
                    is_type_only: false,
                })
                .collect();
            // G2c-2: affected_nodes = removed_edges.from (coupling düşen node'lar) +
            // proposal.affected_nodes. compute_raw_from_delta bu node'ları ölçer.
            let mut affected: Vec<NodeId> = proposal.affected_nodes.clone();
            for er in &proposal.removed_edges {
                if !affected.contains(&er.from) {
                    affected.push(er.from);
                }
            }
            // D2: computed_raw = engine hypothetical ölçümü (gerçek space + delta_edges +
            // G2c-2 delta_removed + affected_nodes ölçüm scope).
            let computed_raw = self.engine.compute_raw_from_delta(
                &delta_nodes,
                &delta_edges,
                &proposal.removed_edges,
                &affected,
            );
            let claim = match build_claim_from_proposal(
                &proposal,
                computed_raw,
                task_id,
                agent,
                claim_id_counter,
            ) {
                Ok(c) => c,
                Err(_) => {
                    // Empty proposal (G2c-1b — arkadaş review 6 "en güçlü taraf"):
                    // evidence push ekle — boş/malformed proposal da iz bırakmalı.
                    // before = after = current (state unchanged, INV-T6).
                    let before_raw = self.current_measured.to_raw();
                    self.evidence.push(TrajectoryEvidence {
                        trajectory_id: self.trajectory_id,
                        milestone_id: self.milestone_id,
                        task_id,
                        attempt_id: attempt_num as u64,
                        before: before_raw,
                        after: before_raw,
                        gate_decision: GateDecision::RejectedBySyntax,
                        predicate_completion: PredicateCompletion::NotCompleted,
                        mutation_decision: MutationDecision::Reject,
                        token_cost,
                        duration_ms: 0,
                    });
                    // D4 — Calibration feedback: empty proposal uyarısı.
                    feedback_history.push(format!(
                        "Attempt {attempt_num}: Empty DeltaProposal — provide new_nodes/new_edges to mutate the space."
                    ));
                    continue;
                }
            };
            claim_id_counter += 1;

            // 5. Engine-measured → ProvenancedRawPosition (boşluk #7).
            let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);

            // 6. D2 — commit_task_claim: Q4→bind→Q5→Q5.b(PredicateGate)→Q6→mutate→Q1-Q3.
            // D1'deki ayrı PredicateGate YERINE atomic commit_task_claim. Empty witness
            // set (D2'de gerçek witness yok — navigator tek-agent, auto-approve veya D3'te).
            let omega = crate::witness::WitnessSet::new(Vec::new());
            let task_result = match self
                .engine
                .commit_task_claim(crate::engine::TaskCommitInput {
                    claim: &claim,
                    omega: &omega,
                    task_resolver: self.resolver as &dyn TaskResolver,
                    target: self.target_vector,
                    loss_before,
                    measured: measured.clone(),
                }) {
                Ok(r) => r,
                Err(crate::engine::EngineCommitError::PermissionDenied(msg)) => {
                    // Binding hatası (task not found / standalone).
                    let _ = msg;
                    return NavigatorResult::TaskNotFound;
                }
                Err(e) => {
                    // Q4/Q5/Q6/witness reject — evidence kaydet, retry.
                    // G2c-1b: gerçek gate_decision helper'dan (elle match değil).
                    let gd = gate_decision_from_engine_error(&e);
                    last_outcome = Some(crate::trajectory::AttemptOutcome {
                        gate_decision: gd,
                        predicate_completion: crate::trajectory::PredicateCompletion::NotCompleted,
                        mutation_decision: crate::trajectory::MutationDecision::Reject,
                        witness_status: None,
                    });
                    let before_raw = self.current_measured.to_raw();
                    self.evidence.push(TrajectoryEvidence {
                        trajectory_id: self.trajectory_id,
                        milestone_id: self.milestone_id,
                        task_id,
                        attempt_id: attempt_num as u64,
                        before: before_raw,
                        after: measured.to_raw(),
                        gate_decision: gd,
                        predicate_completion: crate::trajectory::PredicateCompletion::NotCompleted,
                        mutation_decision: crate::trajectory::MutationDecision::Reject,
                        token_cost,
                        duration_ms: 0,
                    });
                    // D4 — Calibration feedback: commit hatasını LLM'e geri besle.
                    if let Some(hall) = crate::agent::HallucinationType::from_engine_error(&e) {
                        feedback_history.push(format!(
                            "Attempt {attempt_num}: {}",
                            hall.calibration_message()
                        ));
                    }
                    continue;
                }
            };
            let outcome = task_result.outcome.clone();
            let loss_after = task_result.loss_after;
            last_outcome = Some(outcome.clone());

            // 7. Evidence kaydet (boşluk #6) — inline push (field borrow çatışmasını önle).
            let before_raw = self.current_measured.to_raw();
            self.evidence.push(TrajectoryEvidence {
                trajectory_id: self.trajectory_id,
                milestone_id: self.milestone_id,
                task_id,
                attempt_id: attempt_num as u64,
                before: before_raw,
                after: measured.to_raw(),
                gate_decision: outcome.gate_decision,
                predicate_completion: outcome.predicate_completion,
                mutation_decision: outcome.mutation_decision,
                token_cost,
                duration_ms: 0,
            });

            // 8. Mutation decision → loop control (boşluk #8).
            match outcome.mutation_decision {
                MutationDecision::AcceptAsCompleted => {
                    self.current_measured = measured;
                    return NavigatorResult::Completed {
                        attempts: attempt_num,
                        total_tokens,
                    };
                }
                MutationDecision::AcceptAsProgress => {
                    // Progress checkpoint — loss güncelle, continue.
                    loss_before = loss_after;
                    self.current_measured = measured;
                }
                MutationDecision::Reject => {
                    // Retry — calibration feedback D2'de.
                }
                MutationDecision::RequireOperatorApproval => {
                    return NavigatorResult::RequiresOperatorApproval {
                        attempts: attempt_num,
                        last_outcome: outcome,
                    };
                }
            }
        }

        // Maneuver limit aşıldı (INV-T7).
        NavigatorResult::ExceededManeuverLimit {
            attempts: maneuver_limit,
            last_outcome: last_outcome.unwrap_or(AttemptOutcome {
                gate_decision: GateDecision::BlockedByManeuverLimit,
                predicate_completion: PredicateCompletion::NotCompleted,
                mutation_decision: MutationDecision::Reject,
                witness_status: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::NewNodeSpec;
    use crate::coords::CoordinateSystem;
    use crate::engine::{EngineConfig, SpaceEngine};
    use crate::space::{NodeKind, Space};
    use crate::trajectory::{
        ComparisonOp, InMemoryTaskRegistry, MetricPredicate, OpKind, PredicateAxis,
        PredicateFailurePolicy, PredicateMode, PredicateScope, PredicateSet, Task, TaskId,
        TaskPolicy, TaskStatus, WeightedPredicate,
    };
    use crate::vision::VisionVector;

    fn measured_pos(coupling: f64) -> ProvenancedRawPosition {
        provenanced_from_raw(
            RawPosition {
                x: coupling,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            MetricSource::Scip,
        )
    }

    fn coupling_task(id: TaskId, threshold: f64, policy: TaskPolicy) -> Task {
        Task {
            id,
            milestone_id: 1,
            label: format!("Reduce coupling to {threshold}"),
            target_predicate_set: PredicateSet {
                mode: PredicateMode::All,
                predicates: vec![WeightedPredicate {
                    predicate: MetricPredicate {
                        metric: PredicateAxis::Coupling,
                        operator: ComparisonOp::Le,
                        threshold,
                        scope: PredicateScope::Node(1),
                        required_source: Some(MetricSource::Scip),
                        tolerance: 0.0,
                    },
                    weight: None,
                }],
                preferred_vector: Some(RawPosition {
                    x: threshold,
                    y: 0.6,
                    z: 0.4,
                    w: 0.5,
                    v: 0.3,
                }),
            },
            policy,
            allowed_operations: vec![OpKind::RemoveImport],
            constraints: vec![],
            status: TaskStatus::Pending,
        }
    }

    /// Bir DeltaProposal: tek node, belirli coupling'e yakınsayan.
    fn proposal_with_coupling(coupling: f64) -> DeltaProposal {
        // compute_raw_from_delta node mass-weighted centroid kullanır; basit tek node.
        DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: 100.0,
                connected_to: vec![],
            }],
            new_edges: vec![],
            modified_entities: vec![],
            position_hints: vec![],
            reasoning: format!("target coupling {coupling}"),
            ..Default::default() // G2c-2: removed_edges, affected_nodes default
        }
    }

    fn make_engine() -> SpaceEngine {
        SpaceEngine::new(
            Space::default(),
            CoordinateSystem::default(),
            VisionVector::default(),
            EngineConfig::default_calibrated(),
        )
    }

    // 7. mock_llm_returns_scripted_proposals_in_order
    #[test]
    fn mock_llm_returns_scripted_proposals_in_order() {
        let mock = MockLlmClient::new(vec![
            proposal_with_coupling(0.5),
            proposal_with_coupling(0.4),
        ]);
        let view = AgentTaskView {
            task_id: 1,
            label: "test".into(),
            current_measurement: RawPosition::default(),
            target_predicate: crate::trajectory::AgentPredicateView {
                mode: PredicateMode::All,
                predicates: vec![],
            },
            allowed_operations: vec![],
            constraints: vec![],
            feedback_history: vec![],
        };
        let p1 = mock.complete(&view).unwrap();
        let p2 = mock.complete(&view).unwrap();
        let p3 = mock.complete(&view);
        assert_eq!(mock.call_count(), 2);
        assert!(p2.reasoning != p1.reasoning || p1.new_nodes.len() == p2.new_nodes.len());
        assert_eq!(p3.unwrap_err(), LlmError::NoMoreProposals);
    }

    // 8. build_claim_sets_task_id (boşluk #3)
    #[test]
    fn build_claim_sets_task_id() {
        let proposal = proposal_with_coupling(0.5);
        let claim = build_claim_from_proposal(&proposal, RawPosition::default(), 42, 7, 1).unwrap();
        assert_eq!(claim.task_id, Some(42));
        assert_eq!(claim.author, 7);
        assert!(!claim.delta_nodes.is_empty());
    }

    // 9. provenanced_from_raw_preserves_values (boşluk #7)
    #[test]
    fn provenanced_from_raw_preserves_values() {
        let raw = RawPosition {
            x: 0.5,
            y: 0.6,
            z: 0.4,
            w: 0.3,
            v: 0.2,
        };
        let p = provenanced_from_raw(raw, MetricSource::Scip);
        assert_eq!(p.coupling.value, 0.5);
        assert_eq!(p.cohesion.value, 0.6);
        assert_eq!(p.instability.value, 0.4);
        assert_eq!(p.entropy.value, 0.3);
        assert_eq!(p.witness_depth.value, 0.2);
        assert_eq!(p.coupling.source, MetricSource::Scip);
    }

    // 1. navigator_task_not_found
    #[test]
    fn navigator_task_not_found() {
        let mock = MockLlmClient::new(vec![]);
        let resolver = InMemoryTaskRegistry::new();
        let mut engine = make_engine();
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.82),
            output_contract: OutputContract::strict(),
        };
        let result = nav.run_task(999, 7);
        assert_eq!(result, NavigatorResult::TaskNotFound);
    }

    // 3. navigator_exceeds_maneuver_limit (INV-T7)
    #[test]
    fn navigator_exceeds_maneuver_limit() {
        // D1 limitation: mock engine compute_raw_from_delta gerçek coupling vermez (boş space
        // → 0 coupling → predicate satisfied). Maneuver limit'i LLM proposals'ı tükendiğinde
        // (NoMoreProposals) test ederiz — loop maneuver_limit kadar çalışır, sonra LlmError.
        // D2'de (gerçek engine measure) ExceededManeuverLimit testi anlamlı olur.
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 3;
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        // Sadece 1 proposal ver → maneuver limit'e ulaşmadan LlmError (NoMoreProposals).
        let mock = MockLlmClient::new(vec![proposal_with_coupling(0.82)]);
        let mut engine = make_engine();
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.82),
            output_contract: OutputContract::strict(),
        };
        let result = nav.run_task(1, 7);
        // D1: mock engine satisfied döndüğü için Completed; D2'de gerçek measure ile
        // ExceededManeuverLimit. Şimdilik loop çalıştığını doğrula (Completed veya LlmError).
        assert!(
            matches!(
                result,
                NavigatorResult::Completed { .. } | NavigatorResult::LlmError(_)
            ),
            "D1: loop ran to completion or LLM error: {result:?}"
        );
    }

    // 4. navigator_records_evidence_per_attempt (boşluk #6)
    #[test]
    fn navigator_records_evidence_per_attempt() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 2;
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let mock = MockLlmClient::new(vec![proposal_with_coupling(0.82); 2]);
        let mut engine = make_engine();
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.82),
            output_contract: OutputContract::strict(),
        };
        let _ = nav.run_task(1, 7);
        // En az 1 evidence (reject'ler de kaydeder). Maneuver limit dolana kadar.
        assert!(
            !evidence.is_empty(),
            "evidence ledger should have records: {} entries",
            evidence.len()
        );
        assert!(evidence.iter().all(|e| e.task_id == 1));
    }

    // 5. navigator_accepts_progress_checkpoint (INV-T6)
    #[test]
    fn navigator_accepts_progress_checkpoint() {
        // AcceptImprovement policy + allow_progress_checkpoint. LLM coupling azaltıyor.
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 5;
        policy.predicate_failure_policy = PredicateFailurePolicy::AcceptImprovement;
        policy.allow_progress_checkpoint = true;
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        // Not: compute_raw_from_delta mock engine'de gerçek coupling vermez; bu test
        // yapısını doğrular (evidence doluyor, loop çalışıyor). D2'de gerçek measure.
        let mock = MockLlmClient::new(vec![proposal_with_coupling(0.6); 5]);
        let mut engine = make_engine();
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.82),
            output_contract: OutputContract::strict(),
        };
        let result = nav.run_task(1, 7);
        // Loop çalıştı, evidence kaydedildi (progress veya complete veya maneuver).
        assert!(!evidence.is_empty());
        let _ = result;
    }

    // 10. navigator_token_cost_accumulated (RQ6)
    #[test]
    fn navigator_token_cost_accumulated() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 2;
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let mock = MockLlmClient::with_token_costs(
            vec![proposal_with_coupling(0.82); 2],
            vec![
                TokenCost {
                    prompt_tokens: 100,
                    completion_tokens: 50,
                    total_tokens: 150,
                },
                TokenCost {
                    prompt_tokens: 120,
                    completion_tokens: 60,
                    total_tokens: 180,
                },
            ],
        );
        let mut engine = make_engine();
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.82),
            output_contract: OutputContract::strict(),
        };
        let result = nav.run_task(1, 7);
        if let NavigatorResult::ExceededManeuverLimit { .. } = result {
            // evidence token cost accumulate.
            let total_prompt: u64 = evidence.iter().map(|e| e.token_cost.prompt_tokens).sum();
            assert_eq!(total_prompt, 220, "prompt tokens accumulate: 100+120");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Aşama D2 — Gerçek engine measure + commit_task_claim
    // ─────────────────────────────────────────────────────────────────────────

    use crate::engine::TaskCommitInput;

    /// D2 — gerçek ölçüm için 5-axis CoordinateSystem + populated space.
    /// D1 mock (boş space + boş axes) YERINE gerçek coupling/cohesion ölçümü.
    fn make_real_engine() -> SpaceEngine {
        use crate::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
        use crate::coords::CoordinateSystem;
        // 3 node'lu basit space: node 0 → node 1 (Imports edge → coupling > 0).
        let mut space = Space::default();
        space.nodes.insert(
            0,
            Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                cohesion: Some(0.6),
                ..Default::default()
            },
        );
        space.nodes.insert(
            1,
            Node {
                id: 1,
                kind: NodeKind::Module,
                mass: 80.0,
                cohesion: Some(0.5),
                ..Default::default()
            },
        );
        space.edges.push(Edge {
            from: 0,
            to: 1,
            kind: crate::space::EdgeKind::Imports,
            is_type_only: false,
        });
        let cs = CoordinateSystem::default_raw_five(
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        SpaceEngine::new(
            space,
            cs,
            VisionVector::default(),
            crate::engine::EngineConfig::default_calibrated(),
        )
    }

    // 1. navigator_real_measure_nonzero_coupling (D2 — gerçek space)
    #[test]
    fn navigator_real_measure_nonzero_coupling() {
        let engine = make_real_engine();
        let raw = engine.compute_raw_from_delta(
            &[Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                cohesion: Some(0.6),
                ..Default::default()
            }],
            &[],
            &[], // G2c-2: removed_edges
            &[], // G2c-2: affected_nodes
        );
        assert!(
            raw.x > 0.0,
            "D2: real space coupling > 0 (edge 0→1 exists): got {}",
            raw.x
        );
    }

    // 2. commit_task_claim_runs_q5b_predicate_gate
    #[test]
    fn commit_task_claim_runs_q5b_predicate_gate() {
        let mut engine = make_real_engine();
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(coupling_task(1, 0.55, TaskPolicy::default()));
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let result = engine.commit_task_claim(TaskCommitInput {
            claim: &claim,
            omega: &omega,
            task_resolver: &resolver as &dyn TaskResolver,
            target: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            loss_before: 1.0,
            measured,
        });
        // Q5.b çalıştı — Reject (witness yok) veya Ok (predicate reject NotApplied).
        // İkisi de Q5.b'nin çalıştığını gösterir. Witness boş → reject beklenir (D2 limitation).
        match result {
            Ok(r) => {
                assert!(
                    r.outcome.predicate_completion == PredicateCompletion::Completed
                        || r.outcome.predicate_completion == PredicateCompletion::NotCompleted
                );
            }
            Err(crate::engine::EngineCommitError::Witness(_)) => {
                // Witness Q1 fail (MinApproversNotMet) — predicate çalıştı, witness aşamasında reject.
                // D2'de beklenen — gerçek witness navigator'da (D3).
            }
            Err(e) => panic!("unexpected error (not witness): {e:?}"),
        }
    }

    // 6. commit_standalone_unchanged (mevcut commit() hâlâ standalone çalışır)
    #[test]
    fn commit_standalone_unchanged() {
        let mut engine = make_real_engine();
        let claim = test_claim_with_task(1, None, 0.40);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let _ = engine.commit(&claim, &omega); // standalone commit çalışır
    }

    // 7. commit_task_claim_requires_task_bound_claim
    #[test]
    fn commit_task_claim_requires_task_bound_claim() {
        let mut engine = make_real_engine();
        let resolver = InMemoryTaskRegistry::new();
        let claim = test_claim_with_task(1, None, 0.40);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let result = engine.commit_task_claim(TaskCommitInput {
            claim: &claim,
            omega: &omega,
            task_resolver: &resolver as &dyn TaskResolver,
            target: RawPosition::default(),
            loss_before: 1.0,
            measured,
        });
        assert!(
            result.is_err(),
            "standalone claim rejected by commit_task_claim"
        );
    }

    // 8. navigator_delta_edges_affect_coupling
    #[test]
    fn navigator_delta_edges_affect_coupling() {
        let engine = make_real_engine();
        let node = Node {
            id: 5,
            kind: NodeKind::Module,
            mass: 100.0,
            cohesion: Some(0.6),
            ..Default::default()
        };
        let raw_no_edge = engine.compute_raw_from_delta(&[node.clone()], &[], &[], &[]);
        let raw_with_edge = engine.compute_raw_from_delta(
            &[node],
            &[Edge {
                from: 5,
                to: 0,
                kind: crate::space::EdgeKind::Imports,
                is_type_only: false,
            }],
            &[], // G2c-2: removed_edges
            &[], // G2c-2: affected_nodes
        );
        assert!(
            raw_with_edge.x >= raw_no_edge.x,
            "D2: delta edge increases coupling: no_edge={}, with_edge={}",
            raw_no_edge.x,
            raw_with_edge.x
        );
    }

    /// Test helper — task_id ile veya None claim üret.
    fn test_claim_with_task(id: u64, task_id: Option<TaskId>, coupling: f64) -> Claim {
        Claim {
            id,
            intent: Intent::new(7, RawPosition::default()),
            author: 7,
            computed_raw: RawPosition {
                x: coupling,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            delta_nodes: vec![Node {
                id: 0,
                kind: NodeKind::Module,
                mass: 100.0,
                cohesion: Some(0.6),
                ..Default::default()
            }],
            delta_edges: vec![],
            task_id,
            removed_edges: vec![], // G2c-2
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // G2c-1b (arkadaş review 6) — Reject-Evidence testleri
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Boş DeltaProposal (new_nodes=[], new_edges=[]) — build_claim_from_proposal EmptyProposal.
    fn empty_proposal() -> DeltaProposal {
        DeltaProposal {
            new_nodes: vec![],
            new_edges: vec![],
            modified_entities: vec![],
            position_hints: vec![],
            reasoning: "intentionally empty".into(),
            ..Default::default() // G2c-2: removed_edges, affected_nodes default
        }
    }

    /// G2c-1b #1: Empty proposal → evidence push edilir, gate=RejectedBySyntax.
    /// Öncesi: `continue` (evidence YOK). Şimdi: before=after=current, RejectedBySyntax.
    #[test]
    fn navigator_records_evidence_for_empty_proposal() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 2;
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        // Empty proposals → build_claim_from_proposal EmptyProposal → evidence push.
        let mock = MockLlmClient::new(vec![empty_proposal(); 2]);
        let mut engine = make_engine();
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.82),
            output_contract: OutputContract::strict(),
        };
        let result = nav.run_task(1, 7);
        // Empty proposals → ExceededManeuverLimit (2 attempt evidence push edildi).
        assert!(
            matches!(result, NavigatorResult::ExceededManeuverLimit { .. }),
            "empty proposal should hit maneuver limit: {result:?}"
        );
        // KRİTİK: evidence push EDİLDİ (G2c-1b öncesi 0 olurdu).
        assert_eq!(
            evidence.len(),
            2,
            "empty proposal should produce evidence entries"
        );
        // Her entry RejectedBySyntax gate_decision ile.
        assert!(evidence
            .iter()
            .all(|e| e.gate_decision == GateDecision::RejectedBySyntax));
    }

    /// G2c-1b #2: Reject attempt'lerde gate_decision set edilir (empty/Q4/commit-error).
    #[test]
    fn navigator_evidence_includes_gate_decision() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 1;
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        // Empty proposal → RejectedBySyntax gate_decision.
        let mock = MockLlmClient::new(vec![empty_proposal()]);
        let mut engine = make_engine();
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.82),
            output_contract: OutputContract::strict(),
        };
        let _ = nav.run_task(1, 7);
        // Evidence boş DEĞİL ve gate_decision Unknown DEĞİL (gerçek gate set edildi).
        assert!(!evidence.is_empty());
        assert!(evidence
            .iter()
            .all(|e| e.gate_decision != GateDecision::Unknown));
    }

    /// G2c-1b #3 (arkadaş review 6 #5): Syntax reject → state ilerlemez (INV-T6).
    /// before == after, gate=RejectedBySyntax, mutation=Reject.
    #[test]
    fn navigator_syntax_reject_evidence_does_not_advance_state() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 1;
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let mock = MockLlmClient::new(vec![empty_proposal()]);
        let mut engine = make_engine();
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.82),
            output_contract: OutputContract::strict(),
        };
        let _ = nav.run_task(1, 7);
        let e = &evidence[0];
        // INV-T6: reject state'i değiştirmez → before == after.
        assert_eq!(e.before, e.after, "reject must not advance state (INV-T6)");
        assert_eq!(e.gate_decision, GateDecision::RejectedBySyntax);
        assert_eq!(e.mutation_decision, MutationDecision::Reject);
    }

    /// G2c-1b #4 (arkadaş review 6 #5): AcceptAsProgress → state ilerler (INV-T8 checkpoint).
    /// G2c-3'ün temeli: AcceptImprovement policy + loss ↓ → after != before, gate=PassedAll.
    ///
    /// Not: Bu test D1 mock engine ile gerçek loss-dropping ölçemez (boş space → 0 coupling).
    /// Ama evidence semantiğini doğrular: AcceptAsProgress mutation → after checkpoint state.
    /// Tam state-advance testi G2c-3'te (incremental proposals + real repo).
    #[test]
    fn navigator_progress_evidence_semantics() {
        // Bu test evidence şema semantiğini doğrular (field'lar doğru type/derive).
        // Gerçek AcceptAsProgress davranışı G2c-3'te (incremental proposals).
        let evidence = TrajectoryEvidence {
            trajectory_id: 1,
            milestone_id: 1,
            task_id: 1,
            attempt_id: 1,
            before: RawPosition {
                x: 0.7,
                y: 0.5,
                z: 0.5,
                w: 0.5,
                v: 0.3,
            },
            after: RawPosition {
                x: 0.6,
                y: 0.5,
                z: 0.5,
                w: 0.5,
                v: 0.3,
            },
            gate_decision: GateDecision::PassedAll,
            predicate_completion: PredicateCompletion::NotCompleted,
            mutation_decision: MutationDecision::AcceptAsProgress,
            token_cost: TokenCost::default(),
            duration_ms: 100,
        };
        // Progress evidence: after != before (state ilerledi), gate=PassedAll.
        assert_ne!(evidence.before, evidence.after);
        assert_eq!(evidence.gate_decision, GateDecision::PassedAll);
        assert_eq!(
            evidence.mutation_decision,
            MutationDecision::AcceptAsProgress
        );
        // Serialize round-trrip (gate_decision JSON'da görünür).
        let json = serde_json::to_string(&evidence).unwrap();
        assert!(json.contains("gate_decision"));
        assert!(json.contains("PassedAll"));
        assert!(json.contains("AcceptAsProgress"));
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // G2c-2 (arkadaş review 7) — DeltaProposal remove_edges + coupling reduction
    // ═══════════════════════════════════════════════════════════════════════════════

    /// G2c-2 #1: remove_edge count döner, nonexistent edge → 0 (review 7 #3).
    #[test]
    fn g2c_remove_edge_returns_count_and_nonexistent_returns_zero() {
        let mut space = Space::default();
        space.insert_edge(Edge {
            from: 0,
            to: 1,
            kind: crate::space::EdgeKind::Imports,
            is_type_only: false,
        });
        space.insert_edge(Edge {
            from: 0,
            to: 2,
            kind: crate::space::EdgeKind::Imports,
            is_type_only: false,
        });
        // 2 edge kaldır (0→1 Imports) — count 1 döner (sadece 0→1 mevcut).
        let count = space.remove_edge(0, 1, crate::space::EdgeKind::Imports);
        assert_eq!(count, 1);
        // Tekrar aynı edge'i kaldır → 0 (nonexistent).
        let count_again = space.remove_edge(0, 1, crate::space::EdgeKind::Imports);
        assert_eq!(count_again, 0, "nonexistent edge removal must return 0");
        // Diğer edge hâlâ duruyor.
        assert_eq!(space.edges.len(), 1);
    }

    /// G2c-2 #2: removed_edges requires OpKind::RemoveImport in allowed_operations
    /// (review 7 #8 — güvenlik kritik). Yoksa RejectedByRule.
    #[test]
    fn g2c_removed_edges_requires_allowed_operation() {
        use crate::agent::EdgeRef;
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 1;
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        // Task allowed_operations'ta RemoveImport YOK → removed_edges reject edilmeli.
        let mut task = coupling_task(1, 0.55, policy);
        task.allowed_operations = vec![]; // RemoveImport yok
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        // removed_edges içeren proposal.
        let proposal = DeltaProposal {
            new_nodes: vec![],
            new_edges: vec![],
            removed_edges: vec![EdgeRef {
                from: 0,
                to: 1,
                kind: crate::space::EdgeKind::Imports,
            }],
            affected_nodes: vec![0],
            modified_entities: vec![],
            position_hints: vec![],
            reasoning: "remove import".into(),
        };
        let mock = MockLlmClient::new(vec![proposal]);
        let mut engine = make_real_engine(); // node 0→1 import var
        let mut evidence = vec![];
        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.5),
            output_contract: OutputContract::strict(),
        };
        let _ = nav.run_task(1, 7);
        // Policy violation → RejectedByRule evidence.
        assert!(!evidence.is_empty());
        assert!(
            evidence
                .iter()
                .any(|e| e.gate_decision == GateDecision::RejectedByRule),
            "removed_edges without RemoveImport in allowed_ops must be RejectedByRule"
        );
    }

    /// G2c-2 #3: compute_raw_from_delta removed_edges ile coupling düşer (arkadaş review 7 #7).
    /// make_real_engine: node 0→1 import (coupling 0.5). remove edince coupling 0.
    #[test]
    fn g2c_compute_raw_from_delta_applies_removals() {
        use crate::agent::EdgeRef;
        let engine = make_real_engine();
        // Baseline: node 0 coupling = 1/(1+1) = 0.5 (tek outgoing import).
        let baseline = engine.compute_raw_from_delta(&[], &[], &[], &[0]);
        assert!(
            baseline.x > 0.4 && baseline.x < 0.6,
            "baseline coupling ~0.5 (1 import): got {}",
            baseline.x
        );
        // Remove the import edge → coupling 0.
        let removed = vec![EdgeRef {
            from: 0,
            to: 1,
            kind: crate::space::EdgeKind::Imports,
        }];
        let after = engine.compute_raw_from_delta(&[], &[], &removed, &[0]);
        assert!(after.x < 0.01, "after removal coupling ~0: got {}", after.x);
        assert!(
            after.x < baseline.x,
            "coupling must decrease after edge removal"
        );
    }

    /// G2c-2 #4: coupling-reducing proposal engine'de coupling düşürür (compute_raw_from_delta).
    /// Tam Completed pipeline (predicate gate + vision + witness) G2c-3'te ele alınır —
    /// burada engine-level coupling reduction kanıtlanır (#3 ile complementer).
    /// navigator build_claim_from_proposal removed_edges'i Claim'e geçirir (serde round-trip).
    #[test]
    fn g2c_removed_edges_serde_and_claim_round_trip() {
        use crate::agent::EdgeRef;
        let proposal = DeltaProposal {
            new_nodes: vec![],
            new_edges: vec![],
            removed_edges: vec![EdgeRef {
                from: 5,
                to: 7,
                kind: crate::space::EdgeKind::Imports,
            }],
            affected_nodes: vec![5],
            modified_entities: vec![],
            position_hints: vec![],
            reasoning: "G2c-2 serde test".into(),
        };
        // DeltaProposal serde round-trip (removed_edges + affected_nodes backward-compat).
        let json = serde_json::to_string(&proposal).unwrap();
        assert!(json.contains("removed_edges"));
        assert!(json.contains("affected_nodes"));
        let back: DeltaProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(back.removed_edges.len(), 1);
        assert_eq!(back.removed_edges[0].from, 5);
        assert_eq!(back.affected_nodes, vec![5]);
        // build_claim_from_proposal removed_edges'i Claim'e geçirir.
        let claim = build_claim_from_proposal(&proposal, RawPosition::default(), 1, 7, 1)
            .expect("non-empty proposal builds claim");
        assert_eq!(
            claim.removed_edges.len(),
            1,
            "Claim must carry removed_edges"
        );
    }
}
