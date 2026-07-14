//! Lineage-aware effective projection (PR G) — packet-level derived read model.
//!
//! # Mimari merkez (4 tur plan review, sabit)
//! ```text
//! ConceptPacket:X ──ExpectedImplementation(Candidate)──→ CodeEntityCandidate:Z
//! CodeEntityCandidate:Z ──ResolvesTo(Accepted)──→ CodeEntity:W
//! ↓ (derived read model)
//! ConceptPacket:X ──ResolvedImplementationExpectation──→ CodeEntity:W
//! ```
//!
//! PR G grafiği düzeltmez; grafiğin bugün gerçekten bildiğini kayıpsız ve dürüst biçimde
//! projekte eder. `Concept → Candidate → Entity` zinciri grafte yok (edge source her zaman
//! `ConceptPacket:`); gerçek zincir packet-level.
//!
//! # Epistemik dürüstlük
//! Kaynak `ExpectedImplementation` edge `Candidate` statüde kalır (`apply_decision` edge promote
//! etmiyor — lane-sensitive separation). Acceptance `ResolvesTo`'dan gelir (Accepted candidate
//! üzerinde operator-reviewed resolution). Bu yüzden tip "EffectiveImplementation" değil
//! "ResolvedImplementationExpectation" — expectation resolve edildi, effective fact değil.
//!
//! # INV-C7 interaction
//! Derived record write path'e girmez (apply_plan değil, gate'den geçmez) — derived output için
//! INV-C7 high-stake explanation uygulanmaz. Ancak source `ExpectedImplementation` ve `ResolvesTo`
//! edge'leri high-stake; INV-C7 explanation validity canonical store ingress tarafından enforced
//! kabul edilir ve RP1 validation V1 kapsamı dışındadır. Tip ayrımı: committed `ImplementedBy`
//! edge ile derived record karışmaz.
//!
//! # N:1 relation model
//! Unique relation `(packet_id, entity_id)`. Aynı çift birden fazla candidate lineage proof
//! taşıyabilir → `lineages: Vec<ResolvedImplementationLineage>`.

use crate::anchoring::review::ResolutionRecord;
use crate::anchoring::types::{
    ConceptEdge, ConceptNode, ConceptNodeId, ConceptNodeKind, ConceptPacketId,
    InvalidConceptPacketNodeId,
};
use crate::anchoring::{ConceptEdgeKind, DecisionStatus, PositionFamily};
use std::collections::{BTreeMap, BTreeSet};

// ═══════════════════════════════════════════════════════════════════════════════
// ResolvedImplementationBasis — node + edge snapshot primitive (P1-1)
// ═══════════════════════════════════════════════════════════════════════════════

/// Node + edge snapshot — backend transaction/snapshot sınırında üretir.
///
/// **Contract:** `nodes`, `edges` ve `resolution_records` aynı logical snapshot/transaction'dan
/// gelmelidir. InMemory tek immutable borrow ile sağlar; persistent backend'ler transaction/snapshot
/// isolation ile sağlamalıdır. **Derleyici garantisi DEĞİL** — contract-level.
///
/// **Validation YOK:** Duplicate node / dangling endpoint / wrong-kind / missing record
/// kontrolleri projector ([`project_resolved_implementations`]) tarafından fail-closed yapılır.
/// Constructor yalnız owned snapshot shape'ini kurar (P1-1).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedImplementationBasis {
    nodes: Vec<ConceptNode>,
    edges: Vec<ConceptEdge>,
    /// R1a P1-1 (review tur 1) — resolution ledger snapshot. Projector Accepted ResolvesTo
    /// edge ↔ ResolutionRecord triangulation doğrular (INV-C16 projection mirror).
    resolution_records: Vec<ResolutionRecord>,
}

impl ResolvedImplementationBasis {
    /// Caller contract: nodes, edges ve resolution_records aynı logical snapshot'tan gelmelidir.
    pub fn new(
        nodes: Vec<ConceptNode>,
        edges: Vec<ConceptEdge>,
        resolution_records: Vec<ResolutionRecord>,
    ) -> Self {
        Self {
            nodes,
            edges,
            resolution_records,
        }
    }

    /// Node snapshot'ı (read-only).
    pub fn nodes(&self) -> &[ConceptNode] {
        &self.nodes
    }

    /// Edge snapshot'ı (read-only).
    pub fn edges(&self) -> &[ConceptEdge] {
        &self.edges
    }

    /// Resolution ledger snapshot'ı (read-only).
    pub fn resolution_records(&self) -> &[ResolutionRecord] {
        &self.resolution_records
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Derived read model — nested public API + guarantee ladder (P1-3 + P2)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Guarantee ladder (rustdoc — P2):
//   DerivedEdgeReference::new              → yalnız triple shape garanti eder.
//   ResolvedImplementationLineage::try_new → edge kind + candidate endpoint consistency.
//   ResolvedImplementationExpectation::try_new → packet/entity + tüm lineage consistency.
//   project_resolved_implementations       → edge'lerin snapshot'ta bulunması + basis structural validity.

/// Edge triple referansı — `(from, kind, to)`. `ConceptEdge` ID taşımadığından triple
/// edge identity olarak kullanılır (RP1 "unique canonical edge triple'ları").
///
/// **Guarantee ladder (P2):** Bu tip yalnız triple shape garanti eder. Belirli kind
/// (ExpectedImplementation/ResolvesTo) olması veya basis içinde bulunması ÜST constructor'larda
/// doğrulanır.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DerivedEdgeReference {
    from: ConceptNodeId,
    kind: ConceptEdgeKind,
    to: ConceptNodeId,
}

impl DerivedEdgeReference {
    /// Triple shape constructor — herhangi edge kind kabul eder. Validation YOK.
    pub fn new(from: ConceptNodeId, kind: ConceptEdgeKind, to: ConceptNodeId) -> Self {
        Self { from, kind, to }
    }

    /// `ConceptEdge`'den triple referans üret.
    pub fn from_edge(edge: &ConceptEdge) -> Self {
        Self {
            from: edge.from.clone(),
            kind: edge.kind,
            to: edge.to.clone(),
        }
    }

    pub fn from(&self) -> &ConceptNodeId {
        &self.from
    }

    pub fn kind(&self) -> ConceptEdgeKind {
        self.kind
    }

    pub fn to(&self) -> &ConceptNodeId {
        &self.to
    }
}

/// Lineage proof — `(packet → candidate → entity)` zincirinin tek candidate kanıtı.
///
/// **Guarantee ladder (P2):** `try_new` edge kind (ExpectedImplementation + ResolvesTo) +
/// candidate endpoint consistency doğrular. Packet/entity consistency ÜST ctor'da.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ResolvedImplementationLineage {
    candidate_id: ConceptNodeId,
    expected_implementation: DerivedEdgeReference,
    resolution: DerivedEdgeReference,
}

impl ResolvedImplementationLineage {
    /// Fallible smart constructor — edge kind + candidate endpoint consistency doğrular.
    ///
    /// Validation:
    /// - `expected_implementation.kind == ExpectedImplementation`
    /// - `resolution.kind == ResolvesTo`
    /// - `expected_implementation.to == candidate_id` (edge target = candidate)
    /// - `resolution.from == candidate_id` (edge source = candidate)
    pub fn try_new(
        candidate_id: ConceptNodeId,
        expected_implementation: DerivedEdgeReference,
        resolution: DerivedEdgeReference,
    ) -> Result<Self, ResolvedImplementationShapeError> {
        if expected_implementation.kind != ConceptEdgeKind::ExpectedImplementation {
            return Err(ResolvedImplementationShapeError::UnexpectedExpectedImplementationKind {
                found: expected_implementation.kind,
            });
        }
        if resolution.kind != ConceptEdgeKind::ResolvesTo {
            return Err(ResolvedImplementationShapeError::UnexpectedResolutionKind {
                found: resolution.kind,
            });
        }
        if expected_implementation.to != candidate_id {
            return Err(ResolvedImplementationShapeError::ExpectedTargetMismatch);
        }
        if resolution.from != candidate_id {
            return Err(ResolvedImplementationShapeError::ResolutionSourceMismatch);
        }
        Ok(Self {
            candidate_id,
            expected_implementation,
            resolution,
        })
    }

    pub fn candidate_id(&self) -> &ConceptNodeId {
        &self.candidate_id
    }

    pub fn expected_implementation(&self) -> &DerivedEdgeReference {
        &self.expected_implementation
    }

    pub fn resolution(&self) -> &DerivedEdgeReference {
        &self.resolution
    }
}

