# Paper 3 — Handoff Notu (CLI review + supersede + preview + analysis bridge + metric projection + PR C axis-granular evidence + PR D evidence projection + PR E entity resolution core + PR E2 CLI scheme adoption TAMAM)

> **Tarih:** 2026-07-12 (`feat/cli-scheme-adoption` dalı — PR E2 implementasyonu)
> **Dal:** `feat/cli-scheme-adoption` (main `06d3a02` üstünde — PR E merged; plan `05875ff` 3 tur review)
> **Durum:** Faz 8b epistemik çekirdek (PR #48-51) + **CLI accept/reject** (PR #53) + **CLI supersession surface** (PR #54) + **Rich SupersedePreview query** (PR #55) + **Analysis → candidate bridge** (PR #56) + **Analysis metric projection** (PR #57) + **PR C (core axis-granular evidence model)** + **PR D (evidence projection + in-process wiring proof)** + **PR E (entity resolution core + persistence contract)** + **PR E2 (CLI scheme adoption — graph init binding + resolve-code-entity)** TAMAM. Dokuz yüzey kapandı: `OperatorReviewSession` + `SupersedeSession` + `supersede-preview` + `graph init --analyze` (Module identity projection) + metric projection (axis-granular draft, NOT core evidence) + core axis-granular evidence model (per-axis provenance/strength/coverage) + CLI→core evidence projection (draft→evidence conversion + ExpectedImplementation scorer seam) + entity resolution core (CodeIdentityKey + ResolvesTo + CodeEntityResolutionSession + INV-C16 atomic + snapshot v2 migration) + CLI scheme adoption (graph init binding seeding otomatik + `osp review resolve-code-entity` + minimal canonical preview). Paper 3 v1.3 Zenodo'da canlı; v1.4 derive adayı. Sırada: PR F (evidence identity migration), PR G (lineage-aware effective projection), arXiv v1.4.

---

## Nerede duruyoruz

Paper 3 (Concept Anchoring / Genesis Layer) **v1.3 public manuscript** + Faz 8a (OperatorReviewSession) +
Faz 8c (legacy promote kaldırma) + PR #48 (varyant + INV-C14) + PR #49 (`apply_supersede` + INV-C15 atomic) +
PR #50 (`SupersedeSession` + crate-private authority issuer, INV-C15 production invocation) + PR #51
(`mainline_query` deterministic ordering) tamam. Faz 8b'in dört PR'lık kemeri (varyant → atomik mekanizma →
güvenilir sınır → deterministik projeksiyon) kapandı.

**osp-core lib: 588 test** (PR E2 untouched); **osp-cli: 150 unit** (PR E2: 123→150 +27: 8 identity_bridge + 4 analysis_bridge binding + 9 application/review resolution mapper/preview + 6 apply_resolution production target-pinning);
**28 compile-fail** (PR E2 eklemedi); **workspace total ~1086** (osp-desktop hariç); **0 regression**.
Zenodo DOI'leri canlı (P1/P2/P3/pack). arXiv — Faz 8b epistemik çekirdek kapandığı için dondurma gerek yok artık.

## PR E2 — CLI scheme adoption (bu oturumda)

### Kod (`crates/osp-cli/src/`)
- **`identity_bridge.rs`** (yeni) — `AnalysisIdentityContext` (private fields + constructor; scheme +
  policy tek propagation value) + `IdentityBridgeError` (`CoreValidation` + `CanonicalizationDrift`;
  Eq derive YOK — core `CodeIdentityKeyError` Eq değil) + `to_core_identity_key` (CLI
  `CanonicalCodeIdentity` → core `CodeIdentityKey`; drift runtime guard) + `From<PathCasePolicy> for
  CodePathCasePolicy` (exhaustive enum mapping). 8 unit test.
- **`analysis_bridge.rs`** — `CandidateProjectionOutput.code_identity_bindings` + `BridgeRunOutput.
  code_identity_bindings` (candidate başına bir binding; `AnalysisCandidateSeed::try_new` sonrası
  validated/sorted candidate'lardan co-derived — tur 1 review karar #1). `BridgeRunReport.
  projected_identity_bindings` (tur 2 P2-B — `bindings_seeded` değil). `BridgeError::IdentityBridge(
  #[from])`. 4 unit test (pr_e2_*).
- **`commands/graph.rs`** — `--analyze` branch `seed_code_identity_bindings_trusted` çağırır (node
  existence sonrası; `AnchorStore` trait import). Başarılı seeding sonrası ayrı stderr
  `identity bindings seeded: N` (tur 2 P2-B). Legacy `--seed` binding üretmez (PR A semantics preserved).
- **`errors.rs`** — `ExpectedResolutionTarget` (Create/Reuse; tur 2 P0-2 operator-pinned target) +
  `ResolveCodeEntityCommand` (candidate digest + expected_target ikili pinning) +
  `ResolutionOutcomeView` (typed enum + `as_str()`; tur 2 P2-A / tur 3 P2-4) +
  `ResolveCodeEntityMutation` + `PersistedResolveCodeEntityOutput` + `ReviewError` yeni varyantlar
  (CandidateNotAccepted, StaleResolutionBasis, AlreadyResolved, StaleResolutionTarget,
  EntityNotLiveForResolution, EntityIdentityCollision, DuplicateLiveEntity, MissingIdentityBinding).
- **`application/review.rs`** — `ReviewQuery::ResolveCodeEntityPreview` + `ReviewReadOutput::
  ResolveCodeEntityPreview` (tur 3 P1-4 tek read motoru). `execute_resolve_code_entity` (mutate
  envelope) + `execute_resolve_code_entity_preview` (execute_query sarmalar). `apply_resolution`
  (tur 2 P0-1 compile-based — `candidate_query()` YOK; `PresentedResolutionBasis::compile` canonical
  Accepted gate) + `validate_expected_target` (tur 2 P0-2 target pinning). `map_resolution_error(
  candidate_id, error)` (tur 2 P1-A context; `CandidateNotFound(id)` tuple; unit variant'lar
  context'ten ID) + `map_resolution_store_error(candidate_id, source)` (tur 3 P1-2 `BindingWrongKind`
  reachable; tur 3 P1-3 `NotPromotableFrom` status-aware split — non-Accepted → CandidateNotAccepted,
  Accepted+wrong-structural → NotPromotable). `ResolutionPreviewOutput` + `ResolutionTargetPreview`
  (Serialize; tur 3 P1-1B) + `IdentityKeyPreview` + `ResolutionCandidatePreview` + `build_resolve_
  code_entity_preview` (tek read; compile-based target reveal). `expected_target()` infallible
  (tur 3 preview sadeleştirme). 9 unit test (map_resolution_* + resolution_preview_*).
- **`commands/review.rs`** — `ResolutionTargetOutcomeArg` (value_enum; tur 3 P0 colon-free) +
  `ReviewResolveCodeEntityArgs` (`--candidate-digest` + `--target-outcome` + `--target-entity-id` +
  `--target-entity-digest` explicit flags) + `ReviewResolveCodeEntityPreviewArgs` +
  `parse_expected_target` (validation matrisi: Create→id zorunlu digest YOK; Reuse→ikisi zorunlu) +
  `run_review_resolve_code_entity` (text output `as_str()`; tur 3 P2-4) + `run_review_resolve_code_
  entity_preview` + `confirm_with_resolution` (TTY: minimal preview + target reveal + `[y/N]`).
- **`commands/resolve_code_entity_preview_render.rs`** (yeni) — body-only renderer (üç yüzey tek
  renderer; supersede_preview_render pattern).
- **`main.rs`** — `ReviewAction::ResolveCodeEntity` + `ResolveCodeEntityPreview` + dispatch.
- **`review_session.rs`** — `resolve <candidate>` wizard komutu (tur 3 P1-5 — reason YOK; preview →
  confirm → reason prompt → mutation; per-command session — tur 1 review karar #3).

### Testler (0 regression; `RUSTFLAGS="-D warnings"` temiz)
- **osp-cli unit:** 123 → 144 (+21: 8 identity_bridge + 4 analysis_bridge binding + 9 application/
  review resolution mapper/preview)
- **osp-cli integration:** review_flow 21 + supersede_flow 20 + preview_flow 12 + analyze_bridge_flow
  13 (9→13, +4 PR E2 binding seeding) + architecture_guards 2 + **resolution_flow 9 (yeni)**
- **osp-core lib:** 588 (değişmedi — PR E2 core untouched)
- **compile-fail:** 28 (değişmedi — PR E2 eklemedi)

### 3 tur plan review'ün metodolojik dersi
Plan 3 tur review gördü; her tur mimari/claim doğruluğunu sıkıştırdı:
- **Tur 1 (ontolojik):** iki katmanlı kimlik modeli + outcome operator-chosen DEĞİL + binding store-owned.
- **Tur 2 (2 P0 bloklayıcı):** P0-1 `candidate_query()` Accepted bulamaz (compile-based düzeltme) +
  P0-2 target pinning operator presentation sınırına taşınmadı (ExpectedResolutionTarget). 4 P1 +
  2 P2.
- **Tur 3 (1 P0 + 5 P1 + 4 P2):** P0 non-interactive target CLI colon-free + derleme düzeltmeleri
  (Eq derive, Serialize, Result açma) + BindingWrongKind reachable + NotPromotableFrom status-aware
  split (NotPromotableFrom(Accepted) mümkün) + ReviewQuery tek read motoru + wizard reason sırası.

Tüm bulgular core koda karşı doğrulandı (explore agents — `candidate_query` filter, `ResolutionError`
varyant shape'leri, `apply_resolution` reachable `StoreError` set, `resolution_basis_view` Accepted gate,
`BindingWrongKind` resolution reachability, `compute_resolution_target` inactive→hard error policy).

### Tur 3 sabitlenen kritik tasarım kararları
- **`candidate_query()` YOK (tur 2 P0-1):** Accepted candidate candidate_query dışında; tek yol
  `PresentedResolutionBasis::compile` (`resolution_basis_view` Accepted gate canonical).
- **Operator-pinned target (tur 2 P0-2):** `ExpectedResolutionTarget` command'e taşınır; mutation
  lock altında expected↔current karşılaştırma (StaleResolutionTarget).
- **`NotPromotableFrom(Accepted)` mümkün (tur 3 P1-3):** `apply_resolution` step 3 Accepted candidate +
  wrong kind → `NotPromotableFrom(Accepted)`; status-aware split yanlış attribution engeller.
- **`BindingWrongKind` resolution-reachable (tur 3 P1-2):** `resolution_basis_view` Accepted gate
  sonrası (store.rs:1711); mapper'a dahil.
- **Minimal canonical preview V1 (tur 2 P0-2 + tur 3 P1-4):** target reveal Create/Reuse + entity
  id/digest; tek read motoru (ReviewQuery); rich diagnostic future-work.
- **Explicit colon-free target flags (tur 3 P0):** NodeId `CodeEntity:...` colon içerir → split
  kırılgan; `--target-outcome` value_enum + `--target-entity-id` + `--target-entity-digest`.

## PR #48 — ne yapıldı (bu oturumda)

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
| osp-core lib unit tests | 552 (PR C sonrası) |
| osp-cli unit tests | 121 (PR D sonrası 108 + 13 evidence_projection) |
| compile-fail cases (trybuild) | 26 (PR D eklemedi — osp-cli-only) |
| workspace cargo-test (osp-desktop hariç) | ~1001 passed |
| yeni evidence_projection unit tests | 13 (6 happy-path + 2 wiring proof + 5 defensive) |
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
- **osp-cli unit:** 26 → 41 (+15 preview builder: mutlu yol, self/already/incompatible-kind/family/
  non-current/cycle/lineage chain/consolidation/missing/closed-output invariant/node-limit
  closed-output regression/failing-writer render-abort/stage-aware confirmation+reason prompt-
  failure fail-closed).
- **osp-cli integration:** supersede_flow 20 (güncellenen — rich preview body) + review_flow 21
  (değişmedi) + **preview_flow 12** (yeni: mutlu yol text/json, incompatible, cycle, lineage chain,
  missing, ineligible exit 0, self, non-accepted, wizard ineligible, ineligible hide-transition,
  consolidation DAG edge-list).

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

## Durum Değerlendirmesi (2026-07-12 — PR E merge sonrası)

PR C + PR D + PR E tamamlandı (main `f68b2c6`). Bu bölüm tüm pending işleri, debt'leri ve
öncelik sıralamasını yazılı kayıt altına alır — yeni oturumda hiçbir şey kaybolmaması için.

### Tamamlanan milestone'lar (8 yüzey)
1. PR C (#58) — core axis-granular evidence model (`ObservedPhysicalMetrics`)
2. PR D (#59) — evidence projection + in-process wiring proof (`evidence_projection.rs`)
3. PR E (#60) — entity resolution core + persistence contract (`CodeIdentityKey` + `ResolvesTo` + INV-C16)

### Sıradaki işler (öncelik sıralı — PR E2 tamamlandı)

#### PR F — Evidence identity migration (en doğal devam)
- `ObservedCodeEvidence.code_entity_id` → `code_identity_key`; provider `CodeIdentityKey` merkezi;
  `CodeIdentityResolver` + `ResolvedCodeEvidenceProvider` node-facing adapter.
- E1-E8 evidence invariantları (evidence exactly one CodeIdentityKey; candidate+entity aynı evidence;
  resolution evidence kopyalamaz; duplicate YOK; resolver failure typed error; deterministic lookup;
  resolution öncesi/sonrası strength değişmez; node silinmesi ownership değiştirmez).
- **Bağımlılık:** PR E `CodeIdentityKey` hazır + PR E2 binding seeding (evidence resolved entity'ye
  bağlanır); PR F provider migration.

#### PR G — Lineage-aware effective projection
- `Concept → Candidate → Entity` derived `ImplementedBy` (read-only; tarihsel `ExpectedImplementation`
  korunur).
- **Bağımlılık:** PR E `ResolvesTo` edge + PR E2 CLI resolution surface (operator-promoted entity'ler)
  + PR F evidence migration.

#### PR E2 sonrası future-work (HANDOFF bullet'lerinden)
- **Rich diagnostic resolution preview:** lineage, multi-blocker list, identity collision açıklama
  grafiği, candidate→entity ilişki geçmişi, batch uygunluk raporu, alternatif target açıklamaları
  (supersede-preview pattern'inin zengin analogu). V1 minimal canonical preview (target reveal) kapandı.
- **Batch resolution V2:** `osp review resolve-code-entity --from-analysis` (tüm Accepted candidate'ları
  tek session'da resolve). Session-spanning lifetime.
- **Type-level policy mismatch garantisi:** `CanonicalCodeIdentity` hangi policy ile üretildiğini
  taşır veya identity + core key tek opaque projection result birlikte üretilir. Runtime drift guard
  yeterli; gerçek type-level garanti future-work.
- **Machine-readable CLI error envelope:** `operation` metadata taşıyan JSON envelope.

### Technical debt (kapsam dışı bırakılan, future cleanup)

#### `PhysicalCodeVector` unvalidated debt (PR C kapsamı dışı)
- Raw pub fields (NaN coupling enjekte edilebilir). PR C bunu dokunmadı; future cleanup.

#### `PhysicalCodeMetricAxis` placement note
- Canonical `predicate_lowering.rs`'te; neutral modüle taşıma future cleanup.

#### CLI→core dedup (PR D sonrası)
- `AxisSet`/`MetricAxisValue`/`MetricCoverage` → core `PhysicalAxisValue`/`EvidenceCoverage` adopt.
- `minimum_observed_strength` policy doc.

#### run-metadata.json frozen/current debt
- Stratum 22 vs `cumulative_trybuild_context` tutarsızlığı (frozen snapshot 22, current 28).
- Ayrı cleanup PR — PR D/E compile-fail eklemediği için JSON'a dokunmadı.

#### `measured_at` policy
- PR D `now_unix_secs()` fail-closed Result inject; future wall-clock source (NTP/system) policy.

### Paper / yayın pending

#### v1.4 pending paper edits
- Table C6 fixture adları (`c6_intent_cannot_form_observed_code_evidence` rename; yeni collection fixture'ları).
- trybuild 24→26 (PR C) → 28 (PR E) güncelleme.
- Evidence projection boundary (PR D) + entity resolution (PR E) Table'ları.
- INV-C16 runtime invariant (16 invariant; 13 type-enforced + 3 runtime-asserted C14/C15/C16).

#### arXiv v1.4
- v1.3 Zenodo'da canlı; v1.4 derive adayı. Epistemik çekirdek + CLI surface + evidence + entity resolution
  tamam. Endorsement hazır (Jimenez e-postası).

### Future milestone'lar (long-term)

#### Anchoring consumer gap
- Production consumer henüz yok — `AnchorPipeline::run_with_source` çağıran anchoring/ingest/evaluate
  CLI surface future work. PR D compatibility proof (in-crate unit test) seam çalıştığını kanıtlar.

#### Evidence persistence milestone
- `PersistedObservedCodeEvidence` schema version + validated restore + latest/history politikası
  + deterministic ordering + upsert/append semantics. PR D evidence production hazır (in-memory);
  persistence evidence zamanlar arasında güvenli taşır. `ObservedCodeEvidence` Deserialize VERİLMEZ.

#### EvidenceSource abstraction
- `fresh analysis` (PR D) → `validated persisted DTO` (evidence persistence). Consumer değişmez;
  provider'ı besleyen source değişir.

#### ObservedEntityRefresh
- Incremental store'da representation change audit transition (case-only rename →
  aynı NodeId, farklı canonical/digest). Supersede değil; `ObservedEntityRefresh`.

#### Structural relation projection (eski PR E — şimdi future)
- `Imports → ConceptEdge` — ama önce physical relation vs conceptual edge ontolojik sözleşme tasarımı.

### Test envanteri (current protocol)
- osp-core lib: 588 test
- osp-cli unit: 123 test
- compile-fail (trybuild): 28 (osp-core)
- workspace total: ~1039 (osp-desktop hariç)
- 0 regression; `RUSTFLAGS="-D warnings"` temiz.

---

## Sıradaki işler

### Evidence identity migration (PR F — sonraki milestone)
- `ObservedCodeEvidence.code_entity_id` → `code_identity_key`; provider `CodeIdentityKey` merkezi;
  `CodeIdentityResolver` + `ResolvedCodeEvidenceProvider` node-facing adapter. E1-E8 evidence invariantları.
- **Bağımlılık:** PR E `CodeIdentityKey` hazır + PR E2 binding seeding; PR F provider migration.

### Lineage-aware effective projection (PR G)
- `Concept → Candidate → Entity` derived `ImplementedBy` (read-only; tarihsel `ExpectedImplementation`
  korunur).
- **Bağımlılık:** PR E `ResolvesTo` edge + PR E2 CLI resolution surface + PR F evidence migration.

### PR E2 future-work (CLI scheme adoption sonrası)
- **Rich diagnostic resolution preview:** lineage, multi-blocker, collision graph (supersede-preview
  analogu). V1 minimal canonical preview kapandı.
- **Batch resolution V2:** `--from-analysis` (session-spanning).
- **Type-level policy mismatch garantisi:** `CanonicalCodeIdentity` policy taşır; runtime drift guard
  yeterli ama gerçek type-level future-work.

### Persistence milestone (evidence)
- `PersistedObservedCodeEvidence` schema version + validated restore + latest/history politikası
  + deterministic ordering + upsert/append semantics. PR D evidence production hazır (in-memory);
  persistence evidence zamanlar arasında güvenli taşır. `ObservedCodeEvidence` Deserialize VERİLMEZ.

### Anchoring consumer gap (future)
- Production consumer henüz yok — `AnchorPipeline::run_with_source` çağıran anchoring/ingest/evaluate
  CLI surface future work. PR D compatibility proof (in-crate unit test) seam çalıştığını kanıtlar.

### ObservedEntityRefresh (future)
- Incremental store'da representation change audit transition (case-only rename →
  aynı NodeId, farklı canonical/digest). Supersede değil; `ObservedEntityRefresh`.

## Entity resolution core + persistence contract (PR E) — ne yapıldı (bu dalda)

PR E — `CodeEntityCandidate ─ResolvesTo→ CodeEntity` identity resolution core contract + atomik
transition + snapshot persistence. **Dar V1:** identity-resolution core (3 tur plan review).
Ontolojik sözleşme: node identity ≠ physical code identity; ResolvesTo ≠ promotion; evidence
resolution ≠ evidence duplication (PR F).

### Mimari (3 tur plan review, implementation-ready)
- **`CodeIdentityKey` + `CodeIdentityScheme`:** case-policy scheme'in parçası (`AnalysisPathV1 { case_policy }`);
  smart constructor canonicalize (AsciiCaseInsensitive → to_ascii_lowercase); custom Deserialize
  (derive bypass YOK); deterministic `derive_resolved_code_entity_id` (FNV-1a, domain-separated).
- **`CodeIdentityBinding` store-owned:** ConceptNode alanı DEĞİL; `BTreeMap<ConceptNodeId, CodeIdentityKey>`;
  `seed_code_identity_bindings_trusted` bootstrap (node existence + kind + family + duplicate + R7).
- **`ConceptEdgeKind::ResolvesTo`:** 16. variant; high-stake (INV-C7 explanation zorunlu).
- **Status politikası:** source Accepted kalır; target Created=Candidate (otomatik mainline değil),
  Reused=existing live entity (`is_live_code_identity`); edge Accepted + explanation.
- **`ResolutionOutcome { Created, Reused }`:** basis-pinned outcome (`StaleResolutionTarget` —
  create→reuse sessiz dönüşüm YOK); N:1 cardinality (R7).
- **`CodeEntityResolutionSession`:** SupersedeSession mirror; `&mut self` resolve; opaque
  `ResolutionApplication` (private fields, no Deserialize); `ResolutionSessionSummary` + `close`.
- **`PresentedResolutionBasis::compile`:** Accepted candidate için ayrı compile yolu (mevcut
  PresentedBasis::compile Candidate-only); `ResolutionBasisView` + `ResolutionTargetView`.
- **`apply_resolution` 14-step:** lane-sensitive; basis match → candidate validation → freshness →
  binding → R6 → target recompute → basis-pinned match → reuse validation → collision → audit_seq →
  no-fallible mutation block. R1-R10 invariantları.
- **Created entity deterministic material:** canonical = key.canonical_key(), aliases = [].

### Persistence contract (tur 3 P1-1 gerçek envelope shape)
- `AnchorStoreSnapshot` v2: + `resolution_records` + `code_identity_bindings` (3-ledger audit_seq union).
- `ConceptGraphSnapshot::SCHEMA_VERSION = 1` additive (backward-compat).
- `PersistedStore::STORE_SCHEMA_VERSION: 1 → 2`; `PersistedStoreV1` + `AnchorStoreSnapshotV1` +
  `TryFrom<PersistedStoreV1>` explicit migration (revision preserved); header-based version dispatch.
- INV-C16 snapshot validation: binding validation + record endpoint + status forward + R2 key equality
  + R3/R4 kind + R5 family + R6 outgoing cardinality + R7 live entity + three-way triangulation +
  audit_seq density (3-ledger union) + INV-C7 explanation.

### Testler (0 regression)
- osp-core lib: 552 → 576 (+10 identity + 14 resolution: created/reused/failure/bootstrap/snapshot)
- osp-cli unit: 121 → 123 (+2 v1→v2 migration: revision preserved + header dispatch reject)
- compile-fail: 26 → 28 (c16_resolution_application literal + deserialize)
- Workspace total ~1027 (osp-desktop hariç); 0 regression.

### HANDOFF bullet'leri (PR E sonrası)
- **CLI scheme adoption gap:** PR E core canonical identity scheme ekler ama `graph init` node'ları
  otomatik binding taşımaz (PR E2 bridge adoption).
- **Evidence identity migration PR F'de:** `CodeIdentityKey` provider merkezi.
- **Lineage-aware projection PR G'de:** derived ImplementedBy (read-only).
- **CLI surface:** `osp review resolve-code-entity` PR E2 sonrası.
- **N:1 cardinality V1'de desteklenir (R7);** V1 test 1:1 + N:1 kapsar.
- **`ConceptGraphSnapshot::SCHEMA_VERSION = 1` additive;** format bump CLI envelope v2.

## Evidence projection + in-process wiring proof (PR D) — ne yapıldı (bu dalda)

PR D — CLI metric draft'larını (`ProjectedCodeMetric`) core evidence'a (`ObservedCodeEvidence` via
`ObservedPhysicalMetrics`) dönüştürür. Yeni `evidence_projection.rs` modülü — draft→evidence
conversion'ın **tek** sahibi. **Production path:** `graph init --analyze` evidence üretir +
diagnostics yazar (provider construct YOK — production consumer yok). **Compatibility proof:**
in-crate unit test evidence → provider → `ExpectedImplementation` scorer seam'ini kanıtlar
(review tur 5 P1 düzeltme: production `CodeEntityCandidate:` namespace; `ImplementedBy` gate
evidence presence entity-promotion/identity milestone'una kalır).

### Mimari (4 tur plan review sonucu, implementation-ready)
- **`evidence_projection.rs` tek conversion boundary:** `project_observed_evidence(metrics, context)`
  → `EvidenceProjectionOutput`. Source-scan ownership guard bunu doğrular (`ObservedPhysicalMetric::new`/
  `ObservedPhysicalMetrics::try_new`/`ObservedCodeEvidence::new` yalnız bu modülde).
- **Anti-corruption map:** CLI `PhysicalCodeAxis` → core `PhysicalCodeMetricAxis` (5 variant exhaustive;
  "adopt" DEĞİL — CLI enum korunur).
- **Newtype dönüşümü:** `MetricConfidence` → `EvidenceStrength` (InvalidStrength), `MetricCoverage` →
  `EvidenceCoverage` (InvalidCoverage), `MetricAxisValue.get()` → raw `f64` (duplicate validation YOK;
  core constructor kendi validation'ını yapar).
- **Zero coverage reject (tur 4 karar 3):** `coverage=0, strength>0` → `ZeroCoverage { node_id, axis }`.
  PR B confidence formülü (coverage içerir) + zero-confidence omission ile tutarsız → conversion reject.
- **`measured_at` inject:** `EvidenceProjectionContext { measured_at }` caller'dan; `project_analysis`
  wall-clock okumaz (temporal nondeterminism yalnız caller). `now_unix_secs() -> anyhow::Result<u64>`
  fail-closed (tur 3 P2).
- **Production vs compatibility ayrımı (tur 2 net sınır + tur 5 P1 düzeltme):**
  - Production: `graph init` evidence + diagnostics (`CodeEntityCandidate:` namespace; provider YOK).
  - Compatibility proof: in-crate unit test — `ExpectedImplementation` scorer seam (production
    `CodeEntityCandidate:` ID + provider → `code_evidence_score > 0`). `ImplementedBy` gate evidence
    presence **entity-promotion/identity milestone'una kalır** (CodeEntityCandidate → CodeEntity
    transition gerekir; prefix değişikliği R1 tek-kimlik yaklaşımını deler).
- **Report input yüzeyiyle uyumlu:** `input_metric_values`/`evidence_objects_created`/`partial_evidence_objects`
  (distinct_nodes/empty-skip YOK — input yalnız emit edilmiş metric'leri görür).

### Typed error model
`EvidenceProjectionError`: InvalidStrength / ZeroCoverage / InvalidCoverage / InvalidObservation /
InvalidCollection. Node/axis context korunur (anyhow YOK). `BridgeError::EvidenceProjection` sarar.

### Guard matrisi (tur 3 ownership guard)
- metric_projection.rs deny korunur (ObservedCodeEvidence/PhysicalCodeVector YOK).
- **Yeni ownership guard:** core evidence construction token'ları yalnız evidence_projection.rs'de
  (`std::fs` recursive, yeni dep YOK).

### Testler (0 regression)
- osp-cli unit: 108 → 121 (+13 evidence_projection: 6 happy-path + 2 ExpectedImplementation scorer
  seam compatibility proof + 5 defensive contract-drift)
- İki factory: validated (`projected_metric_for_tests`) happy-path + unchecked forged
  (`projected_metric_unchecked_for_contract_tests`) defensive testler.
- Workspace total ~1001 (osp-desktop hariç); 0 regression.

### Persistence KAPSAM DIŞI
PR D evidence production + in-memory provider wiring tamamlar. Store'a persist EDİLMEZ —
`ObservedCodeEvidence` Serialize-only (PR C); persistence kendi restore modelini gerektirir (PR G).
Stderr dürüst: "Evidence runtime consumer: none in graph init" + "Evidence persistence: disabled".

### HANDOFF bullet'leri (PR D sonrası)
- **Entity-promotion/identity milestone (review tur 5 P1):** production bridge `CodeEntityCandidate:<path>`
  üretir; `ImplementedBy` gate `CodeEntity:<name>` (operator-promoted) arar. `CodeEntityCandidate → CodeEntity`
  identity transition/mapping sözleşmesi gerekir (prefix değişikliği R1 tek-kimlik yaklaşımını deler).
  PR D `ExpectedImplementation` scorer seam'i kanıtlar; `ImplementedBy` gate evidence presence bu
  milestone'ın sonrası.
- **Anchoring consumer gap:** production consumer (`AnchorPipeline::run_with_source` çağıran CLI surface)
  henüz yok. Compatibility proof seam çalıştığını kanıtlar; production consumer ayrı milestone.
- **Persistence milestone (PR G):** `PersistedObservedCodeEvidence` DTO + `try_restore()` + schema
  version + latest/history + deterministic ordering + upsert/append + snapshot integration + corruption
  tests. `ObservedCodeEvidence` Deserialize VERİLMEZ.
- **EvidenceSource abstraction (future):** `fresh analysis` (PR D) → `validated persisted DTO` (PR G).
  Consumer değişmez; provider'ı besleyen source değişir.
- **`measured_at` policy:** PR D `now_unix_secs()` fail-closed Result inject; PR G wall-clock source policy.
- **run-metadata.json frozen/current debt:** stratum 22 vs cumulative_trybuild_context 26 tutarsızlığı
  ayrı cleanup PR (tur 3 P3-10).

## Core axis-granular evidence model (PR C) — ne yapıldı (bu dalda)

PR C — `ObservedCodeEvidence` axis-granular observation taşır (tek `PhysicalCodeVector` + tek
`confidence` yerine). INV-C6 güçlenme: zero-strength reject "strength=0 evidence" temsil edilemez
kılar; gate/scorer ayrımı korunur ama korunan kenar durum yok.

### Mimari (4 tur plan review sonucu, implementation-ready)
- **Uniform [0,1] newtype'lar:** `PhysicalAxisValue` + `EvidenceCoverage` + `MetricScalarViolation`
  (NonFinite/BelowMinimum/AboveMaximum). `PhysicalAxisValue::new(value)` axis parametresi YOK —
  axis context `ObservedPhysicalMetricError::InvalidValue { axis, value, violation }` seviyesinde.
  **Plan sapması (R1 review notu):** plan metninde bu skalar newtype'lar Serialize-only
  olarak tasarlanmıştı; implementasyon `NormalizedMetricThreshold` desenini izleyerek **validating
  custom Deserialize** ekledi. Bilinçli iyileştirme — skalar deserialize constructor'dan geçer
  (range-dışı forged edilemez), asıl INV-C6 sınırı (metric/koleksiyon/evidence Deserialize'sız)
  korunur ve yeni `c6_observed_physical_metrics_deserialize` fixture'ı bunu kanıtlar.
- **`PhysicalCodeMetricAxis` reuse:** mevcut enum (predicate_lowering.rs:113 canonical) + `sort_order()`.
  İkinci enum YOK. (Placement note: neutral modüle taşıma future cleanup.)
- **`ObservedPhysicalMetric` (private fields):** `new(axis, value, source, strength, coverage) → Result`.
  value [0,1] validation + strength > 0 (ZeroStrength { axis } reject).
- **`ObservedPhysicalMetrics` (private `Vec`):** `try_new` non-empty + unique-axis + deterministic
  sort_order. `minimum_observed_strength()` normative min-over-axes (coverage katılmaz — upstream
  confidence zaten coverage içerir; double-counting engeli). Missing axes are absent, not zero-strength.
- **`try_to_physical_vector`:** all-5-axes → Ok; missing → `IncompletePhysicalVector { missing }`
  (zero-fill YOK; missing deterministik sort_order).
- **`ObservedCodeEvidence` refactor:** `observations: ObservedPhysicalMetrics` (was: physical_vector +
  metric_source + confidence). Constructor `new(id, observations, time)`.
- **`PhysicalCodeVector` + `PositionVector` unchanged** (PR C kapsamı dışı — unvalidated debt).

### Not 5 güçlenme cümlesi
Önceki modelde "evidence object var, `confidence=0`" temsil edilebiliyordu ve gate (object presence) /
scorer (strength > 0) ayrımı bu kenar duruma dayanıyordu. PR C axis-granular modeli zero-strength reject
uygular (`ObservedPhysicalMetric::new` strength=0 → error), bu yüzden "strength=0 evidence" artık oluşamaz.
Gate hâlâ object presence kontrolü yapar, scorer hâlâ `minimum_observed_strength()` skalarını kullanır;
ama korunmuş kenar durum ortadan kalktı — gate/scorer ayrımı korunur, korumaya gerek kalmaz.

### Provider migration (code_evidence.rs)
`evidence_strength` artık `ev.observations().minimum_observed_strength()`. Gate unchanged (presence check).
Scorer unchanged (scalar). API unchanged. Test migration: 8 construction site (3 değer seti —
entropy/witness representative normalized 1.1/5.0 raw → 0.52/0.68; witness 9.0→0.9 soft-norm).

### Compile-fail (24 → 26, .stderr lifecycle)
- `c6_observed_evidence_literal.rs` — field rename (physical_vector → observations); ad korunur + `.stderr` update.
- `c6_intent_carries_physical_vector.rs` → rename `c6_intent_cannot_form_observed_code_evidence.rs` + `.stderr` rename + delete orphan.
- **Yeni:** `c6_observed_physical_metrics_literal.rs` + `.stderr` (collection literal construct engelli).
- **Yeni:** `c6_observed_physical_metrics_deserialize.rs` + `.stderr` (collection serde boundary).

### Testler (0 regression)
- osp-core lib: 538 → 552 (+14 axis-granular evidence model unit testleri)
- compile-fail: 24 → 26 (+2 collection boundary)
- Workspace total ~987 (osp-desktop hariç); 0 regression.

### PR D dedup listesi (PR C sonrası)
- `PhysicalCodeMetricAxis` reuse (canonical predicate_lowering.rs).
- CLI→core adopt: `AxisSet`/`MetricAxisValue`/`MetricCoverage` → core `PhysicalAxisValue`/`EvidenceCoverage`.
- `minimum_observed_strength` policy doc.
- `PhysicalCodeVector` unvalidated debt: raw pub fields (NaN coupling enjekte edilebilir) — PR C kapsamı dışı.

### v1.4 pending paper edits
- Table C6 fixture adları (`c6_intent_cannot_form_observed_code_evidence` rename; yeni collection fixture'ları).
- trybuild 24→26.

## CLI `osp graph init --analyze` metric projection (PR B) — ne yapıldı (bu dalda)

Analysis metric projection — axis-granular metric draft (NOT core evidence). PR A node
identity projection'a metric projection eklendi.

### Mimari (4 tur plan review sonucu)
- **R1 tek türetim:** `CodeEntityCandidate` pre-derived `ConceptNodeId` taşır; `into_drafts(self)`
  scheme almaz. `AnalysisProjectionIndex` (NodeId→ConceptNodeId) `project_candidate_nodes` içinde
  üretilir; `project_code_metrics` tüketir (scheme/policy YOK).
- **C1 doğrulama sırası:** value → confidence → coverage doğrulama source admission'dan ÖNCE.
  Placeholder + NaN → InvalidMetric error (sessiz skip YOK).
- **C3 validated newtype'lar:** MetricAxisValue/MetricConfidence/MetricCoverage — type invariant.
- **AxisSet(u8) bitset:** 5-elemanlı sabit alan, BTreeSet/Ord gerektirmez.
- **INV-C6:** core'un tam evidence/vector tipi ÜRETİLMEZ (entropy/witness_depth üretilmez,
  zero-fill YOK). Source-scan CI guard (N1 dosya disiplini).

### N2 sözleşme cümlesi
PR B sonrası `--analyze`, kullanılmayan metrik çıktısı için bile metrik geçerliliğine
bağıdır (tutarlılık > kullanılabilirlik).

### Yeni dosyalar
- **`metric_projection.rs`** — PhysicalCodeAxis + AxisSet + MetricAxisValue/Confidence/Coverage
  newtype + ProjectedCodeMetric (private) + project_code_metrics (C1 doğrulama sırası).
  16 unit test.
- **`tests/architecture_guards.rs`** — metric_projection.rs'te tam evidence/vector adları
  yorumda bile yok (C2+N1 source-scan).

### Model uyuşmazlığı notu
Mevcut `ObservedCodeEvidence` (5-axis zorunlu PhysicalCodeVector) ile analyzer (3 axis)
arasında uyuşmazlık. PR B projection seam'de durur (metric draft, evidence değil);
PR C core axis-granular evidence model.

### Çoktan bire normatif
PR A many-to-one identity collision, PR B'de metric aggregation'a dönüştürülmez;
aynı (ConceptNodeId, axis) → DuplicateProjectedAxis error.

### Testler (0 regression)
- osp-cli unit: 92 → 108 (+16 metric_projection)
- architecture_guards: 1 (yeni); analyze_bridge_flow: 9 (metric summary assertions)

## CLI `osp graph init --analyze` — ne yapıldı (PR A dalında)

Analysis → candidate bridge (PR A) — HANDOFF "Sıradaki işler" milestone kapandı.
Analysis `Module` node'ları → `CodeEntityCandidate` ConceptNode (Candidate lane, INV-C5/INV-C2).

### Mimari (6 tur plan review sonucu)
- **İki ayrı source modeli + ortak GraphSeedBuilder:** analysis identity-only
  `AnalysisCandidateSeed`, legacy JSON mevcut semantics; ortak builder graph invariant.
- **Identity-durum sözleşmesi (F-yeni):** NodeId(identity_key)=kalıcı kimlik,
  canonical(display_path)=gözlemlenen yazım, NodeDigest=freshness özeti.
  Case-only rename → aynı NodeId, farklı canonical/digest (INV-C12 muhafazakâr).
- **Typed AnalysisIdentityScheme::PathV1 (O2'):** NodeId derivation scheme üzerinden.
- **One-shot GraphSeedBuilder::build (B1):** partial GraphSeed imkânsız.
- **Builder source-order preservation (O1):** ordering source modellerinin sorumluluğu.
- **GraphSeedNodeDraft private constructors (O3):** INV-C5 constructor sınırında.

### Yeni dosyalar (`crates/osp-cli/src/`)
- **`canonical_identity.rs`** — CanonicalCodeIdentity (display_path/identity_key ayrımı),
  PathCasePolicy (CaseSensitive/AsciiCaseInsensitive), lexical normalizasyon (absolute/
  UNC/drive/trailing-dot reject). 27 unit test.
- **`analysis_bridge.rs`** — AnalysisIdentityScheme, CodeEntityCandidate (identity-only),
  AnalysisCandidateSeed (try_new dedup/collision), project_analysis, BridgeRunReport
  (semantic seed DIŞI, stderr, deterministik). 12 unit test.
- **`graph_seed_builder.rs`** — GraphSeedNodeDraft (private constructors), one-shot
  GraphSeedBuilder (DuplicateNode/NodeIdCollision). 8 unit test.
- **`seed_file.rs`** — `to_graph_seed()` → `into_drafts() + GraphSeedBuilder::build()`
  refactor (F1 legacy compat, frozen characterization yeşil).
- **`commands/graph.rs`** — iki-source init (`--seed`/`--analyze`), Clap ArgGroup,
  typed PathCaseArg ValueEnum, `--path-case`/`--scip` analyze-only, empty warning,
  pre-validation non-destructive.

### Identity-durum sözleşmesi (F-yeni invariant)
`ConceptNodeId` (identity_key, AnalysisIdentityScheme+policy'ye bağlı) = kalıcı entity
kimliği. `canonical` (display_path) = gözlemlenen mevcut repository spelling. `NodeDigest`
= canonical dahil mevcut temsil/freshness özeti. Case-only rename → aynı NodeId, farklı
canonical/digest = INV-C12 muhafazakâr (StaleBasis doğru). Supersession değil representation
refresh (aynı NodeId kendini supersede edemez).

**AnalysisIdentityScheme identity şemasının parçası (O2'):** PathV2 gelirse NodeId algoritması
görünür değişir. Bu PR store'da saklamaz; BridgeRunReport'ta görünür. Future debt: incremental
analysis store metadata'nda scheme+policy saklamalı.

### Kabul kriterleri (21)
INV-C5 (Candidate only), INV-C2 (PhysicalCode analysis, ConceptualIntent legacy F1),
identity-only projection (classification/role graph'a sızmaz — M1), MissingNodePath typed
error (I3), Windows drive-relative/trailing-dot reject (I4/O4'), empty analysis (I7 — library
kabul, CLI warning), DuplicateCanonical vs CaseCollision (O5), bit-equivalent determinism,
INV-C5 negatif test (Accepted üretilemez), NodeId identity_keyden (F-yeni), one-shot builder
(B1), source-order preservation (O1), legacy semantics-identical (F1 frozen characterization).

### Testler (0 regression)
- **osp-cli unit:** 42 → 92 (+50: 27 canonical_identity + 12 analysis_bridge +
  8 graph_seed_builder + 4 characterization).
- **osp-cli integration:** analyze_bridge_flow 8 (yeni) + review_flow 21 + supersede_flow 20
  + preview_flow 12 (değişmedi).
- **osp-core lib:** 538 (değişmedi).

### Future debt
- **O6' hardening:** mevcut `to_graph_seed()` zaten fail-closed (duplicate canonical).
  GraphSeedBuilder NodeIdCollision ek hardening getirir ama mevcut davranışla çelişmiyor
  (canonical dedup önce yakalar).
- **Tek-repository store invariant (I5):** bu PR'da analysis-generated store tek repository
  kapsamı; cross-repository birleştirme desteklenmez (NodeId = kind+identity_key, namespace yok).

### Diğer
- **TUI v2:** dialoguer/rustyline, fuzzy, renk (v1 stdio yeterli).
- **Snapshot content-digest** (v2): elle JSON düzenleme tahrifatı için.
- **arXiv:** v1.3 epistemik çekirdek + CLI accept/reject/supersede/rich-preview surface + analysis bridge tamam; v1.4 derive adayı.
- **Preview↔production primary-sebep hizalaması** (v2, future work): `primary_structural_blocker` sırası `apply_supersede` structural steps 5–10'a dizilir ama production session path (compile precheck currentness) daha erken dönebilir; characterization production-path reddetme sırasına karşı future work.

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
| `crates/osp-cli/src/commands/{graph,review}.rs` | **`osp graph init/status/validate` + `osp review list/show/accept/reject/session`** + **PR E2: `osp review resolve-code-entity` + `resolve-code-entity-preview` (explicit target flags)** |
| `crates/osp-cli/src/review_session.rs` | **interactive wizard (generic R/W)** + **PR E2: `resolve <candidate>` komutu** |
| `crates/osp-cli/src/identity_bridge.rs` | **PR E2: `AnalysisIdentityContext` + `to_core_identity_key` + `From<PathCasePolicy>` (CLI ↔ core mapping)** |
| `crates/osp-cli/src/commands/resolve_code_entity_preview_render.rs` | **PR E2: body-only renderer (üç yüzey tek renderer)** |
| `crates/osp-cli/tests/review_flow.rs` | **11 integration test (stale basis, restart-safe, operator, corrupt, canonical)** |
| `crates/osp-cli/tests/resolution_flow.rs` | **PR E2: 9 integration test (Created mutlu yol, stale basis, not accepted, JSON, preview)** |
| `crates/osp-mcp/tests/inv_c11_agent_surface.rs` | **INV-C11 agent-surface regression (static source scan)** |

## Kullanıcıya not

- **osp-desktop kırık** (PR #40 sonrası API drift: `compute_raw_from_delta` 4 argüman, `Claim`
  `removed_edges`+`task_id` gerektiriyor). CI zaten hariç tutuyor (Tauri webkit bağımlılıkları).
  Ayrı PR adayı — Faz 8b dışı.
- **mainline_query deterministik sıralama** — küçük PR adayı (agent-facing context tekrarlanabilirliği).
- **arXiv** 1 hafta ertelendi; Jimenez e-postası hazır (favorilerde, docs'ta değil).
- **4 DOI canlı:** P1/P2/P3/pack tüm Zenodo'da.

## Commit durumu

✅ **Faz 8b + CLI `osp review` (accept/reject/supersede/resolve-code-entity) + rich SupersedePreview + Analysis bridge + Metric projection + PR C axis-granular evidence + PR D evidence projection + PR E entity resolution core + PR E2 CLI scheme adoption TAMAM.**
- main: `06d3a02` (PR E merged — entity resolution core + persistence contract).
- PR E2: `feat/cli-scheme-adoption` dalı (main `06d3a02` üstünde) — CLI scheme adoption (graph init binding seeding + resolve-code-entity surface + minimal canonical preview); 3 tur plan review implementation-ready → implementasyon tamam.
- PR #48-51 (epistemik çekirdek); PR #52 (stale cleanup); PR #53 (CLI accept/reject); PR #54 (CLI supersession); PR #55 (rich SupersedePreview); PR #56 (analysis bridge); PR #57 (metric projection); **PR C (axis-granular evidence model)**; **PR D (evidence projection + wiring proof)**; **PR E (entity resolution core)**; **PR E2 (CLI scheme adoption)**.
- **osp-core 588 lib** + **osp-cli 144 unit** + **28 compile-fail** + **21 review_flow + 20 supersede_flow + 12 preview_flow + 13 analyze_bridge_flow + 9 resolution_flow + 2 architecture_guards** + **osp-mcp +2 INV-C11** yeşil (`RUSTFLAGS="-D warnings"` temiz).

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
