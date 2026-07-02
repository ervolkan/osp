# Architectural Trajectory Navigation: From Target Coordinates to Measurement Predicates

**OSP Paper 2 Draft v1.2** · Target: arXiv then ACM TOSEM
**Authors:** Volkan ER
**Date:** 2026-07-02
**Companion paper:** *Ontological Space Protocol: Modeling Software as a Conceptual Space with Epistemological Witnessing* (Paper 1, v2.6 — static space). This paper extends OSP to the dynamic, agent-driven regime.
**Revision:** v1.1 → v1.2 (review pass 2): three consistency fixes — RQ7 table row relabeled ("Synthetic RQ9 baseline" → "G2c-4 real-LLM synthetic smoke, not a policy test"); Conclusion now states "external 24/24; real-LLM total 26/26" instead of conflating the two; Related Work citations corrected (ArchUnit [11], Reflexion Models [12]); temperature 0.3 annotated with the single-run stochasticity caveat inline.
**Prior v1 → v1.1:** separated three evidence strata (G2c-3 controlled-mock / G2c-4 real-LLM smoke / G2c-5 external corpus) throughout — the RQ9 mechanism is mock-isolated, not a real-LLM result; fixed section cross-references; split Appendix into A (external), B (real-LLM smoke), C (controlled RQ9 fixture); added Evidence Strata table, run-metadata table, and per-repository graph statistics; softened "generalizes" to "preliminary external-corpus evidence"; flagged RQ8 token difference as outlier-driven; renumbered references standalone [1]–[12]; softened "Software Physics program" to "framing supported."

---

## Abstract

AI coding agents navigate software projects by reasoning toward a goal, but they typically formulate that goal as a target state to be *reached* — a coordinate, a patch, a desired measurement. This creates an epistemological hazard: if an agent can observe the target coordinate it is supposed to produce, its output is no longer an independent measurement of architectural health but a self-fulfilling projection. We present **Architectural Trajectory Navigation**, the dynamic extension of the Ontological Space Protocol (OSP), in which an agent's task is formulated not as a target coordinate but as a **measurement predicate** — a deterministic, engine-evaluated condition on the project's coordinate space (e.g., "coupling of module M ≤ 0.55").

The agent never observes the target coordinate; it observes only the predicate and a structural context (its focus node and current outgoing imports). A **deterministic predicate gate** (Q5.b) measures the agent's proposed structural delta against the predicate *before* any mutation reaches the project space, producing a typed `AttemptOutcome` (Completed / NotCompleted) and a mutation decision (Reject / AcceptAsProgress / AcceptAsCompleted). An **adaptive control loop** (maneuver limit + calibration feedback retry) bounds the agent's attempts and feeds gate rejections back as structured calibration messages rather than terminal failures.

We evaluate the approach on an external corpus of three public repositories spanning three programming languages — **chalk** (JavaScript), **click** (Python), and **cobra** (Go) — using a real LLM (GPT-4o-mini) under a controlled structural harness. Across **24 external cells** (3 repos × 2 task types × 2 policies × 2 feedback modes) plus 2 real-LLM synthetic-smoke cells, the agent achieved **26/26 Completed** under first-attempt-dominant scenarios, at a mean cost of **1104 tokens per completed cell**, with **0 axis regressions** (no cell degraded a non-target architectural axis). Separately, a controlled synthetic fixture with scripted proposals isolates the RQ9 policy mechanism: progress-checkpoint accumulation completes under bounded attempts where strict rejection cannot. We report the external results as preliminary evidence that the structural-proposal pipeline works end-to-end on real analyzed repositories; we are explicit that this measures *structural delta success* (graph-level `removed_edges`), not source-code patch correctness, and that the first-attempt dominance suppresses the signals our controlled-mock experiment uses to distinguish feedback (RQ8) and policy (RQ9) conditions. The implementation (Rust workspace, type-level invariants INV-T1–T8) and raw evidence (JSON) are open-source; corpus repositories are cloneable.

---

## 1. Introduction

Paper 1 established OSP's *static* space: a coordinate system that positions every module along five axes (coupling, cohesion, instability, entropy, witness-depth), advances project time only through a witnessed commit, and rejects vision-violating or rule-violating claims through deterministic pre-mutation gates (Q4–Q6). That model is observational and commit-time: it describes *what is* and gates *what is proposed*.

This paper addresses the complementary question: **how does an AI agent navigate from the current space toward a healthier one?** The answer is not "give the agent the target coordinate and let it optimize," because doing so collapses the distinction between *measurement* and *belief*.

### 1.1 The Problem

Dynamic, agent-driven architectural change faces three structural challenges:

1. **The coordinate-exposure hazard.** If the agent's task is expressed as a target coordinate `P_target` (e.g., "move module M to coupling 0.40, cohesion 0.70"), and the agent can observe `P_target`, then the agent can produce a delta that *declares* those coordinates rather than *achieving* them through genuine structural change. Paper 1 already enforces that the engine — not the agent — computes coordinates from structural deltas (inv #4 of the static model). Extending this to the dynamic regime requires that the *task itself* carry no target coordinate to the agent.

2. **Open-loop architectural drift.** Autonomous coding agents (SWE-agent [10], RepoCoder [9]) operate by file-system actions guided by LLM reasoning. They achieve high task-completion rates but provide no architectural safety net: an agent can introduce a circular import, violate a dependency rule, or raise coupling beyond a project's tolerance, with no deterministic rejection before the change lands. The drift accumulates across attempts because nothing measures the architectural consequence of each step.

3. **Metric alienation, dynamic edition.** Paper 1 noted that software metrics are computed but rarely actionable [7]. The dynamic version of this problem is sharper: even when an agent *could* reduce coupling, it has no bounded notion of *when to stop*, *whether a partial improvement counts*, or *whether fixing one axis broke another*. Without a task model that encodes acceptance as a measurement, "done" is a social judgment, not a measurable property.

### 1.2 Our Approach: Trajectory Navigation

OSP addresses these challenges through three interlocking ideas:

**Task as measurement predicate (INV-T1).** A task is not a coordinate to reach but a `PredicateSet` over measured coordinates — e.g., *coupling(module M) ≤ 0.55 ∧ instability(module M) ≤ 0.60*. The predicate's `preferred_vector` (the ideal center of the acceptance region) exists in the internal task plan but is **stripped from the agent's view** (`AgentTaskView`). The agent sees the predicate, the allowed operations, the focus node, and its current outgoing imports (`AgentStructuralContext`); it never sees the target coordinate. This is the epistemic core of the paper: *the agent is asked to satisfy a measurable condition, not to imitate a desired state.*

**Deterministic predicate gate (Q5.b).** Each agent attempt produces a `DeltaProposal` (additive new nodes/edges and subtractive `removed_edges`). The engine applies the delta to a *hypothetical* copy of the space, recomputes the affected coordinates, and evaluates the predicate gate. The gate yields a typed `AttemptOutcome` — `gate_decision` (PassedAll / RejectedByPredicate / RejectedBySyntax / …), `predicate_completion` (Completed / NotCompleted), and `mutation_decision` (Reject / AcceptAsProgress / AcceptAsCompleted). Nothing the agent says about coordinates is trusted; only what the engine measures counts.

**Adaptive control loop with bounded maneuver and calibration feedback.** The agent retries under a `maneuver_limit` (INV-T7) — a bounded number of attempts per task. Predicate or syntax rejections are not terminal: they are converted into structured `HallucinationType` calibration messages fed back into the next attempt's context (RQ8). A `PredicateFailurePolicy` decides whether a non-yet-satisfying attempt that *improved* the metric is accepted as a progress checkpoint (state advances) or strictly rejected (state frozen) — the lever that separates incremental refactors from one-shot fixes (RQ9).

### 1.3 Contributions

This paper makes four contributions:

1. **Predicate-based task ontology with epistemic invariant INV-T1 (Section 3).** We define a task as a measurement predicate and enforce, at the type level and at runtime, that the agent's view (`AgentTaskView`) contains *no target-coordinate fields* — only the predicate, the allowed operations, the focus node, and structural context. We verify this live: on a running MCP server, the agent-facing response carries zero occurrences of `preferred_vector`, `target_region`, or `milestone_target_vector` (Section 6.3). The companion internal plan (`InternalTaskPlan`) retains the coordinate for navigation; a one-way projection discards it. This generalizes Paper 1's inv #4 (engine computes coordinates) from the *claim* layer to the *task* layer.

2. **Adaptive control loop with witness-policy isolation (Section 4).** We specify the `AgentNavigator` loop — LLM proposal → engine measurement → predicate gate → typed outcome → calibration feedback retry — bounded by a maneuver limit. We isolate the witness policy via a `NavigatorWitnessPolicy` enum: production navigation inherits Paper 1's two-witness security model (`Production`, min_approvers = 2), while controlled experiments use a scoped `HarnessAutoApprove` mode that cannot leak into production. This separation was itself a finding: an early corpus run that produced 0/24 completions traced to an empty witness set silently failing quorum, not to the navigator logic (Section 8.5).

