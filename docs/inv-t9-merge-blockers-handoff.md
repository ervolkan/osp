# INV-T9 Merge Blockers — #70 + #72 Handoff

**Tarih:** 2026-07-18
**Branch:** `fix/inv-t9-witness-suspension`
**PR:** #69 (https://github.com/ontologicalspace/osp/pull/69)
**Current head:** `fa4d8d0`
**INV-T9 Steps 1-6:** ✅ COMPLETE + #71 CLOSED

---

## Yeni Oturumda İlk Komut

```bash
cd P:/Work/SoftwarePhysics
git checkout fix/inv-t9-witness-suspension
git pull  # fa4d8d0 head olmalı
git log --oneline -3
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib  # 816 test geçmeli
```

Sonra bu belgeyi oku, #70 veya #72'den başla (bağımsızlar, sıra önemli değil).

---

## Durum özeti

INV-T9 (External-Evidence Suspension Isolation) implementation **tamamen bitti**:
- Steps 1-6 + tüm scoped closure'lar tamam, reviewer-approved.
- **#71 (canonical decision-basis) KAPANDI** — Step 4a/4b/4c/5/6 tamam.
- PR #69 head `fa4d8d0`, 816 osp-core lib test, clippy 10 baseline (0 yeni).
- v1 byte contract golden vectors ile kilitlendi (`authorization_basis_digest_v1_golden_vector` + `evaluation_context_digest_v1_golden_vector`).

**PR #69 merge-ready DEĞİL** — iki bağımsız merge-blocker kaldı:

| Issue | Başlık | Kategori |
|-------|--------|----------|
| **#70** | EngineMeasurement pipeline (per-axis provenance) | Runtime semantic correctness |
| **#72** | Embedded attempt-evidence integrity | Evidence integrity |

İkisi de **birbirinden ve #71'den bağımsız** — paralel ilerleyebilir, sıra önemli değil. İkisi de bitmeden PR #69 merge edilemez.

---

## #70 — EngineMeasurement pipeline (per-axis provenance)

**Issue:** https://github.com/ontologicalspace/osp/issues/70
**Title:** INV-T9: engine-issued per-axis measurement provenance (EngineMeasurement token)

### Sorun (code-verified)

`provenanced_from_raw(raw, source)` (`navigator.rs:169`) tek bir `MetricSource` alıp **tüm 5 eksene** yazıyor. Tüm production caller'lar `MetricSource::Scip` geçiyor:
- `navigator.rs:728` — `AgentNavigator::run_task`
- `osp-mcp/src/server.rs:768` — `current_measured()`
- `osp-mcp/src/server.rs:867` — claim evaluation

Sonuç: basis, her eksen için `Scip` kaydediyor ama gerçekte:
```
coupling.source       = Scip   // aslında: graph out-degree
cohesion.source       = Scip   // aslında: node.cohesion veya fallback
instability.source    = Scip   // aslında: graph Ce/Ca
entropy.source        = Scip   // aslında: commit-entropy config
witness_depth.source  = Scip   // aslında: witness config
```

`required_source = Scip` olan bir predicate entropy ekseninde **geçersiz** ölçümle geçiyor. **INV-T4 source-requirement bypass edilebilir.**

### İkincil sorun (aynı kök neden)

`TaskCommitInput.measured` (`engine.rs:101-111`) public, caller-set. `commit_task_claim` olduğu gibi kabul ediyor — re-measure yok, source validation yok. Caller, `CoordinateSystem A`'dan ölçüm geçip engine `CoordinateSystem B` ile basis üretebilir.

### Önerilen yön (issue'dan)

Engine-issued, context-bound measurement token:
```rust
pub struct EngineMeasurement {
    measured: ProvenancedRawPosition,           // gerçek per-axis sources
    measurement_input_digest: MeasurementInputDigest,
    base_space_view_revision: SpaceViewRevision,
}

impl SpaceEngine {
    pub fn measure_delta(&self, delta: ...) -> Result<EngineMeasurement, MeasurementError>;
}
```

`TaskCommitInput` `EngineMeasurement` taşır — non-engine-issued measurement authorization path'e giremez.

### Non-trivial noktalar (issue'dan)

