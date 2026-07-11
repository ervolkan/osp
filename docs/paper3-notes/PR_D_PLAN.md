# PR D Plan — Evidence Projection + In-Process Wiring Proof (yeni oturum için)

> **Dal:** `feat/evidence-projection-wiring` (main `db6df38` üstünde — PR D PLAN tur 3 commit)
> **Scope:** osp-cli only (osp-core untouched — PR C evidence model hazır)
> **4 tur plan review sonucu: implementation-ready**

## Özet

CLI metric drafts (`ProjectedCodeMetric`) → core evidence (`ObservedCodeEvidence` via
`ObservedPhysicalMetrics`) conversion. Yeni `evidence_projection.rs` modülü (draft→evidence
boundary). **Production path:** `graph init --analyze` evidence üretir + diagnostics yazar.
**Compatibility proof:** in-crate unit test evidence → `InMemoryCodeEvidenceProvider` →
`AnchorGateContext` → scorer seam'in çalıştığını kanıtlar (production consumer DEĞİL).
**Persistence KAPSAM DIŞI** — ayrı milestone (PR G); PR C Serialize-only sınırı persistence için
kendi restore modelini gerektirir.

## Review tur 4 kararları (3 iyileştirme — implementation-ready)

Tur 3 sonrası mimari bloklayıcı yok; tur 4 üç iyileştirme ile tamamlandı:

1. **Wiring proof gate branch'ini de kanıtlar (tur 4 iyileştirme 1):** tur 3'te "scorer seam
   only" dar iddiaya çekmiştik (dış CLI candidate construct edemez gerekçesiyle). Tur 4 düzeltme:
   candidate'i public `AnchorPipeline` extractor üretir (`CodeEntity:<name>` + "implement" lemması
   → `ImplementedBy`, extractor.rs:80-81). Test genişletildi: `feeds_pipeline_scorer_and_gate`
   + negatif `pipeline_without_provider_rejects_implemented_by`.
2. **`InvalidCollection::DuplicateAxis` test eklendi (tur 4 iyileştirme 2):** Empty unreachable
   ama DuplicateAxis defensive boundary olarak test edilebilir (PR B dedup invariant'ı gelecekte
   kaldırılırsa PR D boundary fail-closed kalır).
3. **Zero coverage reject (tur 4 karar 3):** `coverage=0, strength>0` core'da temsil edilebilir
   ama PR B confidence formülü (coverage içerir) + zero-confidence omission ile tutarsız. Conversion
   boundary'de reject: `ZeroCoverage { node_id, axis }`. Semantik: "sıfır coverage observation değildir".

---

## Review tur 3 kararları (2 bloklayıcı + 2 P2 zorunlu + 6 mekanik)

Plan tur 2'de iki bloklayıcı test-topolojisi problemi vardı; tur 3'te düzeltildi:

1. **Bloklayıcı 1 — Integration test private modüllere erişemez:** `osp-cli` binary-only crate
   (`lib.rs` YOK); `tests/*.rs` private `project_analysis`/`ProjectedCodeMetric` factory'sine erişemez.
   **Çözüm:** compatibility proof in-crate `#[cfg(test)] mod tests`'e taşındı (evidence_projection.rs
   veya analysis_bridge.rs içinde). `analyze_bridge_flow.rs` binary CLI diagnostics integration test
   olarak kalır.
2. **Bloklayıcı 2 — "scorer + gate" iddia kanıtlanamaz:** gate `find_evidence()` yalnız `ImplementedBy`
   candidate'ında çağırır; dış CLI kodu `ExtractedAnchorCandidate`/`AnchorCandidate` construct edemez
   (`pub(crate)`). **Çözüm:** test adı `evidence_projection_feeds_pipeline_scorer`'a daraltıldı
   (provider lookup + minimum strength + code_evidence_score > 0 kanıtlanabilir). Gate evidence
   branch davranışı osp-core gate testlerinin sorumluluğunda kalır.
