# Ontological Space Protocol: Modeling Software as a Conceptual Space with Epistemological Witnessing

**OSP Paper Draft v2.6** · Target: arXiv then ACM TOSEM
**Authors:** Volkan ER
**Date:** 2026-06-24
**Revision:** v2.5 → v2.6 (claim discipline pass): 28-repo/Rust-Go/AI-era-foam claims removed from Abstract/Contributions/Methodology (unsupported by paper tables — reserved for TOSEM revision); test count unified to 330+; RepoCoder/SWE-agent citations fixed ([14],[15]); constant ~155 corrected to median 155, density-sensitive; A4 ~500 lines softened to small auditable module; date-fns D precision harmonized to 0.036.

---

## Abstract

AI-assisted software development lacks a persistent, verifiable representation of project state. Context windows grow stale, architectural rules embedded in documentation are silently violated, and modern Git workflows (squash-merge, rebase) systematically erase review evidence from commit history. We present the **Ontological Space Protocol (OSP)**, a framework that models software projects as conceptual spaces governed by physical rules — coupling, cohesion, instability, entropy, and epistemological witnessing.

OSP positions every module in a coordinate system and advances project time only through a two-witness quorum. We prove that this quorum rule is a **safety-refinement** of authenticated Byzantine Fault Tolerance (BFT) for f = 1: it provides optimal safety against Byzantine witnesses while leaving liveness to standard distributed systems mechanisms (Theorem 1). We introduce a **tri-state witness classification** (Witnessed, Unwitnessed, Unobservable-locally) that resolves the "squash-merge blind spot" — in our 15-repository corpus, 8 repositories use workflows where review evidence is not locally observable from merge commits.

We evaluate OSP on 15 repositories across Python, TypeScript, and JavaScript using tree-sitter (Tier 1) and SCIP semantic indices (Tier 2). SCIP-based LCOM4 cohesion is computed for 13,031 classes across 13 repositories, suggesting language-paradigm differences in cohesion distributions. A binary merge-based classifier would mark 8 repositories as unwitnessed; OSP instead classifies them as Unobservable-locally, avoiding unsupported negative claims. Tree-sitter-derived abstractness values (A = Nₐ/Nc) produce meaningful Martin main-sequence distances, distinguishing architectural balance across projects. A HashMap-based import resolver enables 3000-file analysis in 11.2 seconds median (5-run, release build). Finally, a token-size benchmark on 13 repositories shows that OSP coordinate prompts reduce architectural context size by 99.53% on average versus full repository dumps and by 89.19% versus a structure-aware 2-hop baseline, using a chars/4 token approximation. All data and implementations are open-source and reproducible.

---

## 1. Introduction

### 1.1 The Problem

AI-assisted software development faces three structural challenges:

1. **Context drift**: LLM agents lose track of architectural constraints as context windows grow. Prompts become stale; rules embedded in READMEs are silently violated.

2. **Epistemological opacity**: Modern Git workflows (squash-merge, rebase) erase review metadata from commit history. Tools that rely on merge-commit detection systematically misclassify well-reviewed projects as unwitnessed projects.

3. **Metric alienation**: Software metrics (coupling, cohesion, instability) are computed but rarely actionable. As Tempero and Ralph (2026) argue, practitioners find existing metrics insufficient for architectural decisions [9].

### 1.2 Our Approach: OSP

OSP addresses these challenges through three interlocking ideas:

**Ontological coordinate system.** Every module occupies a position P = (x, y, z, w, v) in a 5-dimensional raw space (coupling, cohesion, instability, entropy, witness-depth), with a derived vision-alignment coordinate u = 1 − θ. The project's architectural vision V_vision is hand-declared; deviation θ measures how far each module has drifted from the vision line.

**Epistemological witnessing.** Time advances only when a claim (e.g., a pull request) receives sufficient witnessing: support(C) ≥ θ_quorum from at least min_approvers independent non-author witnesses. This two-witness rule is not arbitrary policy — we show it provides safety guarantees inspired by authenticated Byzantine Fault Tolerance (Section 4).

**Tri-state witness status.** Rather than binary (witnessed/unwitnessed), OSP classifies each repository's witness status as Witnessed, Unwitnessed, or Unobservable-locally, explicitly distinguishing "evidence unavailable" from "evidence absent."

### 1.3 Contributions

This paper makes four contributions:

1. **Tri-state witness model with BFT safety guarantee** (Section 3–5): We introduce a three-state classification (Witnessed, Unobservable-locally, Unwitnessed) that resolves the squash-merge blind spot affecting 8/15 repositories in our corpus. We prove that OSP's two-witness quorum rule is a safety-refinement of authenticated Byzantine Fault Tolerance for f = 1 under four explicit assumptions (Theorem 1), with liveness delegated to standard distributed systems mechanisms.

2. **Deterministic claim-based gate architecture** (Section 4, 6): We define a two-layer commit pipeline — deterministic Q4–Q6 gates (syntax, vision deviation θ, architectural rules) evaluated before witness-based Q1–Q3 gates — that rejects structurally invalid or architecturally deviating claims before any code generation or witness evaluation. Every metric carries provenance (source, confidence, coverage), ensuring epistemological honesty: "we don't know" is never conflated with "we measured 0.5."

3. **Multi-language empirical evaluation** (Section 7): We analyze 15 repositories across Python, TypeScript, and JavaScript using tree-sitter (Tier 1) and SCIP semantic indices (Tier 2). Real LCOM4 cohesion is computed for 13,031 classes across 13 repositories via scip-python and scip-typescript. Results suggest language-paradigm differences in cohesion distributions (Python y ≈ 0.60, TS/JS y ≈ 0.52).

4. **Token compression via epistemic codec** (Section 7.7): A 13-repository benchmark demonstrates that OSP's coordinate-based prompt reduces architectural context size by 99.53% on average versus full repository dumps and 89.19% versus a structure-aware 2-hop baseline. OSP prompt size remains small and depends on subgraph density rather than repository size; median size is 155 tokens, with dense slices reaching 6.5K.

---

## 2. Motivating Example

Consider a developer asking an AI agent to add a logging feature to an existing module. In a traditional workflow, the agent writes code and submits a PR; the reviewer must manually check architectural constraints.

In OSP, the same interaction proceeds as follows:

1. **Claim creation**: The agent's LLM produces a `DeltaProposal` (structural changes only — new nodes, new edges, entity modifications). The engine **computes** the raw position `P_raw(C)` from the proposed structural changes — the LLM never declares positions (epistemological integrity, Section 3.2). Specifically, the engine applies ΔS to a hypothetical graph (pre-mutation) and runs the analyzer pipeline (Section 6.2) to measure the actual coupling, cohesion, instability, entropy, and witness-depth of the affected modules. This `computed_raw` reflects measured reality, not agent-claimed coordinates.