- `Axis` trait (`coords.rs:393-404`) `compute()`'dan yalnız `f64` döner. Per-axis `MetricSource` exposure yok. Gerçek per-axis provenance için ya `Axis`/axis struct'ları source taşıcak şekilde genişletilmeli, ya da her eksenin origin'ini bilen yeni engine-owned measurement path.
- Tüm production caller'lar `provenanced_from_raw(raw, Scip)`'ten migrate edilmeli.
- `commit_task_claim` signature değişiyor (`TaskCommitInput.measured` → `measurement: EngineMeasurement`).
- `provenanced_from_raw` test/example için kalır; yalnız production authorization path'inden kaldırılır.

### Acceptance criteria (issue'dan)

- [ ] `EngineMeasurement` private-field token, yalnız `SpaceEngine::measure_delta` üretir
- [ ] Per-axis `MetricSource` gerçek origin'i yansıtır (uniform `Scip` değil)
- [ ] `provenanced_from_raw` production authorization path'inden kaldırıldı (navigator + MCP server)
- [ ] `TaskCommitInput` `EngineMeasurement` taşır; non-engine-issued measurement `commit_task_claim`'e giremez
- [ ] Regression test: entropy ekseninde `required_source = Scip` predicate, commit-history-derived entropy ile **fail** olur
- [ ] Regression test: farklı `CoordinateSystem`'dan caller-supplied measurement reject (context↔measurement binding)

### Golden vector etkisi

#70 runtime *üretim yolunu* değiştirirse (alan/encoding değiştirmeden), v1 byte contract **korunur**. Alan veya encoding değişikliği gerekirse golden mismatch oluşur → pre-release v1 revizyonu / v2 kararı gerekir (conformance doc §"v1 byte contract" bölümünde belgelendi).

### Önerilen commit başlangıcı

```
feat(measurement): engine-issued per-axis provenance token (INV-T9 #70)
```

---

## #72 — Embedded attempt-evidence integrity

**Issue:** https://github.com/ontologicalspace/osp/issues/72
**Title:** INV-T9: embedded attempt-evidence integrity — SuspendedAttemptEvidence canonical snapshot

### Sorun

`PendingAuthorizationEnvelope` authorization basis'i embedded + digest'le taşıyor ama **attempt evidence** canonical snapshot olarak bağlı değil — yalnız dolaylı `attempt_evidence_id`. Domain-separated evidence digest yok. record ↔ basis ↔ evidence cross-field verification eksik.

Bugün `RevisionRequired` ve `PendingAuthorization` evidence identifier + witness data taşır ama complete embedded attempt-evidence integrity yok (conformance doc §"RevisionRequired evidence preservation" ve §"Self-contained artifact" bölümlerinde netleştirildi).

### Acceptance criteria (issue'dan, 11 madde)

- [ ] `PendingAuthorizationEnvelope` canonical `SuspendedAttemptEvidence` snapshot taşır (yalnız reference id değil)
- [ ] `RevisionRequired` aynı evidence snapshot'ını taşır (Held ve Rejected path'leri tek production source)
- [ ] Evidence `claim_id`'ye bağlanır
- [ ] Evidence `AuthorizationBasisDigest`'e bağlanır
- [ ] Domain-separated evidence digest var (`osp.attempt-evidence.v1\0` veya benzeri)
- [ ] `PendingAuthorizationEnvelope::verify()` record ↔ basis ↔ evidence cross-field doğrular
- [ ] Held/Rejected runtime evidence ve persisted snapshot tek üretimden gelir
- [ ] Durable lookup yoksa `attempt_evidence_id` kaldırılır (dangling reference yok)
- [ ] Durable lookup varsa kimlik gerçekten resolve edilebilir
- [ ] Tamper ve mismatch testleri var (evidence/basis/claim)
- [ ] PR #69 merge-blocking

### Scope boundary (issue'dan)

Bu issue **yalnız** embedded evidence integrity. `AuthorizationBasisDigest` / `EvaluationContextDigest` canonical byte contract'ını değiştirmez (Step 6 golden vectors ile kilitlendi). Evidence encoding yeni digest tipi gerektirirse kendi domain separator + golden vector alır.

### Önerilen commit başlangıcı

```
feat(authorization): embedded SuspendedAttemptEvidence canonical snapshot (INV-T9 #72)
```

