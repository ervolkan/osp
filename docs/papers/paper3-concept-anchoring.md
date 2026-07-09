# Concept Anchoring: From Human Sentences to Bound Project Work

---

## Abstract

A human sentence may introduce project intent, but it does not by itself create project work. Contemporary embedding-first anchoring — retrieval-augmented generation, GraphRAG, and their kin — can propose *candidate proximity* between a sentence and a concept, but it cannot decide the sentence's ontological role, the authority behind it, whether it has been accepted, or whether it is executable. Proximity is a property of vectors; commitment is a property of protocol acts, and the two cannot substitute for one another.

We present **Concept Anchoring**, the genesis layer of the Ontological Space Protocol (OSP), which turns a human sentence into bound, accepted, measured project work only through a type- and protocol-enforced binding chain: candidate isolation, operator acceptance, predicate lowering, cross-family translation, operator binding, and capability-gated task genesis. Each transition is a gate with a named invariant, enforced at the type boundary, the constructor boundary, or the regression-test boundary. The admissibility and effects of each transition are protocol-enforced; substantive operator decisions remain explicit human judgments. A rule lowers to a `PredicateStub` (structured uncertainty), never to an executable predicate; a translator proposes candidate axis meaning, but only operator binding creates commitment; a task is born only when an accepted candidate, a bound predicate set, and an operator capability meet.

We verify the chain rather than benchmark a semantic extractor. We verify 15 Paper-3-specific invariants: thirteen are type-enforced and two are runtime-asserted. The evidence includes structural type and constructor boundaries, transition tests, and 24 cumulative workspace compile-fail tests, two of which exercise Paper 2's inherited task-genesis boundary. Across 13 golden fixtures and 5 held-out adversarial sentences, we report a five-state conformance: 12 conform, 2 partial, 2 known limitation, 2 reject-as-expected, 0 unexpected failure. An end-to-end binding-chain replay exercises the real sentence-to-`RuleCandidate` pipeline and carries the lowered predicate through binding and task genesis, with the Candidate→Accepted promotion performed programmatically by a review-session API under fresh-basis and no-decision-without-record constraints; six rejected-path replays prove the gates refuse invalid input. A gate that only passes is indistinguishable from no gate.

This is a verification paper, not a semantic-extraction benchmark. The deterministic keyword matcher that drives translation is a placeholder for concept-synthesis work, and it admits a known lexical false-positive (a software *coupling* vs. a pipe *coupling*); the golden and held-out fixtures are self-authored, and the review session is exercised through its programmatic API rather than an interactive console. The matcher is not the contribution; the binding protocol is.

## 1. Introduction

This paper is part of a broader OSP series, but its claim is self-contained: it verifies the binding chain from human sentence to reviewed, bound project work. Its companions establish the layers this chain feeds. Paper 1 established OSP's *static* conceptual space — a coordinate system that positions modules, advances project time only through witnessed commits, and rejects vision- or rule-violating claims through deterministic gates. Paper 2 extended OSP to the *dynamic* regime: an agent navigates from the current space toward a healthier one under measurement predicates rather than target coordinates, with an adaptive control loop and a deterministic predicate gate. Both papers, however, take the existence of structured work as given. They answer *what is the architectural state, and how does an agent move it* — but they do not answer the question that precedes both: *how does a human sentence become the bound, accepted, measured work that the static space witnesses and the dynamic navigator runs?* This paper, the genesis layer, answers that question. Where the static space and the dynamic navigator assume structured work exists, the binding chain is the protocol act that produces it.

### 1.1 The Problem

An agent or a stakeholder types a sentence — *"coupling between modules must not exceed a threshold,"* or *"the payment module reflects the trust vision."* An embedding-first system can find sentences that are *close* to this one and retrieve related concepts, but proximity cannot decide the sentence's ontological role. Is it a rule, a vision, a task, or an assumption? Does it carry authority? Has it been accepted into the project's mainline knowledge? Is it executable? Each of these is a *commitment* — a protocol act that changes the project's reality — and commitment is not a function of vector distance.

### 1.2 Contributions

This paper makes four contributions:

