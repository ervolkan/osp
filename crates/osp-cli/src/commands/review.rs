//! `osp review` altkomutları — canonical one-shot yüzey.
//!
//! Query'ler (list/show) read-only, `--operator` gerekmez. Mutation'lar (accept/reject)
//! `--operator` zorunlu + confirmation (TTY: basis göster + `[y/N]`; non-TTY/`--yes`:
//! `--basis-digest` zorunlu). Hepsi `ReviewApplicationService`'i çağırır (interactive
//! ile aynı service).

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use clap::Args;
use osp_core::anchoring::review::{NodeDigest, OperatorId};
use osp_core::anchoring::types::ConceptNodeId;

use crate::application::repository::FileReviewStore;
use crate::application::review::{ReviewMutationCommand, ReviewQuery};
use crate::application::ReviewApplicationService;
use crate::commands::OutputFormat;

// Interactive session — review_session.rs modülünde (generic R/W). Re-export edilir.
pub use crate::review_session::{run_review_session, ReviewSessionArgs};

/// Argümansız `osp review` — default store + operator prompt ile interactive session.
/// Root flag yoktur (Review 2.tur P1.1); subcommand'lar kendi --store/--operator taşır.
pub fn run_review_session_default() -> anyhow::Result<()> {
    run_review_session(ReviewSessionArgs::default())
}

/// `osp review list` — candidate lane.
#[derive(Args, Debug)]
pub struct ReviewListArgs {
    #[arg(long, default_value = ".osp/anchor-store.json")]
    pub store: PathBuf,
    #[arg(long, default_value = "text")]
    pub format: String,
}

/// `osp review show <id>` — node detayı + basis digest (Candidate için).
#[derive(Args, Debug)]
pub struct ReviewShowArgs {
    /// Node ID (örn "RuleCandidate:CouplingMustNot").
    pub id: String,
    #[arg(long, default_value = ".osp/anchor-store.json")]
    pub store: PathBuf,
    #[arg(long, default_value = "text")]
    pub format: String,
}

/// `osp review accept <id>` — Candidate → Accepted.
#[derive(Args, Debug)]
pub struct ReviewAcceptArgs {
    /// Node ID.
    pub id: String,
    #[arg(long, default_value = ".osp/anchor-store.json")]
    pub store: PathBuf,
    /// Operator kimliği (zorunlu mutation için). `$OSP_OPERATOR` env fallback.
    #[arg(long)]
    pub operator: Option<String>,
    /// Kabul gerekçesi (boş olamaz).
    #[arg(long)]
    pub reason: String,
    /// Operator'ın gördüğü basis digest (non-interactive'de zorunlu). Hex format.
    #[arg(long)]
    pub basis_digest: Option<String>,
    /// Confirmation'ı atla (non-TTY/CI). `--basis-digest` zorunlu.
    #[arg(long)]
    pub yes: bool,
}

/// `osp review reject <id>` — Candidate → Rejected.
#[derive(Args, Debug)]
pub struct ReviewRejectArgs {
    pub id: String,
    #[arg(long, default_value = ".osp/anchor-store.json")]
    pub store: PathBuf,
    #[arg(long)]
    pub operator: Option<String>,
    #[arg(long)]
    pub reason: String,
    #[arg(long)]
    pub basis_digest: Option<String>,
    #[arg(long)]
    pub yes: bool,
}