/// Derived read model — packet → entity unique relation + N lineage proof'ları.
///
/// **Epistemik durum:** Kaynak `ExpectedImplementation` Candidate statüde. Acceptance
/// `ResolvesTo`'dan gelir. Bu yüzden "EffectiveImplementation" değil
/// "ResolvedImplementationExpectation" — expectation resolve edildi, effective fact değil.
///
/// **N:1 model:** Unique relation `(packet_id, entity_id)`. Aynı çift birden fazla candidate
/// lineage proof taşıyabilir.
///
/// **Guarantee ladder (P2):** `try_new` packet/entity + tüm lineage consistency doğrular.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ResolvedImplementationExpectation {
    packet_id: ConceptPacketId,
    entity_id: ConceptNodeId,
    lineages: Vec<ResolvedImplementationLineage>,
}

impl ResolvedImplementationExpectation {
    /// Fallible smart constructor — packet/entity + tüm lineage consistency + sort + dedup.
    ///
    /// Validation:
    /// - `lineages` boş değil (EmptyLineages)
    /// - Her lineage: `expected_implementation.from == packet_id.to_node_id()`
    /// - Her lineage: `resolution.to == entity_id`
    /// - Lineage'lar `candidate_id` ascending sort + dedup
    pub fn try_new(
        packet_id: ConceptPacketId,
        entity_id: ConceptNodeId,
        mut lineages: Vec<ResolvedImplementationLineage>,
    ) -> Result<Self, ResolvedImplementationShapeError> {
        if lineages.is_empty() {
            return Err(ResolvedImplementationShapeError::EmptyLineages);
        }
        let expected_from = packet_id.to_node_id();
        for lineage in &lineages {
            if lineage.expected_implementation.from != expected_from {
                return Err(ResolvedImplementationShapeError::ExpectedSourceMismatch);
            }
            if lineage.resolution.to != entity_id {
                return Err(ResolvedImplementationShapeError::ResolutionTargetMismatch);
            }
        }
        // Deterministic sort + dedup (candidate_id ascending).
        lineages.sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
        lineages.dedup_by(|a, b| a.candidate_id == b.candidate_id);
        Ok(Self {
            packet_id,
            entity_id,
            lineages,
        })
    }

    pub fn packet_id(&self) -> &ConceptPacketId {
        &self.packet_id
    }

    pub fn entity_id(&self) -> &ConceptNodeId {
        &self.entity_id
    }

