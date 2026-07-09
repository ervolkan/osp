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

/// Supersede confirmation presentation — iki endpoint + digests (R3#2).
/// CLI adapter seviyesinde iki `Show` sonucunu birleştiren minimal model; **`ReviewQuery`
/// değildir** (rich lineage/compatibility preview out-of-scope, sonraki PR). One-shot ve
/// interactive adapter aynı presentation'ı render eder → ayrışma yok.
#[derive(Debug, Clone)]
pub struct SupersedePresentation {
    pub superseded: ReviewNodeDetails,
    pub successor: ReviewNodeDetails,
    pub digests: SupersedeDigests,
}

/// Review application service — query + mutation. Repository üzerinden persistent
/// transaction; subcommand ve interactive adapter aynı service'i kullanır.
pub struct ReviewApplicationService<R: ReviewStoreRepository> {
    repo: R,
}

impl<R: ReviewStoreRepository> ReviewApplicationService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Read-only query (list/show). `--operator` gerekmez.
    pub fn execute_query(&self, query: ReviewQuery) -> Result<ReviewReadOutput, ReviewError> {
        let persisted = self.repo.read()?;
        let store = InMemoryAnchorStore::restore_snapshot(persisted.snapshot.clone())
            .map_err(|e| ReviewError::Store(e.to_string()))?;
        let revision = persisted.revision;
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
                    // SupersededAccepted ise successor'u çöz (committed Supersedes edge: successor→superseded).
                    let superseded_by = if n.decision_status
                        == osp_core::anchoring::DecisionStatus::SupersededAccepted
                    {
                        store
                            .graph()
                            .edges()
                            .find(|e| {
                                e.to == n.id
                                    && e.kind == osp_core::anchoring::ConceptEdgeKind::Supersedes
                                    && e.decision_status
                                        == osp_core::anchoring::DecisionStatus::Accepted
                            })
                            .map(|e| e.from.0.clone())
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

    /// Supersede confirmation presentation — iki endpoint + digests (R3#2).
    /// CLI adapter seviyesinde minimal model; **`ReviewQuery` değildir** (rich lineage/
    /// compatibility preview out-of-scope). One-shot ve interactive aynı presentation'ı render eder.
    ///
    /// **Tek snapshot read** (Review P2.1): tek `repo.read()` → iki endpoint aynı restored
    /// snapshot'tan → atomik pair consistency (revision retry gerekmez). Asıl freshness
    /// garantisi commit'teki iki-digest recheck (lock altında).
    ///
    /// **Accepted gate** (Review P1.2): her iki endpoint `Accepted` olmalı; değilse
    /// `EndpointNotCurrent` — operator'e invalid confirmation gösterilmez ("remains current
    /// Accepted" yalan söylemez). Mutation `mainline_query` kontrolünün yerine geçmez;
    /// yalnız preview doğruluğu.
    ///
    /// **Self-supersede gate** (Review 2.tur P1): `superseded == successor` presentation
    /// oluşturulmadan reddedilir — aynı node için çelişkili confirmation ("X supersedes X",
    /// hem SupersededAccepted hem current Accepted) gösterilmez. Mutation'daki early check
    /// korunur; bu yalnız preview doğruluğu.
    pub fn load_supersede_presentation(
        &self,
        superseded: &ConceptNodeId,
        successor: &ConceptNodeId,
    ) -> Result<SupersedePresentation, ReviewError> {
        if superseded == successor {
            return Err(ReviewError::SelfSupersede(superseded.0.clone()));
        }
        let persisted = self.repo.read()?;
        let store = InMemoryAnchorStore::restore_snapshot(persisted.snapshot.clone())
            .map_err(|e| ReviewError::Store(e.to_string()))?;

        let old = node_to_details(&store, superseded)?;
        let new = node_to_details(&store, successor)?;

        // Accepted gate — operator'e invalid confirmation gösterme (P1.2).
        ensure_current_endpoint(&old, SupersedeEndpoint::Superseded)?;
        ensure_current_endpoint(&new, SupersedeEndpoint::Successor)?;

        Ok(SupersedePresentation {
            digests: SupersedeDigests {
                superseded: parse_hex(&old.node_digest_hex)?,
                successor: parse_hex(&new.node_digest_hex)?,
            },
            superseded: old,
            successor: new,
        })
    }
}

/// Endpoint'in Accepted (current mainline) olduğunu doğrula (Review P1.2). Supersede iki
/// Accepted node üzerinde çalışır; presentation invalid endpoint için gösterilmez.
fn ensure_current_endpoint(
    node: &ReviewNodeDetails,
    endpoint: SupersedeEndpoint,
) -> Result<(), ReviewError> {
    if node.decision_status != "Accepted" {
        return Err(ReviewError::EndpointNotCurrent {
            endpoint,
            id: node.id.clone(),
            formatted_status: format_endpoint_status(Some(&node.decision_status)),
        });
    }
    Ok(())
}

/// Graph node → ReviewNodeDetails (digest + superseded_by çözümü).
fn node_to_details(
    store: &InMemoryAnchorStore,
    id: &ConceptNodeId,
) -> Result<ReviewNodeDetails, ReviewError> {
    let n = store
        .graph()
        .nodes_iter()
        .find(|n| &n.id == id)
        .ok_or_else(|| ReviewError::NotFound(id.0.clone()))?;
    let digest = node_digest(n);
    let superseded_by =
        if n.decision_status == osp_core::anchoring::DecisionStatus::SupersededAccepted {
            store
                .graph()
                .edges()
                .find(|e| {
                    e.to == n.id
                        && e.kind == osp_core::anchoring::ConceptEdgeKind::Supersedes
                        && e.decision_status == osp_core::anchoring::DecisionStatus::Accepted
                })
                .map(|e| e.from.0.clone())
        } else {
            None
        };
    Ok(ReviewNodeDetails {
        id: n.id.0.clone(),
        canonical: n.canonical.clone(),
        kind: format!("{:?}", n.node_kind),
        decision_status: format!("{:?}", n.decision_status),
        node_digest_hex: format!("{:016x}", digest.get()),
        superseded_by,
    })
}

/// Hex string → NodeDigest (presentation helper).
fn parse_hex(hex: &str) -> Result<NodeDigest, ReviewError> {
    u64::from_str_radix(hex, 16)
        .map(NodeDigest::from_raw)
        .map_err(|e| ReviewError::Store(format!("invalid node digest hex: {e}")))
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
}