3. **P2 — Defensive error testleri validated factory ile üretilemez:** newtype'lar `[0,1]` doğruluyor;
   `rejects_invalid_strength/coverage` testleri geçersiz input üretemez. **Çözüm:** iki factory —
   validated (`projected_metric_for_tests`) happy-path için + unchecked forged
   (`projected_metric_unchecked_for_contract_tests`) defensive conversion error testleri için.
4. **P2 — Clock fail-open:** `unwrap_or(0)` sistem saati epoch öncesi olduğunda `measured_at=0`
   üretir → geçerli ama aşırı eski evidence. **Çözüm:** `now_unix_secs() -> anyhow::Result<u64>`
   (fail-closed); clock failure store mutation'dan önce.

Mekanik düzeltmeler: main.rs mod declaration, derive zinciri, MetricScalarViolation alias, "map"
vs "adopt" netleştirme, uygulama sırası düzeltme, run-metadata.json untouched.

---

## Üretim + compatibility ayrımı (tur 3 net sınırı)

### Production path (`graph init --analyze`)
```
AnalysisResult → project_code_metrics → ProjectedCodeMetric[]
  → project_observed_evidence → ObservedCodeEvidence[]
  → BridgeRunOutput.evidence_projection
  → graph init diagnostics (stderr)
```
**Provider construct EDİLMEZ** — production consumer (`AnchorPipeline`) bu command'da yok.

### Compatibility proof (in-crate unit test — tur 3 bloklayıcı 1 + tur 4 genişletme)
```
EvidenceProjectionOutput.evidence
  → InMemoryCodeEvidenceProvider::from_evidence
  → AnchorGateContext::with_code_evidence
  → AnchorPipeline::run_with_source (scorer seam + gate ImplementedBy branch)
  → accepted AnchorPlan (ImplementedBy candidate accepted)
```
Bu in-crate unit test **scorer seam + gate evidence presence branch**'in çalıştığını kanıtlar.
Tur 4 genişletme: candidate'i public `AnchorPipeline` extractor üretir (typed `CodeEntity:<name>`
+ "implement" lemması → `ImplementedBy`, extractor.rs:80-81). Dış CLI candidate construct etmez;
extractor public API üzerinden çalışır. Negatif karşı-test: provider yok → `ImplementedBy` reject.

### Out-of-scope
```
evidence persistence (store write)
ObservedCodeEvidence Deserialize
store schema changes
new anchoring CLI command (packet evaluation)
review re-analysis
long-lived provider lifecycle
```

---

## osp-cli değişiklikleri

### A. Yeni `crates/osp-cli/src/evidence_projection.rs` — draft→evidence conversion boundary

`metric_projection.rs` durur (draft üretir); bu modül draft → core evidence dönüşümünün **tek
sahibi**. Source-scan guard bu ownership'i mekanik doğrular (D maddesi).

```rust
/// Conversion context — wall-clock inject edilir (temporal nondeterminism yalnız caller'da).
/// Unit testlerde sabit değer: TEST_MEASURED_AT = 1_700_000_000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvidenceProjectionContext {
    pub(crate) measured_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EvidenceProjectionOutput {
    pub(crate) evidence: Vec<ObservedCodeEvidence>,
    pub(crate) report: EvidenceProjectionReport,
}

/// Report — input yüzeyiyle uyumlu (tur 2 P1 düzeltme).
/// Conversion yalnız emit edilmiş metric'leri görür. Hiç projected metric'i olmayan analysis
/// node'ları bu boundary'nin dışında kalır ve doğal olarak provider lookup'ında bulunmaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvidenceProjectionReport {
    pub(crate) input_metric_values: usize,
    pub(crate) evidence_objects_created: usize,
    pub(crate) partial_evidence_objects: usize,  // < 5 axis (try_to_physical_vector Err)
}

/// Draft metric'leri core evidence'a dönüştürür.
///
/// Input yüzeyi: `metrics` yalnız **emit edilmiş** (admitted) metric'leri içerir — Placeholder/
/// Heuristic/zero-confidence metric'ler `project_code_metrics` tarafından zaten çıkarıldı.
/// Bu nedenle her grouped node'un en az bir metric'i vardır; `ObservedPhysicalMetricsError::Empty`
/// bu conversion yolunda unreachable (defensive olarak handle edilir ama gerçekleşmez).
pub(crate) fn project_observed_evidence(
    metrics: &[ProjectedCodeMetric],
    context: EvidenceProjectionContext,
) -> Result<EvidenceProjectionOutput, EvidenceProjectionError>;
```

