//! `osp graph` altkomutları — Candidate-only seed bootstrap + store durumu.
//!
//! - `init`: CandidateSeedFile → restore-validasyon → write (existing → fail, --force overwrite).
//! - `status`: node/edge/ledger counts + audit_seq.
//! - `validate`: restore + invariant-validasyon (read-only).

use std::path::PathBuf;

use clap::{ArgGroup, Args};
use osp_core::anchoring::store::{AnchorStore, InMemoryAnchorStore};

use crate::analysis_bridge::project_analysis;
use crate::canonical_identity::PathCasePolicy;
use crate::evidence_projection::EvidenceProjectionContext;
use crate::graph_seed_builder::{GraphSeedBuilder, GraphSeedNodeDraft};
use crate::seed_file::CandidateSeedFile;
use crate::store_io::{read_persisted_store, write_persisted_store, PersistedStore, StoreLock};

/// Path case politikası (CLI flag). Yalnız `--analyze` ile (host OS'den bağımsız).
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum PathCaseArg {
    Sensitive,
    AsciiInsensitive,
}

impl From<PathCaseArg> for PathCasePolicy {
    fn from(arg: PathCaseArg) -> Self {
        match arg {
            PathCaseArg::Sensitive => Self::CaseSensitive,
            PathCaseArg::AsciiInsensitive => Self::AsciiCaseInsensitive,
        }
    }
}

/// `osp graph init` — iki source: `--seed <json>` veya `--analyze <repo>`.
/// Clap ArgGroup ile mutual exclusion (exactly one required).
#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("input")
        .required(true)
        .multiple(false)
        .args(["seed", "analyze"])
))]
pub struct GraphInitArgs {
    /// Candidate seed JSON dosyası (nodes-only; status/id alanları yok).
    #[arg(long)]
    pub seed: Option<PathBuf>,
    /// Repo analiz et → Candidate projection (analysis bridge).
    #[arg(long)]
    pub analyze: Option<PathBuf>,
    /// SCIP index path'i (--analyze ile; gerçek LCOM4 cohesion için).
    #[arg(long, requires = "analyze")]
    pub scip: Option<PathBuf>,
    /// Path case politikası (yalnız --analyze; default: ascii-insensitive).
    #[arg(long, value_enum, requires = "analyze")]
    pub path_case: Option<PathCaseArg>,
    /// Canonical store JSON dosyası (persisted AnchorStoreSnapshot).
    #[arg(long)]
    pub store: PathBuf,
    /// Mevcut store dosyasını overwrite et (default: fail).
    #[arg(long)]
    pub force: bool,
}

/// `osp graph status --store <path>` — store durumu.
#[derive(Args, Debug)]
pub struct GraphStatusArgs {
    #[arg(long)]
    pub store: PathBuf,
    /// JSON çıktı (CI/script).
    #[arg(long, default_value = "text")]
    pub format: String,
}

/// `osp graph validate --store <path>` — restore + invariant-validasyon.
#[derive(Args, Debug)]
pub struct GraphValidateArgs {
    #[arg(long)]
    pub store: PathBuf,
    #[arg(long, default_value = "text")]
    pub format: String,
}

