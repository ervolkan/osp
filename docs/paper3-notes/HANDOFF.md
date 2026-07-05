# Paper 3 Taslak Yazımı — Handoff Notu (Aşama 1 tamam, iskelet hazır)

> **Tarih:** 2026-07-05 (güncellendi)
> **Dal:** `docs/paper3-draft` (main `0d269e7`'den)
> **Durum:** Aşama 1 (evidence freeze hardening + held-out + metadata + conformance) TAMAM. Aşama 2 (iskelet) TAMAM. Aşama 3 (bölüm dolgu) bekliyor.

---

## Nerede duruyoruz

Paper 3 (Concept Anchoring / Genesis Layer) **omurgası tamamlandı** — Faz 0-5.1 + 3 hardening PR.
**Aşama 1 evidence freeze 4 review turu sonunda sertleştirildi** (canonical-kesme + marker-kaçırma
tuzakları yapısal imkânsız kılındı). **Aşama 2 iskelet hazır** (`paper3-draft-v1.md`).

## Bu oturumda yapılanlar (3 commit)

### Commit 1 (`481690d`) — evidence freeze hardening + held-out
- **A1:** Adım 1 gerçek pipeline koşusu (seed → `Coupling must not exceed module threshold.` → CouplingMustNot → SingleCandidate(Coupling) gerçek pipeline'dan)
- **A2:** Adım 6 dürüst etiket (feature-gated bypass AÇILMADI; INV-C3 in-crate enforced)
- **A3:** Adım 7 provenance (allowed_operations operator-supplied)
- **A4:** Volatile'lar JSON'dan çıkarıldı (tek evi run-metadata.json)
- **A5:** Snapshot-compare pattern (normal CI compare, PAPER3_FREEZE=1 dondurma)
- **A6:** 4 negatif yol (AxisMismatch, AxisNotInCandidates, TemplateNotSuggested, NotAccepted)
- **§0:** Pre-flight canonical + marker tablosu (6 cümle × 4 sütun, gerçek pipeline koşusu)
- **Aşama 1b:** 5 held-out cümle (4 held_out + 1 regression_anchored), 5-state conformance

### Commit 2 (`8431c9e`) — metadata + conformance
- **Aşama 1c:** run-metadata.md + .json (hash₁, sha256, evidence strata)
- **Aşama 1d:** conformance-results.md + .json (18 cümle: Conform 12, PartialConform 2, KnownLimitation 2, RejectAsExpected 2, UnexpectedFailure 0)

### Commit 3 — iskelet + docs
- **Aşama 2:** `paper3-draft-v1.md` iskelet (tüm bölümler + abstract placeholder + contributions + Appendix A pre-flight tablo + hash cite)
- HANDOFF + STATUS güncellendi

## Review içgörüsü (4 turun birikimi)

*Test altına alınmayan invariant ihlal edilir.* — 4 review turunda yakalanan hataların hepsi iki sınıfa indi:
1. **Kısıt-yayılmaması** — canonical-kesme (A1→B1→B5, 3 tekrar), marker-kaçırma (B2 "must"→"must not")
2. **İddia-implementasyon ayrışması** — B5 None vs NoAxisCandidate, helper duplication

Pre-flight canonical tablosu (§0/Appendix A) her iki sınıfı yapısal imkânsız kılar. Bu, makalenin kendi kanıt dosyalarına uyguladığı metodolojik ilkedir.

## Yeni oturumda yapılacaklar

### Aşama 3: Bölüm dolgu (sıradaki)
`paper3-draft-v1.md` içindeki [TODO] yerlerini doldur:
1. **Abstract** (~350 kelime) — D2-6 hedef
2. **§1 Introduction** — problem (embedding-first anchoring eleştirisi D2-1), approach, contributions (4 madde), terminology mapping (D1-5)
3. **§2 Motivating Example** — e2e-binding-chain-replay.json'a yaslan
4. **§3 Genesis Ontology** — INV-C1..C8 4-column table
5. **§4 Predicate Lowering** (kısa, D2-3) — "a rule is not a predicate"
6. **§5 Cross-Family Translation** (asıl epistemik argüman, D2-3) — INV-P3
7. **§6 Binding & Task Genesis** — three-gate API
8. **§7 Verification Evidence** (D2-4) — 5 strata + "What this does not evaluate"
9. **§8 Related Work** — ~6
10. **§9 Discussion** — ontological binding vs embedding, determinism-first, "storage is not epistemology" (D2-5), "fixture design must be verifiable"
11. **§10 Threats** — self-authored gold standard (D1-2), keyword matcher placeholder, semantik false-positive (held_002), negation (held_003), acceptance seeded
12. **§11 Future Work** — Faz 6/7/8
13. **§12 Conclusion** — "Words do not mutate project reality."
14. **Appendix A** pre-flight tablo (hazır), **Appendix B** e2e replay (hazır), **References** ~12-15

### Sonra: arXiv adayı review

## Önemli dosyalar

| Dosya | Açıklama |
|---|---|
| `docs/paper3-draft-v1.md` | **Paper 3 iskelet** (Aşama 2) |
| `docs/paper3-notes/evidence/e2e-binding-chain-replay.json` | E2E zincir (Adım 1 gerçek pipeline) |
| `docs/paper3-notes/evidence/e2e-rejected-paths-replay.json` | 4 negatif yol |
| `docs/paper3-notes/evidence/held-out-adversarial-fixtures.json` | 5 held-out cümle |
| `docs/paper3-notes/evidence/run-metadata.md` + `.json` | Volatile'lerin tek evi |
| `docs/paper3-notes/evidence/conformance-results.md` + `.json` | 18 cümle 5-state |
| `crates/osp-core/tests/paper3_evidence.rs` | §0 pre-flight + e2e + negatif yollar |
| `crates/osp-core/tests/paper3_heldout.rs` | 5 held-out cümle pipeline koşusu |
| `docs/concept-anchoring-design.md` | Paper 3 tasarım dokümanı (Türkçe, v0.2.1+) |
| `docs/paper2-draft-v1.md` | Paper 2 draft (template — yapısal pattern) |

## Commit durumu

✅ **Aşama 1 + Aşama 2 commit'lendi (3 commit).**
- Commit 1: `481690d` — evidence freeze hardening + held-out (hash₁)
- Commit 2: `8431c9e` — run-metadata + conformance (hash₂)
- Commit 3: `<bu commit>` — iskelet + HANDOFF/STATUS

## Kullanıcıya sorulacak (kapsam dışı)

`docs/interaction-surfaces-design-document.md` — untracked dosya, bu oturumun kapsamı dışında.
Bağlamı bilinmiyor; Paper 3 akışına karışmadı. Ayrı bir iş olarak netleştirilecek.

---

## Çerçeve kararları (onaylı — iskelete uygulandı)

- **Ana tez:** *"The ontological role of a human sentence in project reality is determined not by embedding proximity but by the completeness of the ontological binding chain."*
- **Embedding-first anchoring eleştirisi (D2-1):** *"Embedding can propose candidate proximity, but it cannot decide ontological role, authority, acceptance, or executability."*
- **Verification makalesi (D2-4):** §7 = "Verification Evidence", §7.6 "What this does not evaluate"
- **Lowering = placeholder:** *"The keyword matcher is not the contribution. The contribution is the binding protocol."*
- **Dil:** İngilizce (Paper 1/2 uyumu)

## Review düzeltmeleri (hepsi uygulandı)

- **D1-1:** Evidence freeze yazımdan önce ✅ (Aşama 1 tamam)
- **D1-2:** Held-out/adversarial set + self-authored gold standard threat §10 ✅
- **D1-3:** 11 Paper 3'e özgü invariant (kümülatif 18 bağlam) ✅
- **D1-5:** Contribution ↔ bölüm sırası uyumlu, terminoloji eşlemesi girişte ✅
- **D2-1:** "anti-RAG" → "embedding-first anchoring eleştirisi" ✅
- **D2-2:** "precision/recall" → "golden fixture conformance" ✅
- **D2-3:** §4 kısa, §5 asıl epistemik argüman ✅
- **D2-4:** §7 = "Verification Evidence" ✅
- **D2-5:** KuzuDB story §9.3'te kısa ✅
- **D2-6:** Abstract ~350 kelime hedef (Aşama 3'te doldurulacak)

## 4 review turu içgörüleri (ekstra)

- **Canonical-kesme tuzağı** (A1→B1→B5, 3 tekrar) + **marker-kaçırma** (B2) → §0 pre-flight tablosu yapısal imkânsız kıldı
- **Volatile'lar** evidence JSON'dan çıktı (snapshot drift) → run-metadata.json tek ev
- **A5 snapshot pattern** — normal CI compare, PAPER3_FREEZE=1 dondurma
- **B5 None vs NoAxisCandidate** — koda-bakış ile tekilleştirildi (lowering None döner)
- *"Test altına alınmayan invariant ihlal edilir"* — §9.4 + Appendix A metodolojik ilke
