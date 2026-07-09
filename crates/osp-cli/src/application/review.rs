//! Review application service — tek domain motoru (query + mutation).
//!
//! Query'ler (list/show) read-only — `repository.read()`, `--operator` gerekmez.
//! Mutation'lar (accept/reject) `repository.mutate()` — `expected_basis_digest` precondition
//! (operator'ın gördüğü basis ile karar anındaki aynı — Review 1#1).
//!
//! # Basis-freshness (Review 1#1 + Review 3 son#2)
//! `mutate()` closure'ı içinde (lock altında — yeni TOCTOU penceresi yok):
//! ```text
//! current_digest = node_digest(store'daki node)
//! if current_digest != expected_basis_digest → ReviewError::StaleBasis
//! basis = PresentedBasis::compile(store, id)   // gerçek basis, digest precondition sonrası
//! session.accept/reject(basis, reason)
//! ```
//! Session'ın kendi StaleBasis'i tautolojik (zararsız) — tek mesaja map (Review 4).

use osp_core::anchoring::review::{
    node_digest, NodeDigest, OperatorId, OperatorReviewSession, PresentedBasis,
    PresentedSupersedeBasis, SupersedeError, SupersedeRecord, SupersedeSession,
};
use osp_core::anchoring::store::{InMemoryAnchorStore, StoreError};
use osp_core::anchoring::types::ConceptNodeId;
use osp_core::anchoring::NonEmptyExplanation;

