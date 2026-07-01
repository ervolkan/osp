//! Big Bang — uzay genişlemesi: mutation only (§6).
//!
//! **Faz 1.8 + Faz 2.4 refactor:** Sorumluluk ayrımı (space-engine-design.md §3.5,
//! osp-core-design.md §3.4):
//! - `witness::evaluate(claim, omega) -> WitnessResult` → Q1-Q3 karar (Commit/Reject/Hold)
//! - `bigbang::apply_delta(space, &Delta) -> Vec<NodeId>` → **sadece mutasyon** (infallible)
//! - `SpaceEngine::commit(claim, omega) -> Result<CommitOutcome, EngineCommitError>` → orchestration
//!
//! **ÖNEMLİ:** `commit()` ve `CommitError` kaldırıldı (Faz 2.4 ayrımı). bigbang modülü
//! artık mutation-only; kendi error enum'ı yoktur (`apply_delta` infallible).
//!
//! `apply_delta` pozisyon recomputasyonu **YAPMAZ** — sadece `ΔV ∪ N₁(ΔV)` setini
//! döner (inv #6). Pozisyon güncelleme `SpaceEngine::commit()`'te `CosineDeviation` ile (inv #5).

use crate::agent::EdgeRef;
use crate::space::{Edge, Node, NodeId, Space};

// ═══════════════════════════════════════════════════════════════════════════════
// Delta + Event
// ═══════════════════════════════════════════════════════════════════════════════

/// Uzay genişlemesi sonucu — mutasyonun çıktısı.
///
/// **G2c-2 (arkadaş review 7 #5):** `removed_edges` eklendi — subtractive structural delta.
/// Coupling/instability düşürme (import kaldırma) artık Delta'da temsil edilir.
/// `apply_delta` hem ekleme hem kaldırma uygular.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Delta {
    /// Faz 2.4: full Node objects — event-sourcing replay için (DeltaRecord'tan geri yükle).
    pub new_nodes: Vec<Node>,
    pub new_edges: Vec<Edge>,
    /// **G2c-2:** Kaldırılacak kenarlar. Claim.removed_edges → Delta.removed_edges.
    #[serde(default)]
    pub removed_edges: Vec<EdgeRef>,
    /// `ΔV ∪ N₁(ΔV)` (inv #6). Engine `CosineDeviation` ile reposition yapar.
    pub repositioned: Vec<NodeId>,
}

/// Commit olayı — zamanın ilerlemesinin (`t_c+1`) somut kaydı.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub time_layer: crate::space::TimeLayer,
    pub claim_id: crate::witness::ClaimId,
    pub delta: Delta,
    /// inv #7 — admin override kullanıldıysa `true` (asla sessiz).
    /// Faz 1.5 evaluate hep `false`; override mekanizması Faz 1.11+ (role-weight).
    pub safety_weakened: bool,
    pub override_reason: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// apply_delta — sadece mutasyon (infallible, reusable)
// ═══════════════════════════════════════════════════════════════════════════════

/// `apply_delta(space, &Delta) -> Vec<NodeId>` — uzaya node + edge ekle + repositioned set hesapla.
///
/// **Mutation-only, infallible:** Geçerli delta her zaman uygulanır; error yok.
/// Witness kararı (`evaluate`) burada DEĞİL — `SpaceEngine::commit()` orchestrate eder.
///
/// **Reusable:**
/// - Live commit: `SpaceEngine::commit()` → `evaluate` Commit dönerse → `apply_delta`
/// - Time-travel replay: `persistence::restore()` → milestone + delta replay → `apply_delta`
///
/// **Pozisyon recomputasyonu YAPMAZ** — `repositioned` setini döner (inv #6). Gerçek
/// pozisyon güncelleme engine'de `CosineDeviation` ile (inv #5).
///
/// Returns: `repositioned = ΔV ∪ N₁(ΔV)` (sorted Vec, deterministik test için).
pub fn apply_delta(space: &mut Space, delta: &Delta) -> Vec<NodeId> {
    let mut new_node_ids: Vec<NodeId> = Vec::with_capacity(delta.new_nodes.len());
    for n in &delta.new_nodes {
        let id = n.id;
        space.insert_node(n.clone());
        new_node_ids.push(id);
    }
    for e in &delta.new_edges {
        space.insert_edge(*e);
    }
    // G2c-2: subtractive structural delta — edge kaldırma (coupling/instability düşürme).
    for er in &delta.removed_edges {
        space.remove_edge(er.from, er.to, er.kind);
    }
    compute_reposition_set(space, &new_node_ids)
}

