# Paper 3 — Handoff Notu (Faz 8b sürecinde)

> **Tarih:** 2026-07-07 (PR #48 implementasyonu sırasında güncellendi)
> **Dal:** `faz8b-superseded-status` (PR #48 branch)
> **Base:** `main` (`bee082b`, Faz 8c merged)
> **Durum:** Faz 8b başladı — PR #48 (`DecisionStatus::SupersededAccepted` + `mainline_history()` + INV-C14) implementasyonda.

---

## Nerede duruyoruz

Paper 3 (Concept Anchoring / Genesis Layer) **v1.1 public manuscript** + Faz 8a (OperatorReviewSession) +
Faz 8c (legacy promote kaldırma) tamam. **Faz 8b (supersede vocabulary)** PR #48 ile başladı:
`SupersededAccepted` varyantı, `mainline_history()` kapısı, INV-C14 (acceptance-provenance projection).

**730 test, 0 regression** (PR #48 öncesi 718 + 12 yeni test). Zenodo DOI'leri canlı (P1/P2/P3/pack).
arXiv 1 hafta ertelendi (Faz 8b tamamlansın diye).

## PR #48 — ne yapıldı (bu oturumda)

### Kod
- **`DecisionStatus::SupersededAccepted`** varyantı (sona eklendi, serde isim-bazlı).
- **Enum helper'ları** (semantik tek yerde): `is_current_mainline()` (INV-C3, Accepted only) +
  `preserves_accepted_provenance()` (INV-C14, Accepted + SupersededAccepted).
- **`mainline_history()`** trait metodu — yeni kapı, acceptance-provenance projection
  (chronological replay DEĞİL), deterministik ID sıralaması.
- **`mainline_query` + `task_bridge`** helper'a refactor (behavior-preserving).
- **`NotPromotableFrom`** açık kol (SupersededAccepted terminal).
- **`scorer.rs`** 5. kol: SupersededAccepted = 0.4 (Deprecated 0.2 < 0.4 < Candidate 0.5).
- **`status_from_str` fail-closed** — bilinmeyen token panic (eskiden sessizce Candidate'a düşüyordu).

### Testler (12 yeni)
- `decision_status_projection_matrix_matches_inv_c3_and_c14` (5×2 matrix — Model A'yı sabitler)
- `mainline_history_contains_exactly_accepted_provenance_statuses` (BTreeSet exact-set)
- `mainline_query_is_subset_of_mainline_history` (INV-C14 subset)
- `mainline_history_is_deterministically_ordered`
- `apply_decision_rejects_superseded_accepted_not_promotable` (review.rs, in-crate ctor)
- `superseded_score_is_between_deprecated_and_candidate` (exact 0.4 + aralık)
- `decision_status_superseded_accepted_serde_roundtrip`
- `pre_superseded_status_tokens_remain_compatible` (4 eski token)
- `status_from_str_parses_superseded_accepted`
- `status_from_str_rejects_unknown_token` (typo → panic, `#[should_panic(expected=...)]`)
- `status_from_str_observed_maps_to_candidate_by_design` (paper3-design.md:769 tasarım kararı)
- `superseded_accepted_cannot_seed_task_genesis` (task_bridge regresyonu)

### Dokü
- Makale (`paper3-concept-anchoring.md`): INV-C14 propagation — type-enforced sayısı **10'da kaldı**,
  C14 ayrı paragrafta (runtime-asserted). C4 satırı gelecek kipinde successor invariant.
- Roadmap (`paper3-design.md`): enum (5 varyant), lane model (mutual-exclusion cümlesi).
- `run-metadata.md`: **iki başlık** — frozen snapshot (13, commit `481690d`'nin gerçeği) +
  current protocol (14, INV-C14 sonrası envanter).

## Sıradaki PR'lar (Faz 8b devam)

### PR #49 — `SupersedeApplication` + `apply_supersede` (en büyük)
- **Üretici yol:** SupersededAccepted'ı *üreten* kod. Şu an varyant var, üretici yok.
- **Atomiklik sözü (kritik):** `apply_supersede(old, successor)` statü geçişi + successor edge'i
  **tek işlemde** kurar. INV-C15 (atomik supersede transition):
  - precondition: `old.status == Accepted`, `successor` geçerli, `old != successor`
  - postcondition: `old.status == SupersededAccepted ∧ exactly_one(old --SupersededBy--> successor)`
  - `old ∉ mainline_query ∧ old ∈ mainline_history`
- **Geçici zorlanmayan invariant (PR #48 → #49 arası):** varyant representable (public construction/
  deserialization), ama hiçbir production store-transition API üretmez. PR #49 graph-level
  successor-edge invariant'ı zorlar. **Status-set-edilip-edge-unutulan yol açılmasın.**
- **Halef kaderi (kayıt):** successor reject/deprecate edilirse superseded node'un kaderi ne?
  PR #49-50 tasarım alanı — kapıyı burada tamamen kapatıp orada duvar delme.
- **Successor cardinalite test (review PR #48 ileri not):** makale INV-C14 paragrafı successor
  invariant'ını ilk kez kardinaliteyle telaffuz ediyor — *"every SupersededAccepted node has
  **exactly one** successor"*. Bu, PR #49'un tasarım sözleşmesi oldu. #49 planlanırken şu
  senaryoların `exactly_one` kardinalitesiyle tutarlılığı açıkça test edilmeli:
  - Halef zinciri: A→B→C (A'yı B supersede, B'yi C supersede — A ve B ikisi de SupersededAccepted)
  - Tek düğümü birden fazla kararın supersede etmesi (izinli mi, değil mi? `exactly_one` önlüyor mu?)
  - Döngü/çift yönlü supersede (A→B ve B→A)

### PR #50 — `SupersedeSession` + gate ctor
- Faz 8a `OperatorReviewSession` desenine paralel. Token içeride harcanır.
- `SupersedeAuthority` capability (INV-C4 gate, zaten var) ile entegre.

### PR #51 — CLI `osp review` + desktop Cockpit
- Operator-console surface. `OperatorReviewSession` + `SupersedeSession` interactive loop.

## Model A (normatif sözleşme)

`Deprecated` ve `SupersededAccepted` **mutually exclusive terminal anlamlardır**:
- `Deprecated` = retirement *without* accepted provenance (halefsiz manuel raflama)
- `SupersededAccepted` = *retains* accepted provenance without current effectiveness (halefli replacement)

**No `Accepted → Deprecated` transition is offered.** Gelecekte eklenirse lifecycle/outcome
ayrımına geçilmeli (`DecisionOutcome + LifecycleStatus`) ve `preserves_accepted_provenance` revize edilmeli.

## 6 tur review'ün metodolojik dersi (HANDOFF'a işlendi)

> **Çok-yüzeyli sayım propagation en riskli işlem sınıfıdır.** "Bir enum varyantı ekleyelim"
> boyutundaki bir iş, dokunduğu her yüzeyi (tip, skor, sorgu semantiği, parser, invariant sayımı,
> frozen kanıt sınırı, makale dili, downstream uyumluluk) bilinçli kararlara bağlamayı gerektirir.
> Niteleyici taşımazsan "type-enforced 10" ile "runtime C14" çelişir; frozen koşu ile current
> envanter karışır. **Evidence-first disiplini:** kanıt neyi kanıtladıysa metni onu söylemeli.

Altı turda yakalananlar (sıra ile):
1. mainline_query dar kalmalı (geçmiş ayrı kapı)
2. `status_from_str` fail-open (bloklayıcı) + INV-C14 exact-set test
3. type-enforced sayısı 10'da (C14 runtime) + run-metadata frozen/current ayrımı
4. enum sona eklenmeli + deterministic sıralama + enum helper'ları merkezileştir
5. task_bridge helper kullanmalı + merge-base CI-dayanıklılık
6. task_bridge regresyon testi + `#[should_panic(expected=...)]` + run-metadata doğruluk

### Fail-closed parser'ın gizli keşfi (review takdiri)

`status_from_str`'in `_ => Candidate` catch-all'ı yalnız typo'ları değil, fixture'lardaki
`"Observed"` token'ını da yutuyormuş — davranış oradaydı ama **niyet görünmezdi**. Fail-closed
düzeltme bu bağımlılığı ortaya çıkardı ve doğru işlendi: açık `"Observed" => Candidate` kolu +
tasarım referansı (`paper3-design.md:769` — Observed bir DecisionStatus değil, MetricSource
provenance'ı) + bu kararı sabitleyen ayrı test (`status_from_str_observed_maps_to_candidate_by_design`).

**Ders:** *fail-open kod, niyeti görünmez kılar; fail-closed, gizli bağımlılıkları açığa çıkarır.*
Bu, propagation dersinin canlı kanıtı — küçük bir parser düzeltmesi bile tasarım dokümanındaki
bir kararı (Observed = ayrı lane) kodda görünür kıldı. PR #48'in plan aşamasında öngöremediğimiz
en değerli çıktı bu oldu.

## Önemli dosyalar (güncel)

| Dosya | Açıklama |
|---|---|
| `docs/papers/paper3-concept-anchoring.md` | Paper 3 v1.1 + INV-C14 (14 Paper-3 invariant) |
| `docs/paper3-notes/evidence/run-metadata.md` | İki başlık: frozen snapshot (13) + current protocol (14) |
| `crates/osp-core/src/anchoring/mod.rs` | `DecisionStatus` enum + helper'lar (`is_current_mainline`, `preserves_accepted_provenance`) |
| `crates/osp-core/src/anchoring/store.rs` | `mainline_history()` + helper-refactor + NotPromotableFrom kol |
| `crates/osp-core/src/anchoring/review.rs` | `apply_decision_rejects_superseded_accepted_not_promotable` testi |
| `crates/osp-core/src/anchoring/scorer.rs` | 5. kol (SupersededAccepted = 0.4) |
| `crates/osp-core/src/task_bridge.rs` | `is_current_mainline()` helper + regresyon testi |
| `crates/osp-core/tests/anchoring_mvp.rs` | `status_from_str` fail-closed + parser testleri |

## Kullanıcıya not

- **osp-desktop kırık** (PR #40 sonrası API drift: `compute_raw_from_delta` 4 argüman, `Claim`
  `removed_edges`+`task_id` gerektiriyor). CI zaten hariç tutuyor (Tauri webkit bağımlılıkları).
  Ayrı PR adayı — Faz 8b dışı.
- **mainline_query deterministik sıralama** — küçük PR adayı (agent-facing context tekrarlanabilirliği).
- **arXiv** 1 hafta ertelendi; Jimenez e-postası hazır (favorilerde, docs'ta değil).
- **4 DOI canlı:** P1/P2/P3/pack tüm Zenodo'da.

## Commit durumu

🚧 **PR #48 branch'te (`faz8b-superseded-status`), henüz push edilmedi.**
- main: `bee082b` (PR #47 Faz 8c merged)
- PR #48: kod + test + dokü tamam, doğrulama sonrası push.
