//! OSP LLM Runtime — `OspPrompt` -> `DeltaProposal` via an OpenAI-compatible
//! chat-completion API.
//!
//! Faz 5 #2: Rust crate replacement for `scripts/llm-token-bench.ps1`.
//!
//! ## Design (inv #11 — LLM is stateless)
//!
//! The runtime holds NO agent state. Each [`Runtime::complete`] call is a
//! standalone HTTP request: it serializes an [`OspPrompt`] into a chat
//! message, calls the configured endpoint, and deserializes the assistant
//! response into a [`DeltaProposal`]. All agent state lives in the agent
//! shell (engine side), not here.
//!
//! ## Token accounting
//!
//! [`Completion::usage`] carries real tokenizer counts (`prompt_tokens`,
//! `completion_tokens`, `total_tokens`) reported by the API — used for the
//! RQ5 token-benchmark (§7.8).

use osp_core::agent::{DeltaProposal, OspPrompt};

mod adapter;
mod error;
mod prompt;
mod response;
mod runtime;

pub use adapter::RuntimeLlmClient;
pub use error::LlmError;
pub use prompt::{osp_system_prompt, osp_user_prompt, raw_dump_user_prompt, raw_system_prompt};
pub use response::{RawCompletion, TokenUsage};
pub use runtime::{CompletionRequest, Runtime, RuntimeConfig};

/// Run one OSP-prompt completion and return the parsed proposal + token usage.
///
/// Convenience wrapper for the common case: serialize the prompt, call the
/// endpoint, parse the assistant message as a `DeltaProposal`. If the model
/// returns non-JSON or a schema-violating response, [`LlmError::ProposalParse`]
/// is returned with the raw text attached for diagnostics.
pub fn complete(
    runtime: &Runtime,
    prompt: &OspPrompt,
) -> Result<(DeltaProposal, TokenUsage), LlmError> {
    runtime.complete(prompt)
}
