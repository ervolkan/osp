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
| **Faz 2** | ✅ Tamamlandı | INV-C1..C8 type-level enforcement hardening (compile-time garantiler) |
| **Faz 3a** | ✅ PR30 | AnchorStore trait + serde boundary (osp-core) |
| Faz 3b-c | ⏸️ Ertelendi | KuzuDB arşivlendi (Ekim 2025, Apple satın alma) — successor projeler olgunlaşınca |
| **Faz 4** | ✅ Tamamlandı | Code evidence integration — CodeEvidenceProvider trait + evidence-gated ImplementedBy + INV-C6 type-level |
| Faz 4.1 | Planlandı | PositionSnapshot/HasPosition graph wiring (Faz 4'ten ayrıldı) |
| **Faz 5a** | ✅ PR33a | PredicateStub bridge — TaskCandidate lane + RuleCandidate→PredicateStub + INV-P1 type-level |
| **Faz 5b** | ✅ PR33b | Navigator bridge — MetricThreshold slot binding + Accepted TaskCandidate→Task genesis + INV-P2 type-level |
| Faz 5.1 | Planlandı | Diğer 3 template (MetricDelta/Evidence/Relation) + cross-family translation maturation + OperatorCapability hardening |
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
- [`faz2-invariant-hardening.md`](faz2-invariant-hardening.md) — Faz 2 type-level enforcement evidence
- [`faz3a-anchorstore-trait.md`](faz3a-anchorstore-trait.md) — Faz 3a AnchorStore trait + serde boundary
- [`faz4-code-evidence.md`](faz4-code-evidence.md) — Faz 4 CodeEvidenceProvider + evidence-gated ImplementedBy + INV-C6
- [`faz5a-predicate-stub-bridge.md`](faz5a-predicate-stub-bridge.md) — Faz 5a PredicateStub bridge + TaskCandidate lane + INV-P1
- [`faz5b-navigator-bridge.md`](faz5b-navigator-bridge.md) — Faz 5b Navigator bridge + MetricThreshold slot binding + INV-P2

## Evidence

- [`evidence/`](evidence/) — Faz evidence çıktıları (fixture run report'ları vb.)
