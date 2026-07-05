# Concept Anchoring: From Human Sentences to Bound Project Work

**OSP Paper 3 Draft v1.0 (skeleton)** · Target: arXiv then ACM TOSEM
**Authors:** Volkan ER
**Date:** 2026-07-05
**Companion papers:** *Ontological Space Protocol: Modeling Software as a Conceptual Space with Epistemological Witnessing* (Paper 1, v2.6 — static space); *Architectural Trajectory Navigation: From Target Coordinates to Measurement Predicates* (Paper 2, v1.2 — dynamic, agent-driven). This paper establishes the **genesis layer** that precedes both: how a human sentence becomes bound, accepted, measured project work.
**Revision:** skeleton (Aşama 2). Evidence frozen at commit `481690d` (hash₁); metadata at `8431c9e` (hash₂). Review D1/D2 düzeltmeleri uygulandı; Aşama 3 (bölüm dolgu) sonraki.

---

> **Methodological note (review içgörüsü):** *Test altına alınmayan invariant ihlal edilir.*
> Bu makale sadece ontological binding chain'i sunmaz — kanıt dosyalarının kendisi de
> yapısal olarak doğrulanabilir olmalıdır (pre-flight canonical tablosu, §0/Appendix A).

## Abstract

A human sentence may introduce project intent, but it does not by itself create project work. [TODO ~350 kelime: problem → approach → evidence. D2-6 hedef.]

## 1. Introduction

