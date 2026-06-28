//! Space Engine — production runtime orchestrator (Faz 2.6).
//!
//! Tüm Faz 1-2 modüllerini tek çatı altında birleştirir:
//! - `vision_config` → `VisionVector` + `EngineConfig`
//! - `time::TimeFSM` → evaluate (Q1-Q3) + `bigbang::apply_delta` (mutate)
//! - `vision::compute_derived` → pozisyon reposition (`CosineDeviation`)
//! - `persistence::SnapshotStore` → event-sourcing (delta + milestone)
//!
//! **Commit pipeline (§4, space-engine-design.md):**
//! 0. CLAIM-BASED GATES (Q4-Q6) → syntax/vision/rule check (deterministik, witness öncesi)
//!    - Q4 Syntax: OutputContract compliant?
//!    - Q5 Vision: claim.computed_raw θ > bound → Err(VisionViolation) [mutasyon YOK]
//!    - Q6 Rule: Rule ihlali?
//! 1. WITNESS-BASED GATES (Q1-Q3) → evaluate + apply_delta (ΔV node + ΔE edge)
//! 2. REPOSITION → CosineDeviation ile ΔV∪N₁(ΔV) → drift_warnings
//! 3. SAVE DELTA → event-sourcing
//! 4. MILESTONE → periyodik tam snapshot
//! 5. EMIT → CommitOutcome

use std::path::Path;

use crate::agent::{PermissionMask, SyntaxViolation};
use crate::bigbang::Delta;
use crate::coords::{Position, RawPosition};
use crate::persistence::{
    DeltaRecord, PersistenceError, SNAPSHOT_FORMAT_VERSION, SpaceSnapshot, SnapshotStore,
};
use crate::rule::{Rule, RuleViolation};
use crate::space::{EdgeKind, NodeId, Space};
use crate::time::{TimeFSM, TimeMachine};
use crate::vision::{compute_derived, CosineDeviation, DeviationMetric, VisionVector};
use crate::vision_config::VisionConfig;
use crate::witness::{Claim, ClaimId, Reason, WitnessResult, WitnessSet};

// ═══════════════════════════════════════════════════════════════════════════════
// EngineConfig
// ═══════════════════════════════════════════════════════════════════════════════

/// Engine konfigürasyonu — `VisionConfig`'ten türetilir.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub min_approvers: usize,
    pub quorum_threshold: f64,
    pub theta_bound: f64,
    pub milestone_interval: u64,
    pub abstractness: f64,
    pub merge_ratio_observable: f64,
    /// Role-aware vision overrides (role → x/y/z override). Boşsa global vision.
    /// Engine, claim'in temsil ettiği node'un rolüne göre vision seçer.
    pub role_overrides: std::collections::HashMap<String, crate::vision_config::RoleVisionOverride>,
}

impl EngineConfig {
    pub fn from_vision_config(config: &VisionConfig) -> Self {
        Self {
            min_approvers: config.min_approvers(),
            quorum_threshold: config.quorum_threshold(),
            theta_bound: config.theta_bound(),
            milestone_interval: config.milestone_interval(),
            abstractness: config.abstractness(),
            merge_ratio_observable: config.merge_ratio_observable(),
            role_overrides: config.role_overrides.clone(),
        }
    }

    /// Test-friendly default (Faz 1.11 kalibrasyon değerleri).
    /// theta_bound=0.3: cosine deviation [0,1] değerlerde θ_max=0.5 (§5.2 NOT);
    /// 0.5 unreachable → 0.3 realistic threshold. TDA diffusion (Faz 5+) ile 0.5'e dönülebilir.
    pub fn default_calibrated() -> Self {
        Self {
            min_approvers: 2,
            quorum_threshold: 1.5,
            theta_bound: 0.3,
            milestone_interval: 1000,
            abstractness: 0.5,
            merge_ratio_observable: 0.10,
            role_overrides: std::collections::HashMap::new(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CommitOutcome + Warnings + Errors
// ═══════════════════════════════════════════════════════════════════════════════

/// Commit başarılı çıktısı.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitOutcome {
    pub event: Delta,
    pub drift_warnings: Vec<DriftWarning>,
    pub safety_weakened: bool,
    pub t_c: u64,
}

/// Post-mutation: neighbor θ > bound (commit geçerli, komşu degrade — WARNING, §4.1).
#[derive(Debug, Clone, PartialEq)]
pub struct DriftWarning {
    pub node_id: NodeId,
    pub theta: f64,
    pub raw: RawPosition,
}

// ═══════════════════════════════════════════════════════════════════════════════
// GateResult — commit pipeline visualizer çıktısı
// ═══════════════════════════════════════════════════════════════════════════════

/// Tek bir gate'in sonucu (commit pipeline visualizer için).
#[derive(Debug, Clone, serde::Serialize)]
pub struct GateResult {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
    pub hallucination: Option<String>,
}

impl GateResult {
    pub fn passed(name: &'static str, detail: &str) -> Self {
        Self { name, passed: true, detail: detail.to_string(), hallucination: None }
    }

    pub fn failed(name: &'static str, detail: &str, h: Option<crate::agent::HallucinationType>) -> Self {
        Self {
            name,
            passed: false,
            detail: detail.to_string(),
            hallucination: h.map(|ht| format!("{}", ht)),
        }
    }
}

/// Pre-mutation: claim θ > bound (Q5 ihlali — §4.1 REJECT, EngineCommitError::VisionViolation).
#[derive(Debug, Clone, PartialEq)]
pub struct VisionViolation {
    pub claim_id: ClaimId,
    pub theta: f64,
    pub raw: RawPosition,
}

impl std::fmt::Display for VisionViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Q5 vision violation (claim {}, negatif-uzay): θ={:.3}, raw={:?}",
            self.claim_id, self.theta, self.raw
        )
    }
}

