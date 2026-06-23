# OSP Desktop UI Design — Tauri Application Architecture

> **Status:** Design Spec / Faz 7-8 target
>
> Bu doküman OSP'nin masaüstü uygulamasının UI mimarisini, panel tasarımını ve
> veri akışını tanımlar. Temel referans: arkadaş review'u (Tauri önerisi) +
> OSP mevcut yetenekleri (Faz 0-5).
>
> **Öncelik:** bu doküman > yorumsel kararlar. Tauri seçimi onaylandı.
>
> **Version:** 0.1-draft · 2026-06-23

---

## 1. Mimari

### 1.1 Neden Tauri?

OSP yerel-öncelikli (local-first) bir araçtır: Git geçmişi, tree-sitter parsing,
SCIP indeksleme — hepsi yerel dosya sisteminde çalışır. Web uygulaması bu
erişimi sandbox'lar; Electron ise ~150 MB kurulum ile OSP'nin "hafif"
felsefesine aykırıdır.

**Tauri** (Rust backend + web frontend, ~10 MB binary) üç kritik avantaj sağlar:

1. **osp-core/osp-analyzer doğrudan gömülür** — HTTP layer yok. Space,
   AnalysisResult, CoordinateSystem bellekte direkt frontend'e aktarılır
   (Tauri IPC, JSON serialization).
2. **Local-first** — `git log`, `tree-sitter`, `scip-python` Docker komutları
   Rust backend'te doğrudan çalışır.
3. **Mevcut viz hazır** — `viz/space-browser.html` (D3.js) Tauri webview'da
   direkt çalışır; embedded JSON yerine canlı Rust backend'ten beslenir.

### 1.2 Teknoloji Stack

| Katman | Teknoloji | Gerekçe |
|---|---|---|
| **Desktop shell** | Tauri v2 | Rust-native, ~10 MB, Apache-2.0 uyumlu |
| **Backend** | Rust (osp-core + osp-analyzer embedded) | Doğrudan crate gömme, zero IPC overhead for computation |
| **Frontend framework** | Svelte 5 + TypeScript | Hafif (~2 KB runtime), reaktif, OSP'nin minimal felsefesi |
| **Görselleştirme** | D3.js v7 (2D) + Three.js (3D graf) | Space browser'da doğrulandı |
| **Stil** | Tailwind CSS + CSS variables | Dark theme (GitHub tarzı), responsive |
| **State management** | Svelte stores | Basit, framework-dahili |

**Neden Svelte değil React?** OSP'nin "lightweight" kimliği: Svelte runtime ~2 KB
vs React ~45 KB. Build step var ama Tauri zaten Node.js toolchain gerektiriyor.
Reaktif programlama modeli space visualization için yeterli.

### 1.3 Proje Yapısı

```
crates/
├── osp-core/              # Mevcut — formal model
├── osp-analyzer/          # Mevcut — tree-sitter + SCIP
├── osp-spike/             # Mevcut — frozen reference
└── osp-desktop/           # YENİ — Tauri uygulaması
    ├── src-tauri/         # Rust backend
    │   ├── Cargo.toml     # tauri + osp-core + osp-analyzer deps
    │   ├── tauri.conf.json
    │   └── src/
    │       ├── main.rs    # Tauri app entry + command registrations
    │       ├── commands/  # Tauri IPC command handlers
    │       │   ├── analyze.rs    # analyze_repo → AnalysisResult JSON
    │       │   ├── space.rs      # Space query → nodes/edges JSON
    │       │   ├── commit.rs     # SpaceEngine::commit → outcome JSON
    │       │   ├── vision.rs     # VisionConfig TOML read/write
    │       │   └── witness.rs    # WitnessSet query
    │       └── state.rs   # AppState (SpaceEngine singleton)
    ├── src/               # Svelte frontend
    │   ├── App.svelte     # Main layout (sidebar + panel area)
    │   ├── lib/
    │   │   ├── stores.ts  # Global state (space, vision, engine state)
    │   │   ├── api.ts     # Tauri invoke() wrappers
    │   │   └── types.ts   # TypeScript types (mirror Rust structs)
    │   ├── components/
    │   │   ├── SpaceTopology.svelte
    │   │   ├── VisionEditor.svelte
    │   │   ├── ClaimDiff.svelte
    │   │   ├── CommitPipeline.svelte
    │   │   ├── WitnessDashboard.svelte
    │   │   ├── RuleManager.svelte
    │   │   ├── PermissionManager.svelte
    │   │   ├── ScipManager.svelte
    │   │   ├── TimeTravel.svelte
    │   │   └── CalibrationAlerts.svelte
    │   └── app.css        # Tailwind + dark theme
    ├── package.json
    └── vite.config.ts
```

