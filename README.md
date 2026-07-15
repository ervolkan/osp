# OSP — Ontological Space Protocol

[![CI](https://github.com/ontologicalspace/osp/actions/workflows/ci.yml/badge.svg)](https://github.com/ontologicalspace/osp/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

> Software projects as navigable **conceptual spaces** with physics-like rules and
> BFT-inspired witnessing. OSP positions every module in a multi-dimensional coordinate
> system (coupling, cohesion, instability, entropy, witness-depth) and constrains AI-agent
> commits through deterministic validity gates before mutation.

**Paper:** v2.6 (arXiv then ACM TOSEM target) — 5 research questions, 23-repo corpus across 5 languages
(Python, TypeScript, JavaScript, Rust, Go), 18,952 real LCOM4 classes analyzed via SCIP. See
[`docs/papers/paper1-static-space.md`](docs/papers/paper1-static-space.md).

**Vision source:** [`SoftwarePhysics.txt`](SoftwarePhysics.txt) · **Paper:** [`docs/papers/paper1-static-space.md`](docs/papers/paper1-static-space.md)

---

## Quick Start

### Prerequisites

- **Rust** 1.75+ ([rustup.rs](https://rustup.rs))
- **Git** 2.40+
- **Docker** (optional — for Python/Rust/Go SCIP indices via `scip-python`/`scip-rust`/`scip-go`)
- **Node.js** 16+ (optional — for TypeScript/JavaScript SCIP indices via `scip-typescript`)

### Build & Test

```bash
git clone https://github.com/ontologicalspace/osp.git
cd osp
cargo build --workspace --exclude osp-desktop
cargo test --workspace --exclude osp-desktop   # 1153+ tests across 7 crates
```

> `osp-desktop` is excluded from CI/headless builds (Tauri needs webkit2gtk/glib-sys on Linux).
> Build it locally with `cargo build -p osp-desktop` if the Tauri prerequisites are installed.

### Analyze a Repository

```bash
# Tier 1 only (tree-sitter: coupling, abstractness, instability)
cargo run -p osp-cli -- analyze --repo /path/to/repo

# Tier 1 + Tier 2 (SCIP semantic: real LCOM4 cohesion)
cargo run -p osp-cli -- analyze --repo /path/to/repo --scip /path/to/index.scip
```

### Run the MCP Server (Claude / Cursor)

```bash
# Build the MCP server binary
cargo build -p osp-mcp --release

# Run it over stdio (agent mode — operator tools disabled by default)
./target/release/osp-mcp --workspace /path/to/repo
```

Add to your MCP client config (Claude Desktop / Cursor):
```json
{
  "mcpServers": {
    "osp": {
      "command": "/absolute/path/to/osp-mcp",
      "args": ["--workspace", "/path/to/your/repo"]
    }
  }
}
```

See [`crates/osp-mcp/README.md`](crates/osp-mcp/README.md) for the 4 agent-facing tools
and the INV-T1 guarantee (target coordinates never leak to agents).

**Output example:**
```
repo              nodes edges      κ      A      I      D      y
----------------------------------------------------------------------
django            2966  4659   1.57   0.00   0.66   0.18   0.66
```

- `κ` = coupling density (edges/nodes), `A` = abstractness, `I` = instability (Martin)
- `D` = main-sequence distance |A+I−1|, `y` = LCOM4 cohesion (real if SCIP, `y*` = placeholder)

---

## Generating SCIP Indices

### Python (via Docker)

```bash
docker run --rm -v /path/to/repo:/repo -w /repo \
  sourcegraph/scip-python:latest \
  /usr/local/bin/scip-python index . --output index.scip \
  --project-name myproject --project-version 1.0.0
```

### Rust (via Docker, rust-analyzer)

```bash
docker run --rm -v /path/to/repo:/repo -w /repo \
  sourcegraph/scip-rust:latest \
  rust-analyzer scip . --output /repo/index.scip
```

### Go (via Docker)

```bash
docker run --rm -v /path/to/repo:/repo -w /repo \
  sourcegraph/scip-go:latest \
  scip-go --output /repo/index.scip
```

### TypeScript / JavaScript (via npm)

```bash
npm install -g @sourcegraph/scip-typescript
cd /path/to/repo
scip-typescript index --output index.scip --infer-tsconfig
```

---

## Workspace Structure

```
osp/
├── crates/
│   ├── osp-core/          # Formal model: coords, axes, witness, vision, engine, persistence
│   │   │                   + trajectory (Task=predicate, PredicateGate, navigator)
│   │   ├── agent.rs       # PermissionMask, DeltaProposal, OutputContract, HallucinationType
│   │   ├── rule.rs        # Rule trait, RuleViolation, Q6 rule set
│   │   ├── trajectory.rs  # Paper 2: Trajectory/Milestone/Task/MetricPredicate, INV-T1..T8
│   │   └── navigator.rs   # Paper 2: AgentNavigator.run_task, LlmClient trait
│   ├── osp-analyzer/      # Two-tier analysis: tree-sitter (5 langs) + SCIP LCOM4
│   │   ├── adapters/      # Python, TypeScript, JavaScript, Rust, Go
│   │   ├── scip/          # SCIP loader (impl#[Type] fix), LCOM4 algorithm, SemanticIndex
│   │   └── examples/      # scip_dump, scip_semantic_dump, timing_bench
│   ├── osp-llm-runtime/   # Stateless OpenAI-compatible runtime: OspPrompt → DeltaProposal
│   │   │                   + RuntimeLlmClient (D3 trajectory navigator LLM)
│   ├── osp-cli/           # CLI truth surface: analyze, trajectory init/attempt, task view
│   ├── osp-mcp/           # MCP server (rmcp 0.8): AI agent access surface, INV-T1..T8 enforced
│   ├── osp-desktop/       # Tauri + Babylon.js: 5-panel UI, 3D viewer (Aşama 1-3)
│   └── osp-spike/         # Faz 0 frozen reference (tri-state witness validation)
├── docs/                  # Paper v2.6 + Paper 2 roadmap + invariant spec + MCP design
├── scripts/               # Reproducibility scripts (corpus clone + SCIP + analyze)
├── viz/                   # Paper figures (commit pipeline, space topology, graveyard)
├── Cargo.toml             # Workspace root (7 crates)
└── SoftwarePhysics.txt    # Vision source (immutable)
```

### Three-Paper Strategy

- **Paper 1 (Static Space)** — ✅ done. SCIP + tree-sitter + 5-axis + vision + witness +
  tri-state. 23-repo corpus, 18,952 LCOM4 classes. [`docs/papers/paper1-static-space.md`](docs/papers/paper1-static-space.md)
- **Paper 2 (Dynamic / Agent Trajectory)** — ✍️ draft. Task = measurement predicate,
  PredicateGate, navigator loop, calibration feedback. CLI + MCP truth surfaces done.
  Draft v1.2 written (arXiv candidate after review). [`docs/papers/paper2-agent-trajectory.md`](docs/papers/paper2-agent-trajectory.md)
- **Paper 3 (Genesis Layer / Concept Anchoring)** — ✅ v1.4 manuscript on Zenodo
  (version DOI `10.5281/zenodo.21376820`; concept DOI `10.5281/zenodo.21220992`;
  arXiv yükleme pending). Type-enforced binding chain: candidate isolation → operator
  acceptance → predicate lowering → cross-family translation → operator binding →
  capability-gated task genesis. 16 core binding-chain invariants (13 type-enforced +
  3 runtime-asserted) + Evidence-Identity (EI1–EI8) + Derived-Projection (RP1–RP4)
  parallel families. [`docs/papers/paper3-concept-anchoring.md`](docs/papers/paper3-concept-anchoring.md) · [evidence pack](docs/paper3-notes/evidence-pack/)

---

## Reproducing Paper Results

The 23-repo corpus analysis (RQ1–RQ5) can be reproduced. The primary 15-repo Python/TS/JS corpus:

```bash
# Clone corpus, generate SCIP indices, run analysis, collect results
bash scripts/reproduce-corpus.sh
```

The extended 8-repo Rust/Go corpus (RQ4 cohesion + coupling/instability):

```bash
# Clone Rust/Go repos
powershell -File scripts/clone-corpus.ps1
# Generate SCIP indices (Rust via scip-rust Docker, Go via scip-go Docker) then:
powershell -File scripts/run-corpus.ps1
```

See [`docs/results/scip-cohesion.md`](docs/results/scip-cohesion.md) (primary corpus) and
[`docs/results/corpus28.md`](docs/results/corpus28.md) (extended Rust/Go) for the full datasets.

### Token-Size Benchmark (RQ5)

```bash
# Real GPT-4o-mini token counts across 9 repositories
cargo run -p osp-llm-runtime --example multi_repo_bench
# Raw results: docs/usage-llm-benchmark-multi.json
```

### Timing Benchmark

```bash
cargo run --release --example timing_bench -- /path/to/repo 5
# Outputs: median, min, max, per-run times
```

---

## Phase Status

| Phase | Description | Status |
|---|---|---|
| **0** | Spike validation (squash blind-spot) | ✅ Done |
| **1** | Core formalism + 15 invariants | ✅ Done |
| **2** | Space engine (Q4-Q6 gates, event-sourcing) | ✅ Done |
| **3** | Analyzer (tree-sitter + SCIP LCOM4) | ✅ Done (18,952 classes, 5 langs) |
| **4** | Scale / KùzuDB | ⏸️ Deferred (50k+ nodes) |
| **5** | Agent/LLM OSP Codec | 🔶 Stub types + validate gates + stateless runtime |
| **6** | Multi-Agent Coordination | 📄 Proposal |
| **7** | Academic Paper 1 (Static Space) | ✅ v2.6 (arXiv target) |
| **8** | OSP Desktop UI | ✅ Evaluation snapshot 0.3.4; packaged desktop 0.1.0 (6 panels + role-aware vision + Node Inspector + Confidence) |
| **9** | Custom Axis Marketplace | ⏸️ Planned |
| **P2** | **Paper 2 — Agent Trajectory Navigation** | 🔶 Core + CLI + MCP done; SDK + paper writing pending |
| **P3** | **Paper 3 — Genesis Layer (Concept Anchoring)** | ✅ v1.4 on Zenodo (`21376820`); concept DOI `21220992`; arXiv pending |

**Paper 2 status (A→G1 done):** ontoloji → predicate gate → planner → navigator → gerçek
measure → gerçek LLM → calibration → CLI → MCP (INV-T1 canlı doğrulandı).
See [`docs/STATUS.md`](docs/STATUS.md) for the full stage-by-stage status.

---

## Key Concepts

- **Conceptual Space:** Every module has a position P = (coupling, cohesion, instability, entropy, witness-depth)
- **BFT-Inspired Witnessing:** Two independent witnesses required for commit (f=1 Byzantine safety)
- **Q4-Q6 Claim Gates:** Deterministic syntax/vision/rule checks before witness evaluation
- **MetricValue Provenance:** Every metric carries source, confidence, and coverage
- **15 Invariants:** Structurally enforced at the type level (author-witness rejection, RawPosition/DerivedPosition separation, LLM stateless, etc.)

Full formalism: [`docs/spec/formalism.md`](docs/spec/formalism.md)

---

## Documentation

**Paper 1 (Static Space):**

| Document | Content |
|---|---|
| [`docs/papers/paper1-static-space.md`](docs/papers/paper1-static-space.md) | Paper v2.6 (5 RQs, 23-repo/5-lang corpus, real LCOM4 data, token benchmark) |
| [`docs/spec/formalism.md`](docs/spec/formalism.md) | Mathematical model (coordinate system, BFT proof, commit operator) |
| [`docs/results/scip-cohesion.md`](docs/results/scip-cohesion.md) | Primary 15-repo corpus LCOM4 cohesion results |
| [`docs/results/corpus28.md`](docs/results/corpus28.md) | Extended 23-repo results (Rust/Go cohesion + coupling + foam analysis) |
| [`docs/results/calibration-corpus.md`](docs/results/calibration-corpus.md) | Corpus selection methodology |
| [`docs/results/literature-scan.md`](docs/results/literature-scan.md) | Related work + originality analysis |

**Paper 2 (Dynamic / Agent Trajectory):**

| Document | Content |
|---|---|
| [`docs/STATUS.md`](docs/STATUS.md) | ⭐ Project status summary — stage table, what's done, what's next |
| [`docs/roadmap/paper2-roadmap.md`](docs/roadmap/paper2-roadmap.md) | Roadmap (motivation, ontology, INV-T1..T8, §8 stage plan) |
| [`docs/spec/invariants.md`](docs/spec/invariants.md) | Formal invariant spec (INV-T1..T8 trajectory + INV #1..#15 space) |
| [`docs/spec/mcp-design.md`](docs/spec/mcp-design.md) | MCP server design (6 "never" principles, tool categories, INV matrix) |
| [`docs/paper2-notes/`](docs/paper2-notes/) | Evidence notes per stage (A→G1) — data-driven paper writing source |

*Internal design specs (core/engine/analyzer design, session notes, dogfooding logs)
are maintained privately during development.*

---

## License

Apache-2.0. See [`LICENSE`](LICENSE).

## Citation

If you use OSP in academic work, please cite it using the metadata in
[`CITATION.cff`](CITATION.cff). Three companion papers (Static Space, Agent
Trajectory, Concept Anchoring) and an Evidence Pack are archived on Zenodo —
see `CITATION.cff` for the DOIs.

## Contributing

This project follows a phased development model (see roadmap). Each phase has measurable
Go/No-Go gates. Design decisions are documented in `docs/`.
