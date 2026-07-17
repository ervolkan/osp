//! Rule engine stub — Q6 Rule Gate (agent-prompt-semantics.md §4, space-engine-design.md §4).
//!
//! Sistem invariant'larını (`Rule`) temsil eder. Bir Claim'in ΔS'i kuralları ihlal
//! ederse Q6 Rule Gate reddeder (pre-mutation, witness öncesi).
//!
//! **Faz 2 stub** — tipler tanımlı, implementasyon Faz 5'te gelir.
//! Engine'de `check_claim_rules()` stub olarak her zaman `Ok(())` döner.

use crate::space::Space;

// ═══════════════════════════════════════════════════════════════════════════════
// RuleId + Rule trait
// ═══════════════════════════════════════════════════════════════════════════════

/// Kural tanımlayıcısı — `"arch.layer_separation"`, `"security.no_unsanitized_sql"`.
pub type RuleId = String;

/// Sistem invariant'ı (agent-prompt-semantics.md §4 Q6).
///
/// Bir Rule, Claim'in ΔS'i (yeni node/edge/mutasyon) üzerinde değerlendirilir.
/// İhlal tespit edilirse `Some(RuleViolation)` döner → Q6 reject.
///
/// **Hard Rule vs Soft Rule:**
/// - Hard Rule: statik, değişmez (mimari katman ihlalleri) — StaticGravityIndex'te cache
/// - Soft Rule: dinamik, context-bağımlı — lazy compute
///
/// Faz 5'te gerçek Rule implementasyonları gelir (örn: LayerSeparationRule,
/// NoDirectDbAccessRule). Şu an sadece trait + stub.
pub trait Rule: Send + Sync {
    /// Kural tanımlayıcısı.
    fn id(&self) -> &RuleId;

    /// **INV-T9 Step 4a (reviewer P0-3):** Kural descriptor'ı — `EvaluationContextDigest` için.
    ///
    /// **Default impl YOK** — her rule explicit descriptor beyan etmeli. Axis katmanındaki
    /// zorunlu `Axis::descriptor()` pattern'ı ile aynı: parametreli/farklı semantiğe
    /// sahip custom rule descriptor'ı override etmeyi unutursa Q6 davranışı değişebilir
    /// ama digest aynı kalırdı. Şimdi explicit declaration zorunlu.
    ///
    /// `rule_id` + `semantics_version` + `canonical_parameters`. Rule implementasyonu
    /// değişirse `semantics_version` artırılmalı; bu `EvaluationContextDigest`'i
    /// değiştirir → stale measurement tespiti çalışır.
    ///
    /// **reviewer P2 (determinism contract):** `descriptor()` saf ve deterministik
    /// olmalıdır — aynı değişmeyen rule state'i için her çağrıda aynı sonucu vermelidir.
    /// `evaluate()`'ı etkileyebilecek tüm parametre ve semantics version'ı bağlamalıdır.
    /// `evaluate()` bir değerlendirme sırasında descriptor-affecting state'i MUTATE
    /// ETMEMELİDİR — aksi halde captured context propagation (Q6 ↔ digest aynı snapshot)
    /// güvenilir olmaz.
    fn descriptor(&self) -> crate::authorization::RuleDescriptor;