---

## 2. Rust ↔ Frontend Veri Akışı

### 2.1 Tauri Commands (IPC)

Tauri, Rust fonksiyonlarını `#[tauri::command]` makrosu ile frontend'e açar.
Frontend `invoke("command_name", { args })` ile çağırır.

```rust
// src-tauri/src/commands/analyze.rs
#[tauri::command]
async fn analyze_repo(
    state: tauri::State<'_, AppState>,
    repo_path: String,
    scip_path: Option<String>,
) -> Result<AnalysisResultJson, String> {
    let config = AnalysisConfig {
        scip_index: scip_path.map(PathBuf::from),
        ..Default::default()
    };
    let registry = AdapterRegistry::default_all();
    osp_analyzer::pipeline::analyze_repo_with_config(
        Path::new(&repo_path), &registry, &config
    ).map(|r| r.into()).map_err(|e| e.to_string())
}
```

### 2.2 Command Registry

| Command | Input | Output | Backend |
|---|---|---|---|
| `analyze_repo` | `repo_path, scip_path?` | `AnalysisResultJson` | osp-analyzer |
| `get_space` | — | `SpaceJson { nodes, edges }` | osp-core::Space |
| `get_node_detail` | `node_id` | `NodeDetailJson` | Space + CoordinateSystem |
| `compute_positions` | `delta_nodes, delta_edges` | `RawPositionJson` | `compute_raw_from_delta` |
| `commit_claim` | `claim_json, witness_set_json` | `CommitOutcomeJson` | `SpaceEngine::commit` |
| `validate_proposal` | `DeltaProposalJson` | `Result<(), SyntaxViolationJson>` | `OutputContract::validate` |
| `compute_space_slice` | `target_nodes, k_hops` | `SpaceSliceJson` | `compute_space_slice` |
| `get_vision_config` | — | `VisionConfigJson` | osp-core::vision_config |
| `set_vision_config` | `VisionConfigJson` | `()` | TOML write |
| `get_witness_status` | `repo_path` | `WitnessStatusJson` | osp-core::witness |
| `get_scip_coverage` | `repo_path` | `SemanticCoverageJson` | AnalysisResult |
| `time_travel` | `t_c` | `SpaceJson` | `SnapshotStore::restore` |
| `register_rule` | `rule_config_json` | `()` | `SpaceEngine::register_rule` |

### 2.3 Veri Serileştirme

Rust tipleri `serde::Serialize` ile JSON'a dönüştürülür (zaten derives mevcut).
Frontend TypeScript tipleri Rust tiplerini mirror eder:

```typescript
// src/lib/types.ts
interface RawPosition { x: number; y: number; z: number; w: number; v: number }
interface Node { id: number; kind: string; mass: number; cohesion?: number }
interface Edge { from: number; to: number; kind: string }
interface AnalysisResult {
  space: { nodes: Node[]; edges: Edge[] };
  module_metrics: Record<number, ModuleMetrics>;
  repo_metrics: RepoMetrics;
  semantic_coverage: SemanticCoverage;
}
```

**Performans notu:** Büyük repolar (django 3k nodes) için Space JSON ~500 KB.
Tauri IPC bu boyutu problemsiz taşır. Daha büyük repolar için (50k+) chunked
streaming veya WebSocket düşünülebilir (Faz 4 KùzuDB sonrası).

---

## 3. Panel Tasarımları

### 3.1 Space Topology (Öncelik: MVP)

Modülleri ℝ⁵ uzayda 2D/3D olarak görselleştirir. Mevcut `space-browser.html`
tabanlı, ama canlı Rust backend'ten beslenir.