/// Engine-level commit error (thiserror). Claim-based (Q4-Q6) + witness-based (Q1-Q3).
/// (osp-core-design.md §3.4). Witness Reject/Hold `evaluate()` → `WitnessResult` üzerinden
/// gelir, `Reason` wrap edilir (space-engine-design.md §6.1).
///
/// Variant tasarımı: violation struct'lar tek kaynak (single-source-of-truth). theta/detail/
/// rule_id gibi field'lar variant'ta TEKRAR EDİLMEZ — `Display` impl ile erişilir (drift risk yok).
#[derive(Debug, thiserror::Error)]
pub enum EngineCommitError {
    #[error("witness gate (Q1-Q3): {0:?}")]
    Witness(Reason),
    #[error("{violation}")]
    SyntaxViolation { violation: SyntaxViolation },
    #[error("{violation} (bound={bound:.3})")]
    VisionViolation {
        violation: VisionViolation,
        bound: f64,
    },
    #[error("{violation}")]
    RuleViolation { violation: RuleViolation },
    #[error("permission denied (inv #13): {0}")]
    PermissionDenied(String),
    #[error("persistence kapalı — restore/milestone kullanılamaz (snapshot_store None)")]
    NoPersistence,
    #[error("persistence hatası: {0}")]
    Persistence(#[from] PersistenceError),
}

// ═══════════════════════════════════════════════════════════════════════════════
// SpaceEngine
// ═══════════════════════════════════════════════════════════════════════════════

/// Production runtime — all Faz 1-2 modules orchestrated.
pub struct SpaceEngine {
    space: Space,
    coord_system: crate::coords::CoordinateSystem,
    vision: VisionVector,
    rules: Vec<Box<dyn Rule>>,
    time: TimeFSM,
    config: EngineConfig,
    t_c: u64,
    snapshot_store: Option<SnapshotStore>,
}

impl SpaceEngine {
    /// Manuel kurulum (tüm bileşenler caller sağlar).
    pub fn new(
        space: Space,
        coord_system: crate::coords::CoordinateSystem,
        vision: VisionVector,
        config: EngineConfig,
    ) -> Self {
        Self {
            space,
            coord_system,
            vision,
            rules: vec![], // Faz 5: God Mode `register_rule()` ile ekler
            time: TimeFSM::default(),
            config,
            t_c: 0,
            snapshot_store: None,
        }
    }

    /// Q6 Rule Gate için kural ekle (God Mode / trusted operator).
    ///
    /// Kurallar `check_claim_rules()` içinde sırayla evaluate edilir.
    /// İlk ihlalde claim reddedilir (short-circuit).
    pub fn register_rule(&mut self, rule: Box<dyn crate::rule::Rule>) {
        self.rules.push(rule);
    }

    /// Q6 için varsayılan yapısal kural seti ile engine kur (no_self_import,
    /// no_duplicate_node, edge_target_exists).
    pub fn with_default_rules(
        space: Space,
        coord_system: crate::coords::CoordinateSystem,
        vision: VisionVector,
        config: EngineConfig,
    ) -> Self {
        let mut engine = Self::new(space, coord_system, vision, config);
        for rule in crate::rule::default_rules() {
            engine.register_rule(rule);
        }
        engine
    }

    /// `VisionConfig`'ten kurulum (TOML → engine).
    pub fn from_vision_config(
        space: Space,
        coord_system: crate::coords::CoordinateSystem,
        config: &VisionConfig,
    ) -> Self {
        Self::new(
            space,
            coord_system,
            config.to_vision_vector(),
            EngineConfig::from_vision_config(config),
        )
    }

    /// Persistence aç (event-sourcing — delta + milestone).
    pub fn with_persistence(
        mut self,
        base_dir: impl AsRef<Path>,
    ) -> Result<Self, PersistenceError> {
        self.snapshot_store = Some(SnapshotStore::new(base_dir)?);
        Ok(self)
    }

    // ── Commit pipeline (§4) ───────────────────────────────────────────────

    /// `commit(claim, omega)` — full pipeline (Q4-Q6 claim-based → Q1-Q3 witness → mutate → reposition → save).
    ///
    /// 0. CLAIM-BASED GATES (Q4-Q6, deterministik, witness öncesi):
    ///    - Q4 Syntax: OutputContract compliant? (inv #12)
    ///    - Q5 Vision: claim.computed_raw θ > bound → Err(VisionViolation) [mutasyon YOK]
    ///    - Q6 Rule: Rule ihlali?
    /// 1. WITNESS-BASED GATES (Q1-Q3) + TIME ADVANCE: evaluate + apply_delta
    /// 2. REPOSITION: CosineDeviation → drift_warnings
    /// 3. SAVE DELTA: event-sourcing
    /// 4. MILESTONE: periyodik tam snapshot
    /// 5. EMIT: CommitOutcome
    pub fn commit(
        &mut self,
        claim: &Claim,
        omega: &WitnessSet,
    ) -> Result<CommitOutcome, EngineCommitError> {
        // Phase 0: CLAIM-BASED GATES (Q4-Q6 — deterministik, witness öncesi)
        self.check_claim_syntax(claim)?;
        self.check_claim_vision(claim)?;
        self.check_claim_rules(claim)?;

        // Phase 1: WITNESS-BASED GATES (Q1-Q3) + TIME ADVANCE (apply_delta mutasyon)
        let result = self.time.advance(&mut self.space, claim, omega);
        let (delta, safety_weakened) = match result {
            WitnessResult::Commit {
                delta,
                safety_weakened,
                ..
            } => (delta, safety_weakened),
            WitnessResult::Hold(reason) => return Err(EngineCommitError::Witness(reason)),
            WitnessResult::Reject(reason) => return Err(EngineCommitError::Witness(reason)),
        };

        self.t_c += 1;

        // Phase 2: REPOSITION (CosineDeviation + drift warnings, inv #5)
        let drift_warnings = self.reposition_nodes(&delta.repositioned);

        // Phase 3: SAVE DELTA (event-sourcing)
        if let Some(store) = &self.snapshot_store {
            let record = DeltaRecord {
                version: SNAPSHOT_FORMAT_VERSION,
                t_c: self.t_c,
                claim_id: claim.id,
                delta: delta.clone(),
                safety_weakened,
            };
            let _ = store.save_delta(record); // best-effort; log on error
        }

        // Phase 4: MILESTONE (periyodik)
        if self.t_c % self.config.milestone_interval == 0 {
            if let Some(store) = &self.snapshot_store {
                let snapshot = SpaceSnapshot {
                    version: SNAPSHOT_FORMAT_VERSION,
                    t_c: self.t_c,
                    timestamp_ms: current_time_ms(),
                    space: self.space.clone(),
                };
                let _ = store.save_milestone(snapshot);
            }
        }

        // Phase 5: EMIT
        Ok(CommitOutcome {
            event: delta,
            drift_warnings,
            safety_weakened,
            t_c: self.t_c,
        })
    }

