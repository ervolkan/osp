//! Zaman FSM — `TimeMachine` trait + `TimeFSM` (§3).
//!
//! **Faz 1.8 + Faz 2.4 refactor:** `TimeFSM::advance()` = `evaluate` (Q1-Q3 karar) +
//! `apply_delta` (mutasyon) composition. `bigbang::commit()` kaldırıldı — ayrım:
//! - `witness::evaluate(claim, omega)` → karar (Commit/Reject/Hold)
//! - `bigbang::apply_delta(space, &delta)` → sadece mutasyon
//! - `advance()` → bu ikisinin kompozisyonu, `WitnessResult` döner
//!
//! Stateless (Faz 2 snapshot/persist engine seviyesinde).

use crate::bigbang::apply_delta;
use crate::space::Space;
use crate::witness::{evaluate, Claim, WitnessResult, WitnessSet};

// ═══════════════════════════════════════════════════════════════════════════════
// TimeMachine trait
// ═══════════════════════════════════════════════════════════════════════════════

/// Zaman makinesi — `t_c`'yi ilerleten arayüz (§3.2).
///
/// `advance()` bir Claim'i değerlendirir (Q1-Q3) + (Commit ise) `apply_delta` ile
/// space'i mutasyona uğratır. Dönüş: `WitnessResult` (Commit/Reject/Hold).
pub trait TimeMachine {
    fn advance(&mut self, space: &mut Space, claim: &Claim, omega: &WitnessSet) -> WitnessResult;
}

// ═══════════════════════════════════════════════════════════════════════════════
// TimeFSM — stateless implementasyon (evaluate + apply_delta composition)
// ═══════════════════════════════════════════════════════════════════════════════

/// Stateless zaman FSM'i.
///
/// `evaluate(claim, omega)` → Q1-Q3 karar. Commit ise `apply_delta(space, &delta)`
/// ile space'e mutasyon uygulanır (node/edge ekle, repositioned hesapla). Hold/Reject
/// ise space dokunulmaz.
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeFSM;

impl TimeMachine for TimeFSM {
    fn advance(&mut self, space: &mut Space, claim: &Claim, omega: &WitnessSet) -> WitnessResult {
        match evaluate(claim, omega) {
            WitnessResult::Commit {
                mut delta,
                safety_weakened,
                override_reason,
            } => {
                // Mutasyon: apply_delta node/edge ekler + repositioned set döner (inv #6)
                let repositioned = apply_delta(space, &delta);
                delta.repositioned = repositioned;
                WitnessResult::Commit {
                    delta,
                    safety_weakened,
                    override_reason,
                }
            }
            WitnessResult::Hold(reason) => WitnessResult::Hold(reason),
            WitnessResult::Reject(reason) => WitnessResult::Reject(reason),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::RawPosition;
    use crate::space::{Node, NodeKind};
    use crate::witness::{AgentId, EvidenceEvent, EvidenceId, Intent, WitnessKind};

    fn claim_with(author: AgentId, delta_node_id: u64) -> Claim {
        Claim {
            id: 1,
            intent: Intent::new(author, RawPosition::default()),
            author,
            computed_raw: RawPosition::default(),
            delta_nodes: vec![Node {
                id: delta_node_id,
                kind: NodeKind::Module,
                ..Default::default()
            }],
            delta_edges: vec![],
            task_id: None, // standalone (Paper 1 static flow, INV-T5)
        }
    }
    fn ev(id: EvidenceId, actor: AgentId) -> EvidenceEvent {
        EvidenceEvent::new(id, &format!("s{id}"), WitnessKind::MergeCommit, actor, 1)
    }

    #[test]
    fn advance_commit_mutates_space_and_returns_commit() {
        let mut fsm = TimeFSM::default();
        let mut space = Space::new();
        let claim = claim_with(100, 42);
        let omega = WitnessSet::new(vec![ev(1, 200), ev(2, 300)]);
        let result = fsm.advance(&mut space, &claim, &omega);
        assert!(matches!(result, WitnessResult::Commit { .. }));
        assert_eq!(space.node_count(), 1, "node 42 eklenmiş olmalı");
        assert!(space.nodes.contains_key(&42));
    }

    #[test]
    fn advance_hold_does_not_mutate_space() {
        let mut fsm = TimeFSM::default();
        let mut space = Space::new();
        let claim = claim_with(100, 42);
        let omega = WitnessSet::new(vec![ev(1, 200)]); // Hold (tek witness)
        let result = fsm.advance(&mut space, &claim, &omega);
        assert!(matches!(result, WitnessResult::Hold(_)));
        assert_eq!(space.node_count(), 0, "Hold sonrası mutasyon olmamalı");
    }

    #[test]
    fn advance_commit_result_carries_actual_delta() {
        let mut fsm = TimeFSM::default();
        let mut space = Space::new();
        let claim = claim_with(100, 42);
        let omega = WitnessSet::new(vec![ev(1, 200), ev(2, 300)]);
        if let WitnessResult::Commit { delta, .. } = fsm.advance(&mut space, &claim, &omega) {
            assert_eq!(delta.new_nodes.len(), 1);
            assert_eq!(delta.new_nodes[0].id, 42);
            // repositioned ΔV ∪ N₁(ΔV) — node 42 eklendi, komşu yok → sadece {42}
            assert!(delta.repositioned.contains(&42));
        } else {
            panic!("Commit bekleniyordu");
        }
    }

    #[test]
    fn advance_propagates_safety_weakened() {
        let mut fsm = TimeFSM::default();
        let mut space = Space::new();
        let claim = claim_with(100, 42);
        let omega = WitnessSet::new(vec![ev(1, 200), ev(2, 300)]);
        if let WitnessResult::Commit {
            safety_weakened, ..
        } = fsm.advance(&mut space, &claim, &omega)
        {
            // Faz 1.5 evaluate hep false (admin override Faz 1.11+)
            assert!(!safety_weakened);
        } else {
            panic!("Commit bekleniyordu");
        }
    }

    #[test]
    fn timefsm_is_default_constructible_and_stateless() {
        let mut fsm = TimeFSM::default();
        let mut space = Space::new();
        let claim = claim_with(100, 1);
        let omega = WitnessSet::new(vec![ev(1, 200), ev(2, 300)]);
        // İlk advance
        let _ = fsm.advance(&mut space, &claim, &omega);
        // FSM stateless — ikinci advance aynı claim ile tekrar çalışır (idempotent FSM)
        let claim2 = claim_with(100, 2);
        let result = fsm.advance(&mut space, &claim2, &omega);
        assert!(matches!(result, WitnessResult::Commit { .. }));
        assert_eq!(space.node_count(), 2);
    }

    #[test]
    fn multiple_advances_accumulate_nodes() {
        let mut fsm = TimeFSM::default();
        let mut space = Space::new();
        let omega = WitnessSet::new(vec![ev(1, 200), ev(2, 300)]);
        for i in 1..=5 {
            let claim = claim_with(100, i);
            let r = fsm.advance(&mut space, &claim, &omega);
            assert!(
                matches!(r, WitnessResult::Commit { .. }),
                "advance #{i} Commit olmalı"
            );
        }
        assert_eq!(space.node_count(), 5, "5 commit → 5 node");
    }
}
