//! Interactive review wizard — minimal operator session (stdio, yeni dependency yok).
//!
//! Argümansız `osp review` → operator oturumu açılır. Generic `R: BufRead, W: Write`
//! (Review 1#14) — production `stdin/stdout`, test `Cursor/Vec`.
//!
//! Komutlar: `list`, `show <id>`, `accept <id>`, `reject <id>`, `supersede <old> <new>`, `quit`.
//!
//! Her mutation `ReviewApplicationService` üzerinden gider (one-shot ile aynı service —
//! iki davranış oluşmaz): accept/reject `execute_mutation`, supersede `execute_supersede`.
//! Gösterilen basis/digest taşınır — reload sonrası yeni compile ETME; operator'ın gördüğü
//! ile karar.
//!
//! # Informed-acceptance sırası (Review P1.2)
//! Operator, basis'i GÖRDÜKTEN sonra karar verir:
//! ```text
//! accept <id>
//! → basis ve digest göster
//! → [y/N] confirmation (exact basis)
//! → reason al
//! → aynı digest ile persistent mutation
//! ```
//! Reason basis'ten önce sorulmaz — aksi halde operator görmemiş basis'e gerekçe yazmış olur.
//!
//! v1 minimal: list/show/accept/reject/supersede/quit. v2: dialoguer/rustyline, fuzzy, renk.

use std::io::{BufRead, Write};

use osp_core::anchoring::review::{NodeDigest, OperatorId};
use osp_core::anchoring::types::ConceptNodeId;

use crate::application::repository::FileReviewStore;
use crate::application::review::{ReviewMutationCommand, ReviewQuery};
use crate::application::{ReviewApplicationService, ReviewReadOutput};
use crate::errors::ReviewError;

/// Interactive review session args.
#[derive(clap::Args, Debug)]
pub struct ReviewSessionArgs {
    #[arg(long, default_value = ".osp/anchor-store.json")]
    pub store: std::path::PathBuf,
    /// Operator kimliği. Yoksa prompt açılır (interactive'te). `$OSP_OPERATOR` fallback.
    #[arg(long)]
    pub operator: Option<String>,
}

impl Default for ReviewSessionArgs {
    /// clap `default_value` attribute `#[derive(Default)]` tarafından görülmez;
    /// elle set edilmeli — aksi halde boş path üretilir.
    fn default() -> Self {
        Self {
            store: std::path::PathBuf::from(".osp/anchor-store.json"),
            operator: None,
        }
    }
}

/// Interactive wizard handler — production stdin/stdout.
pub fn run_review_session(args: ReviewSessionArgs) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();
    // Operator kimliği: flag > env > prompt (interactive'te sor).
    let operator = resolve_operator(args.operator.clone(), &mut stdin, &mut stdout)?;
    run_interactive(
        &mut stdin,
        &mut stdout,
        &args.store,
        OperatorId::new(operator),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Operator kimliği: --operator > $OSP_OPERATOR > prompt (interactive'te sor).
/// One-shot'tan farklı: interactive'te prompt açılır, fail etmez.
/// Boş/whitespace flag/env reject (Review 2.tur P2.3).
fn resolve_operator<R: BufRead, W: Write>(
    flag: Option<String>,
    input: &mut R,
    output: &mut W,
) -> Result<String, anyhow::Error> {
    if let Some(op) = flag {
        return normalize_operator(&op);
    }
    if let Ok(env_op) = std::env::var("OSP_OPERATOR") {
        return normalize_operator(&env_op);
    }
    // Interactive prompt.
    write!(output, "Operator identity: ")?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        anyhow::bail!("no operator identity provided (EOF)");
    }
    normalize_operator(&line)
}

/// Operator değerini normalize: trim + boş reject. Flag/env/prompt için ortak.
fn normalize_operator(value: &str) -> Result<String, anyhow::Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("operator identity cannot be empty");
    }
    Ok(trimmed.to_owned())
}

