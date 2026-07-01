//! D3 — RuntimeLlmClient: navigator::LlmClient trait impl for real GPT-4o-mini.
//!
//! Runtime -> navigator::LlmClient adapter. Custom prompt (osp_system_prompt +
//! trajectory task context + AgentTaskView JSON) ile gerçek LLM çağrısı.
//! OspPrompt DEĞİŞMEZ (Paper 1 stub alanları korunur) — complete_raw bypass.
//!
//! **INV-T1:** AgentTaskView serialize edilir (hedef koordinat YOK).
//! **INV-#4:** System prompt agent'a "pozisyon DECLARE ETME" der.
//!
//! **G2:** `last_usage: Mutex<TokenUsage>` (Cell → Mutex) — `LlmClient: Send + Sync`
//! gereği (MCP server Arc<dyn LlmClient> + spawn_blocking). Mutex Sync'tir.

use std::sync::Mutex;

use osp_core::agent::DeltaProposal;
use osp_core::navigator::{LlmClient, LlmError as NavLlmError};
use osp_core::trajectory::{AgentTaskView, TokenCost};

use crate::error::LlmError as RtLlmError;
use crate::prompt::osp_system_prompt;
use crate::response::TokenUsage;
use crate::{CompletionRequest, Runtime};

/// D3 - Runtime -> navigator::LlmClient adapter. Gerçek GPT-4o-mini (veya OpenAI-compatible).
///
/// `Runtime::complete` OspPrompt alır, ama navigator AgentTaskView üretir. Bu adapter
/// `complete_raw`'ı custom CompletionRequest ile çağırır - OspPrompt'u bypass eder.
/// `system` = osp_system_prompt + trajectory task context, `user` = AgentTaskView JSON.
pub struct RuntimeLlmClient {
    runtime: Runtime,
    last_usage: Mutex<TokenUsage>,
}

impl RuntimeLlmClient {
    /// Mevcut Runtime ile adapter kur.
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            last_usage: Mutex::new(TokenUsage::default()),
        }
    }

    /// OPENAI_API_KEY env var'dan Runtime kur + adapter oluştur.
    pub fn from_env() -> Result<Self, RtLlmError> {
        Ok(Self::new(Runtime::from_env()?))
    }
}

impl LlmClient for RuntimeLlmClient {
    fn complete(&self, view: &AgentTaskView) -> Result<DeltaProposal, NavLlmError> {
        let req = CompletionRequest {
            system: trajectory_system_prompt(view),
            user: serde_json::to_string_pretty(view)
                .map_err(|e| NavLlmError::ProposalParse(format!("AgentTaskView serialize: {e}")))?,
        };
        let raw = self.runtime.complete_raw(&req).map_err(map_runtime_error)?;
        *self.last_usage.lock().expect("last_usage poisoned") = raw.usage;
        let (proposal, _) = raw.into_proposal().map_err(|(raw_text, parse_err)| {
            NavLlmError::ProposalParse(format!(
                "LLM response parse failed: {parse_err}\nRaw: {raw_text}"
            ))
        })?;
        Ok(proposal)
    }

    fn last_token_cost(&self) -> TokenCost {
        let u = self.last_usage.lock().expect("last_usage poisoned").clone();
        TokenCost {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }
    }
}

