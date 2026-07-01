# OSP — Ontological Space Protocol

[![CI](https://github.com/ervolkan/osp/actions/workflows/ci.yml/badge.svg)](https://github.com/ervolkan/osp/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

> Software projects as navigable **conceptual spaces** with physics-like rules and
> BFT-inspired witnessing. OSP positions every module in a multi-dimensional coordinate
> system (coupling, cohesion, instability, entropy, witness-depth) and constrains AI-agent
> commits through deterministic validity gates before mutation.

**Paper:** v2.6 (arXiv then ACM TOSEM target) — 5 research questions, 23-repo corpus across 5 languages
(Python, TypeScript, JavaScript, Rust, Go), 18,952 real LCOM4 classes analyzed via SCIP. See
[`docs/paper-draft-v2.6.md`](docs/paper-draft-v2.6.md).

**Vision source:** [`SoftwarePhysics.txt`](SoftwarePhysics.txt) · **Paper:** [`docs/paper-draft-v2.6.md`](docs/paper-draft-v2.6.md)

---

## Quick Start

### Prerequisites

- **Rust** 1.75+ ([rustup.rs](https://rustup.rs))
- **Git** 2.40+
- **Docker** (optional — for Python/Rust/Go SCIP indices via `scip-python`/`scip-rust`/`scip-go`)
- **Node.js** 16+ (optional — for TypeScript/JavaScript SCIP indices via `scip-typescript`)

### Build & Test

```bash
git clone https://github.com/ervolkan/osp.git
cd osp
cargo build --workspace --exclude osp-desktop
cargo test --workspace --exclude osp-desktop   # ~490+ tests across 7 crates
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

### Two-Paper Strategy

- **Paper 1 (Static Space)** — ✅ done. SCIP + tree-sitter + 5-axis + vision + witness +
  tri-state. 23-repo corpus, 18,952 LCOM4 classes. [`docs/paper-draft-v2.6.md`](docs/paper-draft-v2.6.md)
- **Paper 2 (Dynamic / Agent Trajectory)** — in progress. Task = measurement predicate,
  PredicateGate, navigator loop, calibration feedback. CLI + MCP truth surfaces done.
  Paper writing **deferred to the end** (data-driven). [`docs/agent-trajectory-roadmap.md`](docs/agent-trajectory-roadmap.md)

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

See [`docs/scip-cohesion-results.md`](docs/scip-cohesion-results.md) (primary corpus) and
[`docs/corpus28-results.md`](docs/corpus28-results.md) (extended Rust/Go) for the full datasets.

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
| **8** | OSP Desktop UI | ✅ v0.3.4 (6 panels + role-aware vision + Node Inspector + Confidence) |
| **9** | Custom Axis Marketplace | ⏸️ Planned |
| **P2** | **Paper 2 — Agent Trajectory Navigation** | 🔶 Core + CLI + MCP done; SDK + paper writing pending |

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

Full formalism: [`docs/OSP-formalism.md`](docs/OSP-formalism.md)

---

## Documentation

**Paper 1 (Static Space):**

| Document | Content |
|---|---|
| [`docs/paper-draft-v2.6.md`](docs/paper-draft-v2.6.md) | Paper v2.6 (5 RQs, 23-repo/5-lang corpus, real LCOM4 data, token benchmark) |
| [`docs/OSP-formalism.md`](docs/OSP-formalism.md) | Mathematical model (coordinate system, BFT proof, commit operator) |
| [`docs/scip-cohesion-results.md`](docs/scip-cohesion-results.md) | Primary 15-repo corpus LCOM4 cohesion results |
| [`docs/corpus28-results.md`](docs/corpus28-results.md) | Extended 23-repo results (Rust/Go cohesion + coupling + foam analysis) |
| [`docs/calibration-corpus.md`](docs/calibration-corpus.md) | Corpus selection methodology |
| [`docs/literature-scan.md`](docs/literature-scan.md) | Related work + originality analysis |

**Paper 2 (Dynamic / Agent Trajectory):**

| Document | Content |
|---|---|
| [`docs/STATUS.md`](docs/STATUS.md) | ⭐ Project status summary — stage table, what's done, what's next |
| [`docs/agent-trajectory-roadmap.md`](docs/agent-trajectory-roadmap.md) | Roadmap (motivation, ontology, INV-T1..T8, §8 stage plan) |
| [`docs/invariant-spec.md`](docs/invariant-spec.md) | Formal invariant spec (INV-T1..T8 trajectory + INV #1..#15 space) |
| [`docs/mcp-design.md`](docs/mcp-design.md) | MCP server design (6 "never" principles, tool categories, INV matrix) |
| [`docs/paper2-notes/`](docs/paper2-notes/) | Evidence notes per stage (A→G1) — data-driven paper writing source |

*Internal design specs (core/engine/analyzer design, session notes, dogfooding logs)
are maintained privately during development.*

---

## License

Apache-2.0. See [`LICENSE`](LICENSE).

## Contributing

This project follows a phased development model (see roadmap). Each phase has measurable
Go/No-Go gates. Design decisions are documented in `docs/`.
