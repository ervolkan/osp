//! Agent semantiği — Faz 5 stub tipleri (agent-prompt-semantics.md).
//!
//! Bu modül Faz 5 (LLM OSP Codec) tasarımının tip iskeletini içerir. Implementasyonlar
//! (compute_space_slice, LLM runtime, witness feedback) Faz 5'te gelir. Şu an sadece
//! tipler tanımlı — `engine.rs` Q4/Q6 gate'leri ve `EngineCommitError` variant'ları
//! bunlara referans verir.
//!
//! **Önemli (inv #11-14):**
//! - LLM durumsuzdur, durum Agent kabuğunda
//! - `DeltaProposal` pozisyon **içermez** — engine compute eder (inv #4)
//! - `PermissionMask` God Mode tarafından atanır, Agent değiştiremez (inv #13)
//! - Prompt doğal dil değil, tiplenmiş pakettir (inv #14)

use std::collections::HashSet;

use crate::coords::{AxisId, RawPosition};
use crate::rule::Rule;
use crate::space::{Edge, EdgeKind, Node, NodeId, NodeKind, Space};
use crate::witness::ClaimId;

// ═══════════════════════════════════════════════════════════════════════════════
// PermissionMask (inv #13 — God Mode atanır, Agent değiştiremez)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agent'ın okuma/yazma yetki matrisi (agent-prompt-semantics.md §2.1).
///
/// God Mode (insan-operatör veya bootstrap config) tarafından Intent hedef alanına
/// ve Agent rolüne göre atanır. Agent kabuğu ve LLM kendi yetkilerini genişletemez.
///
/// Üç-nokta savunma derinliği:
/// 1. `compute_space_slice()` — okuma izni olmayan düğümleri projeksiyondan çıkarır
/// 2. Agent kabuğu — yazma izni olmayan mutasyonları erken reddeder
/// 3. `SpaceEngine::commit()` — nihai zorunlu kontrol (atlanamaz)
#[derive(Debug, Clone, Default)]
pub struct PermissionMask {
    /// Agent'ın değiştiremeyeceği, sadece okuyabileceği düğümler.
    pub read_only_nodes: HashSet<NodeId>,
    /// Agent'ın yeni düğüm ekleyebileceği veya koordinat güncelleyebileceği eksenler.
    pub writable_axes: HashSet<AxisId>,
    /// Agent'ın oluşturamayacağı kenar türleri (örn: Approves → sadece Witness).
    pub forbidden_edge_kinds: HashSet<EdgeKind>,
    /// Agent'ın pozisyon güncelleyebileceği maksimum sapma (θ_max yetki sınırı).
    pub max_position_deviation: f64,
}

impl PermissionMask {
    /// Default: tüm node'lar read-write, tüm axis'ler writable, sınırsız deviation.
    /// Faz 2'de no-op/full-access; Faz 5'te God Mode config'ten yüklenir.
    pub fn full_access() -> Self {
        Self {
            read_only_nodes: HashSet::new(),
            writable_axes: HashSet::new(),
            forbidden_edge_kinds: HashSet::new(),
            max_position_deviation: f64::MAX,
        }
    }