2. **Claim-based gates (Q4-Q6, deterministic, pre-witness)**:
   - **Q4 (Syntax)**: `DeltaProposal` conforms to `OutputContract` schema — malformed claims rejected before witnesses see them.
   - **Q5 (Vision)**: θ(P_raw(C), V_vision) > θ_bound (e.g., the new module has coupling = 0.95 while the vision declares coupling ≤ 0.4) → claim **rejected** — it cannot enter the objective space.
   - **Q6 (Rule)**: ΔS violates an architectural Rule (e.g., domain → infrastructure direct access) → claim rejected.
   
   These gates run deterministically before any witness evaluation.

3. **Witness evaluation (Q1-Q3)**: If the claim passes Q4-Q6, the engine evaluates the witness set Ω. With two MergeCommit witnesses (support = 2.0 ≥ θ_quorum = 1.5), the claim commits.

4. **Space Commit**: The space expands: ΔV nodes and ΔE edges are added via `apply_delta` (mutation-only, infallible), positions of ΔV ∪ N₁(ΔV) are recomputed.

5. **Drift detection**: If a neighbor module's new position has θ > θ_bound (it was pushed toward negative space by the new edges), a drift warning is emitted — the commit is accepted, but the degradation is reported.

This example illustrates OSP's core property: **negative-space claims cannot enter the objective space** (two-layer safety: Q4-Q6 deterministic validity predicates + Q1-Q3 witness quorum), while **positive-space claims with sufficient witnessing are committed and their side effects are monitored**.

---

## 3. OSP Model

### 3.1 Space

A conceptual space is defined as:

```
S = (V, E, G, t_state)
```

where V is the set of conceptual nodes (9 types: Module, Concept, Feature, Bug, Rule, Agent, Intent, Claim, Witness), E ⊆ V × V × K_E is the typed edge set (8 types including epistemological relations: Witnesses, Approves, Violates), G: V → ℝᵏ is the gravity function (rule-imposed weights), and t_state ∈ {t_m, t_c, t_f} is the temporal layer.