/// `osp graph init` — iki source (`--seed` veya `--analyze`) → Candidate store.
///
/// Aynı StoreLock protokolü (review mutation'ları ile tek concurrency sözleşmesi):
/// lock → existence/force check → source projection → validate → atomic write → unlock.
/// Pre-validation non-destructive (P3+N2): validation/builder hatasında store değişmez,
/// `--force` dahil (eski store atomic rename'e kadar durur).
pub fn run_graph_init(args: GraphInitArgs) -> anyhow::Result<()> {
    // (0) Parent directory oluştur (bootstrap sorumluluğu).
    if let Some(parent) = args.store.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!("cannot create store directory {}: {e}", parent.display())
            })?;
        }
    }

    // (1) Exclusive lock — sabit .lock dosyası (review mutation'ları ile aynı).
    let _lock = StoreLock::acquire(&args.store)
        .map_err(|e| anyhow::anyhow!("cannot acquire store lock: {e}"))?;

    // (2) Existence/force check (lock altında).
    if args.store.exists() && !args.force {
        anyhow::bail!(
            "store file already exists at {}: use --force to overwrite",
            args.store.display()
        );
    }

    // (3) Source → (GraphSeedNodeDraft[], Option<CodeIdentityBinding[]>) (iki source — S2).
    //     Pre-validation non-destructive: hata burada olursa store'a hiç dokunulmaz.
    //     P2: --path-case yalnız --analyze ile (Clap requires yetersiz — explicit kontrol).
    //     PR E2: analyze source binding üretir; seed source (legacy) None (PR A semantics preserved).
    let (drafts, identity_bindings): (
        Vec<GraphSeedNodeDraft>,
        Option<Vec<osp_core::anchoring::types::CodeIdentityBinding>>,
    ) = if let Some(seed_path) = &args.seed {
        // Legacy JSON source — F1 semantics (ConceptualIntent, Candidate, aliases).
        // PR E2: legacy source binding üretmez (PR A legacy semantics preserved).
        if args.path_case.is_some() {
            anyhow::bail!("--path-case can only be used with --analyze");
        }
        if args.scip.is_some() {
            anyhow::bail!("--scip can only be used with --analyze");
        }
        let seed_json = std::fs::read_to_string(seed_path)
            .map_err(|e| anyhow::anyhow!("cannot read seed file {}: {e}", seed_path.display()))?;
        let seed_file = CandidateSeedFile::from_json(&seed_json)
            .map_err(|e| anyhow::anyhow!("invalid seed file: {e}"))?;
        let drafts = seed_file
            .into_drafts()
            .map_err(|e| anyhow::anyhow!("seed validation failed: {e}"))?;
        (drafts, None)
    } else if let Some(repo) = &args.analyze {
        // Analysis source — PhysicalCode, identity_key NodeId, INV-C5 Candidate.
        let registry = osp_analyzer::language::AdapterRegistry::default_all();
        let config = osp_analyzer::contract::AnalysisConfig {
            scip_index: args.scip.clone(),
            ..Default::default()
        };
        let analysis = osp_analyzer::pipeline::analyze_repo_with_config(repo, &registry, &config)
            .map_err(|e| anyhow::anyhow!("analysis failed: {e}"))?;
        let policy = args
            .path_case
            .map(Into::into)
            .unwrap_or(PathCasePolicy::AsciiCaseInsensitive);
        // Evidence projection context — wall-clock inject (fail-closed; store mutation'dan önce).
        let evidence_context = EvidenceProjectionContext {
            measured_at: now_unix_secs()?,
        };
        let bridge_output = project_analysis(&analysis, policy, evidence_context)
            .map_err(|e| anyhow::anyhow!("analysis bridge projection failed: {e}"))?;
        if bridge_output.candidate_seed.is_empty() {
            eprintln!("warning: analysis produced no projectable Module nodes");
        }
        // Metric projection özeti (stderr — draft admission counts).
        let mp = &bridge_output.metric_projection;
        eprintln!(
            "Code metric drafts admitted: {}",
            mp.report.projected_axis_values
        );
        eprintln!(
            "Metrics omitted: placeholder={}, heuristic={}, zero-confidence={}",
            mp.report.skipped_placeholder,
            mp.report.skipped_heuristic,
            mp.report.skipped_zero_confidence
        );
        // Evidence projection özeti (stderr — tur 2 dürüst consumer beyanı).
        let ep = &bridge_output.evidence_projection.report;
        eprintln!("Evidence construction: completed");
        eprintln!("Evidence objects: {}", ep.evidence_objects_created);
        eprintln!("Partial evidence objects: {}", ep.partial_evidence_objects);
        eprintln!("Evidence runtime consumer: none in graph init");
        eprintln!("Evidence persistence: disabled");
        eprintln!("{}", bridge_output.graph_report);
        // PR E2 — binding'leri sakla (into_drafts consume'dan önce); drafts ile birlikte döndür.
        let code_identity_bindings = bridge_output.code_identity_bindings.clone();
        let drafts = bridge_output.candidate_seed.into_drafts();
        (drafts, Some(code_identity_bindings))
    } else {
        // ArgGroup guaranteed exactly one; unreachable.
        anyhow::bail!("either --seed <json> or --analyze <repo> required");
    };

    // (4) One-shot GraphSeedBuilder → GraphSeed (graph-level invariant, partial imkânsız).
    let graph_seed = GraphSeedBuilder::build(drafts)
        .map_err(|e| anyhow::anyhow!("graph seed construction failed: {e}"))?;

    // (5) with_seed → export → restore-validasyon (corrupt seed'i baştan yakala).
    let mut store = InMemoryAnchorStore::with_seed(graph_seed);
    // PR E2 — identity binding seeding (node existence sonrası; analyze source yalnız).
    // seed_code_identity_bindings_trusted: node existence + kind + family + duplicate + R7 validation.
    // Tur 1 review P2-1: başarılı seeding stderr'i durable write SONRASI basılır
    // (restore validation + serialization/write/fsync/atomic replace fail ederse "seeded" iddia edilemez).
    let seeded_binding_count = identity_bindings.as_ref().map_or(0, Vec::len);
    if let Some(bindings) = &identity_bindings {
        if !bindings.is_empty() {
            store
                .seed_code_identity_bindings_trusted(bindings)
                .map_err(|e| anyhow::anyhow!("identity binding seeding failed: {e}"))?;
        }
    }
    let snapshot = store.export_snapshot();
    InMemoryAnchorStore::restore_snapshot(snapshot.clone())
        .map_err(|e| anyhow::anyhow!("post-init restore validation failed (corrupt seed): {e}"))?;

    // (6) Atomic write (lock altında). --force eski dosyayı erkenden silmez (N2);
    //     write_persisted_store atomic_replace (temp + fsync + rename) — eski dosya
    //     rename anına kadar durur.
    let persisted = PersistedStore::from_snapshot(snapshot);
    write_persisted_store(&args.store, &persisted)
        .map_err(|e| anyhow::anyhow!("cannot write store: {e}"))?;

    // PR E2 (tur 1 review P2-1) — durable write sonrası "persisted" mesajı (disk'e yazıldı kesin).
    if seeded_binding_count > 0 {
        eprintln!("identity bindings persisted: {seeded_binding_count}");
    }

    // lock drop → release.
    println!(
        "✓ Graph initialized ({} candidate nodes)",
        persisted.snapshot.graph.nodes.len()
    );
    println!("  Store: {}", args.store.display());
    println!("  Revision: 0");
    Ok(())
}