/// D3 - navigator task için özel system prompt. osp_system_prompt() + trajectory bağlamı.
fn trajectory_system_prompt(view: &AgentTaskView) -> String {
    let m = &view.current_measurement;
    let base = osp_system_prompt();
    let ctx = format!(
        "TRAJECTORY TASK CONTEXT (Paper 2 - Architectural Trajectory Navigation):\n\
You receive an AgentTaskView - a typed epistemic projection of an architecture task.\n\
\n\
CURRENT STATE (engine-measured, NOT your claim):\n\
- coupling (x): {:.3}\n\
- cohesion (y): {:.3}\n\
- instability (z): {:.3}\n\
\n\
TASK:\n\
- task_id: {}\n\
- label: {}\n\
- target_predicate: constraints the NEXT engine-measured state must satisfy\n\
- allowed_operations: structural operations you MAY propose\n\
- constraints: rules that MUST hold\n\
\n\
INSTRUCTIONS:\n\
1. Analyze the target_predicate - what architectural change moves toward satisfying it?\n\
2. Produce a DeltaProposal with structural changes (new_nodes, new_edges, modified_entities).\n\
3. Use ONLY operations from allowed_operations.\n\
4. DO NOT declare positions - the engine measures. position_hints are advisory only (INV-T4).\n\
5. Provide clear reasoning for your proposed changes.\n\
\n\
OUTPUT FORMAT (strict JSON, no markdown fences):\n\
{{\n\
  \"new_nodes\": [],\n\
  \"new_edges\": [],\n\
  \"modified_entities\": [],\n\
  \"position_hints\": [],\n\
  \"reasoning\": \"explain your changes\"\n\
}}\n\
\n\
new_nodes item: {{\"kind\": \"Module\", \"initial_mass\": 100.0, \"connected_to\": [[0, \"Imports\"]]}}\n\
new_edges item: {{\"from\": 0, \"to\": 1, \"kind\": \"Imports\"}}\n\
modified_entities item: {{\"node_id\": 0}}\n\
position_hints: leave empty (engine measures positions)\n\
kind values: Module, Feature, Test, Config, Migration\n\
edge kind values: Imports, Calls, Implements, Extends",
        m.x, m.y, m.z, view.task_id, view.label
    );
    // D4 - Calibration feedback: önceki attempt'lerin hatalarını LLM'e göster.
    let feedback_section = if view.feedback_history.is_empty() {
        String::new()
    } else {
        let items: String = view
            .feedback_history
            .iter()
            .map(|f| format!("- {f}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\n\nPREVIOUS ATTEMPTS FAILED — learn from these errors:\n{items}\n\nDo NOT repeat these mistakes. Adjust your approach.")
    };
    format!("{base}\n\n{ctx}{feedback_section}")
}

/// D3 - Runtime LlmError -> navigator LlmError mapping.
fn map_runtime_error(e: RtLlmError) -> NavLlmError {
    match e {
        RtLlmError::Http(_) | RtLlmError::Status { .. } | RtLlmError::MissingApiKey => {
            NavLlmError::Network(e.to_string())
        }
        RtLlmError::BadResponse(msg) => {
            NavLlmError::ProposalParse(format!("Bad API response: {msg}"))
        }
        RtLlmError::ProposalParse { raw, .. } => NavLlmError::ProposalParse(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osp_core::coords::RawPosition;
    use osp_core::trajectory::{AgentPredicateView, PredicateMode};

    // 1. runtime_llm_client_compiles (compile-time - trait impl)
    #[test]
    fn runtime_llm_client_implements_navigator_trait() {
        fn accepts_llm_client<L: LlmClient>(_c: &L) {}
        // Trait bound compile-time guarantee. Gerçek instance from_env() (API key).
        let _ = accepts_llm_client::<RuntimeLlmClient>;
    }

    // 2. trajectory_system_prompt_includes_task_context
    #[test]
    fn trajectory_system_prompt_includes_task_context() {
        let view = AgentTaskView {
            task_id: 42,
            label: "Reduce coupling".into(),
            current_measurement: RawPosition {
                x: 0.82,
                y: 0.5,
                z: 0.6,
                w: 0.5,
                v: 0.3,
            },
            target_predicate: AgentPredicateView {
                mode: PredicateMode::All,
                predicates: vec![],
            },
            allowed_operations: vec![],
            constraints: vec![],
            feedback_history: vec![],
        };
        let prompt = trajectory_system_prompt(&view);
        assert!(prompt.contains("task_id: 42"), "task_id in prompt");
        assert!(prompt.contains("Reduce coupling"), "label in prompt");
        assert!(prompt.contains("0.820"), "coupling measurement in prompt");
        assert!(
            prompt.contains("DO NOT declare positions"),
            "INV-T4 warning in prompt"
        );
    }

    // 3. error_mapping_runtime_to_navigator
    #[test]
    fn error_mapping_runtime_to_navigator() {
        let net_err = map_runtime_error(RtLlmError::MissingApiKey);
        assert!(matches!(net_err, NavLlmError::Network(_)));

        let status_err = map_runtime_error(RtLlmError::Status {
            code: 500,
            body: "server error".into(),
        });
        assert!(matches!(status_err, NavLlmError::Network(_)));

        let parse_err = map_runtime_error(RtLlmError::ProposalParse {
            raw: "invalid json".into(),
            source: serde_json::from_str::<serde_json::Value>("x").unwrap_err(),
        });
        assert!(matches!(parse_err, NavLlmError::ProposalParse(_)));

        let bad_resp = map_runtime_error(RtLlmError::BadResponse("empty choices".into()));
        assert!(matches!(bad_resp, NavLlmError::ProposalParse(_)));
    }
}
