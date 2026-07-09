# Paper 3 — Handoff Notu (CLI accept/reject + supersession surface TAMAM)

> **Tarih:** 2026-07-09 (CLI supersession branch — commit öncesi)
> **Dal:** `feat/cli-supersession-surface` (main `c897549` üstünde)
> **Durum:** Faz 8b epistemik çekirdek (PR #48-51) + **CLI accept/reject** (PR #53) + **CLI supersession surface** (bu dal) TAMAM. İki session'ın yüzeyi kapandı: `OperatorReviewSession` (accept/reject) + `SupersedeSession` (supersede). `node_digest_hex` unconditional rename, named `SupersedeDigests`, endpoint-specific stale, store-level typed errors (E1 downcast), yön-açık confirmation. Paper 3 v1.3 Zenodo'da canlı; v1.4 derive adayı (accept/reject/supersede evaluated). Sırada: rich `SupersedePreview` (lineage/compatibility/cycle), analysis → candidate bridge.

---

## Nerede duruyoruz

Paper 3 (Concept Anchoring / Genesis Layer) **v1.3 public manuscript** + Faz 8a (OperatorReviewSession) +
Faz 8c (legacy promote kaldırma) + PR #48 (varyant + INV-C14) + PR #49 (`apply_supersede` + INV-C15 atomic) +
PR #50 (`SupersedeSession` + crate-private authority issuer, INV-C15 production invocation) + PR #51
(`mainline_query` deterministic ordering) tamam. Faz 8b'in dört PR'lık kemeri (varyant → atomik mekanizma →
güvenilir sınır → deterministik projeksiyon) kapandı.

**osp-core lib: 503 test** (PR #50: 492→502 +10 SupersedeSession; PR #51: 502→503 +1 determinism);
**24 compile-fail** (değişmedi); **workspace total 765** (osp-desktop hariç); **0 regression**.
Zenodo DOI'leri canlı (P1/P2/P3/pack). arXiv — Faz 8b epistemik çekirdek kapandığı için dondurma gerek yok artık.

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
  **10'da kaldı** (toplam type-enforced 13: 10 genesis + 3 lowering); C14 (projection) + C15 (transition)
  runtime-asserted. Toplam 15. C14/C15 ayrı paragraflarda. C4 satırı şimdiki zaman (apply_supersede kuruldu).
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

## PR #50 — ne yapıldı (bu oturumda)

### Kod
- **`SupersedeSession`** — Faz 8a `OperatorReviewSession` deseninin supersede aynası. Public
  entrypoint; `SupersedeAuthority`'yi **içeride** mint eder (`issue_for_supersede_session`, crate-private),
  `SupersedeApplication`'ı içeride üretir, token dışarı çıkmaz. `supersede()` authority parametresi
  almaz. Sözleşme: *"token dışarı çıkmaz, application dışarıda üretilemez, session public entrypoint'tir,
  store atomik geçişi korur, gerçek operator yetkilendirmesi deployment sınırında kalır."*
- **`issue_for_supersede_session`** (gate.rs) — **crate-private** production issuer (public DEĞİL —
  4-tur review mutabık). `pub(crate)` external capability confinement'i garanti eder; "sole in-crate
  production caller" TCB/code-review discipline. `#[allow(dead_code)]` YOK (production caller canlı kod).
- **`SupersedeSession::supersede` deterministic precedence** (1-11): basis mismatch → tek mainline_query
  snapshot → currentness → çift freshness → counter checked_add (mutation öncesi) → internal authority
  → application → store.apply_supersede (defense-in-depth) → counter assign (başarılı store op sonrası).
- **`SupersedeError` genişletme** — `StaleSupersededBasis`/`StaleSuccessorBasis`/`SupersedeBasisMismatch`
  (store ile aynı ad) /`SessionCounterExhausted`. `NodeDigest` doğrudan (serde derive etmiyor).
- **`SupersedeApplication::new` cleanup** — `#[cfg_attr(not(test), allow(dead_code))]` kaldırıldı
  (production caller SupersedeSession).
- **Candidate proposal kaderi (kalıcı sözleşme — 4-tur review mutabık):** Opsiyon (a) — Candidate
  `Supersedes` edge historical proposal provenance olarak korunur; başarılı session ayrı Accepted
  lineage edge ekler, proposal edge'i promote/delamine ETMEZ (lane-sensitive separation). Kod (store.rs:737)
  + test (review.rs) + paper (line 97) üçü zaten bunu söylüyordu; PR #50 yorumu kalıcı sözleşmeye çevirdi.

### Testler (10 yeni SupersedeSession unit test, 0 failed)

**Sayım metodolojisi (Review PR #50 tur 1 §P2):** tek "total" sayısı yerine kapsamlı döküm —
kapsamlar karışmasın (PR #49 754 vs PR #50 762 tutarsızlığı ders).

| Kapsam | Sayı |
|---|---|
| osp-core lib unit tests | 502 (PR #49 sonrası 492 + 10 yeni SupersedeSession) |
| compile-fail cases (trybuild) | 24 (değişmedi) |
| workspace cargo-test (osp-desktop hariç) | 764 passed |
| yeni SupersedeSession unit tests | 10 |
| downstream crate tests (cli/mcp/analyzer/spike) | yeşil |

1. Mutlu yol (authority_level==Operator internal issuance kanıtı)
2. Stale superseded basis (TOCTOU) + unchanged + counter==0
3. Stale successor basis (TOCTOU) + unchanged + counter==0
4. Basis endpoint mismatch + unchanged + counter==0
5. **Store-rejection passthrough (Tur 3+4 §3)** — seed committed edge (B→A), session.supersede(A,C) →
   `AlreadySuperseded` boxed, **downcast** ile doğrulanır + unchanged + counter==0
6. Close summary (supersedes==1)
7. Zero-supersede close (supersedes==0)
8. A→B→C zincir (summary.supersedes==2, INV-C15 cardinalite)
9. Candidate edge preserved (coexistence; opsiyon (a) lock)
10. Counter exhaustion (u64::MAX → SessionCounterExhausted + unchanged)

### 4-tur review disiplini ( metodolojik ders)
Plan 4 tur review gördü; her tur mimari/claim doğruluğunu sıkıştırdı:
- **Tur 1+2 bloklayıcı:** issuer `pub(crate)` (public DEĞİL), `supersede()` authority parametresiz,
  token içeride mint, counter `checked_add`, Candidate proposal opsiyon (a).
- **Tur 2 isim:** `SupersedeBasisMismatch` (store ile aynı ad); tazelik çift-katman kaydı.
- **Tur 3:** capability confinement vs operator authorization ayrımı; `#[allow(dead_code)]` eklenmez;
  passthrough testi; INV-C11 → PR #51; makale "three PRs earlier" düş.
- **Tur 4:** "sole caller" TCB discipline; passthrough downcast; counter precedence session/store ayrımı;
  snippet comment; `SupersedeSessionSummary` derive seti.

**Tazelik çift-katman (kalıcı kayıt — Tur 2 §3):** digest karşılaştırma hem session'da (typed-error
ergonomisi, erken) hem store'da (`StoreError::Stale*`, defense-in-depth) yaşar. Generic `S::Error`
üzerinden store hatası pattern-match edilemediği için session erken kontrol eder. *Digest semantiği
değişirse iki yer değişmeli (constraint-propagation hata sınıfı).*

## CLI `osp review` vertical slice — ne yapıldı (bu dalda)

### osp-core (`crates/osp-core/src/anchoring/`)
- **`AnchorStoreSnapshot`** + **`SnapshotError`** (`store.rs`/`mod.rs` re-export) — kalıcı store snapshot
  (graph + decision_records + supersede_records + audit_sequence). `ConceptGraphSnapshot` (yalnız graph)
  genişletmesi; restore invariant-validasyonlu.
- **`NodeDigest::from_raw(u64)`** (`review.rs`) — CLI `--basis-digest` için. FNV non-crypto tazelik
  karşılaştırma değeri (güvenlik önlemi DEĞİL, capability token DEĞİL); `PresentedBasis::compile` hala
  tek üretim yolu.
- **`InMemoryAnchorStore::export_snapshot`** + **`restore_snapshot`** — export canonical sıralı
  (nodes→NodeId, edges→(source,kind,target), records→audit_seq) → bit-identik + JSON diff okunabilir.
  Restore: graph schema + node uniqueness + edge endpoints + record→node existence (tek yönlü —
  seed_trusted Accepted ledger'sız) + record→status forward integrity + dense audit_seq (union unique +
  {1..N} + ==N) + INV-C15 üç yönlü triangulation (committed edge ↔ record ↔ status, lane-sensitive,
  cycle absence, successor chain geçerli).
- **`restore_trusted_snapshot` deprecated** + **`restore_graph_only_for_trusted_bootstrap`** açık-ad
  wrapper (graph-only, ledger/audit_seq discard — persistence restoration için DEĞİL).

### osp-cli (`crates/osp-cli/src/`)
- **`store_io.rs`** — `PersistedStore` envelope (revision + store_schema_version + snapshot) + `StoreLock`
  (fs4 OS-level advisory, sabit `.lock` dosyası, process-death'ta release) + `atomic_replace` (aynı dizin
  tmp → fsync → dir sync → Windows `MoveFileEx(MOVEFILE_REPLACE_EXISTING)` / POSIX rename).
- **`application/repository.rs`** — `ReviewStoreRepository` trait + `FileReviewStore`. `mutate()` tek
  persistent transaction: lock → reload → validate → op → **revision R+1 serialize öncesi** → atomic
  save. One-shot ve interactive aynı yol.
- **`application/review.rs`** — `ReviewApplicationService` (query/mutation ayrımı). `ReviewMutationCommand`
  `expected_basis_digest` precondition (lock altında — yeni TOCTOU yok). Session'ın StaleBasis'i
  tautolojik ama zararsız.
- **`seed_file.rs`** — `CandidateSeedFile` DTO (nodes-only; status/id alanları yok, `deny_unknown_fields`,
  duplicate canonical kontrolü, id kind+canonical'dan türetilir). Candidate hard-code → illegal state
  unrepresentable.
- **`commands/graph.rs`** — `osp graph init/status/validate` (Candidate-only bootstrap, existing → fail,
  post-init restore-validasyon).
- **`commands/review.rs`** — `osp review list/show/accept/reject` (confirmation: TTY basis göster +
  `[y/N]`; non-TTY/`--yes` → `--basis-digest` zorunlu). `--operator` zorunlu (mutation), `$OSP_OPERATOR`
  fallback, generic "operator" default yok.
- **`review_session.rs`** — interactive wizard (generic `R: BufRead, W: Write`; her mutation `mutate()`,
  gösterilen basis korunur).

### osp-mcp (`crates/osp-mcp/tests/`)
- **`inv_c11_agent_surface.rs`** — dar adlandırma. Statik kaynak taraması: MCP source'da review/supersede
  authority tool literal'ları + `open_for_operator`/`SupersedeSession::open_for_operator` çağrıları yokluğu.
  "Partial deployment-surface regression test, not process-level isolation proof."

### Testler (0 regression)
- **osp-core lib:** 503 → 521 (+18 AnchorStoreSnapshot test: round-trip, dense audit_seq, C15 üç yönlü
  violation matrisi, canonical bit-identik, forward integrity, successor chain, lane-sensitivity).
- **osp-cli:** 17 unit (store_io 4, repository 3, seed_file 6, review_session 3, +1) + 11 integration
  (graph init/status/validate, review list/accept, stale basis, restart-safe, operator requirement,
  corrupt store, JSON output).
- **osp-mcp:** +2 INV-C11 agent-surface regression.

### Paper güncellemesi (`docs/papers/paper3-concept-anchoring.md`)
- **INV-C11 (:279, :297):** CLI operator-facing / MCP agent-facing yeniden sınıflandırma ("CLI çağıran
  operator" DEME — attribution, auth deployment boundary).
- **Known gap (:93):** "closed for evaluated `AnchorStoreSnapshot` path" (triangulation); alternate
  backends equivalent validation.
- **§10 limitation + §11.3 future work:** CLI `osp review` evaluated; supersession surface + Cockpit +
  remaining sessions future work.
- **PR/Faz YOK** paper prose'ta — "evaluated artifact" dili. paper3.tex (dist) eski v1.3; derive aracı
  sonraki revizyonda senkronize eder.

## CLI `osp review supersede` — ne yapıldı (bu dalda)

PR #53 (accept/reject) üzerine **supersession operator surface** eklendi. Faz 8b'in iki
session'ından ikincisinin yüzeyi kapandı (`OperatorReviewSession` ✓ → `SupersedeSession` ✓).

### osp-cli (`crates/osp-cli/src/`)
- **`node_digest_hex` rename + unconditional** (Aşama 1) — `basis_digest(_hex): Option` →
  `node_digest_hex: String` (tüm statülerde dolu; hex only, raw u64 yok — JS 2^53). Intentional
  JSON breaking rename. `ensure_candidate` helper explicit Candidate gate (accept/reject).
- **Supersede types** (`errors.rs`) — `SupersedeEndpoint` + endpoint-specific errors
  (`EndpointNotCurrent { status: Option }`, `StaleSupersededBasis`, `StaleSuccessorBasis`,
  `SelfSupersede`, `AlreadySuperseded`, `IncompatibleSupersedeEndpoints` 4 family alanı,
  `SupersedeCycle`) + `SupersedeDigests` (named, tuple yok) + `SupersedeCommand` (ayrı) +
  `ReviewSupersedeMutation`/`PersistedSupersedeOutput` (accept/reject'i kirletmez).
- **`apply_supersede`** (`application/review.rs`) — mainline_query (Accepted), iki-digest
  precondition (endpoint-specific stale), `PresentedSupersedeBasis::compile`, `SupersedeSession`.
- **`map_supersede_error` + `map_supersede_store_error`** — source string + downcast typed
  (E1: `SupersedeError::Store` Display "store error" source'u enterpole ETMİYOR → downcast
  `AlreadySuperseded`/`IncompatibleSupersedeEndpoints`/`SupersedeCycle` typed; fallback source).
- **`SupersedePresentation` + `load_supersede_presentation`** (R3#2) — one-shot + interactive
  aynı confirmation metni; revision retry pair olarak (UX, correctness değil).
- **One-shot adapter** (`commands/review.rs`) — `osp review supersede <old> <new>` + yön-açık
  confirmation ("'{successor}' supersedes '{superseded}'", edge `successor→superseded`) + `--format json`.
- **Interactive adapter** (`review_session.rs`) — `supersede <old> <new>` komutu + endpoint-specific
  stale mesajları + help text.
- **`--format json` retroaktif** (R4) — accept/reject mutation da JSON (tutarlı otomasyon contract).

### osp-core değişiklik YOK
`SupersedeSession`/`PresentedSupersedeBasis`/`SupersedeError`/`SupersedeRecord` PR #50'de hazır.
`mutate()` generic — iki-digest op fits verbatim.

### Testler (0 regression)
- **osp-cli unit:** 20 → 26 (+6 mapper: AlreadySuperseded/Incompatible/Cycle/fallback source/
  endpoint-specific stale/SelfSupersede).
- **osp-cli integration:** review_flow 21 + supersede_flow 13 (mutlu yol + yön assert + stale +
  swapped + missing/non-current + self + negatif digest + restart-safe + rename + consolidation
  + chain + interactive + confirmation n).

### Pre-commit count checklist (F2 dersi — 4. kez kaçırdık)
Test sayıları doküman yüzeylerinde elle girilince same-PR test eklemesinde stale kalıyor
(STATUS:39, STATUS:182, run-metadata:49). **Commit öncesi mekanik doğrulama:**
```bash
# Ground truth — her test dosyası için:
for t in review_flow supersede_flow; do
  echo -n "$t: "; cargo test -p osp-cli --test $t 2>&1 | grep -o "[0-9]* passed" | head -1
done
echo -n "unit: "; cargo test -p osp-cli --bin osp 2>&1 | grep -o "[0-9]* passed" | head -1
echo -n "core lib: "; cargo test -p osp-core --lib 2>&1 | grep -o "[0-9]* passed" | head -1
# Sonra grep ile üç yüzeyde aynı sayılar mı:
grep -rn "supersede_flow\|review_flow\|osp-cli:.*unit" docs/STATUS.md docs/paper3-notes/evidence/run-metadata.md
```
Sayı yazıldıktan sonra aynı PR'da test eklendiyse re-propagate ET.

## CLI `osp review supersede-preview` — ne yapıldı (bu dalda)

Rich `SupersedePreview` read-only query — HANDOFF "Sıradaki işler #1" kapandı. Standalone
query (`osp review supersede-preview <old> <new>`) + one-shot TTY confirmation + interactive
wizard confirmation **tek canonical model + tek renderer** kullanır (divergence sıfır; HANDOFF
"aynı preview render eder" cümlesi doğru kaldı).

### osp-core (minimal additive — mutation semantiği değişmez)
- **Üç public read-only accessor + typed compatibility read model** (`store.rs`):
  `committed_supersede_incoming_sources` (Vec source IDs — INV-C15 ≤1, deterministic sorted),
  `inspect_supersede_compatibility`, `would_create_supersede_cycle` (Result<bool> — node existence).
  Canonical private helper `supersede_compatibility_from_parts` + `SupersedeCompatibility` struct.
- **apply_supersede delegasyonları** (12-step precedence KORUNUR): step 5 (incoming → accessor),
  step 6-7 (currentness → `is_current_mainline()`), step 9 (compatibility → canonical helper).
  Mutation semantiği, hata tipleri, error ordering değişmedi. `is_reachable_via_committed_supersedes` private kaldı.
- **Domain policy ayrımı (divergence mekanik engellenir):** incoming → core accessor; currentness →
  `is_current_mainline()`; compatibility → core predicate; cycle → core predicate; identity → saf observation.

### osp-cli (`crates/osp-cli/src/`)
- **Canonical read model** (`application/review.rs`): `SupersedePreviewOutput` + `SupersedeBlockerCode`
  (typed enum + `ordering_key()` — structural steps 5–10'a birebir) + `SupersedeLineagePreview` (bounded
  DAG + typed `LineageTruncation`) + `ProposedSupersedeEdge` + `primary_structural_blocker`.
  `SupersedePresentation` (minimal) kaldırıldı.
- **`build_supersede_preview`** — tüm policy core accessor/predicate'lardan; non-Accepted → blocking_reason
  (hard error DEĞİL; missing → NotFound); self dahil her durumda lineage üretilir (cycle bastırılır).
- **Tek read path:** `read_validated_store` + `execute_query(ReviewQuery::SupersedePreview)` →
  `execute_supersede_preview` sarmalar (çift repo.read yok). List/Show da aynı read motoru.
- **`supersede_preview_render.rs`** (yeni) — body-only renderer (UI state yok; üç yüzey çağırır).
- **`SupersedeConfirmationOutcome { Confirmed, Ineligible, Aborted }`** — exit-code sözleşmesi
  (standalone ineligible exit 0 / mutation ineligible-aborted non-zero / wizard session'a dönüş).
  Self early gate YOK — self blocker-bearing preview üretir.
- **`osp review supersede-preview <old> <new>`** + `ReviewAction::SupersedePreview` + main.rs dispatch.

### Testler (0 regression)
- **osp-core lib:** 526 → 538 (+12: compatibility matrix, incoming accessor 4 vaka, cycle 3 vaka,
  step-9 characterization, currentness, multi-blocker precedence).
- **osp-cli unit:** 26 → 37 (+11 preview builder: mutlu yol, self/already/incompatible-kind/family/
  non-current/cycle/lineage chain/consolidation/missing/closed-output invariant).
- **osp-cli integration:** supersede_flow 20 (güncellenen — rich preview body) + review_flow 21
  (değişmedi) + **preview_flow 10** (yeni: mutlu yol text/json, incompatible, cycle, lineage chain,
  missing, ineligible exit 0, self, non-accepted, wizard ineligible).

### 5 tur plan review'ün metodolojik dersi
Plan 5 tur review gördü; her tur mimari/claim doğruluğunu sıkıştırdı:
- **Tur 1-2:** lineage DAG (Vec<String> değil — consolidation bilgi kaybı) + Accepted gate çelişkisi
  (incoming → SupersededAccepted zorunlu → blocker'a ulaşılamaz) → non-Accepted blocking_reason modeli.
- **Tur 3:** cycle tek source-of-truth (core wrapper) ama compatibility asimetrik divergence → iki
  dar predicate + apply step 9 delegasyonu. `primary_blocker` → `primary_structural_blocker`.
- **Tur 4:** blocking_reasons sırası "core precedence ile uyumlu" iddiası yanlıştı (self planda 1./
  core'da step 8) → core structural steps 5–10'a birebilir hizalama + characterization. self early
  gate tek-model ilkesini bozardı → kaldırıldı.
- **Tur 5:** incoming predicate `bool` değil source ID'leri döndürmeli (presentation duplication);
  self'te lineage her zaman; currentness `is_current_mainline()`. Tek source-of-truth her domain
  kuralı için ayrı canonical predicate (devasa helper değil).

## Sıradaki işler

### Analysis → candidate bridge (sonraki milestone)
- `AnalysisResult → CandidateBatch → GraphSeed` projection protocol. Acceptance kriterleri
  (persistent review lifecycle) geçti; şimdi candidate üretim kaynağı.

### Diğer
- **TUI v2:** dialoguer/rustyline, fuzzy, renk (v1 stdio yeterli).
- **Snapshot content-digest** (v2): elle JSON düzenleme tahrifatı için.
- **arXiv:** v1.3 epistemik çekirdek + CLI accept/reject/supersede surface tamam; v1.4 derive adayı.

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
> **Mekanik PR checklist maddeleri:** `grep -rn "type-enforced" docs/` +
> `grep -rn '"22 "\|22 cumulative\|22 compile-fail' docs/` (compile-fail count propagation) —
> tüm yüzeyleri tek seferde yakalar.

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
| `docs/papers/paper3-concept-anchoring.md` | Paper 3 v1.3 + INV-C14/C15 (15 Paper-3 invariant) |
| `docs/paper3-notes/evidence/run-metadata.md` | İki başlık: frozen snapshot (gen commit `ef022a9`, baseline `481690d`) + current protocol (15, PR #50 production invocation) |
| `crates/osp-core/src/anchoring/mod.rs` | `DecisionStatus` enum + helper'lar (`is_current_mainline`, `preserves_accepted_provenance`) |
| `crates/osp-core/src/anchoring/store.rs` | `mainline_history()` + `apply_supersede` (INV-C15) + `audit_seq` (global) + cycle helper + 11 StoreError varyant + **`AnchorStoreSnapshot` + `SnapshotError` + `export_snapshot`/`restore_snapshot` (validate_snapshot + has_committed_supersedes_cycle) + `restore_graph_only_for_trusted_bootstrap` (deprecate wrapper) + 18 snapshot test** |
| `crates/osp-core/src/anchoring/review.rs` | `OperatorReviewSession` + `DecisionApplication` + **`SupersedeSession` + `SupersedeSessionSummary` (PR #50)** + `SupersedeApplication` + `PresentedSupersedeBasis` + `SupersedeRecord` + `supersede_basis_fingerprint` (4-lane) + **`NodeDigest::from_raw` (CLI --basis-digest)** + 47 unit test |
| `crates/osp-core/src/anchoring/gate.rs` | `SupersedeAuthorityLevel` serde derive (audit) + **`issue_for_supersede_session` crate-private issuer (PR #50)** |
| `crates/osp-core/src/anchoring/scorer.rs` | 5. kol (SupersededAccepted = 0.4) |
| `crates/osp-core/src/task_bridge.rs` | `is_current_mainline()` helper + regresyon testi |
| `crates/osp-core/tests/anchoring_mvp.rs` | `status_from_str` fail-closed + parser testleri |
| `crates/osp-cli/src/store_io.rs` | **`PersistedStore` envelope + `StoreLock` (fs4) + `atomic_replace` (Windows MoveFileEx / POSIX rename) + read/write_persisted_store** |
| `crates/osp-cli/src/application/` | **`repository.rs` (`ReviewStoreRepository` + `FileReviewStore`, tek persistent transaction) + `review.rs` (`ReviewApplicationService`, query/mutation, expected_basis_digest)** |
| `crates/osp-cli/src/seed_file.rs` | **`CandidateSeedFile` DTO (nodes-only, deny_unknown_fields, id derive, Candidate hard-code)** |
| `crates/osp-cli/src/commands/{graph,review}.rs` | **`osp graph init/status/validate` + `osp review list/show/accept/reject/session`** |
| `crates/osp-cli/src/review_session.rs` | **interactive wizard (generic R/W)** |
| `crates/osp-cli/tests/review_flow.rs` | **11 integration test (stale basis, restart-safe, operator, corrupt, canonical)** |
| `crates/osp-mcp/tests/inv_c11_agent_surface.rs` | **INV-C11 agent-surface regression (static source scan)** |

## Kullanıcıya not

- **osp-desktop kırık** (PR #40 sonrası API drift: `compute_raw_from_delta` 4 argüman, `Claim`
  `removed_edges`+`task_id` gerektiriyor). CI zaten hariç tutuyor (Tauri webkit bağımlılıkları).
  Ayrı PR adayı — Faz 8b dışı.
- **mainline_query deterministik sıralama** — küçük PR adayı (agent-facing context tekrarlanabilirliği).
- **arXiv** 1 hafta ertelendi; Jimenez e-postası hazır (favorilerde, docs'ta değil).
- **4 DOI canlı:** P1/P2/P3/pack tüm Zenodo'da.

## Commit durumu

✅ **Faz 8b epistemik çekirdek + CLI `osp review` vertical slice TAMAM.**
- main: `5ed13c1` (PR #52 merged — stale cleanup + paper3 artifact üretim aracı).
- `feat/cli-osp-review` dalı: CLI `osp review` (accept/reject + persistent AnchorStoreSnapshot).
- Faz 8b PR #48-51 merged; CLI osp review bu dalda (commit öncesi).
- **521 lib test** (503 → 521, +18 AnchorStoreSnapshot) + **24 compile-fail** + **osp-cli 17 unit + 11 integration** + **osp-mcp +2 INV-C11** yeşil.

## Yayın durumu (v1.3 → v1.4 adayı)

**Paper 3 v1.3 Zenodo'da yayımlandı** — Faz 8b supersession vocabulary tamam.

| Kayıt | Concept DOI | v1.3 Version DOI | License |
|---|---|---|---|
| Paper 3 | `10.5281/zenodo.21220992` | `10.5281/zenodo.21251821` | CC-BY-4.0 |
| Paper 1 | `10.5281/zenodo.21206545` | (v2.6) | CC-BY-4.0 |
| Paper 2 | `10.5281/zenodo.21207704` | (v1.2) | CC-BY-4.0 |
| Evidence Pack | `10.5281/zenodo.21207762` | (frozen) | CC-BY-4.0 |

- **License düzeltmesi:** Üç makale + evidence pack artık **CC-BY-4.0** (önceki Apache-2.0 yanlıştı — makale yaratıcı eser, kod Apache-2.0 kalır). Tüm Zenodo kayıtları güncellendi.
- **Cite pratiği:** Concept DOI kullanılır (her zaman en son versiyona resolve). Version DOI belirli sürümü işaret eder (v1.3 = `21251821`).
- **arXiv sonrası:** v1.3 epistemik çekirdek (supersession vocabulary) tamamladığı için dondurma gerek yok. Jimenez e-postası hazır (endorsement).
- **PR #52:** makale-kod tutarlılığı (markdown stale + PDF üretim aracı + v1.3 review düzeltmeleri). Merge sonrası arXiv yoluna çıkış.
