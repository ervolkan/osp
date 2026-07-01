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
     Vision: x<=0.30, y>=0.70, z<=0.50.\n\n\
     Output format — DeltaProposal JSON (G2c-4: removed_edges + affected_nodes):"
}

/// **G2c-4 (arkadaş review 10 #2):** Ortak DeltaProposal output format snippet.
/// `osp_system_prompt` + `trajectory_system_prompt` ikisi de bunu kullanır —
/// prompt debt önlenir (iki kopya ayrı güncellenmez).
///
/// `removed_edges` (subtractive delta, G2c-2) + `affected_nodes` (ölçüm scope)
/// içerir. LLM bu şemayı gördüğünde coupling düşürmek için edge kaldırmayı öğrenebilir.
pub fn delta_proposal_output_format_snippet() -> &'static str {
    r#"{
  "new_nodes": [],
  "new_edges": [],
  "removed_edges": [
    {"from": 0, "to": 1, "kind": "Imports"}
  ],
  "affected_nodes": [0],
  "modified_entities": [],
  "position_hints": [],
  "reasoning": "explain your structural changes"
}

RULES:
- removed_edges: remove existing edges (e.g. to reduce coupling, remove outgoing Imports).
  Each entry: {"from": <node_id>, "to": <node_id>, "kind": "Imports"}.
- affected_nodes: node IDs whose position should be re-measured after the change.
  For coupling reduction, list the node whose imports you removed (the "from" node).
- new_nodes / new_edges: add new structural elements (abstractions, modules).
- Do NOT declare positions — the engine measures them."#
}

/// User message: the serialized `OspPrompt` packet + the produce instruction.
///
/// The packet is pretty-printed JSON so the model can read coordinates at a
/// glance; this is also what we measure token cost against.
pub fn osp_user_prompt(prompt: &OspPrompt) -> String {
    let json =
        serde_json::to_string_pretty(prompt).unwrap_or_else(|_| "<serialize failed>".to_string());
    format!(
        "OspPrompt:\n{json}\n\nProduce a DeltaProposal for this intent.\n\nOutput format:\n{}",
        delta_proposal_output_format_snippet()
    )
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
