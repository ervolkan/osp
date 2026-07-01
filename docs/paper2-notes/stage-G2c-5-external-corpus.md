# Stage G2c-5 — External corpus (paper-ready evidence)

> **Aşama:** G2c-5 (external corpus — paper 2 minimum gate'inin son adımı)
> **Tarih:** 2026-07-02
> **Tez:** "Gerçek LLM (GPT-4o-mini), OSP'nin structural context'ini okuyup 3 farklı
> dildeki (JS/Python/Go) external repos üzerinde geçerli coupling/instability reducing
> proposal üretebiliyor mu?"
> **Review entegrasyonu:** Handoff dokümanı (G2c5-and-beyond) plan önerisi.

## Hedef

G2c-1→4 tamamlandı: corpus runner, reject-evidence, remove_edges, policy accumulation,
gerçek LLM smoke (synthetic fixture). **G2c-5 external corpus**, paper-ready external-validity
evidence üretmek — Paper 2 minimum gate'inin son adımı: G2c ✅ + external corpus + evidence
+ failure notes → Paper 2 yazımı.

## Corpus seçimi

```
chalk   JavaScript   github.com/chalk/chalk    (14 js + 5 ts file)
click   Python       github.com/pallets/click  (63 py file)
cobra   Go           github.com/spf13/cobra    (36 go file)
```

**Neden bu 3 repo?** 3 dil çeşitliliği (external validity), küçük/temiz/symbolik,
cloneable (`scripts/clone-corpus.ps1`). chalk+click shallow-clone edildi, cobra zaten
mevcuttu.

## Implementation

### Runner extension (`crates/osp-analyzer/examples/g2c_corpus_matrix.rs`)
- **`--external` CLI flag:** external corpus loop'unu açar/kapatır (`--synthetic-only`
  kardeşi). Local crate corpus'tan ayrı (maliyet/kontrol).
- **`lang` parametresi:** `run_one_experiment` hardcoded `lang: "rust"` yerine dil
  parametresi alır (js/py/go). Analyzer auto-detect eder, `lang` yalnızca metadata etiketi.
- **External corpus loop:** 3 repo × 8 cell (2 task × 2 policy × 2 feedback) = 24 cell.
  `corpus_kind: "external-repo"`.
- **clone-corpus.ps1 güncelleme:** chalk+click eklendi (cobra dahil 10 repo).

### Analyzer değişikliği YOK
`AdapterRegistry::default_all()` extension'a göre dispatch ediyor — JS/Python/Go adapter'ları
hazır. External repo analyze local crate ile aynı kod yolu, sadece path + lang etiketi farklı.

## Sonuç — 26/26 Completed ✅ (gerçek GPT-4o-mini)

```
external corpus (3 repo × 8 cell):  24/24 Completed
synthetic RQ9 baseline:              2/2 Completed
total:                              26/26 Completed, 0 errors
```

### Headline numbers
```
RQ6 (token cost):  mean 1104 tok/cell (chalk 1035, click 1248, cobra 1030)
RQ7 (success):     24/24 Completed (100%), 23/24 first-attempt
RQ8 (feedback):    with 1061 tok vs without 1148 tok — with daha ucuz, success aynı
RQ9 (policy):      Strict 13/13, Accept 13/13 — first-attempt success'te nötr
axis_regression:   0/24 (multi-axis safety confirmed)
```

**Detaylı tablo + RQ analizi:** [`evidence/g2c-external-corpus-results.md`](evidence/g2c-external-corpus-results.md)

## Üç değerli sonuç (hepsi Paper 2 evidence)

1. **External validity kanıtlandı** → G2c-4 synthetic smoke (2/2) external corpus'a
   genellendi (24/24). Gerçek LLM 3 dilde OSP structural proposal üretiyor. ✓
2. **Multi-axis safety confirmed** → 0/24 axis regression. Coupling-reducing edge kaldırmak
   instability'yi bozmuyor (INV-protected). ✓
3. **First-attempt success bastırma etkisi** → RQ8 (feedback) ve RQ9 (policy) sinyalleri
   external corpus'ta nötr — multi-step senaryolarda (G2c-3 synthetic) değerleri kanıtlandı.
   Bu dürüst bir bulgu, Paper 2 threats'te sunulur. ✓

## Threats (paper-grade dürüstlük)

1. **Graph-level structural harness:** delta `removed_edges` (graph-level), gerçek code patch
   değil. Codegen out-of-scope → "structural harness" etiketi.
2. **First-attempt success baskınlığı:** RQ8/RQ9 sinyallerini bastırıyor. Multi-step production
   refactor senaryoları future work.
3. **Cobra düşük edge çözünürlüğü:** cobra 36 node / 1 edge. Go external package import
   resolution (github.com/...) internal Module-Module edge'e çevrilmiyor. OSP analyzer
   package-graph resolution zayıflığı — future work.
4. **Tek LLM, tek run:** GPT-4o-mini, stochasticity ölçülmedi.

## Çıktılar

- `crates/osp-analyzer/examples/g2c_corpus_matrix.rs` — `--external` flag + `lang` param +
  external corpus loop
- `scripts/clone-corpus.ps1` — chalk+click eklendi (10 repo)
- `docs/paper2-notes/evidence/g2c-external-corpus-20260702.json` — 26 cell evidence
- `docs/paper2-notes/evidence/g2c-external-corpus-results.md` — RQ6-9 + threats
- `docs/STATUS.md` — G2c-5 ✅, Paper 2 minimum gate doldu
- `docs/agent-trajectory-roadmap.md` §8 — G2c-5 ✅

## Paper 2 minimum gate — DOLDU ✅

```
G2c-1 corpus runner        ✅
G2c-1b reject-evidence     ✅
G2c-2 remove_edges         ✅
G2c-3 accumulation         ✅
G2c-4 real LLM smoke       ✅
G2c-5 external corpus      ✅   ← bu aşama
evidence JSON + results    ✅
failure notes / threats    ✅
────────────────────────────────
Paper 2 yazımına hazır.
```

**Sonraki adım:** Paper 2 yazımı (data-driven, tüm kanıt toplandı). D5 (OspPrompt
unification) ve H (osp-sdk) opsiyonel — paper'ı geciktirmez.