**Sorumluluklar:**
1. `ConceptNodeId` bazında group (deterministik sıra — sort by `node_id.0`).
2. CLI `PhysicalCodeAxis` → core `PhysicalCodeMetricAxis` **anti-corruption map** (tur 3 P3-8 —
   "adopt" DEĞİL; explicit conversion, CLI enum korunur). 5 variant exhaustive.
3. Newtype dönüşümü — **duplicate validation YOK** (tur 2 P2 düzeltme):
   - `metric.provenance().confidence().get()` → `EvidenceStrength::new(...)` (InvalidStrength)
   - `metric.provenance().coverage().get()` → **zero coverage reject** (tur 4 karar 3):
     `coverage == 0.0` → `ZeroCoverage { node_id, axis }`. Semantik: "sıfır coverage'a sahip metric
     observation değildir; PR B'de omission olması gerekirdi". PR B confidence formülü coverage
     içerir + zero-confidence omission; coverage=0,strength>0 contract drift'i conversion boundary'de
     reject edilir. Geçerli coverage → `EvidenceCoverage::new(...)` (InvalidCoverage — range-dışı).
   - `metric.value().get()` → raw `f64` olarak `ObservedPhysicalMetric::new`'e geçir (core kendi
     `PhysicalAxisValue::new` validation'ını yapar; InvalidObservation altında gelir)
4. `ObservedPhysicalMetric::new(axis, value, source, strength, coverage)` çağır.
5. Her node için `ObservedPhysicalMetrics::try_new(Vec<...>)` — non-empty (input yüzeyi garantisidir)
   + unique-axis (`project_code_metrics` zaten `(ConceptNodeId, axis)` dedup yaptı) + sort_order.
6. `ObservedCodeEvidence::new(node_id, observations, context.measured_at)`.
7. Deterministik sıra.

**`measured_at` içeride üretilmez** — context'ten inject (test deterministic + replay).
`project_analysis` wall-clock okumaz; temporal nondeterminism yalnız caller'ın verdiği `measured_at`.

### Typed error model (tur 3 P2-7 — MetricScalarViolation alias netleştirme)

İki ayrı `MetricScalarViolation` var: CLI (`crate::metric_projection::MetricScalarViolation`) ve
core (`osp_core::anchoring::MetricScalarViolation`). Alias ile ayrıştırılır:

```rust
use osp_core::anchoring::MetricScalarViolation as CoreMetricScalarViolation;
// CLI draft MetricScalarViolation conversion'da kullanılmaz (draft validation zaten tamam).

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum EvidenceProjectionError {
    /// EvidenceStrength dönüşümü hatası (defensive contract-drift — projection confidence'ı
    /// zaten [0,1] doğruluyor, ama typed conversion sınırında eksiksiz olmalı).
    #[error("{node_id} {axis:?} axis EvidenceStrength geçersiz: {source}")]
    InvalidStrength {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
        source: EvidenceStrengthOutOfRange,
    },
    /// Zero coverage reject (tur 4 karar 3 — contract drift defense).
    /// Semantik: "sıfır coverage'a sahip metric observation değildir; PR B'de omission olması
    /// gerekirdi". coverage=0, strength>0 core'da temsil edilebilir ama PR B confidence formülü
    /// (confidence coverage içerir) + zero-confidence omission ile tutarsız → conversion reject.
    #[error("{node_id} {axis:?} axis zero coverage — observation değildir (PR B omission beklenir)")]
    ZeroCoverage {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
    },
    #[error("{node_id} {axis:?} axis EvidenceCoverage geçersiz: {source}")]
    InvalidCoverage {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
        source: CoreMetricScalarViolation,  // core tipi (EvidenceCoverage::new'den)
    },
    /// ObservedPhysicalMetric::new hatası (InvalidValue + ZeroStrength dahil; core constructor
    /// axis/value context zaten taşıyor — InvalidAxisValue ayrı varyant YOK).
    #[error("{node_id} {axis:?} axis observation contract mismatch: {source}")]
    InvalidObservation {
        node_id: ConceptNodeId,
        axis: PhysicalCodeAxis,
        source: ObservedPhysicalMetricError,
    },
    #[error("{node_id} collection contract mismatch: {source}")]
    InvalidCollection {
        node_id: ConceptNodeId,
        source: ObservedPhysicalMetricsError,
    },
}
```

