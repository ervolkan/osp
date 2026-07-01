# OSP Agent Trajectory Roadmap — Mimari Navigasyon Protokolü

> **Durum:** v0.4 — çekirdek + ürünleşme CLI/MCP tamam, G2/H/E + Paper 2 yazımı kaldı
> **Durum tarihi:** 2026-06-30 · **Son implementation milestone:** G1 merge, 2026-06-29
> **Tarihçe:** v0.1 tasarım (2026-06-30) → v0.4 A→G1 implementation tamamlandı (2026-06-29)
> **İlişki:** Paper 1 (Statik Uzay) tamamlandı → bu doküman Paper 2 (Dinamik/Agent)'nin omurgası
> **3D viewer durumu:** DURDURULDU (görsel geliştirme, agent işleri öncelik)

---

## ⭐ Mevccut Durum (2026-06-29 — G2 tamam)

**Tamamlanan implementation (A→G2):**
- ✅ **osp-core** — ontoloji (A) + predicate gate (B/B2) + planner (C) + navigator (D1) +
  gerçek measure (D2). INV-T1..T8 type-level enforced.
- ✅ **osp-llm-runtime** — gerçek LLM adapter (D3) + calibration feedback (D4).
- ✅ **osp-cli** — truth surface, mock + gerçek LLM dispatch (F1).
- ✅ **osp-mcp** — AI access surface (G1) + operator tools + navigator loop (G2).
  INV-T1 + INV-T2 canlı doğrulandı.

**Kalan implementation (sıralı):**
- ⬜ **G2c** — Corpus experiment runner (N repo × M task, RQ6-9 evidence üretimi). Paper 2 zorunlu.
- ⬜ **D5** — OspPrompt unification (prompt debt giderme, D3 complete_raw → OspPrompt.task_view).
- ⬜ **H** — osp-sdk (TypeScript/Python/Rust bindings) — opsiyonel, sona bırakıldı.
- ⬜ **E** — Trajectory correction + 3D UI (opsiyonel, sunum katmanı).

**Paper 2 yazımı:** EN SONA bırakıldı. **Minimum gate:** G2 ✅ + G2c corpus + evidence JSON
+ failure notes. H ve E opsiyonel — paper'ı geciktirmez. Kanıt `docs/paper2-notes/` notlarında
toplandı (A→G2).

**Sonraki adım önerisi:** G2c — corpus experiment runner ile Paper 2 RQ6-9 evidence üretimi.

---

## 1. Motivasyon — Neden Bu Katman?

Mevcut OSP **statik bir uzay** tanımlar: SCIP/tree-sitter ile metrik çıkarımı → 5-axis
coordinates → vision evaluation → tri-state witness. Bu "anlık fotoğraf" almakta mükemmel.

**Ama geliştirme sürecindeki kritik halka eksik:**

```
[İNSAN] task yazar → [LLM agent] kod yazar → [OSP] "uygun mu?" kontrol
   ↑ burası tanımsız                ↑ OSP'nin mevcut gücü
```

OSP **kısıtları** (constraints: coupling ≤ 0.30) biliyor ama **niyeti** (intent: bu
task neden var, hangi role ait) bilmiyor. Agent'a sadece "kurala uy" deniyor,
"şu yöne git" denmiyor.

**Bu doküman:** OSP'yi reaktif bir kapıdan (gate) **proaktif bir mimari navigasyon
protokolüne** taşır. Temel fikir (beyin fırtınası yorumlarından):

> **Task, doğal dilde bir iş değildir; gelecekte engine tarafından ölçülecek mimari
> durum üzerinde doğrulanabilir bir şarttır (predicate).**
> **Agent, kod üreten bir işçi değil; mimari yörüngeyi takip eden bir navigatördür.**

**Önemli ayrım (review 1 düzeltmesi):** Task bir *hareket vektörü* (movement vector)
değildir. Hareket vektörü `Δ⃗T = S_target - S_current` operatör/planner seviyesinde
hesaplanır ve agent'a **verilmez**. Agent sadece predicate'i görür (§3 hibrit model,
INV-T1). Bu ayrım korunmazsa "agent pozisyon belirler" yanılgısı doğar — ki bu OSP'nin
en temel invariant'ını (engine ölçer) ihlal eder.

---

## 2. Tez Cümlesi

> **A task is not a claimed coordinate and not a structural delta.
> A task is a verifiable measurement predicate over future engine-measured coordinates.**

Bu cümle Paper 2'nin tezidir. Mevcut paper'ın *"the engine computes the raw position"*
(§6.1) tanımını bir adım öteye taşır: sadece pozisyonun değil, **task'ın kendisinin**
epistemolojik statüsünü tanımlar.

---

## 3. Hibrit Model: Predicate (güven) + Coordinate (güç)

