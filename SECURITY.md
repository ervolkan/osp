# Security Policy

## Reporting a vulnerability

**Do not open a public GitHub issue for a security vulnerability.**

Please report security vulnerabilities through one of these private channels:

1. **GitHub Private Vulnerability Reporting** (preferred) —
   use the *"Report a vulnerability"* button under the **Security** tab of
   [`ontologicalspace/osp`](https://github.com/ontologicalspace/osp/security).
2. **Email fallback** — `ervolkan@gmail.com` with `[OSP SECURITY]` in the subject line.

## What to include

A good report helps us reproduce and remediate quickly. Please include:

- A description of the issue and its potential impact
- The affected crate, commit, and component
- Steps to reproduce (commands, inputs, or a minimal example)
- Any relevant logs, stack traces, or invariant identifiers
- Your suggested mitigation, if any

## Response timeline

We aim to acknowledge reports within **72 hours** and to provide an initial
assessment within **7 days**. Remediation timelines depend on severity and are
coordinated with the reporter. As a pre-1.0 research project maintained by a
solo owner, response capacity is limited; please bear with us.

## Coordinated disclosure

We prefer coordinated disclosure. Embargoed work proceeds through private
channels until remediation or an agreed disclosure window, after which a public
rationale record is published (see the organization
[`GOVERNANCE.md`](https://github.com/ontologicalspace/.github/blob/main/GOVERNANCE.md)
§6 and §8).

## Scope

This policy covers the OSP reference implementation, including:

- `osp-core` — formal model, coordinate system, witness, trajectory, invariants
- `osp-analyzer` — two-tier analysis (tree-sitter + SCIP)
- `osp-cli` — CLI truth surface
- `osp-mcp` — MCP server (agent access surface, INV-T1..T8 enforced)
- `osp-llm-runtime` — stateless LLM runtime
- `osp-desktop` — Tauri + Babylon.js visualizer
- `osp-spike` — frozen reference

Out of scope:

- Theoretical or "paper-only" claims without an implementation impact
- Issues in third-party dependencies (report upstream)

## OSP-specific note

OSP enforces safety through invariants and BFT-inspired witnessing. Bypass or
weakening of an invariant, a witness/quorum check, or an evidence-integrity
boundary is treated as a **high-risk security-sensitive change** and follows the
high-risk review path in the organization
[`GOVERNANCE.md`](https://github.com/ontologicalspace/.github/blob/main/GOVERNANCE.md)
§3.