`analysis_bridge.rs` → `BridgeError::EvidenceProjection` sarar.

### B. `analysis_bridge.rs` — orchestrator (conversion implementation YOK)

```rust
// BridgeRunOutput + evidence_projection field (derive zinciri: EvidenceProjectionOutput/Report Debug+Clone)
#[derive(Debug, Clone)]
pub(crate) struct BridgeRunOutput {
    pub(crate) candidate_seed: AnalysisCandidateSeed,
    pub(crate) identity_index: AnalysisProjectionIndex,
    pub(crate) graph_report: BridgeRunReport,
    pub(crate) metric_projection: AnalysisMetricProjection,
    pub(crate) evidence_projection: EvidenceProjectionOutput,  // YENİ
}

// project_analysis — temporal nondeterminism yalnız caller'ın measured_at değeridir.
pub(crate) fn project_analysis(
    analysis: &AnalysisResult,
    policy: PathCasePolicy,
    evidence_context: EvidenceProjectionContext,
) -> Result<BridgeRunOutput, BridgeError> {
    let candidate_proj = project_candidate_nodes(analysis, policy, scheme)?;
    let metric_projection = project_code_metrics(analysis, &candidate_proj.identity_index)?;
    let evidence_projection = project_observed_evidence(
        &metric_projection.metrics,
        evidence_context,
    )?;
    Ok(BridgeRunOutput { ..., evidence_projection })
}
```

### C. `commands/graph.rs` — clock + diagnostics (conversion YOK, provider construct YOK)

- `now_unix_secs()` helper — **fail-closed** (tur 3 P2-4 düzeltme):
  ```rust
  fn now_unix_secs() -> anyhow::Result<u64> {
      std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .map(|duration| duration.as_secs())
          .map_err(|error| anyhow::anyhow!("system clock is before UNIX_EPOCH: {error}"))
  }
  ```
  Clock failure store mutation'dan **önce** gerçekleşir (non-destructive validation düzeni korunur).
- `EvidenceProjectionContext { measured_at: now_unix_secs()? }` inject → `project_analysis`.
- **Stderr flip (tur 2 dürüst consumer beyanı):**
  ```
  Evidence construction: completed
  Evidence objects: {N}
  Partial evidence objects: {P}  // < 5 axis (analyzer 3 axis üretir → partial)
  Evidence runtime consumer: none in graph init
  Evidence persistence: disabled
  ```
- **Provider construct YOK** production path'te — in-crate unit test'te (E maddesi).

### D. Guard matrisi (`architecture_guards.rs`) — ownership guard (tur 2 P3 güçlendirme)

Mevcut guard korunur + yeni **ownership guard** eklenir:

```rust
// Mevcut — korunur (metric_projection.rs core evidence YOK)
#[test]
fn metric_projection_does_not_construct_complete_core_evidence() {
    // metric_projection.rs içinde ObservedCodeEvidence / PhysicalCodeVector YOK
}

// YENİ — ownership: core evidence construction yalnız evidence_projection.rs'de
#[test]
fn core_evidence_construction_owned_by_evidence_projection() {
    // Tüm CLI production source dosyalarını tara (src/**/*.rs):
    //   ObservedPhysicalMetric::new
    //   ObservedPhysicalMetrics::try_new
    //   ObservedCodeEvidence::new
    // token'ları yalnız src/evidence_projection.rs'de bulunabilir.
    // analysis_bridge.rs, commands/graph.rs, metric_projection.rs'de YOK.
}
```

