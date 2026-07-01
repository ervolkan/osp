# Stage G2c-4 — Gerçek LLM smoke (RQ6/RQ7 preliminary evidence)

> **Aşama:** G2c-4 (gerçek LLM smoke — prompt enhancement + RQ6/RQ7)
> **Tarih:** 2026-06-29
> **Tez:** "Gerçek LLM, OSP'nin verdiği yapısal bağlamı okuyup geçerli bir
> removed_edges/affected_nodes proposal üretebiliyor mu?"
> **Review entegrasyonu:** Arkadaş review 10 değerlendirmesinin 8 düzeltmesi.

## Gap analizi (explorer doğruladı, review 10)
- **Gap A:** Prompt removed_edges/affected_nodes söylemiyor → LLM additive üretir
- **Gap B:** LLM target node'un import edge'lerini göremiyor
- **Gap C:** Tek malformed JSON tüm run'u öldürüyor (terminal)

## Çözümler (review 10 tüm düzeltmeler)

### Gap A — Ortak prompt helper (review 10 #1, #2)
- `delta_proposal_output_format_snippet()` — removed_edges + affected_nodes JSON örneği
- `osp_system_prompt` + `trajectory_system_prompt` ikisi de kullanır (prompt debt önlenir)

### Gap B — `AgentStructuralContext` (review 10 #3, #4)
```rust
pub struct AgentStructuralContext {
    pub focus_node_id: NodeId,
    pub current_outgoing_imports: Vec<EdgeRef>,  // Vec<NodeId> değil — EdgeRef
}
```
- INV-T1 uyumlu: hedef koordinat DEĞİL, structural context
- navigator: target node'un outgoing Imports edge'lerini space'ten çıkarır

### Gap C — Parse error → feedback retry + token cost (review 10 #5)
- `LlmError::ProposalParse { message, token_cost: Option<TokenCost> }` — parse error da token harcadı
- navigator: ProposalParse terminal değil, feedback'e çevirir (API budget korunur)

## Sonuç — gerçek LLM smoke (GPT-4o-mini)

```
synthetic fixture × CouplingReduction × {StrictReject, AcceptImprovement}
--synthetic-only --llm real, witness_mode: harness_auto_approve, maneuver_limit: 3

synthetic/StrictReject:      Completed, attempts=1, total_tokens=1162
synthetic/AceptImprovement:  Completed, attempts=1, total_tokens=1179
```

**Her iki hücrede Completed, 1 attempt!** GPT-4o-mini prompt enhancement ile **ilk denemede**
geçerli removed_edges proposal üretti.

### Evidence detail
```
gate_decision: PassedAll
predicate_completion: Completed
mutation_decision: AcceptAsCompleted
total_tokens: 1162 (StrictReject), 1179 (AcceptImprovement)
```

## RQ etiketleme (review 10 #7)
- **RQ6 preliminary token cost:** ~1160-1180 tokens/Completed (gerçek GPT-4o-mini)
- **RQ7 real-LLM smoke outcome:** 2/2 Completed (preliminary — küçük subset, "rate" değil)

## Dürüst sınır (review 10)
G2c-4 güçlendirilmiş prompt altında gerçek LLM'in geçerli OSP structural proposal üretip
üretemediğini ölçer. **Dış corpus genellemesi veya üretim seviyesinde gerçek kod başarısı
iddiası taşımaz.** Evidence `real_llm_preliminary` etiketi.

## Üç değerli sonuç (review 10 — hepsi Paper 2 evidence)
1. **Completed** → prompt + schema çalışıyor ✓ (elde edildi)
2. Parse error → feedback retry çalışıyor (G2c-4'te tetiklenmedi — LLM temiz JSON üretti)
3. Additive/invalid → evidence sistemi failure yakalar (G2c-4'te tetiklenmedi)

## Merge şartı (review 10)
- ✅ Prompt contains removed_edges/affected_nodes (test)
- ✅ Örnek JSON geçerli DeltaProposal (test)
- ✅ INV-T1: structural context allowed, target coordinate forbidden (test)

## Testler
- `g2c4_prompt_contains_removed_edges_and_affected_nodes`
- `g2c4_prompt_example_json_parses_as_delta_proposal`
- `g2c4_structural_context_allowed_but_target_coordinate_forbidden` (osp-core)

osp-llm-runtime: 3 yeni G2c-4 test. osp-core: 1 yeni INV-T1 test. Workspace 16 grup yeşil.

## Çıktı
- `crates/osp-llm-runtime/src/prompt.rs` (`delta_proposal_output_format_snippet` — Gap A)
- `crates/osp-llm-runtime/src/adapter.rs` (trajectory_system_prompt structural context — Gap A+B)
- `crates/osp-core/src/trajectory.rs` (`AgentStructuralContext` — Gap B)
- `crates/osp-core/src/navigator.rs` (structural_context kurulumu + parse retry — Gap B+C, LlmError)
- `crates/osp-analyzer/examples/g2c_corpus_matrix.rs` (`--synthetic-only` + `--llm real` synthetic)
- `docs/paper2-notes/evidence/g2c-real-llm-smoke.json` (gerçek LLM evidence)
- STATUS.md/roadmap G2c-4 ✅

**Paper 2 RQ6/RQ7 preliminary evidence: gerçek LLM OSP structural proposal üretebiliyor.**
