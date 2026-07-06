# OSP Spike v2 — Faz 1.10 Entegrasyon Doğrulaması

> Full osp-core pipeline'ın 5 gerçek repo'da uçtan uca çalıştığının kanıtı.
> Tri-state witness + per-node raw positions + derived (θ, D) + vision sapması.
>
> **Önceki:** `docs/spike-results.md` (Faz 0, squash kör-noktası açık).
> **Bu doküman:** Faz 1.10 — kör-nokta çözüldü, osp-core tipleri gerçek veride entegre.

---

## 1. v2 Sonuçları (5 gerçek repo, full geçmiş)

| repo | witness_status | nodes | edges | κ | x | z | w | v | θ | D |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **worms-supabase** | **Unwitnessed** | 26 | 17 | 0.65 | 0.91 | 1.00 | 0.55 | 0.00 | 0.09 | 0.50 |
| **click** | **Witnessed** | 63 | 61 | 0.97 | 0.80 | 0.40 | 0.56 | 0.68 | 0.03 | 0.10 |
| **fastapi** | **UnobservableLocally** | 1.125 | 829 | 0.74 | 0.67 | 1.00 | 0.88 | 0.01 | 0.08 | 0.50 |
| **django** | **UnobservableLocally** | 3.032 | 4.652 | 1.53 | 0.00 | 0.50 | 1.00 | 0.12 | 0.10 | 0.00 |
| **date-fns** | **UnobservableLocally** | 1.610 | 3.623 | 2.25 | 0.67 | 1.00 | 1.00 | 0.26 | 0.06 | 0.50 |

*Sample node = highest-mass (LOC). Vision = (0.4, 0.7, 0.5, 0.5, 0.5) declared "balanced ideal".*
*y (cohesion) = 0.5 placeholder (LCOM4 → Faz 3 SCIP). A (abstractness) = 0.5 placeholder.*

---

## 2. 🎯 Ana Bulgu: Squash Kör-Noktası Çözüldü

### Faz 0 yanılgısı (spike-results.md §2)
Faz 0 `witnessed_ratio` merge-commit varlığına indirgendi. Squash/rebase workflow'unda
merge-commit olmadığı için fastapi/django/date-fns **yanlışlıkla "foam" (unwitnessed)** sanıldı:

| repo | Faz 0 `w_ratio` | Faz 0 yanlış etiket |
|---|---|---|
| fastapi | 0.00 | ~~foam~~ ✗ |
| django | 0.02 | ~~foam~~ ✗ |
| date-fns | 0.06 | ~~foam~~ ✗ |

### Faz 1.10 doğru sınıflama (tri-state)
`spike_witness_classify` üçlü durum ayırt eder (`bridge.rs`):

| repo | merge_ratio | authors | **Faz 1.10 etiket** | Neden |
|---|---|---|---|---|
| worms-supabase | %0 | 1 | **`Unwitnessed`** | Solo + 0 merge → gerçek foam |
| click | %35 | 30 | **`Witnessed`** | Yüksek merge → lokalde gözlemlenebilir |
| fastapi | %0.18 | ~100 | **`UnobservableLocally`** | Çok-author + düşük merge → squash saklıyor |
| django | %1.7 | ~100 | **`UnobservableLocally`** | Aynı |
| date-fns | %5.9 | ~50 | **`UnobservableLocally`** | Aynı |

**fastapi/django/date-fns artık "foam" değil** — `UnobservableLocally`. Bu, Faz 0 spike-results'ın ana
bulgusunun (`§2 squash kör-noktası`) **kod seviyesinde çözüldüğünün** kanıtı.

---

## 3. Per-Repo Sample Node Analizi

Sample = highest-mass (LOC) node — her repo'nun "en büyük modülü".

### `worms-supabase` (Unwitnessed)
- **x=0.91, z=1.00**: En büyük dosya birçok şey import ediyor (yüksek coupling) AMA kimse ona
  import etmiyor (pure unstable leaf, I=1). Foam karakteri: büyük ama izole tek dosya.
- **v=0.00**: Sıfır şahitlik derinliği (solo author, 0 merge).
- **θ=0.09**: Vision'a (balanced ideal) şaşırtıcı biçimde yakın — çünkü vision "balanced" ve bu
  dosyanın değerleri (yüksek x, z) orta-aralıkta. Bu, placeholder vision'in zayıflığını gösterir.

### `click` (Witnessed) — temiz örnek
- **x=0.80, z=0.40**: En büyük modül (olasılıkla `core.py` veya `types.py`) yüksek coupling ama
  dengeli instability (hem import ediyor hem import ediliyor).
- **v=0.68**: Güçlü şahitlik derinliği ( Faz 0'dan `witnessed_ratio=0.35 × ln(1+30) ≈ 1.2`,
  normalize 0.68).
- **θ=0.03, D=0.10**: Vision'a en hizalı repo + main-sequence'e en yakın. **OSP tezinin ideal vakası.**