1. **Genesis ontology with candidate isolation (Section 3).** A human sentence produces a *candidate* — never accepted, never executable — governed by ten type-enforced invariants (INV-C1–C8, C12, C13), plus two runtime-asserted invariants (INV-C14 acceptance-provenance projection, INV-C15 atomic supersession) introduced by the supersede vocabulary.
2. **Predicate lowering — a rule is not a predicate (Section 4).** A `RuleCandidate` lowers to a `PredicateStub` (structured uncertainty), never an `ExecutablePredicateSet` (INV-P1).
3. **Cross-family translation semantics (Section 5).** Concept→Physical translation preserves candidate meaning (INV-P3); binding alone creates commitment.
4. **Three-gate task genesis (Section 6).** Accepted intent + operator-bound predicate + operator capability → `trajectory::Task` (INV-P2; at the task-genesis boundary the chain additionally relies on Paper 2's INV-T2).

### 1.3 Terminology mapping

| Term (this paper) | OSP core type | Connection |
|---|---|---|
| Candidate | `ConceptNode` (`DecisionStatus::Candidate`) | pre-Trajectory |
| Accepted | `ConceptNode` (`DecisionStatus::Accepted`) | pre-Task intent |
| Task | `trajectory::Task` | navigator input (Paper 2) |

## 2. Motivating Example

> *"The sentence never becomes a task by itself."*

To make the protocol concrete, we walk a single human sentence through the entire binding chain, from text to a navigator-ready `trajectory::Task`. Every step below is reproduced by a frozen evidence artifact; the prose follows the artifact's step numbering exactly. The sentence is deliberately ordinary — an architectural rule a senior engineer might utter in a design review:

> *"Coupling must not exceed module threshold."*

Nothing in this sentence is executable. It carries no metric identifier, no threshold, no scope, no operator signature. The chain that turns it into work has eight steps, and each step is a gate.

**Step 1 — Pipeline run (real, not seeded).** The sentence enters the deterministic pipeline as a `ConceptPacket`. A rule-based classifier detects the *"must not"* marker and sets a rule signal; the extractor emits a `DerivesRule` candidate whose target is a freshly named `RuleCandidate:CouplingMustNot` (the name is derived deterministically from the first three words of the sentence). The pipeline applies its plan to the concept graph, and the gate writes the new node with `DecisionStatus::Candidate` (INV-C5). At this point the sentence has produced a *candidate rule* — nothing more. It is not yet a predicate, not yet accepted, not yet a task.

**Step 2 — Candidate isolation (INV-C3).** The `RuleCandidate` now lives in the concept graph, but in the *candidate lane*, segregated from mainline knowledge. No query that reads the project's accepted state can see it. The sentence has a name and a place, but no authority.

**Step 3 — Predicate lowering (INV-P1).** The candidate rule is lowered into a `PredicateStub` — a structured-uncertainty value that records, in type, *what is missing*: four unresolved slots (metric, threshold, scope, comparator), a suggested template (`MetricThreshold`), and zero completeness. The lowering never produces an `ExecutablePredicateSet`; a rule and a predicate are different ontological categories, and lowering names the gap between them rather than papering over it.

**Step 4 — Cross-family translation (INV-P3).** The stub carries a `CrossFamilyHint` that translates the conceptual intent into the physical-code family. Scanning the candidate's canonical form, the translator finds the *coupling* keyword and proposes a single axis candidate (`SingleCandidate(Coupling)`) with `KeywordMatch` source. This is the only step where the sentence's wording influences the physical interpretation, and even here the output is a *proposal of candidate meaning*, not a commitment. The full argument for why this proposal cannot silently harden into certainty occupies Section 5.

**Step 5 — Operator binding (INV-P2).** An operator — holding `OperatorCapability` — binds the stub's unresolved slots: axis `Coupling`, scope `Node(1)`, comparator `Le`, threshold `0.55`. The capability token plus the function boundary of `bind_metric_threshold` together form the only route from a `PredicateStub` to an `ExecutablePredicateSet`. The keyword hint suggested the axis; the operator decided it.

**Step 6 — Accepted verification (INV-C3, real promotion via `OperatorReviewSession`).** The chain now needs an *accepted* intent to bind the predicate to. An `OperatorReviewSession` is opened for an operator, a `PresentedBasis` is compiled from the candidate's current content, and the session's `accept` consumes an internal `OperatorAcceptance` token to perform the Candidate→Accepted promotion under INV-C12 (the basis's `NodeDigest` is re-checked against the candidate's current content to refuse stale-basis TOCTOU) and INV-C13 (the promotion and its `DecisionRecord` ledger append happen atomically in `apply_decision`). The promotion is real, not seeded; the audit trail is the record.

**Step 7 — Task genesis (INV-T2).** With an accepted reference and a bound `ExecutablePredicateSet`, a third capability-gated call — `create_task_from_accepted_candidate` — finally produces a `trajectory::Task`: a deterministic ID derived from the candidate, a `Pending` status, the bound predicate set, and an operator-supplied `allowed_operations` list (`RemoveImport`). The sentence has, at last, become executable work — but work that a Paper 2 navigator can run, not a free-floating instruction.

**Step 8 — Registry.** The task enters the in-memory registry and resolves as navigator-compatible. From here, Paper 2's `AgentNavigator.run_task` can execute it under measurement predicates, maneuver limits, and witness policies. The genesis layer's job is done.

At no point did the sentence become a task by proximity; only type-enforced gates moved it forward. The sentence's wording influenced exactly one step (Step 4, translation), and even there it produced a candidate, not a commitment. And every transition that created authority or executability — acceptance, binding, task genesis — required a token the sentence could not supply; the remaining transitions (pipeline run, candidate isolation, lowering, registry) moved the sentence through state without granting it any.

> **Evidence.** The full eight-step chain — including the real Step 1 pipeline trace, the cross-checked canonical, the deterministic task ID, and the registry resolution — is reproduced in `e2e-binding-chain-replay.json` [18] (Appendix B; integrity recorded in `run-metadata.json`). The chain's *rejected* counterpaths are reproduced in `e2e-rejected-paths-replay.json` [18] (Section 7.5).

## 3. Genesis Ontology

The genesis layer rests on ten type-enforced invariants (INV-C1–C8, C12, C13) that govern how a human sentence enters the concept graph, plus two runtime-asserted invariants (INV-C14, acceptance-provenance projection; INV-C15, atomic supersession transition) that do not belong to the type-enforced set. Each type-enforced invariant names a failure mode that the protocol refuses to allow, and each is enforced somewhere in the type system rather than left to caller discipline. We distinguish two enforcement strengths, and the distinction matters for a verification paper: a *structurally impossible* violation cannot be expressed in code at all, while a *regression-test boundary* violation can be expressed but is caught by a compile-fail test (`trybuild`) before it reaches `main`. Table 1 summarizes the ten type-enforced invariants; INV-C14 and INV-C15 are documented separately below the table because they are enforced by runtime projection/transition tests rather than by type shape.

**Table 1 — Genesis ontology invariants (INV-C1–C8, C12, C13).**

| INV | Name (maxim) | What it prevents | Type-level enforcement |
|---|---|---|---|
| **C1** | *Embedding proposes, never decides* | An embedding vector becoming a node's ontological position; the scorer deciding from raw vectors instead of the hybrid score + threshold + operator approval | Sealed `embedding` module with private inner vector; `AnchorScorer` takes a `ScalarSimilarity(f64)` newtype, never a vector. **Structurally impossible** — no dedicated trybuild; the type shape is the enforcement. |
| **C2** | *Position families do not mix* | Concept, physical-code, and evidence vectors collapsing into one ℝⁿ; conceptual coupling values leaking into physical-code space | Three distinct concrete types (`PhysicalCodeVector`, `ConceptualIntentVector`, `EvidenceVector`) under a `PositionVector` enum whose `family()` is derived from the variant, never a stored field. Compile-fail test: `c2_family_incompatible`. |
| **C3** | *Candidate isolation* | Anchor-produced candidates polluting project-reality mainline; a `TaskCandidate` being run directly by the navigator | `OperatorAcceptance` capability token with a private field, kept `pub(crate)`; promotion is reached through `OperatorReviewSession` , a public type whose `accept`/`reject` consume the token internally. INV-C3 moved from pure structural unrepresentability to a *trusted protocol boundary*: untrusted callers still cannot mint `OperatorAcceptance` directly, but operator-session code can perform reviewed promotion, with INV-C12 (fresh basis for reviewed transitions) and INV-C13 (no decision without record) as audit constraints. Compile-fail tests: `c3_operator_acceptance_construct`, `c3_graph_private`, `c3_conceptgraph_deserialize`. |
| **C4** | *Supersede requires authority* | A weak-authority source (agent, raw embedding) overriding an Accepted decision via `SUPERSEDES` | `SupersedeAuthority` capability token with a private field and `pub(crate)` constructors; the gate emits `SupersedeAuthorityRequired` when authority is absent. The store-level supersede path — `apply_supersede`, which atomically transitions Accepted→`SupersededAccepted` and creates the successor edge (`successor --Supersedes--> superseded`) — is established in the evaluated implementation (INV-C15). `SupersedeSession` is the public trusted-boundary entrypoint for supersession, parallel to `OperatorReviewSession` for promotion. It mints `SupersedeAuthority` internally through a crate-private constructor and creates the opaque `SupersedeApplication`; the capability never leaves the session, giving INV-C4 the same trusted-boundary structure as INV-C3. Compile-fail test: `c4_supersede_authority_construct`. |
| **C5** | *Inferred is not accepted* | An LLM or extractor writing its own output directly as `Accepted`; derivation being treated as acceptance | `apply_plan` is the only **untrusted** write path, and it unconditionally writes `DecisionStatus::Candidate` for every new node and edge; promotion requires the `OperatorAcceptance` token (the C3 gate). Reviewed trusted-boundary paths (`apply_decision`, `apply_supersede`) are audit-recorded exceptions that write non-Candidate status under operator authority — structurally enforced for the untrusted path; no dedicated trybuild. |
| **C6** | *Code-derived intent is hypothesis* | An observed metric (*"coupling 0.82"*) being conflated with an inferred intent interpretation (*"this module reflects payment vision"*) at the same epistemic level | `ObservedCodeEvidence` private fields with a public smart constructor; `ObservedCodeMetricSource` typed enum excludes placeholder sources; `EvidenceStrength` newtype in `[0,1]`; serialize-only (no `Deserialize`). Compile-fail tests: `c6_observed_evidence_literal`, `c6_observed_evidence_deserialize`, `c6_intent_carries_physical_vector`. |
| **C7** | *High-stake edges are explainable* | A high-stake edge (`DerivesRule`, `DerivesTask`, `Supersedes`, …) entering the graph without justification | `NonEmptyExplanation` newtype whose fallible `new()` rejects empty or whitespace strings; the runtime gate emits `MissingExplanation` for the ten high-stake edge kinds. Scope deliberately narrowed from "every edge" to high-stake kinds. **Structurally impossible at construction** — the newtype makes empty explanations unrepresentable; the runtime gate covers the high-stake subset. |
| **C8** | *Concept identity is canonicalized* | The weak-anchor `CreateNode` band spawning `Payment / Payments / Ödeme / SecurePayment` duplicates | `AnchorPlan` fields are `pub(crate)` and serialize-only; a three-layer canon gate (exact canonical, glossary alias, Levenshtein ≤ 2) emits a `CanonicalRedirect` rather than creating a duplicate. Compile-fail tests: `c8_anchorplan_literal`, `c8_anchorplan_deserialize`. |
| **C12** | *Fresh basis for reviewed transitions*  | A reviewed transition authorized on the basis of a view that no longer matches the node's current content (TOCTOU); a fabricated basis | No reviewed acceptance, rejection, or supersession transition may be authorized from a stale or fabricated presented basis. `PresentedBasis` is compile-only from the store (`PresentedBasis::compile`), serialize-only (no `Deserialize`), and carries a `NodeDigest` over the candidate's content (excluding `decision_status`); `OperatorReviewSession::accept` re-reads the digest at decision time and rejects with `StaleBasis` on mismatch. `PresentedSupersedeBasis` binds supersession to the current digests of *both* the superseded and successor nodes; `SupersedeSession::supersede` re-reads those digests immediately before applying the transition. The reference implementation provides *minimal* fresh-basis verification: the presented basis identifies the node(s) and proves freshness. Rich evidence summaries, high-stake explanations, and UI presentation semantics are future integration work. Compile-fail tests: `c12_presented_basis_literal`, `c12_presented_basis_deserialize`. |
| **C13** | *No reviewed operator decision without record*  | An Accepted or Rejected transition reaching the graph without a corresponding ledger entry; a promoted node whose provenance cannot be audited | `DecisionApplication` is opaque (private ctor, no `Deserialize`); `AnchorStore::apply_decision` performs the status transition and the `DecisionRecord` append in a single call. INV-C13 is normative for reviewed operator decisions; in v1 it is verified atomic in the reference in-memory store, and future graph backends must provide a transactional `apply_decision`. The legacy `promote_to_accepted` path was removed before the evaluated release; promotion is now reached exclusively through `OperatorReviewSession` + `apply_decision`, and `seed_trusted` remains a bootstrap-only trusted-boundary exception that writes no ledger record. Compile-fail tests: `c13_decision_application_literal`, `c13_decision_application_deserialize`. |

**On the two enforcement strengths.** Seven of the ten type-enforced genesis invariants (C2, C3, C4, C6, C8, C12, C13) carry dedicated compile-fail tests; the remaining three (C1, C5, C7) are enforced by type shape alone — a sealed module, an unconditional write path, and a non-empty newtype respectively. We list this honestly because a reviewer who counts trybuild tests will find fewer than the invariant count suggests, and the discrepancy is not an oversight: it reflects the protocol's preference for making violations *unrepresentable* where possible and *compile-failed* where not. Together with the three type-enforced lowering/translation invariants (INV-P1–P3, Sections 4–5), INV-C14 (a runtime-asserted projection invariant), and INV-C15 (a runtime-asserted atomic transition invariant), these form the 15 Paper-3-specific invariants — 13 type-enforced (10 genesis + 3 lowering) plus two runtime-asserted; at the task-genesis boundary (Section 6) the chain additionally relies on Paper 2's INV-T2.

**INV-C14 — Acceptance-provenance projection (runtime/test-asserted, not type-level).** The ten invariants above are all type-enforced; INV-C14 is a runtime projection invariant of the Paper-3 set. It states: `mainline_query()` (the agent-facing, current non-superseded accepted projection) is a subset of `mainline_history()` (the acceptance-provenance projection), where `mainline_history() = {n | n.status ∈ {Accepted, SupersededAccepted}}` and `mainline_query() ∩ {SupersededAccepted} = ∅`. This invariant exists because the supersede vocabulary introduces `SupersededAccepted`, a terminal acceptance-lane status for a node that was accepted and later replaced: it retains accepted provenance and remains visible to audit and trajectory analysis, while chronological replay requires the decision/event ledger. The separation prevents superseded decisions from appearing in the current projection; it does not by itself guarantee query ordering or uniqueness of the active decision per concept. It is enforced by a projection-matrix test over the full status space and an exact-set store test, not by a compile-fail test; the separation keeps current and historical acceptance semantics unambiguous.

**INV-C15 — Atomic supersession transition (runtime/test-asserted, not type-level).** INV-C15 governs the store-level producer of `SupersededAccepted`: `apply_supersede(superseded, successor)` may succeed only if both endpoints are currently Accepted, differ from each other, have compatible structural endpoints (same kind + position family, a coarse gate whose semantic replacement judgment lives in the operator-reviewed basis), have no existing *committed* incoming `Supersedes` edge on `superseded`, and introduce no cycle. On success it atomically transitions `superseded` to `SupersededAccepted` and inserts exactly one committed `Supersedes` edge `successor → superseded`; on a returned `Err` the graph, ledgers, and audit sequence remain unchanged. Cardinality is *incoming*: every node transitioned by `apply_supersede` has exactly one committed incoming `Supersedes` edge. The edge is *lane-sensitive*: Candidate `Supersedes` edges (written by `apply_plan` as proposal provenance) do not participate in cardinality or cycle detection; only committed (Accepted) `Supersedes` edges do. Consolidation — one successor superseding multiple old nodes — is permitted (no outgoing-cardinality bound), so a successor may merge several prior decisions into one replacement lineage. Production invocation is `SupersedeSession`, which mints `SupersedeAuthority` internally (crate-private issuer) and creates the opaque `SupersedeApplication` that `apply_supersede` consumes; the Candidate proposal edge is preserved as historical proposal provenance (lane-sensitive separation, not replacement). The successor-edge invariant is established for transition-generated nodes; the representable-but-not-transitioned-node gap (seed/deserialization) is closed for snapshots restored through the evaluated `AnchorStoreSnapshot` path, whose `restore_snapshot` validation triangulates node status, committed supersession edges, and ledger records (incoming-edge cardinality, pair correspondence, cycle absence, dense audit sequence). Alternate persistence backends must implement equivalent validation. INV-C15 is enforced by an error-path matrix over twelve failure paths (spanning eleven `StoreError` variants, since `NodeNotFound` is exercised for each endpoint) plus chain, consolidation, and exhaustion tests; every error path asserts that the graph, both ledgers, and the audit sequence remain unchanged.

## 4. Predicate Lowering (INV-P1)

> *"A rule is not a predicate. A predicate is a rule whose measurable slots have been bound."*

A `RuleCandidate` is an ontological assertion of type *this property should hold*. An executable predicate is an ontological assertion of type *this measurement, against this metric, with this comparator, at this threshold, in this scope*. These are different categories, and lowering names the gap between them rather than papering over it.

The lowering function `lower_rule_to_predicate_stub` takes a `RuleCandidate` and produces a `PredicateStub` — a structured-uncertainty value that records, in type, *what is missing* for the rule to become a predicate. The stub carries four unresolved slots (`Metric`, `Threshold`, `Scope`, `Comparator`), a non-empty list of suggested templates (`MetricThreshold`, `MetricDelta`, `EvidenceRequired`, `RelationExists`), and (from Section 5) a `CrossFamilyHint` that proposes which physical-code axis the rule is *about*. Its `completeness` is the ratio of resolved slots; an untouched `MetricThreshold` stub has completeness `0.0`, not because it is empty but because zero of its four slots are bound.

Two structural properties make the stub honest. First, it cannot be empty: a stub whose `unresolved_slots` is empty while its reason is not `NoTemplateMatch` is a contradiction, and so is a stub whose reason *is* `NoTemplateMatch` while its `suggested_templates` is non-empty. Both are rejected at construction by `PredicateStubError`, so a caller cannot construct a stub that lies about its own state. Second, the stub is serialize-only: it carries a `#[derive(Serialize)]` for audit but no `Deserialize`, so a serialized stub cannot be read back into the graph to bypass the lowering function. The same serde-boundary pattern protects `ExecutablePredicateSet`, `CrossFamilyHint`, and `AnchorPlan`.

The lowering *never* produces an `ExecutablePredicateSet`. That type's only constructor is `bind_metric_threshold` (Section 6, Gate 2), and the function requires an `OperatorCapability` token the lowering does not hold. This is INV-P1's negative claim — not merely "the lowering happens to return a stub" but "the lowering has no path, internal to itself, that reaches an executable predicate." The full epistemic argument for why the stub's *candidate* content cannot silently harden into *commitment* occupies Section 5.

## 5. Cross-Family Translation

> *"Translation preserves candidate meaning; binding alone creates commitment."*

The chain in Section 2 turns on a single epistemological hinge: Step 4, where a conceptual rule (*"coupling must not exceed..."*) is translated into a physical-code axis candidate (`Coupling`). This is the only step where the sentence's wording influences the physical interpretation of the eventual task. It is also the step where the most tempting shortcut lives — and the step where the protocol's discipline matters most. This section makes the argument explicit.

### 5.1 The temptation the invariant defeats

Consider what a less disciplined system would do at Step 4. The translator has found one axis candidate with high confidence; the operator will almost certainly bind it. The temptation is to *collapse the translation into a decision*: take the highest-confidence candidate, declare it the axis, and hand the operator a pre-bound predicate. This collapses two ontological categories — *candidate meaning* (what the sentence suggests) and *executable commitment* (what the project will measure) — into one, in the name of convenience.

In OSP, this shortcut is not a philosophical disagreement; it is a **protocol-level rejection** — outside the protocol boundary, not left to the translator's discretion. The translator and the binder are separated by a function boundary and an `OperatorCapability` token; there is no path, internal to the lowering, that turns a `CrossFamilyHint` into an `ExecutablePredicateSet`. The operator, and only the operator, crosses that boundary. INV-P3 exists to keep that boundary load-bearing.

### 5.2 Ambiguity as a computed value

The translator's output is a `CrossFamilyHint` carrying zero or more axis candidates, and an *ambiguity* level derived from the candidate count. INV-P3 is not a design aspiration we argue for; it is a property we *compute*. The `ambiguity()` accessor is a pure match on `axis_candidates.len()`, so the stored representation and the derived ambiguity cannot fall out of sync — there is no field to drift.

The three ambiguity states are ontological, not merely technical:

- **`SingleCandidate`** (one candidate axis). Translation narrowed the field to one, but did not bind it. The commitment ball is in the operator's court; a mismatch at binding is a strict `AxisMismatch` reject. We renamed this state from `Certain` during review precisely to shed the ontological-certainty connotation — *single* is a count, not a guarantee.
- **`MultipleCandidates`** (two or more axes). Translation genuinely could not disambiguate, and the protocol refuses to silently pick one. The ambiguity is preserved as a first-class value and bounded to the candidate set: the operator may choose one of the proposed axes, but choosing outside the set is an `AxisNotInCandidates` reject.
- **`NoAxisCandidate`** (zero axes). Conceptually, this denotes that translation ran and proposed no physical axis. In the current lowering representation, this *is represented as* the absence of a `CrossFamilyHint` (asserted in held_005), not a stored empty hint; the invariant is the same: no executable commitment is created. This is deliberately distinct from `NoTemplateMatch`, where no template was suggested at all — a rule containing *"azalt"* (reduce) suggests the `MetricDelta` template while still proposing no axis.

### 5.3 Two hint sources — identity and translation

Axis candidates carry a *source* that records how the candidate was reached, and the protocol recognizes two:

- **`KeywordMatch`** (default confidence 1.0). The rule's canonical form contains the axis's own English name as a substring — *coupling*, *cohesion*, *instability*, *entropy*, the witness-depth family. `KeywordMatch` is substring-level identity — which is precisely why held_002 can match the wrong domain: a sentence about couplings in a pipe assembly produces a `Coupling` hint that is lexically correct and semantically wrong (Section 10). The substring nature is the mechanism; the false positive is its honest consequence.
- **`LanguageAlias`** (default confidence 0.9). The rule's canonical form contains a folded Turkish equivalent — *bağımlılık* → *bagiml* → `Coupling`. This is *translation* rather than identity: a weaker, but still deterministic, evidence of intent.

These confidence values are conventional ordering constants for tie-breaking during hint merge, not calibrated estimates. Confidence is never aggregated as a pseudo-probability; the merge rule takes the winning hint whole (all four fields — axis, confidence, source, reason — from one candidate), never blending a hybrid.

### 5.4 The normalize pipeline — a controlled shared space

For KeywordMatch and LanguageAlias to land in the same comparison space, the protocol normalizes both the rule canonical and the stored patterns through a fixed pipeline: NFC composition, then a Turkish-character fold (`I / İ (U+0130) / ı (U+0131) → i`, `Ğ (U+011E) / ğ (U+011F) → g`, …), then ASCII lowercase. Two constraints in this pipeline are load-bearing, and both differ from the naive intuition that "lowercasing is just lowercasing":

First, **NFC must precede the fold.** A decomposed `İ` (U+0049 + U+0307) is two code points before NFC composes it into the precomposed `İ` (U+0130); the fold then maps that precomposed code point to `i`. Running the fold on the decomposed form would miss the match. This property is pinned by a dedicated decomposed-input test.

Second, **the final lowercase is deliberately ASCII-only, not Unicode-aware.** A Unicode-aware lowercase would map `İ` (U+0130) to the sequence `U+0069 + U+0307` (i plus combining dot above), reintroducing the dotted/dotless-I distinction the fold was built to collapse. The ASCII-only step is what keeps the fold's result stable.

The protocol sacrifices locale-correct lowercasing to create a controlled matching space shared by the current English/Turkish fixture set — deterministic, not provably complete. A well-intentioned future "fix" that made the lowercasing Unicode-aware would silently shift that space and break matches involving the dotted/dotless-I distinction; the ASCII-only constraint exists precisely to make that shift a visible regression rather than a silent semantic drift. The held-out fixture held_001 pins the result: *"Modüller arası bağımlılık azaltılmalı."* lowers to a canonical containing *bagiml*, which the alias table matches to `Coupling`.

### 5.5 The membership rule — where binding creates commitment

Translation proposes; binding commits. The single binding rule makes this executable in three branches: if the candidate set is empty, the operator is free to bind any axis; if the set contains the operator's chosen axis, binding proceeds; otherwise, the binding is rejected — with a *precise* error type that names the actual violation: `AxisMismatch` when there was exactly one candidate and the operator chose another, `AxisNotInCandidates` when there were several and the operator chose outside the set. The error type itself carries epistemic information.

The translator may propose a bounded candidate set; only operator binding crosses the boundary into executable commitment.

### 5.6 Restraint as protocol boundary

INV-P3's restraints — no executable predicate from translation, no confidence aggregation, no ontological certainty attached to `SingleCandidate` — are not promises the protocol makes and keeps. Some of them are *structurally impossible to violate*: ambiguity is computed from the candidate count, never stored, so a hint cannot carry a stale or inconsistent ambiguity. The rest are *rejected at the protocol boundary*, enforced by the type boundary, the smart-constructor boundary, and the regression-test suite together. None of them is a promise; a promise can be broken by a determined caller, but a structural impossibility cannot be violated at all, and a protocol boundary can only be crossed by a caller who already holds the capability token.

The example in Section 2 is therefore not a lucky successful parse. It is a demonstration that even a successful parse remains non-executable until binding — and that the binding, in turn, is an act the sentence itself can never perform.

## 6. Binding & Task Genesis (INV-P2, INV-T2)

> *"Accepted intent is not executable work."*

The bridge between the anchoring layer and the trajectory layer is a three-gate API. No gate can be skipped, and no gate can substitute for another, because each proves a different epistemic precondition. Table 2 summarizes the gates; the prose below explains why the count is three and why two distinct capability tokens are involved.

**Table 2 — The three-gate task-genesis API.**

| Gate | Function | Takes | Returns | Capability token | Invariant |
|---|---|---|---|---|---|
| **1** | `verify_accepted_task_candidate` | the concept graph, a candidate node id | `AcceptedTaskCandidateRef` | none (verifies a state already granted upstream) | INV-C3 (the node must already be `Accepted`) |
| **2** | `bind_metric_threshold` | a `PredicateStub`, a `MetricThresholdBinding` | `ExecutablePredicateSet` | `OperatorCapability` | INV-P2 (keyword hint ≠ executable predicate) |
| **3** | `create_task_from_accepted_candidate` | the accepted ref, the bound predicate set, a label, allowed operations, constraints | `trajectory::Task` | `OperatorCapability` | INV-T2 (capability-gated task genesis) |

**Why three gates, not two or one.** A single gate is insufficient because the three inputs — accepted intent, bound predicate, and operator capability — live in two different epistemological domains: graph acceptance (anchoring) and task genesis (trajectory). Collapsing them would merge two ontological categories. Two gates are insufficient because binding and genesis, while both requiring `OperatorCapability`, cross different boundaries: binding crosses *candidate meaning → executable commitment*, while genesis crosses *accepted intent → navigator-runnable work*. Folding them into one function would let a caller who held only one capability mint a task by performing both acts in a single call. The three-gate split keeps each epistemic transition in its own function, with the `AcceptedTaskCandidateRef` — a non-forgeable proof-token whose `id` field is private and whose only constructor is Gate 1 itself — threading Gate 1's result into Gate 3 through Gate 2.

**Two capability tokens, deliberately distinct.** The protocol distinguishes two tokens that look alike but govern different lanes:

- **`OperatorAcceptance`** lives in the anchoring domain and grants the *Candidate → Accepted* transition (INV-C3). Its constructor is `pub(crate)`: external crates and integration tests cannot mint it, by design. The transition is reached through `OperatorReviewSession` (Section 3), which consumes the token internally; the legacy `promote_to_accepted` path was removed before the evaluated release, and `seed_trusted` remains a bootstrap-only trusted-boundary exception outside INV-C13's scope.
- **`OperatorCapability`** lives in the trajectory domain and grants both the *PredicateStub → ExecutablePredicateSet* transition (INV-P2, Gate 2) and the *accepted-ref + bound-predicate → Task* transition (INV-T2, Gate 3). Its `issue()` constructor is public, but the functions that consume it are gated by its presence at the type boundary.

The tokens are kept separate precisely so that the function which creates a task does not also need to ask for acceptance — the accepted state arrives as a proven reference, not as a re-requested capability. This prevents one capability from being overloaded across two epistemic transitions, which is the load-bearing argument for keeping anchoring acceptance and trajectory capability separate.

**Deterministic TaskId derivation.** Gate 3 derives the task's identifier by FNV-1a hashing of the accepted candidate's canonical name (offset basis `0xcbf29ce484222325`, FNV prime `0x100000001b3`), with `0` reserved. A candidate always produces the same task id, which makes the end-to-end replay reproducible across runs and lets parallel tests reason about task identity without coordination. The derivation provides stable identifiers within the evaluated fixtures; it does not constitute a collision-free identity scheme (see Section 10). An atomic counter would have broken reproducibility and parallel-test reasoning; the deterministic hash preserves them.

Taken together, the three gates realize the chain's terminal claim: accepted intent plus operator-bound predicate plus operator capability yields a `trajectory::Task` that Paper 2's navigator can execute — and no proper subset of those three inputs can produce one. The acceptance that Gate 1 verifies is a real promotion performed through `OperatorReviewSession` (Section 3, INV-C3/C12/C13) rather than a seeded state.

## 7. Verification Evidence

*"A gate that only passes is indistinguishable from no gate. These paths prove the gates reject."*

### 7.1 Type-level trybuild (stratum 1)

Twenty-four cumulative compile-fail tests across the workspace exercise representative type-boundary violations for the genesis, lowering, translation, capability, and supersede-construction gates. These tests do not map one-to-one to the 15 Paper-3-specific invariants: thirteen invariants are type-enforced (some by structural type shape rather than a dedicated compile-fail fixture), while two (INV-C14, INV-C15) are runtime-asserted; the supersede-application opacity tests (added by the supersede vocabulary) guard the construction boundary parallel to INV-C13, not the runtime transition semantics. Two compile-fail tests belong to Paper 2's INV-T2 boundary used at task genesis. The tests live in `crates/osp-core/tests/anchoring_typelevel.rs` and its `compile_fail/` fixtures. A contributor who weakens an invariant sees a compile error before the code reaches `main`.

### 7.2 Golden fixture conformance (stratum 2)

Thirteen golden fixtures (`anchoring.fixture.v1` schema) exercise the deterministic pipeline across the spectrum of packet types — `UserVision`, `Requirement`, `AntiGoal`, `Decision`, `Assumption`, and the `DerivesRule` / `DerivesTask` / `ImplementedBy` edge families. We report a five-state conformance rather than a binary pass/fail: 9 Conform, 2 PartialConform, 2 RejectAsExpected, 0 KnownLimitation, 0 UnexpectedFailure (`conformance-results.json` [18]). The classification is *test-referenced but analyst-assigned*: each fixture cites the test that reproduces its behavior, but the conformance state reflects an analyst's judgment about how closely the observed behavior matches the fixture's expected semantics, not a single assertion's verdict.

### 7.3 Held-out adversarial (stratum 3)

Five sentences, four held out during development and one regression-anchored, probe the pipeline on inputs the lowering was not tuned for: a bilingual (Turkish) alias chain, a semantic false-positive (*"couplings in a pipe assembly"*), a negation (*"must not be enforced during tests"*), a multi-axis case (*"coupling and cohesion"*), and a bare-witness regression. Conformance: 3 Conform, 2 KnownLimitation, 0 UnexpectedFailure (`held-out-adversarial-fixtures.json` [18]). The two known limitations are not failures the protocol hides; they are precisely the boundaries Section 10 names — the matcher's lexical false-positive and the classifier's negation blindness — and their presence in the held-out set is what keeps the conformance claim non-tautological.

### 7.4 End-to-End Binding-Chain Replay (stratum 4)

A single frozen replay (`e2e-binding-chain-replay.json` [18]) walks the sentence *"Coupling must not exceed module threshold."* through all eight steps of the binding chain, from the deterministic pipeline run (Step 1, real) through registry insertion (Step 8). Step 1 is a real `run_with_source` call that produces `RuleCandidate:CouplingMustNot` and inserts it into the graph; Step 6 (Candidate→Accepted promotion) is a real `OperatorReviewSession` promotion under INV-C12/C13, rather than a seeded state. The replay is the chain's positive existence proof, and its Step 6 is now the chain's most disciplined surface (§9.5).

### 7.5 End-to-End Rejected Paths Replay (stratum 5)

Six frozen rejected-path records (`e2e-rejected-paths-replay.json` [18]) prove the gates refuse invalid input: `AxisMismatch` (a `SingleCandidate` stub bound to the wrong axis), `AxisNotInCandidates` (a `MultipleCandidates` stub bound outside the set), `TemplateNotSuggested` (a `NoTemplateMatch` stub presented for binding), `NotAccepted` (a still-`Candidate` node presented to `verify_accepted_task_candidate`), `StaleBasis`/`NotFound` (review-session basis freshness and lookup boundary), and `NotPromotableFrom` (already Accepted/Rejected nodes cannot be decided again through the reviewed path). A gate that only passes is indistinguishable from no gate. A structural property of the test design reinforces this: the rejected-path assertions live *inside* the JSON builder, so every normal CI snapshot run re-exercises the rejections. If a gate were ever weakened to let an invalid input through, the builder would produce a different artifact, the snapshot comparison would fail, and the regression would surface before merge.

### 7.5b Supersession evidence scope

The binding chain (Sections 7.4–7.5) is accompanied by frozen positive and rejected-path replays. Supersession (INV-C14/C15) is verified in this artifact release through runtime transition tests rather than a frozen replay: an error-path matrix over twelve failure paths spanning eleven `StoreError` variants, plus chain, consolidation, exhaustion, and atomicity tests, every one asserting that the graph, both ledgers, and the audit sequence remain unchanged on a returned `Err`. No frozen supersession replay is included in this release; the supersession vocabulary's runtime evidence is reproduced by the test suite, not by a JSON artifact.

### 7.6 What this does not evaluate

This is a verification paper, not a semantic-extraction benchmark. Five boundaries are explicit. First, the golden and held-out fixtures are self-authored; a fully independent gold standard is future work. Second, the deterministic keyword matcher that drives translation is a placeholder for concept-synthesis work, and it admits a known lexical false-positive (Section 10). Third, the code-evidence provider is in-memory; real SCIP integration is future work. Fourth, the review session is exercised through its programmatic API; the evaluated `osp review` CLI provides a human-facing surface for candidate review (listing, freshness-bound basis presentation, accept/reject, an interactive review wizard) and supersession (`osp review supersede`, two-endpoint freshness-bound basis), but the rich lineage/compatibility preview, a richer operator console/TUI, and the desktop Cockpit remain future work. The programmatic promotion and supersession are real and audited regardless of surface. Fifth, the chain is verified for structure and type-level enforcement, not for end-to-end *value* to a development team — that is a product-level question the protocol's future interactive integration would address, and it lies outside this paper's scope.

Having stated what the verification evidence does and does not show, we now position OSP against neighboring traditions.

## 8. Related Work

The binding chain intersects nine research neighborhoods. For each, we state what the neighbor does, what it does not provide, and where OSP's contribution falls relative to it.

### 8.1 Requirements traceability

Requirements traceability has been a stated goal of software engineering since at least the Requirements Traceability Matrix (RTM) of IEEE 830 and its successors [1], and the *traceability problem* itself was framed sharply by Gotel and Finkelstein [2], who located the difficulty in the social and provenance structure around requirements rather than in the storage layer. Ramesh and Jarke [3] organized the space into reference models for traceability. OSP's binding chain is, in this neighborhood's terms, a typed and gate-enforced traceability matrix: a requirement enters as a `Candidate`, becomes `Accepted` only through an unforgeable capability, and reaches a `Task` only when the traceability link carries a bound, measurable predicate. Where an RTM records that a requirement is connected to a work item, OSP's chain records *by what authority and with what measurable acceptance* — and the link cannot be edited after the fact by an agent that did not hold the capability at the time.

### 8.2 Controlled Natural Languages

Controlled Natural Languages (CNLs) — Attempto Controlled English and its descendants [4], surveyed by Kuhn [5] — restrict natural language to a fragment that a deterministic parser can translate into a formal representation. OSP's deterministic keyword classifier and normalize pipeline occupy the same philosophical position: a controlled fragment of English (and, via the alias table, Turkish) that a deterministic matcher can translate into axis candidates. The difference is what the translation produces. A CNL typically aims at a logical formula; OSP's translation aims at a `CrossFamilyHint` — a *candidate meaning* that the operator must bind before it becomes an executable predicate. The matcher is therefore an entry point that a future CNL or LLM-assisted concept synthesis could replace without changing the binding chain above it.

### 8.3 GraphRAG and Knowledge Graphs

GraphRAG [6] and Knowledge Graphs [7] build structured representations over text to improve retrieval and reasoning. GraphRAG's entity-relation graph is advisory — it improves the relevance of retrieved context but enforces nothing; a knowledge graph describes a domain but does not gate mutations to it. OSP's concept graph is structurally more constrained (typed ontological nodes with position families and decision statuses) and, critically, *actionable*: the graph's acceptance lane gates which candidates can become tasks, and the membership rule (Section 5.5) gates which bindings can produce executable predicates. Where GraphRAG optimizes retrieval, OSP enforces ontological commitment.

### 8.4 Program analysis and architectural conformance

ArchUnit [8] and Software Reflexion Models [9] check conformance of implementation to intended architecture — ArchUnit by failing tests when a rule is violated, Reflexion Models by reconciling a source model with an intended one. Both are post-hoc: they flag violations after the code has changed. Paper 1's Q6 (Rule) gate already moved conformance checking to a *pre-mutation* position, evaluated on a hypothetical delta; Paper 3's contribution at this layer is the upstream question — *where does the rule come from, and with what authority?* A rule that enters the graph as a `RuleCandidate` and survives candidate isolation (INV-C3) is a rule the operator has accepted; a rule that an agent asserts without that acceptance remains a candidate and cannot gate anything until it is promoted. The genesis layer therefore supplies the provenance that post-hoc conformance checkers assume.

### 8.5 AI coding agents

Autonomous coding agents — SWE-agent [10], RepoCoder [11], and the reflexive-agent family [12] — navigate repositories through file-system actions guided by LLM reasoning, achieving high task-completion rates but providing no architectural safety net: an agent can violate a dependency rule or raise coupling beyond tolerance with no deterministic rejection. These agents operate without a typed notion of *intent*: a natural-language instruction becomes a patch through file edits, with nothing in between that carries authority, acceptance, or a measurable acceptance condition. OSP's genesis layer is the missing upstream: it gives an agent's instruction an ontological status (candidate), a promotion path (operator acceptance), and an executable form (bound predicate → task) that the Paper 2 navigator can then run under measurement and witness policies. The agent never gains the authority to promote its own candidates, by INV-C3.

### 8.6 Position within the OSP series

This paper is the third of three companions. Paper 1 [13] established the *static* conceptual space; Paper 2 [14] established the *dynamic* navigation layer. This paper establishes the *genesis* layer that precedes both. The three layers share the deterministic-gate philosophy; the genesis layer's specific contribution is the binding chain (Sections 2, 5, 6) and the candidate-isolation ontology (Section 3).

### 8.7 Refinement types and typestate

The type-level enforcement discipline of this paper borrows the engineering instinct of the refinement-types [15] and typestate traditions — the idea that illegal states should be made unrepresentable rather than caught at runtime. OSP applies that instinct to project-reality mutation rather than to program-value refinement: where a refinement type narrows the value space of a variable, OSP narrows the *authority space* of a transition (a candidate cannot become accepted without a capability token; a stub cannot become an executable predicate without operator binding).

### 8.8 Description logics and ontologies

The concept graph resembles an ontology at a glance, but OSP's concept graph is deliberately *not* a description-logic reasoner [16]. In an OWL/DL ontology, role and subsumption are decided by open-world inference over axioms; in OSP, a node's role (candidate, accepted, task) is decided by protocol acts — capability-gated transitions recorded in a ledger. The graph does not infer; it witnesses. This contrast sharpens the paper's thesis: the binding chain is a protocol, not a knowledge base, and its guarantees come from gates rather than from reasoning.

### 8.9 Architecture description languages

Architecture Description Languages (ADLs; Medvidovic and Taylor [17]) describe architectural structure — components, connectors, configurations — at a design level. OSP's genesis layer operates on a different question: not *what is the architecture*, but *how does a human sentence become a measured task that can mutate that architecture under gates*. Where an ADL describes, OSP binds intent into executable work; the two are complementary rather than competing, and an ADL could serve as an input source to the genesis layer in future work.

## 9. Discussion

### 9.1 Ontological binding vs embedding
*"Embedding can propose candidate proximity, but it cannot decide ontological role, authority, acceptance, or executability."*

### 9.2 Determinism-first discipline
Every mechanism in this paper is first proven deterministic in isolation (deterministic pipeline, in-memory graph, lexical classifier), with stochastic layers (embedding-assisted candidate generation, LLM-assisted synthesis) deferred to future stochastic layers. The binding chain's correctness does not depend on any model output; a deterministic stub provider reproduces every result in Section 7.

### 9.3 Storage is not epistemology

When the planned graph-database persistence path for the concept graph was deferred, the research-relevant half of persistence-safety was extracted into `osp-core` itself : `ConceptGraph`, `AnchorPlan`, and `AnchorCandidate` are non-`Deserialize`, whether the backend is in-memory or a future graph store. The `AnchorStore` trait carries the principle as its doc-string: *persistence does not weaken epistemic gates.* A candidate cannot become mainline knowledge by being deserialized, regardless of which store implements the trait; the acceptance gate is in the type, not in the storage layer. The protocol's epistemic guarantees are therefore independent of which persistence backend is chosen, and the deferral of any specific backend does not weaken them.

### 9.4 Fixture design must be verifiable
Across four review rounds of the evidence layer, every defect that was caught reduced to one of two failure classes: *constraint non-propagation* (a constraint discovered for one sentence was not applied to all — the canonical-truncation and marker-omission traps, caught three and one times respectively) and *claim-implementation divergence* (an artifact stated a property the test did not actually verify). The pre-flight canonical table (Appendix A) makes both classes structurally impossible by running the real pipeline and the real lowering on every evidence sentence, asserting the canonical, the rule signal, the ambiguity, and the axis candidates in one test. *An invariant that is not under test will be violated* — applied here to the paper's own evidence files, not only to its protocol claims.

### 9.5 The boundary to the human world
The protocol this paper describes is deterministic at every layer it controls: classification, extraction, lowering, translation, binding, and task genesis are all governed by types and tests. There is exactly one place where the chain must accept an input from outside that deterministic world, and that is the moment a human operator decides whether a candidate becomes accepted work. INV-C3, in its original structural form, refused to acknowledge that boundary at all — no code path could perform promotion, which made the protocol perfectly sealed and perfectly unusable. The review session opens the boundary on purpose: `OperatorReviewSession` is the door, and the door is shaped so that the protocol loses nothing essential by opening it. The type system cannot verify that the caller of `open_for_operator` is a human rather than an agent, but it can make the *consequences* of each decision fully recorded (INV-C13) and each decision's *basis* freshness-checked against the candidate's current content (INV-C12). The boundary to the human world is therefore not a soft spot in the protocol but its most disciplined surface: the one place where the deterministic chain consents to be acted upon, and only under audit.

**INV-C11 is a deployment boundary, not a type-level invariant.** INV-C12 and INV-C13 make the decision auditable and deliberate; they do not, by themselves, prevent an agent with operator-surface access from invoking `OperatorReviewSession` or `SupersedeSession` and self-promoting its own candidates (or self-superseding its own accepted decisions). This is not a deficiency unique to OSP — no software governance layer can type-check the caller's humanity; the same boundary exists in any review system (a bot with push access can approve a pull request), and the answer there, as here, is identity and permission separation at the deployment layer, which is deliberately out of protocol scope. What the protocol *can* do is make the bypass visible: every promotion carries a `DecisionRecord` (INV-C13) and every supersession carries a `SupersedeRecord`, both with freshness-checked bases (INV-C12), so an unauthorized transition is not silent. *The protocol can make operator-surface bypass auditable and architecturally out-of-bound; it cannot make an untrusted deployment trustworthy by type alone.* The evaluated artifact draws the surface boundary explicitly: the MCP tool registry is the agent-facing surface and does not expose review or supersession authority operations (asserted by a static regression test), while the `osp review` CLI is the designated operator-facing surface. Authenticating the CLI caller and isolating its credentials from agent processes remain deployment responsibilities (future work will provide process-level isolation guidance).

## 10. Threats to Validity

- **Self-authored gold standard.** The 13 golden fixtures and the 5 held-out sentences were authored by the paper's author. The held-out set provides non-tautological evidence — its sentences were not used during the lowering's development — but it remains self-authored, and a fully independent gold standard is future work. Section 7.2's conformance classification is analyst-assigned for the same reason.
- **Keyword matcher placeholder.** The deterministic keyword matcher that drives translation (Section 5.3) is a placeholder for concept-synthesis work, and it produces a known semantic false-positive: a sentence about couplings in a pipe assembly (held_002) yields a `Coupling` hint that is lexically correct and semantically wrong. *The deterministic matcher can confuse lexical coupling with software coupling. The matcher is not the contribution; the binding protocol is.* The binding chain remains correct because the false-positive produces a *candidate* that the operator can reject, but the matcher's lexical nature bounds its precision.
- **Coarse classifier.** The deterministic classifier cannot parse negation (held_003, *"must not be enforced during tests"*, yields a rule candidate despite the negative intent) and cannot reliably parse typed `Decision:` prefixes (fix_007). Both are documented as known limitations in the conformance table and deferred to concept-synthesis calibration.
- **Stub code-evidence provider.** The code-evidence provider is in-memory and deterministic; real SCIP integration is a future integration concern. The binding chain's correctness does not depend on the provider, but its empirical code-intent coverage does.
- **Acceptance gate strength.** The acceptance gate is no longer exercised by simulation: a real `OperatorReviewSession` performs Candidate→Accepted promotion under INV-C12 (fresh basis) and INV-C13 (no decision without record), with a `NodeDigest` TOCTOU check and an append-only decision ledger. The remaining boundary is honest: the review session does not provide full type-level unforgeability, because the type system cannot verify that the caller is a human operator rather than an agent process. See Section 9.5 for the deployment-boundary interpretation of this risk.
- **Operator identity is attribution, not authentication.** `OperatorId` in v1 is an audit label (attribution), not an authentication proof. The ledger records who *claimed* to make each decision, together with the freshness-checked basis and the reason; it does not verify that the claimed operator is who they say they are. Enterprise identity, signed sessions, and hardware-backed attestation are deployment-layer concerns outside this paper's scope.
- **Re-proposal after rejection is characterized, not resolved.** Rejection is permanent for the node and for the canonical: once a `RuleCandidate:X` is `Rejected`, a later candidate targeting the same canonical does not create a new node or change the rejected node's status — it adds a new edge to the rejected node (characterization test in `store.rs`). This makes re-proposal visible as a collision with a prior rejection, but it is not yet a reversal protocol. A future `ReopenSession` will define normative reversal semantics that present the prior rejection to the operator and record a new decision without erasing the old one.
- **Legacy promotion path (resolved).** The `promote_to_accepted` method — which wrote no ledger record and sat outside INV-C13's scope — was removed before the evaluated release. Promotion is now reached exclusively through `OperatorReviewSession` + `apply_decision`; the `seed_trusted` bootstrap remains as the only trusted-boundary exception that writes no record, and is the sole residual gap against full INV-C13 coverage.
- **TaskId collision is silent.** Task identifiers are derived by applying 64-bit FNV-1a to the accepted candidate's canonical text. Although collisions are expected to be rare, the v1 in-memory registry does not distinguish a repeated insertion of the same candidate from a collision between distinct canonicals and would overwrite the existing entry. A future revision should store the canonical identity alongside the hash and fail closed when an existing identifier resolves to different canonical content.

## 11. Future Work

The genesis layer described here is the deterministic core of a larger program. Three lines of work extend it. First, the lowering's template set — currently `MetricThreshold` with `MetricDelta`, `EvidenceRequired`, and `RelationExists` sketched but not executable — should reach executable form (future executable-template work), so that the binding chain can express delta-based, evidence-based, and relation-based predicates, not only threshold-based ones. Second, the deterministic keyword matcher should be replaced by concept synthesis, in which a code repository's structure proposes concept hypotheses that feed the same binding chain, and by embedding- and LLM-assisted candidate generation; the chain's invariants are designed to survive stochastic candidate sources because the candidate lane and the membership rule do not depend on how the candidate was proposed. Third, the operator-console surface around `OperatorReviewSession` now has an evaluated human-facing CLI: the `osp review` command provides candidate listing, a freshness-bound basis presentation, and accept/reject through `OperatorReviewSession` with persistent `AnchorStoreSnapshot` state (so decisions and ledgers survive process restart), and `osp review supersede` provides the two-endpoint supersession transition through `SupersedeSession` with endpoint-specific freshness preconditions. A rich pair-level preview (lineage, compatibility, cycle), the desktop *Project Reality Cockpit* view of the candidate lane, and the remaining authority sessions (`DeprecateSession`, `ReopenSession`) remain future work so that every status transition has its own audited door; `SupersedeSession` is established alongside `OperatorReviewSession` as the supersede-transition authority.

Four tightening lines complete the threat surface this paper leaves honest. **Process-level isolation for the operator surface (INV-C11):** the protocol makes operator-surface bypass auditable but cannot prevent an agent with shell access from invoking `OperatorReviewSession`/`SupersedeSession`; the evaluated artifact separates the agent-facing MCP surface (which does not expose those constructors) from the operator-facing `osp review` CLI, but future work will provide deployment guidance (separate binary, capability token externalization, credential isolation) for stronger process-level guarantees. **Real backend transaction guarantee (INV-C13):** the v1 in-memory store verifies the atomic status-transition-plus-ledger-append, but graph backends must implement `apply_decision` as a transaction (future transactional backends). **Concurrent review visibility:** v1 review is single-session and Candidate-only; future review-queue work will carry session-claim metadata (which operator is reviewing what) as session state, *not* as a `DecisionStatus` variant — the rejected `InReview` lane taught that operational visibility must not be conflated with ontological status. **Normative reversal:** the characterization test (§10) shows that re-proposal after rejection currently collides with the rejected node without changing its status; a `ReopenSession` will define how a rejected intent can be re-examined without erasing the prior rejection from the ledger.

## 12. Conclusion

> *"Words do not mutate project reality. Only bound, accepted, measured structures can."*

This paper has argued that a human sentence becomes project work only through a type- and protocol-enforced binding chain, and that each transition in the chain is a gate whose admissibility and effects are protocol-enforced while substantive operator decisions remain explicit human judgments. The chain — candidate isolation, operator acceptance, predicate lowering, cross-family translation, operator binding, capability-gated task genesis — turns a sentence into a navigator-ready task, and at no step does proximity or wording suffice. The sentence influences exactly one step (translation), and even there it produces a candidate, not a commitment. Every transition that creates authority or executability requires a capability token the sentence cannot supply.

The three OSP papers form a layered whole. Paper 1 established the static conceptual space in which a project's measured state lives and is witnessed. Paper 2 established the dynamic navigation by which an agent moves that state toward a healthier one, under measurement predicates rather than target coordinates. Paper 3, the genesis layer, answers the question that precedes both: how does a sentence become the bound, accepted, measured work that the static space witnesses and the dynamic navigator runs? The answer is the binding chain, and its ontological consequence is the paper's closing claim — that a project's reality is not the set of sentences said about it, but the set of structures that have survived the gates between a sentence and a task.

---

## Appendix A: Pre-flight Canonical and Marker Table

> *"An invariant that is not under test will be violated."* Across four review rounds of the evidence layer, the canonical-truncation trap (a rule's canonical name is derived from its first three words, while the lowering scans the canonical for axis keywords) was caught three times, and the marker-omission trap (the rule signal requires `must not`, not `must`) once. The table below makes both traps structurally impossible by running the real pipeline and the real lowering on every evidence sentence in a single test.

| Sentence | Canonical (first 3 words) | Normalized | Rule signal | Ambiguity | Axes |
|---|---|---|---|---|---|
| Coupling must not exceed module threshold. | CouplingMustNot | couplingmustnot | true (`must not`) | SingleCandidate | [Coupling] |
| The couplings in the pipe assembly must not be reused. | TheCouplingsIn | thecouplingsin | true (`must not`) | SingleCandidate | [Coupling] (WRONG — physical pipe) |
| Modüller arası bağımlılık azaltılmalı. | ModüllerArasıBağımlılık | modullerarasibagimlilik | true (`malı`) | SingleCandidate | [Coupling] (via `bagiml` alias) |
| Coupling rule must not be enforced during tests. | CouplingRuleMust | couplingrulemust | true (`must not`) | SingleCandidate | [Coupling] |
| Coupling and cohesion must not diverge. | CouplingAndCohesion | couplingandcohesion | true (`must not`) | MultipleCandidates | [Coupling, Cohesion] |
| Witness count must not create metric evidence. | WitnessCountMust | witnesscountmust | true (`must not`) | NoAxisCandidate | [] (bare witness excluded) |

The table is pinned by the `preflight_canonical_and_rule_signal_for_paper3_evidence_sentences` test in `paper3_evidence.rs`, which runs the real deterministic pipeline and the real lowering on each sentence, asserting the canonical name, the rule signal, the ambiguity, and the axis candidates. Any future fixture that trips the canonical or marker traps will fail this test before it reaches the evidence files.

## Appendix B: End-to-End Binding-Chain Replay

The frozen artifact `e2e-binding-chain-replay.json` [18] reproduces the full eight-step chain walked in Section 2: sentence → `RuleCandidate` (real pipeline run) → `PredicateStub` → `CrossFamilyHint` → operator binding → `ExecutablePredicateSet` → verify accepted (real promotion via the review-session API under INV-C12/C13) → create task → registry insertion. The artifact is a deterministic snapshot: the same source produces the same JSON byte-for-byte, and its integrity hash is recorded in `run-metadata.json`.

## References

[1] IEEE. *IEEE Standard for Software Requirements Specifications.* IEEE Std 830-1998; superseded by ISO/IEC/IEEE 29148:2018, *Systems and software engineering — Life cycle processes — Requirements engineering.*

[2] O. Gotel and A. Finkelstein. "An Analysis of the Requirements Traceability Problem." In *Proceedings of the First International Conference on Requirements Engineering (RE'94)*, IEEE, 1994.

[3] B. Ramesh and M. Jarke. "Toward Reference Models for Requirements Traceability." *IEEE Transactions on Software Engineering* 27(1), 2001.

[4] N. E. Fuchs, U. Schwertel, and R. Schwitter. *Attempto Controlled English (ACE).* Language specification, Department of Informatics, University of Zurich, 2008 (extended 2010).

[5] T. Kuhn. "A Survey and Classification of Controlled Natural Languages." *Computational Linguistics* 40(1), 2014.

[6] D. Edge, H. Trinh, N. Cheng, et al. "From Local to Global: A Graph RAG Approach to Query-Focused Summarization." arXiv:2404.16130, 2024.

[7] A. Hogan, E. Blomqvist, M. Cochez, et al. "Knowledge Graphs." *ACM Computing Surveys* 54(4), 2021. arXiv:2003.02320.

[8] A. Muschevici, D. Clarke, and J. Proença. "ArchUnit: Unit Testing Architecture." *IEEE Software* 35(5), 2018.

[9] G. C. Murphy, D. Notkin, and K. J. Sullivan. "Software Reflexion Models: Bridging the Gap Between Design and Implementation." *IEEE Transactions on Software Engineering* 27(4), 2001.

[10] J. Yang, E. Jimenez, I. Wettig, et al. "SWE-agent: Agent-Computer Interfaces Enable Automated Software Engineering." arXiv:2405.15793, 2024.

[11] F. Zhang, B. Chen, Y. Zhang, et al. "RepoCoder: Repository-Level Code Completion Through Iterative Retrieval and Generation." In *EMNLP*, 2023.

[12] N. Shinn, F. Cassano, A. Gopinath, et al. "Reflexion: Language Agents with Verbal Reinforcement Learning." In *NeurIPS*, 2023.

[13] V. Er. "Ontological Space Protocol: Modeling Software as a Conceptual Space with Epistemological Witnessing." OSP Paper 1, v2.6. Zenodo, 2026. doi:10.5281/zenodo.21206545

[14] V. Er. "Architectural Trajectory Navigation: From Target Coordinates to Measurement Predicates." OSP Paper 2, v1.2. Zenodo, 2026. doi:10.5281/zenodo.21207704

[15] R. Freeman and F. Pfenning. "Refinement Types for ML." In *Proceedings of the ACM SIGPLAN Conference on Programming Language Design and Implementation (PLDI)*, 1991.

[16] W3C OWL Working Group. "OWL 2 Web Ontology Language Primer (Second Edition)." W3C Recommendation, 2012.

[17] N. Medvidovic and R. N. Taylor. "A Classification and Comparison Framework for Software Architecture Description Languages." *IEEE Transactions on Software Engineering* 26(1), 2000.

[18] V. Er. OSP Paper 3 — Evidence Pack (Frozen Verification Snapshots). Zenodo, 2026. doi:10.5281/zenodo.21207762