3. **Deterministic predicate gate with typed outcomes and lane discipline (Section 5).** We define the atomic `commit_task_claim` pipeline (Q4 syntax → task binding → Q5 vision → Q5.b predicate gate → Q6 rule → apply) and the `ApplyTarget` lane mapping (INV-T8): `Reject` applies nothing, `AcceptAsProgress` advances a trajectory checkpoint (never the mainline), `AcceptAsCompleted` advances the mainline. A trajectory-loss predicate (INV-T6) distinguishes *completion* (predicate satisfied) from *directional improvement* (loss decreased) and from *regression* (a non-target axis worsened), so a coupling fix that raised instability is rejected even if coupling alone improved.

4. **Multi-language external-corpus evaluation with real LLM (Section 6).** We evaluate on three public repositories across three languages — chalk (JavaScript), click (Python), cobra (Go) — using GPT-4o-mini under a controlled structural harness. Across 24 external cells plus 2 real-LLM synthetic-smoke cells, the agent achieved 26/26 Completed at a mean of 1104 tokens per cell, with 0 axis regressions across 24 cells. We report this as preliminary evidence that the structural-proposal pipeline works on real analyzed repositories, while being explicit about two boundaries: the success is *structural* (graph-level `removed_edges`, not source patches), and first-attempt dominance suppresses the feedback (RQ8) and policy (RQ9) signals that a separate controlled-mock fixture (G2c-3) isolates.

---

## 2. Motivating Example

Consider an agent asked to *reduce the coupling* of a module `M` that imports four dependencies (coupling = 4/(1+4) = 0.80), with an acceptance predicate *coupling(M) ≤ 0.55*. In a conventional agent workflow, the agent is told the goal in natural language, edits source files, and a reviewer later judges whether the result is "good enough." Whether the task is *done* is a judgment, not a measurement; whether the agent *knew the target* is uncontrolled.

In OSP trajectory navigation, the same interaction proceeds as follows:

1. **Task binding (operator side).** A trusted operator (human or bootstrap config) creates a task with predicate *coupling(M) ≤ 0.55*, `allowed_operations = [RemoveImport, ExtractModule]`, and a `preferred_vector` (the ideal coordinate center of the acceptance region). The operator — not the agent — owns the target coordinate (INV-T1). The task is decomposed deterministically into milestones; the agent never decomposes (INV-T2).

2. **Agent view projection (INV-T1).** The engine projects the task to an `AgentTaskView` containing: the predicate (coupling ≤ 0.55 on node M), the allowed operations, the current measured position of M, and an `AgentStructuralContext` naming M as the focus node and listing M's current outgoing import edges (M → D1, M → D2, M → D3, M → D4). The `preferred_vector` is **absent** from this view. The agent is asked to satisfy a measurable condition given a structural situation; it is *not* asked to move toward a coordinate.

3. **Proposal.** The agent produces a `DeltaProposal`. Because the structural context listed M's imports and the prompt specifies the `removed_edges` output field, a capable LLM can propose removing, say, edge M → D1 (an `OpKind::RemoveImport`, which the engine honors rather than treating as a label).

4. **Engine measurement.** The engine applies the delta to a *hypothetical* copy of the space, recomputes M's coupling (now 3/(1+3) = 0.75), and evaluates the predicate: *0.75 ≤ 0.55* is false. The gate yields `NotCompleted`.

5. **Outcome and feedback.** Under `AcceptImprovement` policy, the improvement (0.80 → 0.75) is accepted as a *progress checkpoint* — the trajectory state advances to coupling 0.75, but the mainline is not yet updated (INV-T8: `AcceptAsProgress → TrajectoryCheckpoint`). The agent retries with this updated context. Removing two more edges (M → D2, M → D3) yields coupling 1/(1+1) = 0.50; the predicate is satisfied; the outcome is `Completed` and the mainline advances. Under `StrictReject`, by contrast, the state would freeze at 0.80 after each failed attempt, and the agent would exhaust its maneuver limit without accumulating progress — the mechanism our synthetic experiment (RQ9) isolates.

6. **Regression guard (INV-T6).** If removing M → D1 had raised M's instability above its own hard cap, the gate would reject the attempt *even though coupling improved*, because trajectory loss increased on a non-target axis. Completion is predicate satisfaction; improvement is loss decrease; regression is a non-target axis worsening — three distinct epistemic states, never conflated.

The key contrast with conventional agents: at no point did the agent observe a coordinate to imitate, and at every point "done" was a measurement, not a judgment.

---

## 3. Trajectory Ontology

Paper 1's static space is a set of nodes and edges with computed coordinates, advanced only through witnessed commits. Trajectory navigation adds a *goal-directed* layer: a `Trajectory` is a sequence of `Milestone`s, each decomposed into `Task`s, each attempted zero or more times by an agent producing `DeltaProposal`s that the engine measures against a `PredicateSet`. The ontology is type-level (Rust structs in `osp-core/src/trajectory.rs`), and eight invariants (INV-T1–T8) govern it.

### 3.1 Task as PredicateSet

A `Task` carries a `target_predicate_set` — not a target coordinate, but a condition on measured coordinates:

```rust
pub struct Task {
    pub id: TaskId,
    pub milestone_id: MilestoneId,
    pub label: String,
    pub target_predicate_set: PredicateSet,   // the acceptance condition
    pub policy: TaskPolicy,                    // maneuver limit, failure policy
    pub allowed_operations: Vec<OpKind>,       // RemoveImport, ExtractModule, ...
    pub constraints: Vec<Constraint>,
    pub status: TaskStatus,
}

pub struct PredicateSet {
    pub mode: PredicateMode,                   // All (AND) | Any (OR) | Weighted
    pub predicates: Vec<WeightedPredicate>,
    pub preferred_vector: Option<RawPosition>, // INTERNAL navigation center (debug/distance)
}

pub enum PredicateMode { All, Any, Weighted }
```

The `preferred_vector` is the ideal center of the acceptance region; it exists for navigation math (computing trajectory loss) and operator debugging, but it is **stripped** before reaching the agent (INV-T1, §3.4). Each individual predicate is a `MetricPredicate`:

```rust
pub struct MetricPredicate {
    pub metric: PredicateAxis,                 // Coupling | Cohesion | Instability | Entropy | WitnessDepth
    pub operator: ComparisonOp,                // Le | Lt | Ge | Gt | Eq
    pub threshold: f64,
    pub scope: PredicateScope,                 // Node(id) | Milestone | Trajectory
    pub required_source: Option<MetricSource>, // None | Some(Scip) — provenance gate (INV-T4)
    pub tolerance: f64,
}
```

