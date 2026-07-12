//! PR E2 — Canonical resolution preview text renderer — body-only (UI state yok).
//!
//! Tek renderer, üç yüzey tarafından çağrılır (divergence sıfır):
//!   - `osp review resolve-code-entity-preview` standalone query
//!   - one-shot `osp review resolve-code-entity` TTY confirmation
//!   - interactive wizard `resolve` confirmation
//!
//! Renderer input okumaz, confirmation/reason prompt'u içermez — adapter'larda kalır.

use std::io::{self, Write};

use crate::application::review::{
    ResolutionPreviewOutput, ResolutionTargetPreview,
};

/// Resolution preview'ı text olarak render et (body-only — confirmation/reason prompt YOK).
pub fn render_resolve_code_entity_preview_text<W: Write>(
    output: &mut W,
    preview: &ResolutionPreviewOutput,
) -> io::Result<()> {
    writeln!(output, "Candidate: {}", preview.candidate.id)?;
    writeln!(output, "  Canonical: {}", preview.candidate.canonical)?;
    writeln!(output, "  Kind: {}", preview.candidate.kind)?;
    writeln!(output, "  Status: {}", preview.candidate.status)?;
    writeln!(output, "  Family: {}", preview.candidate.family)?;
    writeln!(output, "  Digest: {}", preview.candidate.digest_hex)?;
    writeln!(output)?;
    writeln!(output, "Identity key:")?;
    writeln!(output, "  scheme: {}", preview.identity_key.scheme)?;
    writeln!(output, "  policy: {}", preview.identity_key.case_policy)?;
    writeln!(output, "  key:    {}", preview.identity_key.canonical_key)?;
    writeln!(output)?;
    writeln!(output, "Resolution target:")?;
    match &preview.target {
        ResolutionTargetPreview::Create {
            proposed_entity_id,
        } => {
            writeln!(output, "  outcome:           create")?;
            writeln!(output, "  proposed_entity:   {proposed_entity_id}")?;
        }
        ResolutionTargetPreview::Reuse {
            entity_id,
            entity_digest_hex,
            entity_status,
        } => {
            writeln!(output, "  outcome:         reuse")?;
            writeln!(output, "  entity:          {entity_id}")?;
            writeln!(output, "  entity digest:   {entity_digest_hex}")?;
            writeln!(output, "  entity status:   {entity_status}")?;
        }
    }
    writeln!(output)?;
    writeln!(output, "  Revision: {}", preview.revision)?;
    Ok(())
}
