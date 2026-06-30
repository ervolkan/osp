# OSP Invariant Specification — Formal Epistemological Contracts

> **Durum:** v0.1 — Aşama A kodlamadan ÖNCE formal kesinlik (review 1 önerisi)
> **Tarih:** 2026-06-30
> **Amaç:** Kod yazarken epistemolojik sınırların bulanıklaşmasını önlemek.
> En büyük tehlike (review 1): feature eklendikçe invariant'ların erozyona uğraması.
> Bu spec her invariant için **yapısal garanti** (type-level) + **test** + **ihlal örneği** tanımlar.

## İlişkili Dokümanlar
- `docs/agent-trajectory-roadmap.md` — INV-T1..T7'nin ontolojik bağlamı
- `docs/paper-draft-v2.6.md` — INV #1..#15'in paper karşılığı
- Kod: `crates/osp-core/src/*.rs` — type-level enforcement

---

## Bölüm A — Mevcut 15 Invariant (Paper 1, statik uzay)

Bu invariant'lar zaten implemente edildi, type-level enforced, test edildi
(review v3 #5: **status her birinde belirtilmedi** — çoğu `implemented/tested`, ama bazıları
stub, örn. INV #5 Lazy diffusion `stub`, INV #6 admin override `implemented`).
Aşama A kodlaması bunları **bozmamalı** — her yeni tip bu listeye uyum kontrolünden geçer.

**Status gösterimi (review v3 #5):** Bölüm A'da her invariant için ayrı Status satırı
eklenmedi (15 invariant, çoğu implemented/tested zaten paper §6.3 ile doğrulandı). Aşağıda
sadece **stub/planned** olanlar işaretlenir:
- INV #5 Lazy diffusion → **stub** (Faz 6+)
- INV #11 LLM stateless → **implemented/tested** (osp-llm-runtime, 9 test)
- Diğerleri INV #1-4, #6-10, #12-15 → **implemented/tested** (osp-core, 136+ test)

Bölüm B'deki INV-T1..T8 için **her birinde ayrı `Status:` satırı** var (Aşama A'da implement).

### INV #1 — Author-witness rejection
**Tanım:** Bir Claim'in yazarı, kendi claim'inin şahidi (approver) olamaz.
**Yapısal garanti:** `CanonicalWitnessSet::canonicalize_for(author)` author'ı çıkarır.
Agent'ın kendi çalışmasını onaylaması ontolojik olarak imkânsız (typle-level, runtime değil).
**Test:** `author_excluded_from_approvers`.
**İhlal örneği:** Agent kendi PR'ini onaylar → self-dealing.

### INV #2 — EvidenceEvent dedup
**Tanım:** Kanıt dedup birimi `(source, actor, claim)` üçlüsüdür.
**Yapısal garanti:** `EvidenceEvent` dedup key bu üçlü; HashSet ile otomatik.
**Test:** `duplicate_evidence_collapsed`.
**İhlal örneği:** Aynı CI run iki kez sayılır → quorum şişer.

### INV #3 — Tri-state witness classification
**Tanım:** Bir Claim'in ontolojik durumu 3'ten fazladır değil: Unobservable-locally /
Unobservable-globally / Observable. Negative claim yok (placeholder → "bilmiyoruz").
**Yapısal garanti:** `WitnessStatus` enum (3 variant).
**Test:** `placeholder_is_not_negative`.
**İhlal örneği:** "Bu repo test yok" (negative) yerine "locally unobservable" (epistemik).

### INV #4 — RawPosition/DerivedPosition separation
**Tanım:** Raw pozisyon (x/y/z/w/v) θ hesabına **girdi**; Derived (u/θ/risk) **çıktı**.
Dairesellik yok. Agent pozisyon declare edemez — engine compute eder.
**Yapısal garanti:** `Position { raw, derived }` ayrık tipler; `compute_derived` tek yönlü.
**Test:** `derived_not_in_theta_input`.
**İhlal örneği:** Agent "coupling 0.3" der, engine kabul eder → hallucination.

### INV #5 — Lazy diffusion
**Tanım:** Diffusion hesabı lazy ( talep edilince), eager değil.
**Yapısal garanti:** `DiffusionDeviation` stub, lazy evaluation.
**Test:** (stub — Faz 6+).
**İhlal örneği:** Her commit'te tam diffusion → O(n²) maliyet.

### INV #6 — Incremental space commit
**Tanım:** Space mutation incremental (apply_delta), full rebuild değil.
**Yapısal garanti:** `apply_delta` mutation-only, infallible.
**Test:** `apply_delta_incremental`.
**İhlal örneği:** Her commit'te space sıfırdan kurulur → event-sourcing bozulur.

### INV #7 — Admin override flag
**Yapısal garanti:** `WitnessResult::Override` admin flag ile.
**Test:** `admin_override_bypasses_quorum`.
**İhlal örneği:** Admin quorum'u manuel skip eder → denetimsiz.

### INV #8 — Network-free core
**Tanım:** osp-core ağ/IO bağımlılığı yok (pure computation).
**Yapısal garanti:** `Cargo.toml` — no network crates.
**Test:** `cargo tree` ile network crate yokluğu.
**İhlal örneği:** core'a HTTP client eklenir → determinizm kaybı.

### INV #9 — WitnessSet-based operator
**Tanım:** W(C, Ω) pure fonksiyon — Ω `WitnessSet`, mutasyon yok.
**Yapısal garanti:** `evaluate(claim, omega) -> WitnessResult` (no &mut).
**Test:** `evaluate_is_pure`.
**İhlal örneği:** evaluate space'i mutate eder → side effect.

### INV #10 — Pure Instability axis
**Tanım:** z (instability) Martin I saf — D (main-seq distance) ayrı metric, z'ye gömülü değil.
**Yapısal garanti:** `DerivedPosition.main_sequence_distance` ayrı alan.
**Test:** `instability_not_conflated_with_d`.
**İhlal örneği:** D, z'ye gömülür → Martin teorisi bozulur.

### INV #11 — LLM stateless
**Tanım:** osp-llm-runtime agent state tutmaz (stateless HTTP).
**Yapısal garanti:** `complete()` tek istek, no session.
**Test:** `runtime_stateless`.
**İhlal örneği:** Runtime conversation history tutar → context drift.

### INV #12 — OutputContract deterministic reject
**Tanım:** LLM çıktısı şemaya uymazsa Q4'te deterministik reddedilir.
**Yapısal garanti:** `OutputContract::validate()` -> `Result<(), SyntaxViolation>`.
**Test:** `output_contract_rejects_invalid`.
**İhlal örneği:** Malformed JSON "best-effort" parse edilir → hallucination.

### INV #13 — PermissionMask trusted-operator assigned
**Tanım:** PermissionMask agent tarafından alınmaz, trusted operator atar.
**Yapısal garanti:** `PermissionMask` constructor operator-only.
**Test:** `permission_operator_assigned`.
**İhlal örneği:** Agent kendi permission'ını yükseltir → privilege escalation.

### INV #14 — Prompt as typed data packet
**Tanım:** Prompt doğal dil string değil, typed `OspPrompt` data packet.
**Yapısal garanti:** `OspPrompt` struct, serde serialize.
**Test:** `prompt_typed_not_string`.
**İhlal örneği:** Prompt string concat → injection, drift.

### INV #15 — Custom axis registration trusted-operator only
**Tanım:** Agent yeni axis tanımlayamaz, sadece trusted operator.
**Yapısal garanti:** `Axis` registration operator-only API.
**Test:** `agent_cannot_register_axis`.
**İhlal örneği:** Agent "vibes" axis ekler → "software physics" bozulur.

---

## Bölüm B — Yeni 8 Trajectory Invariant (Paper 2, dinamik katman)

Bu invariant'lar Aşama A'da implement edilecek. INV #1..#15 ile **çelişmez**,
onların üzerine inşa edilir. Her biri yapısal (type-level) garanti ister.

### INV-T1 — Predicate epistemolojisi (Hibrit modelin kalbi)
**Status:** planned (Aşama A)
**Tanım:** Task, agent'a **predicate** olarak verilir. **Hedef koordinatı** (target coordinate)
agent'a serialize edilmez: `TargetRegion.preferred_vector`, `InternalTaskPlan.milestone_target_vector`,
`Milestone.target_region`. **Mevcut engine-measured koordinat** (`current_measurement`) ise
görülebilir — agent mevcut durumu bilmeli (nerede olduğunu), ama hedefi değil.
**Yapısal garanti:** `AgentTaskView` serde struct'ında **target coordinate alanı YOK**
(`current_measurement: RawPosition` var — bu mevcut durum, serbest). `InternalTaskPlan` ve
`AgentTaskView` ayrık tipler; dönüşüm tek yönlü (engine→agent).
**Test:** `agent_task_view_has_no_target_coordinate` — serde çıktısında target sızıntı
kontrolü (spesifik alan adları, genel "coordinate"/"vector" değil):
```rust
assert!(!json.contains("target_vector"));
assert!(!json.contains("preferred_vector"));
assert!(!json.contains("milestone_target_vector"));
assert!(!json.contains("target_raw"));
assert!(!json.contains("target_region"));
// current_measurement SERBEST — mevcut engine-measured durum, hedef değil
```
**İhlal örneği:** Agent "hedef coupling 0.55" koordinatını görür → "AI söylediği için
doğru" — INV #4 (engine ölçer) erozyona uğrar. **Dikkat:** `current_measurement` sızması
ihlal DEĞİL — agent mevcut konumunu bilmeli. **Ek (review v4 #5):** `PredicateSet` ve
`TargetRegion` içinde `preferred_vector` var (internal navigation/debug); bu agent view'a
asla girmemeli — INV-T1 testi yukarıdaki `preferred_vector` assertion ile korur. İleride
`AgentPredicateSet` / `InternalPredicateSet` ayrımı düşünülebilir (Aşama C).

### INV-T2 — Operator tanımlar hedef (Genesis)
**Status:** planned (Aşama A)
**Tanım:** `Trajectory` ve `Milestone.target_region` trusted operator tarafından
tanımlanır (INV #13, #15 ile uyumlu). Agent hedef belirlemez; sadece oraya giden
structural change (DeltaProposal) üretir.
**Yapısal garanti:** `OperatorCapability` tipi — private constructor (`_private: ()`),
sadece trusted boundary'de (engine bootstrap / God Mode API) üretilebilir. PermissionMask
runtime value (agent üretebilir) YERİNE capability tipi compile-time korur:
```rust
pub struct OperatorCapability { _private: () }  // agent tarafında üretilemez
impl Trajectory {
    pub fn new(cap: &OperatorCapability, region: TargetRegion) -> Self { ... }
}
```
**Test:** `trajectory_requires_operator_capability` — agent kodu `OperatorCapability`
üretemez (private field) → compile error; `Trajectory::new()` capability olmadan çağrılamaz.
**İhlal örneği:** Agent PRD okuyup kendi Trajectory'sini yaratır → halüsinasyon kaskadı
(review 3 — Seçenek B reddedildi).
**Koruma mekanizması:** Seçenek A (insan mimar / God Mode) — capability sadece trusted API'den.

### INV-T3 — Engine ölçer (korunmuş, INV #4'ün dinamik uzantısı)
**Status:** planned (Aşama A)
**Tanım:** Task predicate'i **engine-measured** değer üzerinde değerlendirilir
(`claim.computed_raw`). Agent ölçmez; engine, DeltaProposal'ı apply_delta ile
uygulayıp re-analyze eder, P_raw'ı ölçer.
**Yapısal garanti:** `MetricPredicate::evaluate(pos: &ProvenancedRawPosition)` — input
engine'dan (ProvenancedRawPosition, INV-T4), agent değiştiremez. Predicate
`Claim.computed_raw`'ı okur, agent'ın PositionHint'ini değil.
**Test:** `predicate_uses_computed_raw_not_hint` — PositionHint ile predicate geçse bile
computed_raw ile fail etmeli.
**İhlal örneği:** Agent "coupling 0.4 oldu" der, predicate bunu kabul eder → INV #4 ihlali.
**Koruma mekanizması:** `check_claim_predicate(claim, task)` — claim.computed_raw zorunlu.

### INV-T4 — Predicate provenance (RawPosition provenance taşımalı)
**Status:** planned (Aşama A)
**Tanım:** MetricPredicate `required_source` ile "measured/scip" zorunlu kılabilir.
Placeholder/heuristic kaynaklı ölçümlerle task kapatılamaz (epistemolojik bütünlük).
**Kritik (review v3):** Çıplak `RawPosition` (f64) provenance taşıyamaz. INV-T4'ün
type-level enforce edilmesi için **her axis'in source'unu taşıyan** ölçüm tipi gerekir.
**Yapısal garanti:** `ProvenancedRawPosition` — her axis için `AxisMetric { value, source }`:
```rust
pub struct AxisMetric { pub value: f64, pub source: MetricSource }  // TreeSitter/Scip/Placeholder/Heuristic
pub struct ProvenancedRawPosition {
    pub coupling: AxisMetric, pub cohesion: AxisMetric, pub instability: AxisMetric,
    pub entropy: AxisMetric, pub witness_depth: AxisMetric,
}
// predicate evaluate: required_source ile karşılaştır
fn evaluate(&self, pos: &ProvenancedRawPosition) -> PredicateResult {
    let m = pos.axis(self.metric);  // AxisMetric (value + source)
    if self.required_source.map_or(false, |req| m.source != req) {
        return PredicateResult::SourceInsufficient;  // placeholder ile task kapatılamaz
    }
    self.op.compare(m.value, self.threshold) ? ...
}
```
**Test:** `placeholder_metric_cannot_close_task` — coupling.source=Placeholder ile predicate
satisfied olsa bile `SourceInsufficient` → task Done olmaz.
**İhlal örneği:** "Coupling ölçülmedi (placeholder 0.5) ama 0.55'in altında" → task kapanır →
ölçülmemiş başarı iddiası. ProvenancedRawPosition ile source type-level, runtime check değil.

### INV-T5 — Task ≠ Claim (Aşama B güncelleme: static Claim taskless olabilir)
**Status:** planned (Aşama A) + **implemented (Aşama B — Claim.task_id + TaskBoundClaim)**
**Tanım:** Task bir **şart seti** (PredicateSet), Claim bir **iş** (structural delta).
Bir task birden fazla claim/attempt gerektirebilir (TaskAttempt); bir claim bir task'a
hizmet eder (`Claim.task_id: Option<TaskId>`). **Güncelleme (review v2):** static Claim
(Paper 1, legacy, baseline) `task_id: None` ile çalışmaya devam eder — taskless olabilir.
Ama **Q5.b Predicate Gate sadece `TaskBoundClaim`** kabul eder (`bind_task_claim` ile).
Yani: trajectory-bound Claim requires task_id; static Claim may be taskless; Q5.b only
accepts TaskBoundClaim.
**Yapısal garanti:** `Claim.task_id: TaskId` (required); `Task.target_predicate_set: PredicateSet`;
`TaskAttempt.outcome: AttemptOutcome` (zengin struct — review v2 #5).
**Test:** `one_task_many_attempts` — 3 attempt senaryosu, hepsi aynı task_id, farklı outcome.
**İhlal örneği:** Task = Claim → bir reddedilen claim task'ı öldürür → katı sistem.

**PredicateSet tanımı (review 2 — spec'de eksikti):**
```rust
/// Multi-axis predicate set (roadmap §4.3). Tek MetricPredicate yerine Vec + mode.
/// F5 axis oscillation'ı doğal çözer (multi-axis loss).
/// review v4 #4 — Weighted duplication temizlendi: tek predicate listesi + weight Option.
pub struct PredicateSet {
    pub mode: PredicateMode,               // All (AND) | Any (OR) | Weighted (loss katkı)
    pub predicates: Vec<WeightedPredicate>, // tek liste (weight All/Any'de None)
    pub preferred_vector: Option<RawPosition>, // navigasyon merkezi (debug, distance)
}
pub struct WeightedPredicate {
    pub predicate: MetricPredicate,
    pub weight: Option<f64>,               // None = All/Any modda; Some(w) = Weighted modda
}
pub enum PredicateMode {
    All,                                    // tüm predicate'lar satisfied olmalı (default)
    Any,                                    // en az biri
    Weighted,                               // loss function: weight'lerle (F5)
}
```

### INV-T6 — Failure ≠ regression (review 1 + review v2 güçlendirme)
**Status:** planned (Aşama B/B2)
**Tanım:** Predicate failure negative progress *gerektirmez*. Bir DeltaProposal
predicate'i sağlamasa bile milestone'a yaklaşmış olabilir (loss azaldı). OSP üç şeyi
ayırt eder: (1) **completion** (predicate satisfied), (2) **mutation decision**
(policy'ye göre AcceptAsProgress/Reject/OperatorApproval), (3) **progress signal**
(loss ↓). Predicate failure asla task'ı tamamlamaz ama task bazlı policy izin veriyorsa
bounded progress checkpoint olabilir.
**Yapısal garanti:**
- `AttemptOutcome` struct — gate_decision/predicate_completion/mutation_decision/witness ayrı.
- `MutationDecision` enum — Reject/AcceptAsProgress/AcceptAsCompleted/RequireOperatorApproval.
- `TaskPolicy.predicate_failure_policy` — StrictReject/AcceptImprovement/OperatorApproval.
- Loss function quantitative: `loss_after < loss_before − min_delta AND max_axis_regression respected`.
- **CommitLane** (INV-T8 ile bağlantılı): AcceptAsProgress → sadece TrajectoryCheckpoint lane.
**Test:** `improved_accepted_as_progress_under_policy` — 0.82→0.71 (target 0.55), policy
AcceptImprovement → AcceptAsProgress (checkpoint, task açık). Same with StrictReject →
Reject (record improved).
**İhlal örneği:** "coupling ↓ ama instability +0.35" → max_axis_regression aşıldı →
Reject (axis oscillation tespit, F5). Veya progress checkpoint main branch'a merge
edilir → INV-T8 ihlali (progress ≠ merge).
**Prensip cümlesi:** "Predicate failure never completes a task, but under a task-specific
mutation policy it may be accepted as a bounded progress checkpoint if engine-measured
trajectory loss decreases and no hard invariant is violated."

### INV-T7 — Maneuver limit (review 3)
**Status:** planned (Aşama B2)
**Tanım:** Bir Task için ardışık N (default 5, operator-configurable) reddedilen attempt
sonra sistem **Trajectory Deviation Alert** üretir ve operatöre (God Mode) kontrol devreder.
Sonsuz context-loop ve token patlaması önlenir.
**Yapısal garanti:** `Task.attempts` sayacı + `TrajectoryDeviationAlert` event;
`ManeuverLimit { max_attempts }` operator config.
**Test:** `maneuver_limit_triggers_alert` — 5 reject sonra alert, 6. attempt blocked.
**İhlal örneği:** Agent 50 kez dener, token $50 harcar, hiç ilerlemez → kaynak israfı.
**Koruma mekanizması:** N aşıldığında task `Blocked` → operator replan veya N'i artır.

### INV-T8 — Progress checkpoint isolation (review v3 — INV-T6'dan ayrı)
**Status:** planned (Aşama B)
**Tanım:** `AcceptAsProgress` olan mutation task'ı **tamamlamaz** ve **mainline'a doğrudan
promote edilemez**. Sadece `TrajectoryCheckpoint` (veya `Sandbox`) commit lane içinde kalır.
Mainline'a sadece `AcceptAsCompleted` (predicate satisfied) promote olabilir. Bu OSP'nin
güven modelinin kalbidir — progress checkpoint yanlışlıkla ana branch'e karışırsa epistemolojik
bütünlük zedelenir.
**Yapısal garanti:** `CommitLane` + `ApplyTarget` modeli (review v4 #3 — Reject≠Sandbox):
```rust
pub enum CommitLane { Mainline, TrajectoryCheckpoint, Sandbox }

/// review v4 #3 — Reject "hiç uygulanmaz" demek, Sandbox "uygulanabilir ama izole" demek.
/// Karışıklığı önlemek için MutationDecision → ApplyTarget ayrımı.
pub enum ApplyTarget {
    NotApplied,                        // Reject — delta hiç uygulanmadı (simulated'da kaldı)
    Lane(CommitLane),                  // uygulandı, lane içinde
}
// MutationDecision → ApplyTarget mapping (type-level):
impl MutationDecision {
    fn apply_target(&self) -> ApplyTarget {
        match self {
            MutationDecision::Reject => ApplyTarget::NotApplied,           // hiç uygulanmaz
            MutationDecision::AcceptAsCompleted => ApplyTarget::Lane(CommitLane::Mainline),
            MutationDecision::AcceptAsProgress => ApplyTarget::Lane(CommitLane::TrajectoryCheckpoint),
            MutationDecision::RequireOperatorApproval => ApplyTarget::Lane(CommitLane::Sandbox),
        }
    }
}
// apply_delta: NotApplied → noop; Lane(Mainline) + AcceptAsProgress → compile/runtime reject
```
**Test:** `progress_checkpoint_cannot_promote_to_mainline` — AcceptAsProgress mutation
`CommitLane::Mainline`'a apply_delta çağrısı → Err. Sadece AcceptAsCompleted Mainline'a.
`reject_produces_not_applied` — Reject → `ApplyTarget::NotApplied` (değil Sandbox).
**İhlal örneği:** predicate fail, loss improved, AcceptAsProgress, yanlışlıkla main branch
merge → OSP güven modeli çöker (progress ≠ merge, ama merge oldu).

---

## Bölüm C — İnvariant Çapraz Kontrol Matrisi

Yeni invariant'ların mevcutlarla **çelişmediğinin** formal doğrulanması.

| Yeni | Etkileşim | Mevcut | Uyum? | Not |
|---|---|---|---|---|
| INV-T1 | genişletir | INV #4 | ✅ | INV #4 raw position; INV-T1 task predicate'inin agent'a koordinat sızdırmaması. |
| INV-T1 | genişletir | INV #14 | ✅ | INV #14 prompt typed; INV-T1 AgentTaskView typed + koordinatsız. |
| INV-T2 | genişletir | INV #13 | ✅ | INV #13 permission operator; INV-T2 trajectory operator. |
| INV-T2 | genişletir | INV #15 | ✅ | INV #15 axis operator; INV-T2 milestone region operator. |
| INV-T3 | genişletir | INV #4 | ✅ | INV #4 engine compute raw; INV-T3 predicate computed_raw'ı okur. |
| INV-T3 | bağımsız | INV #12 | ✅ | INV #12 Q4 syntax; INV-T3 Q5.b predicate — farklı gate'ler. |
| INV-T4 | genişletir | INV #3 | ✅ | INV #3 tri-state; INV-T4 placeholder metric task kapatmaz. |
| INV-T5 | yeni | — | ✅ | Task≠Claim ontolojik ayrım. |
| INV-T6 | genişletir | INV #9 | ✅ | INV #9 evaluate pure; INV-T6 AttemptOutcome/MutationDecision pure çıktı (loss-driven). |
| INV-T7 | yeni | INV #7 | ✅ | INV #7 admin override; INV-T7 maneuver limit → admin devralma. |
| INV-T8 | genişletir | INV-T6 | ✅ | INV-T6 loss-driven progress; INV-T8 progress checkpoint lane izolasyonu (progress≠merge). |

**Çatışma yok.** Her yeni invariant mevcutları genişletir veya bağımsız ekler. INV-T8
INV-T6'nın "progress≠merge" yönünü ayrı invariant yapar — ikisi birlikte progress checkpoint
güvenliğini tamamlar.

---

## Bölüm D — İnvariant Erozyonuna Karşı Savunma

Review 1'in ana uyarısı: *"feature eklendikçe epistemolojik sınırlar bulanıklaşır."*
Savunma mekanizmaları:

### D1 — Type-level enforcement (compile-time)
İnvariant'lar runtime check değil, **type-level**. Örnek: `AgentTaskView`'da **target
coordinate alanı yoksa** (`current_measurement` serbest — mevcut engine-measured durum),
serde derive ile agent'a **hedef koordinat** serialize etmek compile error.
Runtime'da "unutulamaz." **Önemli (review v4 #2):** yasak olan hedef koordinat (target);
mevcut durum (`current_measurement`) görülebilir — agent nerede olduğunu bilmeli.

### D2 — Test matrisi
Her invariant için bir test (`*_invariant` isim şablonu). CI'da çalışır. İnvariant
erozyona uğrarsa test fail. Hedef: `crates/osp-core/tests/invariants.rs` (Aşama A).

### D3 — Review checklist (her PR)
Her PR bu spec'in bir invariant'ına dokunuyorsa, PR açıklamasında:
- Hangi invariant?
- Nasıl korunduğu (type/test)?
- Çapraz kontrol matrisi etkileniyor mu?

### D4 — İnvariant spec versiyonlama
Bu doküman versioned. İnvariant eklenirse/değişirse, bu spec güncellenir + paper
karşılığı. "Sessiz erozyon" önlenir.

---

## Sonraki Adım

Bu spec + `agent-trajectory-roadmap.md` oturunca:
1. Aşama A (ontolojik tipler) — bu spec'teki invariant'ları type-level enforce eden tipler.
2. `crates/osp-core/tests/invariants.rs` — D2 test matrisi.
3. Her aşamada spec'e uyum kontrolü.

---

*Bu doküman review 1'in "formal invariant spec" önerisi üzerine kuruldu.
Kaynak: INV #1..#15 (paper-draft-v2.6.md), INV-T1..T7 (agent-trajectory-roadmap.md + 3 review).*
