# Stage G2c-1b — Navigator Reject-Evidence (Paper 2 evidence completeness)

> **Aşama:** G2c-1b (navigator reject-evidence completeness fix)
> **Tarih:** 2026-06-29
> **Tez:** "Navigator'dan geçen her attempt — başarılı veya reddedilmiş — TrajectoryEvidence
> içinde iz bırakır. Hangi gate'te kaldığı kayıtlıdır."
> **Review entegrasyonu:** Arkadaş review 6 değerlendirmesinin 5 noktası tamam.

## Sorun (G2c-1 MVP'den)

G2c-1 corpus runner 24 cell koşturdu ama **0 evidence entry** üretti. Kök neden: navigator'ın
3 reject yolu vardı ama **empty-proposal yolu** (satır 408) `continue` ediyordu — evidence push
ETMİYORDU. Ayrıca mevcut evidence site'lerinde **hangi gate'te reddedildiği** (`gate_decision`)
kayıt değildi. "Agent neden başarısız oldu? Hangi gate'te kaldı?" sorusu cevapsızdı.

## Çözüm (G2c-1b)

### 1. GateDecision enum genişlet (review 6 #1, #2)
```rust
#[derive(..., Default, Serialize, Deserialize)]
pub enum GateDecision {
    #[default]                    // serde backward-compat (eski JSON Unknown default)
    Unknown,
    PassedAll,
    RejectedBySyntax,
    RejectedByVision,
    RejectedByRule,
    RejectedByTaskBinding,        // YENİ — Q5.b binding hatası (PermissionDenied)
    BlockedByManeuverLimit,
}
```
`RejectedByTaskBinding`: Stage B'nin type-safe `TaskBoundClaim` kararına saygı — binding
hatası Syntax'a gömülmez (review 6 #2).

### 2. TrajectoryEvidence +gate_decision alan
```rust
pub struct TrajectoryEvidence {
    // ...
    #[serde(default)]             // eski JSON backward-compat
    pub gate_decision: GateDecision,
    // ...
}
```

### 3. gate_decision_from_engine_error helper (review 6 #2)
Tek noktada mapping — elle dağıtılmış match değil:
```rust
pub fn gate_decision_from_engine_error(err: &EngineCommitError) -> GateDecision {
    match err {
        EngineCommitError::SyntaxViolation { .. } => RejectedBySyntax,
        EngineCommitError::VisionViolation { .. } => RejectedByVision,
        EngineCommitError::RuleViolation { .. } => RejectedByRule,
        EngineCommitError::PermissionDenied(_) => RejectedByTaskBinding,
        EngineCommitError::Witness(_) | NoPersistence | Persistence(_) => Unknown,
    }
}
```

### 4. Empty-proposal YENİ evidence site (review 6 "en güçlü taraf")
```rust
Err(_) => {  // build_claim_from_proposal EmptyProposal
    let before_raw = self.current_measured.to_raw();
    self.evidence.push(TrajectoryEvidence {
        before: before_raw, after: before_raw,  // state unchanged (INV-T6)
        gate_decision: RejectedBySyntax,
        mutation_decision: Reject,
        // ...
    });
    feedback_history.push("Empty DeltaProposal — provide new_nodes/new_edges...");
    continue;
}
```

## Evidence mapping tablosu (review 6 #3, #4)

| Yol | gate_decision | predicate | mutation | before | after |
|---|---|---|---|---|---|
| Empty proposal | RejectedBySyntax | NotCompleted | Reject | current | current |
| Q4 syntax fail | RejectedBySyntax | NotCompleted | Reject | current | current |
| Q5 vision fail | RejectedByVision | NotCompleted | Reject | current | current |
| Q6 rule fail | RejectedByRule | NotCompleted | Reject | current | current |
| Task binding fail | RejectedByTaskBinding | NotCompleted | Reject | current | current |
| Predicate fail + StrictReject | PassedAll | NotCompleted | Reject | current | current |
| Predicate fail + AcceptImprovement | PassedAll | NotCompleted | AcceptAsProgress | current | checkpoint |
| Predicate satisfied | PassedAll | Completed | AcceptAsCompleted | current | measured_after |
| Maneuver limit | BlockedByManeuverLimit | NotCompleted | Reject | current | current |