Bu guard alias/helper ile aşılabilir ama ownership iddiasını doğrudan ifade eder.

### E. In-crate compatibility proof (tur 3 bloklayıcı 1 + tur 4 gate genişletme)

`crates/osp-cli/src/evidence_projection.rs` `#[cfg(test)] mod tests` içinde (binary-only crate —
`tests/*.rs` private modüllere erişemez):

```rust
#[cfg(test)]
mod tests {
    // ...
    /// Compatibility proof (tur 4): evidence → provider → pipeline scorer + gate branch.
    /// Candidate'i public AnchorPipeline extractor üretir (typed CodeEntity:<name> +
    /// "implement" lemması → ImplementedBy, extractor.rs:80-81). Dış CLI candidate construct etmez.
    #[test]
    fn evidence_projection_feeds_pipeline_scorer_and_gate() {
        // Evidence ID: CodeEntity:AuthService
        // Input: "CodeEntity:AuthService implements authentication"
        // 1. projected_metric_for_tests ile evidence üret
        // 2. InMemoryCodeEvidenceProvider::from_evidence
        // 3. AnchorGateContext::with_code_evidence
        // 4. AnchorPipeline::run_with_source
        // Assert: pipeline Ok; ImplementedBy candidate var; target == CodeEntity:AuthService;
        //         code_evidence_score > 0.
    }

    /// Negatif karşı-test (tur 4): provider yok → ImplementedBy reject.
    #[test]
    fn pipeline_without_provider_rejects_implemented_by() {
        // Aynı input ama provider yok (AnchorGateContext::no_authority)
        // Assert: ImplementedBy reject (GateError::ImplementedByRequiresCodeEvidence)
    }
}
```

Bu **compatibility proof** — production consumer DEĞİL. Tur 4 genişletme: test gerçek zinciri
kanıtlar (projected evidence → provider → scorer strength → gate object-presence → accepted
AnchorPlan). Negatif karşı-test provider yokken reject'i doğrular.

### F. `analyze_bridge_flow.rs` — binary CLI diagnostics integration test (mevcut rol korunur)

Stderr assertion lockstep update (tur 3 bloklayıcı 1 — bu test `tests/*.rs` binary invocation):
```
"Code metrics projected (not yet evidence)" → kaldır
"Evidence construction: deferred" → "Evidence construction: completed"
"Evidence runtime consumer: none in graph init" → yeni assertion
"Evidence persistence: disabled" → korunur
```

---

## Test stratejisi (tur 3 P2-3 — iki factory + minimum matris)

### İki test factory (tur 3 P2-3)

`metric_projection.rs` içine iki `#[cfg(test)]` factory (production constructor DEĞİL):

**1. Validated factory (happy-path testler için):**
```rust
#[cfg(test)]
pub(crate) fn projected_metric_for_tests(
    node_id: ConceptNodeId,
    axis: PhysicalCodeAxis,
    value: f64,
    source: ObservedCodeMetricSource,
    confidence: f64,
    coverage: f64,
) -> ProjectedCodeMetric
```
Mevcut validated newtype'ları kullanır (`MetricAxisValue::new`, `MetricConfidence::new`,
`MetricCoverage::new`). 5 axis'i (Entropy/WitnessDepth dahil) kapsar.

**2. Unchecked forged factory (defensive conversion error testleri için — tur 3 P2-3):**
```rust
/// Intentionally bypasses PR B validation to simulate cross-version or
/// contract-drift input at the PR D boundary.
#[cfg(test)]
pub(crate) fn projected_metric_unchecked_for_contract_tests(
    node_id: ConceptNodeId,
    axis: PhysicalCodeAxis,
    value: f64,
    source: ObservedCodeMetricSource,
    confidence: f64,
    coverage: f64,
) -> ProjectedCodeMetric
```
Tuple newtype alanlarını aynı modül içinde doğrudan kurar (validation bypass). Yalnız defensive
conversion error testleri için. Happy-path testleri forged factory KULLANMAZ.

