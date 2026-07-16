//! OSP MCP Server — rmcp `ServerHandler` implementation (`docs/mcp-design.md` §6).
//!
//! Aşama G1 (4 agent-facing tool):
//! - `osp_analyze_workspace` — Observation: repo snapshot (nodes, edges, coverage)
//! - `osp_get_agent_task_view` ⭐ — Observation: INV-T1 projection (NO coordinates)
//! - `osp_check_predicate` — Validation: current predicate status
//! - `osp_submit_delta` — Execution: agent DeltaProposal (single-attempt, Q5.b gate)
//!
//! Aşama G2 eklemeleri:
//! - `osp_trajectory_init` — Operator-only: Trajectory + VisionVector oluştur (INV-T2)
//! - `osp_task_add` — Operator-only: registry'ye Task ekle (INV-T2)
//! - `osp_run_task` ⭐ — Agent-facing: navigator loop (multi-attempt, LLM delta üretir)
//! - `osp_get_attempt_history` — Agent-facing: navigator evidence ledger (RQ6 verisi)
//!
//! ## INV-T1 leak protection
//! Her agent-facing tool çıktısı `McpEnvelope::assert_no_coordinate_leak()`'ten geçer.
//! `preferred_vector`/`target_region`/`milestone_target_vector` string geçerse envelope
//! `TargetCoordinateLeakBlocked` ile değiştirilir (panic-level MCP bug).
//! **Operator-only tool'lar bu kontrolden MUAF** (operator coordinate görür — envelope.rs kuralı).

use std::collections::HashMap;
use std::sync::Arc;

use osp_core::agent::DeltaProposal;
use osp_core::coords::MetricSource;
use osp_core::navigator::{build_claim_from_proposal, provenanced_from_raw, LlmClient};
use osp_core::trajectory::{
    InMemoryTaskRegistry, InternalTaskPlan, OperatorCapability, PredicateSetResult,
    ProvenancedRawPosition, Task, TaskId, TaskResolver, TrajectoryEvidence, TrajectoryId,
};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo, ToolsCapability};
use rmcp::{schemars, tool, tool_handler, tool_router, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::envelope::{EnvelopeError, ErrorCode, McpEnvelope};
use crate::mode::ServerMode;
use crate::workspace::{SharedWorkspace, Workspace};

// ═══════════════════════════════════════════════════════════════════════════════
// Tool input schemas (schemars JsonSchema — rmcp agent'a JSON Schema gösterir)
// ═══════════════════════════════════════════════════════════════════════════════

/// `osp_analyze_workspace` input. Agent path VEREMEZ — workspace startup'ta alınır.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnalyzeWorkspaceInput {
    /// workspace_id gelecekte (WorkspaceRegistry). Şimdilik yok — tek workspace.
    #[serde(default)]
    pub workspace_id: Option<String>,
}

/// `osp_get_agent_task_view` input. INV-T1 ⭐ merkez tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetAgentTaskViewInput {
    /// Task ID (registry'de lookup).
    pub task_id: u64,
}

/// `osp_check_predicate` input.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckPredicateInput {
    /// Task ID — predicate task'a bağlı (INV-T5).
    pub task_id: u64,
}

/// `osp_submit_delta` input. DeltaProposal JSON (structural only — NO positions, inv #4).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubmitDeltaInput {
    /// Task ID (single-attempt bu task için çalışır).
    pub task_id: u64,
    /// DeltaProposal JSON (new_nodes, new_edges, modified_entities, reasoning).
    /// Pozisyon YOK — engine ölçer (INV-T4).
    pub delta_json: JsonValue,
}

// ── G2: Operator-only tool input'ları ──────────────────────────────────────────

/// `osp_trajectory_init` input (operator-only, INV-T2). Vision JSON + label alır.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrajectoryInitInput {
    /// Trajectory label (insan-okur).
    pub label: String,
    /// VisionVector JSON (RawPosition + source). Operator hedef koordinatı verir.
    pub vision_json: JsonValue,
}

/// `osp_task_add` input (operator-only, INV-T2). Task JSON alır, registry'ye ekler.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAddInput {
    /// Task JSON (tüm alanlar pub, serde Deserialize — explorer doğruladı, builder gerekmez).
    pub task_json: JsonValue,
}

// ── G2: Navigator loop tool input'ları (agent-facing) ──────────────────────────

/// `osp_run_task` input. Navigator loop — sadece task_id (LLM delta üretir).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunTaskInput {
    /// Task ID (navigator bu task için maneuver_limit kadar LLM çağrısı yapar).
    pub task_id: u64,
    /// Maneuver limit override (opsiyonel). Yoksa task.policy.maneuver_limit kullanılır.
    #[serde(default)]
    pub maneuver_limit: Option<u32>,
}

/// `osp_get_attempt_history` input. Task'ın navigator evidence ledger'ı.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetAttemptHistoryInput {
    /// Task ID.
    pub task_id: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// OspMcpServer — rmcp ServerHandler + tool router
// ═══════════════════════════════════════════════════════════════════════════════

/// Evidence store — task_id → navigator'ın biriktirdiği evidence listesi (RQ6 verisi).
type EvidenceStore = Arc<std::sync::Mutex<HashMap<TaskId, Vec<TrajectoryEvidence>>>>;