/// `ΔV ∪ N₁(ΔV)` — yeniden konumlanması gereken düğüm seti (inv #6).
///
/// `ΔV`: commit ile eklenen yeni node'lar. `N₁(ΔV)`: bu node'lara komşu (1-hop)
/// mevcut node'lar. Sorted Vec döner (deterministik test için).
fn compute_reposition_set(space: &Space, delta_v: &[NodeId]) -> Vec<NodeId> {
    use std::collections::HashSet;
    let delta_set: HashSet<NodeId> = delta_v.iter().copied().collect();
    let mut result: HashSet<NodeId> = delta_set.clone();
    for e in &space.edges {
        if delta_set.contains(&e.from) {
            result.insert(e.to);
        }
        if delta_set.contains(&e.to) {
            result.insert(e.from);
        }
    }
    let mut v: Vec<NodeId> = result.into_iter().collect();
    v.sort_unstable();
    v
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler — apply_delta mutation only (composition tests time.rs'te)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::{Edge, EdgeKind, Node, NodeKind, TimeLayer};

    fn mod_node(id: u64) -> Node {
        Node {
            id,
            kind: NodeKind::Module,
            ..Default::default()
        }
    }

    fn edge(from: u64, to: u64) -> Edge {
        Edge {
            from,
            to,
            kind: EdgeKind::Imports,
            ..Default::default()
        }
    }

    fn delta(nodes: Vec<Node>, edges: Vec<Edge>) -> Delta {
        Delta {
            new_nodes: nodes,
            new_edges: edges,
            removed_edges: vec![], // G2c-2
            repositioned: vec![],
        }
    }

    // --- apply_delta mutasyon ---

    #[test]
    fn apply_delta_adds_nodes_edges_and_returns_repositioned() {
        let mut space = Space::new();
        space.insert_node(mod_node(1)); // existing
        let d = delta(vec![mod_node(10), mod_node(11)], vec![edge(10, 1)]);

        let repositioned = apply_delta(&mut space, &d);
        assert_eq!(space.node_count(), 3); // 1 + 10 + 11
        assert_eq!(space.edge_count(), 1); // 10→1
                                           // repositioned = {10, 11} ∪ N₁({10,11}) = {10, 11, 1}
        assert!(repositioned.contains(&10), "ΔV dahil");
        assert!(repositioned.contains(&11), "ΔV dahil");
        assert!(repositioned.contains(&1), "N₁(ΔV) dahil (10→1)");
    }

    #[test]
    fn apply_delta_empty_delta_returns_empty() {
        let mut space = Space::new();
        let repositioned = apply_delta(&mut space, &Delta::default());
        assert!(repositioned.is_empty());
        assert_eq!(space.node_count(), 0);
    }

    #[test]
    fn apply_delta_reusable_for_replay_scenario() {
        // Simulate replay: milestone has 1 node, delta adds 2 more
        let mut space = Space::new();
        space.insert_node(mod_node(1)); // "milestone" state

        // "DeltaRecord" replay
        let d = delta(vec![mod_node(2), mod_node(3)], vec![edge(1, 2), edge(2, 3)]);
        let repositioned = apply_delta(&mut space, &d);

        // After replay: 3 nodes, 2 edges
        assert_eq!(space.node_count(), 3);
        assert_eq!(space.edge_count(), 2);
        // repositioned: ΔV={2,3} ∪ N₁({2,3})={1} → {1,2,3}
        assert_eq!(repositioned.len(), 3);
    }

    // --- inv #6: incremental reposition (ΔV ∪ N₁(ΔV)) ---

    #[test]
    fn apply_delta_repositioned_includes_delta_v_and_one_hop_neighbors() {
        // space'te: node 1, node 2, edge 1→2 (existing).
        let mut space = Space::new();
        space.insert_node(mod_node(1));
        space.insert_node(mod_node(2));
        space.insert_edge(edge(1, 2));

        // apply_delta: node 10 ekle (ΔV={10}), edge 10→1 ekle → N₁({10}) = {1}.
        // repositioned = {10, 1}. Node 2 dışarıda (10'ya komşu değil).
        let d = delta(vec![mod_node(10)], vec![edge(10, 1)]);
        let repositioned = apply_delta(&mut space, &d);

        assert!(repositioned.contains(&10), "ΔV dahil");
        assert!(repositioned.contains(&1), "N₁(ΔV) dahil (10→1)");
        assert!(
            !repositioned.contains(&2),
            "node 2 ΔV ∪ N₁(ΔV) dışı — repositioned olmamalı (inv #6)"
        );
    }

    #[test]
    fn apply_delta_repositioned_excludes_unrelated_nodes() {
        // İzole node 99 var; apply_delta node 10 ekler. 99 repositioned DIŞI.
        let mut space = Space::new();
        space.insert_node(mod_node(99));
        let d = delta(vec![mod_node(10)], vec![]);
        let repositioned = apply_delta(&mut space, &d);
        assert!(repositioned.contains(&10));
        assert!(
            !repositioned.contains(&99),
            "izole node repositioned olmamalı (inv #6)"
        );
    }

    #[test]
    fn apply_delta_repositioned_covers_multi_hop_via_delta_edges() {
        // ΔV = {10, 11}, edge 10→11. N₁({10,11}) birbirini içerir.
        let mut space = Space::new();
        let d = delta(vec![mod_node(10), mod_node(11)], vec![edge(10, 11)]);
        let repositioned = apply_delta(&mut space, &d);
        assert!(repositioned.contains(&10));
        assert!(repositioned.contains(&11));
    }

    // --- Event alanları ---

    #[test]
    fn event_carries_time_layer_and_claim_id() {
        let event = Event {
            time_layer: TimeLayer::Simdiki,
            claim_id: 42,
            delta: Delta::default(),
            safety_weakened: false,
            override_reason: None,
        };
        assert_eq!(event.claim_id, 42);
        assert_eq!(event.time_layer, TimeLayer::Simdiki);
    }

    #[test]
    fn delta_default_is_empty() {
        let d = Delta::default();
        assert!(d.new_nodes.is_empty());
        assert!(d.new_edges.is_empty());
        assert!(d.repositioned.is_empty());
    }
}