/// `osp graph status` — node/edge/ledger counts + audit_seq.
pub fn run_graph_status(args: GraphStatusArgs) -> anyhow::Result<()> {
    let persisted =
        read_persisted_store(&args.store).map_err(|e| anyhow::anyhow!("cannot read store: {e}"))?;
    let node_count = persisted.snapshot.graph.nodes.len();
    let edge_count = persisted.snapshot.graph.edges.len();
    let candidate_count = persisted
        .snapshot
        .graph
        .nodes
        .iter()
        .filter(|n| n.decision_status == osp_core::anchoring::DecisionStatus::Candidate)
        .count();
    let accepted_count = persisted
        .snapshot
        .graph
        .nodes
        .iter()
        .filter(|n| n.decision_status == osp_core::anchoring::DecisionStatus::Accepted)
        .count();
    let superseded_count = persisted
        .snapshot
        .graph
        .nodes
        .iter()
        .filter(|n| n.decision_status == osp_core::anchoring::DecisionStatus::SupersededAccepted)
        .count();

    if args.format == "json" {
        let json = serde_json::json!({
            "store_schema_version": persisted.store_schema_version,
            "revision": persisted.revision,
            "node_count": node_count,
            "edge_count": edge_count,
            "candidates": candidate_count,
            "accepted": accepted_count,
            "superseded_accepted": superseded_count,
            "decision_records": persisted.snapshot.decision_records.len(),
            "supersede_records": persisted.snapshot.supersede_records.len(),
            "audit_sequence": persisted.snapshot.audit_sequence,
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("✓ Store status: {}", args.store.display());
        println!("  Revision: {}", persisted.revision);
        println!("  Nodes: {node_count} (candidates: {candidate_count}, accepted: {accepted_count}, superseded: {superseded_count})");
        println!("  Edges: {edge_count}");
        println!(
            "  Decision records: {}",
            persisted.snapshot.decision_records.len()
        );
        println!(
            "  Supersede records: {}",
            persisted.snapshot.supersede_records.len()
        );
        println!("  Audit sequence: {}", persisted.snapshot.audit_sequence);
    }
    Ok(())
}

/// `osp graph validate` — restore + invariant-validasyon (read-only).
pub fn run_graph_validate(args: GraphValidateArgs) -> anyhow::Result<()> {
    let persisted =
        read_persisted_store(&args.store).map_err(|e| anyhow::anyhow!("cannot read store: {e}"))?;
    match InMemoryAnchorStore::restore_snapshot(persisted.snapshot.clone()) {
        Ok(_) => {
            if args.format == "json" {
                let json = serde_json::json!({
                    "valid": true,
                    "revision": persisted.revision,
                    "node_count": persisted.snapshot.graph.nodes.len(),
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("✓ Store valid: {}", args.store.display());
                println!("  Revision: {}", persisted.revision);
                println!(
                    "  All invariants pass (node uniqueness, edge endpoints, ledger/status integrity, dense audit_seq, C15 triangulation)"
                );
            }
            Ok(())
        }
        Err(e) => {
            if args.format == "json" {
                let json = serde_json::json!({
                    "valid": false,
                    "error": e.to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("✗ Store invalid: {e}");
            }
            std::process::exit(1);
        }
    }
}

/// Unix epoch saniye — wall-clock (PR D evidence projection `measured_at`).
///
/// **Fail-closed** (tur 3 P2): sistem saati epoch öncesi olduğunda `measured_at=0` üretmek
/// evidence provenance açısından yanlış (geçerli ama aşırı eski evidence). Clock failure
/// `Result` olarak yukarı taşınır; store mutation'dan **önce** gerçekleşir (non-destructive
/// validation düzeni korunur).
fn now_unix_secs() -> anyhow::Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| anyhow::anyhow!("system clock is before UNIX_EPOCH: {error}"))
}