/// OSP MCP server. `SharedWorkspace` + `TaskRegistry` + `ServerMode` + `LlmClient` taşır.
///
/// **INV-T2:** `operator_capability: None` (agent mode) veya `Some` (operator mode).
/// Agent mode'da operator tool'lar runtime gate ile reddedilir (`allows_operator_tools()` false).
///
/// **G2 — Navigator loop:** `llm: Arc<dyn LlmClient>` startup'ta inject edilir
/// (`--llm mock|real`). `osp_run_task` bu client ile multi-attempt navigator çalıştırır.
///
/// **State:** Workspace analyze-once, registry demo task ile kurulu, evidence per-task saklanır.
pub struct OspMcpServer {
    /// Analyze edilmiş workspace (Arc<Mutex> — sync, spawn_blocking ile köprü).
    workspace: SharedWorkspace,
    /// Task registry (sync Mutex — workspace ile tutarlı, spawn_blocking içine taşınır).
    registry: Arc<std::sync::Mutex<InMemoryTaskRegistry>>,
    /// Server mode (agent/operator).
    mode: ServerMode,
    /// OperatorCapability (operator mode'da Some — INV-T2). Request'ten ASLA.
    operator_capability: Option<OperatorCapability>,
    /// LLM client (G2 — navigator loop için). Mock veya RuntimeLlmClient (startup inject).
    llm: Arc<dyn LlmClient>,
    /// Trajectory ID (tek trajectory G2'de; multi-trajectory G3+).
    trajectory_id: TrajectoryId,
    /// Evidence store — task_id → navigator evidence (osp_get_attempt_history için).
    evidence_store: EvidenceStore,
    /// Tool router (#[tool_handler] bu field'ı kullanır: `self.tool_router`).
    tool_router: ToolRouter<OspMcpServer>,
}

impl OspMcpServer {
    /// Yeni server kur (startup'ta main.rs çağırır). LLM client inject edilir.
    pub fn new(workspace: Workspace, mode: ServerMode, llm: Arc<dyn LlmClient>) -> Self {
        let operator_capability = if mode.allows_operator_tools() {
            // Trusted-boundary API (PR35 hardening) — operator-mode startup.
            Some(OperatorCapability::issue_for_operator_session())
        } else {
            None
        };
        let mut registry = InMemoryTaskRegistry::new();
        // G1: default demo task ekle (coupling <= 0.55). Operator task_add ile override edilebilir.
        registry.insert(default_demo_task());
        Self {
            workspace: Arc::new(std::sync::Mutex::new(workspace)),
            registry: Arc::new(std::sync::Mutex::new(registry)),
            mode,
            operator_capability,
            llm,
            trajectory_id: 1,
            evidence_store: Arc::new(std::sync::Mutex::new(HashMap::new())),
            tool_router: Self::get_tool_router(),
        }
    }

    /// Server mode referansı (main.rs serve için).
    pub fn mode(&self) -> ServerMode {
        self.mode
    }

    /// Shared workspace clone (test için).
    pub fn workspace_handle(&self) -> SharedWorkspace {
        Arc::clone(&self.workspace)
    }

    /// INV-T2 runtime gate — operator tool çağrısında agent mode reddi.
    /// Agent mode'da `OperatorCapabilityRequired` error envelope döndürür.
    /// Operator mode'da capability referansı döndürür (Trajectory::new için gerekli).
    fn gate_operator_tool(&self, tool_name: &str) -> Result<&OperatorCapability, String> {
        if let Some(cap) = self.operator_capability.as_ref() {
            Ok(cap)
        } else {
            let env = McpEnvelope::error(
                tool_name,
                EnvelopeError::new(
                    ErrorCode::OperatorCapabilityRequired,
                    format!(
                        "tool '{tool_name}' requires operator mode — agent mode denied (INV-T2)"
                    ),
                ),
            );
            Err(serde_json::to_string(&env).map_err(|e| e.to_string())?)
        }
    }
}

// ── Tool handler'ları (#[tool_router] on impl block, #[tool] on methods) ──────
//
// Not: `#[tool_router(router = get_tool_router)]` generated function'ı yeniden adlandırır.
// Bu sayede struct field `tool_router` (#[tool_handler] default) ile collision olmaz.

