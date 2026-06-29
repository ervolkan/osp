//! Prompt serialization — `OspPrompt` -> chat message content.
//!
//! Mirrors the prompt format used by `scripts/llm-token-bench.ps1` so the
//! crate is a drop-in replacement for the benchmark. The OSP system prompt
//! instructs the model to emit a `DeltaProposal` JSON; the user prompt embeds
//! the serialized `OspPrompt` packet.

use osp_core::agent::OspPrompt;

/// System message: OSP agent contract (output must be a DeltaProposal).
///
/// Kept in sync with the PowerShell benchmark system prompt (§7.8 baseline).
pub fn osp_system_prompt() -> &'static str {
    "You are an OSP (Ontological Space Protocol) agent. You receive a typed \
     epistemic projection packet containing module coordinates in a \
     5-dimensional architectural space. Respond with a DeltaProposal JSON \
     describing structural changes only (no positions — the engine computes \
     those).\n\n\
     Coordinate axes: x=coupling, y=cohesion, z=instability, w=entropy, \
     v=witness-depth.\n\
     Vision: x<=0.30, y>=0.70, z<=0.50.\n\
     Output format: JSON with fields: new_nodes, new_edges, \
     modified_entities, reasoning."
}

/// User message: the serialized `OspPrompt` packet + the produce instruction.
///
/// The packet is pretty-printed JSON so the model can read coordinates at a
/// glance; this is also what we measure token cost against.
pub fn osp_user_prompt(prompt: &OspPrompt) -> String {
    let json =
        serde_json::to_string_pretty(prompt).unwrap_or_else(|_| "<serialize failed>".to_string());
    format!("OspPrompt:\n{json}\n\nProduce a DeltaProposal for this intent.")
}

/// System message for the raw source-dump baseline (RQ5 comparison only).
pub fn raw_system_prompt() -> &'static str {
    "You are a coding assistant. The user will show you source files and ask \
     you to add a feature."
}

/// User message: raw 2-hop source dump (RQ5 baseline comparison).
///
/// Caller passes pre-collected source snippets; this only assembles them into
/// the same prompt shape used by the PowerShell benchmark so token counts are
/// directly comparable.
pub fn raw_dump_user_prompt(snippets: &[(&str, &str)], task: &str) -> String {
    let mut out = String::from("Here are source files from the project:\n\n");
    for (name, body) in snippets {
        out.push_str(&format!("=== {name} ===\n{body}\n\n"));
    }
    out.push_str(&format!("Task: {task}"));
    out
}