use crate::application::repository::ReviewStoreRepository;
use crate::errors::{
    format_endpoint_status, PersistedReviewOutput, PersistedSupersedeOutput, ReviewError,
    ReviewMutation, ReviewSupersedeMutation, SupersedeCommand, SupersedeDigests, SupersedeEndpoint,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Command tipleri — query/mutation ayrımı (Review 3 son#1)
// ═══════════════════════════════════════════════════════════════════════════════

/// Read-only review komutları — `--operator` gerekmez.
#[derive(Debug, Clone)]
pub enum ReviewQuery {
    /// Candidate lane'i listele (candidate_query).
    List,
    /// Tek node detayı (id ile — Candidate/Accepted/SupersededAccepted görüntüler).
    Show(ConceptNodeId),
    /// Rich supersede preview — lineage DAG + compatibility + structural eligibility.
    /// `osp review supersede-preview <old> <new>` + confirmation yüzeyleri aynı query'yi kullanır.
    SupersedePreview {
        superseded: ConceptNodeId,
        successor: ConceptNodeId,
    },
}

/// Mutation review komutları — `--operator` zorunlu, `expected_basis_digest` precondition.
#[derive(Debug, Clone)]
pub enum ReviewMutationCommand {
    Accept {
        id: ConceptNodeId,
        /// Operator'ın gördüğü basis digest — karar anındaki ile aynı olmalı (Review 1#1).
        expected_basis_digest: NodeDigest,
        reason: String,
    },
    Reject {
        id: ConceptNodeId,
        expected_basis_digest: NodeDigest,
        reason: String,
    },
}

/// Read-only query çıktısı (revision dahil — operator'a güncel revision göstermek için).
#[derive(Debug, Clone)]
pub enum ReviewReadOutput {
    List {
        items: Vec<ReviewListItem>,
        revision: u64,
    },
    Show {
        node: Option<ReviewNodeDetails>,
        revision: u64,
    },
    /// Rich supersede preview (lineage DAG + compatibility + structural eligibility).
    SupersedePreview(SupersedePreviewOutput),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewListItem {
    pub id: String,
    pub canonical: String,
    pub kind: String,
    pub decision_status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewNodeDetails {
    pub id: String,
    pub canonical: String,
    pub kind: String,
    pub decision_status: String,
    /// Node freshness digest (hex). Tüm statülerde dolu — `node_digest` (canonical +
    /// sorted aliases + kind + family) tek source of truth. Candidate review bunu
    /// `--basis-digest` precondition olarak kullanır; supersede iki endpoint için.
    /// Hex string (JS 2^53 sınırı; raw u64 değil). Accept/reject için `ensure_candidate`.
    pub node_digest_hex: String,
    /// Successor node id — SupersededAccepted node'lar için (committed Supersedes edge'inden).
    /// Edge yönü: successor --Supersedes--> superseded; bu node superseded ise successor'u göster.
    pub superseded_by: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ReviewApplicationService
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Rich SupersedePreview — canonical read model (tek model, 3 yüzey render eder)
//
// `osp review supersede-preview` standalone query + one-shot TTY confirmation + interactive
// wizard confirmation aynı `SupersedePreviewOutput` modelini ve tek renderer'ı kullanır
// → divergence sıfır. HANDOFF "aynı preview render eder" cümlesi doğru kalır.
//
// Domain policy ayrımı (divergence mekanik olarak engellenir):
//   incoming policy      → committed_supersede_incoming_sources (core accessor)
//   currentness policy   → DecisionStatus::is_current_mainline()
//   compatibility policy → inspect_supersede_compatibility (core predicate)
//   cycle policy         → would_create_supersede_cycle (core predicate)
//   identity equality    → saf observation (kural yok)
//
// `structurally_eligible` point-in-time read-only assessment — mutation revalidates both
// digests + currentness under lock (preview ≠ commit guarantee).
// ═══════════════════════════════════════════════════════════════════════════════

/// Structural blocker kodu — typed (schema drift'e kapalı). Sıra `ordering_key()` ile
/// `apply_supersede` structural steps 5–10'a birebir hizalı (freshness/basis/audit hariç).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupersedeBlockerCode {
    AlreadySuperseded,
    SupersededNotCurrent,
    SuccessorNotCurrent,
    SelfSupersede,
    IncompatibleKind,
    IncompatibleFamily,
    Cycle,
}

impl SupersedeBlockerCode {
    /// Structural validation step — `apply_supersede` steps 5–10 ile birebir.
    /// Freshness/basis/audit (steps 1–4, 11) preview scope'unda DEĞİL (preview operatörün
    /// expected digest'ini bilmez, sadece current gösterir).
    pub const fn mutation_step(self) -> u8 {
        match self {
            Self::AlreadySuperseded => 5,
            Self::SupersededNotCurrent => 6,
            Self::SuccessorNotCurrent => 7,
            Self::SelfSupersede => 8,
            Self::IncompatibleKind => 9,
            Self::IncompatibleFamily => 9,
            Self::Cycle => 10,
        }
    }

    /// Step 9 tie-break (kind < family — deterministic).
    pub const fn tie_break(self) -> u8 {
        match self {
            Self::IncompatibleKind => 0,
            Self::IncompatibleFamily => 1,
            _ => 0,
        }
    }

    /// Deterministik ordering key — `(mutation_step, tie_break)`.
    pub const fn ordering_key(self) -> (u8, u8) {
        (self.mutation_step(), self.tie_break())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SupersedeBlocker {
    pub code: SupersedeBlockerCode,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SupersedeEndpointPreview {
    pub id: String,
    pub canonical: String,
    pub kind: String,
    pub status: String,
    pub family: String,
    pub node_digest_hex: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SupersedeLineageNode {
    pub id: String,
    pub depth: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SupersedeLineageEdge {
    pub from: String,
    pub to: String,
}

/// Truncation nedeni — `Some` ise en az bir committed outgoing edge output'a sığmadı.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineageTruncation {
    DepthLimit,
    NodeLimit,
    Both,
}

/// Successor outgoing committed Supersedes lineage — bounded DAG (consolidation'da branching).
/// `depth` = root'tan BFS shortest-path (ilk ziyaret). Closed-output: her edge from/to nodes içinde.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SupersedeLineagePreview {
    pub root: String,
    pub nodes: Vec<SupersedeLineageNode>,
    pub edges: Vec<SupersedeLineageEdge>,
    pub truncation: Option<LineageTruncation>,
    pub max_depth: usize,
    pub max_nodes: usize,
    /// Superseded'a gelen committed edge source'u (INV-C15 ≤1; tek core accessor kaynağı).
    pub superseded_incoming: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SupersedeCompatibilityView {
    pub kind_compatible: bool,
    pub family_compatible: bool,
    pub cycle_risk: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProposedSupersedeEdge {
    pub from: String,
    pub kind: String,
    pub to: String,
}

/// Rich supersede preview output — canonical read model.
/// Tek model, üç yüzey (standalone / TTY confirmation / wizard confirmation) render eder.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SupersedePreviewOutput {
    pub revision: u64,
    pub superseded: SupersedeEndpointPreview,
    pub successor: SupersedeEndpointPreview,
    pub lineage: SupersedeLineagePreview,
    pub compatibility: SupersedeCompatibilityView,
    pub proposed_edge: ProposedSupersedeEdge,
    /// Point-in-time structural eligibility (mutation revalidates under lock).
    pub structurally_eligible: bool,
    /// `blocking_reasons[0]` — store structural steps 5–10'daki ilk engel.
    /// CLI production path'in ilk hatasıyla aynı olmak ZORUNDA DEĞİL (self/currentness/digest
    /// precheck'leri store structural sırasına ulaşmadan dönebilir).
    pub primary_structural_blocker: Option<SupersedeBlockerCode>,
    /// `ordering_key()` ile sorted — deterministic.
    pub blocking_reasons: Vec<SupersedeBlocker>,
}

impl SupersedePreviewOutput {
    /// İki endpoint'in digest'leri — confirmation mutation'ın `expected` precondition'ı için.
    /// Point-in-time; mutation lock altında iki-digest recheck yapar (preview ≠ guarantee).
    /// `node_digest_hex` her zaman bizim `{:016x}` formatımızdan gelir → infallible parse.
    pub fn digests(&self) -> SupersedeDigests {
        let superseded = u64::from_str_radix(&self.superseded.node_digest_hex, 16)
            .map(NodeDigest::from_raw)
            .expect("preview node_digest_hex is our own {:016x} output");
        let successor = u64::from_str_radix(&self.successor.node_digest_hex, 16)
            .map(NodeDigest::from_raw)
            .expect("preview node_digest_hex is our own {:016x} output");
        SupersedeDigests {
            superseded,
            successor,
        }
    }
}

/// Lineage DAG traversal sınırları (adversarial/deep chain savunması — production sığ).
const MAX_PREVIEW_LINEAGE_DEPTH: usize = 16;
const MAX_PREVIEW_LINEAGE_NODES: usize = 128;

/// Review application service — query + mutation. Repository üzerinden persistent
/// transaction; subcommand ve interactive adapter aynı service'i kullanır.
pub struct ReviewApplicationService<R: ReviewStoreRepository> {
    repo: R,
}

impl<R: ReviewStoreRepository> ReviewApplicationService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Read-only store + revision yükler (tek read motoru — List/Show/SupersedePreview).
    fn read_validated_store(&self) -> Result<(InMemoryAnchorStore, u64), ReviewError> {
        let persisted = self.repo.read()?;
        let store = InMemoryAnchorStore::restore_snapshot(persisted.snapshot)
            .map_err(|e| ReviewError::Store(e.to_string()))?;
        Ok((store, persisted.revision))
    }

    /// Read-only query (list/show/supersede-preview). `--operator` gerekmez.
    pub fn execute_query(&self, query: ReviewQuery) -> Result<ReviewReadOutput, ReviewError> {
        let (store, revision) = self.read_validated_store()?;
        match query {
            ReviewQuery::List => {
                let candidates = store
                    .candidate_query()
                    .map_err(|e| ReviewError::Store(e.to_string()))?;
                let items: Vec<ReviewListItem> = candidates
                    .into_iter()
                    .map(|n| ReviewListItem {
                        id: n.id.0.clone(),
                        canonical: n.canonical.clone(),
                        kind: format!("{:?}", n.node_kind),
                        decision_status: format!("{:?}", n.decision_status),
                    })
                    .collect();
                Ok(ReviewReadOutput::List { items, revision })
            }
            ReviewQuery::Show(id) => {
                // Candidate lane'de ara, yoksa tüm node'larda.
                let candidates = store
                    .candidate_query()
                    .map_err(|e| ReviewError::Store(e.to_string()))?;
                let node = candidates
                    .into_iter()
                    .find(|n| &n.id == &id)
                    .or_else(|| store.graph().nodes_iter().find(|n| &n.id == &id).cloned());
                let details = node.map(|n| {
                    // Node freshness digest — tüm statülerde (Candidate/Accepted/SupersededAccepted/Rejected).
                    // Tek source of truth; Candidate review `--basis-digest`, supersede iki endpoint için.
                    let digest = node_digest(&n);
                    // SupersededAccepted ise successor'u çöz — core accessor (tek source,
                    // preview/mutation step 5 ile aynı incoming-edge policy).
                    let superseded_by = if n.decision_status
                        == osp_core::anchoring::DecisionStatus::SupersededAccepted
                    {
                        store
                            .committed_supersede_incoming_sources(&n.id)
                            .ok()
                            .and_then(|srcs| srcs.into_iter().next().map(|id| id.0))
                    } else {
                        None
                    };
                    ReviewNodeDetails {
                        id: n.id.0.clone(),
                        canonical: n.canonical.clone(),
                        kind: format!("{:?}", n.node_kind),
                        decision_status: format!("{:?}", n.decision_status),
                        node_digest_hex: format!("{:016x}", digest.get()),
                        superseded_by,
                    }
                });
                Ok(ReviewReadOutput::Show {
                    node: details,
                    revision,
                })
            }
            ReviewQuery::SupersedePreview {
                superseded,
                successor,
            } => {
                let preview = build_supersede_preview(&store, &superseded, &successor, revision)?;
                Ok(ReviewReadOutput::SupersedePreview(preview))
            }
        }
    }

    /// Rich supersede preview convenience entrypoint — `execute_query`'yi sarmalar (tek read).
    pub fn execute_supersede_preview(
        &self,
        superseded: ConceptNodeId,
        successor: ConceptNodeId,
    ) -> Result<SupersedePreviewOutput, ReviewError> {
        match self.execute_query(ReviewQuery::SupersedePreview {
            superseded,
            successor,
        })? {
            ReviewReadOutput::SupersedePreview(output) => Ok(output),
            _ => unreachable!("query/output variant mismatch"),
        }
    }

    /// Mutation (accept/reject). `expected_basis_digest` precondition + session accept/reject.
    pub fn execute_mutation(
        &self,
        command: ReviewMutationCommand,
        operator: OperatorId,
    ) -> Result<PersistedReviewOutput, ReviewError> {
        self.repo
            .mutate(|store| match command {
                ReviewMutationCommand::Accept {
                    id,
                    expected_basis_digest,
                    reason,
                } => {
                    let record = apply_review(
                        store,
                        &id,
                        expected_basis_digest,
                        reason,
                        operator.clone(),
                        true, // accept
                    )?;
                    Ok(ReviewMutation {
                        status: "accepted".into(),
                        node_id: record.candidate_id.0.clone(),
                        decision_sequence: record.seq,
                    })
                }
                ReviewMutationCommand::Reject {
                    id,
                    expected_basis_digest,
                    reason,
                } => {
                    let record = apply_review(
                        store,
                        &id,
                        expected_basis_digest,
                        reason,
                        operator.clone(),
                        false, // reject
                    )?;
                    Ok(ReviewMutation {
                        status: "rejected".into(),
                        node_id: record.candidate_id.0.clone(),
                        decision_sequence: record.seq,
                    })
                }
            })
            .map(|(mutation, revision)| PersistedReviewOutput { mutation, revision })
    }

    /// Supersede mutation (ayrı komut/output — accept/reject'i kirletmez). `repository.mutate`
    /// generic; iki-digest precondition `apply_supersede` closure içinde.
    pub fn execute_supersede(
        &self,
        command: SupersedeCommand,
        operator: OperatorId,
    ) -> Result<PersistedSupersedeOutput, ReviewError> {
        self.repo
            .mutate(|store| {
                let record = apply_supersede(store, &command, operator.clone())?;
                Ok(ReviewSupersedeMutation {
                    status: "superseded".into(),
                    superseded_node_id: record.superseded.0.clone(),
                    successor_node_id: record.successor.0.clone(),
                    decision_sequence: record.seq,
                })
            })
            .map(|(mutation, revision)| PersistedSupersedeOutput { mutation, revision })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// build_supersede_preview — canonical preview builder (tek source: core accessor/predicate'lar)
// ═══════════════════════════════════════════════════════════════════════════════

/// Rich supersede preview üretir. Non-Accepted endpoint'ler blocking_reason olarak raporlanır
/// (hard error DEĞİL — missing node → NotFound hard error kalır). Self-supersede dahil tüm
/// ineligible durumlar blocker-bearing preview üretir.
///
/// **Tüm domain policy core accessor/predicate'lardan:**
/// - incoming → `committed_supersede_incoming_sources` (core step 5)
/// - currentness → `is_current_mainline()` (core step 6-7)
/// - compatibility → `inspect_supersede_compatibility` (core step 9)
/// - cycle → `would_create_supersede_cycle` (core step 10; self'de bastırılır)
fn build_supersede_preview(
    store: &InMemoryAnchorStore,
    superseded: &ConceptNodeId,
    successor: &ConceptNodeId,
    revision: u64,
) -> Result<SupersedePreviewOutput, ReviewError> {
    let sup_node = store
        .graph()
        .node(superseded)
        .ok_or_else(|| ReviewError::NotFound(superseded.0.clone()))?;
    let suc_node = store
        .graph()
        .node(successor)
        .ok_or_else(|| ReviewError::NotFound(successor.0.clone()))?;

    let self_supersede = superseded == successor;

    // Core accessor/predicate'ler — tek source (divergence mekanik olarak engellenir).
    let incoming = store
        .committed_supersede_incoming_sources(superseded)
        .map_err(|e| ReviewError::Store(e.to_string()))?;
    let compat = store
        .inspect_supersede_compatibility(superseded, successor)
        .map_err(|e| ReviewError::Store(e.to_string()))?;
    let cycle_risk = if self_supersede {
        false // self blocker (step 8) cycle'dan (step 10) önce; cycle bastırılır.
    } else {
        store
            .would_create_supersede_cycle(superseded, successor)
            .map_err(|e| ReviewError::Store(e.to_string()))?
    };

    // Raw blockers topla (mutation structural steps 5–10 sırasıyla eklenir, sonra sort).
    let mut blockers: Vec<SupersedeBlocker> = Vec::new();
    if !incoming.is_empty() {
        blockers.push(SupersedeBlocker {
            code: SupersedeBlockerCode::AlreadySuperseded,
            message: format!(
                "{} is already superseded by {}",
                superseded.0,
                incoming[0].0
            ),
        });
    }
    if !sup_node.decision_status.is_current_mainline() {
        blockers.push(SupersedeBlocker {
            code: SupersedeBlockerCode::SupersededNotCurrent,
            message: format!(
                "superseded endpoint is {:?} (not current Accepted)",
                sup_node.decision_status
            ),
        });
    }
    if !suc_node.decision_status.is_current_mainline() {
        blockers.push(SupersedeBlocker {
            code: SupersedeBlockerCode::SuccessorNotCurrent,
            message: format!(
                "successor endpoint is {:?} (not current Accepted)",
                suc_node.decision_status
            ),
        });
    }
    if self_supersede {
        blockers.push(SupersedeBlocker {
            code: SupersedeBlockerCode::SelfSupersede,
            message: "a node cannot supersede itself".into(),
        });
    }
    if !compat.kind_compatible {
        blockers.push(SupersedeBlocker {
            code: SupersedeBlockerCode::IncompatibleKind,
            message: format!(
                "kind mismatch: superseded={:?}, successor={:?}",
                sup_node.node_kind, suc_node.node_kind
            ),
        });
    }
    if !compat.family_compatible {
        blockers.push(SupersedeBlocker {
            code: SupersedeBlockerCode::IncompatibleFamily,
            message: format!(
                "family mismatch: superseded={:?}, successor={:?}",
                sup_node.position_family, suc_node.position_family
            ),
        });
    }
    if cycle_risk {
        blockers.push(SupersedeBlocker {
            code: SupersedeBlockerCode::Cycle,
            message: format!(
                "the proposed edge {} → {} would close an existing cycle",
                successor.0, superseded.0
            ),
        });
    }

    // Deterministik sıralama — ordering_key (mutation step + tie-break).
    blockers.sort_by_key(|b| b.code.ordering_key());

    let structurally_eligible = blockers.is_empty();
    let primary_structural_blocker = blockers.first().map(|b| b.code);

    let superseded_preview = endpoint_preview(sup_node);
    let successor_preview = endpoint_preview(suc_node);

    // Lineage HER ZAMAN (self dahil) successor'dan üretilir.
    let lineage = build_successor_lineage(
        store,
        successor,
        &incoming,
        MAX_PREVIEW_LINEAGE_DEPTH,
        MAX_PREVIEW_LINEAGE_NODES,
    );

    Ok(SupersedePreviewOutput {
        revision,
        superseded: superseded_preview,
        successor: successor_preview,
        lineage,
        compatibility: SupersedeCompatibilityView {
            kind_compatible: compat.kind_compatible,
            family_compatible: compat.family_compatible,
            cycle_risk,
        },
        proposed_edge: ProposedSupersedeEdge {
            from: successor.0.clone(),
            kind: "Supersedes".into(),
            to: superseded.0.clone(),
        },
        structurally_eligible,
        primary_structural_blocker,
        blocking_reasons: blockers,
    })
}

/// ConceptNode → SupersedeEndpointPreview.
fn endpoint_preview(node: &osp_core::anchoring::types::ConceptNode) -> SupersedeEndpointPreview {
    let digest = node_digest(node);
    SupersedeEndpointPreview {
        id: node.id.0.clone(),
        canonical: node.canonical.clone(),
        kind: format!("{:?}", node.node_kind),
        status: format!("{:?}", node.decision_status),
        family: format!("{:?}", node.position_family),
        node_digest_hex: format!("{:016x}", digest.get()),
    }
}

/// Successor outgoing committed Supersedes lineage — bounded DAG (BFS, deterministic).
///
/// Closed-output: her edge from/to output nodes içinde. Diamond preservation: target daha önce
/// görülmüş olsa da edge korunur. Truncation: dahil edilemeyen committed outgoing edge varsa Some.
fn build_successor_lineage(
    store: &InMemoryAnchorStore,
    root: &ConceptNodeId,
    superseded_incoming_sources: &[ConceptNodeId],
    max_depth: usize,
    max_nodes: usize,
) -> SupersedeLineagePreview {
    use osp_core::anchoring::{ConceptEdgeKind, DecisionStatus};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    // Adjacency: from → sorted to set (committed Supersedes outgoing, deterministic).
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for e in store.graph().edges() {
        if e.kind == ConceptEdgeKind::Supersedes && e.decision_status == DecisionStatus::Accepted {
            adjacency
                .entry(e.from.0.clone())
                .or_default()
                .insert(e.to.0.clone());
        }
    }

    // BFS: visited yalnız enqueue engeller; edge set target görülmüş olsa da korunur (diamond).
    let mut nodes: Vec<SupersedeLineageNode> = Vec::new();
    let mut edges: Vec<SupersedeLineageEdge> = Vec::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut truncated_by_depth = false;
    let mut truncated_by_nodes = false;

    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    queue.push_back((root.0.clone(), 0));
    visited.insert(root.0.clone());

    while let Some((current, depth)) = queue.pop_front() {
        if nodes.len() >= max_nodes {
            truncated_by_nodes = true;
            break;
        }
        nodes.push(SupersedeLineageNode {
            id: current.clone(),
            depth,
        });

        if depth >= max_depth {
            // Bu node'un outgoing edge'leri depth limit yüzünden dahil edilemedi.
            if adjacency.contains_key(&current) {
                truncated_by_depth = true;
            }
            continue;
        }

        if let Some(targets) = adjacency.get(&current) {
            for target in targets {
                // Closed-output invariant: edge ancak her iki ucu output nodes'ta olacaksa eklenir.
                // target zaten visited (önceki BFS adımda output'a girdi) → edge güvenle ekle.
                if visited.contains(target) {
                    edges.push(SupersedeLineageEdge {
                        from: current.clone(),
                        to: target.clone(),
                    });
                    continue;
                }
                // Yeni target — ancak node kapasitesi varsa kabul et (visited + queue'ya).
                // Kapasite yoksa target output'a girmez → edge de eklenmez (closed-output).
                if nodes.len() + queue.len() >= max_nodes {
                    truncated_by_nodes = true;
                    continue;
                }
                visited.insert(target.clone());
                queue.push_back((target.clone(), depth + 1));
                edges.push(SupersedeLineageEdge {
                    from: current.clone(),
                    to: target.clone(),
                });
            }
        }
    }

    // Final deterministic sort.
    nodes.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.id.cmp(&b.id)));
    edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));

    let truncation = match (truncated_by_depth, truncated_by_nodes) {
        (true, true) => Some(LineageTruncation::Both),
        (true, false) => Some(LineageTruncation::DepthLimit),
        (false, true) => Some(LineageTruncation::NodeLimit),
        (false, false) => None,
    };

    SupersedeLineagePreview {
        root: root.0.clone(),
        nodes,
        edges,
        truncation,
        max_depth,
        max_nodes,
        superseded_incoming: superseded_incoming_sources.first().map(|id| id.0.clone()),
    }
}

/// Basis-freshness precondition + session accept/reject (lock altında — yeni TOCTOU yok).
///
/// `expected_basis_digest` operator'ın gördüğü basis; current ile karşılaştırılır.
/// Eşleşirse gerçek `PresentedBasis::compile` + session. Session'ın StaleBasis'i
/// tautolojik (lock altında digest zaten kontrol edildi) ama zararsız.
fn apply_review(
    store: &mut InMemoryAnchorStore,
    id: &ConceptNodeId,
    expected_basis_digest: NodeDigest,
    reason: String,
    operator: OperatorId,
    accept: bool,
) -> Result<osp_core::anchoring::review::DecisionRecord, ReviewError> {
    // Node'u bul (candidate_query içinde — Candidate-only v1 scope).
    let candidates = store
        .candidate_query()
        .map_err(|e| ReviewError::Store(e.to_string()))?;
    let node = candidates
        .into_iter()
        .find(|n| &n.id == id)
        .ok_or_else(|| ReviewError::NotFound(id.0.clone()))?;

    // Basis-freshness precondition: operator'ın gördüğü digest ile current aynı mı?
    let current_digest = node_digest(&node);
    if current_digest != expected_basis_digest {
        return Err(ReviewError::StaleBasis);
    }

    // Gerçek PresentedBasis (digest precondition sonrası — kapıyı zayıflatmaz).
    let basis =
        PresentedBasis::compile(store, id).map_err(|e| ReviewError::Store(e.to_string()))?;
    let reason = NonEmptyExplanation::new(reason).map_err(|e| ReviewError::Store(e.to_string()))?;

    let mut session = OperatorReviewSession::open_for_operator(operator);
    if accept {
        session
            .accept(store, id, basis, reason)
            .map_err(map_review_error)
    } else {
        session
            .reject(store, id, basis, reason)
            .map_err(map_review_error)
    }
}

/// osp-core `ReviewError` → CLI `ReviewError` map (tautolojik StaleBasis dahil tek mesaja).
fn map_review_error(e: osp_core::anchoring::review::ReviewError) -> ReviewError {
    use osp_core::anchoring::review::ReviewError as CoreErr;
    match e {
        CoreErr::NotFound(id) => ReviewError::NotFound(id.0),
        CoreErr::StaleBasis { .. } => ReviewError::StaleBasis,
        CoreErr::NotPromotable { current } => ReviewError::NotPromotable(format!("{current:?}")),
        other => ReviewError::Store(other.to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Supersession — apply_supersede + error mapper (R1#2 + E1/G1)
// ═══════════════════════════════════════════════════════════════════════════════

/// `SupersedeError → ReviewError` map. Endpoint-specific stale + NotFound/NotCurrent ayrımı
/// + store-level typed (downcast, G1 4 family alanı, source string fallback — E1).
fn map_supersede_error(e: SupersedeError) -> ReviewError {
    use osp_core::anchoring::review::SupersedeError as E;
    match e {
        E::StaleSupersededBasis { .. } => ReviewError::StaleSupersededBasis,
        E::StaleSuccessorBasis { .. } => ReviewError::StaleSuccessorBasis,
        // N2: core fallback — lock altında tautological (CLI precheck önce); status yok.
        E::SupersededNotCurrent(id) => ReviewError::EndpointNotCurrent {
            endpoint: SupersedeEndpoint::Superseded,
            id: id.0,
            formatted_status: format_endpoint_status(None),
        },
        E::SuccessorNotCurrent(id) => ReviewError::EndpointNotCurrent {
            endpoint: SupersedeEndpoint::Successor,
            id: id.0,
            formatted_status: format_endpoint_status(None),
        },
        E::SelfSupersede(id) => ReviewError::SelfSupersede(id.0),
        E::Store(source) => map_supersede_store_error(source),
        // BasisMismatch/SessionCounterExhausted — Display açıklayıcı, defense-in-depth.
        other => ReviewError::Store(other.to_string()),
    }
}

/// Store-level supersede hataları → typed (E1 downcast). `SupersedeError::Store` Display
/// "store error" (source'u enterpole ETMİYOR) — bu yüzden source'u downcast/string ile al.
/// G2: downcast kanıtlanmış (review.rs:2336 mevcut test pattern).
fn map_supersede_store_error(source: Box<dyn std::error::Error + Send + Sync>) -> ReviewError {
    if let Some(err) = source.downcast_ref::<StoreError>() {
        match err {
            StoreError::AlreadySuperseded(id) => {
                return ReviewError::AlreadySuperseded(id.0.clone());
            }
            // G1: 4 alan (kind×2 + family×2) — family-kaynaklı uyumsuzluk yakalanır.
            StoreError::IncompatibleSupersedeEndpoints {
                superseded_kind,
                successor_kind,
                superseded_family,
                successor_family,
            } => {
                return ReviewError::IncompatibleSupersedeEndpoints {
                    superseded_kind: format!("{superseded_kind:?}"),
                    successor_kind: format!("{successor_kind:?}"),
                    superseded_family: format!("{superseded_family:?}"),
                    successor_family: format!("{successor_family:?}"),
                };
            }
            StoreError::SupersedeCycle {
                superseded,
                successor,
            } => {
                return ReviewError::SupersedeCycle {
                    superseded: superseded.0.clone(),
                    successor: successor.0.clone(),
                };
            }
            _ => {}
        }
    }
    // Fallback: source string (NOT e.to_string() — E1; AuditSequenceExhausted vb. nadir).
    ReviewError::Store(source.to_string())
}

/// Endpoint missing (NotFound) vs existing-but-non-Accepted (EndpointNotCurrent) ayrımı (R1#2).
/// mainline_query Accepted node'ları döner; node orada yoksa graph'ta ara → status ile NotCurrent.
fn endpoint_not_current_or_missing(
    store: &InMemoryAnchorStore,
    id: &ConceptNodeId,
    endpoint: SupersedeEndpoint,
) -> ReviewError {
    if let Some(n) = store.graph().nodes_iter().find(|n| &n.id == id) {
        ReviewError::EndpointNotCurrent {
            endpoint,
            id: id.0.clone(),
            formatted_status: format_endpoint_status(Some(&format!("{:?}", n.decision_status))),
        }
    } else {
        ReviewError::NotFound(id.0.clone())
    }
}

/// Supersede domain transition (R1 akış + R3#4 açık `let mut session`).
///
/// Akış: early SelfSupersede → mainline_query (Accepted) → endpoint resolve (missing/NotCurrent)
/// → iki-digest precondition (endpoint-specific stale) → PresentedSupersedeBasis::compile
/// → SupersedeSession (authority içeride mint).
///
/// R4 comment: digest *içerik* tazeliğini korur; *currency*'yi (hâlâ Accepted mı) compile'ın
/// NotCurrent kontrolü korur (lock altında tautological ama defense-in-depth). Status-kör
/// digest tek başına currency garantisi vermez — compile atlamayın.
fn apply_supersede(
    store: &mut InMemoryAnchorStore,
    cmd: &SupersedeCommand,
    operator: OperatorId,
) -> Result<SupersedeRecord, ReviewError> {
    if cmd.superseded == cmd.successor {
        return Err(ReviewError::SelfSupersede(cmd.superseded.0.clone()));
    }

    let current = store
        .mainline_query()
        .map_err(|e| ReviewError::Store(e.to_string()))?;
    let old = current
        .iter()
        .find(|n| &n.id == &cmd.superseded)
        .ok_or_else(|| {
            endpoint_not_current_or_missing(store, &cmd.superseded, SupersedeEndpoint::Superseded)
        })?;
    let new = current
        .iter()
        .find(|n| &n.id == &cmd.successor)
        .ok_or_else(|| {
            endpoint_not_current_or_missing(store, &cmd.successor, SupersedeEndpoint::Successor)
        })?;

    if node_digest(old) != cmd.expected.superseded {
        return Err(ReviewError::StaleSupersededBasis);
    }
    if node_digest(new) != cmd.expected.successor {
        return Err(ReviewError::StaleSuccessorBasis);
    }

    let basis = PresentedSupersedeBasis::compile(store, &cmd.superseded, &cmd.successor)
        .map_err(map_supersede_error)?;
    let reason = NonEmptyExplanation::new(cmd.reason.clone())
        .map_err(|e| ReviewError::Store(e.to_string()))?;

    let mut session = SupersedeSession::open_for_operator(operator);
    session
        .supersede(store, &cmd.superseded, &cmd.successor, basis, reason)
        .map_err(map_supersede_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use osp_core::anchoring::review::SupersedeError;
    use osp_core::anchoring::store::StoreError;
    use osp_core::anchoring::types::ConceptNodeId;

    /// `Store(Box::new(AlreadySuperseded))` → typed AlreadySuperseded (E1 downcast).
    #[test]
    fn map_supersede_error_already_superseded_typed() {
        let err = map_supersede_error(SupersedeError::Store(Box::new(
            StoreError::AlreadySuperseded(ConceptNodeId("RuleCandidate:X".into())),
        )));
        assert!(
            matches!(err, ReviewError::AlreadySuperseded(ref id) if id == "RuleCandidate:X"),
            "got {err:?}"
        );
    }

    /// IncompatibleSupersedeEndpoints — 4 alan (G1: family dahil). Farklı family, aynı kind.
    #[test]
    fn map_supersede_error_incompatible_carries_family() {
        use osp_core::anchoring::{ConceptNodeKind, PositionFamily};
        let err = map_supersede_error(SupersedeError::Store(Box::new(
            StoreError::IncompatibleSupersedeEndpoints {
                superseded_kind: ConceptNodeKind::RuleCandidate,
                successor_kind: ConceptNodeKind::RuleCandidate, // aynı kind
                superseded_family: PositionFamily::ConceptualIntent,
                successor_family: PositionFamily::PhysicalCode, // farklı family
            },
        )));
        match err {
            ReviewError::IncompatibleSupersedeEndpoints {
                superseded_family,
                successor_family,
                ..
            } => {
                assert!(superseded_family.contains("ConceptualIntent"));
                assert!(successor_family.contains("PhysicalCode"));
            }
            other => panic!("expected IncompatibleSupersedeEndpoints, got {other:?}"),
        }
    }

    /// SupersedeCycle → typed.
    #[test]
    fn map_supersede_error_cycle_typed() {
        let err = map_supersede_error(SupersedeError::Store(Box::new(
            StoreError::SupersedeCycle {
                superseded: ConceptNodeId("RuleCandidate:A".into()),
                successor: ConceptNodeId("RuleCandidate:B".into()),
            },
        )));
        assert!(
            matches!(err, ReviewError::SupersedeCycle { ref superseded, ref successor }
                if superseded == "RuleCandidate:A" && successor == "RuleCandidate:B"),
            "got {err:?}"
        );
    }

    /// Fallback source mesajı korunuyor (R3#6) — AuditSequenceExhausted typed değil ama
    /// source string "store error"a çökmemeli (E1).
    #[test]
    fn map_supersede_error_fallback_preserves_source_message() {
        let err = map_supersede_error(SupersedeError::Store(Box::new(
            StoreError::AuditSequenceExhausted,
        )));
        match err {
            ReviewError::Store(msg) => {
                assert!(
                    msg.contains("audit sequence exhausted"),
                    "fallback should preserve source message, got: {msg}"
                );
            }
            other => panic!("expected Store fallback, got {other:?}"),
        }
    }

    /// Endpoint-specific stale (R1#4).
    #[test]
    fn map_supersede_error_endpoint_specific_stale() {
        let err = map_supersede_error(SupersedeError::StaleSupersededBasis {
            expected: NodeDigest::from_raw(1),
            found: NodeDigest::from_raw(2),
        });
        assert!(matches!(err, ReviewError::StaleSupersededBasis));

        let err = map_supersede_error(SupersedeError::StaleSuccessorBasis {
            expected: NodeDigest::from_raw(1),
            found: NodeDigest::from_raw(2),
        });
        assert!(matches!(err, ReviewError::StaleSuccessorBasis));
    }

    /// SelfSupersede → typed.
    #[test]
    fn map_supersede_error_self_supersede_typed() {
        let err = map_supersede_error(SupersedeError::SelfSupersede(ConceptNodeId(
            "RuleCandidate:X".into(),
        )));
        assert!(matches!(err, ReviewError::SelfSupersede(ref id) if id == "RuleCandidate:X"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Rich SupersedePreview builder unit tests
    // ═══════════════════════════════════════════════════════════════════════════

    use osp_core::anchoring::review::{
        OperatorId, PresentedSupersedeBasis, SupersedeSession,
    };
    use osp_core::anchoring::types::{ConceptNode, ConceptNodeKind, GraphSeed};
    use osp_core::anchoring::{DecisionStatus, PositionFamily};

    /// Test yardımcı: iki Accepted node'lu store.
    fn preview_store_two_accepted() -> InMemoryAnchorStore {
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
        InMemoryAnchorStore::with_seed(seed)
    }

    fn supersede_in_place(store: &mut InMemoryAnchorStore, superseded: &str, successor: &str) {
        let sup = ConceptNodeId(superseded.into());
        let suc = ConceptNodeId(successor.into());
        let basis = PresentedSupersedeBasis::compile(store, &sup, &suc).expect("basis");
        let reason = NonEmptyExplanation::new("t").unwrap();
        let mut session = SupersedeSession::open_for_operator(OperatorId::new("t"));
        session.supersede(store, &sup, &suc, basis, reason).expect("supersede");
    }

    fn accepted_node(id: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        }
    }

    /// Mutlu yol: iki Accepted, compatible, no cycle → eligible, no blockers.
    #[test]
    fn preview_happy_path_eligible_no_blockers() {
        let store = preview_store_two_accepted();
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:Old".into()),
            &ConceptNodeId("RuleCandidate:New".into()),
            1,
        )
        .unwrap();
        assert!(preview.structurally_eligible);
        assert!(preview.blocking_reasons.is_empty());
        assert_eq!(preview.primary_structural_blocker, None);
        assert!(preview.compatibility.kind_compatible);
        assert!(preview.compatibility.family_compatible);
        assert!(!preview.compatibility.cycle_risk);
    }

    /// Self-supersede → SelfSupersede blocker, cycle bastırılır, lineage yine üretilir.
    #[test]
    fn preview_self_supersede_blocker_with_lineage() {
        let store = preview_store_two_accepted();
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:Old".into()),
            &ConceptNodeId("RuleCandidate:Old".into()),
            1,
        )
        .unwrap();
        assert!(!preview.structurally_eligible);
        assert_eq!(
            preview.primary_structural_blocker,
            Some(SupersedeBlockerCode::SelfSupersede)
        );
        // Self blocker varken cycle_risk false (bastırılır).
        assert!(!preview.compatibility.cycle_risk);
        // Lineage yine üretildi (self dahil her durumda).
        assert_eq!(preview.lineage.root, "RuleCandidate:Old");
    }

    /// Already-superseded → AlreadySuperseded blocker (multi-blocker; primary).
    /// INV-C15 kuplajı: incoming → SupersededAccepted → superseded_not_current de tetiklenir.
    #[test]
    fn preview_already_superseded_primary_blocker() {
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(accepted_node("RuleCandidate:Old"));
        seed.rule_candidates.push(accepted_node("RuleCandidate:New"));
        seed.rule_candidates.push(accepted_node("RuleCandidate:Newer"));
        let mut store = InMemoryAnchorStore::with_seed(seed);
        supersede_in_place(&mut store, "RuleCandidate:Old", "RuleCandidate:New"); // New→Old committed
        // Preview: supersede Old (already superseded) → AlreadySuperseded primary.
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:Old".into()),
            &ConceptNodeId("RuleCandidate:Newer".into()),
            3,
        )
        .unwrap();
        assert!(!preview.structurally_eligible);
        // INV-C15 kuplajı: already_superseded (step 5) primary; superseded_not_current (step 6) ek.
        assert_eq!(
            preview.primary_structural_blocker,
            Some(SupersedeBlockerCode::AlreadySuperseded)
        );
        assert!(preview.blocking_reasons.iter().any(|b| b.code == SupersedeBlockerCode::AlreadySuperseded));
        assert!(preview.blocking_reasons.iter().any(|b| b.code == SupersedeBlockerCode::SupersededNotCurrent));
        // superseded_incoming accessor'dan beslenir.
        assert_eq!(
            preview.lineage.superseded_incoming,
            Some("RuleCandidate:New".into())
        );
    }

    /// Incompatible kind → IncompatibleKind blocker (core predicate).
    #[test]
    fn preview_incompatible_kind_blocker() {
        let mut old = accepted_node("RuleCandidate:Old");
        old.node_kind = ConceptNodeKind::RuleCandidate;
        let mut new = accepted_node("Concept:New");
        new.node_kind = ConceptNodeKind::Concept; // diff kind
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(old);
        seed.concepts.push(new);
        let store = InMemoryAnchorStore::with_seed(seed);
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:Old".into()),
            &ConceptNodeId("Concept:New".into()),
            1,
        )
        .unwrap();
        assert!(!preview.structurally_eligible);
        assert_eq!(
            preview.primary_structural_blocker,
            Some(SupersedeBlockerCode::IncompatibleKind)
        );
        assert!(!preview.compatibility.kind_compatible);
    }

    /// Incompatible family → IncompatibleFamily blocker (core predicate; seed hard-code → direct store).
    #[test]
    fn preview_incompatible_family_blocker() {
        let mut old = accepted_node("RuleCandidate:Old");
        old.position_family = PositionFamily::ConceptualIntent;
        let mut new = accepted_node("RuleCandidate:New");
        new.position_family = PositionFamily::PhysicalCode; // diff family
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(old);
        seed.rule_candidates.push(new);
        let store = InMemoryAnchorStore::with_seed(seed);
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:Old".into()),
            &ConceptNodeId("RuleCandidate:New".into()),
            1,
        )
        .unwrap();
        assert!(!preview.structurally_eligible);
        assert_eq!(
            preview.primary_structural_blocker,
            Some(SupersedeBlockerCode::IncompatibleFamily)
        );
        assert!(!preview.compatibility.family_compatible);
    }

    /// Superseded non-Accepted → SupersededNotCurrent blocker.
    #[test]
    fn preview_superseded_not_current_blocker() {
        let mut old = accepted_node("RuleCandidate:Old");
        old.decision_status = DecisionStatus::Rejected;
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(old);
        seed.rule_candidates.push(accepted_node("RuleCandidate:New"));
        let store = InMemoryAnchorStore::with_seed(seed);
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:Old".into()),
            &ConceptNodeId("RuleCandidate:New".into()),
            1,
        )
        .unwrap();
        assert!(!preview.structurally_eligible);
        assert_eq!(
            preview.primary_structural_blocker,
            Some(SupersedeBlockerCode::SupersededNotCurrent)
        );
    }

    /// Cycle: existing committed edge → prospektif cycle. A→B committed, preview B→A.
    /// cycle ek blocker; A artık SupersededAccepted → successor_not_current da tetiklenir.
    #[test]
    fn preview_cycle_reports_prospective_cycle() {
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(accepted_node("RuleCandidate:A"));
        seed.rule_candidates.push(accepted_node("RuleCandidate:B"));
        let mut store = InMemoryAnchorStore::with_seed(seed);
        supersede_in_place(&mut store, "RuleCandidate:A", "RuleCandidate:B"); // committed B→A
        // Preview: supersede B (target), successor A → proposed A→B; B→A mevcut → cycle.
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:B".into()), // superseded
            &ConceptNodeId("RuleCandidate:A".into()), // successor (artık SupersededAccepted)
            2,
        )
        .unwrap();
        assert!(!preview.structurally_eligible);
        assert!(preview.compatibility.cycle_risk);
        assert!(preview.blocking_reasons.iter().any(|b| b.code == SupersedeBlockerCode::Cycle));
    }

    /// Lineage chain: supersede(A, B) → B→A committed. Preview successor=B → [B@0, A@1].
    #[test]
    fn preview_lineage_chain() {
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(accepted_node("RuleCandidate:A"));
        seed.rule_candidates.push(accepted_node("RuleCandidate:B"));
        let mut store = InMemoryAnchorStore::with_seed(seed);
        supersede_in_place(&mut store, "RuleCandidate:A", "RuleCandidate:B"); // B→A
        // Preview successor=B (outgoing chain [B, A]); superseded=A ineligible ama lineage gösterilir.
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:A".into()),
            &ConceptNodeId("RuleCandidate:B".into()),
            2,
        )
        .unwrap();
        assert_eq!(preview.lineage.root, "RuleCandidate:B");
        let ids: Vec<_> = preview.lineage.nodes.iter().map(|n| n.id.clone()).collect();
        assert_eq!(ids, vec!["RuleCandidate:B", "RuleCandidate:A"]);
    }

    /// Lineage consolidation: C→A, C→B (bir successor iki supersede). Branching preserved.
    #[test]
    fn preview_lineage_consolidation() {
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(accepted_node("RuleCandidate:A"));
        seed.rule_candidates.push(accepted_node("RuleCandidate:B"));
        seed.rule_candidates.push(accepted_node("RuleCandidate:C"));
        let mut store = InMemoryAnchorStore::with_seed(seed);
        supersede_in_place(&mut store, "RuleCandidate:A", "RuleCandidate:C"); // C→A
        supersede_in_place(&mut store, "RuleCandidate:B", "RuleCandidate:C"); // C→B
        // Preview successor=C → outgoing [A, B]; superseded=A ineligible ama lineage gösterilir.
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:A".into()),
            &ConceptNodeId("RuleCandidate:C".into()),
            3,
        )
        .unwrap();
        assert_eq!(preview.lineage.root, "RuleCandidate:C");
        let node_ids: Vec<_> = preview.lineage.nodes.iter().map(|n| n.id.clone()).collect();
        assert!(node_ids.contains(&"RuleCandidate:C".to_string()));
        assert!(node_ids.contains(&"RuleCandidate:A".to_string()));
        assert!(node_ids.contains(&"RuleCandidate:B".to_string()));
        // C→A, C→B edges preserved (branching).
        let edge_pairs: Vec<_> = preview
            .lineage
            .edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        assert!(edge_pairs.contains(&("RuleCandidate:C".into(), "RuleCandidate:A".into())));
        assert!(edge_pairs.contains(&("RuleCandidate:C".into(), "RuleCandidate:B".into())));
    }

    /// Missing endpoint → NotFound hard error.
    #[test]
    fn preview_missing_endpoint_not_found() {
        let store = preview_store_two_accepted();
        let err = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:MISSING".into()),
            &ConceptNodeId("RuleCandidate:New".into()),
            1,
        )
        .unwrap_err();
        assert!(matches!(err, ReviewError::NotFound(_)));
    }

    /// Node-limit closed-output regression (Review P2-a): node cap aşımında excluded target'a
    /// edge kalmaz. Küçük max_nodes (3) ile 4-node chain — 4. node excluded, ona edge yok.
    #[test]
    fn preview_lineage_node_limit_excludes_target_edges() {
        // Chain: N0 ← N1 ← N2 ← N3 (N1→N0, N2→N1, N3→N2 committed). Successor=N3.
        // max_nodes=3 → N3@0, N2@1, N1@2 dahil; N0 excluded. N1→N0 edge'inin de çıkarılması gerek.
        let mk = |id: &str| ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(mk("RuleCandidate:N0"));
        seed.rule_candidates.push(mk("RuleCandidate:N1"));
        seed.rule_candidates.push(mk("RuleCandidate:N2"));
        seed.rule_candidates.push(mk("RuleCandidate:N3"));
        let mut store = InMemoryAnchorStore::with_seed(seed);
        supersede_in_place(&mut store, "RuleCandidate:N0", "RuleCandidate:N1"); // N1→N0
        supersede_in_place(&mut store, "RuleCandidate:N1", "RuleCandidate:N2"); // N2→N1
        supersede_in_place(&mut store, "RuleCandidate:N2", "RuleCandidate:N3"); // N3→N2
        let lineage = build_successor_lineage(
            &store,
            &ConceptNodeId("RuleCandidate:N3".into()),
            &[],
            16,
            3, // max_nodes — N0'u exclude etmek için
        );
        assert_eq!(lineage.nodes.len(), 3, "node cap = 3 → exactly 3 nodes");
        assert_eq!(lineage.truncation, Some(LineageTruncation::NodeLimit));
        // Closed-output: excluded N0'a hiçbir edge.
        let node_ids: std::collections::BTreeSet<String> =
            lineage.nodes.iter().map(|n| n.id.clone()).collect();
        assert!(!node_ids.contains("RuleCandidate:N0"), "N0 must be excluded");
        for e in &lineage.edges {
            assert!(
                node_ids.contains(&e.from) && node_ids.contains(&e.to),
                "closed-output violation at node limit: edge {} → {} has excluded endpoint",
                e.from,
                e.to
            );
        }
    }

    /// Closed-output invariant: lineage DAG'deki her edge from/to nodes içinde.
    /// INV-C15 altında diamond (bir node'a iki incoming) validated snapshot'ta kurulamaz
    /// (her superseded node ≤1 incoming committed edge); bu test closed-output'u chain +
    /// consolidation ile doğrular. Builder invalid/direct-store'da da invariant'ı korur.
    #[test]
    fn preview_lineage_closed_output_invariant() {
        // Consolidation: C→A, C→B (successor outgoing branching). Validated INV-C15 altında legal.
        let mk = |id: &str| ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: id.split(':').nth(1).unwrap_or(id).into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent,
        };
        let mut seed = GraphSeed::default();
        seed.rule_candidates.push(mk("RuleCandidate:A"));
        seed.rule_candidates.push(mk("RuleCandidate:B"));
        seed.rule_candidates.push(mk("RuleCandidate:C"));
        let mut store = InMemoryAnchorStore::with_seed(seed);
        supersede_in_place(&mut store, "RuleCandidate:A", "RuleCandidate:C"); // C→A
        supersede_in_place(&mut store, "RuleCandidate:B", "RuleCandidate:C"); // C→B
        let preview = build_supersede_preview(
            &store,
            &ConceptNodeId("RuleCandidate:A".into()),
            &ConceptNodeId("RuleCandidate:C".into()),
            3,
        )
        .unwrap();
        assert_eq!(preview.lineage.root, "RuleCandidate:C");
        // Closed-output: her edge from/to nodes içinde.
        let node_ids: std::collections::BTreeSet<String> =
            preview.lineage.nodes.iter().map(|n| n.id.clone()).collect();
        assert!(!preview.lineage.edges.is_empty(), "expected lineage edges");
        for e in &preview.lineage.edges {
            assert!(
                node_ids.contains(&e.from) && node_ids.contains(&e.to),
                "closed-output violation: edge {} → {} endpoint not in nodes",
                e.from,
                e.to
            );
        }
    }
}