**Saf koordinat modeli** matematiksel olarak güçlü (Yorum 1/3'ün vektör matematiği)
ama **inv ihlali** riski taşır: agent hedef koordinatı görürse "AI söylediği için
doğru" tarafına kayar.

**Saf predicate modeli** epistemolojik olarak güvenli (inv korunur) ama **navigasyon
gücü** eksik — "0.55'in altında" tek nokta değil bir bölge tanımlar.

**Çözüm (onaylanan hibrit):** İki katmanı ayrı ontolojik seviyelere koy:

```
Matematiksel güç    → Milestone katmanı (koordinat vektörleri, Trajectory)
Epistemolojik güven → Task katmanı (measurement predicate, agent'a verilir)
```

**Kritik:** `TargetRegion.preferred_vector` ile `Task.target_predicate_set` arasındaki dönüşüm
**operator/planner seviyesindedir**. Agent predicate'i görür, koordinatı GÖRMEZ.

---

## 4. Yeni Ontolojik Tipler

### 4.1 Trajectory (Yörünge)

```rust
/// Vision'dan türetilmiş, sıralı Milestone'lar dizisi. Bir projenin "nereye
/// gideceği" planı. Operator (insan mimar / planner) tanımlar — agent DEĞİL.
pub struct Trajectory {
    pub id: TrajectoryId,
    pub vision: VisionVector,              // hedef mimari (ℝ³ + source)
    pub milestones: Vec<Milestone>,        // sıralı ara hedefler (waypoints)
    pub created_at: Instant,
    pub status: TrajectoryStatus,          // Planned / Active / Completed / Superseded
}
```

### 4.2 Milestone (Ara Hedef)

```rust
/// Trajectory üzerinde bir waypoint. Uzayda ulaşılması gereken ara koordinat.
/// Operator/planner tarafından tanımlanır; koordinat agent'a verilmez,
/// predicate'e dönüştürülür.
pub struct Milestone {
    pub id: MilestoneId,
    pub label: String,                     // "Repository layer ayrımı"
    pub target_region: TargetRegion,       // kabul bölgesi (tek nokta DEĞİL — review 1, F1)
    pub scope: MilestoneScope,             // node / module / subgraph
    pub tasks: Vec<TaskId>,                // bu milestone'a bağlı tasklar
    pub status: MilestoneStatus,           // Pending / InProgress / Achieved / Failed
}

/// Milestone tek nokta değil, KABUL BÖLGESİ tanımlar (F1 çözümü, review 1).
/// Gerçek mimaride "tam 0.55 coupling, tam 0.70 cohesion" nokta hedefleri kırılgan.
/// Region = predicate bölgesi; preferred_vector = navigasyon için ideal merkez.
pub struct TargetRegion {
    pub predicates: Vec<MetricPredicate>,  // bölgeyi tanımlayan şartlar (AND)
    pub preferred_vector: Option<RawPosition>, // ideal merkez (navigasyon, debug)
}
```

**Neden region, tek nokta değil?** (F1 formalizasyon sorunu — çözüldü)
- Multi-axis predicate (`coupling ≤ 0.55 AND cohesion ≥ 0.70`) tek nokta değil bölge.
- `preferred_vector` navigasyon/distance hesabı için ideal merkez, ama **sert kriter
  değildir** — region içinde herhangi bir nokta milestone'uAchieved yapar.
- Tek nokta hedefler (eski `target_vector: RawPosition`) kırılgandı; region esnek ama
  yine de ölçülebilir (her predicate engine-measured).


### 4.3 Task (Ölçülebilir Niyet) — HİBRİT MODELİN ÖZÜ

```rust
/// Bir Milestone'a ulaşmak için uzayda yapılması gereken ölçülebilir hareketin
/// PREDICATE SET karşılığı. Agent'a bu verilir — koordinat hedefi DEĞİL.
///
/// Multi-axis (review 2): coupling AND cohesion AND instability birlikte.
pub struct Task {
    pub id: TaskId,
    pub milestone_id: MilestoneId,
    pub label: String,                     // "Reduce StoreRepository coupling, preserve cohesion"
    pub target_predicate_set: PredicateSet, // multi-axis ölçüm şartı (epistemolojik güven)
    pub policy: TaskPolicy,                // mutation/progress policy (§4.7) — task bazlı
    pub allowed_operations: Vec<OpKind>,   // agent'ın araç kutusu (OperationPolicy Aşama C)
    pub constraints: Vec<Rule>,            // ek kısıtlar (Q6 Rule gate)
    pub status: TaskStatus,                // Pending/Assigned/InProgress/Completed/Blocked
}

/// Multi-axis predicate set (review 2 — F5 axis oscillation'ı doğal çözer).
/// Tek predicate yerine Vec + birleştirme modu.
/// review v4 #4 — Weighted duplication temizlendi: tek predicate listesi + weight Option.
pub struct PredicateSet {
    pub mode: PredicateMode,               // All (AND) | Any (OR) | Weighted (loss'a katkı)
    pub predicates: Vec<WeightedPredicate>, // tek liste (weight All/Any'de None)
    pub preferred_vector: Option<RawPosition>, // navigasyon merkezi (debug, distance)
}

pub struct WeightedPredicate {
    pub predicate: MetricPredicate,
    pub weight: Option<f64>,               // None = All/Any modda; Some(w) = Weighted modda
}

pub enum PredicateMode {
    All,      // tüm predicate'lar satisfied olmalı (AND) — default
    Any,      // en az biri (OR)
    Weighted, // loss function: weight'lerle (F5 axis oscillation)
}

pub enum TaskStatus { Pending, Assigned, InProgress, Completed, Blocked }
```

**Prensip (review v2 #2 — prensip cümlesi):**
> Predicate failure never completes a task, but under a task-specific mutation
> policy it may be accepted as a bounded progress checkpoint if engine-measured
> trajectory loss decreases and no hard invariant is violated.

(Türkçe: Predicate başarısızlığı task'ı asla tamamlamaz; fakat task bazlı mutation
policy izin veriyorsa, engine tarafından ölçülen trajectory loss azalmış ve hiçbir
hard invariant ihlal edilmemişse bounded progress checkpoint olarak kabul edilebilir.)


### 4.4 MetricPredicate (Ölçüm Şartı) — mevcut MetricValue ile uyumlu

```rust
/// Engine-measured koordinat üzerinde doğrulanabilir şart.
/// MetricValue provenance'ı korur (measured/scip/placeholder/heuristic).
pub struct MetricPredicate {
    pub metric: PredicateAxis,             // hangi eksen (coupling/cohesion/instability/...)
    pub operator: ComparisonOp,            // Lt | Le | Gt | Ge | Eq | Ne
    pub threshold: f64,                    // ölçülecek eşik değeri
    pub scope: PredicateScope,             // Node(id) | Module(name) | Subgraph(ids)
    pub required_source: Option<MetricSource>, // measured/scip zorunluysa (provenance)
    pub tolerance: f64,                    // ε — "≤ 0.55 ± 0.02"
}

pub enum PredicateAxis {
    Coupling, Cohesion, Instability, Entropy, WitnessDepth,
    RiskScore, MainSequenceDistance,
    Custom(String),                        // security.audit, wcag.compliance vb.
}

pub enum ComparisonOp { Lt, Le, Gt, Ge, Eq, Ne }

pub enum PredicateScope {
    Node(NodeId),
    Module(String),
    Subgraph(Vec<NodeId>),
}
```

### 4.5 View Ayrımı — Agent vs Internal (review 1, INV-T1 için kritik)

**En kritik teknik nokta:** Hibrit modelin güvenliği, `TargetRegion.preferred_vector`'ün
**agent'a sızmamasına** bağlı. Mevcut `Intent { target_raw: RawPosition }` bu sızıntıya
açık. Çözüm: iki ayrı view.

```rust
/// Agent'a serialize edilen görünümdür. KOORDİNAT HEDİFİ İÇERMEZ (INV-T1).
/// Agent bu view'ı alır, DeltaProposal üretir. Sadece predicate + mevcut ölçüm +
/// izinli operasyonlar + kısıtlar.
pub struct AgentTaskView {
    pub task_id: TaskId,
    pub label: String,                     // "Reduce StoreRepository coupling"
    pub current_measurement: RawPosition,  // engine-measured mevcut durum
    pub target_predicate: MetricPredicate, // ölçüm şartı (koordinat DEĞİL)
    pub allowed_operations: Vec<OpKind>,   // agent'ın araç kutusu
    pub constraints: Vec<Rule>,            // ek kısıtlar
}

/// Engine/planner/debug içindir. Koordinat hedefini taşır ama agent'a
/// serialize edilmez. `Intent::from_task` bu view'ı kullanır.
pub struct InternalTaskPlan {
    pub task_id: TaskId,
    pub milestone_target_vector: RawPosition,  // koordinat hedefi (operator seviyesi)
    pub task_predicate: MetricPredicate,       // predicate (agent'a verilir)
    pub tolerance: f64,
}

/// Agent'ın yapabileceği structural operasyonlar (review 2 önerisi — Task.allowed_operations
/// için). Planner, Task'a "coupling düşürmek için sadece import'ları soyutla, yeni modül
/// yaratma" diyebilir.
pub enum OpKind {
    AddImport, RemoveImport,
    AddAbstraction,      // interface/trait ekle (dependency inversion)
    ExtractModule,       // mevcut kodu yeni modüle taşı
    AddNode, RemoveNode,
    AddEdge, RemoveEdge,
    ModifyEntity,        // kind/mass/metadata (RawPosition hariç)
}
```

**Serileştirme kuralı:** `AgentTaskView` JSON'a çevrilirken `target_predicate_set`'in
**nominal koordinat karşılığı** asla eklenmez. Engine, `InternalTaskPlan`'dan
`AgentTaskView` üretirken koordinatı düşürür. Bu `docs/invariant-spec.md`'de formal
garanti edilir (INV-T1).

### 4.6 TaskAttempt + AttemptOutcome (review 1 + review v2 #3,#5)

Bir Task birden fazla deneme gerektirebilir (örn. coupling 0.82 → hedef 0.55):
attempt 1 reddedilebilir ama **yön doğru** (0.71), attempt 2 ilerler (0.63), attempt 3
başarır (0.53). Bu ayrım olmadan sistem çok katı olur — her reddi "başarısızlık" sayar.

**Kritik ayrımlar:**
- **INV-T6:** Predicate failure ≠ negative progress. Predicate sağlanmayabilir ama
  milestone'a yaklaşmış olabilir (loss azaldı).
- **simulated vs committed (review v2 #3):** Predicate fail ederse DeltaProposal
  hiç uygulanmamış olabilir (simulated); veya progress checkpoint olarak uygulanmış
  olabilir (committed). Bu ayrım olmadan INV-T6 anlamsız.

```rust
/// Bir Task için tek bir deneme. Agent'ın bir DeltaProposal'ı → Claim → gate akışı.
pub struct TaskAttempt {
    pub id: TaskAttemptId,
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub claim_id: Option<ClaimId>,
    /// Engine tarafından simüle edilen (hypothetical graph + re-analyze) sonucu.
    /// Hard gate'ler (Q4/Q5/Q6) BUNU değerlendirir.
    pub simulated_after: RawPosition,
    /// Eğer mutation kabul edildiyse (AcceptAsProgress/AcceptAsCompleted) gerçek
    /// commit sonrası ölçüm. Reject ise None (simulated'da kaldı).
    pub committed_after: Option<RawPosition>,
    pub measured_before: RawPosition,
    /// Loss function sonucu (F5 — multi-axis trajectory loss). preferred_vector'e
    /// weighted distance. Bu INV-T6'nın quantitative temeli.
    pub loss_before: f64,
    pub loss_after: f64,
    /// Tek enum değil, zengin outcome (review v2 #5) — her boyut ayrı.
    pub outcome: AttemptOutcome,
}

/// review v2 #5 — tek enum yetmez. Gate kararını, predicate sonucunu, mutation
/// kararını, witness durumunu ayrı ayrı taşır.
pub struct AttemptOutcome {
    /// Hard gate'ler (Q4 Syntax / Q5 Vision / Q6 Rule) — deterministik.
    pub gate_decision: GateDecision,
    /// Soft gate Q5.b — predicate completion durumu.
    pub predicate_completion: PredicateCompletion,
    /// Policy'ye göre mutation kararı (§4.7 TaskPolicy).
    pub mutation_decision: MutationDecision,
    /// Witness (Q1-Q3) — mutation kabul edildiyse.
    pub witness_status: Option<WitnessStatus>,
}

pub enum GateDecision {
    PassedAll,
    RejectedBySyntax,       // Q4
    RejectedByVision,       // Q5 θ > bound
    RejectedByRule,         // Q6
    BlockedByManeuverLimit, // INV-T7 — ardışık N reject
}

pub enum PredicateCompletion {
    Completed,              // predicate satisfied → task kapanabilir
    NotCompleted,           // predicate fail — mutation policy'ye bakılır
}

/// Policy'ye göre mutation kararı (§4.7). Predicate fail = Reject DEĞİL her zaman.
pub enum MutationDecision {
    Reject,                    // simulated'da kaldı, hiç uygulanmadı
    AcceptAsProgress,          // trajectory checkpoint olarak uygulandı (loss ↓)
    AcceptAsCompleted,         // predicate satisfied, tamamlandı
    RequireOperatorApproval,   // insan review gerekli (critical domain)
}

pub enum WitnessStatus { // mevcut WitnessResult ile uyumlu
    Hold(Reason), Commit, Override,
}
```

**Üç seviye ayrımı (review v2 #2 — kritik):**
1. **Attempt accepted as progress** — loss azaldı, checkpoint, ama task açık.
2. **Task completed** — predicate satisfied (tüm + mode All).
3. **Milestone achieved** — milestone'ın tüm taskları completed.

`AcceptAsProgress` ≠ main branch merge. Progress checkpoint trajectory branch'te;
task done / milestone achieved / main merge ayrı kararlar.

### 4.7 TaskPolicy + Mutation Karar Mantığı (review v2 #2 — task bazlı policy)

```rust
/// Task bazlı mutation policy. Predicate fail olduğunda mutation reject mi,
/// progress checkpoint mı, operator approval mı — task'ın karakterine göre.
pub struct TaskPolicy {
    pub predicate_failure_policy: PredicateFailurePolicy,
    pub min_improvement_delta: f64,     // loss en az bu kadar azalmalı (improved saymak için)
    pub max_axis_regression: f64,       // hiçbir kritik eksen bu kadar bozulamaz
    pub maneuver_limit: u32,            // INV-T7 — ardışık reject limiti (default 5)
    pub allow_progress_checkpoint: bool, // AcceptAsProgress izinli mi
}

pub enum PredicateFailurePolicy {
    StrictReject,          // default — basit task, predicate fail = reject
    AcceptImprovement,     // büyük refactor — loss ↓ ise progress checkpoint
    OperatorApproval,      // critical domain (security/payment) — insan review
}
```

**Karar akışı (hard gates vs soft gate):**
```
Hard Gates (deterministik):        Soft Gate (policy-driven):
  Q4 Syntax                          Q5.b Task predicate_set
  Q5 Vision (θ ≤ bound)                    ↓
  Q6 Rule                           predicate satisfied?
  provenance / tests                      ├─ Evet → AcceptAsCompleted
       ↓                                  └─ Hayır → loss azaldı mı?
  fail → Reject (GateDecision)                   ├─ Hayır → Reject
  pass ↓                                          └─ Evet → policy:
                                                       StrictReject → Reject (record improved)
                                                       AcceptImprovement → AcceptAsProgress
                                                       OperatorApproval → RequireOperatorApproval
```

**Loss function (F5 axis oscillation):** "improved" tek eksenden değil, weighted loss'tan:
```
loss = w1·coupling_error + w2·cohesion_error + w3·instability_error
     + w4·entropy_error + w5·witness_error
improved ⟺ loss_after < loss_before − min_improvement_delta
         AND hiçbir eksen max_axis_regression değerinden fazla bozulmamış
```

**Evidence Ledger** (`TrajectoryEvidence`) her attempt'in token cost + duration + outcome'unu
kaydeder → RQ6 (token), RQ7 (task success), RQ8 (correction değeri) için ham veri.
Detay §8 Aşama B2.

---

## 5. Uyum Haritası — Mevcut OSP Tipleriyle

Bu bölüm yeni tiplerin mevcut ontolojiye **nasıl oturduğunu** gösterir.
Invariant'lar bozulmadan entegrasyon için kritik.

### 5.1 Ontolojik hiyerarşi (mevcut + yeni)

```
Trajectory (YENİ)
  └─ VisionVector (MEVCUT — vision.rs)         → hedef mimari
       └─ Milestone (YENİ)                       → ara koordinat hedefi
            └─ Task (YENİ)                       → measurement predicate
                 └─ Intent (MEVCUT — witness.rs)  → t_f, agent'ın hedefi
                      └─ Claim (MEVCUT)           → t_m, agent'ın işi
                           └─ DeltaProposal (MEVCUT — agent.rs) → structural change
                                └─ Engine computes P_raw (MEVCUT)
                                     └─ Q5 Vision Gate (MEVCUT)
                                     └─ Q5.b Predicate Gate (YENİ) ← Task kontrolü
```

### 5.2 Mevcut tiplerin yeniden kullanımı

| Mevcut tip | Yeni katmanda rolü | Değişiklik |
|---|---|---|
| `VisionVector` | Trajectory'nin hedefi | Yok (mevcut) |
| `RawPosition` | TargetRegion.preferred_vector | Yok (mevcut) |
| `MetricPredicate` | TargetRegion.predicates, Task.target_predicate_set | Yok (mevcut + genişletme) |
| `MetricValue` + `MetricSource` | MetricPredicate'in provenance'ı | Yok (mevcut) |
| `Intent` (witness.rs) | Task → Intent dönüşümü | **target_raw → predicate'den türetilir** |
| `Claim` (witness.rs) | Task → Claim akışı | **task_id alanı eklenir** |
| `DeltaProposal` (agent.rs) | Agent'ın ürettiği structural change | Yok (mevcut) |
| `Rule` (rule.rs) | Task.constraints | Yok (mevcut) |
| `OutputContract` (agent.rs) | Task.allowed_operations kaynaklı | **allowed_ops alanı eklenir** |

### 5.3 Kritik entegrasyon noktaları

**N1: Intent.target_raw'in durumu — ÇÖZÜLDÜ (review 1, §4.5)**
Mevcut `Intent { agent, target_raw: RawPosition }` doğrudan koordinat alıyor — bu
agent'a sızma riski. Çözüm: **AgentTaskView / InternalTaskPlan ayrımı** (§4.5).
- `AgentTaskView` (agent'a serialize) — **koordinat İÇERMEZ**, sadece predicate.
- `InternalTaskPlan` (engine/planner) — koordinat taşır, agent'a verilmez.
- `Intent::from_task` predicate'in nominal koordinatını `InternalTaskPlan`'dan alır,
  ama bu sadece engine içindir. Agent serialization'ında `target_raw` çıkarılır.
**Garanti:** `docs/invariant-spec.md` INV-T1'de formal — AgentTaskView serde'de
koordinat alanı yok (serde derive ile yapısal zorunluluk).

**N2: Q5 Vision Gate genişletme**
Mevcut Q5: `θ(claim.computed_raw, vision) > bound → reject`.
Yeni Q5.b: `task.target_predicate_set.evaluate(claim.computed_raw) == false → reject`.
İki gate ardışık: önce θ (mimari sapma), sonra predicate (task hedefi).

**N3: Task → Claim zinciri**
Mevcut akış: `DeltaProposal → Claim`. Yeni: `Task → Intent(from task) → Claim(task_id)`.
Claim artık **hangi task'a hizmet ettiğini** bilir → trajectory progress tracking.

---

## 6. Invariant'lar (Korunacak Kurallar)

Yeni katman OSP'nin 15 mevcut invariant'ına zarar vermemeli. Ek invariant'lar:

- **INV-T1 (Predicate epistemolojisi):** Task, agent'a **predicate olarak** verilir.
  Koordinat hedefi (`TargetRegion.preferred_vector`) agent'a gösterilmez. Sadece operator
  ve planner (trusted) görür.

- **INV-T2 (Operator tanımlar hedef):** `Trajectory` ve `Milestone` **trusted operator**
  tarafından tanımlanır (inv #13, #15 ile uyumlu). Agent hedef belirlemez, sadece
  oraya giden structural change (DeltaProposal) üretir.

- **INV-T3 (Engine ölçer — korunmuş):** Task predicate'i **engine-measured** değer
  üzerinde değerlendirilir (`claim.computed_raw`). Agent ölçmez, engine ölçer (inv #4).

- **INV-T4 (Predicate provenance):** MetricPredicate `required_source` alanı ile
  "measured" zorunlu kılabilir. Placeholder/heuristic kaynaklı ölçümlerle task
  kapatılamaz (epistemolojik bütünlük).

- **INV-T5 (Task ≠ Claim):** Task bir **şart** (predicate), Claim bir **iş** (structural
  delta). Bir task birden fazla claim gerektirebilir; bir claim bir task'a hizmet eder.

- **INV-T6 (Failure ≠ regression — review 1):** Predicate failure negative progress
  *gerektirmez*. Bir DeltaProposal predicate'i sağlamasa bile milestone'a yaklaşmış
  olabilir (`MutationDecision::AcceptAsProgress` under AcceptImprovement policy). OSP, completion ile
  directional improvement'ı ayırır — uzun refactoring task'larında sürekli reject
  yerine progress sinyali. Aksi halde sistem çok katı olur.

- **INV-T7 (Maneuver limit — review 3):** Bir Task için ardışık N reddedilen attempt'tan
  sonra sistem kendini kilitler ve **Trajectory Deviation Alert** ile operatöre (God Mode)
  kontrol devreder. Sonsuz context-loop ve token patlaması önlenir. N operator tarafından
  yapılandırılır (default: 5).

- **INV-T8 (Progress checkpoint isolation):** `AcceptAsProgress` bir task'ı **tamamlamaz**
  ve **Mainline'a promote edilemez**. Progress yalnızca `TrajectoryCheckpoint` veya `Sandbox`
  lane içinde kalır — Mainline'a ancak predicate tamamlandığında (`AcceptAsCompleted`)
  promote edilir. Bu, "kısmi iyileştirme" ile "task bitti"nin karıştırılmasını önler (INV-T6
  ile birlikte — progress güvenli ama merge değil). MCP/G2 için kritik güvenlik ayrımı:
  agent "iyileştikçe" Mainline'ı kirletemez.

---

## 7. Adaptif Mimari Kontrol Döngüsü

Yorum 2'nin "Planla → Uygula → Kontrol Et → Düzelt" döngüsünün OSP karşılığı:

```
1. PLANLA
   Operator → Trajectory (vision + milestones)
   Milestone.target_region.predicates + preferred_vector → Task.target_predicate_set (planner, agent görmüyor)

2. UYGULA
   Agent ← Task(predicate + mevcut ölçüm + allowed_ops + constraints)
   Agent → DeltaProposal (structural change)
   Agent kabuğu → Claim (task_id, intent from task, delta_nodes/edges)

3. KONTROL ET
   Engine Q4 Syntax → Q5 Vision (θ) → Q5.b Predicate → Q6 Rule
   [Tüm gate'ler geçerse] Witness Q1-Q3

4. MUTATE + ÖLÇ
   apply_delta → re-analyze → yeni P_raw (engine ölçer)

5. DÜZELT (Trajectory Correction)
   Yeni P_raw vs TargetRegion.predicates → completion (predicate satisfied?)
   Yeni P_raw vs preferred_vector → progress/distance (loss azaldı mı?)
   Task.target_predicate_set.evaluate(P_raw) == Satisfied? → Task Completed
   Mutation policy'ye göre: AcceptAsProgress / Reject / OperatorApproval (§4.7)
   Milestone'ın tüm taskları Completed? → Milestone Achieved
   Tüm milestone'lar Achieved? → Trajectory Completed
   Sapma varsa → Trajectory replan (yeni milestone'lar)
```

---

## 8. Aşamalı İmplementasyon Planı

Her aşama **kanıt/not toplar** (docs/paper2-notes/) — paper en son yazılır.

**Ürünleşme sırası (review — CLI first, MCP second, SDK third, UI later):**
```
osp-core (✅ A/B/C/D1/D2) + osp-llm-runtime (✅ D3/D4)
  → osp-cli (✅ F1 execution surface)
  → osp-mcp (✅ G1 AI access surface) → osp-mcp G2 (operator tools + navigator loop)
  → osp-sdk (H, ⬜ integration) → osp-desktop/3D (E, ⬜ exploration)
```
MCP otorite katmanı DEĞİL, osp-core üzerinde erişim katmanıdır (INV-T1..T8 bypass edilemez).
UI/3D kanıt üretmez — keşif/görselleştirme; Paper 2 evidence CLI/MCP'den gelir.

**Mevcut durum (2026-06-29):** Çekirdek (osp-core + osp-llm-runtime) + ürünleşme
katmanlarından CLI ve MCP G1 tamamlandı. Sıradaki: G2 (MCP operator tools + navigator
loop entegrasyonu) veya H (SDK). **Paper 2 yazımı EN SONA bırakıldı** — tüm kanıt
toplama bittikten sonra data-driven yazılacak.

### Aşama A — Ontolojik Tipler ✅ TAMAMLANDI (2026-06-30)
**Hedef:** Trajectory/Milestone/Task/MetricPredicate tiplerini kodla, INV-T1..T8 type-level.
**Dosyalar:** `crates/osp-core/src/trajectory.rs` (yeni), `coords.rs`, `witness.rs`.
**Sonuç:** 13 test, INV-T1..T8 type-level enforcement. Paper2 notları: stage-A-ontology.md.

### Aşama B — Predicate Gate Integration (Q5.b) ✅ TAMAMLANDI (2026-06-30)
**Hedef:** `check_claim_predicate` + Claim.task_id + TaskBoundClaim + PredicateGate.
**Dosyalar:** `witness.rs` (Claim.task_id), `trajectory.rs` (TaskResolver, bind, PredicateGate).
**Sonuç:** 11 test (10 done-criteria + 1 ek). Paper2 notları: stage-B-predicate-gate.md.

### Aşama B2 — TaskAttempt Ledger (review 1) ✅ B'ye entegre
**Hedef:** `TaskAttempt` + `TrajectoryEvidence` + `AttemptOutcome` + `TaskPolicy`.
**Sonuç:** B ile birlikte (PredicateGateOutput AttemptOutcome üretir).

### Aşama C — Planner / Milestone Decomposition ✅ TAMAMLANDI (2026-06-30)
**Hedef:** DecompositionStrategy (deterministic) + MilestoneDecomposer + Intent::from_task.
**Dosyalar:** `trajectory.rs` (DecompositionPolicy/Strategy/Space), `witness.rs` (Intent::from_task).
**Sonuç:** 9 test. Paper2 notları: stage-C-planner.md.

### Aşama D1 — Agent Navigator Loop (mock LLM) ✅ TAMAMLANDI (2026-06-30)
**Hedef:** LlmClient trait + MockLlmClient + AgentNavigator.run_task + DeltaProposal→Claim bridge.
**Dosyalar:** `crates/osp-core/src/navigator.rs` (yeni).
**Sonuç:** 8 test, 9 boşluktan 7'si kapatıldı. Token cost accumulate (RQ6 ilk evidence).
Paper2 notları: stage-D-agent-loop.md.

### Aşama D2 — Gerçek engine measure + commit() Q5.b entegrasyon ✅ TAMAMLANDI (2026-06-30)
**Hedef:** Navigator gerçek engine measure (compute_raw_from_delta gerçek node/edge),
commit() içine Q5.b PredicateGate + permission entegrasyon, DecompositionSpace engine'den beslenir.
**Dosyalar:** `engine.rs` (`commit_task_claim` — atomic Q4→Q5→Q5.b→Q6→mutate→Q1-Q3),
`navigator.rs` (gerçek measure), trajectory.rs.
**Sonuç:** `commit_task_claim` `commit()` ile paralel ayrı pipeline (Paper 1 caller'ları
kırılmıyor). Atomik Q5.b PredicateGate. D3'te gerçek LLM ile test edildi.
**Paper2 notları:** stage-D2-real-measure.md. RQ6/RQ7 gerçek measure ile.
**Efor:** M (tamamlandı).
**Kritik:** D2 olmadan CLI navigator mock measure verir (gerçekçi değil) — çözüldü.

### Aşama D3 — Gerçek LLM Adapter ✅ TAMAMLANDI (2026-06-30) ⚠ PROMPT DEBT
**Hedef:** `RuntimeLlmClient` — gerçek LLM (GPT-4o-mini) `LlmClient` trait'ini gerçekler.
OspPrompt'tan ayrı trajectory-specific system prompt (INV-T4 warning + JSON format + feedback).
**Dosyalar:** `crates/osp-llm-runtime/src/adapter.rs` (yeni crate).
**Sonuç:** `complete_raw` custom `CompletionRequest` ile OspPrompt'u bypass eder
(OspPrompt ile AgentTaskView'ın ortak alanı yok). `map_runtime_error` hata dönüşümü.
svelte corpus'ta gerçek task denemesi yapıldı. Paper2 notları: stage-D3-real-llm.md.
**⚠ Prompt debt (review 4):** `complete_raw` shortcut'tır, kalıcı mimari karar DEĞİL.
Paper 1 `OspPrompt` typed-packet ilkesini ihlal eder → iki ayrı prompt hattı riski
(kavramsal drift). D5 aşamasında unify edilecek. Efor: M (tamamlandı, debt işaretli).

### Aşama D4 — Calibration Feedback (LLM retry optimization) ✅ TAMAMLANDI (2026-06-30)
**Hedef:** Navigator reject aldığında LLM'e kalibrasyon feedback'i döndürür → retry optimize.
`HallucinationType` (Structural/Vision/Rule/Witness/Undersupported) agent'a mesaj üretir.
**Dosyalar:** `agent.rs` (`HallucinationType::calibration_message`, `from_engine_error`),
`navigator.rs` (feedback_history accumulation in loop), `osp-llm-runtime/adapter.rs`
(trajectory_system_prompt'a feedback section).
**Sonuç:** `AgentTaskView.feedback_history` ile her reject sonrası spesifik mesaj
(Q4 syntax hatası → "modified_entities format", vs.). Daha önce kullanılmayan
`HallucinationType` navigator loop'una bağlandı. Paper2 notları: stage-D4-calibration.md.
**RQ8 adayı:** "Calibration feedback retry başarısını artırıyor mu?" — D4 implementation'da
var, G2 sonrası corpus deneylerinde ölçülebilir. Efor: S-M (tamamlandı).

### Aşama D5 — OspPrompt Unification / task_view integration ⬜ PROMPT DEBT GİDERME
**Hedef:** D3'ün `complete_raw` shortcut'ını kalıcı mimariye taşı. İki prompt hattını
(Paper 1 `OspPrompt` vs Trajectory `complete_raw`) birleştir.
**Plan:**
- `AgentTaskView` → `OspPrompt.task_view: Option<AgentTaskView>`
- `CalibrationFeedback` → `OspPrompt.feedback: Vec<String>`
- `RuntimeLlmClient` → `complete(OspPrompt)` (tek prompt hattı)
- `complete_raw` → sadece benchmark/debug için kalsın
**Gerekçe:** Paper 1 typed-packet ilkesi korunur, kavramsal drift önlenir. G2 sonrası
(navigator loop çalışırken) güvenle refactor edilebilir — API yüzeyi stabilize olduktan sonra.
**Efor:** M (G2 sonrası, opsiyonel ama önerilen).

### Aşama F1 — osp-cli (CLI-first, execution surface) ✅ TAMAMLANDI (2026-06-30)
**Hedef:** `osp` CLI binary — execution surface. osp-core API'sini çağırır, truth surface.
**Crate:** `crates/osp-cli/` (yeni). Mevcut `osp-analyze` binary'si osp-cli'ye sarılır.
**Komutlar:**
```bash
osp analyze --repo ./repo --out .osp/space.json
osp trajectory init --vision <toml>
osp trajectory attempt --task <id> --proposal delta.json --llm mock|real
osp task view <id>
osp evidence export --trajectory <id> --out evidence.json
```
**Prensip:** CLI = "truth surface". UI/MCP/SDK ne yaparsa yapsın, en altta CLI/osp-core
aynı sonucu üretmeli. CI/CD'ye koyması kolay, agent çağırabilir, Paper 2 evidence üretir.
**Dosyalar:** `main.rs` (clap dispatch), `commands/mod.rs`, `mock_llm.rs` (FileMockLlm).
**Sonuç:** Mock + gerçek LLM dispatch (generic `run_navigator<L: LlmClient>`).
Paper2 notları: stage-F-osp-cli.md.
**Efor:** M (tamamlandı).

### Aşama G1 — osp-mcp (AI access surface, MCP second) ✅ TAMAMLANDI (2026-06-29)
**Hedef:** MCP server — AI agent'ların OSP çekirdeğini güvenli kullanması. INV-T1..T8 bypass edilemez.
**Crate:** `crates/osp-mcp/` (rmcp 0.8, tokio async, stdio transport).
**Tools (G1 ilk set — 4 tool):**
```
osp_analyze_workspace     — repo → space snapshot (node/edge count, coverage)
osp_get_agent_task_view   ⭐ INV-T1 test: AgentTaskView (predicate, NO preferred_vector)
osp_check_predicate       — mevcut position ile predicate değerlendirme
osp_submit_delta          — DeltaProposal → engine measure → PredicateGate → outcome
```
**Kritik tasarım prensibi:** `MCP is not an authority layer; MCP is an access layer over osp-core.`
MCP tool çağrısı → osp-core API → invariant checks → deterministic result. MCP kendi başına
karar veremez. `osp_get_agent_task_view` INV-T1'in gerçek dünyadaki testi — **canlı server
üzerinde doğrulandı**: preferred_vector / target_region / milestone_target_vector ASLA döndürülmez.

**INV koruması (G1):**
- INV-T1: AgentTaskView serde-level (preferred_vector alanı yok) + `assert_no_coordinate_leak`
  runtime check (her agent-facing tool çıktısında forbidden token taraması).
- INV-T2: Agent mode (default) — operator tools disabled. OperatorCapability startup'ta
  inject edilir (`--mode operator`), request'ten ASLA.
- INV-T3/T4: engine ölçer (compute_raw_from_delta), source provenance (MetricSource::Scip).
- INV-T5: TaskNotFound error code (claim task-bound değilse).
- INV-T6/T7/T8: commit_task_claim (Q5.b PredicateGate + policy).

**Standart output envelope:** `osp.mcp.v1` — her tool { ok, schema_version, request_id,
tool, result/invariants_checked/warnings } veya { ok:false, error_code, message,
invariants_checked, recoverable }. Deterministic error codes: TARGET_COORDINATE_LEAK_BLOCKED,
OPERATOR_CAPABILITY_REQUIRED, PLACEHOLDER_METRIC_INSUFFICIENT, MANEUVER_LIMIT_EXCEEDED,
TASK_NOT_FOUND, WORKSPACE_NOT_REGISTERED, INVALID_DELTA_PROPOSAL.

**Workspace güvenliği:** `--workspace <path>` startup'ta alınır, canonicalize + exists
kontrolü. Agent raw path veremez (path traversal önü).

**Test:** 8 unit test + 7 INV-T1 integration test (canlı server serialization'da
forbidden token yok). Tüm workspace testleri pass.

**G2 (gelecek):** Operator-only tools (trajectory_init, task_add, milestone_decompose),
WorkspaceRegistry (multi-workspace), dry_run_delta, get_attempt_history, navigator loop
(multi-attempt LLM), gerçek LLM entegrasyon (submit_delta → navigator.run_task).

**Efor:** M (G1 tamam), G2 M-L.

### Aşama G2 — MCP Operator Tools + Navigator Loop ✅ TAMAMLANDI (2026-06-29)
**Hedef:** MCP server'a operator tools + gerçek navigator loop ekle. AI agent MCP üzerinden
multi-attempt navigator çalıştırabilsin.
**Crate:** `crates/osp-mcp/` (G1 üzerine + `osp-llm-runtime` dep).
**Tools (G2 eklemeleri — 4 yeni):**
```
osp_trajectory_init      — Operator-only (INV-T2 gate): Trajectory + VisionVector oluştur
osp_task_add             — Operator-only (INV-T2 gate): registry'ye Task (full JSON) ekle
osp_run_task             ⭐ Agent-facing: navigator loop (multi-attempt, LLM delta üretir)
osp_get_attempt_history  — Agent-facing: navigator evidence ledger (RQ6 token cost verisi)
```
**INV koruması (G2):**
- INV-T2 runtime gate (`gate_operator_tool`): agent mode'da operator tool çağrısı
  `OperatorCapabilityRequired` ile reddedilir. **Canlı doğrulandı.**
- INV-T1: `osp_run_task` AgentTaskView kullanır (navigator loop güvenli), leak check uygulanır.
- INV-T7: maneuver_limit (task.policy veya override).
- INV-T8: MutationDecision→ApplyTarget mapping (AcceptAsProgress→TrajectoryCheckpoint, ASLA Mainline).

**Core değişikliği (Send+Sync):** `LlmClient` trait'ine `Send + Sync` supertrait eklendi
(MCP `Arc<dyn LlmClient>` + spawn_blocking). MockLlmClient/RuntimeLlmClient/FileMockLlm
`Cell → AtomicUsize/Mutex` geçti. Bu, Paper 2'nin "async MCP + sync navigator" bridge'inin
pratik önkoşulu.

**`--llm {mock,real}` flag:** startup'ta LLM client inject (CLI pattern'ı). Mock offline
güvenli (CI), real OPENAI_API_KEY ile GPT-4o-mini.

**Sync→async bridge:** blocking navigator `tokio::task::spawn_blocking` ile. Mutex'ler
(workspace sync + registry sync) spawn_blocking içine taşınır.

**Mevcut `osp_submit_delta` KORUNDU** (kullanıcı kararı: ikisini de tut) — agent delta'sı
single-attempt (Q5.b gate test).

**Test:** 8 unit + 12 integration (INV-T2 gate, INV-T8 lane, navigator result INV-T1,
error code round-trip). Tüm workspace yeşil (16 grup).
**Paper2 notları:** stage-G2-osp-mcp-operator-navigator.md.
**Efor:** M-L (tamamlandı).

### Aşama G2c — Corpus Experiment Runner ⬜ SIRADAKİ (Paper 2 zorunlu)
**Hedef:** N repo × M task × {mock,real} × {strict, accept-improvement} × {feedback, no-feedback}
matrisi ile Paper 2 RQ6-9 evidence üretimi.
**Çıktı:** repo/task_type/policy/llm/attempt_count/completed/token_total/duration/loss_before/
loss_after/axis_regression tablosu + evidence JSON + failure notes (stage-X-failures.md).
**RQ'lar:** RQ6 (token cost), RQ7 (task success), RQ8 (calibration feedback), RQ9 (policy).
**Efor:** M (G2 altyapısı hazır, runner + corpus + analysis script).

### Aşama H — osp-sdk (integration, third) ⬜ MCP sonrası
**Hedef:** TypeScript/Python/Rust bindings. CLI/MCP deneyleri bittikten sonra hangi
fonksiyonların stabil olduğu ortaya çıkar → SDK yüzeyi netleşir.
**Kullanım:** LangGraph/CrewAI entegrasyon, CI/CD plugin, enterprise automation.
**Efor:** L (erken SDK gereksiz bakım yükü).

### Aşama E — Trajectory Correction + UI (opsiyonel, kanıt sonrası) ⬜
**Hedef:** Commit sonrası progress tracking, trajectory replan, 3D viewer'da gösterim.
**Bağımlılık:** 3D viewer Aşama 4 (selection glow) + D2 gerçek measure.
**Konum:** UI/3D = "explanation/exploration layer" — kanıt üretmez, keşif için.
osp-desktop/3D zaten var (Aşama 1-3 hover edge), durdu (agent işleri öncelik).
**Paper2 notları:** Adaptive control loop'un gerçek reponlarda davranışı (görsel).
**Efor:** M-L

---

## 9. Açık Formalizasyon Sorunları (çözülecek)

### F1: Predicate ↔ koordinat tutarlılığı — ÇÖZÜLDÜ (review 1)
Multi-axis predicate için `Milestone.target_vector` **tek nokta değil, TargetRegion** oldu
(§4.2). Region = predicate bölgesi + preferred_vector (ideal merkez, navigasyon için).
Her predicate engine-measured; preferred_vector sert kriter değil. Margin sorunu
`MetricPredicate.tolerance` (ε) içinde çözülür.

### F2: ε tolerance anlamı
Epsilon **Milestone başına** mı (ara hedefe yakınlık), **Task başına** mı (tek ölçümde
kabul), **Trajectory parçası** başına mı (Yorum 3'ün uyarısı: tek commit'te coupling
0.82→0.55 inmez)?
**Çözüm yolu:** Muhtemelen **Trajectory parçası başına** — her Milestone birkaç task/commit.

### F3: Genesis (Yorum 3'ün A/B sorusu)
`S_target`'ı kim tanımlar?
- **Seçenek A (önerilen):** İnsan mimar (God Mode) — invariant'lar korunur.
- **Seçenek B:** Agent PRD'den çıkarır — inv #15 ihlali riski.
**Karar:** AŞAMA A'da Seçenek A. B opsiyonel (paper conjecture, kanıtlanmamış).

### F4: Task kapanma kriteri
"Bug çözüldü ama coupling arttı → task bitmedi" (Yorum 1). Yani task kapanması
**sadece predicate == true** ile mi, yoksa + ek kalite kontrolü ile?
**Çözüm yolu:** Predicate ana kriter, Q5/Q5.b/Q6 ek güvenlik. INV-T6 (failure ≠
regression) ile completion (predicate satisfied) vs directional improvement ayrılır.

### F5: Axis Oscillation (eksen salınımı — review 3)
Ajan bir ekseni düzeltirken (coupling ↓) farkında olmadan başka eksen bozabilir
(instability ↑). Sonraki adımda o ekseni düzeltirken ilk eksen tekrar kaçar → salınım.
**Çözüm yolu (Aşama C):** Tekil eksen optimizasyonu yerine **çok boyutlu kayıp fonksiyonu**
(Loss Function) veya Pareto optimizasyonu. `max_axis_regression` aşımı → `MutationDecision::Reject`
bu durumu tespit eder (distance arttı) → agent'a "eksenler arası gerilimi minimize et"
sinyali. `preferred_vector` navigasyon merkezi olarak salınımı kırar.
**Paper materyali:** RQ8 adayı — multi-axis trajectory'nin single-axis'ten üstünlüğü.

### F6: Sonsuz döngü / token patlaması (Maneuver Limit — review 3)
Agent üst üste aynı kuralı çiğnemeye devam ederse context-loop'a girer, token maliyeti
patlar. **Çözüm yolu (INV-T7):** Bir Task için ardışık N (default 5) reddedilen attempt
sonra **Trajectory Deviation Alert** → operatöre (God Mode) kontrol devri. N operator
tarafından yapılandırılır (task bazlı: karmaşık refactor = 10, basit = 3).
**Paper materyali:** "deterministic gating'in token maliyetine etkisi" — RQ6 verisi.

---

## 10. Paper 2 Not Toplama Mekanizması

Dizin: `docs/paper2-notes/` (her aşamada ayrı not)

```
docs/paper2-notes/
  README.md                    — dizin yapısı + hangi not ne zaman yazıldı
  stage-A-ontology.md          — ontolojik kararlar, invariant ispatları
  stage-B-predicate-gate.md    — Q5.b deterministik reddin etkisi
  stage-B2-attempt-ledger.md   — TaskAttempt evidence, RQ6/RQ7/RQ8 ham veri
  stage-C-planner.md           — task dematerialization matematiği, axis oscillation
  stage-D-agent-loop.md        — token maliyeti, task success, maneuver limit
  stage-X-failures.md          — başarısız denemeler (review 1 — Paper 2 için değerli)
  evidence/                    — ham ölçümler (JSON), corpus sonuçları
    trajectory-bench-svelte.json
    predicate-gate-stats.json
```

**`stage-X-failures.md` (review 1 önerisi):** Başarısız denemeler Paper 2'yi güçlendirir:
agent predicate'i yanlış yorumladı, predicate çok katıydı, placeholder metric ile task
kapatılmaya çalışıldı, multi-axis predicate conflict oluştu, DeltaProposal doğruydu ama
Q6 rule gate reddetti. Bu edge case'ler makaleyi gerçekçi kılar — sadece başarıları değil,
sistemin sınırlarını da gösterir.

**Disiplin:** Her implementasyon aşaması bitiminde o aşamanın notu yazılır.
Paper 2, bu notlardan data-driven yazılır (iddia değil, kanıt).

---

## 11. Paper Stratejisi (iki makale)

### Paper 1 (Mevcut — Statik Uzay) — TAMAMLANDI
SCIP + tree-sitter + 5-axis + vision + witness + tri-state. Kanıtlanmış (23 repo).
**Trajectory referansı:** §11 Future Work'te 1-2 paragraf — "biz bu boşluğun farkındayız".

### Paper 2 (Dinamik/Agent) — bu dokümanın omurgası
"Architectural Trajectory Navigation: From Static Space to Dynamic Software Physics"
- §1 Trajectory ontolojisi (§4 bu doküman)
- §2 Task dematerialization (§9 F1 çözüldü — TargetRegion)
- §3 Adaptive control loop (§7)
- §4 Deterministic predicate gating (Aşama B) — progress ayrımı (INV-T6)
- §5 Attempt evidence & token cost (Aşama B2+D — RQ6)
- §6 Task success (Aşama D — RQ7)
- §7 Multi-axis oscillation (Aşama C, F5)

**RQ adayları (review 4 ile genişletildi):**
- **RQ6 (token cost):** Trajectory prompt toplam token maliyetini düşürüyor mu?
- **RQ7 (task success):** Predicate gate'li navigator, gatesiz/tek-shot agent'a göre daha güvenli mi?
- **RQ8 (calibration feedback — yeni, D4):** Calibration feedback retry başarısını artırıyor mu?
  (D4 implementation'da var; with-feedback vs no-feedback A/B testi G2c'de ölçülecek.)
- **RQ9 (policy — yeni, review 4):** AcceptAsProgress policy, strict reject'e göre uzun refactor
  task'larında token/attempt maliyetini azaltıyor mu? (INV-T6/T8 ile birlikte — progress güvenli
  ama merge değil, manevra maliyetini düşürür mü?)

**Tek makaleye sığdırma tuzağı:** Trajectory + Navigator + döngü + maliyet hepsini
bir paper'a sıkıştırmak → hakemler "çok iddialı, deneysel doğrulama yok" der.
Paper 1 statik uzayı oturtur, Paper 2 onun üzerine inşa eder.

### Paper yazım disiplini — EN SONA BIRAKILDI (2026-06-29 kararı)
**Paper 2 yazımı tüm implementation aşamaları bittikten SONRA yapılacak.** Ara yazım yok,
draft yok. Gerekçe:
1. Kanıt önce — her iddianın karşılığı `docs/paper2-notes/` notlarında data olmalı
   (iddia değil, kanıt). Şu an A→G1 kanıtı toplandı, G2/H/E kaldı.
2. API churn riski — SDK (H) ve G2 bittikten sonra hangi fonksiyonların stabil olduğu
   netleşir. Erken yazım refactor yükü doğurur.
3. Hakem psikolojisi — "kanıtlanmamış sistem" reddi riski. Çekirdek + CLI + MCP + SDK
   tamam + gerçek corpus deneyleri → "calmly shown" paper.

**Paper 2 minimum gate (review 4 netleştirmesi):**
```
ZORUNLU (Paper 2 için):
  ✅ G2 — MCP operator tools + navigator loop
  ⬜ Gerçek LLM corpus deneyleri (G2c — N repo × M task matrisi)
  ⬜ Evidence JSON export (RQ6/RQ7/RQ8/RQ9 ham veri)
  ⬜ Failure notes (stage-X-failures.md — başarısız denemeler)

OPSİYONEL (Paper 2'yi geciktirMEZ):
  ⬜ H — osp-sdk (ürünleşme/entegrasyon katmanı, paper için gerekmez)
  ⬜ E — 3D UI / trajectory correction visualization (sunum katmanı)
```
**Sonuç:** G2 + corpus deneyleri yeterli evidence üretince Paper 2 yazımı başlayabilir.
H ve E beklenmez — SDK ve 3D, paper'ı gereksiz geciktirir.

---

## 12. Karar Günlüğü

| Tarih | Karar | Gerekçe |
|---|---|---|
| 2026-06-30 | İki makaleye bölme | Paper 1 kanıtlanmış, Paper 2 teorik. Bölünme reddi önler. |
| 2026-06-30 | Hibrit model (predicate + coordinate) | Epistemolojik güven + matematiksel güç birlikte. |
| 2026-06-30 | Task = measurement predicate | INV-T3 (engine ölçer) korunur. |
| 2026-06-30 | Genesis = Seçenek A (operator) | INV-T2 (operator tanımlar) korunur. |
| 2026-06-30 | Paper en son, kod önce | Kanıtsız makale zaman kaybı. Data-driven yazım. |
| 2026-06-30 | 3D viewer durduruldu | Görsel geliştirme, agent işleri öncelik. |
| 2026-06-30 | 6 iyileştirme (3 review) — v0.2 | "Task=vektör"→predicate, AgentTaskView ayrımı, TaskAttempt/Ledger (B2), TargetRegion (F1), Maneuver Limit (F6), INV-T6/T7, OpKind, failures.md. |
| 2026-06-30 | INV-T6 (failure ≠ regression) | review 1 — completion vs directional improvement ayrımı. |
| 2026-06-30 | INV-T7 (maneuver limit) | review 3 — sonsuz döngü/token patlaması önleme, God Mode devri. |
| 2026-06-30 | F1 ÇÖZÜLDÜ — TargetRegion | review 1 — tek nokta değil bölge + preferred_vector. |
| 2026-06-30 | F5 Axis Oscillation | review 3 — multi-axis kayıp fonksiyonu, Aşama C. |
| 2026-06-30 | F6 Maneuver Limit | review 3 — INV-T7 ile çözüldü. |
| 2026-06-30 | invariant-spec.md önceliği | review 1 — kod yazmadan formal invariant spec. |
| 2026-06-30 | A/B/B2/C/D1 TAMAMLANDI | Çekirdek (ontology + gate + planner + navigator mock) hazır. 478 test. |
| 2026-06-30 | CLI-first + MCP ürünleşme sırası | review (beyin fırtınası) — CLI first, MCP second, SDK third, UI later. Çekirdek değişmez; CLI/MCP katmanları eklenir (§8 F/G/H). MCP otorite değil, osp-core erişim katmanı. UI/3D keşif, kanıt CLI/MCP'den. |
| 2026-06-30 | D2 TAMAMLANDI | `commit_task_claim` atomic Q5.b pipeline (commit()'e paralel, Paper 1 caller'ları kırılmıyor). |
| 2026-06-30 | D3 TAMAMLANDI | `RuntimeLlmClient` gerçek LLM (GPT-4o-mini), OspPrompt'u bypass eden trajectory prompt. |
| 2026-06-30 | D4 TAMAMLANDI | Calibration feedback — `feedback_history` + `HallucinationType::calibration_message` navigator loop'a bağlandı. |
| 2026-06-30 | F1 TAMAMLANDI | osp-cli (truth surface) — mock + gerçek LLM dispatch. |
| 2026-06-29 | G1 TAMAMLANDI | osp-mcp (rmcp 0.8) — 4 agent-facing tool, INV-T1 canlı doğrulandı (preferred_vector sızıntısı yok). |
| 2026-06-29 | Paper 2 yazımı EN SONA bırakıldı | Tüm implementation (G2/H/E) bitene kadar paper yazımı yok. Kanıt önce, data-driven yazım. API churn riski (SDK bekleniyor). |
| 2026-06-29 | Review 4 entegrasyonu | D3 prompt debt işaretlendi, D5 (OspPrompt unification) eklendi. INV-T8 §6'ya eklendi. Paper 2 minimum gate netleştirildi (G2+corpus zorunlu, H/E opsiyonel). RQ8 (calibration) + RQ9 (AcceptAsProgress policy) adayları eklendi. Tarih satırı kronolojik düzeltildi. |
| 2026-06-29 | G2 TAMAMLANDI | MCP operator tools (trajectory_init, task_add) + navigator loop (osp_run_task) + evidence history. INV-T2 runtime gate canlı doğrulandı. `LlmClient: Send + Sync` (Cell→AtomicUsize/Miosk). `--llm mock\|real` flag. |
| 2026-06-29 | G2 kullanıcı kararı: ikisini de tut | `osp_submit_delta` (agent delta single-attempt) KORUNDU + `osp_run_task` (navigator loop) EKLENDİ. İki farklı semantik (delta test vs uçtan uca loop). |

### Review kaynakları (v0.2 iyileştirmeleri)
- **Review 1 (teknik):** AgentTaskView/InternalTaskPlan ayrımı, TaskAttempt/Ledger, PredicateGateResult, TargetRegion, INV-T6, failures.md, B2 aşaması, "task=vektör" düzeltme.
- **Review v2 (olgunlaştırma):** target_vector referans temizliği, TaskPolicy (task-bazlı mutation), simulated/committed ayrımı, PredicateSet (multi-axis), AttemptOutcome (zengin struct), loss function (F5), prensip cümlesi (progress≠merge), OperationPolicy yol haritası.
- **Review 2 (stratejik):** OpKind enum, multi-agent Paper 3 kapısı.
- **Review 3 (risk):** Maneuver Limit (INV-T7), Axis Oscillation (F5), 3D durdurma teyidi.

---

## 13. Sonraki Adım

Bu doküman **review** için hazır. Rafinasyon sonrası:
1. `docs/paper2-notes/` dizin yapısını kur
2. Aşama A (ontolojik tipler) implementasyonuna başla
3. Her aşamada paper2-notes güncelle

---

*Bu doküman beyin fırtınası (3 yorum + kullanıcı değerlendirmesi) sonucu ortaya çıktı.
Kaynak: Yorum 1 (Vision Path/Trajectory), Yorum 2 (t_f/t_m/t_c uyumu), Yorum 3
(fizik metaforu, Sınır Değer Problemi). Hibrit predicate model: kullanıcı kararı.*