```
┌─────────────────────────────────────────────┐
│ Space Topology                    [2D|3D]   │
│                                               │
│ X:[coupling▼] Y:[instability▼]               │
│ Color:[cohesion▼] Size:[mass▼]               │
│                                               │
│  1.0 ─ · · · · · ○ svelte                    │
│       │      ○ date-fns                      │
│  0.5 ─ │   ○ ○ ○ django                      │
│       │  ○ fastapi                           │
│  0.0 ─ ┼─────────────                        │
│       0.0    0.5    1.0                      │
│                                               │
│ ─ ─ ─ Martin Main Sequence (A+I=1) ─ ─ ─    │
│ ● Witnessed  ◐ Unobservable  ○ Solo          │
│                                               │
│ [click node → detail panel]                  │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- Eksen seçici (7 option: coupling, A, I, D, y, nodes, classes)
- Martin main-sequence overlay (A+I=1 doğrusu)
- θ sapma açısı görselleştirme (vision vector → node, renkli yay)
- Node click → sağ panelde detay (metrics, imports, witness status)
- 3D mode: Three.js ile node-edge graf (coupling-X, instability-Y, cohesion-Z)
- Tri-state witness renk kodlaması

**Backend:** `get_space`, `get_node_detail`

---

### 3.2 Vision Editor (Öncelik: MVP)

`osp-vision.toml` için görsel düzenleyici. Eşikleri slider ile ayarlama,
canlı önizleme (space topology'de değişiklik anında yansır).

```
┌─────────────────────────────────────────────┐
│ Vision Editor                    [Save TOML] │
│                                               │
│ Vision Vector                                 │
│   x (coupling)     ━━━●━━━━ 0.40             │
│   y (cohesion)     ━━━━━●━━ 0.70             │
│   z (instability)  ━━━●━━━━ 0.50             │
│   w (entropy)      ━━━━●━━━ 0.55             │
│   v (witness-depth)━━━━━━●━ 0.80             │
│                                               │
│ Thresholds                                    │
│   θ_bound          ━━●━━━━━━ 0.25            │
│   θ_quorum         ━━━●━━━━━ 1.50            │
│   min_approvers    [ 2 ]                     │
│                                               │
│ [Preview: 12/15 nodes in positive space]     │
│ [3 nodes would be rejected at current θ]     │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- Slider ile 5 eksen + 3 threshold ayarı
- Canlı preview: "kaç node vision'a uyuyor / kaç reddedilir"
- TOML export/import
- Preset'ler (strict, balanced, lenient)

**Backend:** `get_vision_config`, `set_vision_config`

---

### 3.3 Claim Diff Viewer (Öncelik: Yüksek)

Bir Agent'ın `DeltaProposal` önerisini "önce/sonra" pozisyon değişimiyle gösterir.
`compute_raw_from_delta` çıktısını görselleştirir.

```
┌─────────────────────────────────────────────┐
│ Claim Diff — Agent #42, Claim #107           │
│                                               │
│ ΔS: +2 nodes, +1 edge                       │
│                                               │
│  BEFORE            AFTER                      │
│  ○ node 10        ● node 10 (moved)          │
│  ○ node 11        ● node 11 (new)            │
│  ○ node 12        ● node 12 (new)            │
│     ↓                ↓                        │
│  coupling: 0.3     coupling: 0.65 ← Δ+0.35   │
│  θ to vision: 0.15 θ to vision: 0.28 ← Δ+0.13│
│                                               │
│  [Accept]  [Reject]  [Request Changes]       │
│                                               │
│  Position computed by engine (not LLM)       │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- Before/after pozisyon karşılaştırma ( coupling, cohesion, instability)
- Δ değerleri (değişim miktarı)
- θ vision sapma açısı değişimi
- "Engine-computed, not LLM-declared" etiketi (inv #4 görsel kanıt)
- Accept/Reject/Request-changes butonları

**Backend:** `compute_positions`, `commit_claim`

---

### 3.4 Commit Pipeline (Öncelik: Yüksek)

Agent claim'inin Q4→Q5→Q6→Q1-Q3 akışını canlı gösterir. Hangi gate'te
takıldığını görselleştirir.

```
┌─────────────────────────────────────────────┐
│ Commit Pipeline — Claim #107                 │
│                                               │
│  ┌─────┐   ┌─────┐   ┌─────┐   ┌──────┐     │
│  │ Q4  │──▶│ Q5  │──▶│ Q6  │──▶│ Q1-Q3│     │
│  │Syntax│   │Vision│   │Rule │   │Witness│    │
│  │ ✅  │   │ ✅  │   │ ❌  │   │  ⏸   │     │
│  └─────┘   └─────┘   └─────┘   └──────┘     │
│                                               │
│  Q6 FAILED: structural.no_self_import        │
│  Rule: module 10 imports itself              │
│  Severity: Hard                              │
│                                               │
│  Hallucination class: RuleHallucination      │
│  Calibration: "Avoid self-import, use        │
│  composition instead"                        │
│                                               │
│  [Retry with feedback]  [Reject]             │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- 4 gate flowchart (Q4→Q5→Q6→Q1-Q3) — her gate status (✅/❌/⏸)
- Failure detail: hangi gate, hangi değer, hangi kural
- Hallucination classification (5 tür, renk kodlu)
- Calibration feedback preview
- Retry/Reject butonları