### `after` semantiği (review 6 #3)
- **Reject** → `after = before` (state unchanged, INV-T6 — failure ≠ regression)
- **AcceptAsProgress** → `after = checkpoint state` (INV-T8 — asla Mainline, TrajectoryCheckpoint lane)
- **AcceptAsCompleted** → `after = measured_after` (Mainline-equivalent, predicate satisfied)

## Testler (review 6 #5 — 4 test)
- `navigator_records_evidence_for_empty_proposal` — boş proposal → evidence push, gate=RejectedBySyntax
- `navigator_evidence_includes_gate_decision` — reject attempt'lerde gate_decision Unknown DEĞİL
- `navigator_syntax_reject_evidence_does_not_advance_state` — before==after (INV-T6)
- `navigator_progress_evidence_semantics` — AcceptAsProgress şema + serialize round-trip (G2c-3 temel)

osp-core: 282 unit + 4 yeni = 286 test, hepsi yeşil.

## Doğrulama (G2c runner tekrar koştur)
```
G2c-1 (öncesi):  0 evidence entry (24 cell)
G2c-1b (sonrası): 120 evidence entry (24 cell × 5 maneuver_limit)

gate_decision dağılımı:
  RejectedBySyntax: 72  (empty/bad_format proposals — "without feedback" + ilk attempt)
  Unknown:           48  (commit_task_claim success yolu — engine AttemptOutcome gate set etmiyor)
```

**Evidence 0 → 120'ye çıktı.** Reject attempt'ler artık ledger'a giriyor. "Agent neden
başarısız oldu?" sorusu JSON'dan cevaplanabilir: `gate_decision: RejectedBySyntax`.

## Bilinen eksik (G2c-2'ye kadar kritik değil)
48 `Unknown` evidence: commit_task_claim success yolundan gelir. Engine `AttemptOutcome`
üretirken `gate_decision` set ETMİYOR (engine.rs'te hiç assignment yok). PredicateGate
evaluate ediyor ama AttemptOutcome.gate_decision boş kalıyor → navigator success site'i
`outcome.gate_decision` (Unknown) alıyor.

Bu engine.rs değişikliği gerektirir (G2c-1b kapsamı dışında). Success yolu zaten Completed
gösterir, gate_decision Unknown olması RQ8/RQ9 reject analizini etkilemez (reject yolları
doğru set ediliyor). G2c-2'de target-edge proposals ile Completed arttığında düzeltilebilir.

## RQ etkisi (G2c-2/3 için hazır)
- **RQ8 (calibration feedback):** artık "with feedback attempt'leri hangi gate'ten geçti"
  JSON'dan okunabilir (RejectedBySyntax → düzeltilmiş → PassedAll progression)
- **RQ9 (policy):** StrictReject vs AcceptImprovement — reject reason + state advance farkı
  evidence'da (after=before vs after=checkpoint)

G2c-2 (target-edge-aware proposals) yapıldığında, Completed gördüğümüzde "hangi attempt
progress oldu, hangi gate'ten geçti" tamamıyla JSON'da görünür olacak.

## Çıktı
- `crates/osp-core/src/trajectory.rs` (GateDecision +2 variant + Default; TrajectoryEvidence +1 alan)
- `crates/osp-core/src/navigator.rs` (helper + 3 site fix + 1 yeni site + 4 test)
- `crates/osp-mcp/src/server.rs` (GateDecision match genişlet — yeni variantlar)
- STATUS.md/roadmap G2c-1b ✅

**Küçük PR, G2c-2/3'ün ölçülebilir olması için önkoşul.**
