# Stage G2 — osp-mcp Operator Tools + Navigator Loop Notes (Paper 2 evidence)

> **Aşama:** G2 (osp-mcp operator tools + navigator loop entegrasyonu)
> **Tarih:** 2026-06-29
> **Tez:** "AI agent MCP üzerinden gerçek navigator loop'u çalıştırabilir.
> Operator tools (trajectory_init, task_add) INV-T2 runtime gate ile korunur.
> Navigator loop INV-T1 güvenli (AgentTaskView), INV-T7 (maneuver limit), INV-T8
> (progress checkpoint isolation) enforced."
> **Review entegrasyonu:** Arkadaşın değerlendirmesinin 7 noktasının tamamı bu aşamada
> işlendi (tarih, INV-T8, D5 prompt debt, Paper 2 gate, RQ8/9, Readiness Matrix).

## Mimari

### 1. Faz 0 — Doküman düzeltmeleri (review entegrasyonu)
Arkadaşın (review 4) önerilerinin tamamı roadmap/STATUS'a işlendi:
- **Tarih satırı** kronolojik düzeltildi ("Durum tarihi / Son milestone")
- **INV-T8** §6 invariant bölümüne eklendi (progress checkpoint isolation)
- **D5** aşaması eklendi (OspPrompt unification — D3 prompt debt giderme)
- **Paper 2 minimum gate** netleştirildi (G2 + corpus + evidence + failure notes zorunlu;
  H/E opsiyonel, paper'ı geciktirmez)
- **RQ8** (calibration feedback) + **RQ9** (AcceptAsProgress policy) adayları eklendi
- **Readiness Matrix** STATUS.md'ye eklendi (Layer / Status / Paper 2 readiness)

### 2. G2a — Operator tools (INV-T2 runtime gate)
- `osp_trajectory_init` (operator-only): VisionVector JSON + label → Trajectory::new
  (OperatorCapability ile). **Leak check YOK** — operator coordinate görür.
- `osp_task_add` (operator-only): Task JSON (tüm alanlar pub, serde Deserialize) → registry.
- **INV-T2 runtime gate**: `gate_operator_tool()` helper — agent mode'da
  `OperatorCapabilityRequired` error. Canlı doğrulandı (aşağıda).

### 3. G2b — Navigator loop + --llm flag
- `osp_run_task` ⭐ (agent-facing): task_id → AgentNavigator.run_task → NavigatorResult.
  Multi-attempt LLM loop'u (maneuver_limit kadar). LLM delta üretir (agent delta vermez).
- `osp_get_attempt_history` (agent-facing): navigator evidence ledger (RQ6 token cost verisi).
- **`--llm mock|real` flag**: startup'ta LLM client inject (INV-T2 pattern). Mock offline
  güvenli (CI), real OPENAI_API_KEY ile GPT-4o-mini.
- **Sync→async bridge**: blocking navigator `tokio::task::spawn_blocking` ile çalışır.
  Mutex'ler (workspace sync + registry sync) spawn_blocking içine taşınır.
- **Mevcut `osp_submit_delta` KORUNDU** (kullanıcı kararı) — agent delta'sı single-attempt.

## Core değişiklikleri (Send + Sync)
`LlmClient` trait'ine `Send + Sync` supertrait eklendi (MCP `Arc<dyn LlmClient>` + spawn_blocking).
Bu, iki mock implementasyonu Cell → AtomicUsize/Mutex değişikliğini gerektirdi:
- `osp_core::navigator::MockLlmClient`: `Cell<usize>` → `AtomicUsize`
- `osp_llm_runtime::RuntimeLlmClient`: `Cell<TokenUsage>` → `Mutex<TokenUsage>`
- `osp_cli::mock_llm::FileMockLlm`: `Cell<usize>` → `AtomicUsize`

Bu değişiklik Paper 2 için önemli: navigator artık multi-threaded async context'te
çalışabilir (MCP server, gelecekteki multi-agent koordinasyon).

## INV-T2 canlı doğrulaması ⭐
Agent mode'da operator tool çağrısı reddedildi (canlı stdio server):
```
osp-mcp --workspace /tmp/repo  (agent mode, default)
# tools/call osp_trajectory_init
{
  "ok": "false",
  "error_code": "OPERATOR_CAPABILITY_REQUIRED",
  "message": "tool 'osp_trajectory_init' requires operator mode — agent mode denied (INV-T2)",
  "invariants_checked": ["INV-T2"],
  "recoverable": true
}
# MCP protocol: "isError": true
```

Operator mode'da aynı tool başarılı:
```
osp-mcp --mode operator --workspace /tmp/repo
# tools/call osp_trajectory_init
{
  "ok": "true",
  "trajectory_id": 1,
  "label": "reduce-coupling",
  "vision": { "raw": {...}, "source": "GlobalDefault" },
  "mode": "operator"
}
```

8 tool MCP üzerinden erişilebilir (tools/list doğrulandı):
- G1: osp_analyze_workspace, osp_get_agent_task_view, osp_check_predicate, osp_submit_delta
- G2: osp_trajectory_init, osp_task_add, osp_run_task, osp_get_attempt_history

## INV-T8 (progress checkpoint isolation) doğrulama
`MutationDecision::apply_target()` mapping test ile doğrulandı:
- `AcceptAsProgress` → `Lane(TrajectoryCheckpoint)` (ASLA Mainline) ✓
- `AcceptAsCompleted` → `Lane(Mainline)` (tek Mainline yolu) ✓
- `Reject` → `NotApplied` (Sandbox DEĞİL — review v4 #3) ✓
- `RequireOperatorApproval` → `Lane(Sandbox)` (izole) ✓

Bu, navigator loop'unun "kısmi iyileştirme" ile "task bitti"yi karıştırmadığının
type-level kanıtıdır. Agent "iyileştikçe" Mainline'ı kirletemez.

## RQ etkisi (Paper 2)
- **RQ5 (epistemic projection):** G2 operator tools coordinate GÖSTERİR (operator görür),
  agent-facing navigator tools (osp_run_task) INV-T1 güvenli (AgentTaskView).
- **RQ6 (token cost):** `osp_get_attempt_history` navigator evidence ledger döner —
  her task için token/attempt/gate-decision verisi. G2c corpus runner bu üzerinden RQ6 ölçer.
- **RQ7 (task success):** `osp_run_task` NavigatorResult döner (Completed/LimitExceeded/...).
  G2c: N repo × M task ile success rate ölçülebilir.
- **RQ8 (calibration feedback):** D4 implementation + navigator loop → with-feedback vs
  no-feedback A/B G2c'de ölçülebilir (mock proposals ile deterministik test).
- **RQ9 (policy):** TaskPolicy.accept_improvement vs strict — maneuver limit altında
  AcceptAsProgress davranışı ölçülebilir.

## G2 sınırları (G2c/G3'te gelecek)
- Mock LLM default (boş proposals → navigator NoMoreProposals). Gerçek task simülasyonu
  için operator task_add + scripted proposals gerek (G2c corpus runner).
- Tek trajectory (trajectory_id: 1). Multi-trajectory G3+.
- `osp_run_task` maneuver_limit override var ama evidence store per-run accumulate
  ediyor (run'lar arası navigator current_measured güncellenmiyor — G2c'de).

## Doğrulama
- osp-mcp: 8 unit + 12 integration test = 20 test, 0 fail
- Tüm workspace: 16 test grubu yeşil (osp-core 277, osp-analyzer 148+5, osp-llm-runtime 12,
  osp-cli smoke, osp-mcp 20, osp-spike 32)
- Canlı stdio server: 8 tool listelendi, INV-T2 gate agent/operator mode'larda doğru,
  trajectory_init operator mode'da başarılı
- `cargo fmt -p osp-mcp -p osp-cli` clean
- Build: 0 error, 0 warning (osp-mcp)

## Çekirdek Send+Sync değişikliği (Paper 2 altyapı)
`LlmClient: Send + Sync` superthread eklendi. Bu, Paper 2'nin "AI agent gerçek LLM ile
navigator çalıştırır" tezinin pratik önkoşulu — MCP server (async tokio) + navigator
(sync osp-core + blocking reqwest) bridge. AtomicUsize/Mutex kullanımı idiomatik Rust
(cell interior mutability → thread-safe atomic/mutex).

## Crate yapısı (G2 sonrası)
```
crates/osp-mcp/
  Cargo.toml          — + osp-llm-runtime dep
  src/
    lib.rs            — modül deklarasyonu
    envelope.rs       — + OperatorApprovalRequired, NavigatorLlmError ErrorCode
    mode.rs           — ServerMode (Agent/Operator)
    workspace.rs      — Workspace (analyze-once, INV security)
    server.rs         — OspMcpServer (+ 4 G2 tool, gate_operator_tool, evidence_store)
    main.rs           — + --llm {mock,real} flag, build_llm_client startup
  tests/
    inv_t1_leak_test.rs — + G2 INV-T2/T8/error-code test'leri
```