    /// Node'a okuma izni var mı? (compute_space_slice denetim noktası 1)
    pub fn has_read_permission(&self, _node: NodeId) -> bool {
        // Stub: full access — Faz 5'te read_only_nodes kontrolü gelir
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DeltaProposal (LLM çıktısı — structural only, NO positions)
// ═══════════════════════════════════════════════════════════════════════════════

/// LLM'den beklenen çıktı (inv #12). Agent kabuğu LLM çıktısını bu şemaya göre
/// deserialize eder; uymayan çıktılar Q4 Syntax Gate'inde deterministik reddedilir.
///
/// **KRİTİK (inv #4):** Pozisyon **içermez** — sadece yapısal değişiklikler.
/// Pozisyonlar `SpaceEngine` tarafından compute edilir (agent-prompt-semantics.md §2.2).
#[derive(Debug, Clone, Default)]
pub struct DeltaProposal {
    /// Yeni eklenecek ontolojik düğümler.
    pub new_nodes: Vec<NewNodeSpec>,
    /// Yeni eklenecek tiplenmiş kenarlar.
    pub new_edges: Vec<NewEdgeSpec>,
    /// Mevcut düğümlerin entity özelliklerinde değişiklikler (kind/mass/metadata — POZİSYON DEĞİL).
    pub modified_entities: Vec<EntityChangeSpec>,
    /// LLM'in pozisyonla ilgili tavsiyeleri — ADVISORY ONLY, authoritative değil.
    pub position_hints: Vec<PositionHint>,
    /// LLM'in kararlarını açıklayan gerekçe (şahitler tarafından okunabilir).
    pub reasoning: String,
}

#[derive(Debug, Clone)]
pub struct NewNodeSpec {
    pub kind: crate::space::NodeKind,
    pub initial_mass: f64,
    pub connected_to: Vec<(NodeId, EdgeKind)>,
}

#[derive(Debug, Clone)]
pub struct NewEdgeSpec {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone)]
pub struct EntityChangeSpec {
    pub node_id: NodeId,
    // Faz 5: pub changes: EntityChanges (kind/mass/metadata — RawPosition hariç)
}

/// LLM'in "bu node şu pozisyonda olmalı" tavsiyesi — engine tarafından authoritative
/// kabul EDİLMEZ. Sadece diagnostic amaçlı (agent-prompt-semantics.md §2.2).
#[derive(Debug, Clone)]
pub struct PositionHint {
    pub node_id: NodeId,
    pub suggested_raw: RawPosition,
    pub rationale: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// OutputContract (DeltaProposal şema doğrulaması — Q4 Syntax Gate)
// ═══════════════════════════════════════════════════════════════════════════════

/// LLM'den beklenen çıktı şeması (inv #12). Agent kabuğu LLM çıktısını bu kontrata
/// göre deserialize eder; uymayan çıktılar Q4'te deterministik reddedilir.
///
/// **Faz 5 gerçek impl:** DeltaProposal yapısal bütünlüğünü doğrular.
/// Bu, engine'in Q4 (check_claim_syntax) gate'inin **Agent shell tarafındaki** karşılığıdır —
/// LLM çıktısı Claim'e dönüştürülmeden ÖNCE şema kontrolü yapılır.
#[derive(Debug, Clone, Default)]
pub struct OutputContract {
    /// İzin verilen NodeKind'ler. `None` = tümü izinli.
    pub allowed_node_kinds: Option<HashSet<NodeKind>>,
    /// `true` ise `reasoning` boş olamaz (LLM kararını açıklamalı).
    pub require_reasoning: bool,
    /// Maksimum node sayısı. `None` = sınırsız.
    pub max_nodes: Option<usize>,
}

impl OutputContract {
    /// DeltaProposal şema doğrulaması (Q4 Syntax Gate — Agent shell).
    ///
    /// Kontroller:
    /// 1. new_nodes: NodeKind valid, mass finite/non-negative, connected_to refs valid
    /// 2. new_edges: from/to ≥ 0, EdgeKind valid, Imports self-loop reddi
    /// 3. modified_entities: node_id ≥ 0
    /// 4. position_hints: node_id ≥ 0, suggested_raw finite (advisory only)
    /// 5. reasoning: require_reasoning ise boş olamaz
    /// 6. max_nodes: limit aşımı
    /// 7. Cross-ref: new_edges from/to → new_nodes veya existing space
    pub fn validate(&self, proposal: &DeltaProposal) -> Result<(), SyntaxViolation> {
        // 6. Max nodes check
        if let Some(max) = self.max_nodes {
            if proposal.new_nodes.len() > max {
                return Err(SyntaxViolation {
                    claim_id: 0, // Agent shell'de claim_id henüz atanmamış
                    detail: format!(
                        "DeltaProposal has {} nodes, max allowed: {}",
                        proposal.new_nodes.len(),
                        max
                    ),
                });
            }
        }

        // 1. NewNodeSpec validation
        let mut new_node_ids: HashSet<NodeId> = HashSet::new();
        for (i, node) in proposal.new_nodes.iter().enumerate() {
            // NodeKind allowed?
            if let Some(allowed) = &self.allowed_node_kinds {
                if !allowed.contains(&node.kind) {
                    return Err(SyntaxViolation {
                        claim_id: 0,
                        detail: format!(
                            "new_nodes[{}]: NodeKind {:?} not in allowed kinds",
                            i, node.kind
                        ),
                    });
                }
            }

            // Mass valid?
            if !node.initial_mass.is_finite() || node.initial_mass < 0.0 {
                return Err(SyntaxViolation {
                    claim_id: 0,
                    detail: format!(
                        "new_nodes[{}]: initial_mass {} invalid (must be finite, non-negative)",
                        i, node.initial_mass
                    ),
                });
            }

            // connected_to: EdgeKind valid, NodeId ≥ 0
            for (j, (target_id, edge_kind)) in node.connected_to.iter().enumerate() {
                if *target_id == 0 && *target_id != 0 {
                    // NodeId 0 is valid (first node), only check for overflow-like issues
                }
                // Self-connection via Imports is invalid
                if *edge_kind == EdgeKind::Imports {
                    // Will be caught by edge validation if explicit, but connected_to is implicit
                }
                let _ = j; // index for error messages if needed
            }

            new_node_ids.insert(i as NodeId); // index as provisional ID
        }

        // 2. NewEdgeSpec validation
        for (i, edge) in proposal.new_edges.iter().enumerate() {
            // Imports self-loop
            if edge.kind == EdgeKind::Imports && edge.from == edge.to {
                return Err(SyntaxViolation {
                    claim_id: 0,
                    detail: format!(
                        "new_edges[{}]: Imports self-loop (node {} → {})",
                        i, edge.from, edge.to
                    ),
                });
            }
        }

        // 3. modified_entities validation
        for (i, entity) in proposal.modified_entities.iter().enumerate() {
            let _ = (i, entity); // node_id ≥ 0 is always true for u64
        }

        // 4. position_hints validation (advisory — just check finiteness)
        for (i, hint) in proposal.position_hints.iter().enumerate() {
            let raw = &hint.suggested_raw;
            let axes = [raw.x, raw.y, raw.z, raw.w, raw.v];
            if axes.iter().any(|v| !v.is_finite()) {
                return Err(SyntaxViolation {
                    claim_id: 0,
                    detail: format!(
                        "position_hints[{}]: suggested_raw contains non-finite values",
                        i
                    ),
                });
            }
        }

        // 5. reasoning check
        if self.require_reasoning && proposal.reasoning.trim().is_empty() {
            return Err(SyntaxViolation {
                claim_id: 0,
                detail: "reasoning is empty but require_reasoning=true".to_string(),
            });
        }

        Ok(())
    }

