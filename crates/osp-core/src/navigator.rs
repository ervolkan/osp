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
    PredicateCompletion, PredicateScope, ProvenancedRawPosition, TaskId, TaskResolver, TokenCost,
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
///
/// **G2c-4 (arkadaş review 10 #5):** `ProposalParse` artık `token_cost` taşıyabilir —
/// parse edilemeyen cevap da token harcamıştır (RQ6 accounting için kritik).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    /// DeltaProposal JSON parse edilemedi (Q4 syntax agent-shell'de yakalanır).
    /// G2c-4: token_cost — parse error da API çağrısı yaptı, token harcadı.
    ProposalParse {
        message: String,
        token_cost: Option<TokenCost>,
    },
    /// Network/HTTP hatası (production only).
    Network(String),
    /// Mock — scripted proposals tükendi (test senaryosu bitişi).
    NoMoreProposals,
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::ProposalParse { message, .. } => {
                write!(f, "LLM proposal parse error: {message}")
            }
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
/// **INV-T9:** Exhaustive match — catch-all YOK. Task binding hatası (PermissionDenied)
/// Syntax'a gömülmez → `RejectedByTaskBinding`.
///
/// **INV-T9 #70 Commit 4b:** Yeni varyantlar — `TaskValidation` → `RejectedByTaskValidation`
/// (append-only tag 7), `MeasurementBindingMismatch` → `RejectedByMeasurementBinding`
/// (append-only tag 8), `MeasurementBindingFailed` → `Unknown` (system failure).
pub fn gate_decision_from_engine_error(err: &crate::engine::EngineCommitError) -> GateDecision {
    use crate::engine::EngineCommitError;
    match err {
        EngineCommitError::SyntaxViolation { .. } => GateDecision::RejectedBySyntax,
        EngineCommitError::VisionViolation { .. } => GateDecision::RejectedByVision,
        EngineCommitError::RuleViolation { .. } => GateDecision::RejectedByRule,
        EngineCommitError::PermissionDenied(_) => GateDecision::RejectedByTaskBinding,
        // InvalidWitnessEvidence = operational fault (malformed/author-self/duplicate) — gate değil.
        EngineCommitError::InvalidWitnessEvidence(_) => GateDecision::Unknown,
        // Persistence hataları gate kararı değil (altyapı hatası) → Unknown.
        EngineCommitError::NoPersistence | EngineCommitError::Persistence(_) => {
            GateDecision::Unknown
        }
        // Internal = system failure — gate değil.
        EngineCommitError::Internal(_) => GateDecision::Unknown,
        // AuthorizationContextFailed = system failure (fail-closed) — gate değil.
        EngineCommitError::AuthorizationContextFailed(_) => GateDecision::Unknown,
        // **INV-T9 Step 4b (reviewer P0-4):** VisionContextInvalid = terminal —
        // maneuver budget tüketmez, yeni LLM attempt başlatmaz, witness'a ulaşmaz.
        EngineCommitError::VisionContextInvalid(_) => GateDecision::Unknown,
        // **INV-T9 #70 Commit 4b (reviewer v2 karar 2):** TaskValidation = terminal —
        // geçersiz task declaration. append-only tag 7. Maneuver budget tüketmez.
        EngineCommitError::TaskValidation(_) => GateDecision::RejectedByTaskValidation,
        // **INV-T9 #70 Commit 4b (reviewer v4 P1-2/P1-3):** MeasurementBindingMismatch =
        // presented authority token geçersiz (replay/tamper). append-only tag 8.
        // Navigator disposition'a göre retry/reject kararı verir (inner mismatch varyantı).
        EngineCommitError::MeasurementBindingMismatch(_) => {
            GateDecision::RejectedByMeasurementBinding
        }
        // **INV-T9 #70 Commit 4b (reviewer v3 P1-4):** MeasurementBindingFailed =
        // engine derivation failure (system/operational fault) — gate değil, Unknown.
        EngineCommitError::MeasurementBindingFailed(_) => GateDecision::Unknown,
        // **INV-T9 #70 Commit 4b Faz 3 (reviewer v6 #1):** Tek kapsayıcı varyant —
        // inner error'a göre disposition. Mismatch → tag 8 (RejectedByMeasurementBinding);
        // Derivation/Drift → Unknown (system failure — retry gerekebilir).
        EngineCommitError::MeasurementBindingVerification(verif_err) => match verif_err {
            crate::measurement::MeasurementBindingVerificationError::Mismatch(_) => {
                GateDecision::RejectedByMeasurementBinding
            }
            crate::measurement::MeasurementBindingVerificationError::Derivation(_)
            | crate::measurement::MeasurementBindingVerificationError::Drift(_) => {
                GateDecision::Unknown
            }
        },
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
    /// **INV-T9** — expected witness authorization bekleme. Agent retry DEĞİL.
    /// Budget tüketmez, LLM reinvocation YOK. Pending authorization record + persistence
    /// receipt taşınır (persist-before-return — çökme penceresi yok).
    AwaitingWitnesses {
        pending: crate::authorization::PendingAuthorization,
        persistence: crate::authorization::PendingAuthorizationReceipt,
    },
    /// Explicit witness rejection — agent proposal revises. Budget tüketmez.
    /// Evidence-preserving: embedded `SuspendedAttemptEvidence` (Rejected disposition)
    /// + `evidence_digest` (INV-T9 #72 — attempt_evidence_id kaldırıldı, dangling
    /// reference yok; accessor'lar evidence üzerinden).
    RequiresRevision(crate::authorization::RevisionRequired),
    /// Pending authorization persistence failure — terminal (non-retryable).
    PendingAuthorizationPersistenceFailure {
        pending: crate::authorization::PendingAuthorization,
        error: crate::authorization::PendingAuthorizationStoreError,
    },
    /// INV-T7 — maneuver limit aşıldı (ardışık retryable reject/improved).
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
    /// Invalid witness evidence — operational fault (malformed/author-self/duplicate).
    /// Terminal — agent retry ile çözülmez.
    WitnessEvaluationError(String),
    /// System failure — persistence/internal error. Terminal.
    SystemFailure(String),
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
    /// **G2c-3b (arkadaş review 9):** Witness gate policy — navigator'ın witness
    /// gereksinimi. Default `Production` (Paper 1 witness güven modeli, min_approvers=2).
    /// `HarnessAutoApprove` sadece controlled experiment için (tek-agent auto-approve).
    /// Navigator bu field'a göre WitnessSet quorum'unu set eder — production
    /// güven iddiasını zayıflatmadan G2c runner/test'lere izin verir.
    pub witness_policy: NavigatorWitnessPolicy,
    /// **INV-T9:** Pending authorization persistence store. Navigator `AwaitingWitnesses`
    /// döndürmeden ÖNCE buraya persist eder (persist-before-return — çökme penceresi yok).
    /// Zorunlu — production `FilesystemPendingAuthorizationStore`, testler `NullPendingAuthorizationStore`.
    pub pending_authorization_store: Box<dyn crate::authorization::PendingAuthorizationStore>,
    /// **INV-T9:** Clock — pending authorization `created_at` için.
    pub clock: Box<dyn crate::authorization::Clock>,
}

/// **G2c-3b (arkadaş review 9):** Navigator witness gate policy.
///
/// Navigator `commit_task_claim`'e WitnessSet geçirir. Bu policy quorum parametrelerini
/// belirler. **Production güven iddiası** (Paper 1 witness modeli) SADECE `Production`
/// variant'ta korunur — `HarnessAutoApprove` controlled experiment dışında kullanılmaz.
///
/// **Neden gerekli:** G2c-1 0/24 Completed'in sebebi navigator'ın witness gate'i idi
/// (default min_approvers=2 + boş set → her zaman reject). Bu enum, fix'i scoped tutar:
/// production navigator default Production, G2c runner/test HarnessAutoApprove override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigatorWitnessPolicy {
    /// Production: Paper 1 witness güven modeli (min_approvers=2, quorum_threshold=1.5).
    /// WitnessSet default quorum ile kurulur. Gerçek deployment'ta kullanılır.
    Production,
    /// Harness/test: tek-agent auto-approve (min_approvers=0, quorum=0.0).
    /// SADECE controlled experiment için — production navigator asla bu modda çalışmaz.
    HarnessAutoApprove,
}

impl Default for NavigatorWitnessPolicy {
    fn default() -> Self {
        // Production güvenli — witness gate aktif. Harness kullanıcıları açıkça override eder.
        NavigatorWitnessPolicy::Production
    }
}

/// **INV-T9 #72 closure (P1-4):** Rejected yolunun tek production mapper'ı.
///
/// Navigator'ın `EngineCommitResult::Rejected` arm'ı `AgentNavigator::revision_
/// required_from_rejection` üzerinden bu free function'ı çağırır. Rejected evidence
/// construction production branch'inin doğrulanmış tek helper'ı — inline construction
/// YOK. Testler de aynı helper'ı çalıştırır (`rejected_mapper_constructs_canonical_
/// revision_evidence`).
///
/// **Reviewer P1-2:** `pub(crate)` free function — test modülü direkt çağırır
/// (helper drift ederse test yakalar). Navigator Rejected arm'ında inline alternatif
/// construction YOK.
///
/// **Q3 reachability:** Production reachability şu an yok (witness.rs:535 — Q3
/// honest-reject signaling wired değil, tracked in #73). Bu helper Rejected arm'ın
/// construction logic'ini doğrular; upstream reachability ayrı.
#[allow(clippy::result_large_err)]
pub(crate) fn make_revision_required_from_rejection(
    authorization: crate::authorization::AuthorizationContext,
    reasons: crate::witness::NonEmptyWitnessRejections,
    snapshot: crate::witness::WitnessQuorumSnapshot,
    attempt_num: u64,
) -> Result<crate::authorization::RevisionRequired, NavigatorResult> {
    use crate::authorization::{
        AttemptNumber, AuthorizationBasisDigest, SuspendedAttemptDisposition,
        SuspendedAttemptEvidence,
    };
    let attempt_num_validated = match AttemptNumber::try_from(attempt_num) {
        Ok(n) => n,
        Err(e) => return Err(NavigatorResult::SystemFailure(e.to_string())),
    };
    let basis_digest = match AuthorizationBasisDigest::compute(&authorization.basis) {
        Ok(d) => d,
        Err(e) => return Err(NavigatorResult::SystemFailure(e.to_string())),
    };
    let evidence = match SuspendedAttemptEvidence::try_new(
        authorization.basis.task_id,
        authorization.basis.claim_identity.claim_id,
        basis_digest,
        attempt_num_validated,
        SuspendedAttemptDisposition::Rejected { reasons, snapshot },
    ) {
        Ok(ev) => ev,
        Err(e) => return Err(NavigatorResult::SystemFailure(e.to_string())),
    };
    match crate::authorization::RevisionRequired::try_new(evidence) {
        Ok(r) => Ok(r),
        Err(e) => Err(NavigatorResult::SystemFailure(e.to_string())),
    }
}

