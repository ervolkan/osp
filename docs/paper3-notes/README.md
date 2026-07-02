# Paper 3 Notes — Project Genesis Layer

> Paper 3: Concept Anchoring + Concept Synthesis + Operator Acceptance.
> Tasarım dokümanı: [`docs/concept-anchoring-design.md`](../concept-anchoring-design.md) (v0.2.1).

Bu dizin, Paper 3'ün implementasyon evidence'larını ve stage notlarını toplar.
Paper 2'nin `paper2-notes/` pattern'ini izler (README + stage dosyaları + evidence/).

## Fazlar

| Faz | Durum | İçerik |
|---|---|---|
| **Faz 0** | ✅ Tamamlandı | Spec + fixture — 10 golden cümle + `anchoring.fixture.v1` şema + 11 loader testi |
| **Faz 1** | ✅ Tamamlandı | In-memory deterministic MVP — 5 bileşen pipeline (Classifier, Extractor, Scorer, Gate, Store) |
| Faz 2 | Planlandı | INV-C1..C8 type-level enforcement hardening |
| Faz 3 | Planlandı | Kuzu persistence (`osp-kuzu` crate) |
| Faz 4 | Planlandı | Code evidence integration (symbol index, ImplementedBy) |
| Faz 5 | Planlandı | Task/Predicate integration → Paper 2 navigator bridge |
| Faz 6 | Planlandı | Concept Synthesis (code repo → concept hipotezleri) |
| Faz 7 | Planlandı | Embedding + LLM-assisted candidate generation |
| Faz 8 | Planlandı | Desktop integration (Project Reality Cockpit) |

## Disiplin (§11)

```
Faz 0-2'de LLM yok, embedding yok, Kuzu yok.
Saf deterministik classifier + lexical extraction + in-memory graph.
```

Bu OSP'nin inşa prensibiyle birebir (Paper 2'de D1 mock LLM önce, D3 gerçek LLM sonra).
Mekanizma önce deterministic olarak kanıtlanır, stochastic katmanlar sonra eklenir.

## Stage notları

- [`faz1-deterministic-mvp.md`](faz1-deterministic-mvp.md) — Faz 1 implementation evidence

## Evidence

- [`evidence/`](evidence/) — Faz evidence çıktıları (fixture run report'ları vb.)