**Backend:** `commit_claim` (EngineCommitError parse)

---

### 3.5 Witness Dashboard (Öncelik: MVP)

Tri-state witness durumunu, merge_ratio grafiğini, şahitlik geçmişini gösterir.

```
┌─────────────────────────────────────────────┐
│ Witness Dashboard                            │
│                                               │
│  Status Distribution (15 repos)              │
│  ●●●●●● Witnessed (6)                        │
│  ◐◐◐◐◐◐◐◐ Unobservable (8)                  │
│  ○ Unwitnessed (1)                           │
│                                               │
│  Merge Ratio Chart                            │
│  100%│                                        │
│   50%│   ○○○                                  │
│   10%│───┼─┼──── threshold ─────────         │
│    0%│     ○○○○○○○○                          │
│       click django flask rich ...             │
│                                               │
│  Witness History (selected repo)             │
│  commit │ author │ kind │ weight │ verdict   │
│  c1a2b3 │ alice  │ Merge │ 1.0   │ Approve   │
│  d4e5f6 │ bob    │ PR    │ 0.8   │ Approve   │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- Tri-state pie/bar chart
- Merge ratio scatter plot (10% threshold line)
- Per-repo witness history table
- Evidence dedup visualization (inv #2)

**Backend:** `get_witness_status`

---

### 3.6 Rule Manager (Öncelik: Orta)

Q6 kurallarını kaydet/aç/kapat. Violation log.

```
┌─────────────────────────────────────────────┐
│ Rule Manager                      [+ Add]    │
│                                               │
│  Active Rules                                 │
│  ✅ structural.no_self_import    [Hard] [⚙]  │
│  ✅ structural.no_duplicate_node  [Hard] [⚙]  │
│  ✅ structural.edge_target_exists [Hard] [⚙]  │
│  ⬜ arch.layer_separation         [Soft] [⚙]  │
│                                               │
│  Recent Violations                            │
│  Claim #107 │ no_self_import │ node 10→10    │
│  Claim #103 │ duplicate_node │ node 5 exists  │
│  Claim #098 │ edge_target    │ node 99 missing│
│                                               │
│  [+ Create Custom Rule]                      │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- Rule list (enable/disable toggle, severity badge)
- Default rules (NoSelfImport, DuplicateNode, EdgeTargetExists)
- Custom rule creation (TOML or visual builder)
- Violation log (recent rejections by Q6)
- Rule edit (parameters, thresholds)

**Backend:** `register_rule`, custom rule serialization

---

### 3.7 Permission Manager (Öncelik: Orta)

God Mode PermissionMask konfigürasyonu. Read-only node'lar, writable axes,
forbidden edge kinds.