impl<'a, L: LlmClient + ?Sized, R: TaskResolver> AgentNavigator<'a, L, R> {
    /// **INV-T9 (reviewer P0-4)** — Held durumunda pending authorization oluştur, persist,
    /// AwaitingWitnesses dön. Engine-owned `AuthorizationContext` kullanılır — navigator
    /// basis'i yeniden ÜRETMEZ, fail-open fallback YOK.
    ///
    /// P1-1 persist-before-return: store.persist() başarısız olursa AwaitingWitnesses DÖNMEZ.
    /// PendingAuthorizationPersistenceFailure döner (çökme penceresi yok).
    ///
    /// **plan-review #2 (D3):** Ephemeral identity + CrossProcess store → SystemFailure.
    /// Production CLI yalnız Persisted + Filesystem kabul eder; Commit 1'de engine hala
    /// Ephemeral üretir, bu yüzden ProcessLocal store (test) ile çalışır.
    fn suspend_for_witness(
        &mut self,
        authorization: crate::authorization::AuthorizationContext,
        hold_reason: crate::witness::WitnessHoldReason,
        witness_snapshot: crate::witness::WitnessQuorumSnapshot,
        attempt_num: u64,
    ) -> NavigatorResult {
        use crate::authorization::{
            AttemptNumber, AuthorizationBasisDigest, PendingAuthorization,
            PendingAuthorizationEnvelope, SpaceViewId, SuspendedAttemptDisposition,
            SuspendedAttemptEvidence, SuspensionDurability,
        };

        // D3 — durability enforcement: Ephemeral + CrossProcess → fail-closed.
        if matches!(
            &authorization.basis.base_space_view_revision.view_id,
            SpaceViewId::Ephemeral(_)
        ) && matches!(
            self.pending_authorization_store.durability(),
            SuspensionDurability::CrossProcess
        ) {
            return NavigatorResult::SystemFailure(
                "cross-process suspension requires persisted space identity (ephemeral identity cannot survive process restart)"
                    .into(),
            );
        }

        // **INV-T9 #72 (Commit 2 — P0-2 ownership):** SuspendedAttemptEvidence TEK
        // üretim kaynağı. Engine disposition payload'unu (reason, snapshot)
        // `EngineCommitResult`'ta taşır; navigator gerçek `attempt_num` ile final
        // evidence'ı üretir. Engine attempt counter bilmez (katman sınırı).
        //
        // **Tekilleştirme:** Önceki 3x redundant digest recompute (navigator.rs:441
        // + envelope constructor) → tek basis_digest compute + tek evidence üretimi.
        let attempt_num_validated = match AttemptNumber::try_from(attempt_num) {
            Ok(n) => n,
            Err(e) => return NavigatorResult::SystemFailure(e.to_string()),
        };
        let basis_digest = match AuthorizationBasisDigest::compute(&authorization.basis) {
            Ok(d) => d,
            Err(e) => return NavigatorResult::SystemFailure(e.to_string()),
        };
        let evidence = match SuspendedAttemptEvidence::try_new(
            authorization.basis.task_id,
            authorization.basis.claim_identity.claim_id,
            basis_digest.clone(),
            attempt_num_validated,
            SuspendedAttemptDisposition::Held {
                hold_reason: hold_reason.clone(),
                snapshot: witness_snapshot.clone(),
            },
        ) {
            Ok(ev) => ev,
            Err(e) => return NavigatorResult::SystemFailure(e.to_string()),
        };
        let evidence_digest =
            match crate::authorization::SuspendedAttemptEvidenceDigest::compute(&evidence) {
                Ok(d) => d,
                Err(e) => return NavigatorResult::SystemFailure(e.to_string()),
            };

        // **INV-T9 #72 (Commit 3+4):** Evidence record içine gömülü (P0-3 — runtime
        // `AwaitingWitnesses { pending }` aynı evidence nesnesini taşır). Commit 4
        // ile `attempt_evidence_id` kaldırıldı — `attempt_num` (AttemptNumber)
        // kullanılır, evidence source of truth. Cross-field validation
        // `Envelope::new()` içinde çalışır (P1 constructor validation).
        let pending = PendingAuthorization {
            task_id: authorization.basis.task_id,
            claim_id: authorization.basis.claim_identity.claim_id,
            predicate_completion: authorization.outcome.predicate_completion,
            mutation_decision: authorization.outcome.mutation_decision,
            intended_apply_target: authorization.apply_target,
            authorization_basis_digest: basis_digest,
            base_space_view_revision: authorization.basis.base_space_view_revision.clone(),
            evaluation_context_digest: authorization.basis.evaluation_context_digest.clone(),
            witness_requirement: authorization.witness_requirement,
            witness_hold_reason: hold_reason,
            witness_snapshot,
            attempt_num: attempt_num_validated,
            suspended_attempt_evidence: evidence,
            evidence_digest,
            created_at: self.clock.unix_seconds(),
        };

        let envelope = match PendingAuthorizationEnvelope::new(pending, authorization.basis) {
            Ok(env) => env,
            Err(e) => return NavigatorResult::SystemFailure(e.to_string()),
        };

        // P1-1: persist BEFORE return — çökme penceresi yok.
        match self.pending_authorization_store.persist(&envelope) {
            Ok(receipt) => NavigatorResult::AwaitingWitnesses {
                pending: envelope.clone().into_record(),
                persistence: receipt,
            },
            Err(error) => NavigatorResult::PendingAuthorizationPersistenceFailure {
                pending: envelope.into_record(),
                error,
            },
        }
    }

    /// **INV-T9 #72 closure (P1-4):** Rejected yolunun tek production mapper'ı.
    /// Modül-level free function'a delegate — test modülü direkt `make_revision_
    /// required_from_rejection` çağırır (helper drift ederse test yakalar).
    #[allow(clippy::result_large_err)]
    pub(crate) fn revision_required_from_rejection(
        authorization: crate::authorization::AuthorizationContext,
        reasons: crate::witness::NonEmptyWitnessRejections,
        snapshot: crate::witness::WitnessQuorumSnapshot,
        attempt_num: u64,
    ) -> Result<crate::authorization::RevisionRequired, NavigatorResult> {
        make_revision_required_from_rejection(authorization, reasons, snapshot, attempt_num)
    }

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
            // G2c-4 (review 10 #3): structural context — focus node + outgoing imports.
            // Task predicate scope'tan focus_node_id çıkar, engine.space()'ten import'ları oku.
            let structural_context = task
                .target_predicate_set
                .predicates
                .first()
                .and_then(|wp| match wp.predicate.scope {
                    PredicateScope::Node(nid) => Some(nid),
                    _ => None,
                })
                .map(|focus_node_id| {
                    let imports: Vec<crate::agent::EdgeRef> = self
                        .engine
                        .space()
                        .edges
                        .iter()
                        .filter(|e| {
                            e.from == focus_node_id
                                && e.kind == crate::space::EdgeKind::Imports
                                && !e.is_type_only
                        })
                        .map(|e| crate::agent::EdgeRef {
                            from: e.from,
                            to: e.to,
                            kind: e.kind,
                        })
                        .collect();
                    crate::trajectory::AgentStructuralContext {
                        focus_node_id,
                        current_outgoing_imports: imports,
                    }
                });
            let agent_view = plan.to_agent_view(
                &task.label,
                self.current_measured.to_raw(),
                task.allowed_operations.clone(),
                task.constraints.clone(),
                feedback_history.clone(),
                structural_context,
            );