#[tool_router(router = get_tool_router)]
impl OspMcpServer {
    /// `osp_analyze_workspace` — repo snapshot döndür (node/edge count, coverage).
    ///
    /// **INV:** #8 (network-free core), #4 (engine computes raw position).
    #[tool(
        name = "osp_analyze_workspace",
        description = "Analyze the workspace and return a space snapshot (node count, edge count, repo metrics, SCIP coverage). The workspace is fixed at server startup — no path input from the agent."
    )]
    async fn osp_analyze_workspace(
        &self,
        _input: Parameters<AnalyzeWorkspaceInput>,
    ) -> Result<String, String> {
        let summary = {
            let ws = self.workspace.lock().map_err(|e| e.to_string())?;
            ws.snapshot_summary()
        };
        let envelope = McpEnvelope::success(
            "osp_analyze_workspace",
            summary,
            vec!["INV-#4".into(), "INV-#8".into()],
        );
        let checked = envelope.assert_no_coordinate_leak("osp_analyze_workspace");
        serde_json::to_string(&checked).map_err(|e| e.to_string())
    }

    /// `osp_get_agent_task_view` ⭐ — INV-T1 merkez tool.
    ///
    /// Agent'a sadece predicate + current measurement + allowed_ops döner.
    /// preferred_vector / target_region / milestone_target_vector ASLA.
    #[tool(
        name = "osp_get_agent_task_view",
        description = "Get the AgentTaskView for a task — INV-T1 epistemic projection. Returns task_id, label, current_measurement, target_predicate, allowed_operations, constraints. NEVER returns target coordinates (preferred_vector, target_region, milestone_target_vector)."
    )]
    async fn osp_get_agent_task_view(
        &self,
        Parameters(input): Parameters<GetAgentTaskViewInput>,
    ) -> Result<String, String> {
        // 1. Task resolve.
        let task = {
            let reg = self.registry.lock().map_err(|e| e.to_string())?;
            match reg.resolve(input.task_id) {
                Some(t) => t.clone(),
                None => {
                    let env = McpEnvelope::error(
                        "osp_get_agent_task_view",
                        EnvelopeError::new(
                            ErrorCode::TaskNotFound,
                            format!("task {} not found in registry", input.task_id),
                        ),
                    );
                    return serde_json::to_string(&env).map_err(|e| e.to_string());
                }
            }
        };
        // 2. Current measurement (engine-measured).
        let current_raw = {
            let ws = self.workspace.lock().map_err(|e| e.to_string())?;
            ws.current_raw()
        };
        // 3. InternalTaskPlan → AgentTaskView (INV-T1 — koordinat düşürülür).
        let target_vector = task
            .target_predicate_set
            .preferred_vector
            .unwrap_or_default();
        let plan = InternalTaskPlan {
            task_id: task.id,
            milestone_target_vector: target_vector,
            task_predicate: task.target_predicate_set.clone(),
            tolerance: 0.02,
        };
        let agent_view = plan.to_agent_view(
            &task.label,
            current_raw,
            task.allowed_operations.clone(),
            task.constraints.clone(),
            Vec::new(),
            None, // G2c-4: structural context (MCP get_agent_task_view opsiyonel)
        );
        // 4. Serialize + INV-T1 leak check.
        let view_json = serde_json::to_value(&agent_view).map_err(|e| e.to_string())?;
        let envelope =
            McpEnvelope::success("osp_get_agent_task_view", view_json, vec!["INV-T1".into()]);
        let checked = envelope.assert_no_coordinate_leak("osp_get_agent_task_view");
        serde_json::to_string(&checked).map_err(|e| e.to_string())
    }

    /// `osp_check_predicate` — mevcut position ile predicate değerlendir.
    #[tool(
        name = "osp_check_predicate",
        description = "Evaluate the task's predicate against the CURRENT engine-measured position (no delta). Returns predicate_completion: Completed / SourceInsufficient / NotCompleted."
    )]
    async fn osp_check_predicate(
        &self,
        Parameters(input): Parameters<CheckPredicateInput>,
    ) -> Result<String, String> {
        let task = {
            let reg = self.registry.lock().map_err(|e| e.to_string())?;
            match reg.resolve(input.task_id) {
                Some(t) => t.clone(),
                None => {
                    let env = McpEnvelope::error(
                        "osp_check_predicate",
                        EnvelopeError::new(
                            ErrorCode::TaskNotFound,
                            format!("task {} not found", input.task_id),
                        ),
                    );
                    return serde_json::to_string(&env).map_err(|e| e.to_string());
                }
            }
        };
        let (measured, completion) = {
            let ws = self.workspace.lock().map_err(|e| e.to_string())?;
            let m = ws.current_measured();
            let result = task.target_predicate_set.evaluate_completion(&m);
            (m, result)
        };
        let completion_str = match completion {
            PredicateSetResult::Completed => "Completed",
            PredicateSetResult::SourceInsufficient => "SourceInsufficient",
            PredicateSetResult::NotCompleted => "NotCompleted",
        };
        let measured_json = serde_json::to_value(&measured).map_err(|e| e.to_string())?;
        let envelope = McpEnvelope::success(
            "osp_check_predicate",
            serde_json::json!({
                "predicate_completion": completion_str,
                "current_measurement": measured_json,
            }),
            vec!["INV-T3".into(), "INV-T4".into()],
        );
        let checked = envelope.assert_no_coordinate_leak("osp_check_predicate");
        serde_json::to_string(&checked).map_err(|e| e.to_string())
    }

    /// `osp_submit_delta` — DeltaProposal → engine measure → PredicateGate → outcome.
    ///
    /// **INV:** T6 (failure≠regression), T7 (maneuver limit), T8 (progress≠merge).
    #[tool(
        name = "osp_submit_delta",
        description = "Submit a DeltaProposal (structural-only, NO positions) for a task. Engine measures the simulated-after position, PredicateGate evaluates, returns mutation decision. INV-T6/T7/T8 enforced."
    )]
    async fn osp_submit_delta(
        &self,
        Parameters(input): Parameters<SubmitDeltaInput>,
    ) -> Result<String, String> {
        // 1. Parse DeltaProposal (Q4 syntax).
        let proposal: DeltaProposal = match serde_json::from_value(input.delta_json.clone()) {
            Ok(p) => p,
            Err(e) => {
                let env = McpEnvelope::error(
                    "osp_submit_delta",
                    EnvelopeError::new(
                        ErrorCode::InvalidDeltaProposal,
                        format!("DeltaProposal parse failed: {e}"),
                    ),
                );
                return serde_json::to_string(&env).map_err(|e| e.to_string());
            }
        };
        // 2. Task resolve.
        let task = {
            let reg = self.registry.lock().map_err(|e| e.to_string())?;
            match reg.resolve(input.task_id) {
                Some(t) => t.clone(),
                None => {
                    let env = McpEnvelope::error(
                        "osp_submit_delta",
                        EnvelopeError::new(
                            ErrorCode::TaskNotFound,
                            format!("task {} not found", input.task_id),
                        ),
                    );
                    return serde_json::to_string(&env).map_err(|e| e.to_string());
                }
            }
        };
        // 3. Single-attempt submit (engine measure + PredicateGate).
        let outcome_json = {
            let mut ws = self.workspace.lock().map_err(|e| e.to_string())?;
            ws.submit_delta_attempt(&proposal, &task, input.task_id)
                .map_err(|e| e)?
        };
        let envelope = McpEnvelope::success(
            "osp_submit_delta",
            outcome_json,
            vec!["INV-T6".into(), "INV-T7".into(), "INV-T8".into()],
        );
        let checked = envelope.assert_no_coordinate_leak("osp_submit_delta");
        serde_json::to_string(&checked).map_err(|e| e.to_string())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // G2a — OPERATOR-ONLY TOOLS (INV-T2 runtime gate)
    // ═══════════════════════════════════════════════════════════════════════════

    /// `osp_trajectory_init` (operator-only) — Trajectory + VisionVector oluştur.
    ///
    /// **INV-T2:** Agent mode reddedilir. Operator hedef koordinat (vision) verir.
    /// **Leak check YOK** — operator coordinate görür (envelope.rs doc kuralı).
    #[tool(
        name = "osp_trajectory_init",
        description = "OPERATOR-ONLY (INV-T2). Initialize a Trajectory with a vision (target coordinate). Agent mode returns OperatorCapabilityRequired error. Returns trajectory_id + vision summary."
    )]
    async fn osp_trajectory_init(
        &self,
        Parameters(input): Parameters<TrajectoryInitInput>,
    ) -> Result<String, String> {
        // INV-T2 runtime gate.
        let _cap = self.gate_operator_tool("osp_trajectory_init")?;
        // Vision parse.
        let vision: osp_core::vision::VisionVector =
            match serde_json::from_value(input.vision_json.clone()) {
                Ok(v) => v,
                Err(e) => {
                    let env = McpEnvelope::error(
                        "osp_trajectory_init",
                        EnvelopeError::new(
                            ErrorCode::InvalidToolInput,
                            format!("vision_json parse failed: {e}"),
                        ),
                    );
                    return serde_json::to_string(&env).map_err(|e| e.to_string());
                }
            };
        // Trajectory kur (INV-T2 — OperatorCapability ile).
        let trajectory = osp_core::trajectory::Trajectory::new(
            _cap,
            self.trajectory_id,
            input.label.clone(),
            vision.clone(),
        );
        // Sonuç (operator coordinate içerir — leak check YOK).
        let result = serde_json::json!({
            "trajectory_id": trajectory.id,
            "label": trajectory.label,
            "vision": vision,
            "milestone_count": trajectory.milestones.len(),
            "mode": self.mode.as_str(),
        });
        let envelope = McpEnvelope::success("osp_trajectory_init", result, vec!["INV-T2".into()]);
        // Operator tool — leak check INTENTIONALLY skipped (operator görebilir).
        serde_json::to_string(&envelope).map_err(|e| e.to_string())
    }

    /// `osp_task_add` (operator-only) — registry'ye Task ekle.
    ///
    /// **INV-T2:** Agent mode reddedilir. Task JSON tüm alanlarla (pub) deserialize edilir.
    #[tool(
        name = "osp_task_add",
        description = "OPERATOR-ONLY (INV-T2). Add a Task (full JSON: predicate, policy, allowed_operations) to the registry. Agent mode returns OperatorCapabilityRequired error. Returns task_id + confirmation."
    )]
    async fn osp_task_add(
        &self,
        Parameters(input): Parameters<TaskAddInput>,
    ) -> Result<String, String> {
        // INV-T2 runtime gate.
        let _cap = self.gate_operator_tool("osp_task_add")?;
        // Task parse (tüm alanlar pub, serde Deserialize — builder gerekmez).
        let task: Task = match serde_json::from_value(input.task_json.clone()) {
            Ok(t) => t,
            Err(e) => {
                let env = McpEnvelope::error(
                    "osp_task_add",
                    EnvelopeError::new(
                        ErrorCode::InvalidToolInput,
                        format!("task_json parse failed: {e}"),
                    ),
                );
                return serde_json::to_string(&env).map_err(|e| e.to_string());
            }
        };
        let task_id = task.id;
        // Registry'ye ekle (sync Mutex).
        {
            let mut reg = self.registry.lock().map_err(|e| e.to_string())?;
            reg.insert(task.clone());
        }
        // Sonuç (preferred_vector operator görür — leak check YOK).
        let result = serde_json::json!({
            "task_id": task_id,
            "label": task.label,
            "milestone_id": task.milestone_id,
            "maneuver_limit": task.policy.maneuver_limit,
            "predicate_count": task.target_predicate_set.predicates.len(),
            "allowed_operations": task.allowed_operations,
        });
        let envelope = McpEnvelope::success("osp_task_add", result, vec!["INV-T2".into()]);
        serde_json::to_string(&envelope).map_err(|e| e.to_string())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // G2b — NAVIGATOR LOOP TOOLS (agent-facing, INV-T1/T7/T8 enforced)
    // ═══════════════════════════════════════════════════════════════════════════

    /// `osp_run_task` ⭐ — navigator loop (multi-attempt, LLM delta üretir).
    ///
    /// **INV-T1:** AgentTaskView kullanır (navigator loop INV-T1 güvenli).
    /// **INV-T7:** maneuver_limit kadar attempt (task.policy veya override).
    /// **INV-T8:** AcceptAsProgress → TrajectoryCheckpoint lane (Mainline DEĞİL).
    /// Sync→async bridge: blocking navigator `spawn_blocking` ile çalışır.
    #[tool(
        name = "osp_run_task",
        description = "Run the navigator loop for a task (multi-attempt, LLM produces deltas). Returns NavigatorResult: Completed / ExceededManeuverLimit / RequiresOperatorApproval / LlmError. INV-T1/T7/T8 enforced. Evidence stored for osp_get_attempt_history."
    )]
    async fn osp_run_task(
        &self,
        Parameters(input): Parameters<RunTaskInput>,
    ) -> Result<String, String> {
        // Task'ı clone'la (sync lock scope'tan çıkar — navigator uzun süre çalışır).
        let task = {
            let reg = self.registry.lock().map_err(|e| e.to_string())?;
            match reg.resolve(input.task_id) {
                Some(t) => t.clone(),
                None => {
                    let env = McpEnvelope::error(
                        "osp_run_task",
                        EnvelopeError::new(
                            ErrorCode::TaskNotFound,
                            format!("task {} not found", input.task_id),
                        ),
                    );
                    return serde_json::to_string(&env).map_err(|e| e.to_string());
                }
            }
        };
        // Maneuver limit override uygula (varsa task.policy'yi geçici değiştir).
        if let Some(limit) = input.maneuver_limit {
            let mut reg = self.registry.lock().map_err(|e| e.to_string())?;
            if let Some(t) = reg.tasks.get_mut(&input.task_id) {
                t.policy.maneuver_limit = limit;
            }
        }
        // Mutex handle'ları clone'la (Arc) — spawn_blocking içine taşı.
        let workspace = Arc::clone(&self.workspace);
        let registry = Arc::clone(&self.registry);
        let llm = Arc::clone(&self.llm);
        let evidence_store = Arc::clone(&self.evidence_store);
        let trajectory_id = self.trajectory_id;
        let task_id = input.task_id;

        // Sync navigator'ı spawn_blocking ile çalıştır (LLM HTTP blocking).
        let nav_result = tokio::task::spawn_blocking(move || -> Result<JsonValue, String> {
            // Tüm lock'ları bu sync scope içinde al.
            let mut ws = workspace.lock().map_err(|e| e.to_string())?;
            let reg = registry.lock().map_err(|e| e.to_string())?;
            // AgentNavigator: llm (Arc<dyn> → &dyn), resolver (&InMemoryTaskRegistry),
            // engine (&mut SpaceEngine), evidence (&mut Vec).
            let mut evidence: Vec<TrajectoryEvidence> = Vec::new();
            let current_measured = ws.current_measured();
            let target_vector = task
                .target_predicate_set
                .preferred_vector
                .unwrap_or_default();
            let mut nav = osp_core::navigator::AgentNavigator {
                llm: llm.as_ref(),
                resolver: &*reg,
                engine: ws.engine_mut(),
                evidence: &mut evidence,
                trajectory_id,
                milestone_id: task.milestone_id,
                target_vector,
                current_measured: current_measured.clone(),
                output_contract: osp_core::agent::OutputContract::strict(),
                // MCP server = production context → Production witness (min_approvers=2).
                witness_policy: osp_core::navigator::NavigatorWitnessPolicy::Production,
                // INV-T9: production filesystem store.
                pending_authorization_store: Box::new(
                    osp_core::authorization::FilesystemPendingAuthorizationStore::new("."),
                ),
                clock: Box::new(osp_core::authorization::SystemClock),
            };
            let result = nav.run_task(task_id, 1);
            // Evidence'ı store'a kaydet (RQ6 verisi).
            let attempt_count = match &result {
                osp_core::navigator::NavigatorResult::Completed { attempts, .. } => *attempts,
                osp_core::navigator::NavigatorResult::ExceededManeuverLimit {
                    attempts, ..
                } => *attempts,
                osp_core::navigator::NavigatorResult::RequiresOperatorApproval {
                    attempts, ..
                } => *attempts,
                _ => 0,
            };
            let _ = attempt_count;
            if !evidence.is_empty() || attempt_count > 0 {
                let mut store = evidence_store.lock().map_err(|e| e.to_string())?;
                store.entry(task_id).or_default().extend(evidence.clone());
            }
            Ok(serialize_navigator_result(&result))
        })
        .await
        .map_err(|e| format!("navigator task panicked: {e}"))??;

        let envelope = McpEnvelope::success(
            "osp_run_task",
            nav_result,
            vec!["INV-T1".into(), "INV-T7".into(), "INV-T8".into()],
        );
        let checked = envelope.assert_no_coordinate_leak("osp_run_task");
        serde_json::to_string(&checked).map_err(|e| e.to_string())
    }

    /// `osp_get_attempt_history` — task'ın navigator evidence ledger'ı (RQ6 verisi).
    ///
    /// Her osp_run_task çağrısında biriken TrajectoryEvidence listesini döndürür.
    /// Agent-facing — token cost, attempt outcome, gate decisions içerir (coordinate YOK).
    #[tool(
        name = "osp_get_attempt_history",
        description = "Get the navigator evidence ledger for a task (attempt outcomes, token costs, gate decisions). Populated by prior osp_run_task calls. RQ6 token-cost data source."
    )]
    async fn osp_get_attempt_history(
        &self,
        Parameters(input): Parameters<GetAttemptHistoryInput>,
    ) -> Result<String, String> {
        let evidence = {
            let store = self.evidence_store.lock().map_err(|e| e.to_string())?;
            store.get(&input.task_id).cloned().unwrap_or_default()
        };
        let result = serde_json::json!({
            "task_id": input.task_id,
            "attempt_count": evidence.len(),
            "evidence": evidence,
        });
        let envelope = McpEnvelope::success("osp_get_attempt_history", result, vec!["RQ6".into()]);
        let checked = envelope.assert_no_coordinate_leak("osp_get_attempt_history");
        serde_json::to_string(&checked).map_err(|e| e.to_string())
    }
}