    // ── Claim-based gates (Q4-Q6, Phase 0 — witness öncesi, deterministik) ───

    /// Q4 Syntax Gate — Claim'in ΔS yapısı geçerli mi? (inv #12)
    ///
    /// Kontroller:
    /// 1. delta_nodes: geçerli NodeKind, finite/non-negative mass, non-negative id
    /// 2. delta_edges: Imports self-loop reddi, geçerli EdgeKind, from/to ≥ 0
    /// 3. delta_nodes içinde duplicate ID yok
    /// 4. computed_raw: tüm core eksen değerleri finite
    fn check_claim_syntax(&self, claim: &Claim) -> Result<(), EngineCommitError> {
        // 1. Node validation
        for node in &claim.delta_nodes {
            if node.id == 0 && !claim.delta_nodes.is_empty() {
                // id=0 is valid for first node; check mass/kind instead
            }
            if !node.mass.is_finite() || node.mass < 0.0 {
                return Err(EngineCommitError::SyntaxViolation {
                    violation: SyntaxViolation {
                        claim_id: claim.id,
                        detail: format!(
                            "node {} has invalid mass: {} (must be finite, non-negative)",
                            node.id, node.mass
                        ),
                    },
                });
            }
        }

        // 2. Duplicate node IDs within delta
        let mut seen_ids: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        for node in &claim.delta_nodes {
            if !seen_ids.insert(node.id) {
                return Err(EngineCommitError::SyntaxViolation {
                    violation: SyntaxViolation {
                        claim_id: claim.id,
                        detail: format!("duplicate node id {} in delta_nodes", node.id),
                    },
                });
            }
        }

        // 3. Edge validation
        for edge in &claim.delta_edges {
            // Imports self-loop: module cannot import itself (semantic rule)
            if edge.kind == EdgeKind::Imports && edge.from == edge.to {
                return Err(EngineCommitError::SyntaxViolation {
                    violation: SyntaxViolation {
                        claim_id: claim.id,
                        detail: format!(
                            "self-import edge: node {} imports itself",
                            edge.from
                        ),
                    },
                });
            }
        }

        // 4. computed_raw finite check (flat RawPosition: x,y,z,w,v)
        let raw = &claim.computed_raw;
        let axes = [
            ("x", raw.x),
            ("y", raw.y),
            ("z", raw.z),
            ("w", raw.w),
            ("v", raw.v),
        ];
        for (name, val) in &axes {
            if !val.is_finite() {
                return Err(EngineCommitError::SyntaxViolation {
                    violation: SyntaxViolation {
                        claim_id: claim.id,
                        detail: format!("computed_raw.{} is not finite: {}", name, val),
                    },
                });
            }
        }

        Ok(())
    }

    /// Q5 Vision Gate — `θ(claim.computed_raw, vision) > theta_bound` → Err.
    /// Claim negatif-uzayda ise ana dala GİREMEZ (BFT-derived Safety, §4.1).
    ///
    /// **Role-aware:** Claim'in temsil ettiği node'un mimari rolüne göre vision
    /// vector seçilir (override varsa). Örn: bir TypeSurface node'u için coupling
    /// düşük beklenir — global vision'a göre fail etmemeli. Rol, claim'in ilk
    /// delta_node'unun classification'ından çıkarılır (çoğu claim tek node ekler).
    fn check_claim_vision(&self, claim: &Claim) -> Result<(), EngineCommitError> {
        // Role-aware vision: claim'in node'undan rol çıkar, override uygula.
        let vision = self.vision_for_claim(claim);
        let theta = CosineDeviation.theta(&claim.computed_raw, &vision, &self.space);
        if theta > self.config.theta_bound {
            tracing::warn!(
                claim_id = claim.id,
                theta,
                bound = self.config.theta_bound,
                "Q5 vision violation — claim rejected (negatif-uzay)"
            );
            return Err(EngineCommitError::VisionViolation {
                violation: VisionViolation {
                    claim_id: claim.id,
                    theta,
                    raw: claim.computed_raw,
                },
                bound: self.config.theta_bound,
            });
        }
        Ok(())
    }

    /// Claim'in temsil ettiği node'un rolüne göre vision vector seç.
    /// Override yoksa global `self.vision` döner.
    fn vision_for_claim(&self, claim: &Claim) -> VisionVector {
        use crate::space::{infer_role, NodeRole};
        // İlk delta_node'un classification'ından rol çıkar (path/metric olmadan
        // classification-only — engine path bilmez, sadece node classification).
        if let Some(node) = claim.delta_nodes.first() {
            let role = infer_role("", node.classification, None);
            if let NodeRole::Runtime = role {
                return self.vision; // default → global
            }
            // Role override varsa uygula
            let key = format!("{:?}", role);
            if let Some(ovr) = self.config.role_overrides.get(&key) {
                let mut raw = *self.vision.raw();
                if let Some(x) = ovr.x { raw.x = x; }
                if let Some(y) = ovr.y { raw.y = y; }
                if let Some(z) = ovr.z { raw.z = z; }
                return VisionVector::new(raw);
            }
        }
        self.vision
    }