### Minimum test matrisi (tur 4 — DuplicateAxis + zero coverage + gate proof)
```
groups_metrics_by_node_deterministically              // validated factory
maps_all_five_axis_variants_exhaustively              // validated factory (Coupling/Cohesion/Instability/Entropy/WitnessDepth)
preserves_mixed_provenance                            // validated factory (TreeSitter + Scip aynı node)
uses_injected_measured_at                             // validated factory (TEST_MEASURED_AT = 1_700_000_000)
creates_partial_evidence_for_three_axes               // validated factory (analyzer 3 axis → partial)
empty_metric_slice_produces_empty_output              // validated factory (input_metric_values=0)

evidence_projection_feeds_pipeline_scorer_and_gate    // in-crate compatibility proof (tur 4 — scorer + gate)
pipeline_without_provider_rejects_implemented_by      // in-crate negatif karşı-test (tur 4)

rejects_invalid_strength_with_context                 // forged factory (defensive contract-drift)
rejects_invalid_coverage_with_context                 // forged factory (defensive contract-drift)
defensively_handles_observation_contract_mismatch     // forged factory (defensive contract-drift)
rejects_duplicate_axis_per_node_with_context          // forged factory (tur 4 — InvalidCollection::DuplicateAxis)
rejects_zero_coverage_positive_strength_contract_drift // forged factory (tur 4 — ZeroCoverage reject)
```

---

## Kabul kriterleri

1. `evidence_projection.rs` yeni modül — draft→evidence conversion tek sahibi
2. metric_projection.rs durur (draft üretir), core evidence YOK (guard korunur)
3. `PhysicalCodeAxis` → `PhysicalCodeMetricAxis` **anti-corruption map** (tur 3: "adopt" DEĞİL)
4. `MetricConfidence` → `EvidenceStrength` (re-validate, InvalidStrength varyantı)
5. `MetricCoverage` → `EvidenceCoverage` (re-validate, InvalidCoverage varyantı — CoreMetricScalarViolation alias)
6. **Zero coverage reject** (tur 4 karar 3 — `ZeroCoverage { node_id, axis }`; PR B omission sözleşmesi)
7. `MetricAxisValue.get()` → raw `f64` (duplicate validation YOK; core constructor'a bırak)
8. `measured_at` context'ten inject (wall-clock graph.rs'de fail-closed Result, deterministic test)
9. Report input yüzeyiyle uyumlu (input_metric_values / evidence_objects_created / partial —
   `observations.missing_axes().is_empty()` ile partial hesapla, PhysicalCodeVector üretme)
10. Production path: evidence + diagnostics (provider construct YOK)
11. Compatibility proof: **in-crate unit test** (tur 3: `tests/*.rs` DEĞİL) — scorer + gate branch
    (tur 4: `ImplementedBy` candidate public extractor ile; negatif karşı-test provider yok → reject)