    /// Kuralın ΔS üzerinde ihlal durumunu değerlendir.
    ///
    /// `None` = ihlal yok (Q6 geçer). `Some(violation)` = ihlal tespit edildi (Q6 reject).
    ///
    /// Stub implementasyonu: her zaman `None` döner (Faz 5'te gerçek logic).
    fn evaluate(
        &self,
        _new_nodes: &[crate::space::Node],
        _new_edges: &[crate::space::Edge],
        _space: &Space,
    ) -> Option<RuleViolation> {
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Concrete Rule implementations (Q6 default rules)
// ═══════════════════════════════════════════════════════════════════════════════

use crate::space::{Edge, EdgeKind, Node};

/// **NoSelfImportRule** — bir modül kendini import edemez (`Imports` self-loop).
///
/// Semantik: `import self` anlamsız — modül zaten kendi içeriğine sahip.
/// Bu kural hem Q4 (syntax) hem de Q6 (rule) seviyesinde çalışabilir;
/// Q6'da Hard severity ile reddedilir.
pub struct NoSelfImportRule {
    id: RuleId,
}

impl NoSelfImportRule {
    pub fn new() -> Self {
        Self {
            id: "structural.no_self_import".to_string(),
        }
    }
}

impl Default for NoSelfImportRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NoSelfImportRule {
    fn id(&self) -> &RuleId {
        &self.id
    }
    /// **Step 4a:** Explicit descriptor — parametresiz graph-self-loop rule.
    /// Algoritma değişirse (örn sadece Imports değil diğer kind'lar) semantics_version artır.
    fn descriptor(&self) -> crate::authorization::RuleDescriptor {
        crate::authorization::RuleDescriptor {
            rule_id: self.id.clone(),
            semantics_version: 1,
            canonical_parameters: vec![],
        }
    }
    fn evaluate(&self, _nodes: &[Node], edges: &[Edge], _space: &Space) -> Option<RuleViolation> {
        for e in edges {
            if e.kind == EdgeKind::Imports && e.from == e.to {
                return Some(RuleViolation {
                    rule_id: self.id.clone(),
                    detail: format!("node {} imports itself", e.from),
                    severity: RuleSeverity::Hard,
                });
            }
        }
        None
    }
}

/// **DuplicateNodeRule** — mevcut uzayda zaten var olan bir node ID'sini ekleme.
///
/// Semantik: her NodeId benzersiz olmalı. Delta, space'te zaten var olan bir ID'yi
/// tekrar ekleyemez (overwrite yerine yeni ID kullanılmalı).
pub struct DuplicateNodeRule {
    id: RuleId,
}

impl DuplicateNodeRule {
    pub fn new() -> Self {
        Self {
            id: "structural.no_duplicate_node".to_string(),
        }
    }
}

impl Default for DuplicateNodeRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for DuplicateNodeRule {
    fn id(&self) -> &RuleId {
        &self.id
    }
    /// **Step 4a:** Explicit descriptor — parametresiz node-id uniqueness rule.
    fn descriptor(&self) -> crate::authorization::RuleDescriptor {
        crate::authorization::RuleDescriptor {
            rule_id: self.id.clone(),
            semantics_version: 1,
            canonical_parameters: vec![],
        }
    }
    fn evaluate(&self, nodes: &[Node], _edges: &[Edge], space: &Space) -> Option<RuleViolation> {
        for n in nodes {
            if space.nodes.contains_key(&n.id) {
                return Some(RuleViolation {
                    rule_id: self.id.clone(),
                    detail: format!("node {} already exists in space", n.id),
                    severity: RuleSeverity::Hard,
                });
            }
        }
        None
    }
}

/// **EdgeTargetExistsRule** — kenar uçları (from/to) geçerli olmalı.
///
/// Bir kenarın from ve to node'ları ya mevcut space'te ya da delta_nodes içinde
/// tanımlı olmalıdır. Olmayan bir node'a edge eklemek geçersizdir.
pub struct EdgeTargetExistsRule {
    id: RuleId,
}

impl EdgeTargetExistsRule {
    pub fn new() -> Self {
        Self {
            id: "structural.edge_target_exists".to_string(),
        }
    }
}

impl Default for EdgeTargetExistsRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for EdgeTargetExistsRule {
    fn id(&self) -> &RuleId {
        &self.id
    }
    /// **Step 4a:** Explicit descriptor — parametresiz edge-target existence rule.
    fn descriptor(&self) -> crate::authorization::RuleDescriptor {
        crate::authorization::RuleDescriptor {
            rule_id: self.id.clone(),
            semantics_version: 1,
            canonical_parameters: vec![],
        }
    }
    fn evaluate(&self, nodes: &[Node], edges: &[Edge], space: &Space) -> Option<RuleViolation> {
        let delta_ids: std::collections::HashSet<u64> = nodes.iter().map(|n| n.id).collect();
        for e in edges {
            let from_exists = space.nodes.contains_key(&e.from) || delta_ids.contains(&e.from);
            let to_exists = space.nodes.contains_key(&e.to) || delta_ids.contains(&e.to);
            if !from_exists {
                return Some(RuleViolation {
                    rule_id: self.id.clone(),
                    detail: format!(
                        "edge from={} does not exist (not in space or delta)",
                        e.from
                    ),
                    severity: RuleSeverity::Hard,
                });
            }
            if !to_exists {
                return Some(RuleViolation {
                    rule_id: self.id.clone(),
                    detail: format!("edge to={} does not exist (not in space or delta)", e.to),
                    severity: RuleSeverity::Hard,
                });
            }
        }
        None
    }
}

/// Q6 için varsayılan yapısal kural seti — SpaceEngine::with_default_rules() kullanır.
pub fn default_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(NoSelfImportRule::new()),
        Box::new(DuplicateNodeRule::new()),
        Box::new(EdgeTargetExistsRule::new()),
    ]
}