The `required_source` field is the provenance gate (INV-T4): a task may declare that it can only be closed by a measurement whose source is, e.g., `Scip` — a placeholder-sourced value satisfying the numeric threshold is rejected as `SourceInsufficient`, because "we don't know" must never be conflated with "we measured 0.5" (extending Paper 1's MetricValue provenance discipline into the predicate layer).

### 3.2 The Agent View — INV-T1 projection

The engine projects a task to an `AgentTaskView`, which is what the LLM actually receives:

```rust
pub struct AgentTaskView {
    pub task_id: TaskId,
    pub target_predicate: PredicateSet,        // the condition (no coordinate)
    pub current_measurement: RawPosition,      // current state — FREE (where the agent IS)
    pub allowed_operations: Vec<OpKind>,
    pub constraints: Vec<Constraint>,
    pub feedback_history: Vec<String>,         // calibration messages from prior attempts (D4)
    pub structural_context: AgentStructuralContext,  // focus node + current outgoing imports (G2c-4)
}
```

Crucially, `AgentTaskView` has **no `preferred_vector`, no `target_region`, no `milestone_target_vector` field**. The `current_measurement` is present — the agent must know where it *is* — but the coordinate it should *reach* is absent. The companion `InternalTaskPlan` retains the coordinate; conversion is one-way (internal → agent). `structural_context` (added in G2c-4) gives the agent its focus node identifier and current outgoing import edges — structural information, not a coordinate — which is sufficient for the LLM to propose a coupling-reducing `removed_edges` delta.

### 3.3 AttemptOutcome — three distinct epistemic states

Each attempt produces an `AttemptOutcome` that separates three things conventional agents conflate:

```rust
pub struct AttemptOutcome {
    pub gate_decision: GateDecision,             // PassedAll | RejectedByPredicate | RejectedBySyntax | ...
    pub predicate_completion: PredicateCompletion, // Completed | NotCompleted
    pub mutation_decision: MutationDecision,      // Reject | AcceptAsProgress | AcceptAsCompleted | RequireOperatorApproval
    pub witness_status: Option<WitnessStatus>,
}

pub enum MutationDecision { Reject, AcceptAsProgress, AcceptAsCompleted, RequireOperatorApproval }
```

- **Completion** is *predicate satisfaction* — a measurement cleared the bar.
- **Mutation decision** is *what to do with the delta* — reject, checkpoint as progress, accept as completed, or escalate.
- **Progress signal** is *loss decreased* (trajectory loss between current and preferred, INV-T6).

These are distinct: a delta can improve loss (progress) without completing (predicate still false); a delta can complete without being the final answer (if multiple predicates remain); a delta can be rejected for regression even if the target axis improved (a coupling fix that raised instability, INV-T6).

### 3.4 Trajectory Invariants INV-T1–T8

| INV | Name | Guarantee | Type-level enforcement |
|---|---|---|---|
| **T1** | Predicate epistemology | The agent's task is a predicate; the target coordinate is not serialized to the agent. `current_measurement` (where the agent *is*) is free; `preferred_vector` (where it should *go*) is forbidden. | `AgentTaskView` serde struct has no target-coordinate field; `InternalTaskPlan` ↔ `AgentTaskView` is a one-way projection; runtime leak scan asserts zero occurrences of `preferred_vector` / `target_region` / `milestone_target_vector`. |
| **T2** | Operator defines the target (Genesis) | Trajectories and milestone target regions are defined by a trusted operator; the agent produces structural change, never the target itself. | `OperatorCapability` has a private constructor (`_private: ()`), constructible only at the trusted bootstrap boundary; `Trajectory::new` requires the capability. Agent code cannot produce it. |
| **T3** | Engine measures | The predicate is evaluated on an engine-measured value (`claim.computed_raw`), never on an agent-declared `PositionHint`. | `MetricPredicate::evaluate` takes a `ProvenancedRawPosition` produced by the engine; the agent cannot supply it. |
| **T4** | Predicate provenance | A predicate may require a measurement source (e.g., `Scip`); a placeholder-sourced value cannot close a task even if it satisfies the numeric threshold. | `ProvenancedRawPosition` carries per-axis `AxisMetric { value, source }`; `evaluate` returns `SourceInsufficient` when `required_source` is unmet. |
| **T5** | Task ≠ Claim | A Task is a condition set; a Claim is work. A task may require multiple attempts; Q5.b only accepts task-bound claims. | `Claim.task_id: Option<TaskId>` (static Paper-1 claims may be taskless); `bind_task_claim` produces the `TaskBoundClaim` the predicate gate requires. |
| **T6** | Failure ≠ regression | Predicate failure does not require negative progress. Completion, mutation decision, and progress signal are separated; a non-target axis regression rejects even if the target axis improved. | `AttemptOutcome` separates the three; trajectory-loss predicate `loss_after < loss_before − ε ∧ max_axis_regression respected` gates `AcceptAsProgress`. |
| **T7** | Maneuver limit | After N consecutive rejected attempts (default 5, operator-configurable), the system emits a Trajectory Deviation Alert and hands control to the operator, preventing infinite context-loops and token explosion. | `TaskPolicy.maneuver_limit` + `ManeuverLimit` config; counter-driven `NavigatorResult::ExceededManeuverLimit`. |
| **T8** | Progress checkpoint isolation | An `AcceptAsProgress` mutation does not complete the task and cannot be promoted to the mainline; it stays in the `TrajectoryCheckpoint` lane. Only `AcceptAsCompleted` reaches `Mainline`. | `ApplyTarget` lane mapping: `Reject → NotApplied`, `AcceptAsProgress → Lane(TrajectoryCheckpoint)`, `AcceptAsCompleted → Lane(Mainline)`, `RequireOperatorApproval → Lane(Sandbox)`. |

These eight invariants extend Paper 1's INV #1–#15 into the dynamic regime. The most consequential is **INV-T1**: it generalizes Paper 1's inv #4 (the engine computes coordinates from structural deltas) from the *claim* layer to the *task* layer. In Paper 1, an agent could not declare a coordinate because the engine computed it; in Paper 2, an agent cannot even *see* the target coordinate because the task carries only a predicate. This is the epistemic core that the rest of the paper evaluates.

---

## 4. Adaptive Control Loop

The `AgentNavigator` is the loop that drives an agent from the current space toward predicate satisfaction. It is bounded, measured, and feedback-driven — never open-loop.

### 4.1 The loop

```
for attempt in 1..=maneuver_limit:
    view  = project_to_agent(task, current_measured, structural_context, feedback_history)   # INV-T1
    delta = llm.complete(view)                                                                # agent proposes
    claim = build_claim(delta, task_id, affected_nodes)                                       # bind task (INV-T5)
    measured = engine.compute_raw_from_delta(claim)                                          # INV-T3: engine measures
    outcome = engine.commit_task_claim(claim, measured, target, policy)                      # Q5.b gate (§5)
    evidence.push(claim, measured, outcome)                                                   # INV-T6 ledger
    match outcome.mutation_decision:
        AcceptAsCompleted   => return Completed(attempts, tokens)                             # INV-T8 → Mainline
        AcceptAsProgress    => advance checkpoint; continue                                   # INV-T8 → Checkpoint
        Reject              => push calibration feedback; continue                            # D4 retry
        RequireOperatorApproval => return RequiresOperatorApproval                           # escalate
return ExceededManeuverLimit(attempts)                                                        # INV-T7
```

Four properties distinguish this from a conventional generate-and-apply agent:

1. **The agent never sees the target coordinate** (INV-T1). The `project_to_agent` step strips `preferred_vector`; only the predicate, the current measurement, the structural context, and any feedback survive.

2. **The engine, not the agent, measures** (INV-T3). `compute_raw_from_delta` applies the delta to a hypothetical copy of the space, recomputes affected coordinates, and returns a `ProvenancedRawPosition`. The agent's `PositionHint` (if any) is ignored by the gate.

3. **Every attempt leaves evidence** (INV-T6). The `evidence` ledger records `before`, `after`, `gate_decision`, `predicate_completion`, `mutation_decision`, and `token_cost` for each attempt — including rejections. This was itself a finding: an early corpus run (G2c-1) produced 0/24 completions with an *empty* evidence ledger, which hid the true failure mode (a silent witness-quorum failure) until reject-evidence was added (G2c-1b, raising per-cell evidence from 0 to 5 entries).

4. **The loop is bounded** (INV-T7). A `maneuver_limit` caps attempts per task; exceeding it yields `ExceededManeuverLimit` and hands control to the operator, preventing the token-explosion failure mode of unbounded retry.

### 4.2 Calibration feedback (D4)

A rejected attempt is not terminal. When the engine rejects a proposal (syntax error, rule violation, parse error), it classifies the failure as a typed `HallucinationType` and produces a `calibration_message` appended to `feedback_history`. The next attempt's `AgentTaskView` carries this history, so the LLM sees "Attempt 1: empty DeltaProposal — provide new_nodes/new_edges" rather than silently failing the same way. This is the mechanism RQ8 evaluates: feedback should reduce wasted attempts and lower total token cost when the agent needs more than one try.

An important boundary, surfaced by our external-corpus evaluation (§6.6): a *numeric* predicate rejection — a structurally valid proposal whose measured coupling still exceeds the threshold — produces **no** calibration message in the current implementation, because the rejection is a measurement outcome rather than a structural hallucination. The feedback channel is therefore active for syntax/rule/parse failures but silent for numeric near-misses. This shapes the RQ8 result: where the LLM succeeds on the first attempt, the feedback channel never fires, and where it fails numerically, the channel is silent in both the with- and without-feedback arms.

### 4.3 Witness policy isolation

The navigator inherits Paper 1's two-witness commit model: a delta that reaches `Mainline` requires quorum support from independent non-author witnesses (INV-T1 of Paper 1's static model). For *controlled experiments*, requiring two live witnesses would conflate the navigator logic under test with the witness-availability question. We therefore isolate the two via a `NavigatorWitnessPolicy` enum:

```rust
pub enum NavigatorWitnessPolicy {
    Production,           // Paper 1 security model: WitnessSet with min_approvers = 2, quorum 1.5
    HarnessAutoApprove,   // Controlled experiment: WitnessSet.with_quorum(0, 0.0)
}
```

Production navigation uses `Production`; the corpus harness uses `HarnessAutoApprove`. The harness mode is *scoped* — it cannot leak into the production navigator because the policy is an explicit field set at construction, not a global. This separation was forced by a real failure: the G2c-1 corpus run's 0/24 completions traced not to the navigator loop but to an empty `WitnessSet` silently failing quorum (default `min_approvers = 2`, quorum never reached). The fix was not to lower the production threshold but to make the experimental threshold a named, isolated variant (§8.5). Every evidence row records its `witness_mode` so a reader can verify that no production run used auto-approval.

---

## 5. Deterministic Predicate Gating