---

## Conformance doc güncel referans

`docs/paper2-notes/conformance/inv-t9-external-evidence-suspension.md` — güncel:
- §5 "Authorization basis" + "Canonical decision-basis layers (Step 4a/4b/4c/5)" + "v1 byte contract (Step 6 golden vectors)"
- §9 "Deferred boundary" — #70 + #72 merge-blocking, lifecycle follow-up (witness resume CLI, cross-process resume)
- §"Self-contained artifact" — envelope authorization basis için self-contained, evidence için değil (#72)
- §"RevisionRequired evidence preservation" — current evidence identifier + witness data, complete integrity #72'de

---

## Önemli dosya lokasyonları (doğrulanmış)

### #70 için
- `provenanced_from_raw`: `navigator.rs:169`
- Production caller'lar: `navigator.rs:728`, `osp-mcp/src/server.rs:768`, `osp-mcp/src/server.rs:867`
- `TaskCommitInput.measured`: `engine.rs:101-111`
- `commit_task_claim`: `engine.rs:518+`
- `Axis` trait: `coords.rs:393-404`
- `ProvenancedRawPosition` (5 × AxisMetric): `trajectory.rs:118-135`
- `MeasurementInputContext`/`MeasurementInputDigest`: `authorization.rs:398+`, `538+`

### #72 için
- `PendingAuthorizationEnvelope`: `authorization.rs:2178+`
- `PendingAuthorizationEnvelope::verify`: `authorization.rs:2369+`
- `PendingAuthorization` (record): `authorization.rs` (search)
- `RevisionRequired`: `authorization.rs:2140+`
- `attempt_evidence_id`: `authorization.rs` (search — AttemptEvidenceId type)
- `load_pending_authorization`: `authorization.rs:2454+`
- `FilesystemPendingAuthorizationStore`: `authorization.rs` (search)

### Genel (her ikisi için)
- v1 golden vectors: `authorization.rs` test modülü — `authorization_basis_digest_v1_golden_vector` + `evaluation_context_digest_v1_golden_vector`
- DOMAIN_SEPARATOR'lar: `osp.authorization-basis.v1\0`, `osp.evaluation-context.v1\0`, `osp.measurement-input.v1\0`, `osp.space-content.v1\0`
- Shared preimage helper'lar: `canonical_f64_bytes`, `encode_canonical_edge_to_vec`, `encode_vision_subject_to_vec`, `encode_canonical_edge_identity_to_vec`

---

## Governance / risk

- PR #69 GOVERNANCE §3 high-risk (witness/quorum safety + evidence integrity). Merge edilmez — eligible independent review policy-required.
- #70 ve #72各自 high-risk — her biri kendi scoped review turundan geçmeli.
- Force-push yok (incremental, normal push). Her commit bağımsız derlenir + `-D warnings` test geçer.
- v1 byte contract kilitli — breaking change (canonical field/order/tag/encoding) explicit v2 kararı gerektirir. Semantics-version change compatibility impact review'ı gerektirir (her zaman v2 değil).

---

## CI simülasyonu (her commit öncesi)

```bash
RUSTFLAGS="-D warnings" cargo test -p osp-core --lib
RUSTFLAGS="-D warnings" cargo build --workspace --examples --exclude osp-desktop
RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop
cargo fmt -p osp-core -- --check
cargo clippy -p osp-core --lib   # baseline 10; 0 yeni hedef
```

---

## Review zinciri yaklaşımı

INV-T9 serisi incremental scoped review ile ilerledi — her implementasyon commit'i + closure commit'leri ayrı review turlarından geçti. #70 ve #72 için aynı yaklaşım önerilir:
1. Plan (EnterPlanMode) → reviewer onayı
2. Implementasyon commit'leri
3. Scoped review → REQUEST CHANGES/APPROVED
4. Closure commit'leri
5. Issue kapatma (scoped APPROVAL sonrası)

Her review turunda reviewer'ın P0/P1/P2 bulguları closure commit'leriyle kapatıldı — bu seride 6+ review turu oldu, her biri katkı sağladı.

---

*Bu belge INV-T9 merge-blocker'larına (#70, #72) geçiş için handoff'tır. INV-T9 implementation (#71 scope) tamamen bitti.*