    pub fn lineages(&self) -> &[ResolvedImplementationLineage] {
        &self.lineages
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Error tipleri — Shape / Structure / Query ayrımı (P1-2 #[from] chain + P1-3 source status)
// ═══════════════════════════════════════════════════════════════════════════════

/// Aggregate shape error — smart constructor iç tutarlılık (P2 structural vs aggregate ayrımı).
///
/// "Read-model yanlış construct edildi" kategorisi. Graph basis bozuk DEĞİL — record'un
/// internal consistency ihlali.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ResolvedImplementationShapeError {
    #[error("lineages cannot be empty")]
    EmptyLineages,
    #[error("expected implementation edge kind mismatch: found {found:?}")]
    UnexpectedExpectedImplementationKind {
        found: ConceptEdgeKind,
    },
    #[error("resolution edge kind mismatch: found {found:?}")]
    UnexpectedResolutionKind {
        found: ConceptEdgeKind,
    },
    #[error("expected implementation source mismatch (not packet node)")]
    ExpectedSourceMismatch,
    #[error("expected implementation target mismatch (not candidate)")]
    ExpectedTargetMismatch,
    #[error("resolution source mismatch (not candidate)")]
    ResolutionSourceMismatch,
    #[error("resolution target mismatch (not entity)")]
    ResolutionTargetMismatch,
}

/// Basis structural error — graph state bozuk (P2 ayrım: "basis bozuk" vs "read-model yanlış").
///
/// Pure projector'ın fail-closed doğrulama hataları. Backend basis contract'ı ihlal ettiyse
/// veya graph invariant'ları tutmuyorsa üretilir.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ResolvedImplementationStructureError {
    #[error("duplicate node in projection basis: {node_id}")]
    DuplicateNode {
        node_id: ConceptNodeId,
    },
    #[error("missing edge endpoint: {node_id}")]
    MissingEndpoint {
        node_id: ConceptNodeId,
    },
    #[error("candidate has multiple accepted resolutions: {candidate_id}")]
    MultipleResolutions {
        candidate_id: ConceptNodeId,
    },
    /// R1a P1-1 (review tur 1) — Accepted ResolvesTo edge'in ResolutionRecord karşılığı yok
    /// (INV-C16 projection mirror: edge ↔ record ↔ binding triangulation).
    #[error("resolution record not found for edge: {candidate_id} -> {entity_id}")]
    MissingResolutionRecord {
        candidate_id: ConceptNodeId,
        entity_id: ConceptNodeId,
    },
    /// R1a P1-1 (review tur 2) — Duplicate ResolutionRecord (aynı candidate→entity çifti).
    #[error("duplicate resolution record: {candidate_id} -> {entity_id}")]
    DuplicateResolutionRecord {
        candidate_id: ConceptNodeId,
        entity_id: ConceptNodeId,
    },
    /// R1a P1-1 (review tur 2) — Orphan ResolutionRecord (record var, edge yok).
    #[error("orphan resolution record (no matching edge): {candidate_id} -> {entity_id}")]
    OrphanResolutionRecord {
        candidate_id: ConceptNodeId,
        entity_id: ConceptNodeId,
    },
    /// R1a P1-1 (review tur 1) — ResolutionRecord var ama edge endpoint'leri ile uyuşmuyor.
    #[error("resolution record edge mismatch: record {record_candidate} -> {record_entity}, edge {edge_candidate} -> {edge_entity}")]
    ResolutionRecordEdgeMismatch {
        record_candidate: ConceptNodeId,
        record_entity: ConceptNodeId,
        edge_candidate: ConceptNodeId,
        edge_entity: ConceptNodeId,
    },
    /// P1-2 (tur 4) — canonical reverse conversion error (`#[from]`).
    #[error(transparent)]
    InvalidPacketSource(#[from] InvalidConceptPacketNodeId),
    /// P1-4 (tur 3) — non-Accepted ResolvesTo edge (INV-C16 structural).
    #[error("resolution edge is not accepted: {from} -> {to}, status={status:?}")]
    InvalidResolutionStatus {
        from: ConceptNodeId,
        to: ConceptNodeId,
        status: DecisionStatus,
    },
    /// P1-3 (tur 4) — source status mismatch (Accepted required).
    #[error("resolution source is not accepted: {node_id}, status={status:?}")]
    InvalidResolutionSourceStatus {
        node_id: ConceptNodeId,
        status: DecisionStatus,
    },
    #[error("resolution source kind mismatch: {node_id} found {found:?}")]
    InvalidResolutionSourceKind {
        node_id: ConceptNodeId,
        found: ConceptNodeKind,
    },
    #[error("resolution target kind mismatch: {node_id} found {found:?}")]
    InvalidResolutionTargetKind {
        node_id: ConceptNodeId,
        found: ConceptNodeKind,
    },
    #[error("resolution endpoint family mismatch: {node_id} found {found:?}")]
    InvalidResolutionEndpointFamily {
        node_id: ConceptNodeId,
        found: PositionFamily,
    },
    /// R1a P1-2 (review tur 1) — ExpectedImplementation non-Candidate status (fail-closed).
    /// ExpectedImplementation proposal provenance Candidate lane'dir; non-Candidate edge
    /// sessiz skip DEĞİL — typed error (RP1 "kayıpsız" iddiası: yanlış lane'deki edge
    /// görünmez kılınamaz).
    #[error("expected implementation edge is not candidate: {from} -> {to}, status={status:?}")]
    InvalidExpectedImplementationStatus {
        from: ConceptNodeId,
        to: ConceptNodeId,
        status: DecisionStatus,
    },
    /// ExpectedImplementation target kind mismatch (Candidate lane).
    #[error("expected implementation target kind mismatch: {node_id} found {found:?}")]
    InvalidExpectedTargetKind {
        node_id: ConceptNodeId,
        found: ConceptNodeKind,
    },
    // P1-2 (tur 3) — #[from] Shape (smart ctor ? propagation).
    #[error("invalid lineage shape: {0}")]
    Shape(#[from] ResolvedImplementationShapeError),
}

/// Query error — Store (backend IO) + Projection (structural) iki katman (P1-1 tur 2).
#[derive(Debug, thiserror::Error)]
pub enum ResolvedImplementationQueryError<E> {
    #[error("store query failed: {0}")]
    Store(E),
    #[error("invalid implementation lineage basis: {0}")]
    Projection(#[from] ResolvedImplementationStructureError),
}

// ═══════════════════════════════════════════════════════════════════════════════
// project_resolved_implementations — pure projector (fail-closed validation)
// ═══════════════════════════════════════════════════════════════════════════════

/// Pure projector — `ResolvedImplementationBasis` → `Vec<ResolvedImplementationExpectation>`.
///
/// **Fail-closed validation:** Duplicate node, dangling endpoint, wrong-kind/status/family
/// hepsi typed error. Structurally malformed lineage state typed error ile reject edilir.
/// Valid ama unresolved veya non-live lineage state bilinçli olarak effective projection'dan hariç tutulur.
///
/// **Lineage fold (P1-2/P1-3/P1-4):**
/// 1. Fail-closed node index (duplicate node error).
/// 2. Accepted `ResolvesTo` — full endpoint matris (source CodeEntityCandidate + Accepted +
///    PhysicalCode; target CodeEntity + PhysicalCode + live). Non-Accepted → typed error.
///    Duplicate accepted → `MultipleResolutions` (R6 ihlali).
/// 3. `ExpectedImplementation` admission — Candidate status + target CodeEntityCandidate +
///    canonical triple dedup. Packet source `try_from_node_id` (`?` ile `#[from]`).
/// 4. N:1 grouping `(packet_id, entity_id)` + deterministic sort.
pub fn project_resolved_implementations(
    basis: &ResolvedImplementationBasis,
) -> Result<Vec<ResolvedImplementationExpectation>, ResolvedImplementationStructureError> {
    // 1. Fail-closed node index (duplicate node error, O(log n) lookup).
    let mut nodes_by_id: BTreeMap<&ConceptNodeId, &ConceptNode> = BTreeMap::new();
    for node in &basis.nodes {
        if nodes_by_id.insert(&node.id, node).is_some() {
            return Err(ResolvedImplementationStructureError::DuplicateNode {
                node_id: node.id.clone(),
            });
        }
    }

    // 1b. R1a P1-1 (review tur 2) — ResolutionRecord occurrence-aware index.
    //     Duplicate record → fail-closed (BTreeMap::collect sessiz overwrite YOK).
    //     İki yönlü triangulation: edge→record VE record→edge.
    let mut record_pairs: BTreeMap<(ConceptNodeId, ConceptNodeId), usize> = BTreeMap::new();
    for r in &basis.resolution_records {
        let key = (r.candidate_id.clone(), r.entity_id.clone());
        let count = record_pairs.entry(key).or_insert(0);
        *count += 1;
        if *count > 1 {
            return Err(ResolvedImplementationStructureError::DuplicateResolutionRecord {
                candidate_id: r.candidate_id.clone(),
                entity_id: r.entity_id.clone(),
            });
        }
    }

    // 2. Accepted ResolvesTo index — full endpoint matris + triangulation.
    //    R1a P1-2 (review tur 2): triangulation non-live skip'ten ÖNCE — structural validity
    //    → sonra projection'a dahil etme.
    //    R1a P1 (review tur 3): candidate-level R6 cardinality de non-live'den ÖNCE.
    //    İki ayrı map: resolution_by_candidate (R6 tüm edge'ler) + live_resolutions (projection).
    let mut edge_pairs: BTreeMap<(ConceptNodeId, ConceptNodeId), usize> = BTreeMap::new();
    let mut resolution_by_candidate: BTreeMap<ConceptNodeId, ConceptNodeId> = BTreeMap::new();
    let mut live_resolutions: BTreeMap<ConceptNodeId, (ConceptNodeId, DerivedEdgeReference)> =
        BTreeMap::new();
    for edge in &basis.edges {
        if edge.kind != ConceptEdgeKind::ResolvesTo {
            continue;
        }
        // P1-4 (tur 3) — non-Accepted ResolvesTo typed error (INV-C16 structural).
        if edge.decision_status != DecisionStatus::Accepted {
            return Err(ResolvedImplementationStructureError::InvalidResolutionStatus {
                from: edge.from.clone(),
                to: edge.to.clone(),
                status: edge.decision_status,
            });
        }
        // Source validation.
        let source = nodes_by_id
            .get(&edge.from)
            .ok_or_else(|| ResolvedImplementationStructureError::MissingEndpoint {
                node_id: edge.from.clone(),
            })?;
        if source.node_kind != ConceptNodeKind::CodeEntityCandidate {
            return Err(ResolvedImplementationStructureError::InvalidResolutionSourceKind {
                node_id: source.id.clone(),
                found: source.node_kind,
            });
        }
        // P1-3 (tur 4) — source status Accepted required.
        if source.decision_status != DecisionStatus::Accepted {
            return Err(ResolvedImplementationStructureError::InvalidResolutionSourceStatus {
                node_id: source.id.clone(),
                status: source.decision_status,
            });
        }
        if source.position_family != PositionFamily::PhysicalCode {
            return Err(ResolvedImplementationStructureError::InvalidResolutionEndpointFamily {
                node_id: source.id.clone(),
                found: source.position_family,
            });
        }
        // Target validation.
        let target = nodes_by_id
            .get(&edge.to)
            .ok_or_else(|| ResolvedImplementationStructureError::MissingEndpoint {
                node_id: edge.to.clone(),
            })?;
        if target.node_kind != ConceptNodeKind::CodeEntity {
            return Err(ResolvedImplementationStructureError::InvalidResolutionTargetKind {
                node_id: target.id.clone(),
                found: target.node_kind,
            });
        }
        if target.position_family != PositionFamily::PhysicalCode {
            return Err(ResolvedImplementationStructureError::InvalidResolutionEndpointFamily {
                node_id: target.id.clone(),
                found: target.position_family,
            });
        }
        // R1a P1-1 (review tur 2) — iki yönlü occurrence-aware triangulation.
        // Non-live skip'ten ÖNCE: structural validity (edge ↔ record) doğrulanmalı.
        let pair = (edge.from.clone(), edge.to.clone());
        let edge_count = edge_pairs.entry(pair.clone()).or_insert(0);
        *edge_count += 1;
        if *edge_count > 1 {
            return Err(ResolvedImplementationStructureError::MultipleResolutions {
                candidate_id: edge.from.clone(),
            });
        }
        // Edge → record yönü: bu edge'in record karşılığı var mı?
        match record_pairs.get(&pair) {
            Some(_) => {
                // Triangulation başarılı — edge ↔ record eşleşiyor.
            }
            None => {
                // Record yok → belki farklı entity'ye record var (mismatch).
                let mismatch = basis.resolution_records.iter().find(|r| r.candidate_id == edge.from);
                if let Some(r) = mismatch {
                    return Err(ResolvedImplementationStructureError::ResolutionRecordEdgeMismatch {
                        record_candidate: r.candidate_id.clone(),
                        record_entity: r.entity_id.clone(),
                        edge_candidate: edge.from.clone(),
                        edge_entity: edge.to.clone(),
                    });
                }
                return Err(ResolvedImplementationStructureError::MissingResolutionRecord {
                    candidate_id: edge.from.clone(),
                    entity_id: edge.to.clone(),
                });
            }
        }
        // R1a P1 (review tur 3) — candidate-level R6 cardinality non-live skip'ten ÖNCE.
        // Aynı candidate'in iki farklı entity'ye Accepted ResolvesTo edge'i R6 ihlalidir;
        // biri non-live olsa bile structural validity önce doğrulanmalı.
        if resolution_by_candidate
            .insert(edge.from.clone(), edge.to.clone())
            .is_some()
        {
            return Err(ResolvedImplementationStructureError::MultipleResolutions {
                candidate_id: edge.from.clone(),
            });
        }
        // R2 P2-4 (review tur 1) — non-live target skip (structural validity SONRASI).
        // Superseded/Deprecated CodeEntity geçerli tarihsel resolution hedefi olabilir.
        // Projector "hâlihazırda etkin resolution'lar" projekte eder.
        if !target.decision_status.is_live_code_identity() {
            continue;
        }
        // Live projection index — sadece live target'lar ExpectedImplementation admission'da görünür.
        let edge_ref = DerivedEdgeReference::from_edge(edge);
        live_resolutions.insert(edge.from.clone(), (edge.to.clone(), edge_ref));
    }

    // 2b. R1a P1-1 (review tur 2) — Orphan record check (record → edge yönü).
    //     Her record'un Accepted ResolvesTo edge karşılığı olmalı.
    for (pair, _) in &record_pairs {
        if !edge_pairs.contains_key(pair) {
            return Err(ResolvedImplementationStructureError::OrphanResolutionRecord {
                candidate_id: pair.0.clone(),
                entity_id: pair.1.clone(),
            });
        }
    }

    // 3. ExpectedImplementation admission + canonical triple dedup.
    let mut seen_expected: BTreeSet<(ConceptNodeId, ConceptEdgeKind, ConceptNodeId)> =
        BTreeSet::new();
    let mut grouped: BTreeMap<(ConceptPacketId, ConceptNodeId), Vec<ResolvedImplementationLineage>> =
        BTreeMap::new();
    for edge in &basis.edges {
        if edge.kind != ConceptEdgeKind::ExpectedImplementation {
            continue;
        }
        // R1a P1-2 (review tur 1) — ExpectedImplementation non-Candidate fail-closed.
        // ExpectedImplementation proposal provenance Candidate lane'dir; non-Candidate edge
        // (Accepted/Rejected/Deprecated/SupersededAccepted) sessiz skip DEĞİL — typed error.
        if edge.decision_status != DecisionStatus::Candidate {
            return Err(ResolvedImplementationStructureError::InvalidExpectedImplementationStatus {
                from: edge.from.clone(),
                to: edge.to.clone(),
                status: edge.decision_status,
            });
        }
        // Packet source parse (P1-2 — try_from_node_id, ? ile #[from]).
        let packet_id = ConceptPacketId::try_from_node_id(&edge.from)?;
        // Target validation — CodeEntityCandidate required.
        let candidate = nodes_by_id
            .get(&edge.to)
            .ok_or_else(|| ResolvedImplementationStructureError::MissingEndpoint {
                node_id: edge.to.clone(),
            })?;
        if candidate.node_kind != ConceptNodeKind::CodeEntityCandidate {
            return Err(ResolvedImplementationStructureError::InvalidExpectedTargetKind {
                node_id: candidate.id.clone(),
                found: candidate.node_kind,
            });
        }
        // Canonical triple dedup (duplicate expectation occurrence → collapse).
        let expected_key = (edge.from.clone(), edge.kind, edge.to.clone());
        if !seen_expected.insert(expected_key) {
            continue;
        }
        // Resolution lookup — candidate resolve edildiyse lineage üret.
        let Some((entity_id, resolution_ref)) = live_resolutions.get(&edge.to) else {
            continue;
        };
        let expected_ref = DerivedEdgeReference::from_edge(edge);
        let lineage = ResolvedImplementationLineage::try_new(
            edge.to.clone(),
            expected_ref,
            resolution_ref.clone(),
        )?;
        grouped
            .entry((packet_id, entity_id.clone()))
            .or_default()
            .push(lineage);
    }

    // 4. N:1 relation grouping + deterministic sort (packet_id, entity_id ascending).
    //    BTreeMap iteration key ordering → deterministic. try_new sort+dedup lineages.
    grouped
        .into_iter()
        .map(|((packet_id, entity_id), lineages)| {
            ResolvedImplementationExpectation::try_new(packet_id, entity_id, lineages)
                .map_err(ResolvedImplementationStructureError::Shape)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! PR G unit tests — Katman 1 (smart-ctor + shape errors + round-trip + basis construction).
    //!
    //! Katman 2 (lineage fold + RP1) ve Katman 3 (INV + RP3/RP4 + conformance) ayrı
    //! integration test modülünde.

    use super::*;

    // ─────────────────────────────────────────────────────────────────────────
    // Test helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn packet(id: &str) -> ConceptPacketId {
        ConceptPacketId(id.into())
    }

    fn node_id(id: &str) -> ConceptNodeId {
        ConceptNodeId(id.into())
    }

    fn expected_ref(packet: &ConceptPacketId, candidate: &ConceptNodeId) -> DerivedEdgeReference {
        DerivedEdgeReference::new(
            packet.clone().to_node_id(),
            ConceptEdgeKind::ExpectedImplementation,
            candidate.clone(),
        )
    }

    fn resolution_ref(candidate: &ConceptNodeId, entity: &ConceptNodeId) -> DerivedEdgeReference {
        DerivedEdgeReference::new(
            candidate.clone(),
            ConceptEdgeKind::ResolvesTo,
            entity.clone(),
        )
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ConceptPacketId round-trip (P2 non-empty contract)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn concept_packet_id_non_empty_round_trips() {
        let p = packet("packet-1");
        assert_eq!(ConceptPacketId::try_from_node_id(&p.to_node_id()).unwrap(), p);
    }

    #[test]
    fn concept_packet_id_empty_is_rejected() {
        let p = packet("");
        let result = ConceptPacketId::try_from_node_id(&p.to_node_id());
        assert!(result.is_err(), "empty packet ID reject (non-empty contract)");
    }

    #[test]
    fn concept_packet_id_non_packet_prefix_rejected() {
        let n = node_id("Concept:Payment");
        assert!(ConceptPacketId::try_from_node_id(&n).is_err());
    }

    #[test]
    fn concept_packet_id_prefix_constant_single_source_of_truth() {
        // NODE_PREFIX ile to_node_id formatı tutarlı.
        let p = packet("abc");
        let nid = p.to_node_id();
        assert!(nid.0.starts_with(ConceptPacketId::NODE_PREFIX));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ResolvedImplementationBasis (P1-1 public construction)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn basis_new_and_accessors() {
        let nodes = vec![];
        let edges = vec![];
        let basis = ResolvedImplementationBasis::new(nodes, edges, vec![]);
        assert!(basis.nodes().is_empty());
        assert!(basis.edges().is_empty());
        assert!(basis.resolution_records().is_empty());
    }

    #[test]
    fn basis_partial_eq() {
        let n = ConceptNode {
            id: node_id("ConceptPacket:x"),
            canonical: "x".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntityCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::PhysicalCode,
        };
        let b1 = ResolvedImplementationBasis::new(vec![n.clone()], vec![], vec![]);
        let b2 = ResolvedImplementationBasis::new(vec![n], vec![], vec![]);
        assert_eq!(b1, b2);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // DerivedEdgeReference (guarantee ladder base)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn derived_edge_reference_new_and_from_edge() {
        let r = DerivedEdgeReference::new(
            node_id("A"),
            ConceptEdgeKind::ResolvesTo,
            node_id("B"),
        );
        assert_eq!(r.from(), &node_id("A"));
        assert_eq!(r.kind(), ConceptEdgeKind::ResolvesTo);
        assert_eq!(r.to(), &node_id("B"));

        let edge = ConceptEdge {
            from: node_id("X"),
            to: node_id("Y"),
            kind: ConceptEdgeKind::ExpectedImplementation,
            decision_status: DecisionStatus::Candidate,
            explanation: None,
        };
        let r2 = DerivedEdgeReference::from_edge(&edge);
        assert_eq!(r2.from(), &node_id("X"));
        assert_eq!(r2.kind(), ConceptEdgeKind::ExpectedImplementation);
        assert_eq!(r2.to(), &node_id("Y"));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ResolvedImplementationLineage::try_new (shape errors)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn lineage_try_new_happy_path() {
        let candidate = node_id("CodeEntityCandidate:Z");
        let lineage = ResolvedImplementationLineage::try_new(
            candidate.clone(),
            expected_ref(&packet("P"), &candidate),
            resolution_ref(&candidate, &node_id("CodeEntity:W")),
        )
        .unwrap();
        assert_eq!(lineage.candidate_id(), &candidate);
    }

    #[test]
    fn lineage_try_new_rejects_wrong_expected_kind() {
        let candidate = node_id("CodeEntityCandidate:Z");
        let bad_expected = DerivedEdgeReference::new(
            packet("P").to_node_id(),
            ConceptEdgeKind::ResolvesTo, // wrong kind
            candidate.clone(),
        );
        let result = ResolvedImplementationLineage::try_new(
            candidate.clone(),
            bad_expected,
            resolution_ref(&candidate, &node_id("CodeEntity:W")),
        );
        assert!(matches!(
            result,
            Err(ResolvedImplementationShapeError::UnexpectedExpectedImplementationKind { .. })
        ));
    }

    #[test]
    fn lineage_try_new_rejects_wrong_resolution_kind() {
        let candidate = node_id("CodeEntityCandidate:Z");
        let bad_resolution = DerivedEdgeReference::new(
            candidate.clone(),
            ConceptEdgeKind::ImplementedBy, // wrong kind
            node_id("CodeEntity:W"),
        );
        let result = ResolvedImplementationLineage::try_new(
            candidate.clone(),
            expected_ref(&packet("P"), &candidate),
            bad_resolution,
        );
        assert!(matches!(
            result,
            Err(ResolvedImplementationShapeError::UnexpectedResolutionKind { .. })
        ));
    }

    #[test]
    fn lineage_try_new_rejects_expected_target_mismatch() {
        let candidate = node_id("CodeEntityCandidate:Z");
        let bad_expected = DerivedEdgeReference::new(
            packet("P").to_node_id(),
            ConceptEdgeKind::ExpectedImplementation,
            node_id("CodeEntityCandidate:OTHER"), // != candidate
        );
        let result = ResolvedImplementationLineage::try_new(
            candidate.clone(),
            bad_expected,
            resolution_ref(&candidate, &node_id("CodeEntity:W")),
        );
        assert!(matches!(
            result,
            Err(ResolvedImplementationShapeError::ExpectedTargetMismatch)
        ));
    }

    #[test]
    fn lineage_try_new_rejects_resolution_source_mismatch() {
        let candidate = node_id("CodeEntityCandidate:Z");
        let bad_resolution = DerivedEdgeReference::new(
            node_id("CodeEntityCandidate:OTHER"), // != candidate
            ConceptEdgeKind::ResolvesTo,
            node_id("CodeEntity:W"),
        );
        let result = ResolvedImplementationLineage::try_new(
            candidate.clone(),
            expected_ref(&packet("P"), &candidate), // expected.to = candidate (consistent)
            bad_resolution,
        );
        assert!(matches!(
            result,
            Err(ResolvedImplementationShapeError::ResolutionSourceMismatch)
        ));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ResolvedImplementationExpectation::try_new (shape errors + sort + dedup)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn expectation_try_new_happy_path() {
        let p = packet("P");
        let entity = node_id("CodeEntity:W");
        let candidate = node_id("CodeEntityCandidate:Z");
        let lineage = ResolvedImplementationLineage::try_new(
            candidate.clone(),
            expected_ref(&p, &candidate),
            resolution_ref(&candidate, &entity),
        )
        .unwrap();
        let exp = ResolvedImplementationExpectation::try_new(p.clone(), entity.clone(), vec![lineage]).unwrap();
        assert_eq!(exp.packet_id(), &p);
        assert_eq!(exp.entity_id(), &entity);
        assert_eq!(exp.lineages().len(), 1);
    }

    #[test]
    fn expectation_try_new_rejects_empty_lineages() {
        let result = ResolvedImplementationExpectation::try_new(
            packet("P"),
            node_id("CodeEntity:W"),
            vec![],
        );
        assert!(matches!(
            result,
            Err(ResolvedImplementationShapeError::EmptyLineages)
        ));
    }

    #[test]
    fn expectation_try_new_rejects_expected_source_mismatch() {
        let p = packet("P");
        let wrong_packet = packet("OTHER");
        let entity = node_id("CodeEntity:W");
        let candidate = node_id("CodeEntityCandidate:Z");
        // expected from = wrong_packet (mismatch with outer packet_id)
        let bad_expected = DerivedEdgeReference::new(
            wrong_packet.to_node_id(),
            ConceptEdgeKind::ExpectedImplementation,
            candidate.clone(),
        );
        let lineage = ResolvedImplementationLineage::try_new(
            candidate.clone(),
            bad_expected,
            resolution_ref(&candidate, &entity),
        )
        .unwrap();
        let result = ResolvedImplementationExpectation::try_new(p, entity, vec![lineage]);
        assert!(matches!(
            result,
            Err(ResolvedImplementationShapeError::ExpectedSourceMismatch)
        ));
    }

    #[test]
    fn expectation_try_new_rejects_resolution_target_mismatch() {
        let p = packet("P");
        let entity = node_id("CodeEntity:W");
        let wrong_entity = node_id("CodeEntity:OTHER");
        let candidate = node_id("CodeEntityCandidate:Z");
        let bad_resolution = DerivedEdgeReference::new(
            candidate.clone(),
            ConceptEdgeKind::ResolvesTo,
            wrong_entity, // != outer entity_id
        );
        let lineage = ResolvedImplementationLineage::try_new(
            candidate.clone(),
            expected_ref(&p, &candidate),
            bad_resolution,
        )
        .unwrap();
        let result = ResolvedImplementationExpectation::try_new(p, entity, vec![lineage]);
        assert!(matches!(
            result,
            Err(ResolvedImplementationShapeError::ResolutionTargetMismatch)
        ));
    }

    #[test]
    fn expectation_try_new_sorts_and_dedups_lineages_by_candidate() {
        let p = packet("P");
        let entity = node_id("CodeEntity:W");
        let c1 = node_id("CodeEntityCandidate:A");
        let c2 = node_id("CodeEntityCandidate:B");
        let l1 = ResolvedImplementationLineage::try_new(
            c1.clone(),
            expected_ref(&p, &c1),
            resolution_ref(&c1, &entity),
        )
        .unwrap();
        let l2 = ResolvedImplementationLineage::try_new(
            c2.clone(),
            expected_ref(&p, &c2),
            resolution_ref(&c2, &entity),
        )
        .unwrap();
        // Verilen sıra: l2, l1 (reverse) + duplicate l1.
        let exp = ResolvedImplementationExpectation::try_new(
            p,
            entity,
            vec![l2.clone(), l1.clone(), l1.clone()],
        )
        .unwrap();
        // Sort: A, B. Dedup: duplicate A kalkar.
        assert_eq!(exp.lineages().len(), 2);
        assert_eq!(exp.lineages()[0].candidate_id(), &c1);
        assert_eq!(exp.lineages()[1].candidate_id(), &c2);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // #[from] conversion chain (P1-2)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn shape_error_converts_to_structure_error_via_from() {
        let shape = ResolvedImplementationShapeError::EmptyLineages;
        let structure: ResolvedImplementationStructureError = shape.into();
        assert!(matches!(
            structure,
            ResolvedImplementationStructureError::Shape(
                ResolvedImplementationShapeError::EmptyLineages
            )
        ));
    }

    #[test]
    fn invalid_packet_node_id_converts_to_structure_error_via_from() {
        let invalid = InvalidConceptPacketNodeId {
            node_id: node_id("Concept:Payment"),
        };
        let structure: ResolvedImplementationStructureError = invalid.into();
        assert!(matches!(
            structure,
            ResolvedImplementationStructureError::InvalidPacketSource(_)
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PR G Katman 2+3 — lineage fold + INV interaction + conformance (integration tests)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod lineage_fold_tests {
    //! Katman 2 (RP1 soundness + P1'ler) + Katman 3 (RP3/RP4 + conformance).
    //!
    //! Gerçek ConceptNode/ConceptEdge fixture'ları → project_resolved_implementations.

    use super::*;
    use crate::anchoring::review::{ResolutionOutcome, ResolutionRecord};
    use crate::anchoring::types::{
        ConceptEdge, ConceptNode, ConceptNodeId, ConceptNodeKind, ConceptPacketId,
    };
    use crate::anchoring::{ConceptEdgeKind, DecisionStatus, PositionFamily};

    // ─────────────────────────────────────────────────────────────────────────
    // Fixture builders
    // ─────────────────────────────────────────────────────────────────────────

    fn accepted_candidate(path: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(format!("CodeEntityCandidate:{path}")),
            canonical: path.into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntityCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::PhysicalCode,
        }
    }

    fn live_entity(entity_id: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(entity_id.into()),
            canonical: entity_id.into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntity,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::PhysicalCode,
        }
    }

    fn expected_edge(packet: &ConceptPacketId, candidate: &ConceptNodeId) -> ConceptEdge {
        ConceptEdge {
            from: packet.clone().to_node_id(),
            to: candidate.clone(),
            kind: ConceptEdgeKind::ExpectedImplementation,
            decision_status: DecisionStatus::Candidate,
            explanation: None,
        }
    }

    fn resolves_to_edge(candidate: &ConceptNodeId, entity: &ConceptNodeId) -> ConceptEdge {
        ConceptEdge {
            from: candidate.clone(),
            to: entity.clone(),
            kind: ConceptEdgeKind::ResolvesTo,
            decision_status: DecisionStatus::Accepted,
            explanation: None,
        }
    }

    fn resolution_record(candidate: &ConceptNodeId, entity: &ConceptNodeId) -> ResolutionRecord {
        use crate::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};
        use crate::anchoring::review::{OperatorId, SessionId};
        use crate::anchoring::NonEmptyExplanation;
        use std::time::SystemTime;
        let operator = OperatorId::new("test-op");
        ResolutionRecord {
            seq: 1,
            session_id: SessionId::derive(&operator, SystemTime::UNIX_EPOCH),
            operator,
            candidate_id: candidate.clone(),
            entity_id: entity.clone(),
            identity_key: CodeIdentityKey::new(
                CodeIdentityScheme::AnalysisPathV1 {
                    case_policy: CodePathCasePolicy::CaseSensitive,
                },
                "test-key",
            )
            .unwrap(),
            outcome: ResolutionOutcome::Created {
                entity_id: entity.clone(),
            },
            reason: NonEmptyExplanation::new("test resolution").unwrap(),
            candidate_digest: 0,
            entity_digest: 0,
            basis_fingerprint: [0u8; 32],
            at: SystemTime::UNIX_EPOCH,
        }
    }

    fn basis(
        nodes: Vec<ConceptNode>,
        edges: Vec<ConceptEdge>,
        records: Vec<ResolutionRecord>,
    ) -> ResolvedImplementationBasis {
        ResolvedImplementationBasis::new(nodes, edges, records)
    }

    /// Convenience: basis without resolution records (negatif test'ler için).
    fn basis_no_records(nodes: Vec<ConceptNode>, edges: Vec<ConceptEdge>) -> ResolvedImplementationBasis {
        ResolvedImplementationBasis::new(nodes, edges, vec![])
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Katman 2 — happy path + RP1 soundness
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn project_happy_path_single_lineage() {
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc123");
        let edges = vec![
            expected_edge(&packet, &candidate.id),
            resolves_to_edge(&candidate.id, &entity.id),
        ];
        let records = vec![resolution_record(&candidate.id, &entity.id)];
        let b = basis(vec![candidate.clone(), entity.clone()], edges, records);

        let result = project_resolved_implementations(&b).unwrap();
        assert_eq!(result.len(), 1, "tek relation");
        let r = &result[0];
        assert_eq!(r.packet_id(), &packet);
        assert_eq!(r.entity_id(), &entity.id);
        assert_eq!(r.lineages().len(), 1, "tek lineage");
        assert_eq!(r.lineages()[0].candidate_id(), &candidate.id);
    }

    #[test]
    fn project_unresolved_candidate_produces_no_relation() {
        // ExpectedImplementation var ama ResolvesTo yok → derived YOK.
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let edges = vec![expected_edge(&packet, &candidate.id)];
        let b = basis_no_records(vec![candidate], edges);

        let result = project_resolved_implementations(&b).unwrap();
        assert!(result.is_empty(), "unresolved candidate → no relation");
    }

    #[test]
    fn project_n1_two_candidates_same_entity_one_relation_two_lineages() {
        // İki packet aynı entity'ye → iki ayrı relation.
        // Ama iki candidate aynı (packet, entity)'ye → tek relation iki lineage.
        let packet = ConceptPacketId("pkt-1".into());
        let c1 = accepted_candidate("src/auth.rs");
        let c2 = accepted_candidate("src/Auth.rs");
        let entity = live_entity("CodeEntity:shared");
        let edges = vec![
            expected_edge(&packet, &c1.id),
            expected_edge(&packet, &c2.id),
            resolves_to_edge(&c1.id, &entity.id),
            resolves_to_edge(&c2.id, &entity.id),
        ];
        let records = vec![
            resolution_record(&c1.id, &entity.id),
            resolution_record(&c2.id, &entity.id),
        ];
        let b = basis(vec![c1.clone(), c2.clone(), entity], edges, records);

        let result = project_resolved_implementations(&b).unwrap();
        assert_eq!(result.len(), 1, "tek relation (packet, entity)");
        assert_eq!(result[0].lineages().len(), 2, "iki lineage proof");
    }

    #[test]
    fn project_two_packets_same_entity_two_relations() {
        let p1 = ConceptPacketId("pkt-1".into());
        let p2 = ConceptPacketId("pkt-2".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:shared");
        let edges = vec![
            expected_edge(&p1, &candidate.id),
            expected_edge(&p2, &candidate.id),
            resolves_to_edge(&candidate.id, &entity.id),
        ];
        let records = vec![resolution_record(&candidate.id, &entity.id)];
        let b = basis(vec![candidate, entity], edges, records);

        let result = project_resolved_implementations(&b).unwrap();
        assert_eq!(result.len(), 2, "iki relation (farklı packet'ler)");
        // Deterministic sort: pkt-1 < pkt-2.
        assert_eq!(result[0].packet_id(), &p1);
        assert_eq!(result[1].packet_id(), &p2);
    }

    #[test]
    fn project_duplicate_expected_triple_collapses() {
        // Aynı (packet, ExpectedImplementation, candidate) triple'ı iki kez → tek lineage.
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let edges = vec![
            expected_edge(&packet, &candidate.id),
            expected_edge(&packet, &candidate.id), // duplicate triple
            resolves_to_edge(&candidate.id, &entity.id),
        ];
        let records = vec![resolution_record(&candidate.id, &entity.id)];
        let b = basis(vec![candidate, entity], edges, records);

        let result = project_resolved_implementations(&b).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].lineages().len(), 1, "duplicate triple → collapse");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Katman 2 — fail-closed structural errors (P1'ler)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn project_rejects_non_accepted_resolves_to() {
        // P1-4 (tur 3): non-Accepted ResolvesTo → InvalidResolutionStatus (INV-C16 structural).
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let mut edge = resolves_to_edge(&candidate.id, &entity.id);
        edge.decision_status = DecisionStatus::Candidate; // non-Accepted
        let b = basis_no_records(
            vec![candidate, entity],
            vec![expected_edge(&packet, &ConceptNodeId("CodeEntityCandidate:src/auth.rs".into())), edge],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidResolutionStatus { .. }
        ));
    }

    #[test]
    fn project_rejects_non_accepted_resolution_source() {
        // P1-3 (tur 4): source Accepted değil → InvalidResolutionSourceStatus.
        let packet = ConceptPacketId("pkt-1".into());
        let mut candidate = accepted_candidate("src/auth.rs");
        candidate.decision_status = DecisionStatus::Candidate; // non-Accepted source
        let entity = live_entity("CodeEntity:abc");
        let b = basis_no_records(
            vec![candidate, entity.clone()],
            vec![expected_edge(&packet, &ConceptNodeId("CodeEntityCandidate:src/auth.rs".into())),
                 resolves_to_edge(&ConceptNodeId("CodeEntityCandidate:src/auth.rs".into()), &entity.id)],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidResolutionSourceStatus { .. }
        ));
    }

    #[test]
    fn project_rejects_duplicate_accepted_resolution() {
        // R6 ihlali: aynı candidate için iki Accepted ResolvesTo → MultipleResolutions.
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity1 = live_entity("CodeEntity:abc");
        let entity2 = live_entity("CodeEntity:def");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let entity1_id = ConceptNodeId("CodeEntity:abc".into());
        let b = basis(
            vec![candidate, entity1, entity2],
            vec![
                expected_edge(&packet, &candidate_id),
                resolves_to_edge(&candidate_id, &entity1_id),
                resolves_to_edge(&candidate_id, &ConceptNodeId("CodeEntity:def".into())),
            ],
            // R1a P1-1: her iki edge için record var (triangulation geçer), ikinci edge duplicate.
            vec![
                resolution_record(&candidate_id, &entity1_id),
                resolution_record(&candidate_id, &ConceptNodeId("CodeEntity:def".into())),
            ],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        // R6 ihlali: iki edge aynı candidate'ten. Triangulation record-edge mismatch yakalar
        // (ikinci edge entity2'ye ama record candidate→entity1'den geliyor — BTreeMap son
        // yazan kazanır). Bu fail-closed davranış; hangi varyantın önce tetiklendiği
        // implementation detail — önemli olan structurally invalid state'in reject edilmesi.
        assert!(
            matches!(
                err,
                ResolvedImplementationStructureError::MultipleResolutions { .. }
                    | ResolvedImplementationStructureError::ResolutionRecordEdgeMismatch { .. }
                    | ResolvedImplementationStructureError::MissingResolutionRecord { .. }
            ),
            "R6 ihlali structural error: {err:?}"
        );
    }

    #[test]
    fn project_rejects_duplicate_node_in_basis() {
        // Duplicate node → DuplicateNode error.
        let candidate = accepted_candidate("src/auth.rs");
        let b = basis_no_records(vec![candidate.clone(), candidate], vec![]);
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::DuplicateNode { .. }
        ));
    }

    #[test]
    fn project_rejects_multiple_resolutions_when_one_target_is_non_live() {
        // R1a P1 (review tur 3) — aynı candidate'in biri non-live iki farklı entity'ye
        // Accepted ResolvesTo → R6 ihlali non-live skip'ten ÖNCE yakalanmalı.
        let candidate = accepted_candidate("src/auth.rs");
        let live_e = live_entity("CodeEntity:new");
        let deprecated_e = ConceptNode {
            id: ConceptNodeId("CodeEntity:old".into()),
            canonical: "old".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntity,
            decision_status: DecisionStatus::Deprecated, // non-live
            position_family: PositionFamily::PhysicalCode,
        };
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let new_id = ConceptNodeId("CodeEntity:new".into());
        let old_id = ConceptNodeId("CodeEntity:old".into());
        let b = basis(
            vec![candidate, live_e, deprecated_e],
            vec![
                resolves_to_edge(&candidate_id, &old_id),
                resolves_to_edge(&candidate_id, &new_id),
            ],
            vec![
                resolution_record(&candidate_id, &old_id),
                resolution_record(&candidate_id, &new_id),
            ],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::MultipleResolutions { .. }
        ));
    }

    #[test]
    fn project_rejects_missing_endpoint() {
        // ResolvesTo edge'in source node'u basis'te yok → MissingEndpoint.
        let entity = live_entity("CodeEntity:abc");
        let b = basis_no_records(
            vec![entity],
            vec![resolves_to_edge(
                &ConceptNodeId("CodeEntityCandidate:ghost".into()), // not in basis
                &ConceptNodeId("CodeEntity:abc".into()),
            )],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::MissingEndpoint { .. }
        ));
    }

    #[test]
    fn project_rejects_invalid_packet_source() {
        // P1-2: ExpectedImplementation source ConceptPacket: prefix değil → InvalidPacketSource.
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let bad_edge = ConceptEdge {
            from: ConceptNodeId("Concept:Payment".into()), // non-packet prefix
            to: candidate.id.clone(),
            kind: ConceptEdgeKind::ExpectedImplementation,
            decision_status: DecisionStatus::Candidate,
            explanation: None,
        };
        let b = basis_no_records(
            vec![candidate, entity],
            vec![bad_edge],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidPacketSource(_)
        ));
    }

    #[test]
    fn project_rejects_non_candidate_expected_implementation_status() {
        // R1a P1-2 (review tur 1) — ExpectedImplementation non-Candidate fail-closed error.
        // ExpectedImplementation proposal provenance Candidate lane'dir; Accepted/Rejected/
        // Deprecated/SupersededAccepted edge sessiz skip DEĞİL — typed error (RP1 kayıpsız).
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let mut edge = expected_edge(&packet, &candidate.id);
        edge.decision_status = DecisionStatus::Accepted; // non-Candidate → error
        let b = basis_no_records(
            vec![candidate, entity],
            vec![edge],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidExpectedImplementationStatus { .. }
        ));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Katman 3 — RP3 (no explanation) + deterministic sort
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn rp3_no_explanation_in_serialized_output() {
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let entity_id = ConceptNodeId("CodeEntity:abc".into());
        let b = basis(
            vec![candidate, entity],
            vec![
                expected_edge(&packet, &candidate_id),
                resolves_to_edge(&candidate_id, &entity_id),
            ],
            vec![resolution_record(&candidate_id, &entity_id)],
        );
        let result = project_resolved_implementations(&b).unwrap();
        let json = serde_json::to_value(&result).unwrap();
        for relation in json.as_array().unwrap() {
            assert!(relation.get("explanation").is_none(), "RP3: relation explanation yok");
            for lineage in relation["lineages"].as_array().unwrap() {
                assert!(lineage.get("explanation").is_none(), "RP3: lineage explanation yok");
                assert!(
                    lineage["expected_implementation"].get("explanation").is_none(),
                    "RP3: expected_implementation explanation yok"
                );
                assert!(
                    lineage["resolution"].get("explanation").is_none(),
                    "RP3: resolution explanation yok"
                );
            }
        }
    }

    #[test]
    fn deterministic_sort_packet_entity_candidate_ordering() {
        // (packet_id, entity_id) + candidate_id ascending tuple ordering.
        let p_b = ConceptPacketId("pkt-b".into());
        let p_a = ConceptPacketId("pkt-a".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let entity_id = ConceptNodeId("CodeEntity:abc".into());
        let b = basis(
            vec![candidate, entity],
            vec![
                expected_edge(&p_b, &candidate_id),
                expected_edge(&p_a, &candidate_id),
                resolves_to_edge(&candidate_id, &entity_id),
            ],
            vec![resolution_record(&candidate_id, &entity_id)],
        );
        let result = project_resolved_implementations(&b).unwrap();
        // pkt-a < pkt-b deterministic sort.
        assert_eq!(result[0].packet_id(), &p_a);
        assert_eq!(result[1].packet_id(), &p_b);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R2 P2-2 (review tur 1) — eksik error dalı fixture'ları
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn project_rejects_invalid_resolution_source_kind() {
        // ResolvesTo source CodeEntityCandidate değil → InvalidResolutionSourceKind.
        let candidate = ConceptNode {
            id: ConceptNodeId("CodeEntity:not-candidate".into()),
            canonical: "not-candidate".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntity, // wrong kind
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::PhysicalCode,
        };
        let entity = live_entity("CodeEntity:abc");
        let b = basis_no_records(
            vec![candidate, entity],
            vec![resolves_to_edge(
                &ConceptNodeId("CodeEntity:not-candidate".into()),
                &ConceptNodeId("CodeEntity:abc".into()),
            )],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidResolutionSourceKind { .. }
        ));
    }

    #[test]
    fn project_rejects_invalid_resolution_target_kind() {
        // ResolvesTo target CodeEntity değil → InvalidResolutionTargetKind.
        let candidate = accepted_candidate("src/auth.rs");
        let wrong_target = ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept, // wrong kind
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::PhysicalCode,
        };
        let b = basis_no_records(
            vec![candidate, wrong_target],
            vec![resolves_to_edge(
                &ConceptNodeId("CodeEntityCandidate:src/auth.rs".into()),
                &ConceptNodeId("Concept:Payment".into()),
            )],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidResolutionTargetKind { .. }
        ));
    }

    #[test]
    fn project_rejects_invalid_resolution_endpoint_family_source() {
        // ResolvesTo source family PhysicalCode değil → InvalidResolutionEndpointFamily.
        let candidate = ConceptNode {
            id: ConceptNodeId("CodeEntityCandidate:src/auth.rs".into()),
            canonical: "src/auth.rs".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntityCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent, // wrong family
        };
        let entity = live_entity("CodeEntity:abc");
        let b = basis_no_records(
            vec![candidate, entity],
            vec![resolves_to_edge(
                &ConceptNodeId("CodeEntityCandidate:src/auth.rs".into()),
                &ConceptNodeId("CodeEntity:abc".into()),
            )],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidResolutionEndpointFamily { .. }
        ));
    }

    #[test]
    fn project_rejects_invalid_resolution_endpoint_family_target() {
        // ResolvesTo target family PhysicalCode değil → InvalidResolutionEndpointFamily.
        let candidate = accepted_candidate("src/auth.rs");
        let wrong_target = ConceptNode {
            id: ConceptNodeId("CodeEntity:abc".into()),
            canonical: "abc".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntity,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::ConceptualIntent, // wrong family
        };
        let b = basis_no_records(
            vec![candidate, wrong_target],
            vec![resolves_to_edge(
                &ConceptNodeId("CodeEntityCandidate:src/auth.rs".into()),
                &ConceptNodeId("CodeEntity:abc".into()),
            )],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidResolutionEndpointFamily { .. }
        ));
    }

    #[test]
    fn project_rejects_invalid_expected_target_kind() {
        // ExpectedImplementation target CodeEntityCandidate değil → InvalidExpectedTargetKind.
        let packet = ConceptPacketId("pkt-1".into());
        let wrong_target = ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::Concept, // wrong kind
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::PhysicalCode,
        };
        let b = basis_no_records(
            vec![wrong_target],
            vec![ConceptEdge {
                from: packet.to_node_id(),
                to: ConceptNodeId("Concept:Payment".into()),
                kind: ConceptEdgeKind::ExpectedImplementation,
                decision_status: DecisionStatus::Candidate,
                explanation: None,
            }],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidExpectedTargetKind { .. }
        ));
    }