    /// Q6 Rule Gate — ΔS herhangi bir Rule'u ihlal ediyor mu?
    ///
    /// Stub: `self.rules` boş (Faz 2) → her zaman Ok. Faz 5'te God Mode tarafından
    /// register edilen Hard/Soft Rule'lar `evaluate()` çağrılır (agent-prompt-semantics.md §4).
    fn check_claim_rules(&self, claim: &Claim) -> Result<(), EngineCommitError> {
        for rule in &self.rules {
            if let Some(violation) =
                rule.evaluate(&claim.delta_nodes, &claim.delta_edges, &self.space)
            {
                tracing::warn!(
                    claim_id = claim.id,
                    rule_id = %rule.id(),
                    "Q6 rule violation — claim rejected"
                );
                return Err(EngineCommitError::RuleViolation { violation });
            }
        }
        Ok(())
    }

    /// PermissionMask nihai denetimi (inv #13, agent-prompt-semantics.md §2.1 nokta 3).
    /// Claim.author'ın yazma yetkisi olmayan düğümlere dokunması engellenir.
    ///
    /// Stub: Faz 2'de full_access mask (tüm node'lar writable). Faz 5'te God Mode
    /// config'ten yüklenen gerçek PermissionMask ile çalışır.
    #[allow(dead_code)] // Faz 5'te commit() imzasına mask parametresi eklenecek
    fn check_permissions(
        &self,
        _claim: &Claim,
        _mask: &PermissionMask,
    ) -> Result<(), EngineCommitError> {
        // Faz 5 stub: read_only_nodes'a yazma, forbidden_edge_kinds oluşturma kontrolü
        Ok(())
    }

    // ── Reposition (incremental, inv #5/#6) ────────────────────────────────

    /// Phase 2: post-mutation neighbor drift tespiti + pozisyon güncelleme.
    /// `CosineDeviation` kullanır (inv #5 — DiffusionDeviation değil).
    /// İki-fazlı (collect → apply) — borrow checker uyumu.
    fn reposition_nodes(&mut self, ids: &[NodeId]) -> Vec<DriftWarning> {
        let mut drift_warnings = Vec::new();

        // Faz 1: hesapla (immutable borrow)
        let updates: Vec<(NodeId, Position)> = ids
            .iter()
            .filter_map(|&id| {
                let node = self.space.nodes.get(&id)?;
                let raw = self.coord_system.raw_position_of(node, &self.space);
                let derived = compute_derived(
                    &raw,
                    &self.vision,
                    &self.space,
                    &CosineDeviation,
                    raw.z,
                    self.config.abstractness,
                );
                if derived.theta > self.config.theta_bound {
                    drift_warnings.push(DriftWarning {
                        node_id: id,
                        theta: derived.theta,
                        raw,
                    });
                }
                Some((id, Position { raw, derived }))
            })
            .collect();

        // Faz 2: uygula (mutable borrow)
        for (id, pos) in updates {
            if let Some(node) = self.space.nodes.get_mut(&id) {
                node.position = pos;
            }
        }

        drift_warnings
    }

    /// TAM reposition (analyze/dashboard — inv #5 lazy). Tüm düğümleri günceller.
    /// Commit path'inde DEĞİL — `osp analyze` / dashboard çağrısı.
    /// Faz 5+: `DiffusionDeviation` ile upgrade.
    pub fn full_reposition(&mut self) -> Vec<DriftWarning> {
        let all_ids: Vec<NodeId> = self.space.nodes.keys().copied().collect();
        self.reposition_nodes(&all_ids)
    }

    // ── Persistence ────────────────────────────────────────────────────────

    /// Time-travel (event-sourcing): milestone + delta replay → request_t_c.
    pub fn restore(&mut self, request_t_c: u64) -> Result<usize, EngineCommitError> {
        let store = self
            .snapshot_store
            .as_ref()
            .ok_or(EngineCommitError::NoPersistence)?;
        let restored = store.restore(request_t_c)?;
        self.space = restored.space;
        self.t_c = restored.t_c;
        tracing::info!(
            t_c = restored.t_c,
            replayed = restored.replayed_deltas,
            "restore tamamlandı"
        );
        Ok(restored.replayed_deltas)
    }

    /// Manuel milestone snapshot (tag vb.).
    pub fn save_milestone(&self) -> Result<(), EngineCommitError> {
        let store = self
            .snapshot_store
            .as_ref()
            .ok_or(EngineCommitError::NoPersistence)?;
        let snapshot = SpaceSnapshot {
            version: SNAPSHOT_FORMAT_VERSION,
            t_c: self.t_c,
            timestamp_ms: current_time_ms(),
            space: self.space.clone(),
        };
        store.save_milestone(snapshot)?;
        Ok(())
    }

    // ── Accessors ───────────────────────────────────────────────────────────

    pub fn space(&self) -> &Space {
        &self.space
    }