The predicate gate (Q5.b) is the deterministic core that turns an agent proposal into a typed outcome. It is the dynamic analogue of Paper 1's Q4–Q6 pre-mutation gates, evaluated on a hypothetical measurement rather than a committed claim.

### 5.1 The commit_task_claim pipeline

```
commit_task_claim(claim, measured, target, loss_before, policy):
    (Q4)  validate OutputContract — DeltaProposal schema valid                    [deterministic]
    (bind) bind_task_claim — Claim.task_id ↔ Task (TaskBoundClaim, INV-T5)        [deterministic]
    (Q5)  vision deviation θ ≤ θ_bound                                            [deterministic]
    (Q5.b) PredicateGate.evaluate(measured, task.target_predicate_set) →          [deterministic]
              PredicateSetResult { PassedAll | NotCompleted(reason) | SourceInsufficient }
    (Q6)  ∀ Rule R: R(ΔS) ≠ Violated                                              [deterministic]
    apply ApplyTarget per MutationDecision (INV-T8)
```

Every stage is deterministic Rust code with `-D warnings`; there is no LLM in the gate path. The predicate gate (Q5.b) consumes the engine-measured `ProvenancedRawPosition` (INV-T3) and the task's `PredicateSet`, and yields a `PredicateSetResult`. Three outcomes are possible:

- **PassedAll** — every predicate (in `All` mode) is satisfied with sufficient provenance. Completion.
- **NotCompleted(reason)** — at least one predicate is numerically unsatisfied. The reason identifies which axis and by how much.
- **SourceInsufficient** — a predicate's `required_source` is unmet (INV-T4); the measurement is not trustworthy enough to close the task, regardless of the number.

### 5.2 Mutation decision and the lane discipline (INV-T8)

The gate's result, combined with the task's `PredicateFailurePolicy`, produces a `MutationDecision`, which maps to an `ApplyTarget` and a commit lane:

| MutationDecision | ApplyTarget | CommitLane | Meaning |
|---|---|---|---|
| `Reject` | `NotApplied` | (none) | Delta never applied; stays hypothetical. Numeric near-miss, syntax error, rule violation, or regression. |
| `AcceptAsProgress` | `Lane(TrajectoryCheckpoint)` | TrajectoryCheckpoint | Delta applied to a checkpoint; loss decreased but predicate unsatisfied. Cannot reach Mainline. |
| `AcceptAsCompleted` | `Lane(Mainline)` | Mainline | Predicate satisfied; delta promoted to the mainline. |
| `RequireOperatorApproval` | `Lane(Sandbox)` | Sandbox | Delta applied in isolation pending operator decision. |

The invariant that matters most is **progress ≠ merge**: an `AcceptAsProgress` mutation advances the trajectory checkpoint (so the next attempt starts from the improved state) but is structurally barred from the mainline. Only `AcceptAsCompleted` — predicate satisfaction — reaches the mainline. This is what makes incremental refactors safe: an agent can make bounded progress across multiple attempts without any single intermediate step being mistaken for a completed, merged change.

### 5.3 Policy: StrictReject vs AcceptImprovement

The `PredicateFailurePolicy` governs what happens to a *non-completing* attempt whose loss decreased:

- **StrictReject** — the attempt is rejected; the trajectory state is *frozen*. The next attempt starts from the same position. This models a one-shot discipline: only a completed delta advances state.
- **AcceptImprovement** — the attempt is accepted as a progress checkpoint (INV-T8 → TrajectoryCheckpoint); the trajectory state advances to the improved measurement. This models an incremental refactor: partial progress accumulates toward the predicate within the maneuver limit.

This is the lever RQ9 isolates. On a task requiring multiple steps (e.g., removing four import edges one at a time to drop coupling 0.80 → 0.50), `AcceptImprovement` accumulates state to completion within bounded attempts, while `StrictReject` freezes state and exhausts the maneuver limit on the same sequence. The difference is *not* token cost (both use the same number of attempts) but *whether bounded attempts suffice to complete*.

### 5.4 Trajectory loss and the regression guard (INV-T6)

Trajectory loss is the distance between the measured position and the `preferred_vector` (the acceptance-region center). The gate accepts an `AcceptAsProgress` only when:

```
loss_after < loss_before − ε   ∧   max_axis_regression ≤ cap
```

The second clause is the regression guard: if the delta improved the target axis but worsened a non-target axis beyond a cap (e.g., a coupling fix that raised instability by 0.35), the attempt is rejected even though the *primary* metric improved. This resolves the axis-oscillation problem (F5 in the roadmap): an agent that fixes one axis while breaking another is detected and rejected, rather than oscillating between two bad states. Our external-corpus evaluation reports 0 axis regressions across 24 cells (§6.8), evidencing that the LLM's coupling-reducing proposals did not trade one axis for another under this guard.

---

## 6. Evaluation

We evaluate trajectory navigation on two questions of mechanism and two questions of generality. The mechanism questions (RQ8, RQ9) are answered by synthetic controlled fixtures where the independent variable is isolated; the generality questions (RQ6, RQ7) are answered by an external corpus of real repositories analyzed and navigated by a real LLM. An invariant question (RQ5/INV-T1) is answered by live verification.

### 6.1 Research Questions

- **RQ5 (Invariant).** Does the agent-facing task view leak the target coordinate? (INV-T1 live verification.)
- **RQ6 (Cost).** What is the token cost per completed task cell under a real LLM on external repositories?
- **RQ7 (Success).** At what rate does the agent produce a predicate-satisfying structural proposal, and on which attempt?
- **RQ8 (Feedback).** Does calibration feedback change success rate or token cost (with vs. without feedback)?
- **RQ9 (Policy).** Does the progress-accumulation policy (`AcceptImprovement`) enable completion under bounded attempts where strict rejection (`StrictReject`) cannot?

### 6.2 Methodology

**Corpus.** Three public repositories, selected for language diversity and cloneability: **chalk** (JavaScript, 14 `.js` + 5 `.ts` files), **click** (Python, 63 `.py` files), **cobra** (Go, 36 `.go` files). All are cloned via `scripts/clone-corpus.ps1` (shallow clones). Paper 1 analyzed these same repositories for static metrics (RQ4 cohesion); here they are the substrate for dynamic navigation.

**Analyzer.** Each repository is analyzed by `osp-analyzer` with `AdapterRegistry::default_all()`, which dispatches to the JavaScript, Python, or Go tree-sitter adapter by file extension (no language hint required). The resulting `Space` graph provides real module nodes and `Imports` edges; coupling (x) and instability (z) are topological (out-degree / (1+out-degree) and Ce/(Ca+Ce) respectively), computed without a SCIP index. Cohesion (y) is placeholder where no SCIP index is available; tasks predicate on coupling/instability only, so this does not affect the gate (see §6.9 caveat).

**LLM.** GPT-4o-mini via `osp-llm-runtime`'s `RuntimeLlmClient`, invoked through the navigator loop with the structural-context-enhanced prompt (the `removed_edges`/`affected_nodes` output snippet and the `AgentStructuralContext` focus-node + current-outgoing-imports fields introduced in G2c-4). Each cell is a single run (stochasticity is a stated threat, §6.9).

**Experiment matrix.** For each repository: 2 task types (CouplingReduction: coupling ≤ 0.55; InstabilityReduction: instability ≤ 0.60) × 2 policies (StrictReject, AcceptImprovement) × 2 feedback modes (With, Without — the `Without` arm wraps the LLM in a `NoFeedbackWrapper` that clears `feedback_history` before each call). Maneuver limit = 5. This yields 3 × 2 × 2 × 2 = **24 external cells**. The released evidence file additionally contains 2 G2c-4 real-LLM synthetic-smoke rows (24 + 2 = 26 real-LLM rows total). The RQ9 *mechanism* is evaluated on a separate controlled fixture with scripted (mock) proposals, not the real LLM (see Evidence Strata above). Witness mode is `HarnessAutoApprove` for all cells (controlled experiment; production runs would use `Production`, §4.3).

**Target-node selection.** The highest-coupling (for CouplingReduction) or highest-instability (for InstabilityReduction) `Module`/`Concept` node, selected deterministically (score descending, NodeId ascending tie-break). This avoids the bias of always targeting Node(0).

**Evidence strata.** The evaluation draws on three distinct evidence strata that must not be conflated. They differ in corpus, LLM backend, and purpose:

| Stratum | Purpose | Corpus | LLM | Key result |
|---|---|---|---|---|
| **G2c-3 controlled fixture** | RQ9 *policy mechanism* isolation | synthetic (5 nodes) | **mock / scripted** | `AcceptImprovement` completes under bounded attempts; `StrictReject` exhausts the maneuver limit (state frozen) |
| **G2c-4 real-LLM smoke** | schema + structural-context feasibility | synthetic (5 nodes) | GPT-4o-mini | 2/2 Completed (~1160 tokens); proves the real LLM can emit valid `removed_edges` given the structural-context prompt. *Not* a policy test. |
| **G2c-5 external corpus** | external structural-proposal loop | chalk/click/cobra | GPT-4o-mini | 24/24 Completed (~1104 tokens/cell); small-corpus external evidence |

