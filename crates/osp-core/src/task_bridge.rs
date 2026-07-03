//! Task Bridge — anchoring ↔ trajectory protocol boundary (Faz 5b, INV-T2, INV-P2).
//!
//! # Ana tez
//! *Accepted intent is not executable work. Task genesis requires operator capability.*
//! *Executable task = accepted intent + operator-bound predicate + operator capability.*
//!
//! # Protocol boundary (D17)
//! Bu modül `anchoring/` ve `trajectory/` **arasında** yaşar — ikisini de görür ama
//! birine aidiyet etmez. İki farklı epistemik alanı karıştırmadan bağlar:
//!
//! ```text
//! anchoring:   Candidate, Accepted, AnchorStore, OperatorAcceptance (Candidate→Accepted)
//! trajectory:  Task, PredicateSet, OperatorCapability (Task genesis, INV-T2)
//! task_bridge: Accepted TaskCandidate → trajectory::Task (protocol geçişi)
//! ```
//!
//! # Üç kapılı API (D1/D2)
//! 1. `verify_accepted_task_candidate` — Accepted intent doğrula (OperatorAcceptance ile
//!    promote edilmiş olmalı).
//! 2. `bind_metric_threshold` (predicate_lowering.rs) — OperatorCapability ile executable
//!    predicate üret (INV-P2).
//! 3. `create_task_from_accepted_candidate` — OperatorCapability ile trajectory::Task doğur.
//!
//! Hiçbir kapı atlanamaz veya bypass edilemez.

use crate::anchoring::types::{ConceptGraph, ConceptNodeId};
use crate::anchoring::ExecutablePredicateSet;
use crate::anchoring::{ConceptNodeKind, DecisionStatus};
use crate::trajectory::{OpKind, OperatorCapability, RuleRef, Task, TaskId, TaskStatus};

// ═══════════════════════════════════════════════════════════════════════════════
// AcceptedTaskCandidateRef — non-forgeable verify reference (Kontrol 7)
// ═══════════════════════════════════════════════════════════════════════════════

/// Accepted TaskCandidate'ı kanıtlayan reference — *accepted intent* (Kontrol 7).
///
/// # Non-forgeable (Kontrol 7)
/// Private `id` field → dış crate literal construct edemez. **Tek üretim yolu**
/// `verify_accepted_task_candidate()`. Bu, "accepted intent" durumunun operator
/// (OperatorAcceptance) tarafından grant edildiğini tip seviyesinde kanıtlar.
///
/// *Accepted intent ≠ executable work* — bu reference Task değildir, sadece Task
/// genesis'in önkoşuludur. `create_task_from_accepted_candidate` bu reference'ı
/// `ExecutablePredicateSet` + `OperatorCapability` ile birleştirir.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptedTaskCandidateRef {
    id: ConceptNodeId,
}