```
┌─────────────────────────────────────────────┐
│ Permission Manager (God Mode)                │
│                                               │
│  Agent: #42 (auth-service)                    │
│                                               │
│  Read-Only Nodes                              │
│  [×] node 1 (core/config)                    │
│  [×] node 7 (database/schema)                │
│  [+ Add node to read-only]                   │
│                                               │
│  Writable Axes                                │
│  [✓] coupling  [✓] cohesion  [ ] instability │
│                                               │
│  Forbidden Edge Kinds                        │
│  [✓] Approves (witness-only)                 │
│  [ ] Violates                                 │
│                                               │
│  Max Position Deviation: θ_max = 0.30        │
│  ━━━━●━━━━━━━━━━                             │
│                                               │
│  [Apply]  [Reset to Full Access]             │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- Per-agent PermissionMask configuration
- Read-only node picker (search/select from Space)
- Writable axis checkboxes
- Forbidden edge kind checkboxes
- θ_max deviation slider
- Three-point defense visualization (slice → shell → commit)

**Backend:** `PermissionMask` struct (already defined, inv #13)

---

### 3.8 SCIP Index Manager (Öncelik: Orta)

SCIP indeks üretme, coverage görselleştirme, stale detection.

```
┌─────────────────────────────────────────────┐
│ SCIP Index Manager                            │
│                                               │
│  Repository: django                           │
│  SCIP Index: ✓ loaded (index.scip, 2.3 MB)   │
│                                               │
│  Coverage                                     │
│  ████████████████████░░  98.4%               │
│  Files: 2920 / 2966 indexed                  │
│  Classes: 10,054 detected                    │
│  Field accesses: 8,231                       │
│                                               │
│  Cohesion Distribution                        │
│  1.0│  ████████                              │
│  0.7│  ██████████                            │
│  0.5│  █████                                 │
│  0.3│  ██                                    │
│     └────────────                            │
│                                               │
│  [Regenerate Index]  [View Unindexed Files]  │
│  Status: ● Fresh (commit matches)            │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- Index load/generate (Docker scip-python / npm scip-typescript)
- Coverage bar (files_with_scip / files_total)
- Cohesion histogram (LCOM4 distribution per class)
- Stale indicator (index_commit ≠ repo_head)
- Unindexed file list (debug why coverage < 100%)

**Backend:** `AnalysisResult.semantic_coverage`, SCIP commands

---

### 3.9 Time Travel (Öncelik: Düşük)

Event-sourcing replay. t_c slider ile geçmiş space durumlarına gitme.

```
┌─────────────────────────────────────────────┐
│ Time Travel — Event Sourcing Replay           │
│                                               │
│  t_c: 0 ─━●━━━━━━━━━━━━━━━━━━━ 347          │
│       (milestone #1)      (current)           │
│                                               │
│  t_c = 42                                     │
│  Claim: #42 (auth module refactor)            │
│  Author: alice  Witnesses: 2 (bob, carol)    │
│  Δ: +5 nodes, +12 edges, -3 nodes            │
│                                               │
│  Space at t_c=42:                            │
│  Nodes: 156  Edges: 234                      │
│  [View Space Topology at this t_c]           │
│                                               │
│  ◀ Prev (t_c=41)    Next (t_c=43) ▶          │
│  [Restore to this point]                     │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- t_c slider (milestones marked)
- Per-commit delta summary (nodes/edges added/removed)
- Restore to any t_c (milestone + delta replay)
- Space topology snapshot at selected t_c
- Claim history timeline

**Backend:** `time_travel` (SnapshotStore::restore)

---

### 3.10 Calibration Alerts (Öncelik: Orta)

"Agent A'nın claim'i vizyondan %15 saptı" gibi uyarılar. θ dağılımı.

```
┌─────────────────────────────────────────────┐
│ Calibration Alerts                            │
│                                               │
│  ⚠ Active Alerts (3)                         │
│                                               │
│  🔴 Agent #42, Claim #107                    │
│     θ = 0.31 > θ_bound = 0.25                │
│     Vision hallucination (coupling axis)     │
│     [View Claim Diff] [Send Calibration]     │
│                                               │
│  🟡 Agent #38, Claim #103                    │
│     Rule violation: no_self_import            │
│     Rule hallucination                        │
│     [View Pipeline] [Send Calibration]       │
│                                               │
│  🟢 Agent #42, Claim #098                    │
│     θ = 0.22 < θ_bound = 0.25                │
│     Passed all gates ✓                        │
│     [View Commit]                             │
│                                               │
│  θ Distribution (last 50 claims)             │
│  0.5│                                         │
│  0.3│──── θ_bound ────                       │
│  0.2│  ○○○○○○○○○○                           │
│  0.0│○○○○                                    │
│     └────────────                            │
└─────────────────────────────────────────────┘
```

**Özellikler:**
- Active alert list (sorted by severity: red > yellow > green)
- θ distribution chart (last N claims, θ_bound line)
- Per-alert actions: view diff, view pipeline, send calibration
- Historical trend (is the agent drifting over time?)

**Backend:** EngineCommitError history, θ tracking

---

## 4. Önceliklendirme

### MVP (Faz 7 — ilk Tauri release)

| Panel | Neden | Efor |
|---|---|---|
| **Space Topology** | "Conceptual space" tezinin görsel kanıtı — en yüksek etki | M (D3.js mevcut, Tauri entegrasyon) |
| **Vision Editor** | God Mode'un temel aracı — vision ayarlayamadan hiçbir şey çalışmaz | S |
| **Witness Dashboard** | Tri-state classification görsel — paper'ın RQ1 kanıtı | S |

**MVP hedef:** Kullanıcı bir repo açar → space topology görür → vision ayarlar → witness status görür. Tek binary, `cargo install osp-desktop`.

### Phase 2 (Faz 7.5)

| Panel | Neden |
|---|---|
| **Claim Diff Viewer** | Agent etkileşiminin görsel kanıtı |
| **Commit Pipeline** | Q4-Q6 gate akışının görsel kanıtı |
| **Calibration Alerts** | Hallucination detection görsel |

### Phase 3 (Faz 8 — tam dashboard)

| Panel | Neden |
|---|---|
| **Rule Manager** | Q6 kural yönetimi |
| **Permission Manager** | inv #13 God Mode yetki yönetimi |
| **SCIP Index Manager** | SCIP workflow entegrasyonu |
| **Time Travel** | Event-sourcing görsel kanıtı |

---

## 5. Faz Planı

```
Faz 5 (mevcut): LLM codec + compute_space_slice + position computation
  ↓