/// `osp review list` handler.
pub fn run_review_list(args: ReviewListArgs) -> anyhow::Result<()> {
    let repo = FileReviewStore::new(&args.store);
    let service = ReviewApplicationService::new(repo);
    let output = service
        .execute_query(ReviewQuery::List)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let format = OutputFormat::from_str(&args.format);
    match output {
        crate::application::ReviewReadOutput::List { items, revision } => {
            // JSON her zaman üretilmeli (boş liste dahil) — otomasyon contract (Review 3.tur P2.1).
            if format == OutputFormat::Json {
                let json = serde_json::json!({
                    "items": items,
                    "revision": revision,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if items.is_empty() {
                println!("No candidates awaiting review.");
            } else {
                println!("Candidates awaiting review ({}):", items.len());
                for item in &items {
                    println!("  {}  {}  [{}]", item.id, item.canonical, item.kind);
                }
                println!("  Revision: {revision}");
            }
        }
        _ => unreachable!("List query returns List output"),
    }
    Ok(())
}

/// `osp review show <id>` handler.
pub fn run_review_show(args: ReviewShowArgs) -> anyhow::Result<()> {
    let repo = FileReviewStore::new(&args.store);
    let service = ReviewApplicationService::new(repo);
    let output = service
        .execute_query(ReviewQuery::Show(ConceptNodeId(args.id.clone())))
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let format = OutputFormat::from_str(&args.format);
    match output {
        crate::application::ReviewReadOutput::Show { node, revision } => match node {
            None => {
                println!("✗ Node not found: {}", args.id);
                std::process::exit(1);
            }
            Some(details) => {
                if format == OutputFormat::Json {
                    // JSON: revision dahil (text ile aynı bilgi — Review 3.tur P2.1).
                    let json = serde_json::json!({
                        "node": details,
                        "revision": revision,
                    });
                    println!("{}", serde_json::to_string_pretty(&json)?);
                } else {
                    println!("Node: {}", details.id);
                    println!("  Canonical: {}", details.canonical);
                    println!("  Kind: {}", details.kind);
                    println!("  Status: {}", details.decision_status);
                    if let Some(succ) = &details.superseded_by {
                        println!("  Superseded by: {succ}");
                    }
                    if let Some(hex) = &details.basis_digest_hex {
                        println!("  Basis digest: {hex}");
                        println!("    (accept/reject için --basis-digest {hex})");
                    }
                    println!("  Revision: {revision}");
                }
            }
        },
        _ => unreachable!("Show query returns Show output"),
    }
    Ok(())
}

/// `osp review accept` handler.
pub fn run_review_accept(args: ReviewAcceptArgs) -> anyhow::Result<()> {
    run_review_mutation(args, true)
}

/// `osp review reject` handler.
pub fn run_review_reject(args: ReviewRejectArgs) -> anyhow::Result<()> {
    run_review_mutation(args, false)
}

/// Ortak mutation handler (accept/reject). Confirmation modeli (R1#2).
fn run_review_mutation<M: MutationArgs>(args: M, accept: bool) -> anyhow::Result<()> {
    // Operator kimliği (zorunlu).
    let operator = resolve_operator(args.operator())?;
    let operator_id = OperatorId::new(operator);

    // Confirmation modeli: TTY'de basis göster + [y/N]; non-TTY/--yes → --basis-digest zorunlu.
    let is_tty = std::io::stdin().is_terminal();
    let expected_digest = if args.yes() || !is_tty {
        // Non-interactive: --basis-digest zorunlu.
        let hex = args
            .basis_digest()
            .ok_or_else(|| anyhow::anyhow!("--basis-digest <hex> required for non-interactive accept/reject (run `osp review show <id>` to get it)"))?;
        parse_digest_hex(&hex)?
    } else {
        // TTY: basis göster + onaylat. Digest'i gösterilen basis'ten al.
        confirm_with_basis(&args, accept)?
    };

    let id = ConceptNodeId(args.id().to_string());
    let reason = args.reason().to_string();
    let command = if accept {
        ReviewMutationCommand::Accept {
            id,
            expected_basis_digest: expected_digest,
            reason,
        }
    } else {
        ReviewMutationCommand::Reject {
            id,
            expected_basis_digest: expected_digest,
            reason,
        }
    };

    let repo = FileReviewStore::new(args.store());
    let service = ReviewApplicationService::new(repo);
    let output = service
        .execute_mutation(command, operator_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!(
        "✓ {} node {} (decision record #{})",
        if accept { "Accepted" } else { "Rejected" },
        output.mutation.node_id,
        output.mutation.decision_sequence
    );
    println!("  Revision: {}", output.revision);
    Ok(())
}

/// Operator kimliği çöz: --operator > $OSP_OPERATOR > fail (generic "operator" default yok).
/// Boş/whitespace değer reject edilir (Review 2.tur P2.3) — attribution boş olamaz.
fn resolve_operator(flag: Option<String>) -> Result<String, anyhow::Error> {
    if let Some(op) = flag {
        return normalize_operator(&op);
    }
    if let Ok(env_op) = std::env::var("OSP_OPERATOR") {
        return normalize_operator(&env_op);
    }
    Err(anyhow::anyhow!(
        "Operator identity is required. Provide --operator <id> or set OSP_OPERATOR env var."
    ))
}

/// Operator değerini normalize: trim + boş reject. Flag/env/prompt için ortak.
fn normalize_operator(value: &str) -> Result<String, anyhow::Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("operator identity cannot be empty");
    }
    Ok(trimmed.to_owned())
}

/// Hex digest parse → NodeDigest.
fn parse_digest_hex(hex: &str) -> Result<NodeDigest, anyhow::Error> {
    let hex = hex.trim();
    let raw = u64::from_str_radix(hex, 16)
        .map_err(|e| anyhow::anyhow!("invalid --basis-digest (expected hex u64): {e}"))?;
    Ok(NodeDigest::from_raw(raw))
}

/// TTY'de basis göster + onaylat. Gösterilen basis'in digest'ini döner.
fn confirm_with_basis<M: MutationArgs>(
    args: &M,
    accept: bool,
) -> Result<NodeDigest, anyhow::Error> {
    // Önce show ile node'u göster.
    let repo = FileReviewStore::new(args.store());
    let service = ReviewApplicationService::new(repo);
    let output = service
        .execute_query(ReviewQuery::Show(ConceptNodeId(args.id().to_string())))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let node = match output {
        crate::application::ReviewReadOutput::Show { node: Some(n), .. } => n,
        _ => anyhow::bail!("node not found: {}", args.id()),
    };
    let digest_hex = node.basis_digest_hex.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "node {} is not Candidate (status: {}) — only Candidate nodes can be reviewed",
            node.id,
            node.decision_status
        )
    })?;
    let digest = parse_digest_hex(digest_hex)?;

    println!("Candidate: {}", node.id);
    println!("  Canonical: {}", node.canonical);
    println!("  Kind: {}", node.kind);
    println!("  Digest: {digest_hex}");
    println!("  Reason: {}", args.reason());
    print!(
        "  {} this exact basis? [y/N] ",
        if accept { "Accept" } else { "Reject" }
    );
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();
    if input != "y" && input != "yes" {
        anyhow::bail!("aborted by operator");
    }
    Ok(digest)
}

