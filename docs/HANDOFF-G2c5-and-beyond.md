# Handoff — G2c-5 ve Sonrası (yeni oturum için)

> **Tarih:** 2026-07-02
> **Son commit:** 20e438a (G2c-4 merge, PR #24)
> **Branch:** main (güncel)
> **Durum:** G2c-1→4 tamamlandı, G2c-5 (external corpus) sıradaki

---

## ⭐ Proje nerede?

**Paper 2 (Dinamik/Agent Trajectory Navigation) implementation'ı G2c-4'e kadar tamamlandı.**
Tek kalan implementation adımı: **G2c-5 — External corpus** (paper-ready evidence).
Sonra Paper 2 yazımına geçilebilir.

### G2c çizgisi (bu oturumda yapılanlar)
```
G2c-1  ✅ Corpus runner (harness MVP) — FeedbackSensitiveMock, NoFeedbackWrapper, 24 cell
G2c-1b ✅ Reject-evidence — navigator tüm attempt'ler evidence'a girer + gate_decision
G2c-2  ✅ remove_edges — DeltaProposal subtractive delta, OpKind::RemoveImport onurlandırılır
G2c-3  ✅ RQ9 policy accumulation — AcceptImprovement→Completed, StrictReject→LimitExceeded
G2c-3b ✅ Witness policy isolation — NavigatorWitnessPolicy (Production default, HarnessAutoApprove)
G2c-4  ✅ Gerçek LLM smoke — GPT-4o-mini 2/2 Completed, ~1160-1180 tokens (RQ6/RQ7 preliminary)
```

### Mevcut durum (STATUS.md)
```
osp-core          ✅ A→D2 + G2c-1b/2/3/4 (remove_edges, structural context, witness policy)
osp-llm-runtime   ✅ D3/D4 + G2c-4 (prompt enhancement, parse retry)
osp-cli           ✅ F1
osp-mcp           ✅ G1+G2 (operator tools + navigator loop)
osp-analyzer      ✅ Paper 1 + G2c corpus runner example
osp-spike         ✅ Paper 1

G2c-5 external corpus ⬜ SIRADAKI — Paper 2 minimum gate'in son adımı
```

---

## 🎯 G2c-5 — External corpus (sıradaki adım)

### Hedef
Küçük public repos (chalk/click/cobra gibi) ile **paper-ready evidence** üretmek.
Paper 2 minimum gate'in son adımı: G2c ✅ + external corpus + evidence + failure notes → Paper 2 yazımı.

### Mevcut altyapı (G2c-1→4 hazır)
- `crates/osp-analyzer/examples/g2c_corpus_matrix.rs` — corpus runner
  - `--llm mock|real` flag (RuntimeLlmClient GPT-4o-mini)
  - `--synthetic-only` flag (API maliyeti kontrolü)
  - synthetic fixture (5 node, coupling 0.80) — G2c-3/4'te kullanıldı
  - local crate corpus (osp-core/src, osp-cli/src, osp-analyzer/src)
  - `run_synthetic_rq9` + `run_one_experiment` (real repo path)
- Prompt: `delta_proposal_output_format_snippet` (removed_edges + affected_nodes)
- `AgentStructuralContext` (focus_node_id + current_outgoing_imports)
- Parse error → feedback retry
- `NavigatorWitnessPolicy::HarnessAutoApprove` (controlled experiment)

### G2c-5 ne yapacak
1. External repo clone (chalk/click/cobra — `scripts/clone-corpus.ps1` pattern)
2. `run_one_experiment` external repo path ile çalıştır (gerçek repo analyze)
3. Evidence JSON: `g2c-external-corpus-<date>.json`
4. Results.md: external corpus RQ6/RQ7 tablosu
5. Failure notes (stage-X-failures.md — hangi repo'lar neden fail)

### Beklenen zorluklar
- External repo'larda target node yeterli import'a sahip olmayabilir (G2c-1'deki gibi)
- Gerçek LLM external repo'da synthetic fixture kadar temiz proposal üretmeyebilir
- Bu hepsi Paper 2 threats/limitations için değerli veri

### Çalıştırma (gerçek LLM)
```bash
export OPENAI_API_KEY=$(cat docs/llm-apikey.md | tr -d '[:space:]')
cargo run --example g2c_corpus_matrix --release -- --llm real \
  --out docs/paper2-notes/evidence/g2c-external-corpus-<date>.json
```

---

## 📋 Yeni oturum için önemli notlar

### 1. Teknoloji stack
- **LLM:** GPT-4o-mini (RuntimeConfig default), `docs/llm-apikey.md`'de API key
- **MCP SDK:** rmcp 0.8 (resmi Rust MCP SDK), stdio transport
- **Witness gate:** `NavigatorWitnessPolicy` — Production (min_approvers=2) vs HarnessAutoApprove (0)
- **Prompt:** `delta_proposal_output_format_snippet()` ortak helper — removed_edges + affected_nodes

### 2. Önemli mimari kararlar (bu oturumda alınan)
- **INV-T1 (epistemological security):** AgentTaskView hedef koordinat İÇERMEZ. `structural_context` (focus_node_id + imports) SERBEST, `preferred_vector`/`target_region` YASAK.
- **DeltaProposal subtractive:** `removed_edges` + `affected_nodes` (G2c-2). OpKind::RemoveImport engine'de onurlandırılır.
- **Witness policy isolation:** navigator default Production, G2c harness HarnessAutoApprove (review 9 merge blocker çözümü).
- **Parse error retry:** terminal değil, feedback (API budget korunur, token cost kaybolmaz).

### 3. Review entegrasyon geçmişi (arkadaş değerlendirmeleri)
- **Review 4:** tarih, INV-T8, D5 prompt debt, Paper 2 gate, RQ8/9
- **Review 5:** FeedbackSensitiveMock, deterministik top-offender, INV-T4, threats
- **Review 6:** reject-evidence (gate_decision), Unknown default, helper mapping
- **Review 7:** remove_edges, affected_nodes (new_nodes'a target koyma), allowed_ops validation
- **Review 8:** synthetic controlled harness, maneuver_limit=3, feedback sabit, PassedAll
- **Review 9:** witness policy isolation (merge blocker düzeltme), RQ9 sıkılaştır
- **Review 10:** prompt helper, AgentStructuralContext, parse retry token cost, RQ7 "smoke outcome"

### 4. Bilinen debt'ler
- **D5 (OspPrompt unification):** D3 `complete_raw` shortcut hâlâ prompt debt. `AgentTaskView → OspPrompt.task_view` unify edilmeli. G2c-5 sonrası.
- **commit_task_claim AttemptOutcome gate_decision:** success path'te PassedAll set edilmiyor (navigator evidence'da PassedAll görünüyor ama engine AttemptOutcome Unknown kalabilir — G2c-1b bilinen eksik).

### 5. Test durumu
```
cargo test --workspace --exclude osp-desktop
→ 16 test grubu yeşil, -D warnings temiz
→ CI: osp-desktop exclude (Tauri Linux webkit2gtk eksik)
```

### 6. Önemli dosyalar
- `docs/STATUS.md` — proje durumu özeti (Readiness Matrix)
- `docs/agent-trajectory-roadmap.md` — §8 aşama planı, §12 karar günlüğü
- `docs/paper2-notes/` — stage-A'dan stage-G2c-4'e evidence notes
- `docs/paper2-notes/evidence/` — JSON evidence + results.md
- `crates/osp-analyzer/examples/g2c_corpus_matrix.rs` — corpus runner
- `docs/invariant-spec.md` — INV-T1..T8 + INV #1..#15
- `docs/mcp-design.md` — MCP server tasarımı

---

## 🚀 Yeni oturumda nasıl başlayalım?

### Önerilen ilk mesaj
> "G2c-5 external corpus ile devam edelim. Handoff dokümanı: docs/HANDOFF-G2c5-and-beyond.md"

### G2c-5 plan önerisi (yeni oturumda EnterPlanMode ile)
1. External repo seçimi (chalk/click/cobra — küçük, cloneable)
2. `run_one_experiment` external path ile (gerçek repo analyze)
3. `--llm real` ile external corpus smoke
4. Evidence JSON + results.md + failure notes
5. Paper 2 minimum gate kontrolü (G2c ✅ + corpus + evidence + failures)

### Sonra
- **Paper 2 yazımı** (data-driven, tüm kanıt toplandı)
- **D5** (OspPrompt unification — prompt debt giderme)
- **H** (osp-sdk) ve **E** (3D UI) opsiyonel

---

## 📊 Özet — bu oturumda ne yapıldı

**8 PR merge edildi** (G2c-1 → G2c-4 + 3b witness fix):
- PR #20: G2c-1 corpus runner
- PR #21: G2c-1b reject-evidence
- PR #22: G2c-2 remove_edges
- PR #23: G2c-3 policy accumulation + witness isolation
- PR #24: G2c-4 gerçek LLM smoke

**Paper 2 için üretilen evidence:**
- RQ9 (policy accumulation): AcceptImprovement→Completed, StrictReject→LimitExceeded
- RQ6 (token cost): ~1160-1180 tokens/Completed (gerçek GPT-4o-mini)
- RQ7 (smoke outcome): 2/2 Completed (preliminary)
- INV-T1 canlı doğrulama (structural context allowed, target coordinate forbidden)

**Arkadaş review entegrasyonu:** 7 review (4,5,6,7,8,9,10) — her birinin önerileri tamamlandı.

**OSP'nin ontolojik duruşu güçlendi:**
- OpKind::RemoveImport gerçek operasyon (etiket değil)
- DeltaProposal artık additive + subtractive
- Navigator production güven iddiası korundu (witness policy scoped)
- Gerçek LLM typed structural proposal üretebiliyor
