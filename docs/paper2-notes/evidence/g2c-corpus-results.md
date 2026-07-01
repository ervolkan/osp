# G2c Corpus Matrix Results — Paper 2 RQ6-9 Evidence (Harness MVP)

> **Tarih:** 2026-06-29
> **Etiket:** HARNESS VALIDATION (paper evidence değil — review 5 #3)
> **Backend:** Mock LLM (deterministik), `corpus_kind: "local-crate-subtree"`
> **Kaynak:** `g2c-harness-matrix-mock.json` (24 cell, 3 repo × 2 task × 2 policy × 2 feedback)
> **Runner:** `crates/osp-analyzer/examples/g2c_corpus_matrix.rs`

## 1. Experiment Design

### Matris
```
Corpus: 3 local crate subtree (osp-core, osp-cli, osp-analyzer/src)
Tasks:  CouplingReduction (coupling ≤ 0.55), InstabilityReduction (instability ≤ 0.60)
Policy: StrictReject, AcceptImprovement (allow_progress_checkpoint=true)
Feedback: With, Without (NoFeedbackWrapper)
LLM: Mock (FeedbackSensitiveMock + NoFeedbackWrapper)
Maneuver limit: 5
→ 3 × 2 × 2 × 2 = 24 experiment cell
```

### Review 5 entegrasyonu
- **#1 Feedback-sensitive mock:** `FeedbackSensitiveMock` — with/without feedback dalında farklı proposals
- **#2 Incremental proposals / state accumulation:** AcceptImprovement `allow_progress_checkpoint=true` ile progress state ilerletir; StrictReject state sabit
- **#4 Deterministik top-offender:** Node(0) değil, highest coupling/instability score ile node seçimi
- **#6 Zengin evidence şeması:** run_id, git_commit, target_node_id/role/reason, loss_before/after, regression_axes, feedback_count, per-attempt ledger

## 2. Mock Controlled Results

### Outcome dağılımı
```
final_outcome              count
───────────────────────────────
ExceededManeuverLimit         24
Completed                      0
```

**Tüm 24 hücre ExceededManeuverLimit.** Hiçbir task Completed değil.

### Evidence ledger (per-attempt entries)
```
Toplam TrajectoryEvidence entry: 0
```

**0 evidence entry.** Navigator hiçbir attempt'i `commit_task_claim`'e götürmedi.

### Kök neden analizi
Mock proposals iki tür:
- `bad_format_proposal()` — boş (new_nodes=[], new_edges=[]) → navigator empty-proposal check → `continue` (evidence push edilmez)
- `valid_proposal()` — tek isolated Module node (target node'a edge yok)

`valid_proposal` delta node'u hypothetical graph'a ekleniyor ama **target node'a bağlanmıyor**. `compute_raw_from_delta` target node'un coupling'ini değiştirmiyor (yeni isolated node target'in incoming-edge sayısını etkilemez). Predicate `coupling ≤ 0.55` target node üzerinde — sağlanmıyor → `commit_task_claim` reject veya Q4 geçip predicate fail.

**Sonuç:** RQ8/RQ9 "Completed" sinyali bu MVP'de görünmüyor. Bu, mock proposal realism'inin yetersiz olduğunu gösteriyor — delta node target'a **bağlanmalı** (edge remove/add) ki coupling gerçek düşsün.

### RQ8 (calibration feedback)
```
With feedback:    12 cells, 0 completed, 0 evidence entries
Without feedback: 12 cells, 0 completed, 0 evidence entries
```
**MVP sonucu:** feedback farkı görünmüyor (her iki kol da limit). **Yorum:** feedback-sensitive mock çalışıyor ama proposals predicate sağlamadığı için feedback'in "retry başarısı" etkisi ölçülemiyor. Gerçek LLM (G2c-4) veya target-edge'li mock proposals (G2c-2 fix) gerek.

### RQ9 (policy) — G2c-3 synthetic controlled fixture ✅ İLK KANIT
```
synthetic/StrictReject:     ExceededManeuverLimit attempts=3, completed=false
synthetic/AcceptImprovement: Completed attempts=3, completed=true
```
**G2c-3 sonucu (arkadaş review 8):** Policy accumulation mekanizması kanıtlandı.
- **AcceptImprovement** + incremental removal (3 attempt): state ilerler (coupling 0.80→0.75→0.667→0.50), 3. attempt'te predicate satisfied → **Completed**
- **StrictReject** + aynı incremental removal: state donmuş (coupling hep 0.80 ölçülür), predicate hep NotCompleted → **ExceededManeuverLimit**

**Paper 2 cümlesi:** "OSP'de progress checkpoint politikası, uzun refactor benzeri görevlerde state'i adım adım hedefe yaklaştırırken, strict reject politikası aynı task'ı ilerletemez."

**Etiket (review 8 #1):** synthetic controlled fixture — gerçek repo corpus değil. Policy accumulation *mekanizması* kanıtlandı; external corpus genellemesi G2c-5'te.

**Local crate corpus (G2c-3 witness fix sonrası):** navigator `min_approvers=0` fix'i ile local crate corpus artık Completed üretiyor (G2c-1'in 0/24 gizli sebebi witness gate idi). Bu ek bir bulgu — navigator witness policy ayrı bir konu (operator approval vs auto-approve).

## 3. Real LLM Preliminary Results

**G2c-4 (bu PR altyapı hazır, manual run):**
```bash
OPENAI_API_KEY=... cargo run --example g2c_corpus_matrix --release -- --llm real --out evidence-real.json
```
Gerçek LLM (GPT-4o-mini) ile cost-limited küçük subset. Bu MVP'de çalıştırılmadı (API maliyeti).

## 4. Threats / Limitations (review 5 #9)

### Internal validity
1. **Local crate subset bias:** corpus local workspace crate'leri — workspace context, git root, full dependency graph eksik. Paper evidence için external cloneable corpus (G2c-5) gerek.
2. **Mock LLM determinism:** scripted proposals gerçek LLM davranışını temsil etmez. RQ8/RQ9 mekanizma testi için yeterli; external validity iddiası taşımaz.
3. **Proposal script realism:** `valid_proposal` isolated node ekler — gerçek refactor delta'sı değil. Target node coupling'ini düşürmüyor. **Bu MVP'nin en zayıf noktası.**
4. **Cohesion source insufficiency (INV-T4):** Rust subset'lerde SCIP index yok → cohesion Placeholder → task `SourceInsufficient` skip. Bu yüzden MVP cohesion task değil instability task kullandı.

### Construct validity
5. **Small corpus:** 3 crate, 24 cell. İstatistiksel güç yok. Trend/gösterge amaçlı.
6. **Axis regression ölçümü:** evidence boş olduğu için regression_axes boş kaldı. Gerçek evidence ile dolacak.

### External validity
7. **Real LLM stochasticity:** G2c-4 manual run olmadı. Gerçek token cost (RQ6) ve success rate (RQ7) için gerekli.
8. **Maneuver limit:** 5 sabit. Daha yüksek limit ile Completed oranı artabilir ama mock proposals yine de predicate sağlamayabilir.

## 5. Sonraki adımlar (G2c-2 / G2c-3 / G2c-5)

- **G2c-2 fix:** Target-edge-aware mock proposals — delta node target node'a edge remove/add ile bağlansın, coupling gerçek düşsün. RQ8 with/without Completed farkı görünür.
- **G2c-3 fix:** Incremental coupling-dropping proposals (0.82→0.71→0.63→0.53). RQ9 AcceptImprovement progress ilerletir, StrictReject state sabit.
- **G2c-4:** Gerçek LLM küçük subset (manual, cost-limited).
- **G2c-5:** External corpus (chalk/click/cobra), paper-ready evidence.

## 6. Paper 2 etkisi

Bu MVP "harness validation" başarılı: **matris koşuyor, JSON üretiliyor, navigator loop çalışıyor, INV-T1..T8 enforced, deterministik.** Ama RQ8/RQ9 **Completed** sinyali için proposal realism fix gerek (G2c-2/G2c-3). Bu, Paper 2 yazımı için gerçek bulgu: "navigator loop altyapısı çalışıyor ama mock proposal'lar gerçek refactor etkisi yaratmıyor — gerçek LLM ve target-aware proposals gerek."
