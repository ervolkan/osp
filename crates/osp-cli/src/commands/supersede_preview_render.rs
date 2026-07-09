//! Canonical supersede preview text renderer — body-only (UI state yok).
//!
//! Tek renderer, üç yüzey tarafından çağrılır (divergence sıfır):
//!   - `osp review supersede-preview` standalone query
//!   - one-shot `osp review supersede` TTY confirmation
//!   - interactive wizard `supersede` confirmation
//!
//! Renderer input okumaz, confirmation/reason prompt'u içermez — adapter'larda kalır.
//! `eligible: false` → blockers gösterir; confirmation yüzeyleri prompt göstermeden döner.

use std::io::{self, Write};

use crate::application::review::{
    SupersedeBlockerCode, SupersedePreviewOutput, SupersedeEndpointPreview,
};

impl SupersedeBlockerCode {
    /// İnsan-okunabilir blocker başlığı.
    fn label(self) -> &'static str {
        match self {
            Self::AlreadySuperseded => "Already superseded",
            Self::SupersededNotCurrent => "Superseded endpoint is not current",
            Self::SuccessorNotCurrent => "Successor endpoint is not current",
            Self::SelfSupersede => "Self supersede",
            Self::IncompatibleKind => "Incompatible kind",
            Self::IncompatibleFamily => "Incompatible family",
            Self::Cycle => "Supersede cycle",
        }
    }
}

/// Endpoint detay satırlarını yaz (renderer ortak parça).
fn write_endpoint<W: Write>(out: &mut W, label: &str, ep: &SupersedeEndpointPreview) -> io::Result<()> {
    writeln!(out, "  {label}:")?;
    writeln!(out, "    ID: {}", ep.id)?;
    writeln!(out, "    Status: {}", ep.status)?;
    writeln!(out, "    Kind: {}", ep.kind)?;
    writeln!(out, "    Family: {}", ep.family)?;
    writeln!(out, "    Digest: {}", ep.node_digest_hex)?;
    Ok(())
}