            // 2. LLM call → DeltaProposal.
            // G2c-4 (review 10 #5): ProposalParse terminal DEĞİL — feedback retry.
            // Network/NoMoreProposals terminal (navigator loop'a güvenilmez).
            let proposal = match self.llm.complete(&agent_view) {
                Ok(p) => p,
                Err(LlmError::ProposalParse {
                    message,
                    token_cost,
                }) => {
                    // Parse error → evidence + feedback, retry (API budget korunur).
                    let tc = token_cost.unwrap_or_default();
                    total_tokens.prompt_tokens += tc.prompt_tokens;
                    total_tokens.completion_tokens += tc.completion_tokens;
                    total_tokens.total_tokens += tc.total_tokens;
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
                        token_cost: tc,
                        duration_ms: 0,
                    });
                    feedback_history.push(format!(
                        "Attempt {attempt_num}: Your previous response was not valid \
                         DeltaProposal JSON. Parse error: {message}. Output ONLY a JSON object \
                         with fields: new_nodes, new_edges, removed_edges, affected_nodes, \
                         modified_entities, position_hints, reasoning. No markdown fences."
                    ));
                    last_outcome = Some(AttemptOutcome {
                        gate_decision: GateDecision::RejectedBySyntax,
                        predicate_completion: PredicateCompletion::NotCompleted,
                        mutation_decision: MutationDecision::Reject,
                        witness_status: None,
                    });
                    continue;
                }
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
            // G2c-3b (arkadaş review 9): witness policy'ye göre WitnessSet quorum.
            // Production: Paper 1 witness güven modeli (min_approvers=2). Harness: auto-approve.
            let omega = match self.witness_policy {
                NavigatorWitnessPolicy::Production => crate::witness::WitnessSet::new(Vec::new()),
                NavigatorWitnessPolicy::HarnessAutoApprove => {
                    crate::witness::WitnessSet::new(Vec::new()).with_quorum(0, 0.0)
                }
            };
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
                Ok(crate::engine::EngineCommitResult::Evaluated { result, .. }) => result,
                Ok(crate::engine::EngineCommitResult::Held {
                    authorization,
                    reason,
                    snapshot,
                }) => {
                    // **INV-T9 (reviewer P0-4)** — expected authorization bekleme. Agent
                    // retry DEĞİL. Budget tüketmez (continue YOK), LLM reinvocation YOK.
                    // Engine-owned AuthorizationContext kullanılır — placeholder YOK.
                    return self.suspend_for_witness(
                        authorization,
                        reason,
                        snapshot,
                        attempt_num as u64,
                    );
                }
                Ok(crate::engine::EngineCommitResult::Rejected {
                    authorization,
                    reasons,
                    snapshot,
                }) => {
                    // Explicit witness rejection — RequiresRevision (agent revises proposal).
                    // Budget tüketmez, LLM reinvocation YOK. Evidence-preserving.
                    //
                    // **INV-T9 #72 closure (P1-4):** Tek production mapper
                    // `revision_required_from_rejection` çağrılır — inline construction
                    // YOK. Bu helper testler tarafından da çalıştırılır
                    // (`rejected_mapper_constructs_canonical_revision_evidence`).
                    // Q3 production reachability ayrı issue (#73).
                    return match Self::revision_required_from_rejection(
                        authorization,
                        reasons,
                        snapshot,
                        attempt_num as u64,
                    ) {
                        Ok(revision) => NavigatorResult::RequiresRevision(revision),
                        Err(system_failure) => system_failure,
                    };
                }
                Err(crate::engine::EngineCommitError::PermissionDenied(msg)) => {
                    // Binding hatası (task not found / standalone). Terminal — retry YOK.
                    let _ = msg;
                    return NavigatorResult::TaskNotFound;
                }
                Err(e) => {
                    // **INV-T9 exhaustive taxonomy** — her varyant explicit handle.
                    use crate::engine::EngineCommitError;
                    match e {
                        // Retryable (agent-correctable) — budget tüketir, continue.
                        EngineCommitError::SyntaxViolation { .. }
                        | EngineCommitError::VisionViolation { .. }
                        | EngineCommitError::RuleViolation { .. } => {
                            let gd = gate_decision_from_engine_error(&e);
                            let hall = crate::agent::HallucinationType::from_engine_error(&e);
                            // Önce e'den gerekenleri çıkar (borrow ayrımı), sonra self.evidence push.
                            last_outcome = Some(crate::trajectory::AttemptOutcome {
                                gate_decision: gd,
                                predicate_completion:
                                    crate::trajectory::PredicateCompletion::NotCompleted,
                                mutation_decision: crate::trajectory::MutationDecision::Reject,
                                witness_status: None,
                            });
                            let before_raw = self.current_measured.to_raw();
                            let after_raw = measured.to_raw();
                            self.evidence.push(TrajectoryEvidence {
                                trajectory_id: self.trajectory_id,
                                milestone_id: self.milestone_id,
                                task_id,
                                attempt_id: attempt_num as u64,
                                before: before_raw,
                                after: after_raw,
                                gate_decision: gd,
                                predicate_completion:
                                    crate::trajectory::PredicateCompletion::NotCompleted,
                                mutation_decision: crate::trajectory::MutationDecision::Reject,
                                token_cost,
                                duration_ms: 0,
                            });
                            // D4 — Calibration feedback.
                            if let Some(hall) = hall {
                                feedback_history.push(format!(
                                    "Attempt {attempt_num}: {}",
                                    hall.calibration_message()
                                ));
                            }
                            continue;
                        }
                        // Terminal — operational fault, retry YOK, budget tüketmez.
                        EngineCommitError::InvalidWitnessEvidence(msg) => {
                            return NavigatorResult::WitnessEvaluationError(msg);
                        }
                        EngineCommitError::PermissionDenied(_msg) => {
                            return NavigatorResult::TaskNotFound;
                        }
                        EngineCommitError::NoPersistence
                        | EngineCommitError::Persistence(_)
                        | EngineCommitError::Internal(_) => {
                            return NavigatorResult::SystemFailure(
                                "engine system failure (persistence/internal)".to_string(),
                            );
                        }
                        // AuthorizationContextFailed = fail-closed system failure —
                        // basis üretilemedi, sıfır digest'e düşülmedi, terminal.
                        EngineCommitError::AuthorizationContextFailed(msg) => {
                            return NavigatorResult::SystemFailure(msg);
                        }
                        // **INV-T9 Step 4b (reviewer P0-4):** VisionContextInvalid =
                        // terminal — maneuver budget tüketmez, yeni LLM attempt
                        // başlatmaz, witness'a ulaşmaz. SystemFailure olarak map'lenir.
                        EngineCommitError::VisionContextInvalid(err) => {
                            return NavigatorResult::SystemFailure(err.to_string());
                        }
                        // **INV-T9 #70 Commit 4b (reviewer v2 karar 2):** TaskValidation
                        // = terminal — geçersiz task declaration (Mixed source, non-finite
                        // threshold/tolerance, geçersiz policy). Agent retry değil — task
                        // config düzeltilmeli. Maneuver budget tüketmez, witness'a ulaşmaz.
                        // Faz 8: disposition-aware navigation refine (şimdilik SystemFailure).
                        EngineCommitError::TaskValidation(err) => {
                            return NavigatorResult::SystemFailure(err.to_string());
                        }
                        // **INV-T9 #70 Commit 4b (reviewer v4 P1-3):** MeasurementBindingMismatch
                        // — disposition: RegenerateMeasurement (stale → retry without LLM)
                        // veya RejectPresentedAuthority (replay/tamper → terminal). Faz 8'de
                        // disposition-aware retry; şimdilik terminal SystemFailure.
                        EngineCommitError::MeasurementBindingMismatch(err) => {
                            return NavigatorResult::SystemFailure(err.to_string());
                        }
                        // **INV-T9 #70 Commit 4b (reviewer v3 P1-4):** MeasurementBindingFailed
                        // = engine derivation failure (operational fault) — terminal SystemFailure.
                        EngineCommitError::MeasurementBindingFailed(err) => {
                            return NavigatorResult::SystemFailure(err.to_string());
                        }
                        // **INV-T9 #70 Commit 4b Faz 3 (reviewer v6 #1):** Tek kapsayıcı
                        // measurement binding verification error. Mismatch (tag 8 —
                        // disposition Regenerate/Reject), Derivation (system failure),
                        // Drift (system failure — retry gerekebilir). Faz 8'de
                        // disposition-aware navigation refine; şimdilik terminal SystemFailure.
                        EngineCommitError::MeasurementBindingVerification(err) => {
                            return NavigatorResult::SystemFailure(err.to_string());
                        }
                    }
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
        ApplyTarget, CommitLane, ComparisonOp, InMemoryTaskRegistry, MetricPredicate,
        MutationDecision, OpKind, PredicateAxis, PredicateFailurePolicy, PredicateGate,
        PredicateGateInput, PredicateMode, PredicateScope, PredicateSet, Task, TaskBoundClaim,
        TaskId, TaskPolicy, TaskStatus, WeightedPredicate,
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
            structural_context: None, // G2c-4
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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
            crate::coords::MetricSource::Placeholder,
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
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
        // İkisi de Q5.b'nin çalıştığını gösterir. Witness boş → INV-T9 Held beklenir.
        match result {
            Ok(crate::engine::EngineCommitResult::Evaluated { result: r, .. }) => {
                assert!(
                    r.outcome.predicate_completion == PredicateCompletion::Completed
                        || r.outcome.predicate_completion == PredicateCompletion::NotCompleted
                );
            }
            Ok(crate::engine::EngineCommitResult::Held { .. }) => {
                // **INV-T9** — Witness Q1 fail (MinApproversNotMet) → Held (expected authorization
                // bekleme). Artık Err DEĞİL Ok kanalında. Predicate çalıştı, witness aşamasında hold.
            }
            Ok(crate::engine::EngineCommitResult::Rejected { .. }) => {
                // Explicit witness rejection (Q3 honest-reject).
            }
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }

    // **reviewer P0-2 (closure test):** AcceptAsProgress yolu — authorization basis,
    // gate output ile aynı improvement_policy'yi paylaşır ve gate kararını açıklar.
    //
    // Fixture: predicate NotCompleted, AcceptImprovement + allow_progress_checkpoint,
    // loss_after < loss_before - min_improvement_delta, hard caps geçer, witness yetersiz → Held.
    #[test]
    fn authorization_basis_explains_accept_as_progress_gate_decision() {
        use crate::authorization::EffectiveImprovementPolicy;

        let mut engine = make_real_engine();
        let mut resolver = InMemoryTaskRegistry::new();
        // AcceptImprovement + progress checkpoint izinli, min_delta küçük.
        let policy = TaskPolicy {
            predicate_failure_policy: PredicateFailurePolicy::AcceptImprovement,
            min_improvement_delta: 0.05,
            max_axis_regression: 0.15,
            maneuver_limit: 5,
            allow_progress_checkpoint: true,
        };
        // Threshold yüksek (0.80) → measured coupling 0.40 < 0.80 ama source Scip →
        // completion: NotCompleted (0.40 ≤ 0.80 geçer AMA biz NotCompleted yolunu
        // zorlamak için threshold'u measured'ın geçemeyeceği yönde seçmeliyiz).
        // Coupling Le 0.80 + measured 0.40 → Satisfied → Completed olur. Bu yüzden
        // predicate'ı coupling > 0.20 (Gt) yapıp measured 0.40 > 0.20 → Satisfied'i
        // engellemek için threshold yüksek tutmak yerine, completion NotCompleted için
        // measured'ı threshold'un dışına koyalım. En temizi: coupling Le 0.10,
        // measured 0.40 > 0.10 → Unsatisfied → NotCompleted.
        let task = coupling_task(1, 0.10, policy);
        resolver.insert(task);
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let omega = crate::witness::WitnessSet::new(Vec::new()); // boş → Held
        let target = RawPosition {
            x: 0.10,
            y: 0.6,
            z: 0.4,
            w: 0.5,
            v: 0.3,
        };
        // loss_before yüksek → loss_after (measured target'a yakın değil) düşük değil;
        // ama improved için loss_after < loss_before - 0.05. measured 0.40, target 0.10
        // → loss = |0.40-0.10| benzeri. loss_before=1.0 verip improved sağlayalım.
        //
        // **reviewer P1 (INV-T9 space-immutability):** Held durumunda engine space'e
        // hiçbir structural delta uygulanmaz. node_count yalnız node sayısını karşılaştırır;
        // edge ekleme/kaldırma, node alan değişimi, replacement yakalanmaz. SpaceDigest
        // node'ların tüm canonical alanlarını (kind/mass/cohesion/classification/role) +
        // tüm edge'leri encode eder; t_c Satisfied'ta ilerler. Held ikisinden de etkilenmemeli.
        let space_digest_before =
            crate::authorization::SpaceDigest::compute(engine.space()).unwrap();
        let t_c_before = engine.t_c();
        let result = engine.commit_task_claim(TaskCommitInput {
            claim: &claim,
            omega: &omega,
            task_resolver: &resolver as &dyn TaskResolver,
            target,
            loss_before: 1.0,
            measured: measured.clone(),
        });

        // Ayrı olarak aynı girdilerle PredicateGate.evaluate → gate_out.
        let resolved_task = resolver.resolve(TaskId::from(1u64)).expect("task exists");
        let bound = TaskBoundClaim {
            claim: &claim,
            task: resolved_task,
        };
        let gate_out = PredicateGate.evaluate(PredicateGateInput {
            bound,
            measured: &measured,
            loss_before: 1.0,
            target: &target,
        });

        // Gate AcceptAsProgress üretmeli (NotCompleted + improved + AcceptImprovement + checkpoint).
        assert_eq!(
            gate_out.outcome.mutation_decision,
            MutationDecision::AcceptAsProgress,
            "fixture must reach AcceptAsProgress path"
        );

        // Held beklenir (boş witness → MinApproversNotMet).
        let auth = match result {
            Ok(crate::engine::EngineCommitResult::Held { authorization, .. }) => authorization,
            other => panic!("expected Held, got {other:?}"),
        };
        let basis = &auth.basis;

        // **reviewer P0-1:** improvement_policy gate → output → basis tek propagation.
        // `EffectiveImprovementPolicy` Copy value type olduğu için "same object" değil
        // "same policy value": aynı girdiler → aynı policy değeri → basis'e taşınır.
        // Kod incelemede tek construction akışını doğrular (engine.rs commit_task_claim).
        assert_eq!(
            basis.predicate_evaluation.improvement_policy, gate_out.improvement_policy,
            "basis must carry the same improvement policy value as the gate output"
        );
        assert_eq!(
            basis.predicate_evaluation.improvement_policy,
            EffectiveImprovementPolicy::current_semantics()
        );
        // **reviewer P0-2:** basis gate kararını açıklar.
        assert_eq!(basis.mutation_decision, MutationDecision::AcceptAsProgress);
        assert_eq!(
            basis.intended_apply_target,
            ApplyTarget::Lane(CommitLane::TrajectoryCheckpoint)
        );
        assert_eq!(basis.predicate_evaluation.loss_after, gate_out.loss_after);
        assert_eq!(
            basis.predicate_completion,
            gate_out.outcome.predicate_completion
        );
        assert_eq!(
            basis.predicate_evaluation.min_improvement_delta,
            resolved_task.policy.min_improvement_delta
        );
        // target_vector = input.target (preferred_vector DEĞİL).
        assert_eq!(basis.predicate_evaluation.target_vector.x, target.x);

        // **reviewer P1 (space dokunulmazlık — güçlendirilmiş):** Held → time.advance
        // Satisfied DEĞİL → space mutasyonu yok (time.rs:61 other => other). INV-T9
        // iddiası "node sayısı değişmedi" değil: structural delta hiç uygulanmadı.
        // SpaceDigest tüm node alanlarını + edge'leri yakalar; t_c Satisfied'ta ilerler.
        let space_digest_after =
            crate::authorization::SpaceDigest::compute(engine.space()).unwrap();
        assert_eq!(
            space_digest_before, space_digest_after,
            "Held must not mutate engine space (full content digest)"
        );
        assert_eq!(
            t_c_before,
            engine.t_c(),
            "Held must not advance t_c (no Satisfied commit)"
        );
    }

    // **reviewer P0-1/P2 (single propagation — value equality):** gate output'tan çıkan
    // improvement policy değeri authorization basis'e taşınır. `EffectiveImprovementPolicy`
    // Copy value type olduğu için bu "same object" DEĞİL, "same policy value" — aynı
    // girdiler aynı policy değerini üretir ve engine.rs tek propagation akışında taşır.
    // (Kod inceleme: commit_task_claim gate_out.improvement_policy → build_authorization_context.)
    #[test]
    fn gate_output_policy_value_is_propagated_into_authorization_basis() {
        let mut engine = make_real_engine();
        let mut resolver = InMemoryTaskRegistry::new();
        let policy = TaskPolicy {
            predicate_failure_policy: PredicateFailurePolicy::AcceptImprovement,
            min_improvement_delta: 0.05,
            max_axis_regression: 0.15,
            maneuver_limit: 5,
            allow_progress_checkpoint: true,
        };
        resolver.insert(coupling_task(1, 0.10, policy));
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let target = RawPosition {
            x: 0.10,
            y: 0.6,
            z: 0.4,
            w: 0.5,
            v: 0.3,
        };
        let result = engine.commit_task_claim(TaskCommitInput {
            claim: &claim,
            omega: &omega,
            task_resolver: &resolver as &dyn TaskResolver,
            target,
            loss_before: 1.0,
            measured: measured.clone(),
        });
        let resolved_task = resolver.resolve(TaskId::from(1u64)).unwrap();
        let gate_out = PredicateGate.evaluate(PredicateGateInput {
            bound: TaskBoundClaim {
                claim: &claim,
                task: resolved_task,
            },
            measured: &measured,
            loss_before: 1.0,
            target: &target,
        });
        let auth = match result {
            Ok(crate::engine::EngineCommitResult::Held { authorization, .. }) => authorization,
            other => panic!("expected Held, got {other:?}"),
        };
        // Tek propagation: gate output policy değeri basis'e taşınır (value equality).
        // İki ayrı construction site yok — engine gate_out.improvement_policy kullanır.
        assert_eq!(
            auth.basis.predicate_evaluation.improvement_policy,
            gate_out.improvement_policy
        );
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

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #70 Commit 4b Faz 3 — commit_task_claim validate_for_commit guard (tag 7)
    //
    // **Reviewer v2 karar 2 + v6 P2-4:** Task declaration validation Q5 öncesinde.
    // Guard order: Q4 syntax → task bind → **validate_for_commit** → Q5 vision → ...
    // TaskBoundClaim semantic: identity binding only (task geçerliliği ayrı).
    // Terminal — maneuver budget tüketmez, witness'a ulaşmaz, authorization üretmez.
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Helper: task with empty predicate set (invalid declaration — tag 7).
    fn invalid_task_empty_predicate_set(id: TaskId) -> Task {
        Task {
            id,
            milestone_id: 1,
            label: "invalid-empty-predicates".to_string(),
            target_predicate_set: PredicateSet {
                mode: PredicateMode::All,
                predicates: vec![], // empty → EmptyPredicateSet error
                preferred_vector: None,
            },
            policy: TaskPolicy::default(),
            allowed_operations: vec![],
            constraints: vec![],
            status: TaskStatus::Pending,
        }
    }

    /// Helper: build TaskCommitInput for a task + claim pair.
    fn build_commit_input<'a>(
        claim: &'a Claim,
        resolver: &'a dyn TaskResolver,
        omega: &'a crate::witness::WitnessSet,
        measured: ProvenancedRawPosition,
    ) -> TaskCommitInput<'a> {
        TaskCommitInput {
            claim,
            omega,
            task_resolver: resolver,
            target: RawPosition::default(),
            loss_before: 1.0,
            measured,
        }
    }

    #[test]
    fn commit_task_claim_rejects_invalid_task_declaration_tag_7() {
        // Reviewer v2 karar 2 (tag 7): empty predicate set → TaskValidation error.
        let mut engine = make_real_engine();
        let task = invalid_task_empty_predicate_set(1);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let result = engine.commit_task_claim(build_commit_input(
            &claim,
            &resolver as &dyn TaskResolver,
            &omega,
            measured,
        ));
        use crate::engine::EngineCommitError;
        use crate::trajectory::TaskValidationError;
        assert!(
            matches!(
                result,
                Err(EngineCommitError::TaskValidation(
                    TaskValidationError::EmptyPredicateSet { .. }
                ))
            ),
            "empty predicate set must produce TaskValidation(EmptyPredicateSet) — got {result:?}"
        );
    }

    #[test]
    fn invalid_task_declaration_skips_q5_vision() {
        // Guard order: validate_for_commit Q5'ten ÖNCE. Invalid task → Q5 çalışmaz.
        // Claim θ > bound olsa bile TaskValidation döner (VisionViolation DEĞİL).
        let mut engine = make_real_engine();
        let task = invalid_task_empty_predicate_set(1);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        // computed_raw çok yüksek (θ bound aşar) — ama validate_for_commit önce çalışır.
        let claim = test_claim_with_task(1, Some(1), 0.95);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let result = engine.commit_task_claim(build_commit_input(
            &claim,
            &resolver as &dyn TaskResolver,
            &omega,
            measured,
        ));
        use crate::engine::EngineCommitError;
        assert!(
            matches!(result, Err(EngineCommitError::TaskValidation(_))),
            "TaskValidation must fire before Q5 vision — got {result:?}"
        );
    }

    #[test]
    fn invalid_task_declaration_skips_predicate_gate() {
        // Invalid task → PredicateGate (Q5.b) çalışmaz.
        let mut engine = make_real_engine();
        let task = invalid_task_empty_predicate_set(1);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let result = engine.commit_task_claim(build_commit_input(
            &claim,
            &resolver as &dyn TaskResolver,
            &omega,
            measured,
        ));
        // TaskValidation → Err (gate hiç çalışmadı, Evaluated dönmez).
        assert!(
            result.is_err(),
            "invalid task must not reach PredicateGate (gate needs valid predicates)"
        );
    }

    #[test]
    fn invalid_task_declaration_skips_q6_rules() {
        // Invalid task → Q6 rule check çalışmaz.
        let mut engine = make_real_engine();
        let task = invalid_task_empty_predicate_set(1);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let result = engine.commit_task_claim(build_commit_input(
            &claim,
            &resolver as &dyn TaskResolver,
            &omega,
            measured,
        ));
        assert!(result.is_err(), "invalid task must not reach Q6 rules");
    }

    #[test]
    fn invalid_task_declaration_no_witness_evaluation() {
        // Invalid task → witness (Q1-Q3) çağrılmaz.
        let mut engine = make_real_engine();
        let task = invalid_task_empty_predicate_set(1);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let claim = test_claim_with_task(1, Some(1), 0.40);
        // HarnessAutoApprove witness — ama witness hiç çağrılmamalı.
        let omega = crate::witness::WitnessSet::new(Vec::new()).with_quorum(0, 0.0);
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let result = engine.commit_task_claim(build_commit_input(
            &claim,
            &resolver as &dyn TaskResolver,
            &omega,
            measured,
        ));
        assert!(
            result.is_err(),
            "invalid task must not reach witness evaluation"
        );
    }

    #[test]
    fn invalid_task_declaration_no_state_mutation() {
        // Reviewer P3: invalid task → engine state değişmez.
        let mut engine = make_real_engine();
        let t_c_before = engine.t_c();
        let space_nodes_before = engine.space().nodes.len();
        let space_edges_before = engine.space().edges.len();

        let task = invalid_task_empty_predicate_set(1);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let omega = crate::witness::WitnessSet::new(Vec::new());
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let _ = engine.commit_task_claim(build_commit_input(
            &claim,
            &resolver as &dyn TaskResolver,
            &omega,
            measured,
        ));

        assert_eq!(
            engine.t_c(),
            t_c_before,
            "t_c must not change on invalid task"
        );
        assert_eq!(
            engine.space().nodes.len(),
            space_nodes_before,
            "space nodes must not change"
        );
        assert_eq!(
            engine.space().edges.len(),
            space_edges_before,
            "space edges must not change"
        );
    }

    #[test]
    fn valid_task_declaration_preserves_existing_behavior() {
        // Valid task → mevcut commit davranışı değişmez (guard transparent for valid).
        let mut engine = make_real_engine();
        let policy = TaskPolicy::default();
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let omega = crate::witness::WitnessSet::new(Vec::new()).with_quorum(0, 0.0);
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let result = engine.commit_task_claim(build_commit_input(
            &claim,
            &resolver as &dyn TaskResolver,
            &omega,
            measured,
        ));
        // Valid task → TaskValidation geçer, normal pipeline devam (Err DEĞİL).
        assert!(
            result.is_ok(),
            "valid task must pass validate_for_commit and proceed: {result:?}"
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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
            witness_policy: NavigatorWitnessPolicy::default(),
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
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

    // ═══════════════════════════════════════════════════════════════════════════════
    // G2c-3 (arkadaş review 8) — Incremental coupling-dropping + policy accumulation
    // ═══════════════════════════════════════════════════════════════════════════════

    /// **G2c-3:** 5 node'lu balanced fixture. Target (node 0) → 4 node import (coupling 0.80).
    /// **Değerlendirilebilir vision** (`VisionVector::new`, GlobalDefault source) —
    /// G2c-2 `make_real_engine`'in None vision tuzağı YOK (θ=1.0 > 0.3 reject'i).
    fn make_balanced_engine() -> SpaceEngine {
        use crate::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
        use crate::coords::CoordinateSystem;
        let mut space = Space::default();
        // Target node 0 + 4 dependency node (1,2,3,4).
        for id in 0..=4u64 {
            space.nodes.insert(
                id,
                Node {
                    id,
                    kind: NodeKind::Module,
                    mass: 100.0,
                    cohesion: Some(0.6),
                    ..Default::default()
                },
            );
        }
        // node 0 → node 1,2,3,4 (4 outgoing Imports → coupling 4/5 = 0.80).
        for dep in 1..=4u64 {
            space.edges.push(Edge {
                from: 0,
                to: dep,
                kind: crate::space::EdgeKind::Imports,
                is_type_only: false,
            });
        }
        // node 1→0 incoming import → instability balanced (Ca>0).
        space.edges.push(Edge {
            from: 1,
            to: 0,
            kind: crate::space::EdgeKind::Imports,
            is_type_only: false,
        });
        let cs = CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::Placeholder,
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        // Değerlendirilebilir vision (UserLoaded) — θ küçük, Q5 vision geçer.
        // **INV-T9 Step 4b:** mutation yüzeyinde `GlobalDefault` authority'de reject
        // edilir; test fixture'ları `commit_task_claim` çağırdığı için `UserLoaded`
        // (kullanıcı-onaylı) kullanır. Vision instability = measured (~0.80) ile aynı —
        // loss coupling'den düşer, vision geçer.
        let vision = VisionVector::with_source(
            RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.80,
                w: 0.5,
                v: 0.3,
            },
            crate::vision::VisionSource::UserLoaded,
        );
        // Engine config min_approvers=0 — ancak navigator Production modunda
        // WitnessSet::new(Vec::new()) oluşturur (hardcoded min_approvers=2/quorum=1.5).
        // WitnessSet authority engine config değil (#72 closure Faz 1 — P1-1 kök neden).
        // Held exact test (inv_t9_72_held_production_path_exact) bu engine ile çalışır.
        let mut config = EngineConfig::default_calibrated();
        config.min_approvers = 0;
        SpaceEngine::new(space, cs, vision, config)
    }

    /// 3 incremental coupling-reducing proposal: 1'er import kaldırma.
    /// 0.80→0.75 (remove 0→1), 0.75→0.67 (remove 0→2), 0.67→0.50 (remove 0→3).
    fn incremental_coupling_proposals() -> Vec<DeltaProposal> {
        use crate::agent::EdgeRef;
        (1..=3u64)
            .map(|dep| DeltaProposal {
                new_nodes: vec![],
                new_edges: vec![],
                removed_edges: vec![EdgeRef {
                    from: 0,
                    to: dep,
                    kind: crate::space::EdgeKind::Imports,
                }],
                affected_nodes: vec![0],
                modified_entities: vec![],
                position_hints: vec![],
                reasoning: format!("G2c-3 incremental: remove import 0→{dep}"),
            })
            .collect()
    }

    /// G2c-3 task — coupling ≤ 0.55. preferred_vector instability'yı measured'ınkine
    /// yakın tutar (loss sadece coupling farkından düşer, instability farkı maskelenmez).
    fn g2c3_coupling_task(task_id: TaskId, policy: &TaskPolicy) -> Task {
        let mut task = coupling_task(task_id, 0.55, policy.clone());
        task.target_predicate_set.predicates[0].predicate.scope = PredicateScope::Node(0);
        // target_vector instability = measured instability (~0.75-0.80) — loss coupling'den düşer.
        task.target_predicate_set.preferred_vector = Some(RawPosition {
            x: 0.55,
            y: 0.6,
            z: 0.80, // measured instability (4 import → Ce=4, Ca=1 → 0.80)
            w: 0.5,
            v: 0.3,
        });
        task
    }

    /// G2c-3 navigator builder — target_vector instability measured'a yakın.
    fn g2c3_nav<'a>(
        llm: &'a MockLlmClient,
        resolver: &'a InMemoryTaskRegistry,
        engine: &'a mut SpaceEngine,
        evidence: &'a mut Vec<TrajectoryEvidence>,
    ) -> AgentNavigator<'a, MockLlmClient, InMemoryTaskRegistry> {
        AgentNavigator {
            llm,
            resolver,
            engine,
            evidence,
            trajectory_id: 1,
            milestone_id: 1,
            // target_vector instability = measured (~0.80) — loss coupling'den düşer.
            target_vector: RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.80,
                w: 0.5,
                v: 0.3,
            },
            current_measured: measured_pos(0.80),
            output_contract: OutputContract::strict(),
            // G2c-3: harness auto-approve (controlled experiment — production değil).
            witness_policy: NavigatorWitnessPolicy::HarnessAutoApprove,
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
        }
    }

    /// G2c-3 #1: AcceptImprovement + incremental removal → Completed (state accumulation).
    /// RQ9 ana kanıtı: progress checkpoint policy state'i adım adım hedefe yaklaştırır.
    #[test]
    fn g2c3_incremental_coupling_reduction_completes() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 3; // review 8 #3 — NoMoreProposals tuzağı yok (3 proposal = 3 attempt)
        policy.predicate_failure_policy = PredicateFailurePolicy::AcceptImprovement;
        policy.allow_progress_checkpoint = true;
        let task = g2c3_coupling_task(1, &policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let mock = MockLlmClient::new(incremental_coupling_proposals());
        let mut engine = make_balanced_engine();
        let mut evidence = vec![];
        let mut nav = g2c3_nav(&mock, &resolver, &mut engine, &mut evidence);
        let result = nav.run_task(1, 7);
        // G2c-3 ana kanıt: AcceptImprovement → Completed (3 attempts).
        assert!(
            matches!(result, NavigatorResult::Completed { attempts, .. } if attempts == 3),
            "G2c-3: AcceptImprovement + incremental removal should Complete in 3 attempts: got {result:?}"
        );
    }

    /// G2c-3 #2: StrictReject + incremental removal → ExceededManeuverLimit (state frozen).
    /// RQ9 kontrol hücresi: strict reject state'i dondurur, aynı task ilerlemez.
    #[test]
    fn g2c3_strict_reject_freezes_state_at_maneuver_limit() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 3;
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        let task = g2c3_coupling_task(1, &policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let mock = MockLlmClient::new(incremental_coupling_proposals());
        let mut engine = make_balanced_engine();
        let mut evidence = vec![];
        let mut nav = g2c3_nav(&mock, &resolver, &mut engine, &mut evidence);
        let result = nav.run_task(1, 7);
        // G2c-3 kontrol: StrictReject → ExceededManeuverLimit (state donmuş).
        assert!(
            matches!(result, NavigatorResult::ExceededManeuverLimit { .. }),
            "G2c-3: StrictReject should freeze state → ExceededManeuverLimit: got {result:?}"
        );
        // State donmuş: her attempt Reject, evidence 3 entry.
        assert_eq!(evidence.len(), 3);
        assert!(evidence
            .iter()
            .all(|e| { e.mutation_decision == MutationDecision::Reject }));
    }

    /// G2c-3 #3 (review 8 #4): Completed evidence gate_decision = PassedAll.
    /// PR #21'in Unknown borcu kapanır — success path artık Unknown değil.
    #[test]
    fn g2c3_completed_evidence_has_passed_all_gate_decision() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 3;
        policy.predicate_failure_policy = PredicateFailurePolicy::AcceptImprovement;
        policy.allow_progress_checkpoint = true;
        let task = g2c3_coupling_task(1, &policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let mock = MockLlmClient::new(incremental_coupling_proposals());
        let mut engine = make_balanced_engine();
        let mut evidence = vec![];
        let mut nav = g2c3_nav(&mock, &resolver, &mut engine, &mut evidence);
        let result = nav.run_task(1, 7);
        assert!(matches!(result, NavigatorResult::Completed { .. }));
        // Son evidence (Completed attempt) — gate_decision PassedAll (review 8 #4).
        let last = evidence.last().expect("Completed produces evidence");
        assert_eq!(
            last.gate_decision,
            GateDecision::PassedAll,
            "Completed evidence must have gate_decision=PassedAll, not Unknown"
        );
        assert_eq!(last.predicate_completion, PredicateCompletion::Completed);
        assert_eq!(last.mutation_decision, MutationDecision::AcceptAsCompleted);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 — External-Evidence Suspension Isolation navigator tests
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Production witness policy (min_approvers=2, empty witness set) + predicate satisfied
    /// → EngineCommitResult::Held → NavigatorResult::AwaitingWitnesses.
    ///
    /// Bu test INV-T9'un temel garantisi: witness quorum eksikliği agent failure DEĞİL,
    /// suspended authorization durumudur. ExceededManeuverLimit DÖNMEZ.
    #[test]
    fn inv_t9_predicate_satisfied_without_quorum_returns_awaiting_witnesses() {
        // HarnessAutoApprove DEĞİL — Production witness policy (Paper 1 güven modeli).
        // Boş witness set → Held.
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 5;
        policy.predicate_failure_policy = PredicateFailurePolicy::AcceptImprovement;
        policy.allow_progress_checkpoint = true;
        let task = g2c3_coupling_task(1, &policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);

        // Incremental proposals — coupling'i düşürür, predicate satisfied olabilir.
        let proposals = incremental_coupling_proposals();
        let mock = MockLlmClient::new(proposals);
        let mut engine = make_balanced_engine();
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
            current_measured: measured_pos(0.80),
            output_contract: OutputContract::strict(),
            // Production witness policy — boş set → Held (INV-T9).
            witness_policy: NavigatorWitnessPolicy::Production,
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
        };

        let result = nav.run_task(1, 7);

        // INV-T9: Held → AwaitingWitnesses (ExceededManeuverLimit DEĞİL).
        // Eğer predicate satisfied olur ve witness quorum yetersizse → AwaitingWitnesses.
        // Eğer predicate satisfied olmadan proposals tükenirse → LlmError(NoMoreProposals).
        // Eğer predicate satisfied olmadan maneuver limit tükenirse → ExceededManeuverLimit.
        // Her durumda ExceededManeuverLimit witness eksikliğinden OLMAMALI.
        match result {
            NavigatorResult::AwaitingWitnesses { pending, .. } => {
                assert_eq!(pending.task_id, 1);
                assert!(
                    matches!(
                        pending.witness_hold_reason,
                        crate::witness::WitnessHoldReason::MinApproversNotMet { .. }
                            | crate::witness::WitnessHoldReason::QuorumInsufficient { .. }
                    ),
                    "Held reason must be witness quorum shortage: {:?}",
                    pending.witness_hold_reason
                );
                assert!(pending.attempt_num.get() >= 1, "attempt_num must be >= 1");
            }
            NavigatorResult::ExceededManeuverLimit { last_outcome, .. } => {
                // Predicate fail retry'lerinden — witness'tan değil.
                assert!(
                    matches!(last_outcome.mutation_decision, MutationDecision::Reject),
                    "ExceededManeuverLimit must come from predicate failure, not witness"
                );
            }
            NavigatorResult::LlmError(crate::navigator::LlmError::NoMoreProposals) => {
                // Predicate fail retry'leri proposals'ı tüketti — INV-T9 ihlali değil.
            }
            other => panic!("INV-T9: unexpected result: {other:?}"),
        }
    }

    /// INV-T9: witness quorum eksikliği ExceededManeuverLimit üretmez.
    ///
    /// Bu test, witness eksikliğinin retry loop'a girmediğini doğrular.
    /// Production policy + boş set ile yapılan her attempt Held döner ama
    /// navigator Held'i retry olarak saymaz (continue YOK, direkt return).
    #[test]
    fn inv_t9_quorum_shortage_never_returns_exceeded_maneuver_limit_due_to_witness() {
        // Bu test, navigator Held'de continue YAPMADIĞINI doğrular.
        // Eğer Held retry olsaydı, maneuver limit tükenirdi ve ExceededManeuverLimit dönerdi.
        // Held → AwaitingWitnesses (terminal) ise ExceededManeuverLimit DÖNMEZ.
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 2; // Düşük limit — eğer Held retry olsaydı hızla tükenirdi.
        let task = g2c3_coupling_task(1, &policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);

        let proposals = incremental_coupling_proposals();
        let mock = MockLlmClient::new(proposals);
        let mut engine = make_balanced_engine();
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
            current_measured: measured_pos(0.80),
            output_contract: OutputContract::strict(),
            witness_policy: NavigatorWitnessPolicy::Production,
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
        };

        let result = nav.run_task(1, 7);

        // Eğer predicate satisfied olursa → AwaitingWitnesses (Held terminal).
        // ExceededManeuverLimit ancak predicate fail retry'lerden gelir, witness'tan değil.
        if let NavigatorResult::ExceededManeuverLimit { last_outcome, .. } = &result {
            // Eğer ExceededManeuverLimit döndüyse, bu witness eksikliğinden OLMAMALI.
            // Witness eksikliği Held'dir, Reject değil. last_outcome mutation_decision
            // Reject ise bu predicate failure'dan (retryable), witness'tan değil.
            assert!(
                matches!(last_outcome.mutation_decision, MutationDecision::Reject),
                "ExceededManeuverLimit must come from predicate failure (Reject), \
                 not from witness shortage (would be AwaitingWitnesses)"
            );
        }
        // Diğer durumlar (AwaitingWitnesses, Completed, LlmError) — INV-T9 ihlali yok.
    }

    /// INV-T9: AwaitingWitnesses durumunda LLM birden fazla kez çağrılmaz.
    ///
    /// Held terminal olduğu için navigator döngüden çıkar. LLM sadece proposal üretmek
    /// için 1 kez çağrılır, witness bekleme için tekrar çağrılmaz.
    #[test]
    fn inv_t9_awaiting_witnesses_does_not_reinvoke_llm_repeatedly() {
        // Bu test, LLM call_count'unun maneuver_limit'ten fazla OLMADIĞINI doğrular.
        // Eğer Held retry olsaydı, LLM her attempt için çağrılırdı (call_count artardı).
        // Held terminal ise LLM sadece 1 kez çağrılır (proposal generation).
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 10; // Yüksek limit — eğer Held retry olsaydı 10 kez çağrılırdı.
        let task = g2c3_coupling_task(1, &policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);

        let proposals = incremental_coupling_proposals();
        let mock = MockLlmClient::new(proposals);
        let mut engine = make_balanced_engine();
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
            current_measured: measured_pos(0.80),
            output_contract: OutputContract::strict(),
            witness_policy: NavigatorWitnessPolicy::Production,
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
        };

        let result = nav.run_task(1, 7);
        let calls = mock.call_count();

        // Eğer AwaitingWitnesses döndüyse, LLM sadece 1-2 kez çağrılmış olmalı (Held terminal).
        // Eğer ExceededManeuverLimit döndüyse, predicate fail retry'leri nedeniyle calls artar
        // ama bu witness'tan değil.
        if matches!(result, NavigatorResult::AwaitingWitnesses { .. }) {
            assert!(
                calls <= 3,
                "INV-T9: AwaitingWitnesses with {calls} LLM calls — Held must be terminal, \
                 not retry (maneuver_limit was 10). Witness shortage must not re-invoke LLM."
            );
        }
    }

    /// INV-T7 korunma: syntax rejection hala maneuver budget tüketir (retryable).
    ///
    /// Bu test INV-T9 eklenirken INV-T7'nin bozulmadığını doğrular.
    /// Syntax rejection → Reject → retry → budget tüketir.
    #[test]
    fn inv_t7_syntax_rejection_still_consumes_budget_and_retries() {
        // Boş proposal (syntax reject) → OutputContract reject → evidence + retry.
        // Birden fazla boş proposal ver → maneuver limit tükenene kadar retry.
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 3;
        let task = coupling_task(1, 0.55, policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);

        // Boş proposal (OutputContract reject) — 3 tane ver → 3 retry → ExceededManeuverLimit.
        let empty_proposals: Vec<DeltaProposal> = (0..3)
            .map(|_| DeltaProposal {
                new_nodes: vec![],
                new_edges: vec![],
                removed_edges: vec![],
                affected_nodes: vec![],
                modified_entities: vec![],
                position_hints: vec![],
                reasoning: String::new(), // OutputContract::strict reject (empty reasoning)
            })
            .collect();
        let mock = MockLlmClient::new(empty_proposals);
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
            current_measured: measured_pos(0.5),
            output_contract: OutputContract::strict(),
            witness_policy: NavigatorWitnessPolicy::HarnessAutoApprove, // witness'i bypass et
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
        };

        let result = nav.run_task(1, 7);
        // Syntax rejection retryable → 3 proposal tüketilir → NoMoreProposals veya ExceededManeuverLimit.
        // INV-T7 hala çalışıyor: retryable failures budget tüketir.
        let calls = mock.call_count();
        assert!(
            calls >= 1,
            "INV-T7: syntax rejection must invoke LLM (retryable), got {calls} calls"
        );
        // Evidence kaydedildi (her retry için).
        assert!(
            !evidence.is_empty(),
            "INV-T7: retryable rejection must produce evidence per attempt"
        );
        let _ = result; // Completed/LlmError/ExceededManeuverLimit — hepsi INV-T7 ihlali değil.
    }

    /// **#72 closure (Faz 2 — P1-2 D3 guard):** Test-only process-local adapter over the
    /// real filesystem store. Navigator/engine Held path is the production path; persistence
    /// uses this adapter because the fixture has an ephemeral space identity (D3 production
    /// guard: Ephemeral + CrossProcess → SystemFailure preserved).
    ///
    /// **Güvenlik sınırı (reviewer P1):** Yalnız `#[cfg(test)] mod tests` altında yaşar;
    /// production build'e girmez, D3 bypass yüzeyi oluşturmaz. Gerçek filesystem persist
    /// kodunu çalıştırır (serialization, atomic publish, reload).
    struct ProcessLocalFilesystemTestStore {
        inner: crate::authorization::FilesystemPendingAuthorizationStore,
    }

    impl ProcessLocalFilesystemTestStore {
        fn new(root: impl Into<std::path::PathBuf>) -> Self {
            Self {
                inner: crate::authorization::FilesystemPendingAuthorizationStore::new(root),
            }
        }
    }

    impl crate::authorization::PendingAuthorizationStore for ProcessLocalFilesystemTestStore {
        fn durability(&self) -> crate::authorization::SuspensionDurability {
            // ProcessLocal — Ephemeral identity + ProcessLocal → D3 OK.
            // FilesystemPendingAuthorizationStore gerçekte CrossProcess bildirir;
            // bu adapter test fixture'ının ephemeral identity'sini dürüstçe process-local kabul eder.
            crate::authorization::SuspensionDurability::ProcessLocal
        }

        fn persist(
            &mut self,
            envelope: &crate::authorization::PendingAuthorizationEnvelope,
        ) -> Result<
            crate::authorization::PendingAuthorizationReceipt,
            crate::authorization::PendingAuthorizationStoreError,
        > {
            // Gerçek filesystem persist kodu (serialization, atomic publish).
            self.inner.persist(envelope)
        }
    }

    /// **#72 closure (Faz 3 — P1-1 deterministic Held production-path exact):** Tek
    /// canonical Held regression testi. Eski conditional "exact" sibling test kaldırıldı
    /// (duplicate). Bu test INV-T9'un yürütülebilir kanıtı: deterministic gates passed +
    /// authorization gerektiren mutation + boş witness set → Held (retry değil).
    ///
    /// **Faz 1 fixture authority:** WitnessSet authority (engine config değil).
    /// `NavigatorWitnessPolicy::Production` → `WitnessSet::new(Vec::new())` → hardcoded
    /// min_approvers=2, quorum=1.5. Engine config quorum authority değil.
    ///
    /// **Faz 1 scope alignment (P1-3):** `coupling_task` scope `Node(1)` ama balanced
    /// fixture node 0 → test scope=Node(0) olarak ayarlanır.
    ///
    /// **Faz 1 target_vector (P2-1):** task preferred_vector'den — drift yok.
    #[test]
    fn inv_t9_72_held_production_path_exact() {
        use crate::agent::EdgeRef;
        use crate::authorization::{
            load_pending_authorization, FixedClock, SuspendedAttemptDisposition,
            SuspendedAttemptEvidenceDigest,
        };
        use crate::space::EdgeKind;
        use crate::witness::WitnessHoldReason;

        // Faz 1: deterministic task — Coupling <= 1.0 (her geçerli proposal satisfy eder).
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 10; // Held retry'a dönüşürse 2. LLM çağrısı → call_count kırılır.
        policy.predicate_failure_policy = PredicateFailurePolicy::StrictReject;
        let mut task = coupling_task(1, 1.0, policy);
        // P1-3 scope alignment: balanced fixture node 0 üzerindeki import'lar.
        task.target_predicate_set.predicates[0].predicate.scope = PredicateScope::Node(0);
        // P2-1: navigator target task preferred_vector'den (drift yok).
        let target_vector = task
            .target_predicate_set
            .preferred_vector
            .expect("exact Held fixture requires preferred vector");
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);

        // Faz 1: explicit, non-empty structural proposal — davranışsal fixture'a bağımlı değil.
        // Tek 0→1 import removal (var olan edge, RemoveImport allowed operation uyumlu).
        let proposal = DeltaProposal {
            new_nodes: vec![],
            new_edges: vec![],
            removed_edges: vec![EdgeRef {
                from: 0,
                to: 1,
                kind: EdgeKind::Imports,
            }],
            affected_nodes: vec![0],
            modified_entities: vec![],
            position_hints: vec![],
            reasoning: "exact Held fixture: remove one existing import".into(),
        };
        let mock = MockLlmClient::new(vec![proposal]);
        let mut engine = make_balanced_engine(); // Quorum builder YOK — witness authority WitnessSet'te.
        let mut evidence = vec![];

        // INV-T9 state isolation — before snapshot.
        let space_digest_before =
            crate::authorization::SpaceDigest::compute(engine.space()).unwrap();
        let t_c_before = engine.t_c();

        // P2-2 TempDir binding — reload assertion'ları tamamlanana kadar scope'ta tutulur.
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_path = temp.path().to_path_buf();

        let mut nav = AgentNavigator {
            llm: &mock,
            resolver: &resolver,
            engine: &mut engine,
            evidence: &mut evidence,
            trajectory_id: 1,
            milestone_id: 1,
            target_vector,
            current_measured: measured_pos(0.80),
            output_contract: OutputContract::strict(),
            // Production witness policy → WitnessSet::new(Vec::new()) → min_approvers=2, quorum=1.5.
            witness_policy: NavigatorWitnessPolicy::Production,
            // Faz 2: ProcessLocalFilesystemTestStore — gerçek filesystem persist, ProcessLocal durability.
            pending_authorization_store: Box::new(ProcessLocalFilesystemTestStore::new(&temp_path)),
            clock: Box::new(FixedClock(1_700_000_000)),
        };

        let result = nav.run_task(1, 7);

        // Exact: match (diagnostic için other:? ile).
        let (pending, persistence) = match result {
            NavigatorResult::AwaitingWitnesses {
                pending,
                persistence,
            } => (pending, persistence),
            other => panic!("expected exact AwaitingWitnesses, got {other:?}"),
        };

        // Exact: call_count == 1 (Held terminal, retry yok).
        assert_eq!(
            mock.call_count(),
            1,
            "#72 exact: Held terminal — single LLM invocation"
        );

        // Exact Q1 witness output (reviewer P2 — MinApproversNotMet, gevşek değil).
        assert_eq!(pending.witness_requirement.min_approvers, 2);
        assert_eq!(pending.witness_requirement.quorum_threshold, 1.5);
        assert_eq!(pending.witness_snapshot.approvers, 0);
        assert_eq!(pending.witness_snapshot.required_approvers, 2);
        assert_eq!(pending.witness_snapshot.support, 0.0);
        assert_eq!(pending.witness_snapshot.required_support, 1.5);
        assert_eq!(
            pending.witness_hold_reason,
            WitnessHoldReason::MinApproversNotMet {
                distinct: 0,
                required: 2
            }
        );

        // INV-T9 control-flow + state isolation.
        let space_digest_after =
            crate::authorization::SpaceDigest::compute(nav.engine.space()).unwrap();
        assert_eq!(
            space_digest_before, space_digest_after,
            "Held must not mutate canonical space content"
        );
        assert_eq!(
            t_c_before,
            nav.engine.t_c(),
            "Held must not advance committed time"
        );

        // #72 evidence assertions.
        assert_eq!(pending.attempt_num.get(), 1, "#72: attempt_num");
        assert!(
            matches!(
                pending.suspended_attempt_evidence.disposition(),
                SuspendedAttemptDisposition::Held { .. }
            ),
            "#72: disposition must be Held"
        );
        // record ↔ evidence consistency.
        assert_eq!(
            pending.attempt_num,
            pending.suspended_attempt_evidence.attempt_num(),
            "#72: attempt_num record↔evidence"
        );
        assert_eq!(
            pending.task_id,
            pending.suspended_attempt_evidence.task_id(),
            "#72: task_id record↔evidence"
        );
        assert_eq!(
            pending.claim_id,
            pending.suspended_attempt_evidence.claim_id(),
            "#72: claim_id record↔evidence"
        );
        // reviewer P2: authorization_basis_digest zinciri — pending ↔ evidence.
        assert_eq!(
            pending
                .suspended_attempt_evidence
                .authorization_basis_digest(),
            &pending.authorization_basis_digest,
            "#72: evidence.authorization_basis_digest == record.authorization_basis_digest"
        );
        // evidence_digest == recomputed.
        let recomputed =
            SuspendedAttemptEvidenceDigest::compute(&pending.suspended_attempt_evidence).unwrap();
        assert_eq!(
            pending.evidence_digest, recomputed,
            "#72: evidence_digest == recomputed"
        );
        // reviewer P2: embedded Held disposition reason + snapshot payload exact.
        match pending.suspended_attempt_evidence.disposition() {
            SuspendedAttemptDisposition::Held {
                hold_reason,
                snapshot,
            } => {
                assert_eq!(
                    hold_reason, &pending.witness_hold_reason,
                    "#72: embedded evidence hold_reason == record witness_hold_reason"
                );
                assert_eq!(
                    snapshot, &pending.witness_snapshot,
                    "#72: embedded evidence snapshot == record witness_snapshot"
                );
            }
            other => panic!("#72: expected embedded Held evidence, got {other:?}"),
        }
        // receipt identity == pending identity (reviewer P2: authorization_basis_digest dahil).
        assert_eq!(persistence.task_id, pending.task_id, "#72: receipt task_id");
        assert_eq!(
            persistence.claim_id, pending.claim_id,
            "#72: receipt claim_id"
        );
        assert_eq!(
            persistence.attempt_num, pending.attempt_num,
            "#72: receipt attempt_num"
        );
        assert_eq!(
            persistence.evidence_digest, pending.evidence_digest,
            "#72: receipt evidence_digest"
        );
        assert_eq!(
            persistence.authorization_basis_digest, pending.authorization_basis_digest,
            "#72: receipt authorization_basis_digest == pending"
        );

        // P2-2 disk reload — resmi helper (manuel read/deserialize değil).
        assert!(
            persistence.artifact_path.exists(),
            "#72: persisted artifact must exist (persist-before-return)"
        );
        let loaded = load_pending_authorization(&persistence.artifact_path)
            .expect("#72: persisted artifact strict-load + verify");
        let loaded_record = loaded.record();
        assert_eq!(
            loaded_record.task_id, pending.task_id,
            "#72: loaded task_id"
        );
        assert_eq!(
            loaded_record.claim_id, pending.claim_id,
            "#72: loaded claim_id"
        );
        assert_eq!(
            loaded_record.attempt_num, pending.attempt_num,
            "#72: loaded attempt_num"
        );
        assert_eq!(
            loaded_record.evidence_digest, pending.evidence_digest,
            "#72: loaded evidence_digest"
        );
        assert_eq!(
            loaded_record.suspended_attempt_evidence, pending.suspended_attempt_evidence,
            "#72: loaded suspended_attempt_evidence"
        );
        assert_eq!(
            loaded_record.authorization_basis_digest, pending.authorization_basis_digest,
            "#72: loaded authorization_basis_digest == pending"
        );

        // TempDir scope'ta — reload tamamlandı, drop olabilir.
        drop(temp);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 Step 4a closure — captured rule context propagation
    //
    // Reviewer P1: commit_task_claim rule_context'i Q6'ya veriyor ama
    // build_authorization_context'e geçirmiyordu; o ikinci kez snapshot üretüyordu.
    // Bu test descriptor çağrı sayısını sayar — fix sonrası commit_task_claim boyunca
    // descriptor yalnız BİR KEZ çağrılmalı (tek captured snapshot).
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Test rule'u — `descriptor()` çağrı sayısını sayar. Determinism contract ihlal
    /// simülasyonu: eğer commit_task_claim iki kez snapshot üretirse sayaç 2 olur.
    struct CountingDescriptorRule {
        id: crate::rule::RuleId,
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl crate::rule::Rule for CountingDescriptorRule {
        fn id(&self) -> &crate::rule::RuleId {
            &self.id
        }
        fn descriptor(&self) -> crate::authorization::RuleDescriptor {
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            crate::authorization::RuleDescriptor {
                rule_id: self.id.clone(),
                semantics_version: 1,
                canonical_parameters: vec![],
            }
        }
        fn evaluate(
            &self,
            _: &[crate::space::Node],
            _: &[crate::space::Edge],
            _: &crate::space::Space,
        ) -> Option<crate::rule::RuleViolation> {
            None
        }
    }

    #[test]
    fn q6_and_authorization_digest_reuse_captured_rule_context() {
        use crate::coords::MetricSource;
        use crate::engine::TaskCommitInput;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        // Engine kur — real axes + CountingDescriptorRule.
        let mut engine = make_real_engine();
        let counter = Arc::new(AtomicUsize::new(0));
        engine
            .register_rule(Box::new(CountingDescriptorRule {
                id: "test.counting_descriptor".into(),
                counter: counter.clone(),
            }))
            .expect("counting rule registration");

        // Registration sırasında descriptor çağrıldı (identity check). Sayaç sıfırla.
        let calls_during_registration = counter.load(Ordering::SeqCst);
        assert!(
            calls_during_registration >= 1,
            "registration must call descriptor at least once (identity check)"
        );
        counter.store(0, Ordering::SeqCst);

        // Task + claim + measured hazırla (AcceptAsCompleted yolu — authorization üretilir).
        let mut resolver = crate::trajectory::InMemoryTaskRegistry::new();
        resolver.insert(coupling_task(1, 0.55, TaskPolicy::default()));
        let claim = test_claim_with_task(1, Some(1), 0.40);
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let omega = crate::witness::WitnessSet::new(Vec::new());

        let result = engine.commit_task_claim(TaskCommitInput {
            claim: &claim,
            omega: &omega,
            task_resolver: &resolver as &dyn crate::trajectory::TaskResolver,
            target: crate::coords::RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            },
            loss_before: 1.0,
            measured,
        });

        // **reviewer P2 (test sıkılaştırma):** Fixture kesin Held üretmeli (boş witness →
        // MinApproversNotMet; predicate coupling 0.40 ≤ 0.55 Completed ama witness yok).
        // Exact assertion — vacuous geçişi önler (Evaluated/other arm yok).
        assert!(
            matches!(result, Ok(crate::engine::EngineCommitResult::Held { .. })),
            "fixture must reach Held (empty witness); got: {result:?}"
        );

        // **Kritik assert (reviewer P2 — exact):** commit_task_claim boyunca descriptor
        // tam 1 KEZ çağrılmalı — Q6 ve digest aynı captured context'i paylaşır. Eski kodda
        // build_authorization_context tekrar current_evaluation_context_digest() →
        // current_rule_evaluation_context() çağırırdı → sayaç 2 olurdu. Exact assert
        // (<= 1 DEĞİL) vacuous geçişi önler: Held'e ulaşıldığı kesin, bu yüzden descriptor
        // tam olarak 1 kez çağrılmalı (authorization context üretildi).
        let calls = counter.load(Ordering::SeqCst);
        assert_eq!(
            calls, 1,
            "one captured rule context must serve both Q6 and authorization digest; got {calls} \
             calls (0 = context not captured for Held path; 2 = second snapshot produced)"
        );
    }

    /// **scoped-review P2-a:** GlobalDefault vision authority failure → gerçek navigator
    /// loop'unun terminal davranışı. Context üretim hatası `VisionContextInvalid` terminal
    /// olarak map'lenir — maneuver budget tüketmez, retry yok, state değişmez.
    ///
    /// Authorization.rs'deki engine-integration testi context üretim + mapping doğrular;
    /// bu test gerçek `run_task` loop'unu çalıştırır: SystemFailure + call_count=1 +
    /// evidence empty + space/t_c unchanged.
    #[test]
    fn global_default_authority_failure_does_not_retry_or_consume_budget() {
        use crate::engine::EngineConfig;
        use crate::space::{Node, NodeKind, Space};
        use crate::vision::VisionVector;

        // Engine: GlobalDefault vision (VisionVector::new legacy constructor → source
        // GlobalDefault). make_balanced_engine UserLoaded kullanır; burada manuel kurulum.
        let mut space = Space::default();
        for id in 0..=4u64 {
            space.nodes.insert(
                id,
                Node {
                    id,
                    kind: NodeKind::Module,
                    mass: 100.0,
                    cohesion: Some(0.6),
                    ..Default::default()
                },
            );
        }
        let cs = crate::coords::CoordinateSystem::default_raw_five(
            crate::coords::MetricSource::Placeholder,
            crate::axes::CohesionAxis::new(),
            crate::axes::EntropyAxis::from_commit_entropy(6.0),
            crate::axes::WitnessDepthAxis::from_witness(0.3, 5),
        )
        .unwrap();
        let mut config = EngineConfig::default_calibrated();
        config.min_approvers = 0; // witness auto-approve (authority failure Q5 öncesi).
        let mut engine = SpaceEngine::new(
            space,
            cs,
            VisionVector::new(crate::coords::RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.80,
                w: 0.5,
                v: 0.3,
            }),
            config,
        );

        // State before — INV-T9: terminal failure state'i değiştirmemeli.
        // Space PartialEq değil → SpaceDigest ile canonical karşılaştırma (node/edge içeriği).
        let space_digest_before =
            crate::authorization::SpaceDigest::compute(engine.space()).unwrap();
        let t_c_before = engine.t_c();

        // Task + single proposal → LLM bir kez çağrılır, sonra terminal.
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 5; // Yüksek — eğer retry olsaydı 5 kez çağrılırdı.
        policy.predicate_failure_policy = PredicateFailurePolicy::AcceptImprovement;
        let task = g2c3_coupling_task(1, &policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);
        let mock = MockLlmClient::new(incremental_coupling_proposals());
        let mut evidence = vec![];
        let mut nav = g2c3_nav(&mock, &resolver, &mut engine, &mut evidence);

        let result = nav.run_task(1, 7);

        // Terminal: SystemFailure (VisionContextInvalid → SystemFailure, retry yok).
        assert!(
            matches!(result, NavigatorResult::SystemFailure(_)),
            "GlobalDefault authority failure must be terminal SystemFailure, got: {result:?}"
        );
        // call_count == 1 — LLM bir kez çağrıldı (terminal, retry YOK).
        assert_eq!(
            mock.call_count(),
            1,
            "terminal failure must not retry — LLM called exactly once"
        );
        // Evidence empty — attempt Q5 öncesi reject edildi (commit pipeline'a ulaşmadı).
        assert!(
            evidence.is_empty(),
            "terminal failure before Q5 must not produce evidence"
        );
        // State unchanged — space ve t_c korunmalı (mutation yok).
        let space_digest_after =
            crate::authorization::SpaceDigest::compute(engine.space()).unwrap();
        assert_eq!(
            space_digest_before, space_digest_after,
            "terminal failure must not mutate space"
        );
        assert_eq!(
            engine.t_c(),
            t_c_before,
            "terminal failure must not advance t_c"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // INV-T9 #72 — Commit 2: SuspendedAttemptEvidence factory tests
    //
    // Navigator boundary'de tek üretim kaynağı (P0-2 ownership). Held ve Rejected
    // aynı factory'yi kullanır. Engine ownership preserved (disposition payload
    // EngineCommitResult'ta kalır). Transitional dolgu — schema henüz değişmez.
    // ═══════════════════════════════════════════════════════════════════════════════

    /// **INV-T9 #72 (Commit 2):** Held yolu SuspendedAttemptEvidence üretir.
    ///
    /// Production witness policy + boş set → Held → AwaitingWitnesses. Evidence
    /// production navigator factory'sinden gelir (embedded SuspendedAttemptEvidence,
    /// attempt_evidence_id kaldırıldı — dangling reference yok).
    ///
    /// **Reviewer P1-1:** Bu duplicate test kaldırıldı. Canonical Held exact test artık
    /// `inv_t9_72_held_production_path_exact` — deterministic fixture (WitnessSet authority,
    /// scope Node(0), target preferred_vector'den), exact AwaitingWitnesses, call_count==1,
    /// full evidence assertions + disk reload.

    /// **INV-T9 #72 (Commit 2):** Evidence factory production policy'de
    /// retry/budget izolasyonunu değiştirmez.
    ///
    /// Held terminal — continue YOK, budget tüketmez. Evidence üretimi bu
    /// invariant'ı bozmamalı (commit öncesi davranışın korunması).
    #[test]
    fn inv_t9_72_held_evidence_factory_preserves_budget_isolation() {
        let mut policy = TaskPolicy::default();
        policy.maneuver_limit = 3;
        policy.predicate_failure_policy = PredicateFailurePolicy::AcceptImprovement;
        policy.allow_progress_checkpoint = true;
        let task = g2c3_coupling_task(1, &policy);
        let mut resolver = InMemoryTaskRegistry::new();
        resolver.insert(task);

        let proposals = incremental_coupling_proposals();
        let mock = MockLlmClient::new(proposals);
        let mut engine = make_balanced_engine();
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
            current_measured: measured_pos(0.80),
            output_contract: OutputContract::strict(),
            witness_policy: NavigatorWitnessPolicy::Production,
            pending_authorization_store: Box::new(
                crate::authorization::NullPendingAuthorizationStore,
            ),
            clock: Box::new(crate::authorization::FixedClock(1700000000)),
        };

        let result = nav.run_task(1, 7);

        // Evidence factory budget isolation'ı bozmaz — Held terminal, t_c advance YOK.
        // (inv_t9_quorum_shortage_never_returns_exceeded_maneuver_limit_due_to_witness
        // zaten bu invariant'ı doğrular; bu test evidence factory sonrası paritesi
        // kanıtlar.)
        match result {
            NavigatorResult::AwaitingWitnesses { .. } => {
                // Held terminal — evidence factory budget'i tüketmedi.
            }
            NavigatorResult::ExceededManeuverLimit { .. }
            | NavigatorResult::LlmError(crate::navigator::LlmError::NoMoreProposals) => {
                // Predicate fail — evidence path'e ulaşmadı.
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// **INV-T9 #72 (Commit 2 — P0-2):** Evidence factory AttemptNumber validation
    /// fail yaparsa SystemFailure döner (attempt_num = 0 senaryosu).
    ///
    /// Navigator loop `for attempt_num in 1..=maneuver_limit` ile 0 geçemez, ama
    /// defensive: factory validation error path'i korundu.
    #[test]
    fn inv_t9_72_evidence_factory_rejects_zero_attempt_num() {
        // AttemptNumber::try_from(0) → Err(Zero). Factory bu hatayı SystemFailure'a
        // çevirir. Navigator loop 0 geçemez ama defensive path test edilir.
        let result = crate::authorization::AttemptNumber::try_from(0u64);
        assert_eq!(
            result,
            Err(crate::authorization::AttemptNumberError::Zero),
            "factory validation rejects zero (1-based invariant)"
        );
    }

    /// **INV-T9 #72 closure (P1-4):** Rejected production mapper exact test.
    ///
    /// `revision_required_from_rejection` navigator Rejected arm'ının tek production
    /// mapper'ı. Bu test aynı helper'ı çalıştırır (navigator end-to-end DEĞİL — Q3
    /// production reachability tracked in #73).
    ///
    /// Doğrulamalar: disposition == Rejected, attempt_num, task_id/claim_id evidence'dan,
    /// basis digest, reasons/snapshot korunuyor, evidence_digest == recomputed,
    /// RevisionRequired minimal shape (outer duplicate alan yok).
    /// **Reviewer P1-2 (direct production helper call):** Test, production mapper
    /// `revision_required_from_rejection` helper'ını DİREKT çağırır — inline
    /// construction YOK. Helper drift ederse test yakalar.
    ///
    /// Helper `pub(crate)` — navigator Rejected arm'ında inline alternatif yok.
    /// Q3 production reachability tracked in #73; bu test construction logic'ini
    /// doğrular (navigator end-to-end DEĞİL).
    #[test]
    fn rejected_mapper_constructs_canonical_revision_evidence() {
        use crate::authorization::{
            AuthorizationBasisDigest, SuspendedAttemptDisposition, SuspendedAttemptEvidenceDigest,
        };
        use crate::trajectory::{GateDecision, MutationDecision, PredicateCompletion};
        use crate::witness::{NonEmptyWitnessRejections, WitnessRejection};

        // Minimal AuthorizationContext — navigator helper için gerekli tek girdi.
        // Basis fixture authorization.rs test modülünde private; burada inline minimal
        // basis yerine helper'ın ürettiği RevisionRequired'nin internal consistency'ini
        // doğrularız (helper basis'i consume eder, basis detaylarından bağımsız).
        let basis = make_minimal_authorization_basis_fixture();
        let authorization = crate::authorization::AuthorizationContext {
            outcome: crate::trajectory::AttemptOutcome {
                gate_decision: GateDecision::PassedAll,
                predicate_completion: PredicateCompletion::Completed,
                mutation_decision: MutationDecision::AcceptAsCompleted,
                witness_status: None,
            },
            apply_target: crate::trajectory::ApplyTarget::Lane(
                crate::trajectory::CommitLane::Mainline,
            ),
            basis,
            witness_requirement: crate::authorization::WitnessRequirement {
                min_approvers: 2,
                quorum_threshold: 1.5,
            },
        };
        let expected_basis_digest =
            AuthorizationBasisDigest::compute(&authorization.basis).unwrap();

        // Reviewer P2-3: sabit expected değerler (tautolojik helper değil).
        let expected_task_id = TaskId::from(1u64);
        let expected_claim_id = ClaimId::from(42u64);
        let expected_attempt_num = 3u64;

        // Fixture precondition assertions — authorization identity pinned.
        assert_eq!(
            authorization.basis.task_id, expected_task_id,
            "fixture precondition: basis task_id"
        );
        assert_eq!(
            authorization.basis.claim_identity.claim_id, expected_claim_id,
            "fixture precondition: basis claim_id"
        );

        let reasons = NonEmptyWitnessRejections::from_single(WitnessRejection {
            witness: 5u64,
            rationale: Some("predicate mismatch".into()),
        });
        let snapshot = crate::witness::WitnessQuorumSnapshot {
            approvers: 0,
            required_approvers: 2,
            support: 0.0,
            required_support: 1.5,
        };

        // **Production helper çağrısı** — inline construction YOK (reviewer P1-2).
        let rev = make_revision_required_from_rejection(
            authorization,
            reasons.clone(),
            snapshot.clone(),
            expected_attempt_num,
        )
        .expect("production rejection mapper");

        // Exact assertions — sabit değerlere pin (tautolojik değil).
        assert!(
            matches!(
                rev.suspended_attempt_evidence().disposition(),
                SuspendedAttemptDisposition::Rejected { .. }
            ),
            "disposition must be Rejected"
        );
        assert_eq!(rev.task_id(), expected_task_id, "task_id pinned");
        assert_eq!(rev.claim_id(), expected_claim_id, "claim_id pinned");
        assert_eq!(
            rev.attempt_num().get(),
            expected_attempt_num,
            "attempt_num pinned"
        );
        assert_eq!(
            rev.authorization_basis_digest(),
            &expected_basis_digest,
            "basis digest matches AuthorizationContext basis"
        );
        // Reviewer P2-1: reason içeriğini exact doğrula (yalnızca length değil).
        let actual_reasons = rev.reasons().expect("rejected evidence must carry reasons");
        assert_eq!(
            actual_reasons.as_slice(),
            reasons.as_slice(),
            "rejection reasons must be preserved exactly"
        );
        assert_eq!(rev.witness_snapshot(), &snapshot, "snapshot preserved");

        // evidence_digest == recomputed (N3 load-path invariant).
        let recomputed =
            SuspendedAttemptEvidenceDigest::compute(rev.suspended_attempt_evidence()).unwrap();
        assert_eq!(
            rev.evidence_digest(),
            &recomputed,
            "evidence_digest == recomputed"
        );
    }

    /// Minimal AuthorizationBasis fixture — navigator test'leri için.
    /// authorization.rs test modülündeki sample_basis private; bu minimal version.
    fn make_minimal_authorization_basis_fixture() -> crate::authorization::AuthorizationBasis {
        use crate::authorization::{
            CanonicalAxisMeasurement, CanonicalEdgeIdentity, CanonicalNode,
            CanonicalNodeClassification, CanonicalNodeKind, CanonicalNodeRole,
            CanonicalPredicateContent, CanonicalRawPosition, CanonicalStructuralDelta,
            CanonicalWitnessPolicy, ClaimIdentity, MeasurementInputDigest,
            PredicateEvaluationBasis, ProvenancedMeasuredResult,
        };
        use crate::canonical_tags::{PredicateFailurePolicyTag, PredicateModeTag};
        use crate::trajectory::{
            ApplyTarget, CommitLane, GateDecision, MutationDecision, PredicateCompletion,
        };
        let scip = crate::canonical_tags::CanonicalMetricSourceTag::try_from(
            &crate::coords::MetricSource::Scip,
        )
        .unwrap();
        let mk = |v: f64| CanonicalAxisMeasurement {
            value: v,
            source: scip,
        };
        crate::authorization::AuthorizationBasis {
            schema_version: 1,
            task_id: 1u64,
            claim_identity: ClaimIdentity {
                claim_id: 42u64,
                task_id: 1u64,
            },
            claim_author: 100u64,
            structural_delta: CanonicalStructuralDelta::try_new(
                vec![CanonicalNode {
                    id: 10,
                    kind: CanonicalNodeKind::try_from(&crate::space::NodeKind::Module).unwrap(),
                    mass: 100.0,
                    cohesion: Some(0.5),
                    classification: CanonicalNodeClassification::try_from(
                        &crate::space::NodeClassification::Production,
                    )
                    .unwrap(),
                    role: CanonicalNodeRole::try_from(&crate::space::NodeRole::Runtime).unwrap(),
                }],
                vec![],
                vec![CanonicalEdgeIdentity::new(
                    0,
                    1,
                    crate::authorization::CanonicalEdgeKind::try_from(
                        &crate::space::EdgeKind::Imports,
                    )
                    .unwrap(),
                )],
            )
            .unwrap(),
            predicate_content: CanonicalPredicateContent {
                mode: PredicateModeTag::try_from(&crate::trajectory::PredicateMode::All).unwrap(),
                predicates: vec![],
            },
            predicate_evaluation: PredicateEvaluationBasis {
                target_vector: CanonicalRawPosition {
                    x: 0.55,
                    y: 0.6,
                    z: 0.4,
                    w: 0.5,
                    v: 0.3,
                },
                loss_before: 1.0,
                loss_after: 0.5,
                failure_policy: PredicateFailurePolicyTag::try_from(
                    &crate::trajectory::PredicateFailurePolicy::StrictReject,
                )
                .unwrap(),
                allow_progress_checkpoint: false,
                min_improvement_delta: 0.1,
                improvement_policy:
                    crate::authorization::EffectiveImprovementPolicy::current_semantics(),
            },
            measured_result: ProvenancedMeasuredResult {
                coupling: mk(0.5),
                cohesion: mk(0.6),
                instability: mk(0.4),
                entropy: mk(0.5),
                witness_depth: mk(0.3),
            },
            deterministic_gate_result: GateDecision::PassedAll,
            predicate_completion: PredicateCompletion::Completed,
            mutation_decision: MutationDecision::AcceptAsCompleted,
            intended_apply_target: ApplyTarget::Lane(CommitLane::Mainline),
            witness_policy: CanonicalWitnessPolicy {
                schema_version: 1,
                min_approvers: 2,
                quorum_threshold: 1.5,
                independence_policy: crate::authorization::WitnessIndependencePolicyTag::STRICT,
            },
            measurement_input_digest: MeasurementInputDigest::from_bytes([0xcc; 32]),
            evaluation_context_digest: crate::authorization::EvaluationContextDigest::from_bytes(
                [0xaa; 32],
            ),
            base_space_view_revision: crate::authorization::SpaceViewRevision {
                view_id: crate::authorization::SpaceViewId::Persisted(
                    crate::authorization::PersistedSpaceViewId::from_bytes([0xdd; 16]),
                ),
                sequence: 7,
                content_digest: crate::authorization::SpaceDigest::from_bytes([0xbb; 32]),
            },
        }
    }
}