impl AcceptedTaskCandidateRef {
    /// Accepted TaskCandidate'nin ConceptNode ID'si (TaskId türetme için).
    pub fn id(&self) -> &ConceptNodeId {
        &self.id
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TaskGenesisError
// ═══════════════════════════════════════════════════════════════════════════════

/// Task genesis hatası (Patch 6: NotAccepted verify'de, create_task ayrışır).
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum TaskGenesisError {
    #[error("node bulunamadı: {node_id}")]
    NodeNotFound { node_id: ConceptNodeId },
    #[error("node TaskCandidate değil: {node_id}")]
    NotTaskCandidate { node_id: ConceptNodeId },
    #[error("TaskCandidate Accepted değil (operator onayı gerek, INV-C3): {node_id}")]
    NotAccepted { node_id: ConceptNodeId },
}

// ═══════════════════════════════════════════════════════════════════════════════
// verify_accepted_task_candidate — 1. kapı (Patch 6: NotAccepted burada)
// ═══════════════════════════════════════════════════════════════════════════════

/// Accepted TaskCandidate'ı doğrula → `AcceptedTaskCandidateRef` (1. kapı).
///
/// # Kontroller (Patch 6 — NotAccepted burada, create_task'da değil)
/// - node mevcut değil → `NodeNotFound`
/// - `node_kind != TaskCandidate` → `NotTaskCandidate`
/// - `decision_status != Accepted` → `NotAccepted` (OperatorAcceptance ile promote
///   edilmemiş — INV-C3)
/// - hepsi geçerli → `AcceptedTaskCandidateRef`
///
/// *Accepted intent ≠ executable work* — bu fonksiyon sadece intent'in kabul edildiğini
/// kanıtlar. Task genesis için `create_task_from_accepted_candidate` (3. kapı) gerek.
pub fn verify_accepted_task_candidate(
    graph: &ConceptGraph,
    id: &ConceptNodeId,
) -> Result<AcceptedTaskCandidateRef, TaskGenesisError> {
    let node = graph
        .node(id)
        .ok_or_else(|| TaskGenesisError::NodeNotFound {
            node_id: id.clone(),
        })?;
    if !matches!(node.node_kind, ConceptNodeKind::TaskCandidate) {
        return Err(TaskGenesisError::NotTaskCandidate {
            node_id: id.clone(),
        });
    }
    if !matches!(node.decision_status, DecisionStatus::Accepted) {
        return Err(TaskGenesisError::NotAccepted {
            node_id: id.clone(),
        });
    }
    Ok(AcceptedTaskCandidateRef { id: id.clone() })
}

// ═══════════════════════════════════════════════════════════════════════════════
// deterministic TaskId (Patch 8)
// ═══════════════════════════════════════════════════════════════════════════════

/// Accepted TaskCandidate ID'sinden deterministic TaskId üret (Patch 8).
///
/// ```text
/// TaskCandidate:AuthServiceRefactor → Task:from-candidate:AuthServiceRefactor
/// ```
/// Slug/sanitize: prefix'ten sonraki canonical parça task id suffix olur. Boşluk/slash/
/// çift-colon deterministik normalize. Atomic counter yerine — test paralelliği +
/// evidence tekrar üretilebilirliği. Candidate→çok task Faz 5.1'de allocator.
fn deterministic_task_id(candidate_id: &ConceptNodeId) -> TaskId {
    // TaskId = u64. Deterministic hash-benzeri (basit) — canonical string'den stable u64.
    // Aday adından (prefix sonrası) FNV-1a-benzeri hash. Aynı aday her zaman aynı ID.
    let canonical = candidate_id
        .0
        .split_once(':')
        .map(|(_, name)| name)
        .unwrap_or(&candidate_id.0);
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for byte in canonical.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    // 0 reserved (TaskId default) — çakışmayı önle.
    if hash == 0 {
        hash = 1;
    }
    hash // TaskId = u64 alias
}

/// Deterministic task label — candidate canonical'ından.
fn deterministic_task_label(candidate_id: &ConceptNodeId) -> String {
    let canonical = candidate_id
        .0
        .split_once(':')
        .map(|(_, name)| name)
        .unwrap_or(&candidate_id.0);
    format!("Task from accepted candidate: {canonical}")
}

// ═══════════════════════════════════════════════════════════════════════════════
// create_task_from_accepted_candidate — 3. kapı (Patch 1: ExecutablePredicateSet alır)
// ═══════════════════════════════════════════════════════════════════════════════

/// Accepted TaskCandidate + ExecutablePredicateSet + OperatorCapability → trajectory::Task
/// (3. kapı, INV-T2).
///
/// # Patch 1 (blocker) — ExecutablePredicateSet alır, raw PredicateSet DEĞİL
/// Boundary bypass edilemez: *"Task genesis raw PredicateSet ile değil, slotları
/// bağlanmış ExecutablePredicateSet ile olur."* ExecutablePredicateSet sadece
/// `bind_metric_threshold` (2. kapı, OperatorCapability-gated) ile üretilir.
///
/// # INV-T2
/// `cap: &OperatorCapability` — compile-time caller-identity token (body'de kullanılmaz,
/// decompose_milestone pattern'i). *"OperatorCapability olmadan trajectory::Task doğmaz."*
///
/// # Deterministic TaskId (Patch 8)
/// Candidate ID'den deterministic — atomic counter değil. Evidence tekrar üretilebilirlik.
///
/// # milestone_id sentinel
/// Task Trajectory/Milestone gerektirmez (keşif doğruladı). milestone_id = 0 sentinel
/// (standalone task). İleride Trajectory bağlantısı Faz 5.1.
pub fn create_task_from_accepted_candidate(
    accepted: AcceptedTaskCandidateRef,
    predicates: ExecutablePredicateSet,
    _cap: &OperatorCapability,
    label: String,
    allowed_operations: Vec<OpKind>,
    constraints: Vec<RuleRef>,
) -> Result<Task, TaskGenesisError> {
    // accepted reference zaten verify ile kanıtlanmış (Patch 6 — NotAccepted verify'de).
    // ExecutablePredicateSet non-empty by construction (Kontrol 2).
    let task = Task {
        id: deterministic_task_id(accepted.id()),
        milestone_id: 0, // sentinel — standalone (Faz 5.1: Trajectory bağlantısı)
        label,
        target_predicate_set: predicates.into_trajectory_predicate_set(),
        policy: Default::default(),
        allowed_operations,
        constraints,
        status: TaskStatus::Pending,
    };
    Ok(task)
}

/// Convenience: create_task with deterministic label (candidate'dan).
pub fn create_task_from_accepted_candidate_default_label(
    accepted: AcceptedTaskCandidateRef,
    predicates: ExecutablePredicateSet,
    cap: &OperatorCapability,
    allowed_operations: Vec<OpKind>,
    constraints: Vec<RuleRef>,
) -> Result<Task, TaskGenesisError> {
    let label = deterministic_task_label(accepted.id());
    create_task_from_accepted_candidate(
        accepted,
        predicates,
        cap,
        label,
        allowed_operations,
        constraints,
    )
}

#[cfg(test)]
mod tests {
    //! task_bridge.rs unit testleri — verify (4 durum), create_task (capability-gated,
    //! deterministic TaskId), AcceptedTaskCandidateRef non-forgeable.

    use super::*;
    use crate::anchoring::types::{ConceptGraph, ConceptNode, ConceptNodeId};

    fn task_candidate(id: &str, status: DecisionStatus) -> ConceptNode {
        let canonical = id.split_once(':').map(|(_, n)| n).unwrap_or(id);
        ConceptNode {
            id: ConceptNodeId(id.into()),
            canonical: canonical.into(),
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::TaskCandidate,
            decision_status: status,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        }
    }

    fn graph_with(nodes: Vec<ConceptNode>) -> ConceptGraph {
        let mut g = ConceptGraph::new();
        for n in nodes {
            g.insert_node(n);
        }
        g
    }

    // ── verify_accepted_task_candidate (Patch 6: NotAccepted burada) ──────────

    #[test]
    fn verify_accepted_task_candidate_ok() {
        let g = graph_with(vec![task_candidate(
            "TaskCandidate:AuthServiceRefactor",
            DecisionStatus::Accepted,
        )]);
        let id = ConceptNodeId("TaskCandidate:AuthServiceRefactor".into());
        let r = verify_accepted_task_candidate(&g, &id).expect("accepted → ref");
        assert_eq!(r.id(), &id);
    }

    #[test]
    fn verify_rejects_node_not_found() {
        let g = ConceptGraph::new();
        let id = ConceptNodeId("TaskCandidate:Missing".into());
        let err = verify_accepted_task_candidate(&g, &id).unwrap_err();
        assert!(matches!(err, TaskGenesisError::NodeNotFound { .. }));
    }

    #[test]
    fn verify_rejects_not_task_candidate() {
        let g = graph_with(vec![ConceptNode {
            id: ConceptNodeId("Concept:Payment".into()),
            canonical: "Payment".into(),
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::Concept,
            decision_status: DecisionStatus::Accepted,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        }]);
        let id = ConceptNodeId("Concept:Payment".into());
        let err = verify_accepted_task_candidate(&g, &id).unwrap_err();
        assert!(matches!(err, TaskGenesisError::NotTaskCandidate { .. }));
    }

    #[test]
    fn verify_rejects_not_accepted() {
        let g = graph_with(vec![task_candidate(
            "TaskCandidate:StillCandidate",
            DecisionStatus::Candidate,
        )]);
        let id = ConceptNodeId("TaskCandidate:StillCandidate".into());
        let err = verify_accepted_task_candidate(&g, &id).unwrap_err();
        assert!(
            matches!(err, TaskGenesisError::NotAccepted { .. }),
            "Candidate (not Accepted) → NotAccepted (INV-C3)"
        );
    }

    // ── deterministic TaskId (Patch 8) ─────────────────────────────────────────

    #[test]
    fn deterministic_task_id_is_stable_and_unique() {
        let id1 = deterministic_task_id(&ConceptNodeId("TaskCandidate:Foo".into()));
        let id2 = deterministic_task_id(&ConceptNodeId("TaskCandidate:Foo".into()));
        let id3 = deterministic_task_id(&ConceptNodeId("TaskCandidate:Bar".into()));
        assert_eq!(id1, id2, "same candidate → same TaskId (deterministic)");
        assert_ne!(id1, id3, "different candidates → different TaskIds");
        assert_ne!(id1, 0u64, "0 reserved");
    }

    // ── create_task_from_accepted_candidate (Patch 1: ExecutablePredicateSet) ─

    fn executable_set() -> ExecutablePredicateSet {
        use crate::anchoring::{
            bind_metric_threshold, MetricThresholdBinding, NormalizedMetricThreshold,
            PhysicalCodeMetricAxis, PredicateStub, PredicateStubReason, PredicateTemplateId,
        };
        use crate::trajectory::{ComparisonOp, PredicateScope};
        let stub = PredicateStub::new_with_axis_hint(
            ConceptNodeId("RuleCandidate:NoHighCoupling".into()),
            PredicateStubReason::MetricUnresolved,
            vec![
                crate::anchoring::PredicateSlot::Metric,
                crate::anchoring::PredicateSlot::Threshold,
                crate::anchoring::PredicateSlot::Scope,
                crate::anchoring::PredicateSlot::Comparator,
            ],
            vec![PredicateTemplateId::MetricThreshold],
            Some(PhysicalCodeMetricAxis::Coupling),
        )
        .unwrap();
        let binding = MetricThresholdBinding::new(
            PhysicalCodeMetricAxis::Coupling,
            PredicateScope::Node(1),
            ComparisonOp::Le,
            NormalizedMetricThreshold::new(0.55).unwrap(),
        );
        let cap = OperatorCapability::issue();
        bind_metric_threshold(&stub, binding, &cap).unwrap()
    }

    #[test]
    fn create_task_from_accepted_candidate_produces_runnable_task() {
        let g = graph_with(vec![task_candidate(
            "TaskCandidate:AuthServiceRefactor",
            DecisionStatus::Accepted,
        )]);
        let id = ConceptNodeId("TaskCandidate:AuthServiceRefactor".into());
        let accepted = verify_accepted_task_candidate(&g, &id).unwrap();
        let predicates = executable_set();
        let cap = OperatorCapability::issue();
        let task = create_task_from_accepted_candidate_default_label(
            accepted,
            predicates,
            &cap,
            vec![crate::trajectory::OpKind::RemoveImport],
            vec![],
        )
        .expect("accepted + executable + capability → Task");

        assert_eq!(task.status, TaskStatus::Pending);
        assert!(!task.target_predicate_set.predicates.is_empty());
        assert_eq!(
            task.allowed_operations,
            vec![crate::trajectory::OpKind::RemoveImport]
        );
        // Deterministic TaskId
        assert_ne!(task.id, 0u64);
    }

    // ── T6: E2E smoke — tam zincir (RuleCandidate → navigator-resolvable Task) ──

    #[test]
    fn e2e_rule_candidate_to_registry_resolvable_task() {
        // Faz 5b'nin ana iddiası: insan niyetinden navigator-resolvable Task'a zincir.
        // RuleCandidate → lower → PredicateStub → bind → ExecutablePredicateSet
        // → verify Accepted TaskCandidate → create_task → registry.insert → resolve.
        use crate::anchoring::{
            bind_metric_threshold, lower_rule_to_predicate_stub, MetricThresholdBinding,
            NormalizedMetricThreshold, PhysicalCodeMetricAxis,
        };
        use crate::trajectory::{ComparisonOp, InMemoryTaskRegistry, PredicateScope, TaskResolver};

        // 1. RuleCandidate node (canonical coupling kuralı → axis hint Coupling).
        let rule = ConceptNode {
            id: ConceptNodeId("RuleCandidate:NoHighCoupling".into()),
            canonical: "NoHighCoupling".into(),
            aliases: Vec::new(),
            node_kind: ConceptNodeKind::RuleCandidate,
            decision_status: DecisionStatus::Candidate,
            position_family: crate::anchoring::PositionFamily::ConceptualIntent,
        };
        // Accepted TaskCandidate (operator promote etmiş varsay).
        let task_cand = task_candidate(
            "TaskCandidate:AuthServiceRefactor",
            DecisionStatus::Accepted,
        );
        let graph = graph_with(vec![rule.clone(), task_cand]);

        // 2. RuleCandidate → PredicateStub (axis hint Coupling).
        let stub = match lower_rule_to_predicate_stub(&rule).unwrap() {
            crate::anchoring::PredicateLoweringOutcome::Stub(s) => s,
        };
        assert_eq!(
            stub.suggested_axis(),
            Some(PhysicalCodeMetricAxis::Coupling)
        );

        // 3. Operator bind: Coupling ≤ 0.55 (OperatorCapability).
        let binding = MetricThresholdBinding::new(
            PhysicalCodeMetricAxis::Coupling,
            PredicateScope::Node(1),
            ComparisonOp::Le,
            NormalizedMetricThreshold::new(0.55).unwrap(),
        );
        let cap = OperatorCapability::issue();
        let eps = bind_metric_threshold(&stub, binding, &cap).unwrap();

        // 4. Verify Accepted TaskCandidate (OperatorAcceptance ile promote edilmiş).
        let accepted = verify_accepted_task_candidate(
            &graph,
            &ConceptNodeId("TaskCandidate:AuthServiceRefactor".into()),
        )
        .unwrap();

        // 5. create_task (OperatorCapability — Task genesis, INV-T2).
        let task = create_task_from_accepted_candidate_default_label(
            accepted,
            eps,
            &cap,
            vec![crate::trajectory::OpKind::RemoveImport],
            vec![],
        )
        .unwrap();

        // 6. registry.insert → resolve (navigator-resolvable).
        let mut registry = InMemoryTaskRegistry::new();
        registry.insert(task.clone());
        assert_eq!(registry.tasks.len(), 1);
        let resolved = registry.resolve(task.id);
        assert!(resolved.is_some(), "Task registry'den resolve edilebilir");
        assert_eq!(resolved.unwrap().id, task.id);
        // INV-T2: Task genesis yalnız capability ile — navigator şimdi çalıştırabilir.
        // (run_task mock LLM ayrı smoke/PR33c — burada registry-resolvable kanıtı yeter.)
    }
}
