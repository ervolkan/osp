# Concept Anchoring Design Decisions — Genesis Layer for Project Reality Space

> **Sürüm:** v0.2.1-draft (v0.2 → v0.2.1: tutarlılık patch'i — DerivesRisk edge'i + source_reliability rename + D14→D13 numaralandırma + §9 başlık)
> **Tarih:** 2026-07-02
> **Durum:** Tasarım kararı dokümanı — kod yazımından ÖNCE (OSP inşa disiplini)
> **Amaç:** İnsan kelimelerini (ve repo'dan çıkarılan kavramları) proje uzayındaki ontolojik varlıklara bağlayacak Genesis Layer'ın tasarım kararlarını kesinleştirmek.
> **İlişki:** Paper 1 (statik uzay, kanıtlandı) + Paper 2 (dinamik yörünge, v1.2 arXiv adayı) + **Paper 3 (Genesis Layer — bu dokümanın konusu)**

## İlişkili Dokümanlar

- `docs/paper-draft-v2.6.md` — Paper 1 (statik uzay, MetricValue provenance, inv #4 engine ölçer)
- `docs/paper2-draft-v1.md` — Paper 2 (INV-T1 predicate epistemolojisi, INV-T2 operator genesis, INV-T8 lane disiplini)
- `docs/invariant-spec.md` — INV #1-15 (Paper 1) + INV-T1-T8 (Paper 2); INV-C1-C8 bu dokümanda proposal, implementation'da Bölüm C'ye taşınır
- `docs/osp-core-design.md`, `docs/space-engine-design.md` — mevcut core tasarım pattern'leri

## Kaynaklar

Bu doküman üç kaynağı birleştirir:
1. **Özgün gereksinim** — `concept-anchoring-algorithm-requirements.md` (v0.1-draft): ConceptPacket, AnchorCandidate, hibrit skor, KuzuDB modeli
2. **Concept Synthesis + Genesis Layer genişletmesi** — Project Genesis Layer kavramı, PositionFamily, lane modeli, SupersedeAuthority, INV-C1-C8 önerileri
3. **Sınır hassasiyeti düzeltmeleri (5 itiraz)** — SemanticEmbedding position family değil; code structure (Observed) vs code intent (Inferred); kod > doküman; explanation high-stake ile sınırlı; Concept Synthesis sıralaması

---

## 1. Vizyon ve Üçleme Konumlandırma

OSP üç katmanlı bir programdır:

```
Paper 1 — Project Space Physics
  Kodun fiziksel gerçekliğini ölçer (coupling, cohesion, instability, provenance).
  "Ölçülen şey gerçektir."

Paper 2 — Architectural Trajectory Navigation
  Agent'ın bu gerçeklikte nasıl güvenle ilerleyeceğini yönetir.
  "Agent hedef koordinatı görmez; predicate görür, engine ölçer."

Paper 3 — Project Genesis Layer  (BU DOKÜMAN)
  İnsan niyetinin ve repo'dan çıkarılan kavramların bu gerçekliğe nasıl bağlanacağını tanımlar.
  "İnsan cümlesinin ontolojik rolü embedding yakınlığıyla değil, bağlama zinciriyle belirlenir."
```

Paper 2 INV-T2 "operator hedefi tanımlar" der, ama *nasıl* tanımladığını kodlamadı — bugün elle `Trajectory::new(cap, region)` çağrısıyla. Paper 3 bu boşluğu doldurur: insan cümlesinden kanıta uzanan epistemolojik zinciri kurar.

### Ana tez

> **Bir insan cümlesinin proje gerçekliğindeki ontolojik rolü, embedding yakınlığıyla değil, ontolojik bağlama zincirinin tamamlığıyla belirlenir.**

### Bu bir RAG değil

GraphRAG ve RepoCoder metni metne bağlar: *"bu metne benzeyen metinleri bul."* OSP Concept Anchoring farklı bir soru sorar:

```
Bu insan cümlesinin proje gerçekliğindeki ontolojik rolü nedir?
Bu cümle hangi kavrama, kurala, göreve, koda ve kanıta bağlanır?
Bu bağlantı ne kadar güvenilir?
```

Bu yüzden merkezi ilke:

```
Embedding aday üretir.
Ontology karar verir.
Engine ölçer.
Operator kabul eder.
```

Bu, Paper 2'nin "LLM önerir, engine bağlar" ilkesinin genesis katmanındaki yankısıdır. Üç OSP katmanı da aynı hamleyi farklı yerlerde yapar: **anlamı/ölçümü/hedefi sayıya veya vektöre indirgeme, epistemik yapıyı koru.**

---

## 2. Problem: Task Genesis Boşluğu

### Mevcut durum

Paper 2'de task'lar operator tarafından tanımlanır:

```rust
let trajectory = Trajectory::new(&operator_cap, target_region);
```

Bu, INV-T2 ("operator tanımlar hedef") açısından doğru — ama *operator bu hedefi nasıl oluşturuyor?* Bugün bu elle yapılıyor. Bir mimar "coupling'i düşür" dediğinde, bu cümleden bir `PredicateSet`'e giden yol yok.

### İstenen zincir

Concept Anchoring bu boşluğu doldurur:

```
İnsan cümlesi
  → ConceptPacket
  → Concept
  → RuleCandidate
  → PredicateSet
  → TaskCandidate
  → Operator approval
  → Accepted Task / Trajectory
```

Agent doğrudan hedef koordinatı görmez (Paper 2 INV-T1). Agent'a predicate ve mevcut ölçüm verilir. Ama predicate'in *kendisi* artık bir insan cümlesinden türetilmiş olmalı — operator'ün elle yazdığı bir hardcoded değer değil.

### Project Reality Space

Bu katman tamamlandığında OSP, sadece "kod mimarisini ölçen" sistem olmaktan çıkar; insan vizyonunun koda dönüşürken bozulup bozulmadığını izleyen bir **Project Reality Space** haline gelir. Sistem şu soruları yanıtlayabilir:

```
Hangi insan vizyonlarının kod karşılığı eksik?
Hangi mimari kararlar kodda uygulanmamış?
Hangi kod modülleri, vizyonunda karşılığı olmayan şekilde büyümüş?
Hangi agent varsayımları kullanıcı tarafından onaylanmamış?
```

Bu, Paper 3 için ölçülebilir bir katkıdır (bkz. §11 fazlama, Faz 4-5).

---

## 3. Genesis Layer Mimarisi

Genesis Layer üç alt bileşenden oluşur:

```
Genesis Layer =
  Concept Anchoring      (human words → project space)
  + Concept Synthesis    (code repo → concept/vision/rule hipotezleri)
  + Operator Acceptance  (candidate → accepted project knowledge)
```

### 3.1 Concept Anchoring (bu dokümanın odağı)

İnsan kelimesini proje uzayına bağlar. Girdi: doğal dil cümlesi (user vision, requirement, ADR, issue). Çıktı: ConceptPacket + anchor edge'leri + PositionSnapshot.

```
"Kullanıcı ödeme yaparken kendini güvende hissetmeli."
  → ConceptPacket(UserVision)
    → MENTIONS → Concept:Payment
    → MENTIONS → Concept:Trust
    → DERIVES_RISK → Risk:PaymentTrustLoss
    → EXPECTED_IMPLEMENTATION → CodeEntity:PaymentModule (eğer varsa)
```

### 3.2 Concept Synthesis (arkadaşın genişletmesi — Faz 6)

Concept Anchoring'in ters yönü: code repository'den kavram, vizyon ve kural *hipotezleri* üretir. Amaç: kodun gerçek yapısal durumundan, *"bu kod ne yapmaya çalışıyor?"* sorusuna Inferred-status'lu cevaplar üretmek.

```
Code repository analizi
  → observed coupling/cohesion pattern'leri
  → Concept Synthesis
  → "PaymentModule yüksek coupling — muhtemelen ödeme akışını yönetiyor" (Inferred)
  → Concept hipotezleri (Observed structürel metric + Inferred niyet)
```

### 3.3 Bağımlılık sırası (D12)

**Concept Anchoring önce, Concept Synthesis sonra.** Mantıksal bağımlılık açıktır:

```
Concept Anchoring önce:
  Concept node'larını oluşturur (Payment, Trust, vb.)
  Anchor noktalarını belirler

Concept Synthesis sonra:
  Kod → aynı Concept node'larına hipotez bağlar
  "Bu modül muhtemelen Concept:Payment" (Inferred)
```

Ters sıra anlamsızdır: kodan çıkarılan "ödeme kavramı"nı bağlayacak önceden bir `Concept:Payment` node'u yoksa, nereye koyacağız? Concept Synthesis ancak Anchoring'in oluşturduğu ontolojik iskelete bağlanabilir.

Bu sıralama implementation fazlamasına da yansır (§11): Synthesis Faz 6'da, Anchoring Faz 1-5'te.

### 3.4 Tek graph, çoklu coordinate family

Tüm varlıklar aynı `Project Reality Graph` içinde yaşar — insan vizyonu, kod gerçekliği, kararlar, görevler, kanıtlar. Ama her varlık ailesi *kendi coordinate family*'sinde ölçülür. Köprü typed edges ile kurulur (§7, §8).

```
Tek graph (KuzuDB veya in-memory)
  ├── ConceptPacket node'ları     → ConceptualIntent position family
  ├── Concept node'ları            → ConceptualIntent position family
  ├── CodeEntity node'ları         → PhysicalCode position family
  ├── Evidence node'ları           → Evidence position family
  └── Typed edges (MENTIONS, DERIVES_RULE, EXPECTED_IMPLEMENTATION, ...)
```

Bu model §4'te detaylandırılır.

---

## 4. PositionFamily Modeli (D1, D8)

### 4.1 Karar B: Ayrı coordinate family'ler

ConceptPacket'lerin ontolojik konumu ile CodeEntity'lerin fiziksel konumu **aynı uzayda değildir** ve zorla aynı `ℝ⁵` içine sokulmamalıdır. Sebep: eksenler aynı şeyi ölçmüyor.

```
PhysicalCode space (Paper 1):
  coupling, cohesion, instability, entropy, witness_depth

ConceptualIntent space (Paper 3):
  abstraction, vision_alignment, implementation, confidence, risk, code_alignment

Evidence space (Paper 1 + 3):
  confidence, coverage, recency, stability, source_reliability
```

Üç uzayın da eksen seti **tanımlı olmalıdır** (INV-C2 — position family separation). Evidence eksenleri Paper 1'in `MetricValue` modeliyle uyumludur: `confidence` ve `coverage` doğrudan `MetricValue` alanları, `source_reliability` ise `MetricSource`'tan türetilen kaynak güven katsayısıdır (TreeSitter/SCIP güven tabanı), `recency` Paper 1'in `stale_penalty` sinyali, `stability` tekrarlanan ölçümlerin varyansıdır. Kanıtın kalitesi — sadece var/yok değil — uzayda ölçülür.

Coupling (x) ile implementation aynı şey değildir: coupling iki modül arasındaki yapısal bağımlılığı ölçer; implementation bir kavramın kodda gerçekleşme derecesini ölçer. Bunları aynı vektörün bileşenleri gibi ele alırsak model bulanıklaşır.

### 4.2 PositionFamily enum (3 aile)

```rust
pub enum PositionFamily {
    PhysicalCode,       // Paper 1: coupling/cohesion/instability/entropy/witness_depth
    ConceptualIntent,   // Paper 3: abstraction/vision_alignment/implementation/confidence/risk/code_alignment
    Evidence,           // Paper 1+3: confidence/coverage/recency/stability/source_reliability
}
```

Bir node birden fazla position'a sahip olabilir:

```text
ConceptPacket:
  conceptual_position   → ConceptualIntent family (abstraction, vision_alignment, ...)
  semantic_vector_ref   → embedding referansı (family değil — bkz. §4.3)

CodeEntity:
  physical_position     → PhysicalCode family (coupling, cohesion, ...)
  conceptual_alignment  → hangi ConceptPacket'e EXPECTED_IMPLEMENTATION/IMPLEMENTED_BY ile bağlı?
```

PositionSnapshot struct'ı family-aware olur:

```rust
pub struct PositionSnapshot {
    pub id: PositionSnapshotId,
    pub node_id: NodeId,
    pub family: PositionFamily,
    pub vector: PositionVector,      // family'ye göre eksen seti
    pub source: PositionSource,      // MetricSource gibi provenance
    pub confidence: f64,
    pub measured_at: DateTime<Utc>,
}
```

Bu, Paper 1'in MetricValue provenance disiplininin ("ölçümün kaynağını taşı") uzay boyutuna genişletilmesidir: *"position'ın uzay ailesini taşı."* Paper 1 inv #4 "engine ölçer" der; Paper 3 bunu "engine, doğru uzayda ölçer" ile sıkıştırır.

### 4.3 SemanticEmbedding position family DEĞİL (D8 — itiraz)

Özgün yorum, `PositionFamily` enum'ında `SemanticEmbedding`'i dördüncü aile olarak önerdi. **Bu reddedildi.** Sebep:

- INV-C1 (§9) "embedding aday üretir, karar vermez" der.
- Eğer embedding bir position family ise, bir node'un embedding vektörü onun *ontolojik konumu* olur — INV-C1 ile doğrudan çelişki.
- Özgün gereksinim §5.4 zaten "semantic vector ile osp position aynı şey değildir" diyor.

Embedding bir *index/retrieval* yapısıdır, ontolojik konum değil. Doğru tasarım: `semantic_vector_ref: Option<VectorRef>` olarak ConceptPacket içinde bir referans — ayrı coordinate family değil.

Bu, Paper 2 INV-T1 ("agent hedef koordinatı görmez") ile tutarlıdır: anlamı vektöre indirgemeyen aynı prensip, embedding'i de "anlam" değil "arama aracı" olarak konumlandırır.

---

## 5. Candidate vs Accepted Lane Modeli (D2, INV-C3)

### 5.1 INV-T2 gerilimi ve çözümü

Paper 2 INV-T2 "operator hedefi tanımlar" der. Ama Concept Anchoring `ConceptPacket → RuleCandidate → PredicateSet → Task` zincirini *otomatik* türetiyor (özgün §3.4). Bu INV-T2'yi ihlal eder mi?

**Cevap:** İhlal etmez, *eğer* türetilen şey Candidate ise. Ayrım:

```
Otomatik üretilebilir (Candidate lane):
  RuleCandidate
  TaskCandidate
  VisionCandidate
  Concept hipotezleri

Operator onayı olmadan ÜRETİLEMEZ (Accepted):
  Trajectory
  Accepted Rule
  Accepted Task
  Accepted Decision
```

### 5.2 Lane modeli

Bu, Paper 2 INV-T8 ("AcceptAsProgress ≠ Mainline") lane disiplininin genesis katmanına genişletilmesidir:

```
Candidate lane       → serbest üretim (AnchorResolver üretir)
Review lane          → operator/mimar değerlendirmesi
Accepted mainline    → proje gerçekliği (operator kabul etti)
Deprecated lane      → eski bilgi (supersede edildi)
Rejected lane        → negatif epistemik veri (çelişki/ret)
```

Candidate node'lar graph'ta *yaşar* (görünür, sorgulanabilir), ama **mainline knowledge** değildir. Bir Task sorgusu yaparken `decision_status: Accepted` filtresi konur; Candidate'ler dahil edilmez.

### 5.3 INV-C3: Candidate isolation

> **INV-C3 (proposal) — Candidate isolation:** AnchorResolver tarafından üretilen Concept/Rule/Task adayları, operator acceptance olmadan Project Reality mainline'a promote edilemez.

Bu, Paper 2 INV-T8'inin genesis karşılığıdır. INV-T8 "AcceptAsProgress mainline'e çıkamaz" der; INV-C3 "Candidate mainline knowledge olamaz" der. Aynı epistemik prensip: **türetme serbest, onay kapalı.**

### 5.4 DecisionStatus enum

```rust
pub enum DecisionStatus {
    Candidate,       // AnchorResolver üretti, operator görmedi
    InReview,        // operator değerlendiriyor
    Accepted,        // operator kabul etti → mainline
    Deprecated,      // supersede edildi
    Rejected,        // çelişki veya ret
}
```

Her anchor edge ve türetilmiş node bu status'u taşır. Sorgular `Accepted` filtresi ile mainline knowledge'a, `Candidate` filtresi ile "işlem bekleyen" listesine erişir.

---

## 6. Epistemik Hiyerarşi ve Çelişki Çözümü (D3, D10)

### 6.1 "En yeni kazanır" reddedildi

Çelişki çözümünde iki yaklaşım vardır:
- **(A) En yeni bilgi kazanır** — temporal recency.
- **(B) En güvenilir kaynak kazanır** — epistemic authority.

OSP **(B)**'yi seçer. Sebep: OSP'de bilgi güvenilirliği zamandan daha önemlidir. "En yeni" yaklaşımı, yeni bir agent hypothesis'nin eski bir accepted user vision'ı geçersiz kılmasına izin verir — bu INV-T1'in ("agent zayıf epistemik yetkiye sahip") ruhuna aykırıdır.

### 6.2 Düzeltilmiş hiyerarşi (kod > doküman — D10)

Özgün yorumun hiyerarşisi:
```
4. Accepted repository documentation
5. Observed code reality
```

**Bu düzeltildi (itiraz):** 5 (kod) 4'ün (doküman) üzerinde olmalı. Yazılım mühendisliğinin en sağlam prensibi: **kod gerçektir, doküman niyettir.** Kod execute olan, merged, production'da. Doküman stale olabilir, kodla çelişebilir, güncellenmemiş olabilir.

Paper 1'in ontolojisi de böyledir: `MetricSource::TreeSitter/Scip` (koddan ölçüm) en yüksek güven; doküman metni bu hiyerarşide yer almaz bile. Eğer doküman > kod yaparsak, "kod dokümandan saptı" durumunda "kod yanlış" demiş oluruz — tehlikeli ve yanlış.

Düzeltilmiş sıra:

```
1. Operator accepted decision         (en yüksek — INV-T2 genesis)
2. Explicit user vision               (kullanıcıdan gelen, kaynak etiketli)
3. Witnessed architect decision       (Paper 1 witness modeli)
4. Observed code reality              ← yükseldi (metric/fiziksel yapı, Paper 1)
5. Accepted repository documentation  ← düştü (README, ADR — stale olabilir)
6. Inferred architecture hypothesis   (koddan çıkarılan niyet, INV-C6)
7. Agent hypothesis                   (en düşük — INV-T1 zaten görmüyor)
8. Raw embedding similarity           (INV-C1: aday üretir, karar vermez)
```

### 6.3 Kod > doküman sınırlaması (D9 ile bağlantılı)

"Kod > doküman" yalnızca kodun **ölçülen yapısı** için geçerlidir (coupling 0.82, cohesion 0.57). Kodun *niyeti* ("bu modül ödeme vizyonunu yansıtıyor olmalı") hâlâ Inferred'dir (§7). Yani:

- **Coupling 0.82** → Observed, kod > doküman hiyerarşisinde 4. sırada.
- **"PaymentModule ödeme akışını yönetiyor"** → Inferred, 6. sırada.

İkisi farklı epistemik seviyelerde. Bu ayrım §7'de detaylandırılır.

### 6.4 SupersedeAuthority capability gate (D3)

`SUPERSEDES` edge'i capability-gated'tir. Sadece yeterli epistemik yetkiye sahip kaynak accepted kararları geçersiz kılabilir:

```rust
pub enum SupersedeAuthority {
    Operator,                   // her şeyi supersede edebilir
    ExplicitUser,               // architect decision ve altını
    WitnessedArchitectDecision, // documentation ve altını
}
```

**Agent yapamayacağı şey:**
```
AgentHypothesis --SUPERSEDES--> AcceptedDecision   ❌ (capability yetersiz)
```

**Agent yapabileceği şey:**
```
AgentHypothesis --CONTRADICTS?--> AcceptedDecision  ✅ (çelişki önerir, geçersiz kılmaz)
```

Yani agent "çelişki önerir", "geçersiz kılamaz." Çelişki operator'ün dikkatine sunulur (Review lane), ama mainline knowledge değişmez ta ki operator veya daha yüksek yetkili bir kaynak supersede edene kadar.

Bu, Paper 2 INV-T1'inin ("agent hedef koordinatı görmez") genesis katmanındaki karşılığıdır: agent'ın epistemik yetkisi sınırlıdır, accepted knowledge'ı değiştiremez.

### 6.4.1 Hiyerarşi ↔ SupersedeAuthority eşlemesi

8 seviyeli epistemik hiyerarşi (§6.2) **çelişki çözümünde kimin kazanacağını** belirler — bu bir sıralamadır, tüm seviyeleri kapsar. 3 seviyeli `SupersedeAuthority` ise **kimin mevcut accepted kararları aktif olarak geçersiz kılabileceğini** belirler — bu bir yetkilendirmedir, sadece "karar verebilen" katmanları kapsar.

| Hiyerarşi seviyesi | `SupersedeAuthority` | Açıklama |
|---|---|---|
| 1. Operator accepted decision | `Operator` | Her şeyi supersede edebilir |
| 2. Explicit user vision | `ExplicitUser` | Architect decision ve altını supersede edebilir |
| 3. Witnessed architect decision | `WitnessedArchitectDecision` | Documentation ve altını supersede edebilir |
| 4. Observed code reality | — | Karar değil, kanıt. `SUPERSEDES`'e konu olmaz; `DRIFTS_FROM`/`CONTRADICTS?` üretir. |
| 5. Accepted repository documentation | — | Karar değil, referans. `SUPERSEDES`'e konu olmaz. |
| 6. Inferred architecture hypothesis | — | Karar değil, hipotez. Review önerisi üretir. |
| 7. Agent hypothesis | — | Karar değil, hipotez. Sadece `CONTRADICTS?` önerebilir. |
| 8. Raw embedding similarity | — | Karar değil, sinyal. Sadece aday üretir (INV-C1). |

Alt seviyeler (4-8) `SUPERSEDES` yetkisine sahip değildir; sadece `CONTRADICTS?` önerirler. Önemli ayrım: **observed code reality güçlüdür ama karar değildir** — kod "şu accepted decision ile çelişiyor" sinyali üretir ama "bu kararı geçersiz kıldım" diyemez. Kod gerçeği gösterir, operator kararı değiştirir.

### 6.5 INV-C4: Supersede requires authority

> **INV-C4 (proposal) — Supersede requires authority:** Accepted kararları sadece yeterli epistemik yetkiye sahip kaynak (`SupersedeAuthority`) geçersiz kılabilir. Düşük yetkili kaynaklar (agent, raw embedding) sadece `CONTRADICTS?` önerir.

---

## 7. Code Structure vs Code Intent (D9, INV-C6)

### 7.1 INV-C6'nın orijinal halinin problemi

Özgün INV-C6 önerisi: "Repo analizinden üretilen vizyon/kural cümleleri Observed/Inferred statüsündedir, gerçek kullanıcı vizyonu sayılmaz."

Bu *niyet* çıkarımında doğru ama *yapı* gözleminde yanlış. İkisini ayırmak şarttır:

- **Kod metric'leri** (coupling 0.82, cohesion 0.57) → **Observed/Measured**. Paper 1'in tüm iddiası budur: ölçülen şey gerçektir. Eğer bunu "hipotez" yaparsak Paper 1'in ontolojik temelini çökertiriz.
- **Koddan niyet çıkarımı** ("bu modülün amacı X olmalı", "bu yapı bir vizyonu yansıtıyor olmalı") → **Inferred**. Bu hipotez.

Eğer ikisini aynı Inferred sepetine koyarsak "coupling 0.82" ile "LLM bence bu modül ödeme vizyonu" aynı epistemik seviyeye iner — bu OSP'nin ontolojik duruşunu zedeler. Kod gerçektir; koddan çıkarılan *anlam* yorumdur.

### 7.2 Düzeltilmiş INV-C6

> **INV-C6 (proposal, düzeltilmiş) — Code-derived intent is hypothesis:** Koddan çıkarılan **niyet/vizyon yorumları** Inferred statüsündedir; kodun **ölçülen fiziksel yapısı** (metric'ler) Observed'dır (Paper 1). İkisi karıştırılamaz.

Bu düzeltme Paper 1 ile Paper 3'ü ontolojik olarak uyumlu tutar. Paper 1 "kod metric'leri gerçektir" der; Paper 3 "koddan çıkarılan anlam gerçeğin yorumudur" der. Çelişki yok, tamamlayıcı.

### 7.3 Concept Synthesis'teki uygulanması (D9)

Concept Synthesis (Faz 6) kod analizi yaparken iki tür çıktı üretir:

```
Concept Synthesis output:
  Structural facts (Observed):
    "PaymentModule coupling = 0.82"
    "PaymentModule 7 outgoing imports"
    → PhysicalCode position family (Paper 1 metric'leri)

  Intent hypotheses (Inferred):
    "PaymentModule muhtemelen ödeme akışını yönetiyor"
    "Bu yapı yüksek olasılıkla PaymentTrust vizyonunu yansıtıyor"
    → ConceptualIntent position family, DecisionStatus: Candidate/Inferred
```

İlki Paper 1'in `MetricSource::TreeSitter/Scip` güveniyle taşınır. İkincisi INV-C5 ("Inferred is not accepted") altında Candidate olarak işaretlenir.

### 7.4 INV-C5: Inferred is not accepted

> **INV-C5 (proposal) — Inferred is not accepted:** Koddan veya LLM'den çıkarılan her bilgi Inferred başlar; insan/operator onayıyla Accepted olur.

Bu, §5 candidate/accepted lane modelinin epistemik karşılığıdır. Türetme serbest (Candidate), onay kapalı (Accepted).

---

## 8. Anchor Skoru ve Edge'ler

### 8.1 Hibrit skor formülü (D5)

Özgün gereksinim §8.1'in 7 bileşenli skoru korunur — INV-C1 ("embedding aday üretir, karar vermez") gereği, semantic_similarity sadece 7 bileşenden biridir:

```text
AnchorScore =
    0.25 * semantic_similarity      (embedding aday üretir)
  + 0.20 * ontology_type_compatibility
  + 0.15 * graph_context_score
  + 0.15 * domain_term_match
  + 0.10 * code_evidence_score
  + 0.10 * temporal_trust_score
  + 0.05 * decision_status_score
  - contradiction_penalty
  - staleness_penalty
```

| Bileşen | Anlamı |
|---|---|
| `semantic_similarity` | Embedding ile anlamsal yakınlık (0.25 — en yüksek ama tek başına karar vermez) |
| `ontology_type_compatibility` | Packet türü ile hedef node türü uyumu |
| `graph_context_score` | Hedef node'un komşuları da metinle ilişkili mi? |
| `domain_term_match` | Domain sözlüğü / alias / terim eşleşmesi |
| `code_evidence_score` | Kodda gerçek karşılık var mı? (Paper 1 symbol index) |
| `temporal_trust_score` | Kaynak güncel ve güvenilir mi? (§6 hiyerarşi) |
| `decision_status_score` | Accepted / Candidate / Deprecated durumu |
| `contradiction_penalty` | Mevcut accepted decision ile çelişki cezası (INV-C4) |
| `staleness_penalty` | Eski bilgi cezası |

**Faz 0-2 notu:** Bu ağırlıklar (0.25/0.20/0.15/...) başlangıç önerisidir, calibration ile ayarlanır. İlk MVP'de deterministic rule-based classifier kullanıldığı için `semantic_similarity` bileşeni placeholder (0.0 veya lexical skor) — gerçek embedding Faz 7'de gelir.

### 8.2 Threshold policy

```text
score >= 0.80  → StrongLink (high confidence anchor)
0.60 <= score < 0.80  → TentativeLink / RequireOperatorReview
0.40 <= score < 0.60  → CreateNode veya WeaklyAnchored (CreateNode öncesi dedup/canonicalize gate'i zorunludur — Q13, INV-C8)
score < 0.40  → MarkUnanchored
```

Özel kural: `semantic_similarity` yüksek ama `ontology_type` farklıysa node merge edilmez; sadece `RELATED_TO` veya `SAME_TOPIC` ilişkisi kurulur. Bu INV-C2 (position family separation) gereğidir: aynı anlama gelen ama farklı ontolojik role sahip şeyler karıştırılamaz.

### 8.3 ConceptEdgeKind (15 tür: 14 ontolojik + 1 meta)

```rust
pub enum ConceptEdgeKind {
    // --- 14 ontolojik edge (anlam ilişkisi taşır) ---
    Mentions,                // ConceptPacket → Concept (düşük stake)
    Refines,                 // Concept → Concept (düşük stake)
    DerivesRule,             // ConceptPacket → RuleCandidate (high stake)
    DerivesTask,             // ConceptPacket → TaskCandidate (high stake)
    DerivesRisk,             // ConceptPacket → RiskCandidate (high stake, v0.2.1)
    Constrains,              // RuleCandidate → CodeEntity (high stake)
    ExpectedImplementation,  // ConceptPacket → CodeEntity (high stake)
    ImplementedBy,           // TaskCandidate → CodeEntity (high stake)
    EvidencedBy,             // CodeEntity → Evidence (high stake)
    Contradicts,             // ConceptPacket → Decision (high stake)
    Supersedes,              // ConceptPacket → Decision (high stake, capability-gated §6.4)
    RelatedTo,               // Concept ↔ Concept (düşük stake)
    AntiGoalOf,              // Concept → Concept (high stake)
    DependsOnDecision,       // ConceptPacket → Decision (düşük stake)

    // --- 1 meta edge (anlam ilişkisi değil, graph navigation) ---
    HasPosition,             // ConceptPacket → PositionSnapshot (meta)
}
```

High-stake edge sayısı: **10** (DerivesRule, DerivesTask, DerivesRisk, Constrains, ExpectedImplementation, ImplementedBy, EvidencedBy, Contradicts, Supersedes, AntiGoalOf). Düşük-stake: **4** (Mentions, Refines, RelatedTo, DependsOnDecision). `DerivesRisk` v0.2.1'de eklendi — §3.1 örneğinde kullanılan `DERIVES_RISK` edge'inin enum karşılığıdır; insan vizyonundan risk türetmek, rule/task türetmek kadar high-stake bir epistemik eylemdir (INV-C7 explanation zorunlu).

Kullanım örnekleri:

```text
ConceptPacket --MENTIONS--> Concept
ConceptPacket --DERIVES_RULE--> RuleCandidate
ConceptPacket --DERIVES_TASK--> Task
ConceptPacket --DERIVES_RISK--> RiskCandidate  (high stake, v0.2.1)
Rule --CONSTRAINS--> CodeEntity
Task --EXPECTED_IMPLEMENTATION--> CodeEntity
CodeEntity --EVIDENCED_BY--> TestResult
ConceptPacket --CONTRADICTS--> Decision
ConceptPacket --SUPERSEDES--> OldDecision  (capability-gated)
ConceptPacket --HAS_POSITION--> PositionSnapshot
```

### 8.4 Explanation zorunluluğu — high-stake ile sınırlı (D11)

Özgün INV-C7 önerisi "her anchor edge skor kırılımı ve gerekçesi olmadan graph'a yazılamaz" der. İlke doğru ama *her* edge için explanation zorunluğu graph'ı şişirir ve düşük-stake bağlantıları pahalılaştırır.

**Düzeltme (itiraz):** explanation *high-stake edge'ler* için zorunlu; düşük-stake opsiyonel:

| Stake | Edge türleri | Explanation |
|---|---|---|
| **High** | DerivesRule, DerivesTask, DerivesRisk, Contradicts, Supersedes, ExpectedImplementation, ImplementedBy, EvidencedBy, AntiGoalOf, Constrains | **zorunlu** (skor kırılımı + reason) |
| **Düşük** | Mentions, RelatedTo, Refines, DependsOnDecision | opsiyonel |

Bu, INV-C7'yi pratik hale getirir: her `Mentions` için elle gerekçe yazmak iş kârlığını düşürür ama `DerivesRule` ("bu cümleden şu kural türetiliyor") için gerekçe şarttır.

### 8.5 AnchorDecisionKind

```rust
pub enum AnchorDecisionKind {
    StrongLink,              // score >= 0.80
    TentativeLink,           // 0.60 <= score < 0.80
    CreateNode,              // aday yok, yeni Concept node oluştur
    CreateIntermediateNode,  // yüksek soyutluklu cümle → ara node (§5.5 özgün)
    MarkContradiction,       // çelişki tespit edildi
    MarkUnanchored,          // score < 0.40, bağlanamadı
    RequireOperatorReview,   // high-stake, operator onayı gerek
}
```

### 8.6 Doğrudan kod bağlama riski (D6)

Özgün gereksinim §5.5'in prensibi korunur: yüksek soyutluklu insan cümlesi doğrudan code module'a bağlanmamalıdır. Arada concept/rule/task/predicate node'ları olmalıdır.

```text
Yanlış:  "Ödeme güven vermeli" → PaymentService

Doğru:   ConceptPacket
           → Concept:Payment
           → Concept:Trust
           → RuleCandidate:PaymentTrustFeedback
           → Task:ImprovePaymentTrust
           → CodeEntity:PaymentService
```

Bu, INV-C3'ün (candidate isolation) yapısal bir kısıtıdır: yüksek abstraction'dan düşük abstraction'a (kod) doğrudan sıçrama yasaktır; ontolojik zincir tam olmalıdır.

---

## 9. INV-C1..C8 Invariant Önerileri (Proposal)

Aşağıdaki 8 invariant Paper 3'ün omurgasıdır. **Durum: proposal** — implementation sırasında (`crates/osp-core/src/anchoring/` oluşunca) `docs/invariant-spec.md` Bölüm C'ye taşınır. INV #1-15 (Paper 1) ve INV-T1-T8 (Paper 2) ile çakışmadıkları doğrulanmalıdır.

### INV-C1 — Embedding proposes, never decides

**Tanım:** Embedding sadece candidate üretir; nihai anchor kararı hibrit skor (§8.1) + threshold policy + operator onayı ile verilir. Embedding vektörü bir node'un ontolojik konumu olamaz (position family değil, §4.3).

**Yapısal garanti:** `AnchorResolver` trait'i embedding'i `CandidateRetriever` üzerinden aday üretiminde kullanır; `AnchorScorer` ve `AnchorGate` embedding'i görmez. `PositionFamily` enum'unda `SemanticEmbedding` yoktur.

**Not (skalar vs vektör):** `AnchorScorer` embedding **vektörünü** görmez; sadece `CandidateRetriever` tarafından önceden hesaplanmış **skalar similarity skorunu** (0.0–1.0) alır. Embedding vektörü scorer'ın erişim alanı dışındadır. Skorun %25'i (D5) embedding'den gelse bile vektörün kendisi scorer'a girmez — sadece ondan türetilmiş tek sayı girer. Bu, INV-C1'in "embedding proposes, never decides" ilkesini korur: vektör aday üretir, similarity skoru diğer 6 bileşenle birlikte karar verir.

**İhlal örneği:** Bir node'un "position" alanı embedding vektörü olur → anlam vektöre indirgenir, INV-C1 ihlal.

**Paralel:** Paper 2 INV-T1 ("agent hedef koordinatı görmez") ve Paper 1 inv #4 ("engine ölçer, agent beyan etmez") ile aynı prensip: anlam/ölçüm/hedef sayıya indirgenmez.

### INV-C2 — Position family separation

**Tanım:** Conceptual, Physical ve Evidence position family'leri karıştırılamaz. Bir node'un conceptual position'ı ile physical position'ı farklı vektörlerdir; aynı ℝ⁵ içine zorla sokulmaz.

**Yapısal garanti:** `PositionSnapshot.family: PositionFamily` alanı; `PositionVector` family'ye göre farklı eksen seti taşır. **Her üç family'nin (PhysicalCode, ConceptualIntent, Evidence) eksen seti tanımlı olmalıdır** (§4.1) — boş/eksik eksen setine sahip family type-level olarak reddedilir. Bu, Evidence family'nın "test/metric kanıt konumları" belirsizliğinde kalmasını engeller.

**İhlal örneği:** ConceptPacket'e coupling/instability değerleri verilir → conceptual ile physical uzay karışır, model bulanıklaşır.

### INV-C3 — Candidate isolation

**Tanım:** AnchorResolver tarafından üretilen Concept/Rule/Task adayları, operator acceptance olmadan Project Reality mainline'a promote edilemez.

**Yapısal garanti:** Her türetilmiş node `decision_status: DecisionStatus` taşır; mainline sorgular `Accepted` filtresi uygular. Candidate → Accepted geçişi `OperatorCapability` gerektirir (Paper 2 INV-T2 ile aynı capability modeli).

**İhlal örneği:** AnchorResolver bir TaskCandidate üretir, navigator doğrudan çalıştırır → candidate mainline knowledge oldu, INV-C3 ihlal.

**Paralel:** Paper 2 INV-T8 ("AcceptAsProgress ≠ Mainline") genesis karşılığı.

### INV-C4 — Supersede requires authority

**Tanım:** Accepted kararları sadece yeterli epistemik yetkiye sahip kaynak (`SupersedeAuthority`: Operator, ExplicitUser, WitnessedArchitectDecision) geçersiz kılabilir. Düşük yetkili kaynaklar (agent, raw embedding) sadece `CONTRADICTS?` önerir.

**Yapısal garanti:** `SUPERSEDES` edge'i `SupersedeAuthority` capability'si gerektirir; agent kaynağı bu capability'ye sahip değildir.

**İhlal örneği:** AgentHypothesis `SUPERSEDES` ile AcceptedDecision'ı geçersiz kılar → zayıf epistemik yetki güçlüyü ezer, §6 hiyerarşi ihlal.

### INV-C5 — Inferred is not accepted

**Tanım:** Koddan veya LLM'den çıkarılan her bilgi Inferred başlar; insan/operator onayıyla Accepted olur.

**Yapısal garanti:** `DecisionStatus` akışı: `Candidate` (türetildi) → `InReview` → `Accepted`. Inferred kaynak (Agent, LLM, Concept Synthesis) default `Candidate` status'la başlar.

**İhlal örneği:** LLM ürettiği kuralı doğrudan `Accepted` status'la yazmaya çalışır → türetme onaya eşit oldu, INV-C5 ihlal.

### INV-C6 — Code-derived intent is hypothesis (düzeltilmiş)

**Tanım:** Koddan çıkarılan **niyet/vizyon yorumları** Inferred statüsündedir; kodun **ölçülen fiziksel yapısı** (metric'ler) Observed'dır (Paper 1). İkisi karıştırılamaz.

**Yapısal garanti:** Concept Synthesis iki tür çıktı üretir: structural facts (`MetricSource::TreeSitter/Scip`, Observed) ve intent hypotheses (`DecisionStatus::Candidate`, Inferred). Aynı node'da karışmaz.

**İhlal örneği:** "PaymentModule coupling 0.82" (Observed metric) ile "PaymentModule ödeme vizyonunu yansıtıyor" (Inferred yorum) aynı epistemik seviyede Accepted status'a konur → Paper 1 ontolojisi çöker.

**Not:** Bu invariant'ın orijinal hali ("code-derived = hypothesis") düzeltildi — kod metric'leri Observed'dır, sadece koddan çıkarılan *niyet* Inferred'dir (§7).

### INV-C7 — Anchor decision explainable (high-stake ile sınırlı)

**Tanım:** High-stake anchor decisions (rule/task derivation, contradiction, supersession, expected implementation, evidenced-by) skor kırılımı ve gerekçe olmadan graph'a yazılamaz. Düşük-stake edge'ler (Mentions, RelatedTo) opsiyonel.

**Yapısal garanti:** High-stake `ConceptEdgeKind` türleri için `AnchorDecision.explanation` zorunlu; `AnchorGate` bunu doğrular (A1-A6 gate'leri, özgün §9.8). Düşük-stake edge'ler explanation olmadan yazılabilir.

**İhlal örneği:** `DerivesRule` edge'i gerekçe olmadan graph'a yazılır → denetlenemez türetme, INV-C7 ihlal.

**Not:** Bu invariant'ın kapsamı orijinalinden ("her edge") daraltıldı — yüksek-stake ile sınırlı (§8.4).

### INV-C8 — Concept identity must be canonicalized (v0.2)

**Tanım:** Yeni Concept node oluşturulmadan önce canonical key, alias ve glossary dedup kontrolü zorunludur. Zayıf anchor (0.40–0.60 → CreateNode, §8.2) yeni bir ontolojik varlık doğurduğundan, canonicalize gate'i olmadan graph "Payment / Payments / Ödeme / SecurePayment" gibi varyantlarla kirlenir — anchoring kalitesi düşer, agent task üretimi bozulur.

**Yapısal garanti:** `AnchorStore::find_concepts_by_canonical(name, aliases)` metodu CreateNode öncesi çağrılır (§8.2 gate notu, Q13). İki aşamalı dedup: (1) **lexical/glossary dedup (Faz 1-2)** — aynı canonical key, glossary terimi, alias veya edit distance ≤ 2 match varsa yeni node oluşturulmaz, mevcut node'a `TentativeLink`; (2) **embedding dedup (Faz 7)** — cosine similarity ≥ 0.85 ile `SameAsCandidate` (review lane).

**İhlal örneği:** `Concept:Payment` zaten varken anchor resolver "Payments" / "Ödeme" için yeni node oluşturur → graph kirlenir, INV-C8 ihlal.

**Not:** Bu invariant v0.2'de eklendi (arkadaş incelemesi sonucu, Q13 ile birlikte).

---

## 10. Karar Özeti (D1-D13)

Her karar: **karar + gerekçe + OSP ile tutarlılık + itiraz varsa çözüm.**

### D1 — PositionFamily = 3 aile (seçenek B)

**Karar:** ConceptPacket conceptual uzayda, CodeEntity physical uzayda yaşar. Aynı `ℝ⁵` içine zorla sokulmaz. `PositionFamily` enum'ı: `PhysicalCode | ConceptualIntent | Evidence` (3 aile).

**Gerekçe:** Coupling (physical) ile implementation (conceptual) aynı şeyi ölçmez. Karıştırılırsa model bulanıklaşır.

**OSP tutarlılık:** Paper 1 inv #4 "engine ölçer" → "engine doğru uzayda ölçer" genişletmesi.

### D2 — Candidate vs Accepted lane modeli

**Karar:** Otomatik türetme Candidate lane'de serbest; Accepted (mainline) operator onayı gerektirir. `DecisionStatus` enum ile her node status taşır.

**Gerekçe:** Otomatik türetme INV-T2'yi ihlal etmez *eğer* Candidate ise. Operator onayı olmadan Accepted olmaz.

**OSP tutarlılık:** Paper 2 INV-T8 ("AcceptAsProgress ≠ Mainline") genesis genişletmesi.

### D3 — Kod > doküman epistemik hiyerarşi

**Karar:** Çelişki çözümünde "en yeni" değil "en güvenilir kaynak" kazanır. Hiyerarşi: operator > user vision > architect decision > **observed code reality** > documentation > inferred hypothesis > agent hypothesis > raw embedding.

**Gerekçe:** Yazılım mühendisliğinin temeli: kod gerçektir, doküman niyettir. Kod execute olan, doküman stale olabilir.

**OSP tutarlılık:** Paper 1 ontolojisi (MetricSource güven hiyerarşisi).

### D4 — Code-derived concepts: Observed (structure) vs Inferred (intent)

**Karar:** Koddan çıkarılan yapısal metric'ler Observed (Paper 1); koddan çıkarılan niyet/vizyon yorumları Inferred. İkisi karıştırılamaz.

**Gerekçe:** INV-C6'nın orijinal hali ("code-derived = hypothesis") Paper 1'in ontolojik temelini çökertirdi. Düzeltme: kod gerçektir, koddan çıkarılan anlam yorumdur.

**OSP tutarlılık:** Paper 1 "ölçülen şey gerçektir" + Paper 3 "çıkarılan anlam yorumdur" — tamamlayıcı, çelişkisiz.

### D5 — Anchor score ağırlıkları

**Karar:** 7 bileşenli hibrit skor (semantic 0.25 + ontology 0.20 + graph 0.15 + domain 0.15 + code 0.10 + temporal 0.10 + decision 0.05 − penalties). Faz 0-2'de ağırlıklar calibration öncesi başlangıç değerleridir.

**Gerekçe:** INV-C1 gereği semantic_similarity en yüksek ağırlığa sahip ama tek başına karar vermez — 6 başka bileşenle dengelenir.

### D6 — Operator approval boundary

**Karar:** High-stake anchor decision'lar operator onayı gerektirir. Tam liste (§8.4 ile aynı): DerivesRule, DerivesTask, DerivesRisk, Contradicts, Supersedes, ExpectedImplementation, ImplementedBy, EvidencedBy, AntiGoalOf, Constrains (10 edge). Düşük-stake (Mentions, Refines, RelatedTo, DependsOnDecision) serbest. Ayrıca yüksek abstraction → doğrudan kod bağlantısı yasak (ara node şart).

**Gerekçe:** INV-C3 (candidate isolation) + INV-T2 (operator genesis).

### D7 — In-memory vs Kuzu store sınırı

**Karar:** `osp-core` KuzuDB bilmez. `AnchorStore` trait'i soyutlaması; `InMemoryAnchorStore` (Faz 0-2), `KuzuAnchorStore` ayrı `osp-kuzu` crate'inde (Faz 3+).

**Gerekçe:** Paper 1/2'nin "küçük auditable TCB" felsefesi (INV-T4 A4 assumption). osp-core'u DB bağımlılığından korur, deterministik test edilebilirliği korur.

### D8 — SemanticEmbedding position family DEĞİL

**Karar:** `PositionFamily` enum'ında `SemanticEmbedding` yoktur. Embedding `semantic_vector_ref` olarak ConceptPacket içinde bir referanstır.

**Gerekçe (itiraz 1):** Eğer embedding position family ise bir node'un ontolojik konumu olur → INV-C1 ("embedding proposes, never decides") ile çelişir. Embedding index/retrieval aracıdır, konum değil.

**OSP tutarlılık:** Özgün gereksinim §5.4 + Paper 2 INV-T1.

### D9 — Code structure (Observed) vs code intent (Inferred)

**Karar:** Bkz. D4 — bu karar, D4'ün itiraz-2 bağlamındaki uygulamasıdır (INV-C6 düzeltmesi).

**Gerekçe (itiraz 2):** İtiraz-2, INV-C6'nın orijinal halinin ("code-derived = hypothesis") Paper 1 ontolojik temelini çökertme riskine işaret etmişti; D4 bu riski "kodun ölçülen yapısı gerçektir, koddan çıkarılan anlam yorumdur" ayrımıyla giderir.

### D10 — Code > documentation conflict resolution

**Karar:** Epistemik hiyerarşide observed code reality (4.) accepted repository documentation (5.) üzerinde.

**Gerekçe (itiraz 3):** Kod gerçektir, doküman niyettir. "Kod dokümandan saptı" durumunda kod yanlış demek tehlikeli ve yanlış.

**OSP tutarlılık:** Paper 1 MetricSource güven hiyerarşisi.

### D11 — Explanation zorunluluğu high-stake edge'lerle sınırlı

**Karar:** INV-C7 kapsamı daraltıldı — high-stake edge'ler (10 tür) için explanation zorunlu; düşük-stake (4 tür) opsiyonel.

**Gerekçe (itiraz 4):** Her edge için explanation zorunluluğu graph'ı şişirir, düşük-stake bağlantıları pahalılaştırır. İş kârlılığını düşürür.

### D12 — Concept Synthesis, Anchoring sonrasına schedule'lanmış

**Karar:** Concept Synthesis Faz 6'da, Concept Anchoring Faz 1-5'te. Ters sıra anlamsız.

**Gerekçe (itiraz 5):** Concept Synthesis'in çıktısını bağlayacak ontolojik iskelet (Concept node'ları) önce Anchoring ile oluşmalı. Mantıksal bağımlılık.

### D13 — Concept canonicalization and dedup (v0.2, D14→D13 v0.2.1)

**Karar:** Yeni Concept node oluşturulmadan önce canonical key, alias ve glossary dedup kontrolü zorunludur (INV-C8). CreateNode (0.40–0.60 bandı) doğrudan yeni ontolojik varlık doğurmaz; önce mevcut node'lara bağlanmayı dener. İki aşama: lexical/glossary dedup (Faz 1-2), embedding dedup (Faz 7).

**Gerekçe:** `0.40 ≤ score < 0.60 → CreateNode` kuralı canonicalize gate'i olmadan graph kirliliğine yol açar (Payment/Payments/Ödeme varyantları). Bu, anchoring kalitesini düşürür ve dolaylı olarak agent task üretimini bozar.

**OSP tutarlılık:** INV-C3 (candidate isolation) yapısal bir kısıtı — yeni varlık oluşturma da onay disiplinine tabi. Q13 ile birlikte tanımlandı (v0.2).

---

## 11. Fazlama

```
Faz 0 — Spec + fixture
  Bu design doc (bitti)
  + 10 golden fixture cümlesi (özgün §17)
  + beklenen ConceptPacket/edge/position sonuçları
  Çıktı: docs/concept-anchoring-design.md, crates/osp-core/tests/fixtures/anchoring/*.json

Faz 1 — In-memory deterministic MVP
  ConceptPacket, AnchorCandidate, AnchorPlan, PositionSnapshot tipleri
  Rule-based classifier (Türkçe/EN domain glossary)
  Lexical/domain extractor
  InMemoryAnchorStore
  In-memory graph candidate retrieval
  Çıktı: cargo test -p osp-core anchoring

Faz 2 — Scoring + decision
  Hibrit score formülü (D5)
  Threshold policy (0.80/0.60/0.40)
  Strong/Tentative/Create/Contradiction kararları
  INV-C1-C8 enforcement (type-level)

Faz 3 — Kuzu persistence
  osp-kuzu crate (AnchorStore impl)
  Node/rel tabloları (özgün §10)
  AnchorPlan persist/read roundtrip
  Graph query örnekleri (özgün §11)

Faz 4 — Code evidence integration
  CodeEntity symbol index (Paper 1 analyzer bridge)
  EXPECTED_IMPLEMENTATION vs IMPLEMENTED_BY ayrımı
  Kod kanıtı olmayan yüksek riskli vizyon sorguları

Faz 5 — Task/Predicate integration → Paper 2 navigator bridge
  RuleCandidate → PredicateSet üretimi
  ConceptPacket → TaskCandidate üretimi
  Candidate → Accepted (operator approval)
  Agent Navigator'a bridge (INV-T2 doldu)

Faz 6 — Concept Synthesis (D12 sırası)
  Code repo analizi → concept/vizyon/rule hipotezleri
  Structural facts (Observed) vs intent hypotheses (Inferred) ayrımı (D9)
  Concept node'larına hipotez bağlama

Faz 7 — Embedding + LLM-assisted candidate generation
  Gerçek embedding (semantic_similarity bileşeni)
  LLM-assisted classification/extraction (INV-C1: önerir, bağlamaz)
  Numeric calibration of score weights (D5)

Faz 8 — Desktop integration (Project Reality Cockpit)
  ConceptPacket inspector
  Anchor explanation panel
  Weakly anchored / contradiction / evidence gap görselleştirme
  "Human Vision → Decision → Rule → Task → Code → Evidence" zincir görünümü
```

**Kritik disiplin:** Faz 0-2'de LLM yok, embedding yok, Kuzu yok. Saf deterministik classifier + lexical extraction + in-memory graph. Bu OSP'nin inşa prensibiyle birebir (Paper 2'de D1 mock LLM önce, D3 gerçek LLM sonra; G2c-3 mock önce, G2c-4 gerçek sonra). Mekanizma önce deterministic olarak kanıtlanır, stochastic katmanlar sonra eklenir.

---

## 12. Açık Sorular

### Orijinal sorular (özgün §16)

**Q1.** `ConceptPacketType` listesi ilk sürümde kaç türle sınırlı tutulmalı?
*(Öneri: 6 tür — UserVision, Requirement, RuleCandidate, Risk, Decision, Assumption. AntiGoal, Example, Question Faz 1+ eklenebilir.)*

**Q2.** `Concept` alias sözlüğü manuel mi başlatılmalı, yoksa repo analizinden mi çıkarılmalı?
*(Öneri: Faz 1'de manuel domain glossary; Faz 6 Concept Synthesis ile repo-analiz tabanlı zenginleştirme.)*

**Q3.** `RuleCandidate` otomatik `Rule` olabilir mi, yoksa her zaman operator onayı mı gerekir?
*(Karar: INV-C3 gereği her zaman operator onayı. Candidate → Accepted geçişi kapalı.)*

**Q4.** `ExpectedImplementation` ile `ImplementedBy` arasındaki geçiş hangi kanıtla yapılmalı?
*(Öneri: Faz 4 code evidence integration — symbol index + test evidence ile.)*

**Q5.** Embedding storage Kuzu içinde mi, Qdrant sidecar'da mı tutulmalı?
*(Faz 7'ye ertelendi — Q8 ile birleştirildi.)*

**Q6.** Türkçe/İngilizce kavram eşleştirme için ilk domain glossary nasıl kurulmalı?
*(Öneri: Manuel seed glossary + alias mapping — "ödeme→Payment", "güven→Trust/SecurityPerception". Faz 6'da Concept Synthesis ile genişletilir.)*

**Q7.** Çelişki çözümünde hangi karar türü kazanmalı: en yeni mi, en güvenilir kaynak mı, operator kararı mı?
*(Karar: D3 ile netleşti — en güvenilir kaynak. Hiyerarşi §6.2.)*

### Yeni sorular

**Q8.** Embedding storage architecture (Faz 7) — Kuzu vector index mi, harici Qdrant sidecar mı?
*(Erteklendi: Faz 7 implementation karar. osp-core embedding-bağımsız kalmalı.)*

**Q9.** Türkçe/EN çok dilli domain glossary yönetimi — nasıl senkronize edilir, versiyonlanır?
*(Faz 1'de manuel JSON glossary; Faz 6 Concept Synthesis ile otomatik zenginleştirme.)*

**Q10.** Çelişki çözümünde "en yeni" vs "en güvenilir" — D3 ile netleşti, ama *stale accepted decision* nasıl tespit edilir?
*(Açık: temporal_trust_score + staleness_penalty ile, ama threshold calibration Faz 7'de.)*

**Q11.** Paper 3 evidence planı — hangi metrikler ölçülecek?
*(Öneri: anchor precision/recall (golden fixture), embedding-only vs hybrid yanlış anchor oranı, evidence-gap detection başarısı, candidate→accepted conversion rate.)*

**Q12.** Concept Synthesis'in Inferred çıktıları ne sıklıkla operator review'una gitmeli?
*(Açık: high-confidence Inferred'lar otomatik Candidate, low-confidence'lar RequireOperatorReview. Threshold Faz 6'da.)*

**Q13.** Zayıf anchor (0.40–0.60) ile oluşturulan Concept node'ları nasıl merge/dedup edilir (D13)?

*(Karar: INV-C8 + §8.2 gate notu. CreateNode öncesi iki aşamalı canonicalize kontrolü: (1) **Lexical/glossary dedup (Faz 1-2)** — `AnchorStore::find_concepts_by_canonical(name, aliases)` çağrılır; aynı canonical key, glossary terimi veya alias match varsa yeni node oluşturulmaz, mevcut node'a `TentativeLink`. Edit distance ≤ 2 threshold. (2) **Embedding dedup (Faz 7)** — yüksek cosine similarity (≥ 0.85) ile merge candidate `SameAsCandidate` (review lane). Belirsiz durum operator review'a.)*

---

## Sonuç

Concept Anchoring, OSP'yi "kod mimarisi ölçüm protokolü" olmaktan çıkarıp **insan niyeti ile kod kanıtı arasında epistemolojik işletim sistemi** haline getirir. Bu doküman, o geçişin tasarım kararlarını kesinleştirir.

Üçleme tamamlanır:

```
Paper 1 — Project Space Physics          (kod gerçeğini ölçer)
Paper 2 — Architectural Trajectory Navigation  (agent gezinir)
Paper 3 — Project Genesis Layer          (insan niyetini bağlar)  ← bu doküman
```

Ana tez hatırlatma:

> **İnsan niyetini kavrama, kavramı karara, kararı kurala, kuralı göreve, görevi claim'e, claim'i ölçülmüş koda, kodu evidence'a bağlamak.**

Bu zincirin her halkası epistemik olarak korunur: embedding aday üretir ama karar vermez (INV-C1); candidate mainline olamaz (INV-C3); agent accepted'ı supersede edemez (INV-C4); koddan çıkarılan niyet hipotezdir (INV-C6). OSP'nin ontolojik duruşu — anlamı/ölçümü/hedefi sayıya indirgememek — genesis katmanında da korunur.

**Sonraki adım:** Bu doküman (v0.2.1) Faz 0'a geçer — önce 10 golden cümle fixture'ları (`crates/osp-core/tests/fixtures/anchoring/*.json`) hazırlanır, sonra Faz 1 in-memory MVP başlar.

---

## Değişiklik Kaydı

### v0.2 → v0.2.1 (2026-07-02) — tutarlılık patch'i (ikinci review pass)

İkinci arkadaş incelemesi sonucu alınan 5 tutarlılık düzeltmesi:

- **§9 başlık:** `INV-C1..C7` → `INV-C1..C8` (v0.2'de INV-C8 eklenmişti ama bölüm başlığı atlanmıştı — benim hatam).
- **`DerivesRisk` edge'i eklendi (`ConceptEdgeKind`):** §3.1 örneğinde kullanılan `DERIVES_RISK` edge'inin enum karşılığı yoktu. Risk, OSP'de birinci sınıf epistemik nesnedir (ConceptPacketType'da `Risk` zaten var, §12 Q1); insan vizyonundan risk türetmek, rule/task türetmek kadar high-stake. Edge sayısı 14 → **15** (14 ontolojik + 1 meta); high-stake 9 → **10**. §8.4 tablosu ve D6 kararı senkronize güncellendi.
- **`source_quality` → `source_reliability`:** Bu saklanan bir alan değil, `MetricSource`'tan türetilen bir güven katsayısı olduğundan "reliability" doğrusu (Rust tipinde daha net). 4 yerde güncellendi (§4.1 eksen listesi, §4.1 açıklama, §4.2 enum yorumu, v0.2 changelog kaydı).
- **D6 high-stake listesi §8.4 ile eşitlendi:** D6'da 5 edge örnek olarak geçiyordu, §8.4'te 9 edge vardı. Artık D6 tam listeyi (10 edge, DerivesRisk dahil) veriyor ve §8.4'e cross-ref koydu.
- **D14 → D13 yeniden numaralandırma:** v0.2'de "D13 atlandı, D14 eklendi" yaklaşımı doküman hissini bozuyordu ("D13 nerede?" sorusu). Evidence eksenleri INV-C2 kapsamında çözüldüğü için D13 = Evidence Position Axes *yapılmadı* (INV-C2 ile çakışır); bunun yerine D14→D13 yeniden numaralandırıldı, karar listesi D1-D13 ardışık oldu.

**Sayılar (v0.2.1 itibariyle):** INV-C 8 (C1-C8), karar D1-D13 ardışık, edge 15 (14 ontolojik + 1 meta), high-stake 10, position family 3 (hepsinin eksen seti tanımlı).
**Zincirleme takip düzeltmeleri (aynı review pass'ın ikinci yarısı):** `DerivesRisk` eklenmesi ve `source_reliability` rename'i sonrası atlanan 4 tutarlılık patch'i:

- **D11:** "high-stake edge'ler (9 tür)" → "(10 tür)" — `DerivesRisk` eklenince D11 eski kalmıştı.
- **§8.3 kullanım örnekleri:** `DERIVES_RISK` örneği eklendi (`ConceptPacket --DERIVES_RISK--> RiskCandidate`) — enum'da var, §3.1'de var, ama kullanım örneklerinde yoktu.
- **§4.1 Evidence açıklaması:** "`confidence`/`coverage`/`source_reliability` doğrudan `MetricValue` alanları" cümlesi yumuşatıldı — `confidence`/`coverage` doğrudan `MetricValue` alanı, ama `source_reliability` `MetricSource`'tan türetilen bir güven katsayısıdır (rename'in anlamsal gerekçesiyle tutarlı).
- **Sonraki adım:** "(v0.2) bir review pass daha alır" → "(v0.2.1) Faz 0'a geçer" — artık review döngüsü kapandı, fixture aşamasındayız.

### v0.1 → v0.2 (2026-07-02) — review pass

Arkadaş incelemesi (7 nokta) ve üç yönlü beyin fırtınası sonucu alınan değişiklikler:

**Yazım / tutarlılık düzeltmeleri (düşük risk):**
- §8.3: `ConceptEdgeKind` başlık "13 tür" → "14 tür (13 ontolojik + 1 meta)"; enum'da `HasPosition` meta olarak yorumlandı.
- 2 yerde typo: "ödemevizyonu" → "ödeme vizyonu".
- D9 → D4'e referansla kısaltıldı (örtüşme temizliği).
- INV-C1: "vektörü görmez, skalar similarity'yi görür" notu eklendi (ilk okuyanda çelişki algısını giderir).

**Tasarım ekleri (spec seviyesi):**
- §4.1: Evidence position family'nin eksen seti tanımlandı (confidence/coverage/recency/stability/source_reliability) — Paper 1 `MetricValue` ile uyumlu. §4.2 enum yorumu güncellendi.
- §6.4.1: 8 seviyeli epistemik hiyerarşi ↔ 3 seviyeli `SupersedeAuthority` mapping tablosu eklendi ("kod güçlüdür ama karar değildir" ayrımı).
- INV-C2: Yapısal garanti genişletildi — "her üç family'nin eksen seti tanımlı olmalı" (Evidence'ın belirsiz kalmasını engeller).
- INV-C8 (yeni) + D13 (v0.2.1'de D14→D13 yeniden numaralandırıldı): Concept identity canonicalization — CreateNode öncesi dedup gate'i zorunlu (lexical/glossary Faz 1-2, embedding Faz 7).
- §8.2: CreateNode threshold satırına dedup gate notu; Q13: concept merge/dedup sorusu eklendi.

**INV-C sayısı:** 7 → 8. **Karar sayısı:** D1-D12 → D1-D12, D14 (o anki adıyla; v0.2.1'de D14→D13 yeniden numaralandırıldı, bkz. aşağıda).

---

*Doküman kaynakları: özgün `concept-anchoring-algorithm-requirements.md` (v0.1-draft) + Concept Synthesis/Genesis Layer genişletmesi + 5 sınır hassasiyeti düzeltmesi. INV-C1-C8 implementation'da `docs/invariant-spec.md` Bölüm C'ye taşınır.*