**Ontological time-layer mapping.** Each time layer has a primary ontological category: `Intent` lives in `t_f` (potential gradient — issues, roadmap), `Claim` lives in `t_m` (miş'li — candidate, unwitnessed, analogous to an open PR), and committed knowledge lives in `t_c` (şimdiki — witnessed reality, main branch). The flow is `Intent (t_f) → projection → Agent → Belief (t_m) → [Witness W] → Knowledge (t_c)`. Intent does not mutate space; it exerts a gradient from `t_f` into `t_m` through an epistemic projection (Section 9).

Unlike traditional dependency graphs, OSP's edge types include epistemological relations, enabling the BFT-inspired commit semantics described in Section 4.

### 3.2 Coordinate System and Metric Provenance

Each node occupies a raw position P_raw = (P_core, P_custom):

**Core raw axes (5, fixed):**

| Axis | Metric | Source |
|---|---|---|
| x | Coupling | out-degree(Imports) / (1 + out-degree) |
| y | Cohesion | 1/LCOM4 (field-access graph, Section 6.3) |
| z | Instability | Ce/(Ca+Ce) (Martin, pure) |
| w | Entropy | Shannon H (commit→file distribution) |
| v | Witness-depth | witnessed_ratio × ln(1 + distinct_witnesses) |

P_core ∈ ℝ⁵ is present in every project. **Custom raw axes (N, pluggable)** extend the space for domain-specific physics: security (`security.audit` — vulnerability density), accessibility (`wcag.compliance`), performance (`perf.budget`), compliance (`compliance.hipaa`). Custom axes are registered by the trusted operator only (human or bootstrap config); agents cannot introduce new axes — this preserves the "software physics" invariant that physical laws are observer-controlled, not AI-controlled. P_custom ∈ ℝᴺ, N varies per project. The full coordinate system is thus ℝ^(5+N), variable-length per project.

A derived position P_derived = (u, θ, D, risk) captures vision alignment (u = 1 − θ), deviation angle, Martin main-sequence distance (D = |A + I − 1|), and composite risk. Abstractness A = Nₐ/Nc is a diagnostic input for D, not a raw axis.

**MetricValue provenance.** Every metric carries a source tag (TreeSitter, SCIP, Placeholder, Heuristic), a confidence score, and a coverage ratio:

```
confidence = source_base × coverage × stale_penalty
```

For example, TreeSitter-based metrics have base confidence 0.75; SCIP-based metrics have 0.95 × coverage × (0.5 if stale). Placeholder metrics have confidence = 0.0. This ensures that "cohesion = 0.5 because unknown" is never confused with "cohesion = 0.5 because measured." `MetricValue` is produced by the analyzer for all measured metrics (value + source + confidence + coverage). Runtime `RawPosition` stores normalized `f64` values for fast geometric computation, while the corresponding provenance is preserved in `AnalysisResult` and used to weight or qualify cross-project comparisons.

### 3.3 Vision Vector

V_vision is **hand-declared** through three layers: architectural rules (DDD, layering), domain/witness policies (review-required, branch-protection), and non-functional requirements. LLMs may propose adjustments but never auto-apply — preserving human control.

### 3.4 Cosine Deviation Limit

**Finding.** With all axis values ∈ [0,1] (non-negative), CosineDeviation produces θ ∈ [0, 0.5]. The orthogonal limit is unreachable for non-zero vectors, meaning the theoretical π/2 threshold (θ = 0.5) cannot trigger drift warnings. We set θ_bound = 0.2–0.3 based on empirical observation: this range successfully separates modules whose cosine similarity drops below 0.4–0.6 (indicating significant architectural divergence) from those that remain reasonably aligned. Full sensitivity requires Diffusion Distance (Future Work §11.1).

---

## 4. Witness System and Time Semantics

### 4.1 Witness Types and Evidence Deduplication

| Type | Weight | Source |
|---|---|---|
| MergeCommit | 1.0 | `git rev-list --merges` |
| PRMerged | 0.8 | merge-commit signature text |
| TrailerReviewed | 0.7 | `Reviewed-by:` commit trailer |
| CoAuthored | 0.4 | `Co-authored-by:` trailer |

Evidence events are deduplicated by (source, actor, claim) — only the strongest evidence per event survives. This prevents inflation when a merge-commit (1.0) and a PR-merged record (0.8) represent the same review.

### 4.2 Commit Operator

```
W(C, Ω) = Commit(δS)  iff
    (Q4) OutputContract compliant — ΔS schema valid              [claim-based, deterministic]
    (Q5) θ(P_raw(C), V_vision_raw) ≤ θ_bound                     [claim-based, deterministic]
    (Q6) ∀ Rule R: R(ΔS) ≠ Violated                               [claim-based, deterministic]
    (Q1) |distinct_non_author_approvers(Ω)| ≥ min_approvers       [witness-based]
    (Q2) support(Ω) ≥ θ_quorum  (default 1.5)                     [witness-based]
    (Q3) ∀ honest W ∈ Ω: W.verdict ≠ Reject                       [witness-based]
```

**Two-layer gate structure.** Q4-Q6 are **claim-based and deterministic** — they inspect the claim's own properties (schema validity, vision deviation, rule compliance) and run **before** any witness evaluation. A syntactically invalid or vision-violating claim is rejected deterministically, never reaching the witness set. Q1-Q3 are **witness-based** — they require evidence from independent observers (quorum, approver count, absence of honest rejection). Only claims passing Q4-Q6 enter witness evaluation. *(Figure 1 illustrates the two-layer pipeline: Q4-Q6 deterministic gates → Q1-Q3 witness gates → commit.)*

Q5 (vision) is checked **before** any mutation (pre-commit safety gate). If Q5 fails, the claim is rejected and no code enters the objective space. The split into Q4 (Syntax), Q5 (Vision), Q6 (Rule) refines what was previously a single "Q4" gate — enabling distinct calibration feedback per gate (structural hallucination, vision hallucination, rule hallucination).

> **Figure 1: Two-layer commit pipeline.** Panel (a): a valid claim passes Q4 Syntax → Q5 Vision → Q6 Rule (deterministic, claim-based) → Q1-Q3 Witness (quorum-based) → Commit. Panel (b): a rule-violating claim is rejected at Q6 with hallucination classification and calibration feedback. Green = passed, red = failed, gray = skipped. Positions are engine-computed (inv #4). *Rendered: `viz/figure1-commit-pipeline.png` and `viz/pipeline-figure.html`.*

### 4.3 Tri-State Witness Status

Local Git analysis cannot observe all review events. We classify witness status as:

| Status | Condition | Meaning |
|---|---|---|
| Witnessed | merge_ratio ≥ 10% ∧ support ≥ quorum | Sufficient local evidence |
| Unwitnessed | distinct_authors ≤ 1 (solo) | Genuine lack of review |
| Unobservable-locally | multi-author ∧ merge_ratio < 10% | Squash/rebase hides evidence |

The 10% threshold was selected from an observed gap in our 15-repository corpus: merge_ratio shows a bimodal distribution with a gap between 5.9% (date-fns) and 11.6% (svelte). No repository falls in this gap. We therefore treat 10% as a corpus-calibrated threshold rather than a universal constant.

---

## 5. BFT-Inspired Safety-Refinement

### 5.1 Assumptions

**A1 (Authenticated identities).** Each witness has a verifiable identity (GPG key, GitHub account, CI token). A Byzantine witness can forge its own signature but cannot forge another witness's signature. This maps to authenticated Byzantine agreement [2].

**A2 (Bound on Byzantine witnesses).** At most f = 1 of the n ≥ 3 participants (author + ≥ 2 witnesses) is Byzantine. This is a practical threshold matching GitHub's typical review model (author + 2 reviewers).

**A3 (Honest witness soundness).** An honest witness does not approve a claim that it cannot validate against observed evidence and project rules. Specifically, an honest witness that detects a vision violation (θ > θ_bound) or rule violation must reject. This is the soundness requirement linking witness behavior to the validity predicates.

**A4 (Deterministic engine as trusted computing base).** The engine's Q4–Q6 gate evaluation is deterministic, type-checked Rust code that constitutes the trusted computing base (TCB) of the system — analogous to an OS kernel or database engine. If the TCB is compromised, all guarantees fail; this is a standard assumption in systems security. OSP minimizes this TCB by isolating gate evaluation in a small, auditable Rust module with dedicated unit and integration tests, compiled with `-D warnings`. Witnesses provide evidence (Q1–Q3); the engine applies rules (Q4–Q6). The boundary between formal guarantee and practical mechanism is explicit: safety against f = 1 Byzantine witness is guaranteed *given* A4 (engine integrity), while liveness depends on practical mechanisms (reviewer availability, timeout handling) that are not formally modeled.

### 5.2 Theorem and Proof

**Theorem 1 (Safety-Refinement).** Under assumptions A1–A4, OSP's two-witness commit rule provides safety against f = 1 Byzantine witness, mapped to an authenticated BFT quorum model for f = 1.

*Proof.*

**Lemma 1 (Lower Bound — n = 2 is insufficient for liveness).** With one non-author witness (n = 2, author weight = 0), OSP can preserve safety but cannot guarantee liveness under f = 1. If the sole witness is Byzantine-silent or rejects a valid claim, support ≤ 1.0 < θ_quorum = 1.5, and the system must Hold. Thus n = 2 is insufficient to provide both safety and liveness; at least two independent non-author witnesses are required.

**Lemma 2a (Safety — two layers).** Under A1–A4, safety is provided by two independent mechanisms:

*Layer 1 — Deterministic validity predicates (Q4-Q6, claim-based, A4):* Claims with schema violations (Q4), vision deviation θ > θ_bound (Q5), or rule violations (Q6) are rejected by the trusted engine before mutation, regardless of witness support. These are the BFT validity predicates, applied deterministically — no witness (Byzantine or honest) can override them (A4). The LLM never declares positions (P_raw is engine-computed from ΔS), so Q5 measures actual measured deviation, not agent-claimed deviation.

*Layer 2 — Witness quorum (Q1-Q3, A1+A3):* For claims that pass Q4-Q6 (well-formed, vision-aligned, rule-compliant), safety is provided by the quorum rule under A1 (authenticated identities) and A3 (honest soundness). With two witnesses (n = 3), at most one can be Byzantine (A2). An honest witness (A3) rejects a claim it cannot validate; thus a malicious claim passing Q4-Q6 but lacking genuine support has at most one approver (the Byzantine witness, support ≤ 1.0 < 1.5) and cannot commit. The negative-space claim never enters the objective space. □

**Lemma 2b (Liveness — conditional).** Strict-synchronous liveness is not guaranteed at n = 3: a Byzantine-silent witness leaves support below quorum, causing Hold. This is an omission fault in a partially synchronous system. Standard mechanisms resolve this: (a) requesting a third witness (n = 4 > f + 1), or (b) timeout-based retry under partial synchrony assumptions [11].

∴ OSP provides optimal safety against f = 1 Byzantine witnesses; liveness requires standard practical mechanisms.  □

**Corollary (Quorum consistency).** θ_quorum = 1.5 ensures: two strong witnesses (2 × MergeCommit = 2.0 ≥ 1.5) commit; a single witness (1.0 < 1.5) does not — self-merge prevention is structural.

### 5.3 Scope and Limitations

OSP's mapping to BFT is a **safety-refinement**, not a full equivalence. Dolev-Strong requires message rounds, synchrony assumptions, and authenticated broadcast — OSP does not implement these. Instead, OSP adapts the quorum threshold (n > f + 1 for authenticated, f = 1) to software knowledge commits, providing the same safety guarantee under explicit assumptions (authenticated witness identities, local evidence observation).

---

## 6. Implementation

### 6.1 Architecture

The implementation comprises three Rust crates with 330+ unit/integration tests:

**osp-core** (136 unit + 6 integration tests): Ontological primitives (Node, Edge, Space, 9 NodeKinds, 8 EdgeKinds), coordinate system (5 core + N custom axes with pluggable `Axis` trait; `CustomRawPosition` + `MetricValue` provenance for custom axes), witness system (EvidenceEvent, CanonicalWitnessSet, tri-state WitnessStatus; `evaluate()` covers Q1-Q3 only), vision (CosineDeviation, DiffusionDeviation stub, compute_derived), space commit (`apply_delta` mutation-only, infallible; no `commit()` — separation of concerns), TimeFSM (`evaluate` + `apply_delta` composition), SpaceEngine (Q4-Q6 claim-based gates → Q1-Q3 witness → `apply_delta` → reposition → persist), and event-sourcing persistence (milestone snapshots + per-commit deltas). The **agent interaction layer** (`agent.rs`: PermissionMask, DeltaProposal, OutputContract, SyntaxViolation) and **rule engine contracts** (`rule.rs`: Rule trait, RuleViolation) define the foundational types and contracts for Faz 5 LLM integration — types are implemented and unit-tested; full gate logic arrives in Faz 5.

**osp-analyzer** (97 unit/integration tests): Two-tier code analysis with 5 language adapters (Python, TypeScript, JavaScript, Rust, Go via tree-sitter), abstractness computation (A = Nₐ/Nc), LCOM4 cohesion algorithm (bipartite graph → connected components, validated on 13,031 classes), SCIP semantic index loader (protobuf parsing with symbol-string inference fallback for indexers that omit `SymbolKind`), and full analysis pipeline with `--scip` CLI integration. Re-exports `MetricValue`/`MetricSource`/`MetricValueError` from osp-core (single canonical source). SCIP index generation uses `scip-python` (Docker container, `--project-name`/`--project-version` flags) for Python repos and `scip-typescript` (npm, `--infer-tsconfig`) for TypeScript/JavaScript repos.

**osp-spike** (32 tests): Faz 0 validation spike (frozen reference).

**15 implementation invariants** are structurally enforced at the type level: the original 10 (author-witness rejection, EvidenceEvent dedup, tri-state witness, RawPosition/DerivedPosition separation, lazy diffusion, incremental space commit, admin override flag, network-free core, WitnessSet-based operator, pure Instability axis) plus 5 Faz 5 additions (#11 LLM stateless, #12 OutputContract deterministic reject, #13 PermissionMask trusted-operator assigned, #14 prompt as typed data packet, #15 custom axis registration trusted-operator only — agents cannot define new axes).

### 6.2 Two-Tier Analysis

Tier 1 (tree-sitter, always-on) extracts imports, class definitions, and abstractness from syntactic structure. Tier 2 (SCIP, optional) provides semantic field-access data for LCOM4 cohesion computation. When SCIP is unavailable, cohesion defaults to 0.5 with confidence = 0.0 (placeholder).

### 6.3 LCOM4 Cohesion

For each class, OSP builds a bipartite graph (methods ↔ fields) from SCIP field-access occurrences. Connected components of this graph yield the LCOM4 value: LCOM4 = 1 indicates cohesion; LCOM4 ≥ 2 indicates fragmentation. Module-level cohesion is the method-count-weighted average of class-level cohesion.

**Validation:** The LCOM4 algorithm is validated on both synthetic structures (15 hand-crafted classes covering God class, constructor bridging, static isolation, fragmented responsibilities) and **real repositories** via SCIP deployment. We generated SCIP semantic indices for all 15 corpus repos using `scip-python` (Docker, for Python) and `scip-typescript` (npm, for TypeScript/JavaScript), yielding **13,031 class analyses** across 13/15 repos. Two function-oriented repos (lodash, worms-supabase) have zero classes by design — LCOM4 is not applicable to function-only modules, and OSP correctly reports placeholder cohesion (confidence = 0.0) for these. Section 7.6 (RQ4) presents the empirical cohesion distribution.

### 6.4 Event-Sourcing Persistence

Full Space snapshots are stored at milestones (tags, periodic intervals). Per-commit deltas are stored individually. Time-travel: load nearest milestone ≤ t_c → replay deltas. Disk efficiency: 340 periodic snapshots (170 MB) vs. 1 milestone + 340 deltas (2.2 MB) — **98% reduction**.

---

## 7. Evaluation

### 7.1 Research Questions

**RQ1:** Can tri-state witness classification distinguish squash-workflow projects from genuine absence of review evidence?

**RQ2:** Do measured abstractness values produce meaningful main-sequence distances?

**RQ3:** Does the pipeline scale to medium-to-large open-source repositories?

**RQ4:** Do LCOM4 cohesion values (from SCIP semantic indices) reveal meaningful architectural differences across repositories?

**RQ5:** Does OSP's epistemic codec provide consistent architectural context compression across repository scales and project maturities?

### 7.2 Methodology

**Corpus.** Our corpus comprises 15 open-source repositories selected for diversity across language (Python 9, TypeScript 3, JavaScript 3), maturity (small libraries to large frameworks), and workflow (merge-commit, squash, rebase, solo). One Python repository is a solo-author baseline. Repositories were cloned with full Git history.

**Environment.** Windows 11, 32 GB RAM, Rust 1.75+, release build (`cargo build --release`). Each repository was analyzed 5 times (warm filesystem cache); median timing reported with range. Timing measured from process start to analysis completion (includes file I/O, tree-sitter parsing, import resolution, graph construction, metric computation).

**Metrics.** Node count (source files), edge count (internal import edges), abstractness (A = Nₐ/Nc from tree-sitter class definitions), instability (Martin I = Ce/(Ca+Ce)), main-sequence distance (D = |A + I − 1|), witness status (tri-state), merge_ratio (% of commits that are merge commits).

**Token-size benchmark.** For RQ5, we benchmark a 13-repository subset using `cargo run --example token_benchmark -- <repo-path>`. We compare three context-transfer strategies: (1) full repository dump, defined as concatenating `.py/.ts/.js/.rs/.go` source files; (2) structure-aware 2-hop context, defined as the target node plus imports and imports-of-imports (`k=2` BFS); and (3) OSP coordinate prompt, containing 5-axis coordinates per node, typed edges, vision thresholds, rules, and output contract. Token counts use the standard `chars / 4` approximation; this measures prompt-size reduction, not model-specific tokenizer behavior or task success.

### 7.3 RQ1: Tri-State Witness Classification

| Status | Count | Repos |
|---|---|---|
| Witnessed | 6 | click, requests, flask, rich, svelte, commander |
| Unobservable-locally | 8 | fastapi, django, date-fns, httpx, pydantic, chalk, lodash, vitest |
| Unwitnessed | 1 | worms-supabase (solo author, 0 merges) |

**Result.** Binary classification would label all 8 Unobservable-locally repos as "unwitnessed." Tri-state classification correctly identifies them as having multi-author collaboration with hidden review evidence, reserving "unwitnessed" for genuine solo projects.

### 7.4 RQ2: Measured Abstractness

Full dataset (Table 1 — see Appendix):

| repo | A (placeholder) | A (measured) | D (placeholder) | D (measured) |
|---|---|---|---|---|
| django | 0.5 | 0.00 | 0.32 | 0.18 |
| fastapi | 0.5 | 0.01 | 0.36 | 0.13 |
| date-fns | 0.5 | 0.05 | 0.42 | 0.02 |
| vitest | 0.5 | 0.02 | 0.13 | 0.35 |

**Result.** Measured abstractness values reveal architectural patterns invisible to placeholders. date-fns has the smallest main-sequence distance (D = 0.036), indicating conformance to Martin's main-sequence model. vitest has the largest observed main-sequence distance in our corpus (D = 0.352), with very low abstractness relative to its instability. django shows extreme concrete-heaviness (11,014 total types, 16 abstract → A = 0.001) [12].

### 7.5 RQ3: Scale

| repo | files | nodes | edges | time (median, 5 runs) | range |
|---|---|---|---|---|---|
| click | 63 | 63 | 61 | 0.58s | 0.57–0.63 |
| svelte | 3,448 | 3,448 | 4,232 | 4.37s | 4.30–4.45 |
| django | 2,966 | 2,966 | 4,652 | 11.15s | 10.46–12.69 |

Each repository analyzed 5 times (release build, warm filesystem cache); median reported. Variance is low (±5% for large repos), confirming measurement stability.

The import resolver was refactored from O(N×M) linear scan to O(1) HashMap lookup, reducing django analysis time from 119.4s to 11.3s (10.6× speedup). Remaining time is dominated by tree-sitter parsing (~4ms/file for Python).

### 7.6 RQ4: LCOM4 Cohesion from SCIP

We generated SCIP semantic indices for all 15 corpus repositories using `scip-python` (Docker) and `scip-typescript` (npm), then computed per-module LCOM4 cohesion via bipartite method-field access graphs. Results cover **13,031 class analyses** across 13/15 repos.

| repo | lang | SCIP classes | **y (cohesion)** | SCIP coverage |
|---|---|---|---|---|
| click | Py | 133 | **0.67** | 100% |
| django | Py | 10,054 | **0.66** | 98.4% |
| flask | Py | 115 | **0.63** | 100% |
| fastapi | Py | 673 | **0.62** | 99.6% |
| httpx | Py | 81 | **0.62** | 100% |
| rich | Py | 213 | **0.60** | 100% |
| vitest | TS | 705 | **0.54** | 91.0% |
| chalk | JS | 10 | **0.54** | 38.5% |
| pydantic | Py | 323 | **0.52** | 18.7% |
| commander | JS | 23 | **0.52** | 7.5% |
| date-fns | TS | 105 | **0.51** | 96.4% |
| svelte | TS | 376 | **0.51** | 2.4% |
| requests | Py | 25 | **0.49** | 51.4% |
| worms-supabase | Py | 0 | 0.50* | — |
| lodash | JS | 0 | 0.50* | — |

**Result.** LCOM4 cohesion values suggest meaningful architectural differences:

1. **Python repos cluster higher** (y = 0.49–0.67, median ~0.62) — class-based OOP design with constructors that bridge fields, yielding higher cohesion by LCOM4's definition.

2. **TypeScript/JavaScript repos cluster lower** (y = 0.51–0.54) — more functional style, fewer classes, lighter method-field coupling per class.

3. **date-fns (D = 0.036)** has the smallest observed main-sequence distance *and* moderate cohesion (y = 0.51) — a well-balanced modular architecture.

4. **Function-oriented repos** (lodash, worms-supabase) have zero classes — LCOM4 is not applicable, and OSP correctly reports placeholder cohesion (MetricValue source = Placeholder, confidence = 0.0). This demonstrates the provenance model's epistemological honesty: "we don't know" is never confused with "we measured 0.5."

5. **SCIP coverage varies** (2.4%–100%) — the MetricValue confidence formula (`0.95 × coverage × stale_penalty`) propagates this uncertainty into the coordinate position, ensuring that low-coverage repos (e.g., svelte at 2.4%) contribute proportionally less weight to vision comparisons.

**Caveat.** The observed cohesion distribution suggests a language/paradigm signal (Python higher, TS/JS lower), but several TS/JS repos have low SCIP coverage (svelte 2.4%, commander 7.5%, pydantic 18.7%). High-coverage repos (≥90%: django, fastapi, click, flask, httpx, rich, vitest, date-fns) provide the most reliable signal. A larger corpus with uniformly high coverage would be needed to confirm the language-paradigm hypothesis.

---

### 7.7 RQ5: Token Compression via Epistemic Codec

We benchmarked token-size reduction on a 13-repository subset using three context-transfer strategies: full repository dump, structure-aware 2-hop context, and OSP coordinate prompt. Token counts use the standard `chars / 4` approximation and therefore measure approximate prompt size, not model-specific tokenizer cost.

| repo | full repo tokens | 2-hop tokens | OSP tokens | saving vs full | saving vs 2-hop |
|---|---:|---:|---:|---:|---:|
| chalk | 11.9K | 7.5K | 595 | 94.99% | 92.07% |
| django | 5.3M | 1.7K | 155 | 100.00% | 91.09% |
| vitest | 1.4M | 60K | 6.5K | 99.53% | 89.12% |
| svelte | 1.0M | 8.6K | 1.5K | 99.85% | 82.44% |
| **mean (13 repos)** | — | — | — | **99.53%** | **89.19%** |

**Result.** OSP coordinate prompts reduce architectural context size by **99.53% on average** compared with full repository dumps and by **89.19% on average** compared with a structure-aware 2-hop baseline. The 2-hop baseline is the stronger comparison: it approximates a reasonable context-selection strategy where an agent sends only the target file neighborhood rather than the full repository. Even under this stronger baseline, OSP is roughly an order of magnitude smaller because it transfers coordinate topology instead of source text.

| Statistic | Saving vs Full | Saving vs 2-Hop | OSP Tokens |
|---|---:|---:|---:|
| Median | 99.93% | 89.12% | 155 |
| Q1 (25th pct) | 99.85% | 82.64% | 155 |
| Q3 (75th pct) | 99.98% | 92.07% | 595 |
| IQR | 0.13% | 9.43% | 440 |
| Min | 94.99% | 68.62% | 155 |
| Max | 100.00% | 99.14% | 6,500 |

The tight IQR for savings versus full dump (0.13%) confirms that OSP's compression is consistent across repository sizes. The wider IQR for savings versus 2-hop (9.43%) reflects graph-density variation: densely connected repositories (vitest, svelte) produce larger subgraph slices, but even the minimum saving (68.62%) represents a 3:1 compression.

**Caveat.** This benchmark measures prompt-size compression, not end-to-end agent performance. It uses a chars/4 approximation rather than model-specific tokenizers, and the OSP prompt serialization is intentionally compact: it contains node identifiers, 5-axis coordinates, typed edges, vision thresholds, rules, and output contract, but not raw source code. Future work will measure model-specific tokenization, real LLM calls, task success, and quality/cost trade-offs.

## 8. Related Work

### 8.1 Software Metrics and Architecture Quality

Software metrics have a long history [9, 6]. McCabe's cyclomatic complexity [6] and Halstead's measures established quantitative analysis of code quality. Martin's Clean Architecture [7] introduced Instability (I), Abstractness (A), and the Main Sequence (A + I = 1) — directly informing OSP's coordinate axes. Fenton and Bieman [12] provide the rigorous foundation for software measurement that OSP builds upon; OSP's MetricValue provenance model extends their framework by attaching confidence and coverage to each value, ensuring that measurement limitations are propagated through the analysis pipeline. Tempero and Ralph [9] argue that existing metrics are insufficient for architectural decisions; OSP's coordinate system addresses this by positioning metrics in a navigable space rather than presenting them as isolated scalar values.

### 8.2 Software Visualization

CodeCity [3] and polymetric views visualize software as 3D cities. OSP shares the spatial metaphor but adds physics-like rules (gravity from architectural constraints, deviation angles from vision vectors) and, critically, temporal dynamics via witnessing. CodeCity is observational; OSP's space constrains agents through deterministic claim-based gates (Q4 Syntax, Q5 Vision, Q6 Rule) checked before witness evaluation.

### 8.3 Knowledge Graphs and Graph RAG

GraphRAG [5] generates entity-relation graphs for LLM retrieval. OSP's conceptual space is more structured: typed ontological nodes with gravity functions and explicit time semantics. GraphRAG retrieves; OSP constrains through a deterministic validity filter that rejects negative-space claims before mutation.

### 8.4 Mining Software Repositories

Git workflow analysis identifies review patterns through merge-commit detection [13]. OSP extends this with a tri-state classification that explicitly distinguishes "evidence unavailable" (squash/rebase) from "evidence absent" (genuine absence of review evidence) — a distinction absent in prior work to our knowledge. Amit and Feitelson's Corrective Commit Probability [13] informs OSP's witness-depth metric through commit-quality signal extraction.

### 8.5 Byzantine Fault Tolerance

Dolev-Strong [2] provides authenticated Byzantine agreement with threshold n > f + 1. OSP adapts this threshold to software knowledge commits (Section 5). FLP [10] proves deterministic consensus impossibility in asynchronous systems; OSP's liveness gap (Lemma 2b) is better characterized as an omission fault under partial synchrony [11], resolved by standard mechanisms.

### 8.6 AI Coding Agents and Code Retrieval

Current AI coding agents (e.g., Copilot Workspace, Devin, Cursor) operate on flat text streams and lack a persistent, verifiable model of project architecture. They treat architectural rules as advisory prompts — easily ignored or circumvented when the LLM produces plausible-looking but rule-violating code. OSP is **not a coding agent** but a **state management and gating protocol** that can constrain any such agent: by positioning the agent's output in the conceptual space before mutation, OSP rejects vision-violating or rule-violating claims deterministically (Q4–Q6), independent of the agent's persuasiveness. This provides an explicit pre-mutation validity layer that is generally absent from prompt-only coding-agent workflows.

**GraphRAG** [5] generates entity-relation graphs from source code for LLM retrieval, improving context relevance over keyword search. However, GraphRAG provides no enforcement mechanism — the graph is advisory, not a gate. An LLM can still produce code that violates the graph's implicit constraints. OSP differs by making the graph *actionable*: positions are computed, deviations are measured (θ), and violations are rejected (Q5) before mutation. GraphRAG optimizes retrieval; OSP enforces architectural constraints.

**RepoCoder** [14] uses iterative retrieval-and-generation to improve code completion by feeding model outputs back as context. RepoCoder's context grows dynamically but remains unstructured text — there is no coordinate system, no provenance, and no gate. OSP's coordinate prompt is compact in our benchmark (median 155 tokens) and carries deterministic constraints (vision vector, rules) that RepoCoder lacks, but we do not yet compare against RepoCoder end-to-end.

**SWE-agent** [15] and similar autonomous agents navigate repositories through file-system actions (open, search, edit) guided by LLM reasoning. They achieve impressive task completion rates but provide no architectural safety net: a SWE-agent can produce code that violates dependency rules, introduces circular imports, or drifts from the project's architectural vision without any deterministic rejection. OSP operates at a different layer — it does not replace the agent's file-navigation strategy but constrains the agent's *output* through Q4–Q6 gates before any mutation reaches the objective space.

---

## 9. Discussion

### 9.1 Squash-Merge Blind Spot

The most impactful empirical finding: 8 of 15 analyzed repositories use squash/rebase workflows, making their review activity invisible to merge-commit-based analysis. OSP's tri-state classification is, to our knowledge, the first to explicitly distinguish "unobservable-locally" from "unwitnessed" in the context of mining software repositories [13].

### 9.2 Metric Provenance

MetricValue's source/confidence/coverage model ensures that placeholder metrics (confidence = 0.0) are never confused with measured metrics. This is critical for cross-project comparison: in our corpus, lodash and worms-supabase have zero classes (function-only repos) and correctly report placeholder cohesion (y = 0.50, confidence = 0.0), while click reports LCOM4 cohesion (y = 0.67, confidence = 0.95) from 133 SCIP-analyzed classes. Comparing these without provenance would be epistemologically invalid — "we don't know" (placeholder) must never be conflated with "we measured 0.5" (real).

### 9.3 Evidence Deduplication

The same review event can be recorded as both MergeCommit (1.0) and PRMerged (0.8). Without deduplication, support scores inflate, potentially passing quorum with a single review. OSP's (source, actor, claim) deduplication prevents this systematically.

### 9.4 Token Compression via Epistemic Codec

A key motivation for OSP is reducing the context-transfer cost of LLM-assisted development. Current AI coding agents often operate on flat text streams: source files, selected neighborhoods, or concatenated project context. OSP replaces raw source context with a **typed epistemic projection packet** (`OspPrompt`): a coordinate-based subgraph slice containing node identifiers, 5-axis positions, typed edges, vision thresholds, rules, and output contract.

RQ5 provides an approximate token-size benchmark for this codec. Across 13 repositories, OSP coordinate prompts reduce architectural context size by 99.53% on average compared with full repository dumps and by 89.19% compared with a structure-aware 2-hop baseline. This does not yet prove end-to-end LLM quality improvement: the benchmark uses a chars/4 approximation and measures prompt size rather than task success. However, the result supports the structural claim that OSP can transfer architectural context as compact coordinate topology rather than raw file content.

The deterministic Q4–Q6 gates may further reduce wasted generation by rejecting malformed, vision-violating, or rule-violating proposals before they enter witness evaluation. Measuring real tokenizer counts, actual LLM calls, energy/cost, and task success remains future work.

### 9.5 Hallucination as Epistemic Data

Rather than discarding rejected LLM proposals as errors, OSP classifies them as structured epistemic data. Each gate failure produces a typed `HallucinationType` (Structural, Vision, Rule, Witness, Undersupported) with a calibration message. This "negative space" of architectural failures is valuable for three reasons: (1) it maps the boundary of what an LLM can reliably propose within a given architecture, (2) repeated failures signal either model limitations or non-intuitive framework design, and (3) the failure distribution across language paradigms (Python vs TypeScript) reveals how different coding conventions interact with LLM generation patterns. Future work will explore anonymized telemetry of hallucination patterns as an open dataset for AI safety and software engineering research. Such telemetry must be treated as sensitive: even anonymized failures may leak proprietary architecture, rule sets, or product intent. Any public dataset therefore requires redaction, consent, and project-level privacy controls.

---

## 10. Threats to Validity

**Internal validity.** SCIP coverage varies significantly across repositories (2.4%–100%). Low-coverage repositories (svelte 2.4%, commander 7.5%, pydantic 18.7%) report cohesion values derived from a small subset of classes; their MetricValue confidence (0.95 × coverage) quantifies this uncertainty, but readers should weight these values accordingly. Tree-sitter abstractness detection may miss indirect inheritance chains (e.g., multi-level ABC inheritance in Python).

**External validity.** The 15-repository corpus may not generalize across all language ecosystems, project sizes, or organizational workflows. The 10% merge-ratio threshold for tri-state classification is calibrated on GitHub-hosted Python/TypeScript/JavaScript projects and may differ for other platforms (GitLab, Bitbucket), organizations with formal merge policies, or repositories using unconventional branching strategies.

**Construct validity.** The token benchmark (Section 7.7) measures **architectural context size compression**, not end-to-end task success rate. A smaller prompt does not guarantee better LLM output — the OSP coordinate prompt trades file content for geometric abstraction, which may lose information relevant to specific tasks (e.g., exact variable names, implementation patterns). The chars/4 token approximation does not capture model-specific tokenizer behavior (e.g., GPT-4o vs Claude token boundaries). CosineDeviation is a geometric proxy for architectural deviation with a structural θ_max = 0.5 limit in [0,1]-normalized spaces; Diffusion Distance (future work) may provide better sensitivity beyond this boundary. No comparative baseline against GraphRAG, RepoCoder, or production AI coding agents (Copilot, Devin) has been conducted — our baselines (full dump, 2-hop context) are structural lower bounds, not competitive system comparisons.

**Ethics and privacy.** Hallucination telemetry and architectural failure traces may expose sensitive project structure even when source code is omitted. Any public telemetry dataset requires explicit consent, redaction, and policy controls.

---

## 11. Future Work

1. **Diffusion Distance**: Replace cosine with graph Laplacian-based diffusion for full sensitivity beyond θ = 0.5.
2. ~~**SCIP deployment**~~ → **Done (v2.3).** LCOM4 cohesion computed for 13/15 repos via scip-python (Docker) + scip-typescript (npm). Future: scip-rust and scip-go for Rust/Go repos; multi-run SCIP index stability.
3. **Faz 5 — Agent/LLM OSP Codec**: Typed epistemic projection packets (`OspPrompt` — not natural language), stateless LLM runtime (state lives in Agent shell), `PermissionMask` (trusted-operator assigned, three-point defense in depth), hallucination classification (5 types: structural/vision/rule/witness/undersupported — each with calibration feedback), three-layer space slice engine (Intent-Driven Gravity → Vision/Rules → Permission/Evidence; see Figure 2 in extended version), and hybrid gravity index (static Hard Rules + lazy dynamic Intent+Vision cache).
4. **Custom Axis Marketplace**: Domain-specific physics as signed packages (`security.audit`, `wcag.compliance`, `perf.budget`) — registry-based discovery, calibration sharing, community network effect.
5. **Malicious Witness Detection**: Sybil-resistant witness weighting.
6. **Scale validation**: Test 50k–100k node repositories; integrate KùzuDB if needed.
7. **Lean formalization**: Mechanically verify Theorem 1.
8. **Corpus expansion (50+ repos)**: Extend to Rust, Go, Java, and C# repositories. We propose a 4-category selection per language: (a) stable heavy (e.g., tokio, gin), (b) stable modern (e.g., axum, svelte), (c) AI-era volatile A (e.g., langchainjs, autogpt), (d) AI-era volatile B (early LLM-wrapper repos). This enables testing whether OSP coordinates distinguish structurally mature software from high-star, low-stability AI-era repositories.
9. **Model-specific LLM measurement (RQ6)**: Extend the RQ5 chars/4 benchmark with model-specific tokenizers (e.g., OpenAI and Anthropic), actual LLM calls, task success, latency, and cost/energy measurements. The key question is not only prompt-size reduction, but whether coordinate prompts preserve enough architectural information for successful agent work.
10. **Graph database integration**: KùzuDB or similar for persistent conceptual space storage, enabling Cypher-based structural queries (e.g., "find unwitnessed modules with coupling > 0.8 and rule violations") and incremental recompute without full in-memory graph loading.

---

## 12. Conclusion

OSP transforms software projects from flat text repositories into navigable conceptual spaces. By combining ontological modeling with BFT-inspired witnessing, OSP provides a mathematically grounded framework for AI-assisted development that preserves human sovereignty. The tri-state witness model addresses the squash-merge blind spot affecting 8 of 15 repositories in our corpus; measured abstractness values make Martin's main-sequence distance actionable for cross-project architectural comparison; and LCOM4 cohesion (13,031 classes analyzed via SCIP) suggests that Python repositories cluster higher (y ≈ 0.62) while TypeScript/JavaScript repositories cluster lower (y ≈ 0.52), a language-paradigm signal requiring larger-corpus validation.

Beyond measurement, OSP's typed epistemic codec and deterministic gate architecture provide a compact, verifiable interface between LLM agents and project state. A 13-repository token-size benchmark shows 99.53% average reduction versus full repository dumps and 89.19% versus a structure-aware 2-hop baseline under a chars/4 approximation. This supports the claim that architectural context can be transferred as coordinate topology rather than raw file content. Structured hallucination classification further reframes rejected proposals as epistemic data: not merely errors, but measurements of the boundary between agent belief and project reality.

---

## Appendix A: Full 15-Repository Dataset

| repo | lang | files | nodes | edges | commits | authors | merge% | status | A | A_src | I | D | **y** | cov | time(s) |
|---|---|---:|---:|---:|---:|---:|---:|---|---:|---|---:|---:|---:|---:|---:|
| worms-supabase | Py | 26 | 26 | 17 | 50 | 1 | 0.0% | Unwitnessed | 0.42 | TS | 0.50 | 0.36 | 0.50* | — | 1.0 |
| chalk | JS | 13 | 13 | 11 | 376 | 72 | 5.6% | Unobservable | 0.00 | TS | 0.81 | 0.35 | 0.54 | 38% | — |
| click | Py | 63 | 63 | 61 | 3,242 | 467 | 35.2% | Witnessed | 0.02 | TS | 0.63 | 0.36 | 0.67 | 100% | 1.4 |
| requests | Py | 37 | 37 | 21 | 6,717 | 818 | 24.0% | Witnessed | 0.05 | TS | 0.43 | 0.49 | 0.49 | 51% | — |
| httpx | Py | 60 | 60 | 4 | 1,643 | 263 | 2.5% | Unobservable | 0.07 | TS | 0.50 | 0.45 | 0.62 | 100% | — |
| commander | JS | 159 | 159 | 135 | 1,553 | 205 | 18.3% | Witnessed | 0.00 | TS | 0.81 | 0.16 | 0.52 | 8% | — |
| flask | Py | 83 | 83 | 131 | 5,581 | 873 | 30.9% | Witnessed | 0.01 | TS | 0.71 | 0.34 | 0.63 | 100% | — |
| rich | Py | 213 | 213 | 404 | 4,523 | 310 | 23.7% | Witnessed | 0.04 | TS | 0.71 | 0.36 | 0.60 | 100% | — |
| pydantic | Py | 533 | 533 | 1,016 | 8,353 | 830 | 0.3% | Unobservable | 0.020 | TS | 0.70 | 0.22 | 0.52 | 19% | 13.1 |
| fastapi | Py | 1,125 | 1,133 | 831 | 7,336 | 912 | 0.2% | Unobservable | 0.01 | TS | 0.70 | 0.13 | 0.62 | 100% | 6.9 |
| vitest | TS | 2,235 | 2,236 | 1,881 | 5,995 | 788 | 0.1% | Unobservable | 0.020 | TS | 0.57 | 0.35 | 0.54 | 91% | — |
| date-fns | TS | 1,610 | 1,550 | 3,579 | 2,588 | 442 | 5.9% | Unobservable | 0.05 | TS | 0.93 | 0.02 | 0.51 | 96% | — |
| lodash | JS | 27 | 27 | 0 | 8,494 | 357 | 2.3% | Unobservable | 0.500 | PH | 0.500 | 0.000 | 0.50* | — | — |
| django | Py | 2,966 | 2,966 | 4,659 | 34,704 | 3,408 | 1.7% | Unobservable | 0.00 | TS | 0.66 | 0.18 | 0.66 | 98% | 11.3 |
| svelte | TS | 3,448 | 3,450 | 4,232 | 13,364 | 977 | 11.6% | Witnessed | 0.00 | TS | 0.92 | 0.21 | 0.51 | 2% | 5.1 |

*A_src: TS = tree-sitter (Tier 1, confidence ~0.75), PH = placeholder (no types detected, confidence = 0.0).
**y** = LCOM4 cohesion (1/LCOM4, method-count-weighted average per module). **y\*** = placeholder (0 classes — function-only repo, LCOM4 N/A). **cov** = SCIP coverage ratio (files with SCIP data / total source files). Measured cohesion values from scip-python (Docker) + scip-typescript (npm) indices — 13,031 classes analyzed across 13/15 repos. Timing: release build, 5-run median (warm cache).*

---

## Appendix B: Token Benchmark Results

Token counts use the `chars / 4` approximation. The benchmark compares full repository dump, structure-aware 2-hop context, and OSP coordinate prompt. Full benchmark command: `cargo run --example token_benchmark -- <repo-path>`.

| Repo | Lang | Files | Full Repo Tokens | OSP Tokens | Savings vs Full | 2-Hop Tokens | Savings vs 2-Hop |
|---|---|---:|---:|---:|---:|---:|---:|
| chalk | JS | 13 | 11.9K | 595 | 94.99% | 7.5K | 92.07% |
| requests | Py | 37 | 105K | 155 | 99.85% | 2.8K | 94.53% |
| lodash | JS | 27 | 988K | 155 | 99.98% | 18K | 99.14% |
| click | Py | 63 | 225K | 155 | 99.93% | 3.6K | 95.65% |
| flask | Py | 83 | 152K | 155 | 99.90% | 1.8K | 91.53% |
| commander.js | JS | 159 | 165K | 155 | 99.91% | 1.0K | 85.04% |
| rich | Py | 213 | 449K | 155 | 99.97% | 2.1K | 92.65% |
| pydantic | Py | 534 | 1.9M | 155 | 99.99% | 3.5K | 95.53% |
| fastapi | Py | 1133 | 1.0M | 155 | 99.98% | 893 | 82.64% |
| date-fns | TS | 1610 | 804K | 155 | 99.98% | 494 | 68.62% |
| vitest | TS | 2241 | 1.4M | 6.5K | 99.53% | 60K | 89.12% |
| django | Py | 2968 | 5.3M | 155 | 100.00% | 1.7K | 91.09% |
| svelte | TS | 3451 | 1.0M | 1.5K | 99.85% | 8.6K | 82.44% |
| **Mean** | — | — | — | — | **99.53%** | — | **89.19%** |

The OSP prompt includes 5-axis coordinates, typed edges, vision thresholds, rules, and output contract, but excludes raw source code. These results should be interpreted as prompt-size compression; task success and model-specific tokenizer behavior require a separate benchmark.

---

## References

[1] Lamport, L., Shostak, R., & Pease, M. (1982). The Byzantine Generals Problem. *ACM TOPLAS* 4(3).

[2] Dolev, D. & Strong, H.R. (1983). Authenticated algorithms for Byzantine agreement. *SIAM J. Comput.* 12(4).

[3] Wettel, R. & Lanza, M. (2007). Visualizing Software Systems as Cities. *VISSOFT*.

[4] Hogan, A. et al. (2021). Knowledge Graphs. *ACM Computing Surveys* 54(4). arXiv:2003.02320.

[5] Edge, D. et al. (2024). From Local to Global: A Graph RAG Approach. arXiv:2404.16130.

[6] McCabe, T. (1976). A Complexity Measure. *IEEE TSE*.

[7] Martin, R.C. (2017). *Clean Architecture*. Pearson.

[8] Cohen-Steiner, D., Edelsbrunner, H. & Harer, J. (2006). Stability of persistence diagrams. *Discrete & Computational Geometry*.

[9] Tempero, E. & Ralph, P. (2026). Making Software Metrics Useful. arXiv:2603.16012.

[10] Fischer, M.J., Lynch, N. & Paterson, M. (1985). Impossibility of distributed consensus with one faulty process. *JACM* 32(2).

[11] Chandra, T.D. & Toueg, S. (1996). Unreliable failure detectors for reliable distributed systems. *JACM* 43(2).

[12] Fenton, N.E. & Bieman, J. (2014). *Software Metrics: A Rigorous and Practical Approach* (3rd ed.). CRC.

[13] Amit, I. & Feitelson, D.G. (2020). The Corrective Commit Probability. arXiv:2007.10912.

[14] Zhang, F., et al. (2023). RepoCoder: Repository-Level Code Completion Through Iterative Retrieval and Generation. EMNLP.

[15] Yang, J., et al. (2024). SWE-agent: Agent-Computer Interactions Enable Software Engineering Language Models. arXiv:2405.15793.

---

*Paper draft v2.6 · OSP project · 2026-06-24 · All data reproducible from `docs/` and `crates/` · LCOM4 cohesion from SCIP deployment (13,031 classes) · Token benchmark from `token_benchmark` (13 repos, chars/4 approximation)*
