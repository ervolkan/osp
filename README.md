# OSP — Ontological Space Protocol

[![CI](https://github.com/ervolkan/osp/actions/workflows/ci.yml/badge.svg)](https://github.com/ervolkan/osp/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

> Software projects as navigable **conceptual spaces** with physics-like rules and
> BFT-inspired witnessing. OSP positions every module in a multi-dimensional coordinate
> system (coupling, cohesion, instability, entropy, witness-depth) and constrains AI-agent
> commits through deterministic validity gates before mutation.

**Paper:** v2.3 (ICSE/FSE target) — 4 research questions, 15-repo corpus, 13,031 real LCOM4
classes analyzed via SCIP. See [`docs/paper-draft.md`](docs/paper-draft.md).

**Vision source:** [`SoftwarePhysics.txt`](SoftwarePhysics.txt) · **Roadmap:** [`docs/roadmap.md`](docs/roadmap.md)

---

## Quick Start

### Prerequisites

- **Rust** 1.75+ ([rustup.rs](https://rustup.rs))
- **Git** 2.40+
- **Docker** (optional — for Python SCIP indices via `scip-python`)
- **Node.js** 16+ (optional — for TypeScript/JavaScript SCIP indices via `scip-typescript`)

### Build & Test

```bash
git clone https://github.com/ervolkan/osp.git
cd osp
cargo build --workspace
cargo test --workspace          # 275 tests
```

### Analyze a Repository

```bash
# Tier 1 only (tree-sitter: coupling, abstractness, instability)
cargo run --bin osp-analyze -- /path/to/repo

# Tier 1 + Tier 2 (SCIP semantic: real LCOM4 cohesion)
cargo run --bin osp-analyze -- --scip /path/to/index.scip /path/to/repo
```

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
│   │   ├── agent.rs       # Faz 5 stubs: PermissionMask, DeltaProposal, OutputContract
│   │   └── rule.rs        # Faz 5 stubs: Rule trait, RuleViolation
│   ├── osp-analyzer/      # Two-tier analysis: tree-sitter (5 langs) + SCIP LCOM4
│   │   ├── adapters/      # Python, TypeScript, JavaScript, Rust, Go
│   │   ├── scip/          # SCIP loader, LCOM4 algorithm, SemanticIndex
│   │   └── examples/      # scip_dump, scip_semantic_dump, timing_bench
│   └── osp-spike/         # Faz 0 frozen reference (tri-state witness validation)
├── docs/                  # 8 design docs + paper v2.3 + scip-cohesion-results
├── scripts/               # Reproducibility scripts (see below)
├── Cargo.toml             # Workspace root
└── SoftwarePhysics.txt    # Vision source (immutable)
```

---

## Reproducing Paper Results

The 15-repo corpus analysis (RQ1–RQ4) can be reproduced:

```bash
# Clone corpus, generate SCIP indices, run analysis, collect results
bash scripts/reproduce-corpus.sh
```

See [`docs/scip-cohesion-results.md`](docs/scip-cohesion-results.md) for the full dataset.

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
| **2** | Space Engine (Q4-Q6 gates, event-sourcing) | ✅ Done |
| **3** | Analyzer (tree-sitter + SCIP LCOM4) | ✅ Done (13,031 classes) |
| **4** | Scale / KùzuDB | ⏸️ Deferred (50k+ nodes) |
| **5** | Agent/LLM OSP Codec | 🔶 Stub types ready |
| **6** | Multi-Agent Coordination | 📄 Proposal |
| **7** | Academic Paper | ✅ v2.3 submit-ready |
| **8** | Custom Axis Marketplace | ⏸️ Planned |

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

| Document | Content |
|---|---|
| [`docs/OSP-formalism.md`](docs/OSP-formalism.md) | Mathematical model (coordinate system, BFT proof, commit operator) |
| [`docs/agent-prompt-semantics.md`](docs/agent-prompt-semantics.md) | Faz 5 Agent/LLM protocol spec |
| [`docs/implementation-invariants.md`](docs/implementation-invariants.md) | 15 structurally enforced invariants |
| [`docs/paper-draft.md`](docs/paper-draft.md) | ICSE/FSE paper v2.3 |
| [`docs/scip-cohesion-results.md`](docs/scip-cohesion-results.md) | 15-repo corpus LCOM4 results |
| [`docs/multi-agent-coordination.md`](docs/multi-agent-coordination.md) | Faz 6 proposal (Shared Horizons) |
| [`docs/roadmap.md`](docs/roadmap.md) | Phase roadmap + status |

---

## License

Apache-2.0. See [`LICENSE`](LICENSE).

## Contributing

This project follows a phased development model (see roadmap). Each phase has measurable
Go/No-Go gates. Design decisions are documented in `docs/`.
