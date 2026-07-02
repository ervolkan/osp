# Faz 3a (PR30) Evidence — AnchorStore Trait + Serde Boundary

> **Tarih:** 2026-07-03
> **Durum:** ✅ PR30 — review + commit
> **Tasarım:** [`docs/concept-anchoring-design.md`](../concept-anchoring-design.md) v0.2.1, D7, §11 Faz 3

## Özet

Faz 3'ün ilk PR'ı (3-PR stratejisi): `AnchorStore` trait abstraction + serde boundary.
`osp-core`'a Kuzu bilgisi girmeden persistence altyapısı hazırlandı.

```
Faz 2: Illegal states unrepresentable.
Faz 3: Persisted states cannot bypass illegal-state boundaries.
```

## Teslim edilenler

### AnchorStore trait (D7 abstraction)
```rust
pub trait AnchorStore {
    type Error: std::error::Error + Send + Sync + 'static;
    fn seed_trusted(&mut self, seed: &GraphSeed) -> Result<(), Self::Error>;
    fn apply_plan(&mut self, plan: &AnchorPlan) -> Result<ApplyResult, Self::Error>;
    fn promote_to_accepted(&mut self, node_id: &ConceptNodeId, _cap: &OperatorAcceptance) -> Result<(), Self::Error>;
    fn find_concepts_by_canonical(&self, name: &str) -> Result<Vec<ConceptNode>, Self::Error>;
    fn mainline_query(&self) -> Result<Vec<ConceptNode>, Self::Error>;
    fn candidate_query(&self) -> Result<Vec<ConceptNode>, Self::Error>;
    fn node_count(&self) -> Result<usize, Self::Error>;
    fn edge_count(&self) -> Result<usize, Self::Error>;
}
```
- **Fallible** — backend (Kuzu IO) fail olabilir; associated `type Error`.
- **Owned `Vec<ConceptNode>`** — borrow-tied `impl Iterator` trait method olamaz.
- `InMemoryAnchorStore` impl (associated `Error = StoreError`).
- `seed` → `seed_trusted` (trusted bootstrap boundary).

### Serde boundary (INV-C3/C8 persistence koruması)
| Tip | Serialize | Deserialize | Sebep |
|---|---|---|---|
| `AnchorPlan` | ✅ | ❌ | INV-C8 — "canon gate'ten geçmiş" serde ile delinmesin |
| `AnchorCandidate` | ✅ | ❌ | INV-C8 |
| `ConceptGraph` | ❌ | ❌ | INV-C3 — private field'lar; Accepted reconstruct engel |
| **`ConceptGraphSnapshot`** (yeni) | ✅ | ✅ | Trusted restore path |
| **`PersistedAnchorPlanAudit`** (yeni) | ✅ | ✅ | DB read (apply edilemez audit) |
| `GraphSeed` | ✅ | ✅ | Fixture persist |

- `InMemoryAnchorStore::restore_trusted_snapshot(snapshot)` — trusted restore (INV-C3 boundary).
- `PersistedAnchorPlanAudit` — apply edilemez (AnchorPlan değil), sadece inspect/audit.

### StoreError
- thiserror + serde derive (Kuzu hataları persist edilebilir).
- `ConceptNodeId` Display impl (thiserror `#[error]` için).

## Compile-fail test (trybuild)
- `c8_anchorplan_deserialize.rs` — `serde_json::from_str::<AnchorPlan>` → compile error (Deserialize yok).

**INV-C8 persistence boundary garanti:** serde ile AnchorPlan reconstruct imkansız.

## Test kapsamı
```
626 yeşil (workspace, -D warnings)
├── 83 anchoring unit (+4: seed_trusted, restore_snapshot, 2 serde roundtrip)
├── 6 trybuild compile-fail (+1: AnchorPlan deserialize)
├── 11 Faz 0 fixture + 10 Faz 1 MVP (API regression)
└── Paper 1/2 regresyon yok
```

## Faz 3 → sonraki PR'lar
- **PR31**: osp-kuzu spike (`kuzu` crate API doğrulama).
- **PR32**: KuzuAnchorStore impl (generic schema + Cypher queries).

## TCB notu
`OperatorAcceptance` hala `pub(crate)` — osp-kuzu üretemez. Faz 8 AnchorService
(osp-core) token ile `promote_to_accepted` çağırır. Kuzu persist-only; authority üretmez.