### `fastapi` (UnobservableLocally)
- **z=1.00, v=0.01**: En büyük dosya pure-unstable leaf + neredeyse sıfır lokal şahitlik sinyali
  (squash workflow review'yi saklıyor).
- **w=0.88**: Yüksek entropi (yaygın değişim — olgun repo karakteri).

### `django` (UnobservableLocally)
- **x=0.00, z=0.50**: En büyük dosya (olasılıkla `__init__.py` veya settings) hiç import etmiyor
  (x=0) ve izole/nötr instability (z=0.5 convention).
- **w=1.00**: Maksimum entropi (34k commit, 3k dosyaya yayılmış — olgun-scale).
- **D=0.00**: A placeholder=0.5 + I=0.5 → main-sequence üzerinde. Placeholder A nedeniyle anlamlı değil henüz.

### `date-fns` (UnobservableLocally)
- **κ=2.25**: En yüksek coupling density (modüler ama içten bağlı — her fonksiyon ayrı dosyada,
  ortak `_lib/` internal'ları paylaşıyor).
- **w=1.00, v=0.26**: Yaygın değişim + düşük lokal şahitlik (rebase workflow).

---

## 4. Faz 0 ↔ Faz 1.10 Karşılaştırması

| Metrik | Faz 0 (spike-results.md) | Faz 1.10 (bu doküman) | İyileşme |
|---|---|---|---|
| Witness sınıflama | Binary (w_ratio, squash kör) | **Tri-state** (Witnessed/Unwitnessed/Unobservable) | fastapi/django/date-fns doğru |
| Koordinat | Repo-level aggregate (κ, H, w_depth) | **Per-node RawPosition** (x,y,z,w,v) | Her modül kendi konumu |
| Sapma θ | Repo-level proxy (hub_ratio×…) | **Per-node cosine** (raw vs vision) | Geometric, vision-aware |
| D (Martin) | Yok | **Ayrı derived metric** (inv #10) | Main-sequence diagnosable |
| Pozisyon | Repo-tek-sayı | **Raw + Derived struct** (inv #4 dairesellik) | Tip-güvenli |

---

## 5. Pipeline Doğrulaması — Tüm osp-core Modülleri Çalıştı

| Modül | Faz | v2'de rolü | Durum |
|---|---|---|---|
| `space` | 1.1 | `spike_graph_to_space` → generic graf | ✅ |
| `coords` (RawPosition/Derived) | 1.4 | Per-node (x,y,z,w,v) + derived (u,θ,D) | ✅ |
| `axes` (Coupling/Entropy/WitnessDepth/Instability/Cohesion) | 1.3/1.9 | 5 raw eksen (4 gerçek + 1 placeholder) | ✅ |
| `witness` (Evidence/WitnessSet/tri-state) | 1.5 | Tri-state sınıflama altyapısı | ✅ |
| `bridge` | 1.6 | Faz 0 → osp-core çeviri + tri-state heuristic | ✅ |
| `vision` (VisionVector/Cosine/compute_derived) | 1.7 | θ + D hesabı | ✅ |
| `bigbang` + `time` | 1.8 | (v2'de doğrudan kullanılmadı — Faz 2 commit akışı) | ✅ (tested) |

**128 birim test yeşil** (osp-core 96 + osp-spike 32). Tüm 10 invariant yapısal guarantee.

---

## 6. Kalibrasyon Açıkları (Faz 1.11 + Faz 3)

### Placeholder değerler (v2'de nötr, Faz 3'te gerçek)
- **y (cohesion) = 0.5**: LCOM4 için tree-sitter class/field analizi gerek. Faz 3 SCIP ile gerçek.
- **A (abstractness) = 0.5**: Sınıf sayımı gerek. D = |A + I − 1| bu yüzden v2'de yarı-anlamlı.
  django D=0.00 (A=0.5+I=0.5 kesişimi) placeholder sanatı, mimari gerçek değil.

### Vision (declared sample)
- `(0.4, 0.7, 0.5, 0.5, 0.5)` keyfi "balanced ideal". θ değerleri buna göre düşük (0.03-0.10).
  Faz 2 Space Engine'elle-deklare vision (mimari kurallardan) gelecek; o zaman θ daha anlamlı.

### Tri-state heuristic eşikleri
- `MERGE_RATIO_OBSERVABLE = %10` (`bridge.rs`). Faz 1.11 kalibrasyon korpusunda (15-20 repo) tune.
  date-fns %5.9 → UnobservableLocally (eşik altı); %10 olsaydı Witnessed olabilirdi.

---

## 7. Go/No-Go: Faz 1 ENTEGRE

| Kriter | Sonuç |
|---|---|
| Full pipeline 5 repo'da crash'siz çalıştı mı? | ✅ (django 34k commit dahil) |
| Tri-state squash kör-noktasını çözdü mü? | ✅ fastapi/django/date-fns `UnobservableLocally` |
| Per-node RawPosition anlamlı üretildi mi? | ✅ (coupling, instability per-node doğru) |
| DerivedPosition (u, θ, D) hesaplandı mı? | ✅ (placeholder A/y ile) |
| osp-core tipleri gerçek veride tutarlı mı? | ✅ (Faz 0 graph → Space bridge koruyucu) |

**Karar: Faz 1 GO.** Pipeline entegre, tipler sağlam, tri-state tezi kanıtlı.

---

## 8. Sıradaki Adımlar

- **Faz 1.11 (kalibrasyon):** 15-20 repo korpusu (Py/Rust/TS/Go) ile `MERGE_RATIO_OBSERVABLE`,
  weight'ler (1.0/0.8/0.7/0.4), `θ_quorum` tune.
- **Faz 2 (Space Engine):** elle-deklare vision parse, `commit()` akışında per-node reposition
  (`CosineDeviation` ile), persistence.
- **Faz 3 (SCIP):** gerçek LCOM4 (y) + abstractness (A) → D anlamlı.

---

*Veri: 2026-06-19 · 5 repo, tam geçmiş · `spike-output-v2.json` (makine-okur)*