// ═══════════════════════════════════════════════════════════════════════════════
// RuleViolation (Q6 failure — EngineCommitError::RuleViolation)
// ═══════════════════════════════════════════════════════════════════════════════

/// Q6 Rule Gate failure — ΔS bir Rule'u ihlal ediyor.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleViolation {
    pub rule_id: RuleId,
    pub detail: String,
    /// İhlal edilen kural kategorisi (kalibrasyon geri bildirimi için).
    pub severity: RuleSeverity,
}

impl std::fmt::Display for RuleViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Q6 rule violation ({:?} rule {}): {}",
            self.severity, self.rule_id, self.detail
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSeverity {
    /// Kritik mimari ihlal — kesin reject.
    Hard,
    /// Yumuşak ihlal — warning + reject (Faz 5'te policy-bağımlı olabilir).
    Soft,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::{Edge, Node, NodeKind};

    // --- Concrete Rule tests ---

    #[test]
    fn no_self_import_rule_detects_self_loop() {
        let rule = NoSelfImportRule::new();
        let space = Space::new();
        let nodes: Vec<Node> = vec![];
        let edges = vec![Edge {
            from: 5,
            to: 5,
            kind: EdgeKind::Imports,
            ..Default::default()
        }];
        let v = rule.evaluate(&nodes, &edges, &space);
        assert!(v.is_some(), "self-import should be detected");
        assert_eq!(v.unwrap().rule_id, "structural.no_self_import");
    }

    #[test]
    fn no_self_import_rule_allows_normal_edge() {
        let rule = NoSelfImportRule::new();
        let space = Space::new();
        let edges = vec![Edge {
            from: 1,
            to: 2,
            kind: EdgeKind::Imports,
            ..Default::default()
        }];
        assert!(rule.evaluate(&[], &edges, &space).is_none());
    }

    #[test]
    fn no_self_import_rule_allows_other_self_loops() {
        // Calls self-loop (recursion) is semantically valid
        let rule = NoSelfImportRule::new();
        let space = Space::new();
        let edges = vec![Edge {
            from: 3,
            to: 3,
            kind: EdgeKind::Calls,
            ..Default::default()
        }];
        assert!(rule.evaluate(&[], &edges, &space).is_none());
    }

    #[test]
    fn duplicate_node_rule_detects_existing_id() {
        let rule = DuplicateNodeRule::new();
        let mut space = Space::new();
        space.insert_node(Node {
            id: 10,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let delta_nodes = vec![Node {
            id: 10,
            kind: NodeKind::Module,
            mass: 2.0,
            ..Default::default()
        }];
        let v = rule.evaluate(&delta_nodes, &[], &space);
        assert!(v.is_some(), "duplicate ID should be detected");
        assert_eq!(v.unwrap().severity, RuleSeverity::Hard);
    }

    #[test]
    fn duplicate_node_rule_allows_new_id() {
        let rule = DuplicateNodeRule::new();
        let mut space = Space::new();
        space.insert_node(Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        let delta_nodes = vec![Node {
            id: 2,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        }];
        assert!(rule.evaluate(&delta_nodes, &[], &space).is_none());
    }

    #[test]
    fn edge_target_exists_rule_detects_missing_target() {
        let rule = EdgeTargetExistsRule::new();
        let space = Space::new();
        // Edge references node 99 which doesn't exist
        let edges = vec![Edge {
            from: 1,
            to: 99,
            kind: EdgeKind::Imports,
            ..Default::default()
        }];
        let v = rule.evaluate(&[], &edges, &space);
        assert!(v.is_some(), "missing edge target should be detected");
    }

    #[test]
    fn edge_target_exists_rule_allows_delta_provided_node() {
        let rule = EdgeTargetExistsRule::new();
        let space = Space::new();
        // Node 99 is in delta_nodes — edge is valid
        let nodes = vec![Node {
            id: 99,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        }];
        let edges = vec![Edge {
            from: 1,
            to: 99,
            kind: EdgeKind::Imports,
            ..Default::default()
        }];
        // from=1 also needs to exist — add it to delta
        let nodes = vec![
            Node {
                id: 1,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            },
            nodes[0].clone(),
        ];
        assert!(rule.evaluate(&nodes, &edges, &space).is_none());
    }

    #[test]
    fn default_rules_has_three_rules() {
        let rules = default_rules();
        assert_eq!(rules.len(), 3);
    }

    #[test]
    fn rule_violation_carries_severity() {
        let v = RuleViolation {
            rule_id: "arch.layer".to_string(),
            detail: "domain → infrastructure direct".to_string(),
            severity: RuleSeverity::Hard,
        };
        assert_eq!(v.severity, RuleSeverity::Hard);
    }
}