/// Canonical preview body renderer — üç yüzey aynı çıktıyı alır.
///
/// Input okumaz, UI state taşımaz. Confirmation yüzeyleri bunu çağırır, ardından
/// kendi reason/confirm akışlarını yürütür. `structurally_eligible: false` ise
/// blockers gösterilir.
pub(crate) fn render_supersede_preview_text<W: Write>(
    output: &mut W,
    preview: &SupersedePreviewOutput,
) -> io::Result<()> {
    writeln!(output, "Supersession preview (revision {})", preview.revision)?;
    writeln!(output)?;

    write_endpoint(output, "Superseded endpoint", &preview.superseded)?;
    write_endpoint(output, "Successor endpoint", &preview.successor)?;
    writeln!(output)?;

    // Proposed edge (yön-açık).
    writeln!(
        output,
        "Proposed committed edge:  {} --{}--> {}",
        preview.proposed_edge.from, preview.proposed_edge.kind, preview.proposed_edge.to
    )?;
    // State transition sonuçları yalnız structurally eligible durumda gösterilir.
    // ineligible'da gerçekleşmeyecek geçişi kesin sonuç gibi gösterme (Review P1).
    if preview.structurally_eligible {
        writeln!(output, "If successfully committed:")?;
        writeln!(
            output,
            "  {} → SupersededAccepted (retains provenance, no longer current)",
            preview.superseded.id
        )?;
        writeln!(
            output,
            "  {} remains current Accepted",
            preview.successor.id
        )?;
    } else {
        writeln!(
            output,
            "Requested transition is currently blocked; no state change will be applied."
        )?;
    }
    writeln!(output)?;

    // Lineage (successor outgoing committed DAG + superseded incoming).
    // Edge-list gösterimi — consolidation'da branching korunur (chain.join sahte chain üretmez).
    if preview.lineage.edges.is_empty() {
        writeln!(
            output,
            "Lineage:  {} has no committed supersession history",
            preview.lineage.root
        )?;
    } else {
        writeln!(
            output,
            "Lineage (successor outgoing committed DAG):"
        )?;
        // Sade chain tespiti: her node ≤1 outgoing ve ≤1 incoming → compact chain göster.
        use std::collections::BTreeMap;
        let mut out_count: BTreeMap<&str, usize> = BTreeMap::new();
        for e in &preview.lineage.edges {
            *out_count.entry(e.from.as_str()).or_default() += 1;
        }
        let is_simple_chain = preview
            .lineage
            .edges
            .iter()
            .all(|e| *out_count.get(e.from.as_str()).unwrap_or(&0) == 1)
            && preview.lineage.edges.len() + 1 == preview.lineage.nodes.len();
        if is_simple_chain {
            // Compact chain: root → ... → leaf (edges sıralı).
            let mut chain: Vec<String> = Vec::new();
            // root = from olan ama to olmayan node (veya ilk edge'in from'u).
            let tos: std::collections::BTreeSet<&str> =
                preview.lineage.edges.iter().map(|e| e.to.as_str()).collect();
            let root = preview
                .lineage
                .nodes
                .iter()
                .find(|n| !tos.contains(n.id.as_str()))
                .map(|n| n.id.clone())
                .unwrap_or_else(|| preview.lineage.root.clone());
            let mut adj: BTreeMap<String, String> = BTreeMap::new();
            for e in &preview.lineage.edges {
                adj.insert(e.from.clone(), e.to.clone());
            }
            let mut cur = root;
            chain.push(cur.clone());
            while let Some(next) = adj.get(&cur) {
                chain.push(next.clone());
                cur = next.clone();
            }
            writeln!(output, "  {}", chain.join(" → "))?;
        } else {
            // DAG — edge-list (branching korunur).
            for e in &preview.lineage.edges {
                writeln!(
                    output,
                    "  {} --Supersedes--> {}",
                    e.from, e.to
                )?;
            }
        }
        if let Some(t) = preview.lineage.truncation {
            writeln!(output, "  (truncated: {:?} — DAG may be larger)", t)?;
        }
    }
    if let Some(incoming) = &preview.lineage.superseded_incoming {
        writeln!(
            output,
            "  Superseded already has incoming from: {}",
            incoming
        )?;
    }
    writeln!(output)?;

    // Compatibility + cycle.
    writeln!(output, "Compatibility:")?;
    writeln!(
        output,
    "  Kind compatible: {}    Family compatible: {}    Cycle risk: {}",
        preview.compatibility.kind_compatible,
        preview.compatibility.family_compatible,
        preview.compatibility.cycle_risk
    )?;
    writeln!(output)?;

    // Structural eligibility + blockers.
    writeln!(
        output,
        "Structurally eligible at revision {}: {}",
        preview.revision,
        if preview.structurally_eligible { "yes" } else { "no" }
    )?;
    if let Some(primary) = preview.primary_structural_blocker {
        writeln!(output, "Primary structural blocker:")?;
        let primary_blocker = preview
            .blocking_reasons
            .iter()
            .find(|b| b.code == primary)
            .expect("primary_structural_blocker has matching entry");
        writeln!(output, "  {} — {}", primary.label(), primary_blocker.message)?;
        let additional: Vec<_> = preview
            .blocking_reasons
            .iter()
            .filter(|b| b.code != primary)
            .collect();
        if !additional.is_empty() {
            writeln!(output, "Additional blockers:")?;
            for b in additional {
                writeln!(output, "  {} — {}", b.code.label(), b.message)?;
            }
        }
    }
    writeln!(output)?;

    // Freshness link + operator notes.
    if preview.structurally_eligible {
        writeln!(
            output,
            "Freshness tokens (pass to supersede as --superseded-digest/--successor-digest):"
        )?;
        writeln!(
            output,
            "  --superseded-digest {}    --successor-digest {}",
            preview.superseded.node_digest_hex, preview.successor.node_digest_hex
        )?;
        writeln!(output)?;
    }
    writeln!(
        output,
    "Operator note: this is a read-only point-in-time assessment."
    )?;
    writeln!(
        output,
    "  Mutation revalidates both digests and currentness under lock."
    )?;
    // Self-supersede operatör notu.
    if preview
        .blocking_reasons
        .iter()
        .any(|b| b.code == SupersedeBlockerCode::SelfSupersede)
    {
        writeln!(output, "  Both endpoint IDs are identical.")?;
    }

    Ok(())
}
