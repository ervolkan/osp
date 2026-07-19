# INV-T9 — External-Evidence Suspension Isolation

**Conformance fix:** Paper 2 model–implementation gap
**Branch:** `fix/inv-t9-witness-suspension`
**Status:** implemented (awaiting eligible independent review — GOVERNANCE §3 high-risk)
**Date:** 2026-07-16

---

## 1. Model expectation

Paper 2 separates four epistemically distinct outcomes:

```
proposal validity  ≠  predicate completion  ≠  witness authorization  ≠  mainline commit
```

The navigator pseudocode in Paper 2 treats `RequireOperatorApproval` as a terminal
escalation — only genuine `Reject` (agent-correctable structural failure) enters the
retry loop. Paper 1 treats insufficient quorum as `Hold` (not "claim wrong", but
"insufficient epistemic evidence to commit yet").

**Expected control flow:**

```
predicate satisfied → deterministic gates passed → witness quorum insufficient
                    → Hold / AwaitingWitnesses (NOT Reject, NOT retry)
```

## 2. Previous implementation behavior

```
WitnessResult::Hold(reason) → Err(EngineCommitError::Witness(reason))
                            → navigator generic Reject path
                            → retry loop
                            → same proposal re-produced N times
                            → ExceededManeuverLimit
```

The implementation classified an **external-evidence shortage** as an
**agent-correctable structural failure**. `Reject` carried too many meanings:

```
Reject = malformed proposal
       OR predicate unsatisfied
       OR axis regression
       OR rule violation
       OR insufficient witnesses    ← wrong category
       OR witness rejection          ← wrong category
```

## 3. Identified gap

> The implementation classified an external-evidence shortage as an agent-correctable
> structural failure, consuming maneuver budget (INV-T7 misuse) and producing
> `ExceededManeuverLimit` for what is actually a suspended-authorization state.

This is not a demo behavior gap — it is a **Paper 2 model–implementation conformance
gap**. The canonical demo cannot ship with the wrong behavior normalized.

## 4. Normative correction

**INV-T9 — External-Evidence Suspension Isolation** (Model B):

> Once an attempt has passed all deterministic gates and produced a mutation decision
> requiring external authorization, insufficient external evidence or witness quorum
> MUST transition the claim to a suspended authorization state. It MUST NOT:
> - initiate a new agent attempt,
> - consume additional maneuver budget,
> - invoke the LLM again,
> - mutate the engine space,
> - invoke project-space mutation persistence or apply the structural delta.
>
> It MUST persist only the pending-authorization suspension record (via the injected
> `PendingAuthorizationStore`), atomically published as a crash-consistent resumption
> artifact, BEFORE the suspended result is returned.

**INV-T7 cross-reference (maneuver-budget scope clarified):**

> Only outcomes that require a new structural proposal from the agent consume maneuver
> budget. Permission, persistence, internal, and invalid-witness-evidence failures do
> not consume maneuver budget (terminal). Vision violations are retryable and consume
> budget. External-authorization suspension is governed by INV-T9.

## 5. Implementation evidence

### Domain outcome ≠ operational fault

```rust
pub enum EngineCommitResult {
    Applied(TaskCommitResult),                                    // AcceptAsCompleted + AcceptAsProgress
    Held { reason: WitnessHoldReason, snapshot: WitnessQuorumSnapshot },
    Rejected { reasons: NonEmptyWitnessRejections, snapshot: WitnessQuorumSnapshot },
}

pub enum EngineCommitError {
    SyntaxViolation { .. },      // retryable
    VisionViolation { .. },      // retryable
    RuleViolation { .. },        // retryable
    InvalidWitnessEvidence(..),  // terminal — malformed/author-self/duplicate
    PermissionDenied(..),        // terminal
    NoPersistence,               // terminal
    Persistence(..),             // terminal
    Internal(..),                // terminal
}
```

`Hold` and `Rejected` are **expected domain outcomes**, not errors. Engine `commit_task_claim`
returns `Result<EngineCommitResult, EngineCommitError>`.

