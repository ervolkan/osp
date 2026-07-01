# Stage G2c-2 — DeltaProposal + remove_edges (ontolojik dürüstlük)

> **Aşama:** G2c-2 (subtractive structural delta — coupling reduction)
> **Tarih:** 2026-06-29
> **Tez:** "`OpKind::RemoveImport` LLM'e 'yapabilirsin' diyor — engine bunu onurlandırmalı.
> Coupling düşürmek edge kaldırma gerektirir. DeltaProposal additive-only → ontolojik tutarsızlık."
> **Review entegrasyonu:** Arkadaş review 7 değerlendirmesinin tüm önerileri.

## Sorun (ontolojik tutarsızlık)

G2c-1 corpus runner 0/24 Completed üretti. Explorer'lar kök nedeni ortaya çıkardı:
- **Coupling = `out_degree_value(Imports)` = `deg/(1+deg)`.** 0.82 → 0.53 için ~4-5 import'tan ~1'e = edge **kaldırma**.
- **`DeltaProposal` additive-only** — `removed_edges` YOK. `Space`'te `remove_edge` YOK.
- **`OpKind::RemoveImport` etiket ama engine karşılığı yok** — OSP'nin deterministic structural control iddiası zayıf.
- **`compute_raw_from_delta` sadece `delta_nodes` centroid'ini ölçer** — target node `delta_nodes`'ta değilse ölçülmüyor.

Bu G2c-1'in "proposal realism eksikliği" değil — **type-system gap**.

## Çözüm (Path A — ontolojik dürüstlük)

### 1. `Space::remove_edge` count döndürür (review 7 #3)
```rust
pub fn remove_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) -> usize {
    let before = self.edges.len();
    self.edges.retain(|e| !(e.from == from && e.to == to && e.kind == kind));
    before - self.edges.len()  // 0 = nonexistent edge removal (Q4/Q6 yakalar)
}
```

### 2. `EdgeRef` + `DeltaProposal +removed_edges +affected_nodes` (review 7 #1, #6)
```rust
pub struct EdgeRef { pub from: NodeId, pub to: NodeId, pub kind: EdgeKind }

pub struct DeltaProposal {
    pub new_nodes: Vec<NewNodeSpec>,
    pub new_edges: Vec<NewEdgeSpec>,
    pub removed_edges: Vec<EdgeRef>,     // G2c-2: subtractive delta
    pub affected_nodes: Vec<NodeId>,      // G2c-2: ölçüm scope (new_nodes'a target KOYMA)
    // ...
}
```
**Ontolojik ayrım (review 7 #6):** `new_nodes` = yeni varlık, `affected_nodes` = ölçülecek mevcut varlık. Target node'u `new_nodes`'a koymak ontolojik tutarsızlık.

### 3. Zincir: `DeltaProposal.removed_edges → Claim.removed_edges → Delta.removed_edges → apply_delta`
```
DeltaProposal.removed_edges
  → build_claim_from_proposal → Claim.removed_edges
  → witness::evaluate → Delta.removed_edges
  → bigbang::apply_delta → Space.remove_edge
  → engine::compute_raw_from_delta → hypothetical.remove_edge (ölçüm için)
```

### 4. `compute_raw_from_delta` kaldırma uygula + affected_nodes ölç (review 7 #7)
```rust
pub fn compute_raw_from_delta(
    &self, delta_nodes, delta_edges, delta_removed, affected_nodes
) -> RawPosition {
    // 1. hypothetical clone
    // 2. remove_edges (eklemeden ÖNCE)
    // 3. insert nodes + edges
    // 4. measure_ids = affected_nodes (boşsa delta_nodes)
    // 5. centroid over measure_ids
}
```

### 5. `allowed_operations` validation (review 7 #8 — güvenlik kritik)
```rust
if !proposal.removed_edges.is_empty()
    && !task.allowed_operations.contains(&OpKind::RemoveImport) {
    // policy violation → RejectedByRule
}
```
Agent herhangi bir task'ta edge silemez — `OpKind::RemoveImport` allowed_ops'ta olmalı.

## Testler (4 test, review 7)
- `g2c_remove_edge_returns_count_and_nonexistent_returns_zero` — count döner, 0 = nonexistent
- `g2c_removed_edges_requires_allowed_operation` — güvenlik: RemoveImport yoksa RejectedByRule
- `g2c_compute_raw_from_delta_applies_removals` — coupling 0.5 → 0 (edge remove)
- `g2c_removed_edges_serde_and_claim_round_trip` — backward-compat + Claim taşıma

osp-core: 286 unit + 4 G2c-2 = 290 test yeşil.

## Dürüst not (review 7 #10)
**G2c-2 graph-level structural harness.** Gerçek repo code patch'i değil — `compute_raw_from_delta`
hipotetik grafa edge kaldırma uygular. Paper 2'de "controlled structural harness" olarak
kullanılır. Gerçek LLM + actual code patch daha sonraki aşama (G2c-4/5).

## INV tutarlılığı
- INV-#4 (pozisyon declare etme): kaldırma structural, pozisyon YOK ✓
- INV-T3 (engine ölçer): hypothetical'ta uygular, ölçer ✓
- INV-T6 (failure≠regression): reject'te apply uygulanmaz ✓
- **Yeni**: removed_edges + allowed_ops → policy ihlali RejectedByRule

## Sonuç (G2c-2 sonrası)
- `OpKind::RemoveImport` gerçek operasyon (etiket değil) — ontolojik söz tutuldu
- Engine coupling-reducing structural proposals ölçebiliyor (compute_raw_from_delta remove uygular)
- `affected_nodes` ontolojik düz model (new_nodes overload edilmez)
- G2c-3 (incremental + policy accumulation) sağlam temel üzerinde

## Bilinen eksik (G2c-3'te)
Tam Completed pipeline test'i (predicate gate + vision + witness) bu PR'da engine-level
coupling-reduction ile sınırlı kaldı — vision gate (θ bound) ve predicate source kontrolü
full navigator loop'te ayrı ele alınmalı. G2c-3 incremental proposals + balanced vision ile
tam Completed sinyali hedeflenir.

## Çıktı
- 6 osp-core dosyası: space, agent, bigbang, witness, engine, navigator (+ persistence test)
- g2c_corpus_matrix.rs: coupling_reducing_proposal (G2c-3'te matrise entegre)
- STATUS.md/roadmap G2c-2 ✅

**Path A — ontolojik dürüstlük. OSP'nin deterministic structural control iddiası güçlendi.**
