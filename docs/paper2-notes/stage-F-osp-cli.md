# Stage F1 — osp-cli (CLI-first execution surface) Notes (Paper 2 evidence)

> **Aşama:** F1 (osp-cli iskeleti + MockLlmClient) — TAMAMLANDI
> **Tarih:** 2026-06-30
> **Tez:** "CLI = truth surface. UI/MCP/SDK ne yaparsa yapsın, en altta CLI/osp-core aynı
> sonucu üretmeli. osp-cli, D2 navigator'ını kullanım yüzeyine çıkarır."
> **Kapsam:** osp-cli crate + analyze/trajectory attempt/evidence komutları. MockLlmClient.

---

## Karar 1: osp-cli crate (CLI-first, execution surface)

**Karar:** Yeni `crates/osp-cli/` crate. clap derive subcommands: analyze, trajectory
(init/attempt), task view, evidence export. CLI çalıştıran insan = operator (INV-T2).
**Gerekçe:** Arkadaş yorumu — CLI first, MCP second. CLI = truth surface; agent
decomposition yapamaz, hedef koordinat göremez (INV-T1).
**Kanıt:** `osp analyze P:/repos/osp-spike/svelte` → 3448 node, 4217 edge, A=0.884.
`osp trajectory attempt` → D2 navigator gerçek measure.
**Paper materyali:** §3 Adaptive control loop — "CLI as execution surface."

---

## Karar 2: osp-cli sarar, osp-analyze kalır (backward compat)

**Karar:** osp-analyzer crate'inde osp-analyze binary korunur. osp-cli `osp analyze`
onun pipeline'ını reuse eder (analyze_repo_with_config).
**Gerekçe:** Backward compat — mevcut osp-analyze kullanıcıları bozulmaz.
**Kanıt:** osp-cli `osp analyze` çalışır, osp-analyze hâlâ mevcut.

---

## Karar 3: D2 navigator gerçek measure (svelte corpus validation)

**Karar:** `osp trajectory attempt` D2 navigator'ını kullanır — gerçek analyze_repo →
space → SpaceEngine → compute_raw_from_delta. MockLlmClient scripted proposals.
**Gerekçe:** D1 mock 0 coupling çözüldü (D2). CLI navigator'ı gerçek measure ile çalışır.
**Kanıt:** svelte corpus'ta `osp trajectory attempt` → evidence ledger dolu:
- after position gerçek (x=0.0, z=0.5, w=0.46...) — mock 0 DEĞİL
- predicate_completion: NotCompleted, mutation_decision: Reject (coupling ≤ 0.55 sağlanmadı)
- D2 commit_task_claim çalıştı

---

## Karar 4: MockLlmClient (FileMockLlm JSON adapter)

**Karar:** `osp trajectory attempt --proposals file.json` MockLlmClient ile.
FileMockLlm JSON'dan scripted DeltaProposal yükler. D3'te RuntimeLlmClient ile değiştirilebilir.
**Gerekçe:** F1'de gerçek LLM API key gerekmez (decoupling). Test deterministic.
**Kanıt:** test-proposals.json → FileMockLlm → navigator loop çalıştı.

---

## Komutlar (F1)

```bash
osp analyze <repo> [--scip <index>] [--out space.json]  # analyze_repo_with_config reuse
osp trajectory init --repo <repo> [--vision <toml>]     # SpaceEngine kur
osp trajectory attempt <task-id> --repo <repo>          # D2 navigator, MockLlmClient
  --proposals <delta.json> --maneuver-limit 5
osp task view <task-id> --repo <repo> --predicate "..."  # AgentTaskView (INV-T1)
osp evidence export [--input <file>] [--out evidence.json]
```

---

## Svelte corpus doğrulama (F1)

```
osp analyze P:/repos/osp-spike/svelte:
  node_count: 3448, edge_count: 4217, A: 0.884

osp trajectory attempt 1 --repo svelte --proposals test.json --maneuver-limit 2:
  Evidence ledger dolu — gerçek measure (after x=0.0, z=0.5, w=0.46)
  predicate_completion: NotCompleted, mutation_decision: Reject
  D2 commit_task_claim çalıştı
```

---

## Aşama F1'de YAPILMAYAN (F2/D3+)

- **Gerçek LLM adapter** (RuntimeLlmClient) — D3
- **OspPrompt genişletme** (AgentTaskView → OspPrompt) — D3
- **osp-mcp** — Aşama G
- **Persistent task registry** (in-memory) — E
- **Task add/view tam implementasyon** (D2 navigator integration) — F2

---

## Doğrulama (F1)

osp-cli workspace build, -D warnings temiz, fmt temiz. svelte corpus integration:
analyze + trajectory attempt gerçek measure ile çalıştı.

---

*Bu not `docs/paper2-notes/README.md` disiplinine uyar: karar + gerekçe + kanıt + edge case + paper materyali.*