### Single canonical witness model

`WitnessDisposition` (Satisfied/Held/Rejected) is the single TimeFSM output type.
`WitnessResult` is a deprecated type alias — no wrapper/conversion chain. `WitnessHold`
struct does not exist. `WitnessHoldReason`:
- `MinApproversNotMet` (Q1)
- `QuorumInsufficient` (Q2)
- `EvidenceNotLocallyObservable` (inv #3 tri-state — NOT invalid evidence)

### Authorization basis (BLAKE3, domain-separated, canonical)

`AuthorizationBasisDigest` — BLAKE3 with `"osp.authorization-basis.v1\0"` domain
separation. Full canonical structural delta (not a lossy digest). Predicate content
always bound (id alone insufficient). `EvaluationContextDigest` covers the claim-specific
effective vision-gate context, the ordered rule-evaluation context, and their semantics
versions. `SpaceViewRevision` is store-scoped + lane-qualified.
Float canonicalization: NaN rejected, -0.0 normalized, little-endian, sorted
collections, `f64::to_bits()`. `created_at` NOT in digest. `Clock` trait
(`SystemClock`/`FixedClock`) — core never calls `SystemTime::now()` directly.

### Canonical decision-basis layers (Step 4a/4b/4c/5 + Step 6 golden vectors)

The basis digest encoding evolved across INV-T9 Steps 4a–5; Step 6 golden vectors
lock the resulting v1 byte contract.

- **Step 4a — Rule sequence binding:** `RuleEvaluationContext` (ordinal-aware
  `pub(crate)` snapshot) is shared by Q6 (`check_claim_rules_with_context`) and the
  digest. Registration order is semantically significant (first-match short-circuit);
  the digest encodes ordinals, not sorted `rule_id`. `RULE_EVALUATION_SEMANTICS_VERSION`.
- **Step 4b — Claim-specific effective vision:** `EffectiveVisionGateContext` binds the
  **effective** vision for a claim (subject + effective vector + source from one decision
  tree), not the global vision or all role overrides. Authority validation:
  `None → VisionUnavailable`, `GlobalDefault → VisionAuthorityInsufficient`,
  subject/source mismatch rejected. Captured-context pattern: one production, shared by
  Q5 + `build_authorization_context` + digest.
- **Step 4c — EvaluationContextDigest cleanup:** `EngineConfig` parameter removed from
  `compute`. Digest binds only Q5 vision-gate + Q6 ordered-rule inputs + semantics
  versions. Removed: `min_approvers`/`quorum_threshold` (in `CanonicalWitnessPolicy`),
  `milestone_interval` (persistence cadence), `abstractness` (post-apply derived
  position; not axis measurement), `merge_ratio_observable` (digest filler).
- **Step 5 — Defensive structural-delta integrity:** `CanonicalEdgeIdentity`
  (from,to,kind — `is_type_only` excluded from removal identity). Private fields +
  custom Deserialize (`deny_unknown_fields`). Identity-based duplicate/cross-list
  conflict detection (`is_type_only`-independent). Non-normalizing `validate()` +
  as-is digest encoder (single canonicalization in `try_new`). Typed error taxonomy
  (`DuplicateNodeId` vs `UnsortedNodes`; `DuplicateEdge` vs `UnsortedNewEdges`/
  `UnsortedRemovedEdges`).

### v1 byte contract (Step 6 golden vectors + #72 evidence digest)

Step 6 golden vectors (`authorization_basis_digest_v1_golden_vector`,
`evaluation_context_digest_v1_golden_vector`) **establish and lock** the first
compatibility-supported v1 byte contract for the currently defined canonical models.
#72 adds `suspended_attempt_evidence_digest_v1_golden_vector` for the new
domain-separated evidence digest. The expected values (non-normative mirror —
executable normative values are the test constants):

| Digest | Domain separator | v1 golden hex |
|--------|------------------|---------------|
| `AuthorizationBasisDigest` | `osp.authorization-basis.v1\0` | `7f67f2acf97bc9747b9f708437eb6a3454628f3cb4c23541e48e00554a4945f5` |
| `EvaluationContextDigest` | `osp.evaluation-context.v1\0` | `b2e7e883e0af8bdbff02e691d39f1574caaeb6be9d1a29e8467a3b99d79f1a5f` |
| `SuspendedAttemptEvidenceDigest` (#72) | `osp.attempt-evidence.v1\0` | `3cfb984502df3382fec90111b5afd19a5d6543c071c98ba6c3fc3f7a0fe0052c` |

**Byte contract vs runtime semantic correctness:** Golden vectors lock the canonical
byte encoding of the currently-defined v1 models. They do **not** prove runtime data is
correctly produced. #70 (EngineMeasurement pipeline — per-axis provenance,
engine-issued measurement) remains required for runtime semantic correctness. If #70
changes the runtime *production path* without changing field/encoding, the v1 byte
contract is preserved; if it changes fields or encoding, golden mismatch surfaces a
pre-release v1 revision / v2 decision.

Breaking changes (canonical field/order/tag/encoding) after this lock require an
explicit v2 domain-separator decision (`osp.authorization-basis.v2\0` /
`osp.evaluation-context.v2\0` / `osp.attempt-evidence.v2\0`).

**#72 schema finalize:** `osp.pending-authorization.v1` schema adds
`suspended_attempt_evidence` + `evidence_digest` fields to `PendingAuthorization` and
`suspended_attempt_evidence` to `RevisionRequired`; the dangling `attempt_evidence_id`
field is removed from both. PR #69 is pre-merge, so this is the first correct v1
finalize (no v2 bump required — the change is documented and the schema was not yet
compatibility-supported).

### Pending authorization (Model B + Sabitleme 1)

`PendingAuthorization` carries predicate completion, mutation decision, intended apply
target, authorization basis digest, base space-view revision, evaluation context
digest, witness requirement, **witness hold reason** (Sabitleme 1), witness snapshot,
attempt num (AttemptNumber, 1-based), embedded `SuspendedAttemptEvidence` snapshot,
evidence digest, created_at. All authorization-gated mutations covered
(AcceptAsCompleted + AcceptAsProgress).

**#72 closure:** The dangling `attempt_evidence_id` field is removed; `attempt_num`
(AttemptNumber) replaces it as trajectory sequence only (not an evidence lookup key).
Embedded `SuspendedAttemptEvidence` + `evidence_digest` are the source of truth.

### Self-contained artifact (Sabitleme 3)

`PendingAuthorizationEnvelope` embeds the full canonical `AuthorizationBasis` alongside
the digest. `verify()` recomputes the digest on load and rejects mismatches (tamper /
corruption detection). Single canonical schema string `"osp.pending-authorization.v1"`
(no separate schema_version in record).

**#72 (Implemented — awaiting scoped review):** The envelope is now self-contained
w.r.t. **both** the authorization basis **and** the attempt evidence. Complete embedded
attempt-evidence integrity is established (closure commit landed; issue OPEN pending
scoped review):
- Canonical `SuspendedAttemptEvidence` snapshot (common header + tagged disposition enum)
  is embedded in `PendingAuthorization` and `RevisionRequired`.
- **Closure (P0-1):** `RevisionRequired` minimal shape — only `evidence_digest` +
  `suspended_attempt_evidence`; no outer duplicate fields. Accessors via evidence.
- **Closure (P0-1 load/creation ayrımı):** `Envelope::new()` = creation (digest
  compute + write); `try_new_with_verified_digests()` = load (stored digest preserved,
  recompute + compare). Tampered digest on load no longer "repaired".
- **Closure (P0-2 strict wire):** `PendingAuthorization`, `PendingAuthorizationEnvelope`,
  `RevisionRequired`, `SuspendedAttemptEvidence`, `SuspendedAttemptDisposition` use
  custom Deserialize + `deny_unknown_fields`. Stale `attempt_evidence_id` field rejected.
- **Closure (P0-2 persist-boundary):** `persist()` calls `verify()` before any side
  effect; invalid envelope cannot reach disk (`InvalidEnvelope` error).
- **Closure (P0-3 single sort key):** `canonical_rejection_key` is the single source
  for constructor canonicalization, wire strict check, digest encoding, duplicate
  detection. No Rust-tuple vs lexicographic-byte drift.
- **Closure (P1-1 API normalize / wire strict):** Production API `try_new` normalizes
  arbitrary input; wire load `try_from_canonical_wire` strictly rejects non-canonical
  order (no silent normalization).
- **Closure (P1-2 evidence constructor validation):** `validate_evidence_semantics`
  in constructor — Held hold_reason↔snapshot, Rejected snapshot finite/non-neg +
  canonical + duplicate-free. Envelope `verify()` defensive replay.
- **Closure (P1-3 record-internal vs envelope verification):** `validate_internal`
  (record↔evidence) separate from envelope `verify()` (record↔basis).
- Domain-separated `SuspendedAttemptEvidenceDigest` (`osp.attempt-evidence.v1\0`).
- Typed mismatch errors (P1): each invariant violation produces a distinct error
  variant for diagnostic clarity.
- Surface-specific disposition: `PendingAuthorization` only Held, `RevisionRequired`
  only Rejected.

**Held/Rejected verification scope:**
- Held evidence propagation is verified end-to-end through the navigator (when the
  fixture produces Held).
- Rejected evidence construction is verified through the production mapper
  (`revision_required_from_rejection`) used by the navigator's Rejected arm
  (`rejected_mapper_constructs_canonical_revision_evidence` test).
- Upstream production reachability of the Rejected arm is currently absent because
  witness Q3 honest-reject signaling is not wired; this is tracked separately in #73.
- Deterministic Held fixture (predicate satisfied → Held) is a fixture-design gap;
  current fixtures may produce ExceededManeuverLimit/NoMoreProposals instead. This
  gap is documented and not silently treated as evidence production success.

### Navigator-owned persistence (P0-1)

`PendingAuthorizationStore` trait + `FilesystemPendingAuthorizationStore`. Navigator
calls `persist()` BEFORE returning `AwaitingWitnesses` — no external suspended result
without a published artifact. No `AwaitingWitnesses` result is externally returned
unless its pending artifact has first been successfully published.

**#72 (artifact identity migration):** Artifact filename now uses evidence identity —
`task-{task_id}--claim-{claim_id}--attempt-{attempt_num}--{evidence_digest}.json`.
The same basis with different evidence produces distinct artifacts (no false conflict).
Receipt carries `task_id`, `attempt_num`, `evidence_digest` alongside `claim_id` and
`authorization_basis_digest`.

No-clobber (create_new): silent overwrite forbidden. Idempotent: same evidence digest +
same content → success; same evidence path + different content → BasisConflict; same
basis + different evidence digest → separate artifact. Crash-consistent publish:
same-dir temp → write_all → sync_all → atomic no-clobber rename.

### Exhaustive navigator mapping (no catch-all)

```rust
Ok(EngineCommitResult::Applied(result)) => { /* Completed/Progress */ }
Ok(EngineCommitResult::Held { .. }) => { /* persist → AwaitingWitnesses */ }
Ok(EngineCommitResult::Rejected { .. }) => { /* RequiresRevision */ }
Err(EngineCommitError::SyntaxViolation { .. }) => { /* RetryAgent + calibration */ continue; }
Err(EngineCommitError::VisionViolation { .. }) => { /* RetryAgent + calibration */ continue; }
Err(EngineCommitError::RuleViolation { .. }) => { /* RetryAgent + calibration */ continue; }
Err(EngineCommitError::InvalidWitnessEvidence(..)) => { /* terminal */ }
Err(EngineCommitError::PermissionDenied(..)) => { /* terminal */ }
Err(EngineCommitError::NoPersistence) | Err(EngineCommitError::Persistence(..)) => { /* terminal */ }
Err(EngineCommitError::Internal(..)) => { /* terminal */ }
```

Budget isolation: Held/Rejected/terminal paths have no `continue`. Authorization
waiting consumes no additional maneuver budget (proposal generation counts once).

### `RevisionRequired` evidence preservation (#72 Implemented — awaiting scoped review)

`NavigatorResult::RequiresRevision(RevisionRequired)` carries task_id, claim_id,
authorization basis digest, witness reasons (NonEmpty), witness snapshot, and the
canonical `SuspendedAttemptEvidence` snapshot (Rejected disposition). Embedded evidence
+ domain-separated digest closes the attempt-evidence + basis binding loss on the
Rejected path.

**Scope (P1 daraltma):** Full `AuthorizationBasis` reconstruction on the Rejected path
remains a separate concern (depends on an embedded/persisted basis surface); this
struct carries the evidence snapshot and digest binding, not the full basis. The
dangling `attempt_evidence_id` field is removed — `attempt_num()` accessor retrieves
the trajectory attempt number via the embedded evidence.

## 6. Test evidence

### INV-T9 pozitif (14)
```
predicate_complete_without_quorum_returns_awaiting_witnesses
progress_checkpoint_witness_hold_returns_awaiting_witnesses (Model B)
awaiting_witnesses_does_not_reinvoke_llm
awaiting_witnesses_does_not_apply_mainline_mutation
held_outcome_does_not_mutate_engine_space
held_outcome_does_not_call_persistence_apply
awaiting_witnesses_preserves_authorization_basis
awaiting_witnesses_records_exactly_one_structural_attempt
quorum_shortage_never_returns_exceeded_maneuver_limit
pending_artifact_is_persisted_before_awaiting_result_is_returned
pending_artifact_failure_returns_non_retryable_persistence_failure
pending_artifact_failure_does_not_reinvoke_llm
pending_artifact_failure_does_not_mutate_space
pending_authorization_preserves_witness_hold_reason (Sabitleme 1)
```

### INV-T7 korunma (6)
```
syntax_rejection_consumes_maneuver_budget
predicate_near_miss_consumes_or_advances_according_to_policy
retryable_rejection_still_reinvokes_llm
maneuver_limit_still_bounds_structural_retries
accept_as_progress_behavior_is_unchanged
vision_violation_reinvokes_llm_and_consumes_maneuver_budget (Sabitleme 2)
```

### Error taxonomy (4)
```
permission_denied_does_not_reinvoke_llm
permission_denied_does_not_consume_maneuver_budget
persistence_failure_does_not_consume_maneuver_budget
invalid_witness_evidence_is_terminal_not_retry
```

### Witness sınıflandırma (9)
```
min_approvers_not_met_is_hold
quorum_insufficient_is_hold
evidence_not_locally_observable_is_hold_not_invalid
explicit_witness_rejection_is_not_hold
duplicate_witness_is_not_counted_as_second_approver
author_vote_does_not_satisfy_independent_approver_requirement
invalid_witness_evidence_is_not_requires_revision
explicit_rejection_preserves_witness_snapshot_and_claim_identity
rejected_witness_reasons_are_non_empty
```

### Artifact idempotency + schema + basis (8)
```
pending_artifact_is_idempotent_for_identical_basis
pending_artifact_never_silently_overwrites_different_basis
pending_artifact_filename_uses_validated_ids_only
failed_artifact_write_leaves_no_partial_visible_record
pending_authorization_rejects_unknown_schema_version
pending_artifact_contains_authorization_basis (Sabitleme 3)
pending_artifact_recomputes_matching_basis_digest (Sabitleme 3)
pending_artifact_rejects_basis_digest_mismatch (Sabitleme 3)
```

### Digest canonicalization (11)
```
authorization_basis_digest_uses_domain_separation
normalizes_negative_zero
rejects_nan
is_order_independent_for_set_fields
changes_when_witness_policy_changes
changes_when_base_lane_changes
changes_when_claim_changes
changes_when_rule_set_changes
changes_when_vision_policy_changes
changes_when_predicate_content_changes_even_if_id_is_same
is_stable_for_identical_claim
```

### Continuity (3)
```
pending_authorization_round_trips_through_serde
carries_base_space_view_revision
pending_record_contains_everything_required_for_future_resume
```

### Legacy fixture
> The legacy reproduction fixture (osp-mcp `inv_t1_submit_delta_outcome_has_no_target_coordinate`)
> is retained, but its expected result changes from `ExceededManeuverLimit` (via legacy
> `attempt_outcome` reject JSON) to `Held` (`commit_result: Held` + `commit_state:
> awaiting_witnesses`). The test permanently asserts that the previous behavior does
> not recur.

## 7. Failure-class decision table

| Predicate | Deterministic gates | Witness | Result | LLM retry? | Budget |
|-----------|---------------------|---------|--------|-----------|--------|
| fail | pass | not evaluated | retry/reject | yes | +1 |
| complete | pass | quorum hold | AwaitingWitnesses | no | +0 extra |
| complete | pass | explicit reject | RequiresRevision | no | +0 extra |
| complete | pass | quorum reached | Completed | no | +0 |
| syntax violation | — | — | RetryAgent | yes | +1 |
| vision violation | — | — | RetryAgent (Sabitleme 2) | yes | +1 |
| rule violation | — | — | RetryAgent | yes | +1 |
| predicate near-miss | — | — | per policy | per policy | per policy |
| permission denied | — | — | PermissionFailure (terminal) | no | +0 |
| persistence failure | — | — | SystemFailure (terminal) | no | +0 |
| internal failure | — | — | SystemFailure (terminal) | no | +0 |
| invalid witness evidence | — | — | WitnessEvaluationError (terminal) | no | +0 |

## 8. Compatibility / migration impact

This is an **API-level breaking change** (semantic correctness does not imply
non-breaking):

- **JSON schema:** `osp.trajectory-attempt.v1` (new). Legacy `attempt_outcome`
  reject JSON replaced by `commit_result: Held/Rejected` + `commit_state` +
  `next_action` for authorization outcomes.
- **`NavigatorResult`:** exhaustive match in downstream crates breaks — new
  `AwaitingWitnesses` + `RequiresRevision` variants. Callers updated: osp-cli,
  osp-mcp, osp-analyzer g2c example.
- **`EngineCommitResult`:** `commit_task_claim` return type changed from
  `Result<TaskCommitResult, EngineCommitError>` to
  `Result<EngineCommitResult, EngineCommitError>`. Callers updated: navigator,
  osp-mcp submit_delta_attempt.
- **`WitnessResult`:** now deprecated type alias for `WitnessDisposition`. Old
  `Commit/Hold/Reject` variants removed; use `Satisfied/Held/Rejected`. Migration:
  mechanical rename.
- **`EngineCommitError::Witness(Reason)`:** REMOVED. Legacy `commit()` (standalone/Paper 1)
  Held/Rejected now returns `Internal` error (use `commit_task_claim` for INV-T9
  conformance — it returns `EngineCommitResult::Held/Rejected`).
- **`EngineCommitError` new variants:** `InvalidWitnessEvidence(String)`, `Internal(String)`.
- **`EngineCommitResult::Applied`:** renamed to `Evaluated` (covers NotApplied reject +
  Mainline/Checkpoint applied — `apply_target` in `TaskCommitResult` carries the distinction).
- **CLI exit codes:** new contract (`exit_codes` module). 0 Completed, 10
  AwaitingWitnesses, 11 RequiresRevision, 12 ExceededManeuverLimit, 13
  RequiresOperatorApproval, 20 WitnessEvaluationError, 40
  PendingAuthorizationPersistenceFailure, 70 SystemFailure, 80 TaskNotFound, 90 LlmError.
- **Navigator store/clock:** `Box<dyn PendingAuthorizationStore>` + `Box<dyn Clock>`
  (ZORUNLU, not Optional). Production wires `FilesystemPendingAuthorizationStore` +
  `SystemClock`; tests use `NullPendingAuthorizationStore` + `FixedClock`.
- **Persisted evidence schema:** AttemptEvidence + AuthorizationEvent separation
  planned for P1 (currently single composite TrajectoryEvidence record).

## 9. Deferred boundary

INV-T9 Steps 1-6 + #72 (closure landed, scoped review pending) established suspension
semantics, claim continuity, budget isolation, persist-before-return, exhaustive error
taxonomy, canonical decision-basis (rule sequence binding, claim-specific effective
vision, structural-delta defensive integrity), canonical v1 byte contract (golden
vectors), store hardening, and embedded attempt-evidence integrity (canonical
`SuspendedAttemptEvidence` snapshot + domain-separated evidence digest + full
record↔basis↔evidence cross-field verification). The following remain **merge-blocking**
(tracked as separate issues):

- **#70 — EngineMeasurement pipeline:** real per-axis provenance + engine-issued
  measurement token. The golden vectors lock the v1 byte encoding of the currently
  defined models; they do not prove runtime data is correctly produced. #70 is the
  runtime semantic-correctness blocker.
- **#73 — witness Q3 honest-reject production wiring:** `EngineCommitResult::Rejected`
  production reachability. #72 evidence construction for Rejected is verified through
  the production mapper; upstream reachability (real witness gate producing Rejected)
  is tracked in #73. PR #69 merge decision requires governance call on whether
  Rejected end-to-end reachability is merge-blocking.

**#72 (Implemented — awaiting scoped review):** Embedded attempt-evidence integrity
landed. Canonical `SuspendedAttemptEvidence` snapshot in `PendingAuthorization` +
`RevisionRequired` (minimal shape), domain-separated evidence digest
(`osp.attempt-evidence.v1\0`), record ↔ basis ↔ evidence cross-field verification
(11-adım chain with typed mismatch errors), store identity migration (artifact
filename + receipt use evidence identity), creation/load path ayrımı (P0-1), persist-
boundary verify (P0-2), single canonical rejection key (P0-3), strict wire + API
normalize ayrımı (P1-1), evidence constructor semantic validation (P1-2), record-
internal vs envelope verification ayrımı (P1-3). Issue OPEN pending scoped review.

**Separate lifecycle follow-up (not merge-blocking):**

- **Witness resume workflow:** `osp trajectory status`, `osp witness add`,
  `osp trajectory resume` CLI + store-backed persistence.
- **Cross-process resume orchestration:** pending artifact load + staleness re-measure
  (`current_revision == base_revision` → continue; `!=` → remeasure).

The canonical authorization-basis + attempt-evidence data model is complete and can be
reused by lifecycle resume work.

## 10. High-risk GOVERNANCE disclosure

This change is GOVERNANCE §3 high-risk (witness/quorum safety + evidence integrity).
Independent review is **policy-required**. This PR is not merged until an eligible
independent reviewer is engaged. CI green → "ready for eligible independent review",
not merge.

During the solo phase, high-risk independent review is **policy-enforced rather than
branch-enforced**. The Project Owner prepares the qualifying review record
(spec + tests + this evidence note); an eligible independent reviewer evaluates it.
Self-review evidence ≠ eligible independent review.

---

## Paper 2 manuscript propagation (blocking follow-up PR)

The following Paper 2 surfaces must be updated before the next publication or
canonical paper release:

- Abstract / Contributions count (8 → 9 invariants)
- §3.4 invariant table (INV-T1..T9)
- Adaptive Control Loop section
- Witness policy isolation section
- INV-T7 maneuver-limit description (cross-reference INV-T9)
- Discussion / Conclusion
- Test/evidence manifest

This propagation is a **separate PR** that blocks new Paper 2 version / arXiv
revision production. Published Zenodo deposits are never rewritten — the next
version will incorporate INV-T9.