/// NavigatorResult → JSON (serialize variant + attempts + tokens + last outcome).
/// Coordinate İÇERMEZ (INV-T1 — AttemptOutcome predicate/gate info taşır, koordinat değil).
fn serialize_navigator_result(result: &osp_core::navigator::NavigatorResult) -> JsonValue {
    use osp_core::navigator::NavigatorResult;
    match result {
        NavigatorResult::Completed {
            attempts,
            total_tokens,
        } => serde_json::json!({
            "outcome": "Completed",
            "attempts": attempts,
            "total_tokens": total_tokens.total_tokens,
        }),
        NavigatorResult::ExceededManeuverLimit {
            attempts,
            last_outcome,
        } => serde_json::json!({
            "outcome": "ExceededManeuverLimit",
            "attempts": attempts,
            "last_mutation_decision": format!("{:?}", last_outcome.mutation_decision),
            "last_predicate_completion": format!("{:?}", last_outcome.predicate_completion),
        }),
        NavigatorResult::AwaitingWitnesses { pending, persistence } => serde_json::json!({
            "outcome": "AwaitingWitnesses",
            "task_id": pending.task_id,
            "claim_id": pending.claim_id,
            "witness_hold_reason": pending.witness_hold_reason.as_reason_str(),
            "commit_state": "awaiting_witnesses",
            "mainline_mutation": "not_applied",
            "pending_artifact": persistence.artifact_path.to_string_lossy(),
            "next_action": "await_external_evidence",
        }),
        NavigatorResult::RequiresRevision(rev) => serde_json::json!({
            "outcome": "RequiresRevision",
            "task_id": rev.task_id,
            "claim_id": rev.claim_id,
            "rejecting_witnesses": rev.reasons.as_slice().iter().map(|r| r.witness).collect::<Vec<_>>(),
            "commit_state": "rejected_by_witness",
            "next_action": "requires_revision",
        }),
        NavigatorResult::PendingAuthorizationPersistenceFailure { pending, error } => serde_json::json!({
            "outcome": "PendingAuthorizationPersistenceFailure",
            "task_id": pending.task_id,
            "claim_id": pending.claim_id,
            "error": error.to_string(),
        }),
        NavigatorResult::WitnessEvaluationError(msg) => serde_json::json!({
            "outcome": "WitnessEvaluationError",
            "message": msg,
        }),
        NavigatorResult::SystemFailure(msg) => serde_json::json!({
            "outcome": "SystemFailure",
            "message": msg,
        }),
        NavigatorResult::TaskNotFound => serde_json::json!({ "outcome": "TaskNotFound" }),
        NavigatorResult::RequiresOperatorApproval {
            attempts,
            last_outcome,
        } => serde_json::json!({
            "outcome": "RequiresOperatorApproval",
            "attempts": attempts,
            "last_mutation_decision": format!("{:?}", last_outcome.mutation_decision),
            "last_apply_target": format!("{:?}", last_outcome.mutation_decision.apply_target()),
        }),
        NavigatorResult::LlmError(e) => serde_json::json!({
            "outcome": "LlmError",
            "error": e.to_string(),
        }),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ServerHandler trait impl (#[tool_handler] default: self.tool_router field)
// ═══════════════════════════════════════════════════════════════════════════════

#[tool_handler]
impl ServerHandler for OspMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "osp-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            instructions: Some(format!(
                "OSP MCP Server — Ontological Space Protocol agent access surface. \
                 Mode: {}. Tools: osp_analyze_workspace, osp_get_agent_task_view, \
                 osp_check_predicate, osp_submit_delta. INV-T1..T8 enforced — \
                 target coordinates never exposed to agents.",
                self.mode.as_str()
            )),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace helpers (MCP-specific ops on osp-core engine)
// ═══════════════════════════════════════════════════════════════════════════════

impl Workspace {
    /// Mevcut engine-measured RawPosition.
    /// G1: default measured position (osp-cli ile uyumlu). Engine full re-measure G2'de.
    pub fn current_raw(&self) -> osp_core::coords::RawPosition {
        osp_core::coords::RawPosition {
            x: 0.7,
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.3,
        }
    }

    /// Mevcut ProvenancedRawPosition (INV-T4 source ile).
    pub fn current_measured(&self) -> ProvenancedRawPosition {
        provenanced_from_raw(self.current_raw(), MetricSource::Scip)
    }

    /// Tek DeltaProposal'ı değerlendir (single attempt — no LLM loop).
    ///
    /// **Akış:** proposal → engine.compute_raw_from_delta (measure) → Claim build →
    /// commit_task_claim → AttemptOutcome. INV-T6/T7/T8 gate içinde enforced.
    pub fn submit_delta_attempt(
        &mut self,
        proposal: &DeltaProposal,
        task: &Task,
        task_id: TaskId,
    ) -> Result<JsonValue, String> {
        use osp_core::space::{Edge, Node, NodeId};
        use osp_core::witness::WitnessSet;

        // Empty proposal check.
        if proposal.new_nodes.is_empty() && proposal.new_edges.is_empty() {
            return Ok(serde_json::json!({
                "attempt_outcome": {
                    "gate_decision": "RejectedBySyntax",
                    "predicate_completion": "NotCompleted",
                    "mutation_decision": "Reject",
                    "witness_status": null,
                },
                "apply_target": "NotApplied",
                "loss_after": null,
                "measured_after": null,
                "message": "DeltaProposal has no nodes/edges",
            }));
        }

        // 1. DeltaProposal → delta_nodes + delta_edges.
        let delta_nodes: Vec<Node> = proposal
            .new_nodes
            .iter()
            .enumerate()
            .map(|(i, spec)| Node {
                id: (10_000 + i as NodeId),
                kind: spec.kind,
                mass: spec.initial_mass,
                ..Default::default()
            })
            .collect();
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

        // 2. Engine measure (INV-T3 — agent değiştiremez). G2c-2: removed_edges +
        // affected_nodes geçir (coupling-reducing proposals için).
        let mut affected: Vec<osp_core::space::NodeId> = proposal.affected_nodes.clone();
        for er in &proposal.removed_edges {
            if !affected.contains(&er.from) {
                affected.push(er.from);
            }
        }
        let computed_raw = self.engine_mut().compute_raw_from_delta(
            &delta_nodes,
            &delta_edges,
            &proposal.removed_edges,
            &affected,
        );

        // 3. Claim build + commit_task_claim.
        let claim = match build_claim_from_proposal(&proposal, computed_raw, task_id, 1, 1) {
            Ok(c) => c,
            Err(e) => {
                return Ok(serde_json::json!({
                    "attempt_outcome": {
                        "gate_decision": "RejectedBySyntax",
                        "predicate_completion": "NotCompleted",
                        "mutation_decision": "Reject",
                        "witness_status": null,
                    },
                    "apply_target": "NotApplied",
                    "loss_after": null,
                    "measured_after": null,
                    "message": format!("claim build: {e}"),
                }))
            }
        };
        let measured = provenanced_from_raw(claim.computed_raw, MetricSource::Scip);
        let target = task
            .target_predicate_set
            .preferred_vector
            .unwrap_or_default();
        let loss_before = osp_core::trajectory::trajectory_loss(&self.current_measured(), &target);
        let omega = WitnessSet::new(Vec::new());
        let mut tmp_reg = InMemoryTaskRegistry::new();
        tmp_reg.insert(task.clone());
        let result = match self
            .engine_mut()
            .commit_task_claim(osp_core::engine::TaskCommitInput {
                claim: &claim,
                omega: &omega,
                task_resolver: &tmp_reg as &dyn TaskResolver,
                target,
                loss_before,
                measured: measured.clone(),
            }) {
            Ok(osp_core::engine::EngineCommitResult::Applied(r)) => r,
            Ok(osp_core::engine::EngineCommitResult::Held { reason, snapshot }) => {
                // **INV-T9** — expected authorization bekleme. Agent failure DEĞİL.
                return Ok(serde_json::json!({
                    "commit_result": "Held",
                    "witness_hold_reason": reason.as_reason_str(),
                    "witness_snapshot": {
                        "approvers": snapshot.approvers,
                        "required_approvers": snapshot.required_approvers,
                        "support": snapshot.support,
                        "required_support": snapshot.required_support,
                    },
                    "commit_state": "awaiting_witnesses",
                    "mainline_mutation": "not_applied",
                    "measured_after": serde_json::to_value(&measured).map_err(|e| e.to_string())?,
                    "next_action": "await_external_evidence",
                }));
            }
            Ok(osp_core::engine::EngineCommitResult::Rejected { reasons, snapshot }) => {
                // Explicit witness rejection — RequiresRevision.
                let witness_ids: Vec<_> = reasons.as_slice().iter().map(|r| r.witness).collect();
                return Ok(serde_json::json!({
                    "commit_result": "Rejected",
                    "rejecting_witnesses": witness_ids,
                    "witness_snapshot": {
                        "approvers": snapshot.approvers,
                        "required_approvers": snapshot.required_approvers,
                        "support": snapshot.support,
                        "required_support": snapshot.required_support,
                    },
                    "commit_state": "rejected_by_witness",
                    "mainline_mutation": "not_applied",
                    "measured_after": serde_json::to_value(&measured).map_err(|e| e.to_string())?,
                    "next_action": "requires_revision",
                }));
            }
            Err(e) => {
                return Ok(serde_json::json!({
                    "attempt_outcome": {
                        "gate_decision": "RejectedBySyntax",
                        "predicate_completion": "NotCompleted",
                        "mutation_decision": "Reject",
                        "witness_status": null,
                    },
                    "apply_target": "NotApplied",
                    "loss_after": null,
                    "measured_after": null,
                    "message": format!("commit_task_claim: {e}"),
                }))
            }
        };

        // 4. Serialize outcome (Applied path).
        let gate_str = match result.outcome.gate_decision {
            osp_core::trajectory::GateDecision::PassedAll => "PassedAll",
            osp_core::trajectory::GateDecision::RejectedBySyntax => "RejectedBySyntax",
            osp_core::trajectory::GateDecision::RejectedByVision => "RejectedByVision",
            osp_core::trajectory::GateDecision::RejectedByRule => "RejectedByRule",
            osp_core::trajectory::GateDecision::RejectedByTaskBinding => "RejectedByTaskBinding",
            osp_core::trajectory::GateDecision::BlockedByManeuverLimit => "BlockedByManeuverLimit",
            osp_core::trajectory::GateDecision::Unknown => "Unknown",
        };
        let pred_str = match result.outcome.predicate_completion {
            osp_core::trajectory::PredicateCompletion::Completed => "Completed",
            osp_core::trajectory::PredicateCompletion::NotCompleted => "NotCompleted",
        };
        let mut_str = match result.outcome.mutation_decision {
            osp_core::trajectory::MutationDecision::Reject => "Reject",
            osp_core::trajectory::MutationDecision::AcceptAsProgress => "AcceptAsProgress",
            osp_core::trajectory::MutationDecision::AcceptAsCompleted => "AcceptAsCompleted",
            osp_core::trajectory::MutationDecision::RequireOperatorApproval => {
                "RequireOperatorApproval"
            }
        };
        let apply_str = match result.apply_target {
            osp_core::trajectory::ApplyTarget::NotApplied => "NotApplied",
            osp_core::trajectory::ApplyTarget::Lane(lane) => match lane {
                osp_core::trajectory::CommitLane::Mainline => "Mainline",
                osp_core::trajectory::CommitLane::TrajectoryCheckpoint => "TrajectoryCheckpoint",
                osp_core::trajectory::CommitLane::Sandbox => "Sandbox",
            },
        };
        Ok(serde_json::json!({
            "attempt_outcome": {
                "gate_decision": gate_str,
                "predicate_completion": pred_str,
                "mutation_decision": mut_str,
                "witness_status": null,
            },
            "apply_target": apply_str,
            "loss_after": result.loss_after,
            "measured_after": serde_json::to_value(&measured).map_err(|e| e.to_string())?,
        }))
    }
}

/// Demo task — G1 için coupling <= 0.55 predicate (operator-only task_add Aşama G2'de).
fn default_demo_task() -> Task {
    use osp_core::coords::{MetricSource, RawPosition};
    use osp_core::trajectory::{
        ComparisonOp, MetricPredicate, OpKind, PredicateAxis, PredicateFailurePolicy,
        PredicateMode, PredicateScope, PredicateSet, TaskPolicy, TaskStatus, WeightedPredicate,
    };
    Task {
        id: 1,
        milestone_id: 1,
        label: "Reduce coupling to 0.55 (demo task)".into(),
        target_predicate_set: PredicateSet {
            mode: PredicateMode::All,
            predicates: vec![WeightedPredicate {
                predicate: MetricPredicate {
                    metric: PredicateAxis::Coupling,
                    operator: ComparisonOp::Le,
                    threshold: 0.55,
                    scope: PredicateScope::Node(0),
                    required_source: Some(MetricSource::Scip),
                    tolerance: 0.0,
                },
                weight: None,
            }],
            preferred_vector: Some(RawPosition {
                x: 0.55,
                y: 0.6,
                z: 0.4,
                w: 0.5,
                v: 0.3,
            }),
        },
        policy: TaskPolicy {
            maneuver_limit: 5,
            predicate_failure_policy: PredicateFailurePolicy::StrictReject,
            ..Default::default()
        },
        allowed_operations: vec![OpKind::RemoveImport],
        constraints: vec![],
        status: TaskStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::McpEnvelope;

    #[test]
    fn envelope_error_codes_round_trip() {
        let err = EnvelopeError::new(ErrorCode::TaskNotFound, "task 99 not found");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("TASK_NOT_FOUND"));
        assert!(json.contains("INV-T5"));
    }

    #[test]
    fn success_envelope_no_leak_passes() {
        let env = McpEnvelope::success(
            "osp_get_agent_task_view",
            serde_json::json!({ "task_id": 1, "label": "clean" }),
            vec!["INV-T1".into()],
        );
        let checked = env.assert_no_coordinate_leak("osp_get_agent_task_view");
        assert!(matches!(checked, McpEnvelope::Success { .. }));
    }

    #[test]
    fn success_envelope_with_leak_blocked() {
        let env = McpEnvelope::success(
            "osp_get_agent_task_view",
            serde_json::json!({ "target_region": { "x": 0.5 } }),
            vec![],
        );
        let checked = env.assert_no_coordinate_leak("osp_get_agent_task_view");
        match checked {
            McpEnvelope::Error { error, .. } => {
                assert_eq!(error.error_code, ErrorCode::TargetCoordinateLeakBlocked);
            }
            _ => panic!("should have blocked"),
        }
    }
}
