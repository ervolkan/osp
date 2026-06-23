# OSP — Ontological Space Protocol: Yol Haritası

> Bu doküman, `SoftwarePhysics.txt` vizyonunun somut mühendislik yol haritasıdır.
> Canlı bir dokümandır; her Faz bitiminde güncellenir.
> Tek otorite: bu dosya + `SoftwarePhysics.txt` (vizyon kaynağı).
>
> **Authoritative dokümanlar (Faz bazında):**
> - Faz 1 (formalizm + core): `OSP-formalism.md` + `osp-core-design.md`
> - Faz 2 (space engine): `space-engine-design.md`
> - Faz 3 (analyzer): `scip-analyzer-design.md` + `scale-bench-v3.md`
> - Faz 5 (agent/LLM codec): `agent-prompt-semantics.md`
> - Cross-cutting (invariants): `implementation-invariants.md` (15 invariant)
> - Faz 6 proposal (multi-agent): `multi-agent-coordination.md` (0.1-draft, henüz implement değil)

---

## 0. Vizyon (tek cümle)

Yazılım projelerini ve AI agent üretimini, **kavramsal uzayın (S) fizik kuralları** üzerinden
yöneten; **epistemolojik şahitlik (W)** ile zamanı ilerleten; **geometrik sapma (θ)** ile
hatayı derhal tespit eden; ve LLM ile **koordinat-tabanlı sıkıştırılmış protokol (OSP)** üzerinden
konuşan yerel (local/God-Mode) bir motor.

---

## 1. Çekirdek Formalizm (Faz 1'de kesinlenecek)

| Kavram | Formül | Açıklama |
|---|---|---|
| **Uzay** | `S = (V, E, G)` | V: kavram düğümleri, E: bağımlılık vektörleri, G: kural/kütleçekim |
| **Agent** | `A(I) = ΔS_local` | Niyet → izole yerel uzay (PR/taslak) |
| **Zaman** | `t_{c+1} = t_c + W(C, Ω)` | Yalnızca çift-şahitli quorum (WitnessSet Ω) ile ilerler |
| **Kalibrasyon** | `cos θ = (V_vision · P_agent) / (\|V_vision\| \|P_agent\|)` | Sapma açısı; θ > θ_eşik → negatif uzay |

**Üç zaman katmanı:**
- **Miş'li zaman (t_m):** Agent'ın öznel/lokal uzayı — iddia düzeyinde, maliyeti düşük.
- **Şimdiki zaman (t_c):** Onaylanmış nesnel uzay — main branch, production, kesin kurallar.
- **Gelecek zaman (t_f):** Potansiyel — issue'lar, taslaklar, feature branch'ler.

---

## 2. Mimari (Local God Mode)