/// Generic interactive loop. `R`/`W` test edilebilir I/O (Review 1#14).
pub fn run_interactive<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    store_path: &std::path::Path,
    operator: OperatorId,
) -> Result<(), ReviewError> {
    let repo = FileReviewStore::new(store_path);
    let service = ReviewApplicationService::new(repo);

    writeln!(
        output,
        "OSP review session — operator: {}",
        operator.as_str()
    )
    .ok();
    writeln!(
        output,
        "Commands: list, show <id>, accept <id>, reject <id>, supersede <old> <new>, resolve <candidate>, quit"
    )
    .ok();
    writeln!(output).ok();

    loop {
        // Her döngüde candidate sayısını göster.
        match service.execute_query(ReviewQuery::List) {
            Ok(ReviewReadOutput::List { items, revision: _ }) => {
                writeln!(output, "{} candidates awaiting review.", items.len()).ok();
            }
            Ok(_) => {}
            Err(e) => {
                writeln!(output, "✗ cannot list candidates: {e}").ok();
            }
        }
        write!(output, "> ").ok();
        output.flush().ok();

        let mut line = String::new();
        if input.read_line(&mut line).unwrap_or(0) == 0 {
            break; // EOF
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("");

        match cmd {
            "quit" | "q" | "exit" => {
                writeln!(output, "Session ended.").ok();
                break;
            }
            "list" | "l" => match service.execute_query(ReviewQuery::List) {
                Ok(ReviewReadOutput::List { items, revision }) => {
                    for item in &items {
                        writeln!(output, "  {}  {}  [{}]", item.id, item.canonical, item.kind).ok();
                    }
                    writeln!(output, "  Revision: {revision}").ok();
                }
                _ => {}
            },
            "show" | "s" => {
                let id = match parts.next() {
                    Some(id) => id,
                    None => {
                        writeln!(output, "Usage: show <id>").ok();
                        continue;
                    }
                };
                show_node(&service, id, output);
            }
            "accept" | "a" | "reject" | "r" => {
                let accept = cmd == "accept" || cmd == "a";
                let id = match parts.next() {
                    Some(id) => id.to_string(),
                    None => {
                        writeln!(output, "Usage: {} <id>", cmd).ok();
                        continue;
                    }
                };
                // P1.2 informed-acceptance: basis göster → confirm → reason → mutation.
                run_informed_mutation(&service, input, output, &id, accept, &operator);
            }
            "supersede" | "sup" => {
                let old = match parts.next() {
                    Some(id) => id.to_string(),
                    None => {
                        writeln!(output, "Usage: supersede <old> <new>").ok();
                        continue;
                    }
                };
                let new = match parts.next() {
                    Some(id) => id.to_string(),
                    None => {
                        writeln!(output, "Usage: supersede <old> <new>").ok();
                        continue;
                    }
                };
                if let Err(e) = run_informed_supersede(&service, input, output, &old, &new, &operator) {
                    writeln!(output, "✗ preview render failed: {e}").ok();
                }
            }
            "resolve" | "res" => {
                // PR E2 (tur 3 P1-5) — resolve <candidate> (reason YOK; preview → confirm → reason).
                let candidate = match parts.next() {
                    Some(id) => id.to_string(),
                    None => {
                        writeln!(output, "Usage: resolve <candidate>").ok();
                        continue;
                    }
                };
                if let Err(e) =
                    run_informed_resolution(&service, input, output, &candidate, &operator)
                {
                    writeln!(output, "✗ preview render failed: {e}").ok();
                }
            }
            other => {
                writeln!(
                    output,
                    "✗ unknown command: {other} (list/show/accept/reject/supersede/resolve/quit)"
                )
                .ok();
            }
        }
        writeln!(output).ok();
    }
    Ok(())
}

/// Node detayını göster (show komutu).
fn show_node<W: Write>(
    service: &ReviewApplicationService<FileReviewStore>,
    id: &str,
    output: &mut W,
) {
    match service.execute_query(ReviewQuery::Show(ConceptNodeId(id.into()))) {
        Ok(ReviewReadOutput::Show { node: Some(n), .. }) => {
            writeln!(
                output,
                "  {} — {} [{}] status={}",
                n.id, n.canonical, n.kind, n.decision_status
            )
            .ok();
            writeln!(output, "  digest: {}", n.node_digest_hex).ok();
        }
        Ok(_) => {
            writeln!(output, "✗ node not found: {id}").ok();
        }
        Err(e) => {
            writeln!(output, "✗ {e}").ok();
        }
    }
}

/// Informed-acceptance akışı (Review P1.2):
/// 1. basis ve digest göster (lock öncesi show)
/// 2. [y/N] confirmation (exact basis)
/// 3. reason al
/// 4. aynı digest ile persistent mutation
///
/// Operator'ın gördüğü basis ile karar anındaki aynı olmalı (expected_basis_digest).
fn run_informed_mutation<R: BufRead, W: Write>(
    service: &ReviewApplicationService<FileReviewStore>,
    input: &mut R,
    output: &mut W,
    id: &str,
    accept: bool,
    operator: &OperatorId,
) {
    // (1) Basis göster + digest al (Candidate değilse abort).
    let (digest, canonical) = match get_basis_for(service, id, output) {
        Some(x) => x,
        None => return,
    };

    // (2) Confirmation: operator exact basis'i gördü ve onaylıyor mu?
    write!(
        output,
        "  {} this exact basis? [y/N] ",
        if accept { "Accept" } else { "Reject" }
    )
    .ok();
    output.flush().ok();
    let mut confirm = String::new();
    if input.read_line(&mut confirm).unwrap_or(0) == 0 {
        return;
    }
    let confirm = confirm.trim().to_lowercase();
    if confirm != "y" && confirm != "yes" {
        writeln!(output, "  aborted by operator").ok();
        return;
    }

    // (3) Reason — confirmation sonrası (basis görmüş operator gerekçe yazıyor).
    write!(output, "  Reason: ").ok();
    output.flush().ok();
    let mut reason = String::new();
    if input.read_line(&mut reason).unwrap_or(0) == 0 {
        return;
    }
    let reason = reason.trim().to_string();
    if reason.is_empty() {
        writeln!(output, "✗ reason cannot be empty").ok();
        return;
    }

    // (4) Mutation — operator'ın gördüğü digest ile (lock altında precondition).
    let command = if accept {
        ReviewMutationCommand::Accept {
            id: ConceptNodeId(id.to_string()),
            expected_basis_digest: digest,
            reason,
        }
    } else {
        ReviewMutationCommand::Reject {
            id: ConceptNodeId(id.to_string()),
            expected_basis_digest: digest,
            reason,
        }
    };
    match service.execute_mutation(command, operator.clone()) {
        Ok(out) => {
            writeln!(
                output,
                "✓ {} {} (record #{}, revision {})",
                if accept { "Accepted" } else { "Rejected" },
                out.mutation.node_id,
                out.mutation.decision_sequence,
                out.revision
            )
            .ok();
        }
        Err(ReviewError::StaleBasis) => {
            writeln!(output, "✗ stale basis: node changed since you viewed it").ok();
        }
        Err(e) => {
            writeln!(output, "✗ {e}").ok();
        }
    }
    let _ = canonical; // canonical gösterildi; unused uyarısını bastır.
}

/// Interactive informed-supersede: rich preview göster → confirm → reason → mutation.
/// Non-current endpoint'ler hard error değil blocker-bearing preview (structurally_eligible=false →
/// prompt yok). Informed I/O zinciri fail-closed: render + confirmation/reason prompt write/flush
/// `?` ile yayılır (hata yutulmaz → mutation yürümez).
fn run_informed_supersede<R: BufRead, W: Write>(
    service: &ReviewApplicationService<FileReviewStore>,
    input: &mut R,
    output: &mut W,
    old: &str,
    new: &str,
    operator: &OperatorId,
) -> std::io::Result<()> {
    use crate::commands::supersede_preview_render::render_supersede_preview_text;
    use crate::errors::SupersedeCommand;

    // (1) Rich preview (tek canonical model — standalone query ile aynı).
    let preview = match service
        .execute_supersede_preview(ConceptNodeId(old.into()), ConceptNodeId(new.into()))
    {
        Ok(p) => p,
        Err(e) => {
            writeln!(output, "✗ {e}").ok();
            return Ok(());
        }
    };

    // (2) Canonical renderer (body only) — standalone ile aynı çıktı.
    // Render hatası informed-basis garantisini kırar (preview eksik → confirmation/mutation
    // devam edemez) — .ok() ile yutma, ? ile yay (Review P1-c).
    render_supersede_preview_text(output, &preview)?;

    // ineligible → confirmation prompt yok, session'a dön.
    if !preview.structurally_eligible {
        return Ok(());
    }

    write!(output, "  Apply this exact supersession? [y/N] ")?;
    output.flush()?;
    let mut confirm = String::new();
    if input.read_line(&mut confirm).unwrap_or(0) == 0 {
        return Ok(());
    }
    if confirm.trim().to_lowercase() != "y" && confirm.trim().to_lowercase() != "yes" {
        writeln!(output, "  aborted by operator").ok();
        return Ok(());
    }

    // (3) Reason.
    write!(output, "  Reason: ")?;
    output.flush()?;
    let mut reason = String::new();
    if input.read_line(&mut reason).unwrap_or(0) == 0 {
        return Ok(());
    }
    let reason = reason.trim().to_string();
    if reason.is_empty() {
        writeln!(output, "✗ reason cannot be empty").ok();
        return Ok(());
    }

    // (4) Mutation — gösterilen preview'ın digest'leri ile (lock altında recheck).
    let command = SupersedeCommand {
        superseded: ConceptNodeId(old.into()),
        successor: ConceptNodeId(new.into()),
        expected: preview.digests(),
        reason,
    };
    match service.execute_supersede(command, operator.clone()) {
        Ok(out) => {
            writeln!(
                output,
                "✓ {} supersedes {} (record #{}, revision {})",
                out.mutation.successor_node_id,
                out.mutation.superseded_node_id,
                out.mutation.decision_sequence,
                out.revision
            )
            .ok();
        }
        Err(crate::errors::ReviewError::StaleSupersededBasis) => {
            writeln!(
                output,
                "✗ superseded endpoint changed since you viewed it; review both again"
            )
            .ok();
        }
        Err(crate::errors::ReviewError::StaleSuccessorBasis) => {
            writeln!(
                output,
                "✗ successor endpoint changed since you viewed it; review both again"
            )
            .ok();
        }
        Err(e) => {
            writeln!(output, "✗ {e}").ok();
        }
    }
    Ok(())
}

/// PR E2 (tur 3 P1-5) — Interactive informed-resolution: minimal preview → confirm → reason → mutation.
///
/// Wizard informed-review sözleşmesi: operator görmediği basis'e gerekçe yazmasın.
/// Akış: `resolve <candidate>` → preview (target reveal) → `[y/N]` → reason prompt → mutation
/// (preview'dan çıkarılan digest + expected_target ile; per-command session — tur 1 karar #3).
///
/// Informed I/O zinciri fail-closed: render + confirmation/reason prompt write/flush `?` ile yayılır.
fn run_informed_resolution<R: BufRead, W: Write>(
    service: &ReviewApplicationService<FileReviewStore>,
    input: &mut R,
    output: &mut W,
    candidate: &str,
    operator: &OperatorId,
) -> std::io::Result<()> {
    use crate::commands::resolve_code_entity_preview_render::render_resolve_code_entity_preview_text;
    use crate::errors::{ExpectedResolutionTarget, ResolveCodeEntityCommand};

    // (1) Minimal canonical preview (tek canonical model — standalone query ile aynı).
    let preview = match service
        .execute_resolve_code_entity_preview(ConceptNodeId(candidate.into()))
    {
        Ok(p) => p,
        Err(e) => {
            writeln!(output, "✗ {e}").ok();
            return Ok(());
        }
    };

    // (2) Canonical renderer (body only) — standalone ile aynı çıktı.
    render_resolve_code_entity_preview_text(output, &preview)?;

    // (3) Confirmation — tam target gösterildi, operator pins.
    write!(output, "  Resolve this exact candidate and target basis? [y/N] ")?;
    output.flush()?;
    let mut confirm = String::new();
    if input.read_line(&mut confirm).unwrap_or(0) == 0 {
        return Ok(());
    }
    if confirm.trim().to_lowercase() != "y" && confirm.trim().to_lowercase() != "yes" {
        writeln!(output, "  aborted by operator").ok();
        return Ok(());
    }

    // (4) Reason — confirmation SONRASI (operator görmediği basis'e gerekçe yazmasın).
    write!(output, "  Reason: ")?;
    output.flush()?;
    let mut reason = String::new();
    if input.read_line(&mut reason).unwrap_or(0) == 0 {
        return Ok(());
    }
    let reason = reason.trim().to_string();
    if reason.is_empty() {
        writeln!(output, "✗ reason cannot be empty").ok();
        return Ok(());
    }

    // (5) Mutation — preview'dan çıkarılan digest + expected_target (lock altında recheck).
    let candidate_digest = preview.candidate_digest();
    let expected_target: ExpectedResolutionTarget = preview.expected_target();
    let command = ResolveCodeEntityCommand {
        candidate: ConceptNodeId(candidate.into()),
        expected_candidate_digest: candidate_digest,
        expected_target,
        reason,
    };
    match service.execute_resolve_code_entity(command, operator.clone()) {
        Ok(out) => {
            writeln!(
                output,
                "✓ resolved {} → {} ({}, record #{}, revision {})",
                out.mutation.candidate_node_id,
                out.mutation.entity_node_id,
                out.mutation.outcome.as_str(),
                out.mutation.resolution_sequence,
                out.revision
            )
            .ok();
        }
        Err(crate::errors::ReviewError::StaleResolutionBasis) => {
            writeln!(
                output,
                "✗ candidate changed since you viewed it; review it again"
            )
            .ok();
        }
        Err(crate::errors::ReviewError::StaleResolutionTarget) => {
            writeln!(
                output,
                "✗ resolution target drifted since you viewed it; review it again"
            )
            .ok();
        }
        Err(e) => {
            writeln!(output, "✗ {e}").ok();
        }
    }
    Ok(())
}
fn get_basis_for(
    service: &ReviewApplicationService<FileReviewStore>,
    id: &str,
    output: &mut dyn Write,
) -> Option<(NodeDigest, String)> {
    match service.execute_query(ReviewQuery::Show(ConceptNodeId(id.into()))) {
        Ok(ReviewReadOutput::Show { node: Some(n), .. }) => {
            // Explicit Candidate gate (node_digest_hex unconditional — R3#1).
            if n.decision_status != "Candidate" {
                writeln!(
                    output,
                    "✗ node {} is not Candidate (status: {}) — only Candidate nodes can be reviewed",
                    n.id, n.decision_status
                )
                .ok();
                return None;
            }
            let hex = &n.node_digest_hex;
            writeln!(output, "  Basis: {} — {} (digest {hex})", n.id, n.canonical).ok();
            Some((
                NodeDigest::from_raw(u64::from_str_radix(hex, 16).expect("valid hex from show")),
                n.canonical.clone(),
            ))
        }
        _ => {
            writeln!(output, "✗ node not found: {id}").ok();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn setup_store(dir: &std::path::Path, id: &str) -> std::path::PathBuf {
        use crate::store_io::{write_persisted_store, PersistedStore};
        use osp_core::anchoring::store::InMemoryAnchorStore;
        use osp_core::anchoring::types::{ConceptNode, ConceptNodeId, ConceptNodeKind, GraphSeed};
        use osp_core::anchoring::{DecisionStatus, PositionFamily};

        let node = ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(node);
        let store = InMemoryAnchorStore::with_seed(seed);
        let path = dir.join("store.json");
        write_persisted_store(
            &path,
            &PersistedStore::from_snapshot(store.export_snapshot()),
        )
        .unwrap();
        path
    }

    /// Interactive informed-acceptance: accept → basis göster → confirm(y) → reason → Accepted.
    /// P1.2: operator basis'i gördükten sonra karar verir.
    #[test]
    fn interactive_accept_shows_basis_before_confirmation() {
        let dir = tempdir().unwrap();
        let path = setup_store(dir.path(), "RuleCandidate:X");
        // accept <id> → confirmation y → reason
        let input = Cursor::new(b"accept RuleCandidate:X\ny\ngood rule\nquit\n");
        let mut output = Vec::new();
        run_interactive(
            &mut std::io::BufReader::new(input),
            &mut output,
            &path,
            OperatorId::new("test-op"),
        )
        .unwrap();
        let out = String::from_utf8(output).unwrap();
        // Basis confirmation prompt gösterildi.
        assert!(
            out.contains("this exact basis?"),
            "expected confirmation prompt, got: {out}"
        );
        assert!(out.contains("Basis: RuleCandidate:X"), "basis shown: {out}");
        assert!(out.contains("Accepted"), "accepted: {out}");
    }

    /// Interactive: confirmation 'n' → abort, mutation uygulanmaz.
    #[test]
    fn interactive_reject_confirmation_aborts_mutation() {
        let dir = tempdir().unwrap();
        let path = setup_store(dir.path(), "RuleCandidate:X");
        // accept → confirmation n → abort.
        let input = Cursor::new(b"accept RuleCandidate:X\nn\nquit\n");
        let mut output = Vec::new();
        run_interactive(
            &mut std::io::BufReader::new(input),
            &mut output,
            &path,
            OperatorId::new("op"),
        )
        .unwrap();
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("aborted by operator"), "abort: {out}");
        assert!(
            !out.contains("Accepted"),
            "mutation should NOT apply on abort: {out}"
        );
    }

    /// Interactive: unknown command handled gracefully.
    #[test]
    fn interactive_unknown_command_handled() {
        let dir = tempdir().unwrap();
        let path = setup_store(dir.path(), "RuleCandidate:X");
        let input = Cursor::new(b"bogus\nquit\n");
        let mut output = Vec::new();
        run_interactive(
            &mut std::io::BufReader::new(input),
            &mut output,
            &path,
            OperatorId::new("op"),
        )
        .unwrap();
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("unknown command"), "output: {out}");
    }

    /// Interactive: empty reason (after confirmation) rejected.
    #[test]
    fn interactive_empty_reason_after_confirmation_rejected() {
        let dir = tempdir().unwrap();
        let path = setup_store(dir.path(), "RuleCandidate:X");
        // accept → confirmation y → empty reason.
        let input = Cursor::new(b"accept RuleCandidate:X\ny\n\nquit\n");
        let mut output = Vec::new();
        run_interactive(
            &mut std::io::BufReader::new(input),
            &mut output,
            &path,
            OperatorId::new("op"),
        )
        .unwrap();
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("reason cannot be empty"), "output: {out}");
        assert!(
            !out.contains("Accepted"),
            "empty reason should not accept: {out}"
        );
    }

    /// resolve_operator: flag yoksa env yoksa prompt açılır.
    #[test]
    fn resolve_operator_prompts_when_flag_and_env_absent() {
        let input = Cursor::new(b"prompted-op\n");
        let mut output = Vec::new();
        let op = resolve_operator(None, &mut std::io::BufReader::new(input), &mut output).unwrap();
        assert_eq!(op, "prompted-op");
        let out = String::from_utf8(output).unwrap();
        assert!(out.contains("Operator identity:"), "prompt shown: {out}");
    }

    /// Failing-writer: render hatası informed-basis garantisini korur — confirmation/reason/
    /// mutation adımlarına geçilmez (Review P1-c). run_informed_supersede Err döner.
    use std::io::{self, Write};
    /// Writer that fails after `limit` bytes (simulates broken pipe / full buffer).
    struct FailingWriter {
        written: usize,
        limit: usize,
    }
    impl Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.written >= self.limit {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "writer failed"));
            }
            let n = std::cmp::min(buf.len(), self.limit - self.written);
            self.written += n;
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// İki Accepted node'lu store (supersede için) — review_session test yardımcısı.
    fn setup_two_accepted_store(dir: &std::path::Path) -> std::path::PathBuf {
        use crate::store_io::{write_persisted_store, PersistedStore};
        use osp_core::anchoring::store::InMemoryAnchorStore;
        use osp_core::anchoring::types::{ConceptNode, ConceptNodeId, ConceptNodeKind, GraphSeed};
        use osp_core::anchoring::{DecisionStatus, PositionFamily};
        let mk = |id: &str| ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(mk("RuleCandidate:Old"));
        seed.rule_candidates.push(mk("RuleCandidate:New"));
        let store = InMemoryAnchorStore::with_seed(seed);
        let path = dir.join("store2.json");
        write_persisted_store(&path, &PersistedStore::from_snapshot(store.export_snapshot()))
            .unwrap();
        path
    }

    #[test]
    fn supersede_render_failure_aborts_mutation() {
        let dir = tempdir().unwrap();
        let path = setup_two_accepted_store(dir.path());
        let repo = crate::application::repository::FileReviewStore::new(&path);
        let service = ReviewApplicationService::new(repo);
        let mut input = Cursor::new(b"y\nreason\n"); // confirmation + reason (asla okunmamalı)
        let mut output = FailingWriter { written: 0, limit: 5 }; // render çok erken fail
        let result = run_informed_supersede(
            &service,
            &mut input,
            &mut output,
            "RuleCandidate:Old",
            "RuleCandidate:New",
            &OperatorId::new("op"),
        );
        // Render hatası ? ile yayılır → Err döner; confirmation/reason/mutation yürümez.
        assert!(result.is_err(), "render failure must propagate Err");
        // Revision unchanged — mutation gerçekleşmedi.
        let persisted = crate::store_io::read_persisted_store(&path).unwrap();
        assert_eq!(persisted.revision, 0, "mutation must not run after render failure");
    }

    /// Content-aware stage writer: yazılan içerikte `trigger` byte dizisi geçtikten SONRA
    /// fail eder. Byte-offset değil — gerçekten hedef prompt render edildikten sonra kırılır,
    /// böylece test adı/iddia edilen stage ile çalışan branch birebir eşleşir (Review P3).
    /// Buffer taşımalı write'larda `windows` ile parça eşleşmesi arar (split prompt güvenli).
    struct StageFailingWriter {
        sink: Vec<u8>,
        trigger: &'static [u8],
        triggered: bool,
    }
    impl Write for StageFailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.sink.extend_from_slice(buf);
            if self.sink.windows(self.trigger.len()).any(|w| w == self.trigger) {
                self.triggered = true;
            }
            if self.triggered {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "stage writer failed"));
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.triggered {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "stage flush failed"));
            }
            Ok(())
        }
    }

    /// Helper: content-aware stage failure testi — trigger byte dizisi render edildikten sonra
    /// fail, mutation yok. `trigger` = hedef prompt string'i (confirmation/reason).
    fn run_stage_failure_test(trigger: &'static [u8]) -> (std::io::Result<()>, u64) {
        let dir = tempdir().unwrap();
        let path = setup_two_accepted_store(dir.path());
        let repo = crate::application::repository::FileReviewStore::new(&path);
        let service = ReviewApplicationService::new(repo);
        let mut input = Cursor::new(b"y\ngood reason\n"); // asla tam okunmamalı
        let mut output = StageFailingWriter {
            sink: Vec::new(),
            trigger,
            triggered: false,
        };
        let result = run_informed_supersede(
            &service,
            &mut input,
            &mut output,
            "RuleCandidate:Old",
            "RuleCandidate:New",
            &OperatorId::new("op"),
        );
        let revision = crate::store_io::read_persisted_store(&path).unwrap().revision;
        (result, revision)
    }

    /// Confirmation prompt failure: preview body render edildi, confirmation prompt yazımı başarısız
    /// → Err, confirmation/reason tüketilmedi, revision unchanged (Review P1).
    /// Content-aware writer "Apply this exact supersession" trigger'ı gördükten sonra fail eder
    /// — gerçekten confirmation stage'de kırılır (byte-offset fragility yok, Review P3).
    #[test]
    fn supersede_confirmation_prompt_failure_aborts_mutation() {
        let (result, revision) = run_stage_failure_test(b"Apply this exact supersession");
        assert!(result.is_err(), "confirmation prompt failure must propagate Err");
        assert_eq!(revision, 0, "mutation must not run after confirmation prompt failure");
    }

    /// Reason prompt failure: preview + confirmation prompt başarılı, operator "y" verdi,
    /// reason prompt yazımı başarısız → Err, reason/mutation uygulanmadı (Review P1).
    /// Content-aware writer yalnız "Reason:" trigger'ı gördükten sonra fail eder — confirmation
    /// stage'i tamamlanır, gerçekten reason stage'de kırılır (byte-offset fragility yok, Review P3).
    #[test]
    fn supersede_reason_prompt_failure_aborts_mutation() {
        let (result, revision) = run_stage_failure_test(b"  Reason:");
        assert!(result.is_err(), "reason prompt failure must propagate Err");
        assert_eq!(revision, 0, "mutation must not run after reason prompt failure");
    }
}