Faz 7: osp-desktop Tauri iskelet
  - crates/osp-desktop/ setup (Tauri v2 + Svelte + Vite)
  - 3 Tauri command (analyze_repo, get_space, get_vision_config)
  - MVP paneller: Space Topology + Vision Editor + Witness Dashboard
  - Space Browser'dan D3.js migration
  ↓
Faz 7.5: Agent interaction panels
  - Claim Diff Viewer + Commit Pipeline + Calibration Alerts
  - compute_raw_from_delta + commit_claim Tauri commands
  - Hallucination classification UI
  ↓
Faz 8: Full dashboard
  - Rule Manager + Permission Manager + SCIP Manager + Time Travel
  - register_rule, PermissionMask config, SnapshotStore replay
  - 3D space topology (Three.js)
  ↓
Faz 8+: Polish
  - Multi-repo comparison
  - Export reports (PDF/HTML)
  - Plugin system (custom panels)
  - Internationalization (TR/EN)
```

---

## 6. Tasarım Prensipleri

1. **Local-first:** Tüm hesaplama yerel. Cloud bağımlılık yok. Gizlilik tasarım
   gereği — kod hiçbir sunucuya gönderilmez.

2. **God Mode sovereignty:** İnsan operatör her zaman son kararı verir. UI,
   Agent'ın önerilerini gösterir ama asla otomatik commit yapmaz (accept/reject
   butonu her zaman insan tıklaması gerektirir).

3. **Epistemological transparency:** Her metrik provenance taşır (MetricValue:
   source/confidence/coverage). UI, "0.5 çünkü ölçtük" ile "0.5 çünkü bilmiyoruz"
   arasındaki farkı görsel olarak gösterir (placeholder `*` işareti, confidence bar).

4. **Progressive disclosure:** Yeni kullanıcı sadece Space Topology görür.
   İleri kullanıcı Vision Editor + Rule Manager açar. Expert kullanıcı Time Travel
   + Permission Manager kullanır. Panel'ler sidebar'da kategoriye göre gruplanır.

5. **Dark theme default:** Yazılım geliştiriciler için dark theme standart.
   GitHub tarzı (#0d1117 background, #58a6ff accent).

---

## 7. Referanslar

- `viz/space-browser.html` — mevcut D3.js görselleştirme (Faz 3.6, MVP taban)
- `docs/agent-prompt-semantics.md` — OspPrompt, PermissionMask, compute_space_slice
- `docs/OSP-formalism.md` — coordinate system, BFT proof, commit operator
- `docs/implementation-invariants.md` — 15 invariant (UI bunları görselleştirir)
- `docs/multi-agent-coordination.md` — Faz 6 Shared Horizon (UI'da pool görünümü)
- Tauri docs: https://v2.tauri.app/
- Svelte docs: https://svelte.dev/

---

*Bu doküman OSP desktop uygulamasının UI mimarisini tanımlar. Tauri + Svelte
seçimi onaylanmıştır. MVP (Faz 7) 3 panel ile başlar, Faz 8'de tam dashboard'a
uzanır.*

*Sürüm: 0.1-draft · 2026-06-23 · Status: Design Spec (Faz 7-8 target)*