[TODO: Paper 1/2'den ayrım — "static space" ve "dynamic navigation" öncesi "genesis layer".]

### 1.1 The Problem

Embedding-first anchoring (RAG, GraphRAG) proposes candidate proximity but cannot decide ontological role, authority, acceptance, or executability. [D2-1: "anti-RAG" → "embedding-first anchoring eleştirisi".]

### 1.2 Our Approach: Ontological Binding Chain

A sentence becomes project work only by traversing a type-enforced binding chain: candidate isolation → operator acceptance → predicate lowering → operator binding → task genesis. Each gate is type-level enforced (INV-C1..C8, INV-P1..P3).

### 1.3 Contributions

This paper makes four contributions:

1. **Genesis ontology with candidate isolation (Section 3).** A human sentence produces a *candidate* — never accepted, never executable. INV-C1..C8 type-level enforcement. *"Embedding proposes; it does not decide."*
2. **Predicate lowering — a rule is not a predicate (Section 4).** A `RuleCandidate` lowers to a `PredicateStub` (structured uncertainty), never an `ExecutablePredicateSet`. INV-P1. *"The keyword matcher is not the contribution. The contribution is the binding protocol."*
3. **Cross-family translation semantics (Section 5).** Concept→Physical translation preserves candidate meaning (INV-P3); binding alone creates commitment. [D2-3: §4 kısa, §5 asıl epistemik argüman.]
4. **Three-gate task genesis (Section 6).** Accepted intent + operator-bound predicate + operator capability → `trajectory::Task`. INV-P2 (keyword ≠ executable), INV-T2 (capability-gated genesis).

### 1.4 Terminology mapping [D1-5]

| Term (this paper) | OSP core type | Paper 1/2 connection |
|---|---|---|
| Candidate | `ConceptNode` (DecisionStatus::Candidate) | pre-Trajectory |
| Accepted | `ConceptNode` (DecisionStatus::Accepted) | pre-Task intent |
| Task | `trajectory::Task` | Paper 2 navigator input |

## 2. Motivating Example

*"The sentence never becomes a task by itself."*

End-to-end binding chain replay: `"Coupling must not exceed module threshold."` → RuleCandidate → PredicateStub → CrossFamilyHint → operator binding → ExecutablePredicateSet → verify → Task. Evidence: `e2e-binding-chain-replay.json` (commit `481690d`). Adım 1 gerçek pipeline koşusu; Adım 6 acceptance INV-C3 gereği seeded (in-crate enforced).

## 3. Genesis Ontology (INV-C1..C8)

4-column table: invariant → type-level enforcement → what it prevents → evidence. [TODO: INV-C1 embedding-proposes, C2 family, C3 candidate-isolation, C4 supersede-authority, C5 inferred-not-accepted, C6 code-intent-hypothesis, C7 explainable, C8 canonicalized.]

## 4. Predicate Lowering (INV-P1)

*"A rule is not a predicate. A predicate is a rule whose measurable slots have been bound."*

RuleCandidate → PredicateStub (structured uncertainty: unresolved slots + suggested templates + cross-family hint). Never ExecutablePredicateSet. [D2-3: kısa bölüm.]

## 5. Cross-Family Translation (INV-P3)

*"Translation preserves candidate meaning; binding alone creates commitment."* [D2-3: asıl epistemik argüman burada.]

CrossFamilyHint: ambiguity computed (SingleCandidate / MultipleCandidates / NoAxisCandidate). KeywordMatch + LanguageAlias sources. Deterministic normalize (NFC → TR fold → ASCII lowercase). [TODO: detay.]

## 6. Binding & Task Genesis (INV-P2, INV-T2)

Three-gate API: (1) verify_accepted_task_candidate, (2) bind_metric_threshold (OperatorCapability), (3) create_task_from_accepted_candidate (OperatorCapability). *"Accepted intent ≠ executable work."*

## 7. Verification Evidence

*"A gate that only passes is indistinguishable from no gate. These paths prove the gates reject."* [D2-4: §7 = "Verification Evidence".]

### 7.1 Type-level trybuild (stratum 1)
11 Paper 3'e özgü (kümülatif 18 bağlam). `tests/anchoring_typelevel.rs`.

### 7.2 Golden fixture conformance (stratum 2)
13 fixture, 5-state: Conform 9, PartialConform 2, RejectAsExpected 2. `conformance-results.json`.

### 7.3 Held-out adversarial (stratum 3)
5 fixture (4 held_out + 1 regression_anchored). Conform 3, KnownLimitation 2. `held-out-adversarial-fixtures.json`.

### 7.4 End-to-End Binding-Chain Replay (stratum 4)
`e2e-binding-chain-replay.json`. Adım 1 gerçek pipeline; Adım 6 INV-C3 seeded.

### 7.5 End-to-End Rejected Paths Replay (stratum 5)
`e2e-rejected-paths-replay.json`. 4 yol: AxisMismatch, AxisNotInCandidates, TemplateNotSuggested, NotAccepted.

### 7.6 What this does not evaluate
Self-authored gold standard, keyword matcher placeholder, stub provider, Faz 8 operator console (acceptance seeded). [TODO: §10'a da akar.]

## 8. Related Work

[TODO ~6: requirements traceability, CNL, GraphRAG, program analysis, AI agents, P1+P2.]

## 9. Discussion

### 9.1 Ontological binding vs embedding
*"Embedding can propose candidate proximity, but it cannot decide ontological role, authority, acceptance, or executability."*

### 9.2 Determinism-first discipline
Mekanizma önce deterministic kanıtlanır (Faz 1), stochastic katmanlar sonra (Faz 6/7).

### 9.3 Storage is not epistemology [D2-5]
KuzuDB (Ekim 2025 arşiv) story kısa: persistence epistemic gate'i zayıflatmaz. *"Persistence does not weaken epistemic gates."*

### 9.4 Fixture design must be verifiable (review içgörüsü)
4 review turunda yakalanan hataların hepsi iki sınıfa indi: (1) kısıt-yayılmaması (canonical-kesme, marker-kaçırma), (2) iddia-implementasyon ayrışması. Pre-flight canonical tablosu (§0/Appendix A) her iki sınıfı yapısal imkânsız kılar. *"Test altına alınmayan invariant ihlal edilir"* — bu makalenin kendi kanıt dosyalarına uyguladığı metodolojik ilke.

## 10. Threats to Validity

- **Self-authored gold standard** — 13 fixture'ı makale yazarı yazdı (D1-2). Held-out set (5) totoloji-olmayan kanıt ama yine self-authored.
- **Keyword matcher placeholder** — INV-P1 lowering canonical keyword taraması yapar; semantik false-positive üretür (held_002 "couplings in pipe assembly" → YANLIŞ Coupling hint). *"The deterministic matcher can confuse lexical coupling with software coupling. The matcher is not the contribution; the binding protocol is."*
- **Coarse classifier** — negasyon yakalayamaz (held_003), Decision: typed-prefix parse edemeyebilir (fix_007). Faz 6 calibration.
- **Stub provider** — code evidence in-memory, gerçek SCIP entegrasyonu Faz 4 sonrası.
- **Acceptance seeded** — INV-C3 OperatorAcceptance pub(crate) → integration test promote yapamaz, acceptance state seeded. Faz 8 operator console gerçek API.

## 11. Future Work

- Faz 5.2/5.3: MetricDelta + EvidenceRequired + RelationExists executable
- Faz 6: Concept Synthesis (code repo → concept hipotezleri)
- Faz 7: Embedding + LLM-assisted candidate generation
- Faz 8: Desktop integration (Project Reality Cockpit) + operator console (real INV-C3 promote)

## 12. Conclusion

*"Words do not mutate project reality. Only bound, accepted, measured structures can."*

---

## Appendix A: Pre-flight Canonical + Marker Tablosu

*"Test altına alınmayan invariant ihlal edilir."* — 4 review turunda 3 kez yakalanan canonical-kesme tuzağı (A1→B1→B5) + 1 marker-kaçırma tuzağı yapısal imkânsız kılındı.

| Cümle | Canonical (ilk 3 kelime) | Normalize | Rule signal | Ambiguity | Axes |
|---|---|---|---|---|---|
| Coupling must not exceed module threshold. | CouplingMustNot | couplingmustnot | true (must not) | SingleCandidate | [Coupling] |
| The couplings in the pipe assembly must not be reused. | TheCouplingsIn | thecouplingsin | true (must not) | SingleCandidate | [Coupling] (YANLIŞ — fiziksel boru) |
| Modüller arası bağımlılık azaltılmalı. | ModüllerArasıBağımlılık | modullerarasibagimlilik | true (malı) | SingleCandidate | [Coupling] (via bagiml alias) |
| Coupling rule must not be enforced during tests. | CouplingRuleMust | couplingrulemust | true (must not) | SingleCandidate | [Coupling] |
| Coupling and cohesion must not diverge. | CouplingAndCohesion | couplingandcohesion | true (must not) | MultipleCandidates | [Coupling, Cohesion] |
| Witness count must not create metric evidence. | WitnessCountMust | witnesscountmust | true (must not) | NoAxisCandidate | [] (bare witness excluded) |

Tablo `paper3_evidence.rs::preflight_canonical_and_rule_signal_for_paper3_evidence_sentences` testinde gerçek pipeline koşusuyla pinlenmiştir. Gelecekte fixture ekleyen herkes aynı ağa takılır.

## Appendix B: End-to-End Binding-Chain Replay

`e2e-binding-chain-replay.json` (commit `481690d`). 8 adım: sentence → RuleCandidate (REAL pipeline) → PredicateStub → CrossFamilyHint → operator binding → ExecutablePredicateSet → verify accepted (SEEDED, INV-C3) → create task → registry.

## References

[TODO ~12-15, numeric [N]: requirements traceability, CNL, GraphRAG, program analysis, AI agents, P1/P2.]
