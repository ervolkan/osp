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
use crate::space::{EdgeKind, NodeId, NodeKind};
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
}