    #[test]
    fn project_non_live_target_skipped_not_error() {
        // R2 P2-4 (review tur 1) — non-live target skip (hard-error DEĞİL).
        // Superseded CodeEntity geçerli tarihsel resolution hedefi; projector etkin
        // resolution'lar only projekte eder.
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let superseded_entity = ConceptNode {
            id: ConceptNodeId("CodeEntity:superseded".into()),
            canonical: "superseded".into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntity,
            decision_status: DecisionStatus::Deprecated, // non-live
            position_family: PositionFamily::PhysicalCode,
        };
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let entity_id = ConceptNodeId("CodeEntity:superseded".into());
        let b = basis(
            vec![candidate, superseded_entity],
            vec![
                expected_edge(&packet, &candidate_id),
                resolves_to_edge(&candidate_id, &entity_id),
            ],
            vec![resolution_record(&candidate_id, &entity_id)],
        );
        let result = project_resolved_implementations(&b).unwrap();
        assert!(
            result.is_empty(),
            "non-live target skip → no relation (etkin resolution'lar only)"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R1a P1-1 (review tur 1) — ResolutionRecord triangulation test'leri
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn project_rejects_missing_resolution_record() {
        // Accepted ResolvesTo edge var ama ResolutionRecord yok → MissingResolutionRecord.
        let packet = ConceptPacketId("pkt-1".into());
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let entity_id = ConceptNodeId("CodeEntity:abc".into());
        let b = basis_no_records(
            vec![candidate, entity],
            vec![
                expected_edge(&packet, &candidate_id),
                resolves_to_edge(&candidate_id, &entity_id),
            ],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::MissingResolutionRecord { .. }
        ));
    }

    #[test]
    fn project_rejects_resolution_record_edge_mismatch() {
        // ResolutionRecord var ama edge endpoint ile uyuşmuyor → ResolutionRecordEdgeMismatch.
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let entity_id = ConceptNodeId("CodeEntity:abc".into());
        // Record candidate→entity diyor ama edge candidate→different-entity.
        let different_entity_id = ConceptNodeId("CodeEntity:different".into());
        let different_entity = live_entity("CodeEntity:different");
        let b = basis(
            vec![candidate, entity, different_entity],
            vec![resolves_to_edge(&candidate_id, &different_entity_id)],
            // Record candidate→entity (edge ile uyuşmuyor).
            vec![resolution_record(&candidate_id, &entity_id)],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::ResolutionRecordEdgeMismatch { .. }
                | ResolvedImplementationStructureError::MissingResolutionRecord { .. }
        ));
    }

