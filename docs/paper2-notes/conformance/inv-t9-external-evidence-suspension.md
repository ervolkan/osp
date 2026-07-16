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
always bound (id alone insufficient). `EvaluationContextDigest` covers vision config +
rule-set + semantics versions. `SpaceViewRevision` is store-scoped + lane-qualified.
Float canonicalization: NaN rejected, -0.0 normalized, little-endian, sorted
collections, `f64::to_bits()`. `created_at` NOT in digest. `Clock` trait
(`SystemClock`/`FixedClock`) — core never calls `SystemTime::now()` directly.

### Pending authorization (Model B + Sabitleme 1)

`PendingAuthorization` carries predicate completion, mutation decision, intended apply
target, authorization basis digest, base space-view revision, evaluation context
digest, witness requirement, **witness hold reason** (Sabitleme 1), witness snapshot,
attempt evidence id, created_at. All authorization-gated mutations covered
(AcceptAsCompleted + AcceptAsProgress).

### Self-contained artifact (Sabitleme 3)

`PendingAuthorizationEnvelope` embeds the full canonical `AuthorizationBasis` alongside
the digest. `verify()` recomputes the digest on load and rejects mismatches (tamper /
corruption detection). Single canonical schema string `"osp.pending-authorization.v1"`
(no separate schema_version in record).

### Navigator-owned persistence (P0-1)

`PendingAuthorizationStore` trait + `FilesystemPendingAuthorizationStore`. Navigator
calls `persist()` BEFORE returning `AwaitingWitnesses` — no external suspended result
without a published artifact. No `AwaitingWitnesses` result is externally returned
unless its pending artifact has first been successfully published.

No-clobber (create_new): silent overwrite forbidden. Idempotent: same claim+digest+
content → success; same path+different content → BasisConflict; same claim+different
digest → separate artifact. Crash-consistent publish: same-dir temp → write_all →
sync_all → atomic no-clobber rename.

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

### `RevisionRequired` evidence preservation

`NavigatorResult::RequiresRevision` carries task_id, claim_id, authorization basis
digest, witness snapshot, attempt evidence id (Commit 4 expands this to full
`RevisionRequired` struct).

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
  mechanical rename. Legacy `commit()` (standalone/Paper 1) retains
  `EngineCommitError::Witness(Reason)` via conversion helpers.
- **CLI exit codes:** new contract (`exit_codes` module). 0 Completed, 10
  AwaitingWitnesses, 11 RequiresRevision, 12 ExceededManeuverLimit, 13
  RequiresOperatorApproval, 80 TaskNotFound, 90 LlmError. Snapshot tests will pin
  these.
- **Persisted evidence schema:** AttemptEvidence + AuthorizationEvent separation
  planned for P1 (currently single composite TrajectoryEvidence record).

## 9. Deferred boundary (P1 follow-up PR)

This PR establishes suspension semantics, claim continuity, budget isolation, and
the versioned pending record. The following are **deferred to the immediately
following lifecycle PR**:

- **Witness resume workflow:** `osp trajectory status`, `osp witness add`,
  `osp trajectory resume` CLI + store-backed persistence.
- **Navigator persist-before-return wiring:** engine must expose
  `evaluation_context_digest` + `base_space_view_revision` for full
  `AuthorizationBasis` construction. Navigator currently passes
  `pending_authorization_store: None` (store + envelope ready, wiring pending).
- **Cross-process resume:** pending artifact load + staleness re-measure
  (`current_revision == base_revision` → continue; `!=` → remeasure).
- **Stale authorization basis detection:** on resume, if gate policy (vision/
  rule-set) changed since basis measurement, remeasure before witness evaluation.
- **`new_evidence_reuses_the_same_claim` test:** pending resume integration test.

The `PendingAuthorizationEnvelope` embeds the full `AuthorizationBasis`, so P1 can
persist and resume without redesign — the data model is complete.

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
