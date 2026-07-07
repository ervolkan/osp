# Paper 3 — Handoff Notu (Faz 8b sürecinde — PR #49 tamam)

> **Tarih:** 2026-07-07 (PR #49 implementasyonu sırasında güncellendi)
> **Dal:** `faz8b-apply-supersede` (PR #49 branch)
> **Base:** `main` (`a392191`, PR #48 merged)
> **Durum:** Faz 8b ilerliyor — PR #48 (varyant + INV-C14) ✅ merged, PR #49 (`apply_supersede` + INV-C15) implementasyonda, PR #50 (`SupersedeSession`) sırada.

---

## Nerede duruyoruz

Paper 3 (Concept Anchoring / Genesis Layer) **v1.1 public manuscript** + Faz 8a (OperatorReviewSession) +
Faz 8c (legacy promote kaldırma) + PR #48 (varyant + INV-C14) tamam. **PR #49** Faz 8b'nin üretici
yolunu ekler: `SupersedeApplication` + `apply_supersede` (INV-C15 atomic transition). PR #50
(`SupersedeSession` + production authority issuer) sırada.

**752 test, 0 regression** (PR #48 sonrası 730 + 22 yeni test). 24 trybuild (2 opacity eklendi).
Zenodo DOI'leri canlı (P1/P2/P3/pack). arXiv ertelendi (Faz 8b tamamlansın diye).

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
- Makale (`paper3-concept-anchoring.md`): INV-C14 propagation — **genesis** type-enforced sayısı
  **10'da kaldı** (toplam type-enforced 13: 10 genesis + 3 lowering); C14 tek runtime-asserted.
  C14 ayrı paragrafta. C4 satırı gelecek kipinde successor invariant.
- Roadmap (`paper3-design.md`): enum (5 varyant), lane model (mutual-exclusion cümlesi).
- `run-metadata.md`: **iki başlık** — frozen snapshot (evidence generation commit `ef022a9`,
  baseline `481690d`) +
  current protocol (14, INV-C14 sonrası envanter).

## PR #49 — ne yapıldı (bu oturumda)

### Kod
- **`SupersedeApplication`** — opaque (private fields + `pub(crate)` ctor + no Deserialize).
  Authority parametre ister ama `Copy` → *"issuance-gated, not linearly consumed"*. Production
  issuer PR #50.
- **`PresentedSupersedeBasis`** — iki-endpoint'li basis (çift digest: superseded + successor),
  `mainline_query`'den derlenir. TOCTOU: her iki node da karar anında taze.
- **`SupersedeRecord`** + global **`audit_seq`** (decision ile paylaşımlı → cross-ledger total order).
  Ayrı `supersede_ledger`.
- **`apply_supersede`** — INV-C15 atomic transition. 12-step deterministic precedence:
  basis mismatch → NodeNotFound → stale digests → committed incoming → status → self →
  compat → cycle → audit_seq → mutation. `checked_add` overflow hardening.
- **Lane-sensitive `Supersedes`** — Candidate proposal (apply_plan) vs Accepted committed lineage
  (apply_supersede). Cardinality/cycle SADECE Accepted lane. Consolidation serbest (outgoing sınır yok).
- **Edge yönü:** `successor --Supersedes--> superseded` (tasarım doc §8.3, C4 gate semantiği).
  Inverse reading: `superseded --SupersededBy--> successor`.
- **`StoreError`** 11 yeni varyant + `SupersedeError` (compile error evreni).
- **`supersede_basis_fingerprint`** — 4 bağımsız FNV-1a lane (256-bit), length-prefixed framing.

### Testler (22 yeni, 752 total, 0 failed)
- Mutlu yol + A→B→C zincir + consolidation + projection
- Error-path matrisi (12 varyant) + malformed factory (NodeNotFound/SelfSupersede için private basis)
- audit_seq exhaustion + cross-ledger monotonic seq
- Fingerprint stabil + direction-sensitive
- serde round-trip + 2 opacity trybuild (C13-paralel boundary)

## Sıradaki PR'lar (Faz 8b devam)

### PR #50 — `SupersedeSession` + production authority issuer
- Faz 8a `OperatorReviewSession` desenine paralel. `SupersedeApplication`'ı içeride üretir.
- **Production `issue_for_supersede_session` ctor** (gate.rs'deki `#[cfg(test)]` issuer'lar
  production'a taşınır; `SupersedeAuthority` hala Copy → "issuance-gated, not consumed").
- **Candidate proposal kaderi (Review PR #49 tur 2):** pipeline `Supersedes` Candidate proposal
  üretir → session inceler → apply. Proposal edge silinmez mi, promote mu edilir, askıda mı kalır?
  PR #50 tasarım sorusu. apply_supersede committed edge ekler, matching Candidate edge'e dokunmaz.
- **Halef kaderi (kayıt):** successor reject/deprecate edilirse superseded node'un kaderi ne?
  PR #50-51 tasarım alanı.

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
> boyutundaki bir iş, dokunduğu her yüzey (tip, skor, sorgu semantiği, parser, invariant sayımı,
> frozen kanıt sınırı, makale dili, downstream uyumluluk) bilinçli kararlara bağlamayı gerektirir.
> "genesis type-enforced 10" ile "Paper-3 total type-enforced 13" ayrımı korunmazsa, lowering
> invariant'ları taksonomide kaybolur; frozen koşu ile current envanter karışır.
> **Evidence-first disiplini:** kanıt neyi kanıtladıysa metni onu söylemeli.
> **Mekanik PR checklist maddesi:** `grep -rn "type-enforced" docs/` — tüm yüzeyleri tek seferde yakalar.

Altı turda yakalananlar (sıra ile):
1. mainline_query dar kalmalı (geçmiş ayrı kapı)
2. `status_from_str` fail-open (bloklayıcı) + INV-C14 exact-set test
3. genesis type-enforced sayısı 10'da (toplam type-enforced 13: 10 genesis + 3 lowering; C14 runtime) + run-metadata frozen/current ayrımı
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
| `docs/papers/paper3-concept-anchoring.md` | Paper 3 v1.1 + INV-C14/C15 (15 Paper-3 invariant) |
| `docs/paper3-notes/evidence/run-metadata.md` | İki başlık: frozen snapshot (gen commit `ef022a9`, baseline `481690d`) + current protocol (15) |
| `crates/osp-core/src/anchoring/mod.rs` | `DecisionStatus` enum + helper'lar (`is_current_mainline`, `preserves_accepted_provenance`) |
| `crates/osp-core/src/anchoring/store.rs` | `mainline_history()` + `apply_supersede` (INV-C15) + `audit_seq` (global) + cycle helper + 11 StoreError varyant |
| `crates/osp-core/src/anchoring/review.rs` | `SupersedeApplication` + `PresentedSupersedeBasis` + `SupersedeRecord` + `supersede_basis_fingerprint` (4-lane) + 22 test |
| `crates/osp-core/src/anchoring/gate.rs` | `SupersedeAuthorityLevel` serde derive (audit) |
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

🚧 **PR #49 branch'te (`faz8b-apply-supersede`), henüz push edilmedi.**
- main: `a392191` (PR #48 merged — varyant + INV-C14)
- PR #49: kod + test + dokü tamam, doğrulama sonrası push.