/// Mutation arg trait — accept/reject ortak yüzey.
pub trait MutationArgs {
    fn id(&self) -> &str;
    fn store(&self) -> &std::path::Path;
    fn operator(&self) -> Option<String>;
    fn reason(&self) -> &str;
    fn basis_digest(&self) -> Option<String>;
    fn yes(&self) -> bool;
}

impl MutationArgs for ReviewAcceptArgs {
    fn id(&self) -> &str {
        &self.id
    }
    fn store(&self) -> &std::path::Path {
        &self.store
    }
    fn operator(&self) -> Option<String> {
        self.operator.clone()
    }
    fn reason(&self) -> &str {
        &self.reason
    }
    fn basis_digest(&self) -> Option<String> {
        self.basis_digest.clone()
    }
    fn yes(&self) -> bool {
        self.yes
    }
}

impl MutationArgs for ReviewRejectArgs {
    fn id(&self) -> &str {
        &self.id
    }
    fn store(&self) -> &std::path::Path {
        &self.store
    }
    fn operator(&self) -> Option<String> {
        self.operator.clone()
    }
    fn reason(&self) -> &str {
        &self.reason
    }
    fn basis_digest(&self) -> Option<String> {
        self.basis_digest.clone()
    }
    fn yes(&self) -> bool {
        self.yes
    }
}