12. Stderr flip: "deferred" → "completed"; "consumer: none in graph init" (dürüst)
13. analyze_bridge_flow.rs stderr assertion update (lockstep) — binary CLI diagnostics
14. Guard matrisi: metric_projection.rs deny korunur + ownership guard yeni (std::fs recursive, yeni dep YOK)
15. Persistence KAPSAM DIŞI (store'a yazma YOK; store schema büyütme YOK; Deserialize YOK)
16. osp-core untouched (PR C evidence model hazır)
17. Typed error (anyhow YOK) — node/axis context korunur; InvalidStrength + ZeroCoverage + MetricScalarViolation alias
18. İki test factory (validated + unchecked forged) + minimum test matrisi (13 test)
19. main.rs mod declaration (`mod evidence_projection;`) (tur 3 P2-5)
20. Derive zinciri: EvidenceProjectionOutput/Report Debug+Clone+PartialEq (tur 3 P2-6)

---

## Küçük uygulama notları (tur 4 review önerileri)

### Partial count — PhysicalCodeVector üretme (gereksiz)
Report için `try_to_physical_vector()` çağırmak gereksiz. Unique-axis invariantı bulunduğundan:
```rust
let is_partial = !observations.missing_axes().is_empty();  // semantik olarak açıklayıcı
```

### Validated factory — expect mesajları açık
```rust
MetricAxisValue::new(value)
    .expect("projected_metric_for_tests value must be in [0,1]");
```
Forged factory ile normal factory çağrı yerleri görsel olarak ayrılmalı.

### Ownership guard — std::fs recursive (yeni dep YOK)
`std::fs::read_dir` ile recursive helper. Test modülleri aynı source dosyasında olduğundan
authorized `evidence_projection.rs` içindeki constructor token'ları doğal olarak kabul edilir.

---

## Uygulama sırası (tur 3 P3-9 — factory önce, test bağımlılığı doğru)

0. `main.rs` — `mod evidence_projection;` declaration (tur 3 P2-5)
1. `metric_projection.rs` — iki `#[cfg(test)]` factory (validated + unchecked forged)
2. `evidence_projection.rs` (types + `project_observed_evidence` + typed error + derive zinciri + unit testler + in-crate compatibility proof)
3. `analysis_bridge.rs` (`BridgeRunOutput` + `BridgeError::EvidenceProjection` + orchestrator)
4. `commands/graph.rs` (fail-closed clock helper + context inject + stderr flip — provider YOK)
5. `architecture_guards.rs` (metric_projection.rs deny korunur + ownership guard yeni)
6. `analyze_bridge_flow.rs` (stderr assertion lockstep update — binary CLI diagnostics)
7. Workspace validation (`RUSTFLAGS="-D warnings" cargo test --workspace --exclude osp-desktop`)
8. Doküman/count updates (HANDOFF/STATUS/run-metadata — frozen/current ayrımı)

---

## Doküman güncellemeleri (frozen/current ayrımı — tur 2 P3 + tur 3 P3-10)

- **run-metadata.md:** yalnız **Current protocol metadata** güncellenir (osp-cli test counts,
  evidence_projection.rs boundary). **Frozen snapshot değişmez.**
- **run-metadata.json:** **untouched** — PR D bu karma frozen/current metadata yüzeyini değiştirmez
  (tur 3 P3-10: mevcut JSON stratum 22 / cumulative_trybuild_context 26 tutarsızlığı borç olarak kalır;
  PR D compile-fail eklemediği için JSON'a dokunmaz).
- **STATUS.md:** osp-cli test counts + Faz 8b/PR D completion.
- **HANDOFF.md:** PR D completion entry + PR E/G/F roadmap güncelleme.

---

## HANDOFF bullet'leri (PR D sonrası)

- **Anchoring consumer gap (tur 2 P2 düzeltme — review DEĞİL):** production consumer henüz yok.
  `AnchorPipeline::run_with_source` çağıran anchoring/ingest/evaluate CLI surface future work.
  Provider gerçek consumer `AnchorGateContext.code_evidence` + scorer; `graph init` bunu çalıştırmıyor.
- **Persistence milestone (PR G):** `PersistedObservedCodeEvidence` schema version + validated
  restore + latest/history politikası + deterministic ordering + upsert/append semantics + snapshot
  integration + corruption tests. `ObservedCodeEvidence` Deserialize VERİLMEZ — serde-friendly
  persisted DTO `try_restore()` → runtime tip (PR C smart-constructor boundary korunur).
- **EvidenceSource abstraction (future):** `EvidenceSource = fresh analysis` (PR D) →
  `EvidenceSource = validated persisted DTO` (PR G). Consumer değişmez; provider'ı besleyen source değişir.
- **v1.4 pending paper edits:** evidence_projection.rs boundary + compatibility proof semantics.
- **`measured_at` policy:** PR D `now_unix_secs()` fail-closed Result inject; PR G wall-clock source (NTP/system) policy.
- **run-metadata.json frozen/current debt:** stratum 22 vs cumulative_trybuild_context 26 tutarsızlığı
  ayrı cleanup PR (tur 3 P3-10).

---

## run-metadata: current protocol — osp-cli test counts update; run-metadata.json untouched.