The G2c-3 fixture uses scripted proposals to isolate the *policy variable* (the navigator's accumulation behavior) from LLM stochasticity; it is the sole source of the RQ9 *mechanism* claim. G2c-4 and G2c-5 use a real LLM and report *success-rate and cost*; because the real LLM satisfies predicates on the first attempt, they do not differentiate the policies (RQ9 external result is neutral, §6.7). Section 6 reports the real-LLM strata (G2c-4, G2c-5) for RQ5–RQ8 and the controlled-mock stratum (G2c-3) for the RQ9 mechanism.

**Reproducibility.** The corpus runner is `crates/osp-analyzer/examples/g2c_corpus_matrix.rs`; invocation is `--llm real --synthetic-only --external --out <file>`. Raw evidence is released as `docs/paper2-notes/evidence/g2c-external-corpus-20260702.json` (24 external + 2 G2c-4 synthetic-smoke rows). The G2c-3 controlled-mock run and the full mock validation run (50 cells, 0 errors) are released as reproducibility checks that do not consume API budget.

**Run metadata (real-LLM strata).**

| Parameter | Value |
|---|---|
| Model | `gpt-4o-mini` (OpenAI) |
| Temperature | 0.3 (non-zero; a single run per cell — stochasticity is a stated threat, §6.9) |
| Witness mode | `harness_auto_approve` (min_approvers = 0; controlled experiment) |
| Maneuver limit | 5 (external), 3 (G2c-3 controlled fixture) |
| Prompt schema | `delta_proposal_output_format_snippet` (removed_edges + affected_nodes) + `AgentStructuralContext` |
| OSP commit (run) | `e331fc2` |
| Corpus commits | chalk `aa06bb5`, click `6ec99f8`, cobra `ad460ea` (shallow clones via `scripts/clone-corpus.ps1`) |

### 6.3 RQ5: Epistemic Projection (INV-T1)

We verify INV-T1 live on a running MCP server (`osp-mcp`, stdio transport). The agent-facing tool `osp_get_agent_task_view` returns a JSON envelope. We scan the serialized response for the forbidden coordinate tokens.

| Forbidden token | Occurrences in agent view |
|---|---|
| `preferred_vector` | 0 |
| `target_region` | 0 |
| `milestone_target_vector` | 0 |
| `task_id`, `target_predicate`, `current_measurement` | present (allowed) |

**Result.** The agent-facing view carries zero target-coordinate fields. Two layers enforce this: (1) *type-level* — `AgentTaskView` has no such field, so serde cannot serialize it; (2) *runtime* — `McpEnvelope::assert_no_coordinate_leak` scans the response and blocks emission (returning `TARGET_COORDINATE_LEAK_BLOCKED`) if a forbidden token appears. Seven INV-T1 integration tests cover the projection. The `current_measurement` field (where the agent *is*) is intentionally present — knowing the current state is not a leak; knowing the target state would be.

**Caveat.** INV-T1 is verified on the *structural* projection. The `AgentStructuralContext` (focus node + outgoing imports) added in G2c-4 is structural information, not a coordinate, and is permitted. A test (`g2c4_structural_context_allowed_but_target_coordinate_forbidden`) pins this distinction: the focus node identifier and its import edges are allowed; the preferred vector is not.

### 6.4 RQ6: Token Cost

| Repository | Language | Mean tokens/cell | Min | Max |
|---|---|---:|---:|---:|
| chalk | JavaScript | 1035 | 1007 | 1050 |
| click | Python | 1248 | 1025 | 2303 |
| cobra | Go | 1030 | 1020 | 1039 |
| **Overall (24 cells)** | — | **1104** | 1007 | 2303 |

**Result.** The mean cost per completed external cell is **1104 tokens** (total external: 26,503 tokens across 24 cells). Cost is largely language-independent: chalk (JS) and cobra (Go) cluster at ~1030–1035 tokens; click (Python) is higher (1248) primarily because it has the largest graph (63 nodes, 40 edges) and thus a larger structural context. The single outlier — click/Coupling/Accept/Without at 2303 tokens — is the only cell requiring two attempts (the first proposal was syntactically rejected, the second succeeded); its cost is roughly 2× a single-attempt cell, consistent with the prompt being sent twice.

**Result (vs. synthetic baseline).** The G2c-4 synthetic-fixture smoke (2 cells) measured 1162 / 1179 tokens per Completed. The external-corpus mean (1104) is consistent with this, indicating that adding real-repository structural context (the target node's actual outgoing imports) does not materially inflate prompt size relative to the synthetic fixture. This extends Paper 1's RQ5 finding (coordinate prompts are compact) into the dynamic regime: the *per-attempt* prompt remains compact even when it carries real focus-node imports.

**Caveat.** Token counts are real GPT-4o-mini API responses (not a chars/4 approximation). They measure prompt+completion size per cell, not end-to-end task cost including analysis or any hypothetical code-generation step. The structural delta produced is a graph-level `removed_edges` proposal, not a source patch (§6.9).

### 6.5 RQ7: Task Success

| Corpus | Cells | Completed | First-attempt |
|---|---:|---:|---:|
| External (chalk/click/cobra) | 24 | 24 | 23 |
| G2c-4 real-LLM synthetic smoke (not a policy test) | 2 | 2 | 2 |
| **Total (real-LLM evidence)** | **26** | **26** | **25** |

**Result.** Across 24 external cells, the agent produced a predicate-satisfying structural proposal in **24/24 (100%)** cases, with **23/24 on the first attempt**. The single two-attempt cell (click/Coupling/Accept/Without) succeeded on the second attempt after a syntactic rejection. This extends the G2c-4 synthetic smoke (2/2) to a small external corpus across three languages: given the structural context (focus node + current outgoing imports) and the `removed_edges` output contract, GPT-4o-mini reliably identifies and removes the correct import edge to satisfy the coupling/instability predicate on the first attempt. We frame this as preliminary external-corpus evidence rather than a generalization claim, given the corpus size (3 repos, 24 cells) and single-run design.

**Caveat (the dominant boundary).** These are *structural-proposal* successes: the agent produced a `DeltaProposal` whose `removed_edges`, when measured by the engine on a hypothetical graph, satisfied the predicate. This is *not* a source-code patch success. Translating a `removed_edges` delta into an actual code edit (removing an `import`/`use`/`require` statement and repairing downstream references) requires a code-generation step that is out of scope for this paper (§6.9, Future Work §10). We label this a **controlled structural harness** result throughout. A second boundary: the success rate is high partly because the tasks are *single-edge-removal* tasks; multi-step production refactors would exercise the maneuver limit and the feedback/policy mechanisms more heavily (RQ8, RQ9).

### 6.6 RQ8: Calibration Feedback

| Feedback mode | Completed | Mean tokens |
|---|---:|---:|
| With feedback | 12/12 | 1061 |
| Without feedback | 12/12 | 1148 |

**Result.** Success rate is identical (12/12) under both feedback modes. Token cost is marginally lower *with* feedback (1061 vs. 1148), but this difference is driven by the single two-attempt cell (click/Coupling/Accept/Without, 2303 tokens) falling in the Without arm, not by a systematic feedback effect. **It should not be interpreted as a stable cost advantage.** By the headline numbers, feedback is neutral on success and approximately neutral on cost under first-attempt-dominant conditions.

**Caveat (a worked honesty episode).** This neutral result deserves a precise reading, because it is easy to misread as "feedback does not help." The mechanism is subtler: (1) in 23 of 24 cells the LLM succeeded on the first attempt, so the feedback channel *never fired* — there was nothing to feed back; (2) in the one cell that needed a second attempt, the rejection was *syntactic* (a Q4 gate failure), which does push a calibration message, but that cell happened to be in the Without arm where the wrapper clears it. The deeper boundary, noted in §4.2, is that *numeric* predicate rejections (a valid proposal whose measured coupling still exceeds the threshold) produce no calibration message in the current implementation — so even in a multi-attempt numeric-near-miss scenario, the with- and without-feedback arms would receive the same (empty) feedback. We therefore report RQ8 as **neutral under first-attempt-dominant conditions**, with the mechanism's value demonstrated only in the synthetic multi-step fixture (RQ9 progression, where feedback-fixed mode was used). External-corpus validation of feedback's value requires scenarios that force multiple numeric-near-miss attempts — left to future work.

### 6.7 RQ9: Policy Accumulation

| Condition | Corpus | Outcome |
|---|---|---|
| StrictReject | Synthetic (maneuver_limit=3) | ExceededManeuverLimit, state frozen at 0.80 |
| AcceptImprovement | Synthetic (maneuver_limit=3) | Completed at attempt 2; state 0.80→0.75→0.667→0.50 |
| StrictReject | External (24 cells subset) | 13/13 Completed (first-attempt) |
| AcceptImprovement | External (24 cells subset) | 13/13 Completed (first-attempt) |

**Result (synthetic — mechanism).** On a controlled fixture (target node with 4 imports, coupling 0.80, three incremental edge-removal proposals), `AcceptImprovement` accumulates state to completion (coupling 0.80 → 0.75 → 0.667 → 0.50, predicate satisfied at attempt 2), while `StrictReject` freezes state at 0.80 after each rejection and exhausts the maneuver limit. Both consume the same three attempts; the difference is *whether bounded attempts suffice to complete*, not token cost. This is the mechanism RQ9 isolates: progress-checkpoint accumulation enables completion under bounded attempts where strict rejection cannot.

**Result (external — generality).** On the external corpus, both policies complete 13/13 cells — because the LLM satisfies the predicate on the first attempt, the policy lever is never exercised. The external result is therefore **neutral**, consistent with RQ8: first-attempt dominance suppresses the very mechanism the policy controls.

**Caveat.** The policy distinction is real (proven synthetically) but invisible under first-attempt success. Demonstrating its external value requires multi-step production refactor tasks where no single attempt satisfies the predicate — precisely the scenario our single-edge-removal tasks do not construct. We report this honestly: RQ9's mechanism is established; its external generalization is conditioned on task difficulty and remains open.

### 6.8 Multi-axis safety (axis regression)

| Measure | Count (24 external cells) |
|---|---:|
| Improved (loss decreased) | 12 |
| Same (loss unchanged) | 12 |
| Regressed (a non-target axis worsened beyond cap) | **0** |

**Result.** No external cell exhibited an axis regression: the LLM's coupling-reducing proposals never raised instability (or vice versa) beyond the regression cap. Twelve cells improved loss; twelve held it constant (the InstabilityReduction cells, where removing an outgoing import changes Ce but the loss expression against the preferred vector nets to near-zero change — a multi-axis arithmetic artifact, not a failure). This evidences that the regression guard (INV-T6) was not merely defined but *unnecessary* on this corpus — the LLM's structural proposals were axis-coherent. The guard's value would appear on tasks that tempt axis trade-offs, which this corpus's single-edge-removal design does not.

### 6.9 Threats to Validity (evaluation-scoped)

**Internal.** First-attempt dominance suppresses the RQ8 (feedback) and RQ9 (policy) signals; their mechanism is proven synthetically but not externally. The structural-context prompt (G2c-4) is tuned to make single-edge removal easy, which inflates first-attempt success.

**Construct.** Success is *structural-proposal* success (graph-level `removed_edges`), not source-patch success; the codegen step is out of scope. cobra's Space graph has 36 nodes but only 1 internal `Imports` edge (Go external-package imports like `github.com/spf13/...` are not resolved to Module–Module internal edges), so its coupling signal is weak — yet it still completed because the single edge sufficed. This reflects an analyzer limitation (package-graph resolution), not a navigator limitation.

**External.** Three repositories, 24 cells — trend/indication only, no statistical power. A single LLM (GPT-4o-mini) and a single run per cell; stochasticity and cross-model variation are unmeasured. Cohesion (y) is placeholder (no SCIP index on these repos); tasks avoid the cohesion axis, so this does not affect the gate but limits the axes exercised.

---

## 7. Related Work

### 7.1 Software Metrics and Architectural Quality

Software metrics have a long history [7, 5, 8]. Martin's Clean Architecture [6] introduced Instability (I), Abstractness (A), and the Main Sequence (A + I = 1) — directly informing OSP's coordinate axes. Paper 1 positioned these metrics in a navigable space with provenance; Paper 2 makes them *actionable as task predicates*; the question "is this module done?" becomes "does this module's measured coupling satisfy the predicate?", a measurement rather than a judgment. Fenton and Bieman [8] provide the measurement foundation OSP's `ProvenancedRawPosition` extends by carrying per-axis source into the predicate layer (INV-T4). Tempero and Ralph [7] argue existing metrics are insufficient for architectural decisions; trajectory navigation addresses the dynamic version of this — metrics become the *acceptance condition* for agent work, not merely a dashboard.

### 7.2 AI Coding Agents and Code Retrieval

SWE-agent [10] and similar autonomous agents navigate repositories through file-system actions guided by LLM reasoning, achieving high task-completion rates but providing no architectural safety net: an agent can violate a dependency rule or raise coupling beyond tolerance with no deterministic rejection. RepoCoder [9] uses iterative retrieval-and-generation, feeding outputs back as context; its context grows as unstructured text with no coordinate system, no provenance, and no gate. OSP trajectory navigation operates at a different layer — it does not replace the agent's file-navigation strategy but constrains the agent's *output* through the predicate gate (Q5.b) before any mutation reaches the project space. The adaptive control loop (§4) provides what these agents lack: a bounded, measured, feedback-driven path from proposal to accepted change, where "accepted" means a predicate was satisfied by an engine measurement, not that the agent stopped trying.

### 7.3 Graph RAG and Knowledge Graphs

GraphRAG [4] generates entity-relation graphs for LLM retrieval, improving context relevance over keyword search, but provides no enforcement — the graph is advisory, not a gate. Knowledge graphs [3] model structured relationships but are likewise representational. OSP's conceptual space is more structured (typed ontological nodes with gravity functions) and, critically, *actionable*: the predicate gate measures a hypothetical delta against the space and rejects violations before mutation. Where GraphRAG optimizes retrieval, OSP enforces architectural constraints; where a knowledge graph describes, OSP's trajectory navigates.

### 7.4 Architectural Conformance Checking

ArchUnit [11] and Software Reflexion Models [12] check conformance of implementation to intended architecture. ArchUnit enforces rules in tests (a rule violation fails a test); Reflexion Models reconcile a source model with an intended model, surfacing divergences. OSP's Q6 (Rule) gate is a conformance check, but trajectory navigation adds what these post-hoc checkers lack: a *pre-mutation* gate evaluated on a hypothetical measurement, so a rule-violating delta is rejected before it lands rather than flagged after the fact. The predicate gate (Q5.b) further generalizes conformance from binary rule satisfaction to measurable quality targets (coupling ≤ threshold), with typed outcomes (progress vs. completion vs. regression) that support incremental refactoring.

### 7.5 Byzantine Fault Tolerance and the Witness Model

Paper 1 showed that OSP's two-witness commit rule is a safety-refinement of authenticated BFT for f = 1 under explicit assumptions (Theorem 1, Paper 1 §5). Trajectory navigation inherits this for mainline commits: only an `AcceptAsCompleted` mutation — predicate-satisfied — reaches the mainline, and it is still subject to the witness quorum. The progress-checkpoint lane (INV-T8) is the dynamic addition: partial progress can accumulate in a *separate* lane without breaching the mainline's witness security. The `NavigatorWitnessPolicy` isolation (§4.3) ensures that the controlled-experiment relaxation (auto-approve) cannot leak into the production witness model.

### 7.6 OSP Paper 1 (Static → Dynamic)

This paper is the dynamic companion to Paper 1. Paper 1's static space answers "what is the architectural state, and how did it get there?" (coordinates, witnessing, commit-time gates). Paper 2 answers "how does an agent move from the current state toward a healthier one, safely?" (trajectory, predicates, adaptive loop). The two share the coordinate system, the MetricValue provenance, and the deterministic-gate philosophy; Paper 2's contribution is the task-as-predicate ontology (INV-T1) and the adaptive control loop that turns a static gate into a navigation mechanism. The combination — a static space with provenance-aware metrics, a dynamic trajectory with predicate-gated navigation — is what we term *Software Physics*: software treated as a conceptual space governed by measurable, enforceable physical rules.

---

## 8. Discussion

### 8.1 Task Dematerialization: From Coordinates to Predicates

The central conceptual move of this paper is *dematerializing the task*: expressing the goal as a measurement predicate rather than a target coordinate. This is not merely a representational choice; it changes the epistemics. A target coordinate given to an agent is a thing to be *imitated* — the agent can produce a delta that declares those coordinates (were it allowed to), collapsing measurement into belief. A predicate given to an agent is a condition to be *satisfied* — the agent must produce a structural change whose *engine-measured* consequence meets the condition, and it cannot observe the condition's ideal center (INV-T1). The agent is asked "reduce coupling below 0.55," not "move to (0.40, 0.70, …)"; the former is measurable, the latter is imitable. Dematerialization is what makes the engine, rather than the agent, the arbiter of "done."

### 8.2 Epistemic Projection as a Live Property

INV-T1 is not a design aspiration we argue for; it is a property we *verify live* (§6.3). The agent-facing response carries zero target-coordinate fields, enforced both type-level (the struct has no such field) and runtime (a leak scan blocks emission). This two-layer defense matters because the temptation to "just pass the target vector to help the agent" is constant in practice — it would make prompt engineering easier and short-term success rates higher. The invariant exists precisely to make that shortcut a compile error and a runtime block, not a judgment call. The `AgentStructuralContext` addition (G2c-4) tested the boundary: structural context (focus node + imports) is allowed, target coordinate is not, and a test pins the distinction so future prompt changes cannot silently erode it.

### 8.3 First-Attempt Dominance — A Worked Honesty Episode

The most instructive finding of the external evaluation is what it *did not* show. RQ8 (feedback) and RQ9 (policy) are the mechanism questions the paper set out to answer; the external corpus answered them as *neutral*, because the LLM succeeded on the first attempt in 23 of 24 cells, suppressing the very mechanisms those questions isolate. It would be tempting to either (a) report this as "feedback and policy don't matter," or (b) design harder tasks until they do. We do neither: we report the neutral result *with its mechanism explained* (first-attempt dominance; numeric-near-miss produces no feedback message), point to the synthetic experiments where the mechanisms *are* demonstrated (RQ9 progression), and flag the external validation of feedback/policy value as future work conditioned on task difficulty. The honesty is not incidental — it is the contribution. A framework that cannot report a neutral result clearly cannot be trusted to report a positive one.

### 8.4 Multi-axis Safety and the Regression Guard

The 0/24 axis-regression result (§6.8) is a case where an invariant (INV-T6) was *defined* and *unnecessary* on the evaluated corpus. The LLM's single-edge-removal proposals were axis-coherent: removing an outgoing import to reduce coupling did not raise instability beyond the cap. We report this not as "the guard works" (it was never triggered) but as "the LLM's structural proposals, under this prompt design, did not tempt axis trade-offs." The guard's value would appear on tasks that genuinely trade axes (e.g., extracting a module to reduce coupling while increasing inter-module instability), which the current task design does not construct. This is a precise scope claim: multi-axis safety is *enforced* (the guard exists and is tested); its *necessity* on external repos remains open.

### 8.5 Witness Policy Isolation

The `NavigatorWitnessPolicy` enum is a small type with a large lesson. The G2c-1 corpus run's 0/24 completions did not fail because the navigator loop was broken; they failed because an empty `WitnessSet` silently failed quorum on every attempt, and the empty evidence ledger hid this. Two things went wrong: the production witness threshold was silently active in an experiment, and the evidence ledger did not record rejections. The fixes — a *named, isolated* `HarnessAutoApprove` variant (so the relaxation cannot leak into production) and reject-evidence (so failures leave traces) — are the kind of engineering rigor that the "software physics" framing demands. Every evidence row now carries its `witness_mode`, so a reader can confirm that no production run used auto-approval, and that the production navigator retains Paper 1's two-witness security model.

---

## 9. Threats to Validity

**Internal validity.** The external-corpus results are dominated by first-attempt success, which suppresses the feedback (RQ8) and policy (RQ9) mechanisms; their value is demonstrated synthetically but not generalized externally. The structural-context prompt (G2c-4) is tuned to make single-edge removal straightforward, which inflates first-attempt success and thus the headline RQ7 rate. The synthetic RQ9 fixture is controlled but small (one topology, one target); its accumulation result holds for that fixture and is not a general proof.

**External validity.** The corpus comprises three repositories across three languages — sufficient to demonstrate cross-language structural-proposal generation but without statistical power. A single LLM (GPT-4o-mini) was used; cross-model variation (Claude, Gemini, local models) is unmeasured, and the prompt is tuned for the OpenAI tokenizer/format. Each cell is a single run; stochasticity and seed sensitivity are not characterized.

**Construct validity.** The central construct boundary: success is *structural-proposal* success (a `removed_edges` delta whose engine-measured consequence satisfies the predicate), **not** source-code patch success. The codegen step that would translate a graph-level delta into an actual `import`/`use`/`require` removal with downstream reference repair is out of scope. We label this a *controlled structural harness* throughout and do not claim production code-edit success. cobra's graph (36 nodes, 1 internal edge) reflects an analyzer limitation (Go external-package import resolution), not a navigator limitation. Cohesion (y) is placeholder on these repos (no SCIP index); tasks avoid the cohesion axis, so the gate is unaffected, but the evaluation does not exercise the cohesion predicate path.

**Ethics and privacy.** Agent trajectory evidence (attempts, rejections, calibration messages) may reveal sensitive architectural intent even when source code is omitted. Any telemetry based on trajectory logs requires redaction and consent, as noted in Paper 1 §10.

---

## 10. Future Work

1. **Structural delta → source patch (codegen).** The current pipeline stops at a predicate-satisfying graph-level `removed_edges` delta. Translating this into an actual code edit (removing an import statement and repairing downstream references) via tree-sitter edits or language-specific tooling (Roslyn, rust-analyzer) would close the gap between structural-proposal success and production code-edit success.
2. **Multi-step production refactor scenarios.** The first-attempt dominance that suppresses RQ8/RQ9 externally is a function of task design. Constructing tasks that require multiple attempts to satisfy (e.g., multi-module refactors where no single edge removal suffices) would exercise the feedback channel and the policy lever, enabling external validation of their value.
3. **Multi-LLM and multi-run benchmarking.** Cross-model variation (Claude, Gemini, local models) and stochasticity (multiple seeds per cell) would characterize robustness rather than reporting a single run.
4. **Numeric-near-miss feedback.** The current feedback channel is silent on numeric predicate rejections (a valid proposal whose measured coupling still exceeds the threshold). Feeding back the measured value and the gap ("coupling is 0.62, threshold 0.55") would activate the feedback channel in multi-attempt scenarios and could change the RQ8 result.
5. **Corpus expansion.** Extending beyond three repositories to a statistically powered corpus (e.g., the Paper 1 23-repository set) would strengthen external-validity claims and enable cross-language success-rate comparison.
6. **Prompt unification (D5).** The `osp-llm-runtime` currently retains a prompt debt (a `complete_raw` shortcut vs. the unified `OspPrompt.task_view`). Unifying these would simplify the trusted computing base of the prompt path.
7. **Axis-oscillation tasks.** Constructing tasks that tempt axis trade-offs (coupling↓ at instability↑) would externally exercise the regression guard (INV-T6), which the current corpus leaves untriggered.

---

## 11. Conclusion

We presented Architectural Trajectory Navigation, the dynamic extension of the Ontological Space Protocol, in which an AI agent's task is a measurement predicate rather than a target coordinate. The agent never observes the coordinate it is supposed to reach (INV-T1, verified live); a deterministic predicate gate (Q5.b) measures the agent's structural delta against the predicate before any mutation; an adaptive control loop bounds attempts and feeds rejections back as calibration. Eight type-level invariants (INV-T1–T8) govern the ontology, extending Paper 1's static guarantees into the goal-directed regime.

On the external corpus (chalk/JavaScript, click/Python, cobra/Go), GPT-4o-mini produced predicate-satisfying structural proposals in **24/24 cells** at a mean cost of 1104 tokens per cell, with 0 axis regressions. Including two real-LLM synthetic-smoke cells, the real-LLM evidence totals **26/26 Completed**. We report this as preliminary evidence that the structural-proposal pipeline works on small external repositories under a controlled structural harness, while being explicit about two boundaries: success is structural (graph-level `removed_edges`, not source patches), and first-attempt dominance suppresses the feedback and policy mechanisms whose value is demonstrated synthetically. The neutral external results for RQ8 and RQ9 are reported with their mechanism explained rather than suppressed — the honesty is the contribution.

Together with Paper 1's static space, trajectory navigation provides the dynamic counterpart required by the *Software Physics* framing: software modeled as a conceptual space with provenance-aware metrics (Paper 1), navigated by agents whose tasks are measurable conditions evaluated by a deterministic engine (Paper 2). Together, these results motivate a broader Software Physics program rather than completing it — the framing is supported by one implementation path and a small external corpus, not a general proof. The central separation — between the agent that proposes and the engine that measures — is the epistemic core of the work.

---

## Appendix A: G2c-5 External Corpus Dataset

**Per-repository graph statistics** (from `analyze_repo_with_config`, `AdapterRegistry::default_all()`):

| repo | language | commit | files analyzed | nodes | edges (internal Imports) | target node (role) | target coupling score |
|---|---|---|---:|---:|---:|---|---:|
| chalk | JavaScript | `aa06bb5` | 19 | 13 | 11 | Utility | 0.500 |
| click | Python | `6ec99f8` | 63 | 63 | 40 | Support | 0.800 |
| cobra | Go | `ad460ea` | 36 | 36 | **1** | Support | 0.500 |

cobra's single internal `Imports` edge reflects an analyzer limitation, not a property of the repository: Go external-package imports (`github.com/spf13/...`) are not resolved to Module–Module internal edges by the current tree-sitter import resolver. cobra nevertheless completed all 8 cells because the single resolved edge sufficed to move the target node's coupling below the threshold. This is listed as a construct-validity threat (§6.9).

**Full 24-cell dataset.** All cells: real GPT-4o-mini, witness_mode = harness_auto_approve, maneuver_limit = 5. Source: `docs/paper2-notes/evidence/g2c-external-corpus-20260702.json` (rows where `corpus_kind = "external-repo"`).

| repo | lang | task | policy | feedback | attempts | tokens | completed | loss before→after |
|---|---|---|---|---|---:|---:|---|---|
| chalk | js | Coupling | Strict | with | 1 | 1039 | ✅ | 0.610→0.568 |
| chalk | js | Coupling | Strict | w/o | 1 | 1007 | ✅ | 0.610→0.568 |
| chalk | js | Coupling | Accept | with | 1 | 1034 | ✅ | 0.610→0.568 |
| chalk | js | Coupling | Accept | w/o | 1 | 1030 | ✅ | 0.610→0.568 |
| chalk | js | Instability | Strict | with | 1 | 1050 | ✅ | 0.424→0.424 |
| chalk | js | Instability | Strict | w/o | 1 | 1041 | ✅ | 0.424→0.424 |
| chalk | js | Instability | Accept | with | 1 | 1042 | ✅ | 0.424→0.424 |
| chalk | js | Instability | Accept | w/o | 1 | 1037 | ✅ | 0.424→0.424 |
| click | py | Coupling | Strict | with | 1 | 1166 | ✅ | 0.658→0.568 |
| click | py | Coupling | Strict | w/o | 1 | 1175 | ✅ | 0.658→0.568 |
| click | py | Coupling | Accept | with | 1 | 1167 | ✅ | 0.658→0.568 |
| click | py | Coupling | Accept | w/o | 2 | 2303 | ✅ | 0.658→0.568 |
| click | py | Instability | Strict | with | 1 | 1052 | ✅ | 0.424→0.424 |
| click | py | Instability | Strict | w/o | 1 | 1041 | ✅ | 0.424→0.424 |
| click | py | Instability | Accept | with | 1 | 1051 | ✅ | 0.424→0.424 |
| click | py | Instability | Accept | w/o | 1 | 1025 | ✅ | 0.424→0.424 |
| cobra | go | Coupling | Strict | with | 1 | 1021 | ✅ | 0.610→0.568 |
| cobra | go | Coupling | Strict | w/o | 1 | 1020 | ✅ | 0.610→0.568 |
| cobra | go | Coupling | Accept | with | 1 | 1028 | ✅ | 0.610→0.568 |
| cobra | go | Coupling | Accept | w/o | 1 | 1028 | ✅ | 0.610→0.568 |
| cobra | go | Instability | Strict | with | 1 | 1038 | ✅ | 0.424→0.424 |
| cobra | go | Instability | Strict | w/o | 1 | 1038 | ✅ | 0.424→0.424 |
| cobra | go | Instability | Accept | with | 1 | 1039 | ✅ | 0.424→0.424 |
| cobra | go | Instability | Accept | w/o | 1 | 1031 | ✅ | 0.424→0.424 |

---

## Appendix B: G2c-4 Real-LLM Synthetic Smoke

2 cells. Real GPT-4o-mini on the 5-node synthetic fixture (same topology as Appendix C, but with the real LLM rather than scripted proposals). Purpose: feasibility — does the real LLM emit a valid `removed_edges` proposal given the structural-context prompt? *Not* a policy test. Source: `docs/paper2-notes/evidence/g2c-external-corpus-20260702.json` (rows where `corpus_kind = "synthetic-controlled-fixture"`).

| stratum | task | policy | feedback | attempts | tokens | completed | gate_decision |
|---|---|---|---|---:|---:|---|---|
| G2c-4 smoke | Coupling | StrictReject | fixed_with | 1 | 1162 | ✅ | PassedAll |
| G2c-4 smoke | Coupling | AcceptImprovement | fixed_with | 1 | 1179 | ✅ | PassedAll |

**Reading.** Under the real LLM, both policies complete on the first attempt (the LLM removes enough edges in one proposal to satisfy coupling ≤ 0.55). This is why the external corpus (Appendix A) and this smoke test report RQ9 as *neutral*: first-attempt success leaves no room for the policy to differentiate. The policy *mechanism* is isolated only under scripted proposals that force multi-step accumulation (Appendix C).

---

## Appendix C: G2c-3 Controlled RQ9 Policy Fixture

The policy-accumulation mechanism (RQ9) is isolated on a controlled synthetic fixture with **scripted (mock) proposals**, to remove LLM stochasticity and isolate the policy variable. Source: `docs/paper2-notes/evidence/g2c-harness-matrix-mock.json` (G2c-3 rows).

**Fixture.** Five nodes (Node 0 = target Module, Nodes 1–4 = dependencies). Node 0 imports Nodes 1–4 (4 outgoing `Imports` edges → coupling = 4/5 = 0.80). Node 1 imports Node 0 (balancing instability). Vision/preferred vector targets coupling ≤ 0.55. Three incremental scripted proposals, each removing one import edge (Node 0 → Node 1, then → Node 2, then → Node 3).

| policy | attempts | outcome | completed | final coupling |
|---|---:|---|---|---:|
| StrictReject | 3 | ExceededManeuverLimit | ❌ | 0.80 (frozen) |
| AcceptImprovement | 3 | Completed | ✅ | 0.50 |

**StrictReject (maneuver_limit = 3).** Each attempt is rejected (coupling still > 0.55 after removing one edge: 0.75, then 0.667, then 0.50 — but state is *frozen* at 0.80 because rejections do not advance the trajectory). Outcome: `ExceededManeuverLimit`, completed = false.

**AcceptImprovement (maneuver_limit = 3).** Each attempt is accepted as a progress checkpoint (loss decreased): state advances 0.80 → 0.75 → 0.667. At the third attempt (removing the third edge), coupling reaches 0.50 ≤ 0.55; predicate satisfied; outcome `Completed`. Note: completion occurs because state accumulated, not because the third proposal alone would have satisfied the predicate from the initial state.

**Interpretation.** Both conditions consume the same three attempts and roughly the same tokens. The difference is *structural*: `AcceptImprovement` allows bounded incremental refactors to accumulate toward a predicate that no single step satisfies; `StrictReject` demands each step satisfy the predicate independently or makes no progress. This is the mechanism RQ9 isolates; its external generalization requires multi-step tasks that the single-edge-removal external corpus does not construct (§6.9).

---

## References

[1] Lamport, L., Shostak, R., & Pease, M. (1982). The Byzantine Generals Problem. *ACM TOPLAS* 4(3).

[2] Dolev, D. & Strong, H.R. (1983). Authenticated algorithms for Byzantine agreement. *SIAM J. Comput.* 12(4).

[3] Hogan, A. et al. (2021). Knowledge Graphs. *ACM Computing Surveys* 54(4). arXiv:2003.02320.

[4] Edge, D. et al. (2024). From Local to Global: A Graph RAG Approach. arXiv:2404.16130.

[5] McCabe, T. (1976). A Complexity Measure. *IEEE TSE*.

[6] Martin, R.C. (2017). *Clean Architecture*. Pearson.

[7] Tempero, E. & Ralph, P. (2026). Making Software Metrics Useful. arXiv:2603.16012.

[8] Fenton, N.E. & Bieman, J. (2014). *Software Metrics: A Rigorous and Practical Approach* (3rd ed.). CRC.

[9] Zhang, F., et al. (2023). RepoCoder: Repository-Level Code Completion Through Iterative Retrieval and Generation. EMNLP.

[10] Yang, J., et al. (2024). SWE-agent: Agent-Computer Interactions Enable Software Engineering Language Models. arXiv:2405.15793.

[11] Muschevici, R., Clarke, D. & Proenca, J. (2018). Architectural Conformance Checking with ArchUnit. *IEEE Software* 35(5).

[12] Murphy, G.C., Notkin, D. & Sullivan, K.J. (2001). Software Reflexion Models: Bridging the Gap between Design and Implementation. *IEEE TSE* 27(4).

**Companion:** *Ontological Space Protocol: Modeling Software as a Conceptual Space with Epistemological Witnessing* (Paper 1, v2.6) — provides the static-space foundation (coordinate system, MetricValue provenance, witness model, Q4–Q6 gates) and the BFT/quorum analysis (Theorem 1) that this paper extends into the dynamic, agent-driven regime.

---

*Draft v1. Evidence: `docs/paper2-notes/evidence/`. Implementation: `crates/osp-core`, `crates/osp-llm-runtime`, `crates/osp-analyzer`, `crates/osp-mcp`. Corpus runner: `crates/osp-analyzer/examples/g2c_corpus_matrix.rs`. All repositories cloneable via `scripts/clone-corpus.ps1`.*
