//! Runtime: stateless HTTP client wrapping an OpenAI-compatible endpoint.

use std::time::Duration;

use osp_core::agent::{DeltaProposal, OspPrompt};
use serde::Serialize;

use crate::error::LlmError;
use crate::prompt::{osp_system_prompt, osp_user_prompt};
use crate::response::{parse_raw, RawCompletion, TokenUsage};

/// Endpoint + model + auth configuration. Cloneable for shared use.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// OpenAI-compatible chat-completion URL.
    pub endpoint: String,
    /// Model id, e.g. `"gpt-4o-mini"`.
    pub model: String,
    /// Bearer token. Required (non-empty) before any call.
    pub api_key: String,
    /// Per-request timeout (HTTP connect + read).
    pub timeout: Duration,
    /// Sampling temperature.
    pub temperature: f32,
    /// Max completion tokens.
    pub max_tokens: u32,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_key: String::new(),
            timeout: Duration::from_secs(60),
            temperature: 0.3,
            max_tokens: 500,
        }
    }
}

impl RuntimeConfig {
    /// Read the API key from the `OPENAI_API_KEY` env var.
    pub fn with_env_api_key(mut self) -> Result<Self, LlmError> {
        let key = std::env::var("OPENAI_API_KEY").map_err(|_| LlmError::MissingApiKey)?;
        if key.trim().is_empty() {
            return Err(LlmError::MissingApiKey);
        }
        self.api_key = key;
        Ok(self)
    }
}

/// One chat message in the request payload.
#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// The subset of the chat-completion request body we serialize.
#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    max_tokens: u32,
    temperature: f32,
}

/// A pre-built pair of (system, user) messages — used so callers can measure
/// input size before/after the call without re-serializing.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system: String,
    pub user: String,
}

impl CompletionRequest {
    /// Build the standard OSP request: system prompt + serialized `OspPrompt`.
    pub fn osp(prompt: &OspPrompt) -> Self {
        Self {
            system: osp_system_prompt().to_string(),
            user: osp_user_prompt(prompt),
        }
    }

    /// Total input size in characters (for RQ5 prompt-size reporting).
    pub fn input_chars(&self) -> usize {
        self.system.len() + self.user.len()
    }
}

/// Stateless OpenAI-compatible runtime (inv #11 — no agent state held).
#[derive(Debug, Clone)]
pub struct Runtime {
    config: RuntimeConfig,
    client: reqwest::blocking::Client,
}

impl Runtime {
    /// Construct from config. Errors only if the HTTP client cannot be built.
    pub fn new(config: RuntimeConfig) -> Result<Self, LlmError> {
        if config.api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey);
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()?;
        Ok(Self { config, client })
    }

    /// Convenience: default config + `OPENAI_API_KEY` from env.
    pub fn from_env() -> Result<Self, LlmError> {
        Self::new(RuntimeConfig::default().with_env_api_key()?)
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Run one OSP-prompt completion and parse the result into a `DeltaProposal`.
    ///
    /// Use [`complete_raw`] when you only need token counts or want to inspect
    /// the assistant text before parsing — this method fails if the model
    /// returns a schema-violating response.
    pub fn complete(&self, prompt: &OspPrompt) -> Result<(DeltaProposal, TokenUsage), LlmError> {
        let req = CompletionRequest::osp(prompt);
        let raw = self.complete_raw(&req)?;
        raw.into_proposal()
            .map_err(|(raw, source)| LlmError::ProposalParse { raw, source })
    }

    /// Lower-level entrypoint: run an arbitrary (system, user) pair and return
    /// the raw assistant text + token usage. Never fails on proposal-shape
    /// grounds — used by the benchmark to compare OSP vs raw source-dump
    /// prompts with identical HTTP plumbing.
    pub fn complete_raw(&self, req: &CompletionRequest) -> Result<RawCompletion, LlmError> {
        let body = ChatRequest {
            model: &self.config.model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: &req.system,
                },
                ChatMessage {
                    role: "user",
                    content: &req.user,
                },
            ],
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };
        let serialized = serde_json::to_vec(&body)
            .map_err(|e| LlmError::BadResponse(format!("request serialize: {e}")))?;

        tracing::debug!(
            model = %self.config.model,
            input_chars = req.input_chars(),
            "sending completion request"
        );

        let resp = self
            .client
            .post(&self.config.endpoint)
            .bearer_auth(&self.config.api_key)
            .header("Content-Type", "application/json")
            .body(serialized)
            .send()?;

        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            return Err(LlmError::Status {
                code: status.as_u16(),
                body: text,
            });
        }
        parse_raw(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osp_core::agent::{OspPrompt, OutputContract};
    use osp_core::space::TimeLayer;
    use osp_core::vision::VisionVector;

    #[test]
    fn completion_request_osp_serializes_prompt() {
        let prompt = OspPrompt {
            vision: VisionVector::default(),
            time_ref: TimeLayer::default(),
            permissions: Default::default(),
            output_contract: OutputContract::default(),
        };
        let req = CompletionRequest::osp(&prompt);
        assert!(req.user.contains("OspPrompt:"));
        assert!(req.user.contains("\"vision\""));
        assert!(req.input_chars() > 0);
    }

    #[test]
    fn runtime_new_rejects_empty_api_key() {
        let cfg = RuntimeConfig::default(); // empty key
        assert!(matches!(Runtime::new(cfg), Err(LlmError::MissingApiKey)));
    }

    #[test]
    fn config_with_env_api_key_missing_errors() {
        // Ensure the var is absent for this test.
        std::env::remove_var("OPENAI_API_KEY");
        assert!(matches!(
            RuntimeConfig::default().with_env_api_key(),
            Err(LlmError::MissingApiKey)
        ));
    }
}
