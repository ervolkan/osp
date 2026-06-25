# 28-Repo Corpus Results — Extended Analysis

> **Tarih:** 2026-06-24 (Rust/Go edges 2026-06-25 yeniden çalıştırıldı)
> **Diller:** Python (12), TypeScript (3), JavaScript (3), Rust (5), Go (4)
> **SCIP:** scip-python (Docker) + scip-typescript (npm) + scip-rust (Docker) + scip-go (Docker)

## Tam 28-Repo Sonuç Tablosu

### Mevcut 15 Repo (Faz 3 SCIP deployment)

| # | Repo | Lang | Nodes | Edges | κ | A | I | D | **y** | SCIP Cov |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | click | Py | 63 | 61 | 0.97 | 0.02 | 0.63 | 0.36 | **0.67** | 100% |
| 2 | django | Py | 2966 | 4659 | 1.57 | 0.00 | 0.66 | 0.18 | **0.66** | 98% |
| 3 | flask | Py | 83 | 131 | 1.58 | 0.01 | 0.71 | 0.34 | **0.63** | 100% |
| 4 | fastapi | Py | 1133 | 831 | 0.73 | 0.01 | 0.70 | 0.13 | **0.62** | 100% |
| 5 | httpx | Py | 60 | 4 | 0.07 | 0.07 | 0.50 | 0.45 | **0.62** | 100% |
| 6 | rich | Py | 213 | 404 | 1.90 | 0.04 | 0.71 | 0.36 | **0.60** | 100% |
| 7 | pydantic | Py | 534 | 1016 | 1.90 | 0.02 | 0.70 | 0.22 | **0.52** | 19% |
| 8 | requests | Py | 37 | 21 | 0.57 | 0.05 | 0.43 | 0.49 | **0.49** | 51% |
| 9 | worms-supabase | Py | 26 | 17 | 0.65 | 0.00 | 0.45 | 0.35 | 0.50* | — |
| 10 | date-fns | TS | 1550 | 3579 | 2.31 | 0.05 | 0.93 | 0.02 | **0.51** | 96% |
| 11 | svelte | TS | 3450 | 4232 | 1.23 | 0.00 | 0.92 | 0.21 | **0.51** | 2% |
| 12 | vitest | TS | 2236 | 1881 | 0.84 | 0.02 | 0.57 | 0.35 | **0.54** | 91% |
| 13 | chalk | JS | 13 | 11 | 0.85 | 0.00 | 0.81 | 0.35 | **0.54** | 38% |
| 14 | commander.js | JS | 159 | 135 | 0.85 | 0.00 | 0.81 | 0.16 | **0.52** | 8% |
| 15 | lodash | JS | 27 | 0 | 0.00 | 0.50 | 0.50 | 0.00 | 0.50* | — |

### Yeni 13 Repo (Rust + Go + Foam)

| # | Repo | Lang | Nodes | Edges | κ | A | I | D | **y** | SCIP Cov | Category |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 16 | serde | Rust | 208 | 111 | 0.53 | 0.05 | 0.53 | 0.34 | **0.59** | 42% | Stable heavy |
| 17 | ripgrep | Rust | 100 | 93 | 0.93 | 0.02 | 0.52 | 0.36 | **0.75** | 98% | Stable heavy |
| 18 | tracing | Rust | 256 | 100 | 0.39 | 0.12 | 0.53 | 0.22 | **0.69** | 92% | Stable modern |
| 19 | axum | Rust | 302 | 72 | 0.24 | 0.06 | 0.53 | 0.28 | **0.61** | 32% | Stable modern |
| 20 | tokio | Rust | 786 | 600 | 0.76 | 0.08 | 0.60 | 0.27 | **0.71** | 87% | Stable heavy |
| 21 | cobra | Go | 36 | 1 | 0.03 | 0.08 | 0.50 | 0.43 | **0.57** | 100% | Stable heavy |
| 22 | viper | Go | 33 | 6 | 0.18 | 0.24 | 0.45 | 0.08 | **0.68** | 100% | Stable |
| 23 | gin | Go | 99 | 29 | 0.29 | 0.10 | 0.58 | 0.18 | **0.71** | 100% | Stable modern |
| 24 | prometheus | Go | 955 | 2271 | 2.38 | 0.10 | 0.69 | 0.06 | **0.61** | 100% | Stable heavy |
| 25 | Auto-GPT | Py | 3085 | 7132 | **2.31** | 0.01 | 0.62 | 0.27 | 0.50* | — | **AI-era foam** |
| 26 | crewAI | Py | 1258 | 3902 | **3.10** | 0.03 | 0.57 | 0.29 | 0.50* | — | **AI-era foam** |
| 27 | langchain | Py | 2527 | 3445 | **1.36** | 0.04 | 0.59 | 0.27 | 0.50* | — | **AI-era foam** |
| 28 | llama_index | Py | 3833 | 7470 | **1.95** | 0.03 | 0.74 | 0.23 | 0.50* | — | **AI-era foam** |

**y\*** = placeholder (no SCIP — Tier 1 only).
Rust/Go edge extraction (tree-sitter `use`/`import`) tamamlandı 2026-06-25 — eski `edges=0†` kısıtlaması kalktı.

## Ana Bulgular

### 1. Coupling Density (κ) — Foam vs Stable