```
┌──────────────────────────── LOCAL GOD MODE (istemci, Rust) ────────────────────────────┐
│                                                                                        │
│   ┌─────────────┐    ┌──────────────────┐    ┌──────────────────┐    ┌──────────────┐ │
│   │ Space Engine│───►│ Time/Witness FSM │───►│ Calibration (θ)  │───►│ Persistence  │ │
│   │ graf+coords │    │ miş'li→şimdiki   │    │ negatif uzay     │    │ Layer        │ │
│   └──────▲──────┘    └──────────────────┘    └──────────────────┘    └──────────────┘ │
│          │                                                                             │
│   ┌──────┴──────┐    ┌──────────────────┐    ┌──────────────────┐    ┌──────────────┐ │
│   │ Code→Space  │    │ Comparison &     │    │ LLM OSP Codec    │    │ Observability│ │
│   │ Mapper (TS) │    │ Resonance ("O mu?│    │ + Hallucination  │    │ Prometheus + │ │
│   │             │    │  Bu mu?")        │    │   Guardrails     │    │ Grafana      │ │
│   └─────────────┘    └──────────────────┘    └──────────────────┘    └──────────────┘ │
│                                                                                        │
│   ┌─────────────────────────────────────────────────────────────────────────────────┐ │
│   │ Visualization / Dashboard (Faz 2'den itibaren PARALEL — feedback loop erken)    │ │
│   └─────────────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Teknoloji Kararları (Kilitli)

| Katman | Karar | Gerekçe |
|---|---|---|
| **Çekirdek dil** | **Rust** | Determinizm + tip güvenliği + in-memory graf için performans. |
| **AST / multi-lang** | **Tree-sitter** | Çok dilli repo analizi için tek çözüm; Faz 3.5 Language Adapter System'in temeli. |
| **Git geçmişi** | `git` CLI (std::process) | Spike'ta taşınabilirlik; Faz 2+ `git2` crate değerlendirilecek. |
| **Graf veri yapısı** | `petgraph` (başlangıç) → custom (optimizasyon) | Hızlı başlangıç, sonra profile-driven rewrite. |
| **Görselleştirme** | Web (WASM + canvas/three.js) | Rust → wasm-bindgen; browser'da topolojik harita. |
| **LLM entegrasyonu** | API-agnostik (OpenAI uyumlu) + yerel (llama.cpp/ONNX) | God-Mode filtre uygulanacak herhangi bir backend. |

---

## 4. Fazlar

### Faz 0 — Tez Doğrulama Spike'ı (1–2 hafta) ⚡
**Tez:** *Uzay topolojisi + şahitlik oranı, star/yıldız/dokümantasyonun anlatamayacağı bir şey söyler mi?*

- **[0.0] Literatür taraması** (geri bildirimden): Software Visualization, Topological Analysis of Code, Ontological Modeling of Software, Git Archeology. `docs/literature-scan.md`.
- **[0.1]** 5 repo seçimi (kör-test spektrumu): iyi-mimari / framework / büyük-olgun / JS / foam.
- **[0.2]** Tree-sitter ile bağımlılık grafi çıkar (Python + JS başlangıç).
- **[0.3]** `git log` → şahitlik oranı türet (merge-commit vs direkt-commit).
- **[0.4]** Metrikler: kuplaj yoğunluğu, entropi, şahitlik derinliği, sapma-açısı proxy'si.
- **[0.5]** Karşılaştırma raporu (JSON + insan-okur tablo).
- **[0.6] Go/No-Go:** 5 repo'yu ayırt edebiliyor muyuz? Metrikler → aday eksen seçimi (Faz 1 girdisi).

### Faz 1 — Ontolojik Primitifler + Koordinat Sistemi (2–3 hafta) 🧮
- Veri modeli: `Node, Edge, Space, Intent, Claim, Witness, WitnessSet, TimeLayer`.
- **Semantik eksenler** (Faz 0'dan veri güdümlü kesinlenecek):
  - Raw: `x` Kuplaj · `y` Kohezyon · `z` Martin Instability (`I` saf) · `w` İstikrar (entropi) · `v` Şahitlik derinliği
  - Derived: `u` Vizyon hizalaması · `D` Main-sequence distance · `θ` sapma
- Witness operatörü `W(C, Ω)` (WitnessSet tabanlı), tri-state `WitnessStatus`, `EvidenceEvent` dedup.
- **Detaylı uygulama planı:** `docs/OSP-formalism.md §9` (yeniden sıralı: witness+position önce, eksenler sonra).
- **Implementation invariant'ları:** `docs/implementation-invariants.md` (15 adet — yük-taşıyan kararlar kilitli).

### Faz 2 — Space Engine + Persistence Layer (3–4 hafta) ⚙️
- In-memory graf motoru + Time/Witness FSM + θ hesabı + space snapshot (`t_c` checkpoint).
- **[YENİ — Persistence Layer]** (geri bildirimden): KùzuDB / SurrealDB / SQLite+graph / custom binary format. *Faz 2 başında karar verilecek, karşılaştırma raporu `docs/persistence-decision.md`.*
- Zamanda yolculuk: snapshot'lar arası diff.

### Faz 3 — Code→Space Mapper / Repo Analyzer (3–4 hafta) 🔬
- Tree-sitter adaptörleri (Faz 3.5'e uzanacak yapı).
- Git geçmişi → şahitlik kalitesi.
- "Big Bang" genişleme simülasyonu.
- **[YENİ — Scale Test Suite]** (geri bildirimden): 100k+ commit / binlerce dosya. Linux kernel, Chromium, Rust monorepo gibi dev monorepo'larda performans + bellek profili. `docs/scale-bench.md`.

### Faz 3.5 — Language Adapter System (1–2 hafta, Faz 3 ile örtüşür) 🌐
- Tree-sitter gramerlerini tak-çalıştır adapter yapısı. Dile özel "kavram düğümü" eşleştirme kuralları (ör. C# `namespace/class`, Python `module/class`, Rust `mod/struct`).

### Faz 3.6 — Space Browser Visualization (1–2 gün, erken başlatma) 📊
> **Amaç:** "Conceptual space" tezini görsel olarak kanıtla. Paper için interaktif figür.
> Tek dosya statik HTML + D3.js — backend gerektirmez.

- **2D scatter plot:** 15 repo'yu ℝ⁵ uzayda göster. Kullanıcı 5 eksenden 2'sini seçer (x/y/color/size dropdown).
- **Martin main-sequence overlay:** A+I=1 doğrusu.
- **Tri-state witness renk kodlaması:** Witnessed (yeşil) / Unobservable (sarı) / Solo (gri).
- **Click → detail:** Modül/repo detayları (cohesion, coverage, class count).
- **JSON input:** `osp-analyze --json` çıktısını okur.
- Çıktı: `viz/space-browser.html` — tek dosya, GitHub Pages'e deploy edilebilir, paper supplement link.

### Faz 4 — Karşılaştırma & Rezonans + Güvenlik (2–3 hafta) ⚖️
- `S_projem × S_kütüphane` merge simülasyonu + rezonans skoru + karar matrisi.
- **[YENİ — Güvenlik]** (geri bildirimden):
  - **Malicious Witness Detection:** Şahitlik grafındaki anormallikler (bir kullanıcının self-approve'u, bot-farm onayları).
  - **Sybil Resistance:** Sahte kimlik/şahit seli saldırılarına karşı kütleçekim-ağırlıklı güven skoru.

### Faz 5 — LLM OSP Codec + Guardrails (3–4 hafta) 🔌
> **Tasarım sözleşmesi:** `docs/agent-prompt-semantics.md` (1.0-final) — Agent semantiği,
> epistemik projeksiyon (`OspPrompt`), Q4-Q6 claim-based gates, hallucination sınıflandırması.

- Koordinat ↔ LLM paket codec'i (`OspPrompt` ↔ `DeltaProposal`, tiplenmiş — inv #14).
- **Epistemik projeksiyon motoru** (`compute_space_slice`): üç katmanlı alt-uzay seçimi
  (Intent Gravity k=2 → Vision/Rules → Permission/Evidence).
- **Q4-Q6 claim-based gates** (witness öncesi, deterministik): Syntax (inv #12) → Vision (Q5) → Rule (Q6).
- **Hallucination sınıflandırması** (5 tür): Structural / Vision / Rule / Witness / Undersupported —
  her gate failure karşılık gelen kalibrasyon geri bildirimi üretir.
- **PermissionMask** (inv #13): üç-nokta denetim (slice → Agent kabuğu → `commit()` nihai).
- Sapmada **early-exit** (LLM kod üretmeden reddet — Q5 vision gate).
- Token/enerji ölçüm harness'i → makalenin güçlü tablosu.
- **Hibrit Gravity Index** (Statik + Lazy Dynamic): LRU cache, MVP'de tam temizlik,
  Faz 5+'ta incremental (reverse index).
- **[YENİ]** (geri bildirimden):
  - **LLM Hallucination Guardrails:** Üretilen koordinat uzay-dışıysa reddet (Q4-Q6).
  - **Deterministic OSP Output zorunluluğu:** LLM serbest metin değil, parse-edilebilir
    `DeltaProposal` dönmeli (inv #12, #14).
  - **LLM durumsuz, durum Agent kabuğunda** (inv #11).

### Faz 5.5 — Commit Pipeline Visualization (1–2 gün, Faz 5 sonrası) 📊
> **Amaç:** Agent claim'inin Q4-Q6 → Q1-Q3 akışını canlı göster. "God Mode filter"
> görsel olarak kanıtla.
- **Pipeline flowchart:** Claim → Q4 Syntax → Q5 Vision → Q6 Rule → Q1-Q3 Witness → Commit/Reject
- **Real-time gate status:** Hangi gate'te takıldı, hangi değer (θ, rule_id, witness count)
- **Hallucination classification overlay:** 5 tür (Structural/Vision/Rule/Witness/Undersupported)
- **Vision deviation gauge:** θ açısı görsel (vision vector vs claim position)
- Web tabanlı (Faz 3.6 space browser ile aynı frontend stack)

### Faz 6 — Görselleştirme & Observability (Faz 2'den PARALEL) 📊
- **[YENİ — Erken başlatma]** (geri bildirimden): Faz 2-3 ile paralel. Feedback loop görselleştirme olmadan çok yavaş.
- 2D/3D topolojik harita (wasm), negatif uzay kırmızı vurgu, vizyon hattı overlay.
- **[YENİ — Observability]** (geri bildirimden): θ dağılımı, witness kalitesi, rezonans skoru → Prometheus + Grafana. Motor kendi içgörülerini de izlemeli.

### Faz 7 — Akademik Makale (Faz 3–5 sonrası) 📄
- Formalizm (Faz 1) + case study (Faz 0/3) + enerji tasarrufu (Faz 5) + güvenlik (Faz 4).

### Faz 6 — Multi-Agent Coordination & Shared Horizons (proposal, Faz 5 sonrası) 🌐
> **Tasarım proposal:** `docs/multi-agent-coordination.md` (0.1-draft). Şu an implement
> edilmez — Faz 5 stabilize olunca değerlendirilecek.

- **Shared Horizon modeli:** Çoklu-ajan koordinasyonu — izole (private) vs dolanık (shared)
  çalışma modları. Sanal kütle (virtual mass) ile ajanlar birbirinin commit öncesi inançlarını
  kütleçekimsel olarak görür.
- **`t_m_pool` ontolojik genişleme:** `t_m` alt-katmanları (private vs pool-shared belief).
- **Typed `PoolSignal` koordinasyon kanalı:** Reservation/Warning/Checkpoint — free-form chat
  REDDEDİLDİ (inv #14 korunur).
- **Sub-witnessing ≠ quorum:** Havuz içi witnessing sadece conflict detection; quorum hala dış
  şahitlerden (BFT güven modeli korunur, §7).
- **Açık sorular:** Atomic vs Partial commit, cascade rework, liveness/fault tolerance.
- **Invariant adayları:** #16 (sub-witnessing quorum'a sayılmaz), #17 (t_m_pool sanal kütle).

### Faz 8 — Custom Axis Marketplace (Faz 5 sonrası, platform vizyonu) 🧩
> OSP'yi framework'ten platforma çevirir. Altyapı Faz 1'de hazır (`Axis` trait, formalism §2.2);
> bu Faz distribution + trust + discovery katmanı.

- **Custom Axis SDK:** `osp-axis-*` crate template — `Axis` trait reallemesi için scaffolding,
  calibration tooling, test harness.
- **Signed packages:** Her axis imzalı (SHA256 + GPG/cosign), trust chain God Mode'a bağlı (inv #15).
- **Registry/Index:** npm/crates.io analojisi — `osp axis install security.audit@1.2.0`.
- **Discovery:** Kategori bazlı (security, accessibility, performance, compliance, team, testing).
- **Calibration sharing:** Her axis'in `θ_bound` weight + normalization sabitleri metadata olarak
  dağıtılır; community kalibrasyon verisi birikebilir.
- **Network effect:** Community physics rules biriktikçe değer artar — OSP'nin defensible moat'ı.

**Örnek marketplace axis'leri:** `security.audit` (CVE), `wcag.compliance` (accessibility),
`perf.budget` (latency), `compliance.hipaa`/`compliance.sox`, `team.bus_factor`, `test.mutation_score`.

---

## 5. Kesit-Konular (Tüm Fazlar)

| Konu | Durum |
|---|---|
| **Versioning & Space Migration** *(geri bildirim)* | Açık. Eski snapshot'lar yeni ontolojiye göre nasıl re-interpret edilir? `docs/migration-rfc.md` — Faz 2 başında tasarlanacak. Semver benzeri uzay şema sürümü. |
| **Literatür taraması** *(geri bildirim)* | Faz 0 içinde `[0.0]`. |
| **Observability** *(geri bildirim)* | Faz 6 ama tracing katmanı Faz 2'den hazır. |
| **Teşhis kültürü** | Her Faz'da ölçülebilir Go/No-Go kapısı. |

---

## 6. Faz 0 — İlk Sprint Görev Dökümü (şimdi başlıyoruz)

| # | Görev | Çıktı | Durum |
|---|---|---|---|
| 0.0 | Literatür taraması (5 alan, 3–5 kaynak/alanda) | `docs/literature-scan.md` | ⬜ |
| 0.1 | 5 repo seçimi (kör-test) + gerekçe | `docs/spike-repos.md` | ✅ |
| 0.2 | Rust workspace iskeleti + `osp-spike` crate | `crates/osp-spike/` | 🟡 |
| 0.3 | Tree-sitter bağımlılık grafı çıkarıcı (Python+JS) | `crates/osp-spike/src/graph.rs` | ⬜ |
| 0.4 | Git → şahitlik oranı çıkarıcı | `crates/osp-spike/src/witness.rs` | 🟡 (iskelet) |
| 0.5 | Metrik hesaplayıcı | `crates/osp-spike/src/metrics.rs` | ⬜ |
| 0.6 | Karşılaştırma raporu (JSON + tablo) | `crates/osp-spike/src/report.rs` | ⬜ |
| 0.7 | 5 repo üzerinde çalıştır + Go/No-Go | `docs/spike-results.md` | ✅ |

**Tanım:** ⬜ bekliyor · 🟡 iskelet hazır · ✅ tamamlandı

---

## 7. Tanımlar (Sözlük)

- **God Mode:** İstemci bilgisayarda koşan, LLM çıktısını uzay fizik kurallarıyla filtreleyen yerel üst katman.
- **Big Bang:** Onaylanmış bir feature'ın uzaya girişi — yeni düğüm + ilişkilerin anlık oluşumu.
- **Negatif Uzay:** Hatalı/eksik işin vizyon hattı dışına düşmesi (anti-matter).
- **Rezonans:** İki uzayın (proje vs kütüphane) merge simülasyonundaki uyum skoru.
- **Şahitlik (W):** Bir iddianın nesnel uzaya kabulü için gereken bağımsız doğrulama.

---

*Sürüm: 0.4 · Son güncelleme: Faz 5 agent-prompt-semantics + custom axis modeli entegrasyonu (2026-06-21) · Authoritative doc linkleri Faz başına eklendi · Faz 8 marketplace vizyonu*