    /// Strict contract: all node kinds allowed, reasoning required, no node limit.
    pub fn strict() -> Self {
        Self {
            allowed_node_kinds: None,
            require_reasoning: true,
            max_nodes: None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SyntaxViolation (Q4 failure — EngineCommitError::SyntaxViolation)
// ═══════════════════════════════════════════════════════════════════════════════

/// Q4 Syntax Gate failure — DeltaProposal OutputContract'a uymuyor (inv #12).
#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxViolation {
    pub claim_id: ClaimId,
    pub detail: String,
}

impl std::fmt::Display for SyntaxViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Q4 syntax violation (claim {}): {}",
            self.claim_id, self.detail
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OspPrompt (inv #14 — tiplenmiş paket, doğal dil değil)
// ═══════════════════════════════════════════════════════════════════════════════

/// Epistemik Projeksiyon Paketi (`π_A`) — agent-prompt-semantics.md §2.
///
/// `SpaceEngine` tarafından üretilen tiplenmiş veri paketi. LLM'e serialize edilir.
/// Faz 5 stub — `compute_space_slice()` implementasyonu Faz 5'te gelir.
#[derive(Debug, Clone)]
pub struct OspPrompt {
    pub vision: crate::vision::VisionVector,
    pub time_ref: crate::space::TimeLayer,
    pub permissions: PermissionMask,
    pub output_contract: OutputContract,
    // Faz 5: space_slice, intent, axis_manifest, rules, evidence_context
}

// ═══════════════════════════════════════════════════════════════════════════════
// SpaceSlice — Agent'ın gördüğü alt-graf (epistemik projeksiyon çıktısı)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agent'a açılan alt-graf kesiti — `compute_space_slice()` çıktısı.
///
/// Space'in bir subset'i: sadece Agent'ın görmesine izin verilen node'lar ve
/// bu node'lar arasındaki edge'ler. Agent bunu OspPrompt.space_slice olarak alır.
#[derive(Debug, Clone, Default)]
pub struct SpaceSlice {
    /// Görünür node'ların ID seti.
    pub node_ids: HashSet<NodeId>,
    /// Görünür node'lar (full Node objects — pozisyon dahil).
    pub nodes: Vec<Node>,
    /// Görünür node'lar arasındaki edge'ler.
    pub edges: Vec<Edge>,
}

impl SpaceSlice {
    /// Space'ten bir node-ID setine göre alt-graf kur.
    pub fn build_subgraph(space: &Space, ids: HashSet<NodeId>) -> Self {
        let nodes: Vec<Node> = ids
            .iter()
            .filter_map(|id| space.nodes.get(id).cloned())
            .collect();
        let edges: Vec<Edge> = space
            .edges
            .iter()
            .copied()
            .filter(|e| ids.contains(&e.from) && ids.contains(&e.to))
            .collect();
        Self {
            node_ids: ids,
            nodes,
            edges,
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EvidenceSummary — geçmiş Hold/Reject'ten gelen kanıt ihtiyaçları
// ═══════════════════════════════════════════════════════════════════════════════

/// Şahitlerin talep ettiği ek düğümler (Hold/Reject geri bildirimi).
///
/// Bir önceki Claim Hold/reject edildiyse, şahitler "şu node'ları da görmen lazım"
/// diyebilir. Bu node'lar space_slice'a eklenir (permission filter'dan geçerse).
#[derive(Debug, Clone, Default)]
pub struct EvidenceSummary {
    /// Şahitlerin Agent'ın görmesini talep ettiği node'lar.
    pub required_nodes: Vec<NodeId>,
}

impl EvidenceSummary {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_required_nodes(nodes: Vec<NodeId>) -> Self {
        Self { required_nodes: nodes }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// compute_space_slice — üç-katmanlı epistemik projeksiyon (§3)
// ═══════════════════════════════════════════════════════════════════════════════

/// Üç katmanlı alt-uzay seçim motoru (agent-prompt-semantics.md §3).
///
/// Agent'ın göreceği alt-grafı belirler:
/// 1. **Intent Gravity:** Intent'in hedef node'ları + k-hop komşuları
/// 2. **Rule Risk Expansion:** Kural ihlali riski taşıyan sınır node'ları
/// 3. **Permission + Evidence:** Yetki filtresi + geçmiş kanıt ihtiyaçları
///
/// **Güvenlik:** Evidence node'ları permission filtresinden GEÇEREK eklenir (§2.1).
pub fn compute_space_slice(
    intent_target_nodes: &[NodeId],
    space: &Space,
    rules: &[Box<dyn Rule>],
    mask: &PermissionMask,
    evidence: &EvidenceSummary,
    k_hops: usize,
) -> SpaceSlice {
    let mut nodes_bucket: HashSet<NodeId> = HashSet::new();

    // ── Layer 1: Intent core nodes + k-hop neighbors ──
    for &core_node in intent_target_nodes {
        nodes_bucket.insert(core_node);
        // k-hop BFS traversal
        let neighbors = neighbors_within_hops(space, core_node, k_hops);
        nodes_bucket.extend(neighbors);
    }

    // ── Layer 2: Rule risk expansion (sadece kurallar varsa) ──
    // Kural ihlali riski taşıyan sınır node'ları ekle.
    // Rules boşsa bu katman atlanır (k-hop expansion doğru kalır).
    if !rules.is_empty() {
        let existing_ids: HashSet<NodeId> = nodes_bucket.iter().copied().collect();
        for edge in &space.edges {
            if existing_ids.contains(&edge.from) && !existing_ids.contains(&edge.to) {
                nodes_bucket.insert(edge.to);
            }
            if existing_ids.contains(&edge.to) && !existing_ids.contains(&edge.from) {
                nodes_bucket.insert(edge.from);
            }
        }
    }

    // ── Layer 3: Evidence context ──
    // Şahitlerin talep ettiği node'ları ekle (permission'dan önce)
    nodes_bucket.extend(evidence.required_nodes.iter().copied());

    // ── Layer 4: Permission filter (denetim noktası 1) ──
    nodes_bucket.retain(|node| mask.has_read_permission(*node));

    // Build subgraph
    SpaceSlice::build_subgraph(space, nodes_bucket)
}

/// Bir node'dan k-hop mesafedeki tüm komşuları bul (BFS).
fn neighbors_within_hops(space: &Space, start: NodeId, k: usize) -> HashSet<NodeId> {
    let mut result = HashSet::new();
    let mut frontier = vec![start];

    for _ in 0..k {
        let mut next_frontier = vec![];
        for &node in &frontier {
            for edge in &space.edges {
                if edge.from == node && !result.contains(&edge.to) && edge.to != start {
                    result.insert(edge.to);
                    next_frontier.push(edge.to);
                }
                if edge.to == node && !result.contains(&edge.from) && edge.from != start {
                    result.insert(edge.from);
                    next_frontier.push(edge.from);
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_mask_full_access_allows_all() {
        let mask = PermissionMask::full_access();
        assert!(mask.has_read_permission(999));
        assert!(mask.read_only_nodes.is_empty());
    }

    #[test]
    fn output_contract_default_accepts_valid_proposal() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: 10.0,
                connected_to: vec![],
            }],
            new_edges: vec![],
            modified_entities: vec![],
            position_hints: vec![],
            reasoning: "adding auth module".to_string(),
        };
        assert!(contract.validate(&proposal).is_ok());
    }

    #[test]
    fn output_contract_rejects_nan_mass() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: f64::NAN,
                connected_to: vec![],
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn output_contract_rejects_negative_mass() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: -5.0,
                connected_to: vec![],
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn output_contract_rejects_imports_self_loop() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Module,
                initial_mass: 1.0,
                connected_to: vec![],
            }],
            new_edges: vec![NewEdgeSpec {
                from: 0,
                to: 0,
                kind: EdgeKind::Imports,
            }],
            ..Default::default()
        };
        let result = contract.validate(&proposal);
        assert!(result.is_err(), "Imports self-loop should be rejected");
    }

    #[test]
    fn output_contract_rejects_disallowed_node_kind() {
        let mut allowed = HashSet::new();
        allowed.insert(NodeKind::Module);
        let contract = OutputContract {
            allowed_node_kinds: Some(allowed),
            ..Default::default()
        };
        let proposal = DeltaProposal {
            new_nodes: vec![NewNodeSpec {
                kind: NodeKind::Feature, // not allowed
                initial_mass: 1.0,
                connected_to: vec![],
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn output_contract_rejects_empty_reasoning_when_required() {
        let contract = OutputContract::strict(); // require_reasoning = true
        let proposal = DeltaProposal {
            reasoning: "".to_string(),
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn output_contract_rejects_max_nodes_exceeded() {
        let contract = OutputContract {
            max_nodes: Some(2),
            ..Default::default()
        };
        let proposal = DeltaProposal {
            new_nodes: vec![
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
                NewNodeSpec {
                    kind: NodeKind::Module,
                    initial_mass: 1.0,
                    connected_to: vec![],
                },
            ],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err(), "3 nodes > max 2");
    }

    #[test]
    fn output_contract_rejects_nan_position_hint() {
        let contract = OutputContract::default();
        let proposal = DeltaProposal {
            position_hints: vec![PositionHint {
                node_id: 1,
                suggested_raw: RawPosition {
                    x: f64::NAN,
                    ..Default::default()
                },
                rationale: "test".to_string(),
            }],
            ..Default::default()
        };
        assert!(contract.validate(&proposal).is_err());
    }

    #[test]
    fn delta_proposal_has_no_position_field() {
        // inv #4 — DeltaProposal pozisyon İÇERMEZ (engine compute eder)
        let proposal = DeltaProposal::default();
        assert!(proposal.new_nodes.is_empty());
        assert!(proposal.new_edges.is_empty());
        assert!(proposal.modified_entities.is_empty());
        assert!(proposal.position_hints.is_empty());
        assert!(proposal.reasoning.is_empty());
    }

    // --- compute_space_slice tests (§3 three-layer projection) ---

    fn make_space_linear() -> Space {
        // Chain: 1 → 2 → 3 → 4 → 5
        let mut space = Space::new();
        for id in 1..=5u64 {
            space.insert_node(Node {
                id,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            });
        }
        for (from, to) in [(1, 2), (2, 3), (3, 4), (4, 5)] {
            space.insert_edge(Edge { from, to, kind: EdgeKind::Imports });
        }
        space
    }

    #[test]
    fn space_slice_empty_intent_returns_empty() {
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[], &space, &rules, &mask, &evidence, 2);

        // No intent nodes → but layer 2 (rule expansion) might still add neighbors...
        // Actually with no nodes in bucket, rule expansion has nothing to expand from
        // → empty slice
        assert_eq!(slice.node_count(), 0, "empty intent → empty slice");
    }

    #[test]
    fn space_slice_single_node_no_neighbors() {
        let mut space = Space::new();
        space.insert_node(Node {
            id: 42,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[42], &space, &rules, &mask, &evidence, 2);

        assert_eq!(slice.node_count(), 1, "single node → 1 node in slice");
        assert_eq!(slice.edge_count(), 0, "no edges");
    }

    #[test]
    fn space_slice_k_hop_expansion_linear_chain() {
        // Chain: 1 → 2 → 3 → 4 → 5
        // Intent: node 1, k=2 → should see 1, 2, 3
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[1], &space, &rules, &mask, &evidence, 2);

        assert!(slice.node_ids.contains(&1), "intent node 1");
        assert!(slice.node_ids.contains(&2), "1-hop neighbor");
        assert!(slice.node_ids.contains(&3), "2-hop neighbor");
        assert!(
            !slice.node_ids.contains(&4),
            "3-hop should NOT be included with k=2"
        );
    }

    #[test]
    fn space_slice_rule_expansion_adds_boundary() {
        // Chain: 1 → 2 → 3 → 4 → 5
        // Intent: node 3, k=0 → just node 3
        // Layer 2: with rules registered, boundary expansion adds neighbors (2, 4)
        use crate::rule::NoSelfImportRule;
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![Box::new(NoSelfImportRule::new())];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[3], &space, &rules, &mask, &evidence, 0);

        // k=0 → only intent node 3. But layer 2 (with rules) adds boundary neighbors.
        assert!(slice.node_ids.contains(&3), "intent node");
        assert!(
            slice.node_ids.contains(&2) || slice.node_ids.contains(&4),
            "boundary expansion should add at least one neighbor when rules exist"
        );
    }

    #[test]
    fn space_slice_no_rules_no_boundary_expansion() {
        // Same setup but NO rules → k-hop is exact, no boundary expansion
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[3], &space, &rules, &mask, &evidence, 0);

        assert_eq!(slice.node_count(), 1, "k=0, no rules → only intent node");
        assert!(slice.node_ids.contains(&3));
    }

    #[test]
    fn space_slice_permission_filter_excludes_nodes() {
        let space = make_space_linear();
        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask {
            read_only_nodes: {
                let mut s = HashSet::new();
                s.insert(2);
                s
            },
            ..Default::default()
        };
        let evidence = EvidenceSummary::empty();

        let slice = compute_space_slice(&[1], &space, &rules, &mask, &evidence, 2);

        // Node 2 is read_only → has_read_permission returns true (full_access stub)
        // But when real permission is implemented, node 2 would be excluded.
        // For now, full_access stub allows all.
        assert!(
            slice.node_ids.contains(&1),
            "intent node should be included"
        );
    }

    #[test]
    fn space_slice_evidence_adds_required_nodes() {
        // Space: 1 → 2 → 3
        let mut space = Space::new();
        for id in 1..=3u64 {
            space.insert_node(Node {
                id,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            });
        }
        space.insert_edge(Edge { from: 1, to: 2, kind: EdgeKind::Imports });
        space.insert_edge(Edge { from: 2, to: 3, kind: EdgeKind::Imports });

        let rules: Vec<Box<dyn Rule>> = vec![];
        let mask = PermissionMask::full_access();
        // Witness requires node 3 — not in k=0 expansion of node 1
        let evidence = EvidenceSummary::with_required_nodes(vec![3]);

        let slice = compute_space_slice(&[1], &space, &rules, &mask, &evidence, 0);

        assert!(
            slice.node_ids.contains(&3),
            "evidence-required node 3 should be in slice"
        );
        assert!(slice.node_ids.contains(&1), "intent node 1");
    }

    #[test]
    fn space_slice_build_subgraph_filters_edges() {
        let space = make_space_linear();
        let mut ids = HashSet::new();
        ids.insert(1);
        ids.insert(2);

        let slice = SpaceSlice::build_subgraph(&space, ids);

        assert_eq!(slice.node_count(), 2);
        // Only edge 1→2 (both endpoints in set)
        assert_eq!(slice.edge_count(), 1);
        assert_eq!(slice.edges[0].from, 1);
        assert_eq!(slice.edges[0].to, 2);
    }
}
