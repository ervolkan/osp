# OSP — Session Handoff Document

> **Tarih:** 2026-06-29
> **Session:** Review-driven desktop UX/semantics pass + type-only import distinction
> **Sonraki session:** Node-level witness (roadmap #9) veya arXiv submission
> **Branch:** `feat/desktop-vision-semantics-pass` → PR #1 (github.com/ervolkan/osp/pull/1)

---

## Bu Session'da Yapılanlar (Özet)

İki review turu (harici) sonucu OSP desktop'ında **mimari karar destek sistemi**
oluşturuldu ve analyzer'da **type-only import ayrımı** (kök çözüm) eklendi.
4 commit, 407→414 test.

### Review turu 1 — Engine semantics + UX foundations

**`521c1b6 feat(core): VisionSource provenance + VisionVerdict semantics + infer_role fix`**
- `VisionSource` enum (None/GlobalDefault/BuiltinRole/RoleProfile/UserLoaded) —
  MetricValue provenance modelinin vision karşılığı. "Vision: not loaded" + "θ pass"
  çelişkisini çözer.
- `VisionVerdict` enum (pass/warning/advisory/reject/inconclusive) +
  `evaluate_node_vision()` — node analiz seviyesi karar semantiği. Commit pipeline
  Q5 gate'ten ayrı (Q5 hard reject; VisionVerdict repo-wide analiz).
- **Bug fix:** `infer_role` sıralaması düzeltildi — Support check'i TypeSurface'tan
  önce. `tests/types/*.ts` artık Support'a düşüyor (advisory), TypeSurface'a değil.
  Review'in "TypeSurface 8 reject" şikayetinin kaynağı buydu.
- 2 diagnostic example: `role_diagnostic.rs` (classifier kalitesi),
  `vision_breakdown.rs` (verdict dağılımı).

**`1723368 feat(desktop): decision-support UX pass`**
- Analysis scope filtresi (All/Architectural/Production/Support) — Support-heavy
  repo'larda production sağlığını boğmaması için.
- Deterministik recommendations (breakdown → template mesaj, LLM değil).
- Vision preset'leri (DDD/Microservice/Library/Framework/...) provenance-aware.
- Per-axis bars (score DEĞİL — provenance-aware, MetricValue uyumlu).
- Collapsible sidebar + conditional inspector + reject halo + conditional scatter zone.

### Review turu 2 — Calibration + kök çözüm

**`9b18903 fix(core): TypeSurface coupling calibration + vision_breakdown metric-source fix`**
- `vision_breakdown.rs` metrik kaynağı düzeltildi (`node.position.raw` →
  `module_metrics`) — sahte "0 pass" bulgusu **çürütüldü**, gerçek resim: 91% pass.
- TypeSurface coupling target 0.05 → 0.20 (geçici workaround, type-only sınırlaması için).

**`730b6c7 feat(analyzer): type-only import distinction` (KÖK ÇÖZÜM)**
- TS `import type {Foo}` ve `import {type Foo}` artık runtime dependency değil →
  coupling/instability'den hariç (value-only degree).
- `Edge::is_type_only: bool` + `#[serde(default)]` (backward-compat, Node.classification pattern).
- `Space::out_degree_value/in_degree_value` (type-only hariç); CouplingAxis/InstabilityAxis geçti.
- Tree-sitter textual byte-range detection (grammar `type` qualifier'ı anonim token,
  byte-range ile recover). 3 form: statement-level, per-specifier, mixed→value-wins.
- TypeSurface target 0.20 → 0.05 geri alındı (workaround artık gerekmiyor).
- **Sonuç:** TypeSurface gerçek runtime coupling 0.20 → **0.04** (mimari norma uyum).

### Doğrulama (svelte, 3448 nodes)

| Metrik | Öncesi | Sonrası |
|---|---|---|
| TypeSurface gerçek runtime coupling | 0.20 (şişirilmiş) | **0.04** (0.05 target'a uyum) |
| TypeSurface reject | 8 | **3** (gerçek high-coupling) |
| Total pass | 3142 (91%) | **3152 (91.4%)** |
| Test sayısı | 407 | **414** (+7) |

---

## Mevcut Durum (Test/Build)

```
414 test, 0 fail, 0 warning
5 crate: osp-core + osp-analyzer + osp-spike + osp-desktop + osp-llm-runtime
Branch: feat/desktop-vision-semantics-pass (PR #1 açık)
```

---

## Sıradaki Implementation Öncelikleri

### #1: Node-level witness (YÜKSEK ÖNCELİK — roadmap #9)

**Sorun:** Witness Dashboard repo-level (merge ratio, author sayısı). Node-level
evidence yok — "bu node risky AMA historically stable mı?" sorusu cevapsız.

**Çözüm:**
- `crates/osp-analyzer/src/` — git blame/log per-file extraction
- NodeJson'e witness alanları ekle: `commits_touching`, `distinct_authors`,
  `last_modified`, `churn`, `ownership_concentration`, `recent_volatility`
- Composite risk: vision deviation θ + node witness → `risk_score` (§3.2 stub'ı doldur)
- Frontend: Node Inspector'a "Witness" bölümü

**Neden şimdi:** Metrikler artık doğru (type-only ayrımı yapıldı), bu yüzden
composite risk anlamlı olur. Review'in "battle-tested vs speculative" konusu.

**Efor:** M (4-6 saat)

---

### #2: arXiv submission (YÜKSEK ÖNCELİK — roadmap #7)

**Sorun:** Paper v2.6 içerik olarak hazır ama format/derleme bekliyor.

**Çözüm:**
- paper-draft-v2.6.md → LaTeX derleme (arXiv şablonu)
- Bibliyografya (.bib) derleme
- Figure'ler (Figure 1-3) vektör format
- type-only import ayrımı + VisionSource/VisionVerdict paper'a ekle (§3.2, §6.5)

**Efor:** M (format derleme + yeni sonuçların entegrasyonu)

---

### #3: Python `if TYPE_CHECKING:` idiom (ORTA ÖNCELİK)

**Sorun:** Type-only import ayrımı sadece TS'de. Python'da `if TYPE_CHECKING:`
bloğu içindeki import'lar da type-only sayılmalı ama ayrı mekanizma gerektirir
(if_statement walk + TYPE_CHECKING identifier detection).

**Çözüm:**
- `crates/osp-analyzer/src/adapters/python.rs` — TYPE_CHECKING block detection
- `if TYPE_CHECKING:` içindeki import_statement'ları `is_type_only=true` işaretle

**Efor:** S-M (2-4 saat)

---

### #4: 3D OSP Space visualization (ORTA ÖNCELİK — roadmap #8)

**Sorun:** 2D scatter iki eksen gösteriyor, 5 eksenli uzayın keşfi kısıtlı.

**Çözüm:**
- Three.js / plotly 3D scatter
- 3 eksen: coupling/cohesion/instability; mass=size, entropy/witness-depth=color
- Mode semantics artık temiz (role/risk/metric + type-only ayrımı), 3D anlamlı olur

**Efor:** M-L

---

### #5: Task-success benchmark (DÜŞÜK ÖNCELİK — roadmap #6)

**Sorun:** RQ5 token savings'i gösteriyor ama task success (kod kalitesi) boşluğu var.

**Çözüm:** Real LLM ile OSP prompt vs raw dump → üretilen kodun kalitesi karşılaştırması.

**Efor:** L (evaluation framework gerekli)

---

## Yeni Session'da İlk Soru

```
"Node-level witness (#1) ile başlayalım mı?"
```

Dosya: `crates/osp-analyzer/src/` (git blame extraction yeni)
Başlangıç noktası: `Witness Dashboard` paneli zaten repo-level; node-level'a genişlet.
Paper bağlantısı: §3.2 `risk_score` stub'ı doldur.

---

## Önemli Dosyalar

| Dosya | İçerik |
|---|---|
| `docs/paper-draft-v2.6.md` | Paper v2.6 (arXiv target; type-only ayrımı + VisionSource eklenecek) |
| `docs/roadmap.md` | Faz durum + öncelikler (bu session'la güncellendi) |
| `crates/osp-core/src/vision.rs` | VisionSource + VisionVerdict (yeni) |
| `crates/osp-core/src/space.rs` | Edge::is_type_only + out_degree_value/in_degree_value (yeni) |
| `crates/osp-core/src/axes.rs` | CouplingAxis/InstabilityAxis value-only degree (yeni) |
| `crates/osp-analyzer/src/adapters/shared.rs` | walk_imports_typed + TS type-only detection (yeni) |
| `crates/osp-analyzer/examples/role_diagnostic.rs` | Classifier diagnostic tool (yeni) |
| `crates/osp-analyzer/examples/vision_breakdown.rs` | Verdict breakdown + calibration diagnostic (yeni) |
| `crates/osp-desktop/frontend/index.html` | Decision-support UX (scope/recommendations/presets/bars) |

---

## Bu Session'da Öğrenilen Teknik Dersler

### 1. Diagnostic tool'da yanlış field okuma → sahte bulgu

**Bulgu:** `vision_breakdown.rs` coupling/instability'yi `node.position.raw.{x,z}`'den
okuyordu — ama pipeline bu field'ları **hiç populate etmiyor** (sadece `node.cohesion`;
coupling/instability `module_metrics` map'inde). Sonuç: her node (0.00, 0.50, 0.00)
okundu → sahte "0 pass / 84 reject" bulgusu.

**Ders:** Diagnostic tool yazarken backend'in gerçekten hangi field'ı populate
ettiğini doğrula. `node.position.raw` ile `module_metrics` farklı yerler —
desktop lib.rs zaten doğru yerden (`module_metrics`) okuyordu. Diagnostic'i backend
 mirror'ı yap, ayrı okuma yapma.

**Etki:** "0 pass" teşhisi yanlıştı, gerçek resim 91% pass. TypeSurface calibration
(0.05→0.20) aslında gereksizdi — type-only ayrımı (gerçek kök çözüm) yapıldıktan
sonra 0.05'e geri döndü.

### 2. Tree-sitter grammar anonim token'ları → textual byte-range recovery

**Bulgu:** TS grammar `import type`'daki `type`/`typeof` keyword'ünü **anonim token**
olarak consume ediyor (named node değil, field değil). Structural AST'den
ayırt edilemez. Ama kaynak byte'lardan textual olarak recover edilebilir:
`import` keyword'ünün bitişi ile `import_clause` başlangıcı arasındaki gap.

**Ders:** Tree-sitter grammar bazen qualifier'ları discard eder. AST node-types.json
kontrol et; eğer field yoksa textual byte-range check fallback'i kullan. Per-specifier
(`import {type Foo}`) için her specifier'ın `name` field'ından önceki gap'ı kontrol et.

### 3. Edge struct'a field ekleme → Default derive pattern

**Bulgu:** `Edge`'e `is_type_only: bool` ekleyince ~30 `Edge { from, to, kind }`
literal'i kırıldı (testler dahil). `Default` derive + `..Default::default()` pattern'i
tüm literal'ları tek seferde onardı.

**Ders:** Struct'a opsiyonel field eklerken: (1) `#[serde(default)]` backward-compat
için, (2) `Default` derive + `..Default::default()` mevcut literal'ları korur.
`Node.classification`/`Node.role` zaten bu pattern'i kullanıyordu — aynı deseni
uyguladık. Yeni field eklemek tek dosyada değil, workspace-wide ~30 yerde değişiklik.

### 4. Value-wins dedup: type+value import aynı dosyaya

**Bulgu:** Bir dosyaya hem `import type {Foo}` hem `import {Bar}` yapılırsa, iki
ayrı edge yerine tek edge olmalı. Hangisi kazanır? **Value** — çünkü runtime
dependency mevcut, coupling gerçek.

**Ders:** Dedup HashSet→HashMap yaparken "hangisi kazanır" kuralını net tanımla.
`existing.is_type_only && !new.is_type_only → overwrite false` kuralı mimari
açıdan doğru: runtime import varsa type-only statüsü geçersiz.

### 5. Calibration gamification tuzağı

**Bulgu:** Review "Vision Score 84/100" önerdi. Bu MetricValue provenance'ını
(§3.2) tek scalar'da eritir + gamification'a davet eder ("85'e çıkarayım" için
threshold gevşetme). İtiraz ettim, yerine per-axis bar + provenance etiketi yaptım.

**Ders:** Metrik araçlarında scalar score'lardan kaçın. OSP'nin değeri "84/100"
değil "8 node coupling'de reject, hepsi TypeSurface, threshold 0.30 belki gevşek"
diyebilmekte. Score gizler, breakdown + provenance gösterir.

### 6. Deterministik recommendation > LLM açıklama

**Bulgu:** Review "8 reject → AI açıklama" önerdi. LLM hallucination riski + OSP'nin
"deterministic gate" kimliğine zıt. Yerine deterministik pattern recognition yaptım:
role-axis mismatch, axis dominance, scope suggestion, placeholder dilution.

**Ders:** "Karar destek sistemi" LLM olmak zorunda değil. Pattern'leri insan-okur
template'lere çevirmek deterministic, §9.5 "hallucination as epistemic data" ile
uyumlu, ucuza. LLM yalnızca LLM-arena (Faz 5) katmanında, analiz katmanında değil.

---

## Git Commit History (bu session)

```
730b6c7 feat(analyzer): type-only import distinction (TS import type excluded from coupling)
9b18903 fix(core): TypeSurface coupling calibration + vision_breakdown metric-source fix
1723368 feat(desktop): decision-support UX pass — recommendations, presets, scope, provenance bars
521c1b6 feat(core): VisionSource provenance + VisionVerdict semantics + infer_role fix
```

Tümü `feat/desktop-vision-semantics-pass` branch'inde, PR #1 olarak açıldı:
https://github.com/ervolkan/osp/pull/1

---

## Sonraki Session İçin Hızlı Başlangıç

```
1. docs/session-handoff.md oku (bu dosya)
2. docs/roadmap.md "Implementation Öncelikleri" bölümüne bak — güncellendi
3. PR #1 merge edilmiş olabilir; kontrol et: gh pr status
4. Kod durum: 414 test, 5 crate
5. Corpus repo'lar: P:\repos\osp-spike\ (svelte dahil)
6. Diagnostic tool'lar: cargo run --example role_diagnostic / vision_breakdown
7. İlk soru: "Node-level witness (#1) ile başlayalım mı?"
```

---

*Sürüm: 0.6.3 · 2026-06-29 · review-driven UX/semantics pass + type-only import · 414 test · 5 crate · PR #1*