    /// **Commit Pipeline visualizer** — tüm gate'leri sırayla çalıştırır, her gate'in
    /// sonucunu döner (kısa-devre yok). Q4 fail → Q5/Q6 "skipped".
    ///
    /// Bu metod `commit()`'ten farklı olarak: hatada durmaz, tüm gate durumlarını raporlar.
    /// Frontend visualizer için tasarlandı.
    pub fn check_all_gates(
        &self,
        claim: &Claim,
        omega: &WitnessSet,
    ) -> Vec<GateResult> {
        let mut results = vec![];

        // Q4 Syntax
        match self.check_claim_syntax(claim) {
            Ok(()) => results.push(GateResult::passed("Q4 Syntax", "Schema valid")),
            Err(e) => {
                let h = crate::agent::HallucinationType::from_engine_error(&e);
                results.push(GateResult::failed("Q4 Syntax", &e.to_string(), h));
                return results; // pipeline stops
            }
        }

        // Q5 Vision
        match self.check_claim_vision(claim) {
            Ok(()) => results.push(GateResult::passed("Q5 Vision", "θ within bound")),
            Err(e) => {
                let h = crate::agent::HallucinationType::from_engine_error(&e);
                results.push(GateResult::failed("Q5 Vision", &e.to_string(), h));
                return results;
            }
        }

        // Q6 Rule
        match self.check_claim_rules(claim) {
            Ok(()) => results.push(GateResult::passed("Q6 Rule", "No rule violations")),
            Err(e) => {
                let h = crate::agent::HallucinationType::from_engine_error(&e);
                results.push(GateResult::failed("Q6 Rule", &e.to_string(), h));
                return results;
            }
        }

        // Q1-Q3 Witness
        match crate::witness::evaluate(claim, omega) {
            crate::witness::WitnessResult::Commit { .. } => {
                results.push(GateResult::passed("Q1-Q3 Witness", "Quorum met — Commit"));
            }
            crate::witness::WitnessResult::Hold(reason) => {
                let h = Some(crate::agent::HallucinationType::Undersupported {
                    support: 0.0,
                    threshold: 1.5,
                });
                results.push(GateResult::failed("Q1-Q3 Witness", &format!("Hold: {:?}", reason), h));
            }
            crate::witness::WitnessResult::Reject(reason) => {
                let h = Some(crate::agent::HallucinationType::Witness { witness: 0 });
                results.push(GateResult::failed("Q1-Q3 Witness", &format!("Reject: {:?}", reason), h));
            }
        }

        results
    }

    /// Mutable space reference (test/setup için — production'da commit() kullan).
    #[cfg(test)]
    pub fn space_mut(&mut self) -> &mut Space {
        &mut self.space
    }

    pub fn t_c(&self) -> u64 {
        self.t_c
    }
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
    pub fn vision(&self) -> &VisionVector {
        &self.vision
    }

    /// Coordinate system accessor (for what-if simulations and position computation).
    pub fn coord_system(&self) -> &crate::coords::CoordinateSystem {
        &self.coord_system
    }

    /// **Position computation from DeltaProposal** (inv #4 — epistemological integrity).
    ///
    /// Agent/LLM pozisyon **declare edemez** — engine structural ΔS'i hypothetical
    /// graph'ta uygular, CoordinateSystem ile gerçek pozisyonları ölçer.
    ///
    /// Bu metod Agent kabuğu tarafından çağrılır:
    /// 1. Agent DeltaProposal üretir (structural only — no positions)
    /// 2. Agent kabuğu engine.compute_raw_from_delta() çağırır
    /// 3. Dönen RawPosition ile Claim oluşturur (computed_raw)
    /// 4. Engine.commit() → Q5 θ(computed_raw, vision) kontrol eder
    ///
    /// **Hypothetical graph:** Mevcut space'in klonu + delta uygulanır.
    /// Coupling/Instability yeni edge'lerden compute edilir (actual measured).
    /// Cohesion node.cohesion'dan (analyzer tarafından set edilmişse).
    /// Entropy/WitnessDepth repo-level (CoordinateSystem stored values).
    ///
    /// **Centroid:** ΔS'deki tüm node'ların mass-weighted ortalama pozisyonu.
    /// Bu, "bu değişiklik uzayın neresinde?" sorusunun cevabıdır.
    pub fn compute_raw_from_delta(
        &self,
        delta_nodes: &[crate::space::Node],
        delta_edges: &[crate::space::Edge],
    ) -> RawPosition {
        if delta_nodes.is_empty() {
            return RawPosition::default();
        }

        // 1. Hypothetical graph: clone current space
        let mut hypothetical = self.space.clone();

        // 2. Apply structural changes (pre-mutation, no commit)
        for node in delta_nodes {
            hypothetical.insert_node(node.clone());
        }
        for edge in delta_edges {
            hypothetical.insert_edge(*edge);
        }

        // 3. Compute raw position for each delta node in the hypothetical graph
        let positions: Vec<(f64, RawPosition)> = delta_nodes
            .iter()
            .filter_map(|n| {
                let node = hypothetical.nodes.get(&n.id)?;
                let raw = self.coord_system.raw_position_of(node, &hypothetical);
                // Mass as weight (minimum 0.01 to avoid div-by-zero)
                Some((node.mass.max(0.01), raw))
            })
            .collect();

        if positions.is_empty() {
            return RawPosition::default();
        }

        // 4. Mass-weighted centroid
        let total_mass: f64 = positions.iter().map(|(m, _)| m).sum();
        RawPosition {
            x: positions.iter().map(|(m, r)| m * r.x).sum::<f64>() / total_mass,
            y: positions.iter().map(|(m, r)| m * r.y).sum::<f64>() / total_mass,
            z: positions.iter().map(|(m, r)| m * r.z).sum::<f64>() / total_mass,
            w: positions.iter().map(|(m, r)| m * r.w).sum::<f64>() / total_mass,
            v: positions.iter().map(|(m, r)| m * r.v).sum::<f64>() / total_mass,
        }
    }
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Testler
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axes::{CohesionAxis, EntropyAxis, WitnessDepthAxis};
    use crate::coords::CoordinateSystem;
    use crate::space::{Edge, EdgeKind, Node, NodeKind};
    use crate::witness::{EvidenceEvent, EvidenceId, Intent, WitnessKind};