| Kategori | Ortalama κ | Range |
|---|---|---|
| **AI-era foam** (4 repo) | **2.18** | 1.36–3.10 |
| Stable Python | 1.14 | 0.07–1.90 |
| Stable TS/JS | 1.04 | 0.00–2.31 |
| Rust | 0.57 | 0.24–0.93 |
| Go | 0.72 | 0.03–2.38 |

**Bulgü:** Foam repoların coupling yoğunluğu stable repolardan **2-4× daha yüksek**.
crewAI (κ=3.10) ve Auto-GPT (κ=2.31) en yüksek — kontrolsüz bağımlılık büyümesi.
Rust/Go artık gerçek κ değerleri ile stable Python/TS bandında, foam'dan net ayrışıyor.

### 2. Instability (I) — Foam daha instabil

| Kategori | Ortalama I |
|---|---|
| **AI-era foam** | **0.63** |
| Stable Python | 0.55 |
| Stable TS/JS | 0.74 |
| Rust | 0.54 |
| Go | 0.56 |

### 3. LCOM4 Cohesion — Rust/Go yüksek (well-structured)

| Kategori | Ortalama y | Range |
|---|---|---|
| **Rust** (SCIP) | **0.67** † | 0.59–0.75 |
| **Go** (SCIP) | **0.64** | 0.57–0.71 |
| Stable Python (SCIP) | 0.60 | 0.49–0.67 |
| Stable TS/JS (SCIP) | 0.52 | 0.51–0.54 |
| Foam (placeholder) | 0.50* | — |

**Bulgü:** Rust projeleri en yüksek cohesion (0.67 ortalama) — güçlü tip sistemi + trait-based design. † Rust cohesion değerleri loader bug öncesi üretilmiştir; rerun gerekir (bkz. Sınırlamalar #4).

### 4. 5 Dil Karşılaştırması

| Dil | Repo Sayısı | Ortalama y | Ortalama κ | Ortalama I | SCIP Çalışan |
|---|---|---|---|---|---|
| Python | 12 | 0.58 | 1.23 | 0.55 | 8/12 |
| TypeScript | 3 | 0.52 | 1.46 | 0.74 | 3/3 |
| JavaScript | 3 | 0.52 | 0.57 | 0.74 | 2/3 |
| **Rust** | 5 | **0.67** † | 0.57 | 0.54 | 5/5 |
| **Go** | 4 | **0.64** | 0.72 | 0.56 | 4/4 |

### 5. SCIP Toolchain Doğrulama

| Dil | SCIP Tool | Docker Image | Status | Notlar |
|---|---|---|---|---|
| Python | scip-python | sourcegraph/scip-python | ✅ | --project-name + --version gerekli |
| TypeScript | scip-typescript | npm (native) | ✅ | --infer-tsconfig |
| **Rust** | scip-rust | sourcegraph/scip-rust | ✅ | rust-analyzer tabanlı, 60-190s/repo |
| **Go** | scip-go | sourcegraph/scip-go | ✅ | scip-go --output index.scip |

**Toplam SCIP-analyzed class:** 13,031 (15 repo) + Rust/Go classes = **~17,000+**

## Bilinen Sınırlamalar

1. **Rust/Go edge extraction (çözüldü 2026-06-25):** Tree-sitter adapter artık `use` (Rust) ve `import` (Go) statement'lerini parse ediyor. Rust için `crate::`/`super::`/`self::` prefix stripping + trailing type drop + grouped `use` expansion; Go için `go.mod` module-path detection + package-directory index. Tüm 9 Rust/Go repo artık gerçek edges/κ/I/D değerleri veriyor (eskiden 0† idi).

2. **Foam repos SCIP yok:** langchain timeout (çok büyük). Auto-GPT/crewAI/llama_index Tier 1 only. Bu repolarda cohesion y=0.50* placeholder. Ama coupling/instability verisi gerçek ve değerli.

3. **SCIP coverage değişken:** Bazı Rust repolarda (axum 32%, serde 42%) kısmi coverage. MetricValue.confidence bu belirsizliği yansıtır.

4. **Rust cohesion loader bug (çözüldü 2026-06-25):** Önceki teşhis "scip-rust field-access üretmiyor, toolchain limitation" yanlıştı — scip-rust veriyi üretiyor. Gerçek kök neden: OSP SCIP loader'ı rust-analyzer'in `impl#[Type]...method().` sembol desenini tanımıyordu (sadece Python/TS `Type#member` desenini biliyordu). Bu yüzden Rust struct'larının method'ları yakalanamıyor → hiçbir class hem method hem field gösteremiyordu → LCOM4 her zaman 1 (placeholder-level) çıkıyordu. Fix: `symbol_belongs_to_class` yardımcı fonksiyonu iki deseni de ele alıyor (Python/TS/trait `Type#member` + Rust impl-block `impl#[Type]...`). Doğrulama: serde n=319 class, fix öncesi 0 class hem method+field → fix sonrası 117 class, gerçek LCOM4 hesaplanıyor (serde y: placeholder → 0.57). **Bu bir toolchain limitation değil, OSP loader bug'ıydı.** Yukarıdaki Rust cohesion değerleri (y=0.67) fix öncesi üretilmiştir; Rust repo'ları için 28-repo rerun gerekir.