    #[test]
    fn project_rejects_duplicate_resolution_record() {
        // R1a P1-1 (review tur 2) — duplicate ResolutionRecord (aynı candidate→entity).
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let entity_id = ConceptNodeId("CodeEntity:abc".into());
        let b = basis(
            vec![candidate, entity],
            vec![resolves_to_edge(&candidate_id, &entity_id)],
            vec![
                resolution_record(&candidate_id, &entity_id),
                resolution_record(&candidate_id, &entity_id), // duplicate
            ],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::DuplicateResolutionRecord { .. }
        ));
    }

    #[test]
    fn project_rejects_orphan_resolution_record() {
        // R1a P1-1 (review tur 2) — orphan record (record var, edge yok).
        let candidate = accepted_candidate("src/auth.rs");
        let entity = live_entity("CodeEntity:abc");
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let entity_id = ConceptNodeId("CodeEntity:abc".into());
        let b = basis(
            vec![candidate, entity],
            vec![], // edge yok
            vec![resolution_record(&candidate_id, &entity_id)], // record var → orphan
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::OrphanResolutionRecord { .. }
        ));
    }

    #[test]
    fn project_packet_id_whitespace_rejected() {
        // R1a P2 (review tur 2) — whitespace/control packet ID reject.
        let candidate = accepted_candidate("src/auth.rs");
        let b = basis_no_records(
            vec![candidate],
            vec![ConceptEdge {
                from: ConceptNodeId("ConceptPacket:   ".into()), // whitespace
                to: ConceptNodeId("CodeEntityCandidate:src/auth.rs".into()),
                kind: ConceptEdgeKind::ExpectedImplementation,
                decision_status: DecisionStatus::Candidate,
                explanation: None,
            }],
        );
        let err = project_resolved_implementations(&b).unwrap_err();
        assert!(matches!(
            err,
            ResolvedImplementationStructureError::InvalidPacketSource(_)
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PR G review tur 1 — gerçek store integration test (RP4-b snapshot equality + RP1)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod store_integration_tests {
    //! R1a P1-3 / R2 P2-3 — gerçek InMemoryAnchorStore ile integration test.
    //! - resolved_implementation_expectation_query() (default method) çağrılır
    //! - export_snapshot before/after equality (RP4-b)
    //! - RP1 non-tautological (gerçek store state'inden expected manuel kurulur)

    use super::*;
    use crate::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};
    use crate::anchoring::review::{
        CodeEntityResolutionSession, OperatorId, PresentedResolutionBasis,
    };
    use crate::anchoring::store::{AnchorStore, InMemoryAnchorStore};
    use crate::anchoring::types::{
        CodeIdentityBinding, ConceptNode, ConceptNodeKind, GraphSeed,
    };
    use crate::anchoring::{DecisionStatus, NonEmptyExplanation, PositionFamily};

    fn accepted_candidate(path: &str) -> ConceptNode {
        ConceptNode {
            id: ConceptNodeId(format!("CodeEntityCandidate:{path}")),
            canonical: path.into(),
            aliases: vec![],
            node_kind: ConceptNodeKind::CodeEntityCandidate,
            decision_status: DecisionStatus::Accepted,
            position_family: PositionFamily::PhysicalCode,
        }
    }

    fn store_with_resolved_candidate() -> InMemoryAnchorStore {
        let mut seed = GraphSeed::default();
        seed.code_entities.push(accepted_candidate("src/auth.rs"));
        let mut store = InMemoryAnchorStore::with_seed(seed);
        let key = CodeIdentityKey::new(
            CodeIdentityScheme::AnalysisPathV1 {
                case_policy: CodePathCasePolicy::CaseSensitive,
            },
            "src/auth.rs",
        )
        .unwrap();
        store
            .seed_code_identity_bindings_trusted(&[CodeIdentityBinding {
                node_id: ConceptNodeId("CodeEntityCandidate:src/auth.rs".into()),
                identity_key: key,
            }])
            .unwrap();

        // Resolve candidate → entity materialize.
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let mut session = CodeEntityResolutionSession::open_for_operator(OperatorId::new("op"));
        let basis = PresentedResolutionBasis::compile(&store, &candidate_id).unwrap();
        session
            .resolve(
                &mut store,
                &candidate_id,
                basis,
                NonEmptyExplanation::new("test resolution").unwrap(),
            )
            .unwrap();
        store
    }

    #[test]
    fn store_query_returns_expected_projection_with_expected_implementation() {
        // R1a P1-3 (review tur 2) — gerçek ExpectedImplementation edge + resolution → non-empty.
        // Tam akış: candidate seed + ExpectedImplementation edge (graph_mut) → binding →
        //           apply_resolution → query → non-empty projection.
        use crate::anchoring::types::ConceptEdge;

        let mut store = store_with_resolved_candidate();
        let candidate_id = ConceptNodeId("CodeEntityCandidate:src/auth.rs".into());
        let packet = ConceptPacketId("pkt-integration".into());

        // ExpectedImplementation edge'i graph'a ekle (packet → candidate, Candidate lane).
        store.graph_mut().insert_edge(ConceptEdge {
            from: packet.clone().to_node_id(),
            to: candidate_id.clone(),
            kind: ConceptEdgeKind::ExpectedImplementation,
            decision_status: DecisionStatus::Candidate,
            explanation: None,
        });

        // Query → non-empty projection.
        let result = store.resolved_implementation_expectation_query().unwrap();
        assert_eq!(result.len(), 1, "tek relation (packet → entity)");
        assert_eq!(result[0].packet_id(), &packet);
        assert_eq!(result[0].lineages().len(), 1, "tek lineage");
        assert_eq!(result[0].lineages()[0].candidate_id(), &candidate_id);
    }

    #[test]
    fn store_query_returns_empty_without_expected_implementation() {
        // R1a P1-3 negatif — ExpectedImplementation edge yok → boş projection.
        let store = store_with_resolved_candidate();
        let result = store.resolved_implementation_expectation_query().unwrap();
        assert!(
            result.is_empty(),
            "ExpectedImplementation edge yok → boş projection (dürüst)"
        );
    }

    #[test]
    fn store_query_rpsnapshot_unchanged() {
        // RP4-b — export_snapshot before/after equality.
        let store = store_with_resolved_candidate();
        let before = store.export_snapshot();
        let _result = store.resolved_implementation_expectation_query().unwrap();
        let after = store.export_snapshot();
        assert_eq!(
            before, after,
            "RP4-b: projection store snapshot'ı değiştirmez"
        );
    }

    #[test]
    fn store_basis_includes_resolution_records() {
        // R1a P1-1 — basis resolution_records içerir (triangulation için).
        let store = store_with_resolved_candidate();
        let basis = store.resolved_implementation_basis().unwrap();
        assert!(
            !basis.resolution_records().is_empty(),
            "basis resolution ledger snapshot içerir (triangulation)"
        );
    }
}