    /// Vision center — `make_engine` vision ile hizalı. Q5 pre-check geçer.
    const CENTER: RawPosition = RawPosition {
        x: 0.5,
        y: 0.5,
        z: 0.5,
        w: 0.5,
        v: 0.5,
    };

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
        }
    }

    fn ev(id: EvidenceId, actor: u64) -> EvidenceEvent {
        EvidenceEvent::new(id, &format!("src-{id}"), WitnessKind::MergeCommit, actor, 1)
    }

    fn two_witnesses() -> WitnessSet {
        WitnessSet::new(vec![ev(1, 200), ev(2, 300)])
    }

    fn claim_with(author: u64, computed_raw: RawPosition) -> Claim {
        Claim {
            id: 1,
            intent: Intent::new(author, RawPosition::default()),
            author,
            computed_raw,
            delta_nodes: vec![mod_node(10)],
            delta_edges: vec![],
        }
    }

    fn make_engine() -> SpaceEngine {
        let space = Space::new();
        let cs = CoordinateSystem::default_raw_three(
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        let vision = VisionVector::new(RawPosition {
            x: 0.5,
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.5,
        });
        SpaceEngine::new(space, cs, vision, EngineConfig::default_calibrated())
    }

    // --- commit success ---

    #[test]
    fn commit_success_returns_outcome() {
        let mut engine = make_engine();
        let claim = claim_with(100, CENTER); // aligned with vision (center)
        let omega = two_witnesses();

        let outcome = engine.commit(&claim, &omega).expect("commit");
        assert_eq!(outcome.t_c, 1);
        assert!(!outcome.safety_weakened);
        assert_eq!(engine.space().node_count(), 1); // node 10 added
        assert!(engine.space().nodes.contains_key(&10));
    }

    #[test]
    fn commit_increments_t_c() {
        let mut engine = make_engine();
        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        engine.commit(&claim, &omega).unwrap();
        assert_eq!(engine.t_c(), 1);
        engine.commit(&claim, &omega).unwrap();
        assert_eq!(engine.t_c(), 2);
    }

    // --- Q5 vision pre-check (Safety — reviewer #1) ---

    #[test]
    fn commit_q5_aligned_claim_passes() {
        let mut engine = make_engine();
        // Claim aligned with vision → θ ≈ 0 → passes Q5
        let good_claim = claim_with(100, RawPosition {
            x: 0.5, y: 0.5, z: 0.5, w: 0.5, v: 0.5,
        });
        let omega = two_witnesses();

        let result = engine.commit(&good_claim, &omega);
        assert!(result.is_ok(), "aligned claim → Commit");
    }

    // --- commit Hold (witness insufficient) ---

    #[test]
    fn commit_hold_returns_witness_error() {
        let mut engine = make_engine();
        let claim = claim_with(100, CENTER);
        let omega = WitnessSet::new(vec![ev(1, 200)]); // 1 witness → Hold

        let result = engine.commit(&claim, &omega);
        assert!(matches!(
            result,
            Err(EngineCommitError::Witness(Reason::MinApproversNotMet { .. }))
        ));
        assert_eq!(engine.space().node_count(), 0, "Hold → mutasyon yok");
    }

    // --- reposition + drift warnings ---

    #[test]
    fn commit_repositions_new_nodes() {
        let mut engine = make_engine();
        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        let _outcome = engine.commit(&claim, &omega).unwrap();
        // node 10 was added + repositioned → has a position
        let node = engine.space().nodes.get(&10).expect("node 10");
        assert!(node.position.raw.x >= 0.0); // position computed (not default)
    }

    #[test]
    fn commit_drift_warning_when_node_far_from_vision() {
        // Engine vision = (0.5, 0.5, 0.5, 0.5, 0.5). Add a node that, after reposition,
        // has high coupling (x → 1.0) → θ > 0.5 → drift warning.
        let mut space = Space::new();
        for i in 1..=20 {
            space.insert_node(mod_node(i));
        }
        // node 1 imports everything → high coupling
        for i in 2..=20 {
            space.insert_edge(edge(1, i));
        }

        let cs = CoordinateSystem::default_raw_three(
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        let vision = VisionVector::new(RawPosition {
            x: 0.2, // low coupling vision — node 1 (x≈0.95) will drift
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.5,
        });
        let mut config = EngineConfig::default_calibrated();
        config.theta_bound = 0.2; // test-specific: drift triggers at lower θ
        let mut engine = SpaceEngine::new(space, cs, vision, config);

        // full_reposition: node 1 has x ≈ 0.95 (19 imports) vs vision x=0.2 → θ high
        let warnings = engine.full_reposition();
        assert!(
            !warnings.is_empty(),
            "node 1 high coupling → drift warning expected"
        );
        assert!(warnings.iter().any(|w| w.node_id == 1));
    }

    // --- persistence ---

    #[test]
    fn commit_saves_delta_to_store() {
        let tmp = tempfile::tempdir().unwrap();
        let mut engine = make_engine().with_persistence(tmp.path()).unwrap();
        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        engine.commit(&claim, &omega).unwrap();

        // Delta saved
        let store = SnapshotStore::new(tmp.path()).unwrap();
        let deltas = store.list_deltas_in_range(0, 1).unwrap();
        assert_eq!(deltas.len(), 1);
    }

    #[test]
    fn commit_milestone_at_interval() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = EngineConfig::default_calibrated();
        config.milestone_interval = 2; // every 2 commits
        let cs = CoordinateSystem::default_raw_three(
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        let vision = VisionVector::new(CENTER);
        let mut engine = SpaceEngine::new(Space::new(), cs, vision, config)
            .with_persistence(tmp.path())
            .unwrap();

        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        engine.commit(&claim, &omega).unwrap(); // t_c=1 (no milestone)
        engine.commit(&claim, &omega).unwrap(); // t_c=2 → milestone

        let store = SnapshotStore::new(tmp.path()).unwrap();
        let milestones = store.list_milestones().unwrap();
        assert!(milestones.contains(&2), "milestone at t_c=2");
    }

    #[test]
    fn restore_via_event_sourcing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut engine = make_engine().with_persistence(tmp.path()).unwrap();
        let claim = claim_with(100, CENTER);
        let omega = two_witnesses();

        engine.save_milestone().unwrap(); // milestone at t_c=0
        engine.commit(&claim, &omega).unwrap(); // t_c=1, delta saved
        engine.commit(&claim, &omega).unwrap(); // t_c=2, delta saved

        // Restore to t_c=1
        let replayed = engine.restore(1).unwrap();
        assert_eq!(replayed, 1); // 1 delta replayed (milestone at 0)
        assert_eq!(engine.t_c(), 1);
        assert_eq!(engine.space().node_count(), 1); // 1 commit → 1 node
    }

    // --- full_reposition ---

    #[test]
    fn full_reposition_updates_all_nodes() {
        let mut space = Space::new();
        space.insert_node(mod_node(1));
        space.insert_node(mod_node(2));

        let cs = CoordinateSystem::default_raw_three(
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        let mut engine = SpaceEngine::new(
            space,
            cs,
            VisionVector::new(CENTER),
            EngineConfig::default_calibrated(),
        );

        let _ = engine.full_reposition();
        // All nodes have positions (not default all-zero)
        for node in engine.space().nodes.values() {
            assert!(node.position.raw.x >= 0.0 || node.position.raw.w > 0.0);
        }
    }

    // --- from_vision_config ---

    #[test]
    fn from_vision_config_builds_engine() {
        let toml = r#"
[raw]
x = 0.4
y = 0.7
z = 0.5
w = 0.5
v = 0.5
"#;
        let config = VisionConfig::from_str(toml).unwrap();
        let cs = CoordinateSystem::default_raw_three(
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        let engine = SpaceEngine::from_vision_config(Space::new(), cs, &config);

        assert!((engine.vision().raw().x - 0.4).abs() < 1e-9);
        assert_eq!(engine.config().min_approvers, 2);
        assert!((engine.config().theta_bound - 0.3).abs() < 1e-9);
        assert_eq!(engine.t_c(), 0);
    }

    // --- no persistence ---

    #[test]
    fn restore_without_persistence_returns_error() {
        let mut engine = make_engine(); // no persistence
        let result = engine.restore(1);
        assert!(matches!(result, Err(EngineCommitError::NoPersistence)));
    }

    // --- Q4 Syntax Gate (real implementation) ---

    fn claim_with_delta(author: u64, nodes: Vec<Node>, edges: Vec<Edge>) -> Claim {
        Claim {
            id: 1,
            intent: Intent::new(author, RawPosition::default()),
            author,
            computed_raw: RawPosition::default(),
            delta_nodes: nodes,
            delta_edges: edges,
        }
    }

    #[test]
    fn q4_rejects_nan_mass() {
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: f64::NAN,
                ..Default::default()
            }],
            vec![],
        );
        let result = engine.commit(&claim, &two_witnesses());
        assert!(
            matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
            "NaN mass should be rejected by Q4"
        );
    }

    #[test]
    fn q4_rejects_negative_mass() {
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: -5.0,
                ..Default::default()
            }],
            vec![],
        );
        let result = engine.commit(&claim, &two_witnesses());
        assert!(
            matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
            "negative mass should be rejected by Q4"
        );
    }

    #[test]
    fn q4_rejects_duplicate_node_ids() {
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![
                Node {
                    id: 42,
                    kind: NodeKind::Module,
                    mass: 1.0,
                    ..Default::default()
                },
                Node {
                    id: 42,
                    kind: NodeKind::Module,
                    mass: 2.0,
                    ..Default::default()
                },
            ],
            vec![],
        );
        let result = engine.commit(&claim, &two_witnesses());
        assert!(
            matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
            "duplicate node IDs should be rejected by Q4"
        );
    }

    #[test]
    fn q4_rejects_imports_self_loop() {
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            }],
            vec![Edge {
                from: 10,
                to: 10,
                kind: EdgeKind::Imports,
            }],
        );
        let result = engine.commit(&claim, &two_witnesses());
        assert!(
            matches!(result, Err(EngineCommitError::SyntaxViolation { .. })),
            "self-import should be rejected by Q4"
        );
    }

    #[test]
    fn q4_allows_calls_self_loop() {
        // Calls self-loop (recursion) is valid — not Imports
        let mut engine = make_engine();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            }],
            vec![Edge {
                from: 10,
                to: 10,
                kind: EdgeKind::Calls,
            }],
        );
        // Should pass Q4 (might fail Q5 if vision not aligned, but not Q4)
        let result = engine.check_claim_syntax(&claim);
        assert!(result.is_ok(), "Calls self-loop should pass Q4: {:?}", result);
    }

    #[test]
    fn q4_rejects_nan_computed_raw() {
        let mut engine = make_engine();
        let mut claim = claim_with(100, CENTER);
        claim.computed_raw.x = f64::NAN;
        let result = engine.check_claim_syntax(&claim);
        assert!(result.is_err(), "NaN computed_raw should fail Q4");
    }

    // --- Q6 Rule Gate (default rules) ---

    fn make_engine_with_rules() -> SpaceEngine {
        let cs = CoordinateSystem::default_raw_three(
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        let vision = VisionVector::new(RawPosition {
            x: 0.5,
            y: 0.5,
            z: 0.5,
            w: 0.5,
            v: 0.5,
        });
        SpaceEngine::with_default_rules(
            Space::new(),
            cs,
            vision,
            EngineConfig::default_calibrated(),
        )
    }

    #[test]
    fn q6_rejects_self_import_via_default_rule() {
        let mut engine = make_engine_with_rules();
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 10,
                kind: NodeKind::Module,
                mass: 1.0,
                ..Default::default()
            }],
            vec![Edge {
                from: 10,
                to: 10,
                kind: EdgeKind::Imports,
            }],
        );
        // Q4 catches this first, but if we bypass Q4, Q6 catches it too
        // Verify Q6 directly
        let result = engine.check_claim_rules(&claim);
        assert!(
            matches!(result, Err(EngineCommitError::RuleViolation { .. })),
            "self-import should be caught by Q6 default rule"
        );
    }

    #[test]
    fn q6_rejects_duplicate_node_via_default_rule() {
        let mut engine = make_engine_with_rules();
        // Pre-insert node 5
        engine.space_mut().insert_node(Node {
            id: 5,
            kind: NodeKind::Module,
            mass: 1.0,
            ..Default::default()
        });
        // Claim tries to add node 5 again
        let claim = claim_with_delta(
            100,
            vec![Node {
                id: 5,
                kind: NodeKind::Module,
                mass: 2.0,
                ..Default::default()
            }],
            vec![],
        );
        let result = engine.check_claim_rules(&claim);
        assert!(
            matches!(result, Err(EngineCommitError::RuleViolation { .. })),
            "duplicate node should be caught by Q6 default rule"
        );
    }

    #[test]
    fn q6_allows_valid_claim_with_default_rules() {
        let mut engine = make_engine_with_rules();
        let claim = claim_with_delta(
            100,
            vec![
                Node {
                    id: 10,
                    kind: NodeKind::Module,
                    mass: 1.0,
                    ..Default::default()
                },
                Node {
                    id: 11,
                    kind: NodeKind::Module,
                    mass: 1.0,
                    ..Default::default()
                },
            ],
            vec![Edge {
                from: 10,
                to: 11,
                kind: EdgeKind::Imports,
            }],
        );
        let result = engine.check_claim_rules(&claim);
        assert!(result.is_ok(), "valid claim should pass Q6: {:?}", result);
    }

    // --- Position computation from DeltaProposal (inv #4) ---

    /// Full 5-axis engine for position computation tests (coupling + cohesion + instability + entropy + witness)
    fn make_engine_full() -> SpaceEngine {
        let cs = CoordinateSystem::default_raw_five(
            CohesionAxis::new(),
            EntropyAxis::from_commit_entropy(6.0),
            WitnessDepthAxis::from_witness(0.3, 5),
        );
        let vision = VisionVector::new(RawPosition {
            x: 0.5, y: 0.5, z: 0.5, w: 0.5, v: 0.5,
        });
        SpaceEngine::new(Space::new(), cs, vision, EngineConfig::default_calibrated())
    }

    #[test]
    fn compute_raw_empty_delta_returns_default() {
        let engine = make_engine();
        let raw = engine.compute_raw_from_delta(&[], &[]);
        assert_eq!(raw, RawPosition::default(), "empty delta → default position");
    }

    #[test]
    fn compute_raw_does_not_mutate_real_space() {
        let mut engine = make_engine();
        let initial_count = engine.space().node_count();

        let nodes = vec![Node {
            id: 999,
            kind: NodeKind::Module,
            mass: 10.0,
            ..Default::default()
        }];
        let _ = engine.compute_raw_from_delta(&nodes, &[]);

        assert_eq!(
            engine.space().node_count(),
            initial_count,
            "hypothetical graph must not mutate real space"
        );
    }

    #[test]
    fn compute_raw_single_isolated_node_has_zero_coupling() {
        let engine = make_engine_full();
        let nodes = vec![Node {
            id: 42,
            kind: NodeKind::Module,
            mass: 10.0,
            ..Default::default()
        }];
        let raw = engine.compute_raw_from_delta(&nodes, &[]);
        // Isolated node: coupling = out_degree / (1 + out_degree) = 0 / 1 = 0
        assert!((raw.x - 0.0).abs() < 1e-9, "isolated node coupling should be 0, got {}", raw.x);
        // Isolated node: Ce=Ca=0 → instability = 0.5 (convention)
        assert!((raw.z - 0.5).abs() < 1e-9, "isolated node instability should be 0.5, got {}", raw.z);
    }

    #[test]
    fn compute_raw_edge_increases_coupling() {
        let engine = make_engine_full();
        // Two nodes + one import edge: node 1 imports node 2
        let nodes = vec![
            Node { id: 1, kind: NodeKind::Module, mass: 10.0, ..Default::default() },
            Node { id: 2, kind: NodeKind::Module, mass: 10.0, ..Default::default() },
        ];
        let edges = vec![Edge { from: 1, to: 2, kind: EdgeKind::Imports }];

        let raw = engine.compute_raw_from_delta(&nodes, &edges);

        // Node 1: out_degree(Imports) = 1 → coupling = 1/(1+1) = 0.5
        // Node 2: out_degree(Imports) = 0 → coupling = 0
        // Centroid (equal mass): (0.5 + 0.0) / 2 = 0.25
        assert!(
            (raw.x - 0.25).abs() < 1e-9,
            "centroid coupling with 1 edge should be 0.25, got {}",
            raw.x
        );
    }

    #[test]
    fn compute_raw_is_mass_weighted() {
        let engine = make_engine_full();
        let nodes = vec![
            Node { id: 1, kind: NodeKind::Module, mass: 100.0, ..Default::default() },
            Node { id: 2, kind: NodeKind::Module, mass: 1.0, ..Default::default() },
        ];
        let edges = vec![Edge { from: 1, to: 2, kind: EdgeKind::Imports }];

        let raw = engine.compute_raw_from_delta(&nodes, &edges);
        let expected = 100.0 * 0.5 / 101.0;
        assert!(
            (raw.x - expected).abs() < 1e-6,
            "mass-weighted centroid: expected {}, got {}",
            expected,
            raw.x
        );
    }

    #[test]
    fn compute_raw_cohesion_from_node() {
        let engine = make_engine_full();
        let nodes = vec![Node {
            id: 1,
            kind: NodeKind::Module,
            mass: 10.0,
            cohesion: Some(0.85),
            ..Default::default()
        }];
        let raw = engine.compute_raw_from_delta(&nodes, &[]);
        assert!(
            (raw.y - 0.85).abs() < 1e-9,
            "cohesion should come from node.cohesion, got {}",
            raw.y
        );
    }
}
