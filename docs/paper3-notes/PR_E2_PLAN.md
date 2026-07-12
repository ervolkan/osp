# PR E2 Plan — CLI scheme adoption (graph init binding + resolve-code-entity) (yeni oturum için)

> **Dal:** `feat/cli-scheme-adoption` (main `06d3a02` üstünde — PR E merged)
> **Scope:** `osp-cli` (graph init bridge + resolve-code-entity surface); `osp-core` untouched (PR E contract hazır)
> **Tur 3 implementation-ready (1 P0 + 5 P1 + 4 P2 tur 2 review düzeltmesi işlendi)**

## Tur 3 review kararları (1 P0 bloklayıcı + 5 P1 zorunlu + 4 P2 iyileştirme)

Tur 2 ana mimariyi onayladı (candidate_query düzeltmesi, target pinning, validated/sorted binding,
minimal preview V1, per-command session, NotFound reuse). Fakat **uygulamaya geçmeden önce** bir
P0 CLI sözleşme problemi ve beş P1 derlenebilirlik/error-mapping problemi kaldı. Tüm bulgular core
koda karşı doğrulandı.

### P0 — Non-interactive target argüman sözleşmesi eksik + exact pinning ile çelişiyor (bloklayıcı)

**Bulgu:** Tur 2 `ReviewResolveCodeEntityArgs` yalnız `candidate_digest: Option<String>` içeriyor
fakat `parse_target_flag(&args)` çağrılıyor — struct'ta target alanı YOK. Derlenebilir CLI sözleşmesi
yok. Ek olarak tur 2'nin `--target create[:entity_id]` / `--target reuse:entity_id:digest_hex`
formatı iki problem taşıyor:
1. **Create target ID opsiyonel olamaz** — exact pinning gereği `proposed_entity_id` zorunlu;
   `--target create` yetersiz.
2. **Colon-delimited format NodeId ile çakışıyor** — `CodeEntity:a1b2c3d4...` zaten colon içerir;
   `reuse:CodeEntity:a1b2c3d4:98cd...` parse açısından kırılgan (`split(':')` kullanılamaz).

**Çözüm:** explicit ayrı flag'ler (colon-free):
```rust
#[arg(long, value_enum)] pub target_outcome: Option<ResolutionTargetOutcomeArg>,  // Create/Reuse
#[arg(long)]               pub target_entity_id: Option<String>,                  // NodeId (colon-safe)
#[arg(long)]               pub target_entity_digest: Option<String>,              // hex
```
Validation matrisi: Create → entity_id zorunlu, digest verilmemeli; Reuse → ikisi de zorunlu.

### P1-1 — Dört doğrudan derleme problemi (zorunlu)

**A. `IdentityBridgeError` için `Eq` derive edilemez:** core `CodeIdentityKeyError` yalnız
`#[derive(Debug, Clone, PartialEq)]` (Eq YOK). Core untouched → CLI enum'unda `Eq` derleme hatası.
**Düzeltme:** `#[derive(Debug, Clone, PartialEq)]` (Eq çıkartılır — zaten gerekli değil).

**B. `ResolutionTargetPreview` serialize edilemiyor:** outer `ResolutionPreviewOutput`
`#[derive(Serialize)]` ama enum `Serialize` değil → derive hatası.
**Düzeltme:** enum'a `#[derive(Serialize)]` + `#[serde(tag = "outcome", rename_all = "snake_case")]`.

**C. `to_expected_target()` sonucu açılmamış:** fonksiyon `Result` döner ama `let expected_target =
preview.to_expected_target();` açılmıyor → tuple tipiyle uyuşmaz. **Düzeltme:** `?` ile aç, veya
preview modelini infallible yap (P2-previewsadeleştirme).

**D. `parse_target_flag(&args)` argüman eksik:** P0 sözleşme problemine bağlı. `parse_expected_target(&args)`
ile explicit target flag'leri kullanılır.

### P1-2 — `BindingWrongKind` resolution sırasında reachable (zorunlu)

**Bulgu + doğrulama:** Tur 2 `BindingWrongKind`'ı "seeding-only" kabul edip mapper'dan çıkardı.
Fakat `resolution_basis_view` (store.rs:1711-1715, Accepted gate SONRASI) açıkça
`StoreError::BindingWrongKind { kind }` döner — Accepted candidate + `node_kind != CodeEntityCandidate`
durumunda. Resolution compile yolunda reachable.

**Doğrulama sonucu (önemli nüans):** `apply_resolution` step (3) (store.rs:1534-1537) wrong-kind
için `NotPromotableFrom(candidate.decision_status)` kullanır (BindingWrongKind DEĞİL). Ama
`resolution_basis_view` Accepted gate SONRASI `BindingWrongKind` döner. **İki fonksiyon asymmetrik.**
CLI `compile` → `resolution_basis_view` çağırır → `BindingWrongKind` reachable.

**Çözüm:** mapper `BindingWrongKind`'ı içerir:
```rust
SE::BindingWrongKind { kind } => ReviewError::NotPromotable(
    format!("candidate kind is {kind:?}; expected CodeEntityCandidate")),
```

### P1-3 — `CandidateNotAccepted` varyantı mevcut mapper'da kullanılmıyor (zorunlu)

**Bulgu + doğrulama:** Tur 2 `CandidateNotAccepted` CLI varyantı ekledi ama compile yolundaki
non-Accepted candidate core'dan `ResolutionError::Store(StoreError::NotPromotableFrom(status))`
olarak gelir (ResolutionError'ın doğrudan CandidateNotAccepted varyantı DEĞİL — o apply_resolution
session yolunda). Mevcut mapper `NotPromotableFrom(status)` → `ReviewError::NotPromotable` map'ler;
`CandidateNotAccepted` kullanılmaz.

**Doğrulama sonucu (önemli nüans):** `NotPromotableFrom(Accepted)` mümkün — `apply_resolution`
step (3) (store.rs:1535) Accepted candidate + wrong kind durumunda `NotPromotableFrom(Accepted)`
döner. Bu durumda "not accepted" mesajı yanlış olur.

**Çözüm:** store mapper da candidate context alır + status-aware split:
```rust
fn map_resolution_store_error(
    candidate_id: &ConceptNodeId,
    source: Box<dyn Error + Send + Sync>,
) -> ReviewError

SE::NotPromotableFrom(status) if *status != DecisionStatus::Accepted => ReviewError::CandidateNotAccepted {
    id: candidate_id.0.clone(),
    status: format!("{status:?}"),
},
SE::NotPromotableFrom(status) => ReviewError::NotPromotable(
    format!("accepted node is not structurally eligible for resolution (status={status:?})")),
```
Candidate/Rejected/Deprecated → `CandidateNotAccepted`; Accepted + wrong structural → `NotPromotable`.
İkinci kol `NotPromotableFrom(Accepted)` yanlış attributionsunu engeller.

### P1-4 — Preview `ReviewQuery` mimarisine dahil edilmeli (zorunlu)

**Bulgu:** Tur 2 `execute_resolve_code_entity_preview` yönteminden söz etti ama `ReviewQuery` /
`ReviewReadOutput` genişletmesini tanımlamadı. Mevcut application service sözleşmesi: `read_validated_store()`
tek read, `execute_query()` tüm read-only yüzeylerin tek motoru (store + revision aynı read'den).

**Çözüm:**
```rust
pub enum ReviewQuery {
    // ... mevcut ...
    ResolveCodeEntityPreview { candidate: ConceptNodeId },
}
pub enum ReviewReadOutput {
    // ... mevcut ...
    ResolveCodeEntityPreview(ResolutionPreviewOutput),
}
```
`build_resolve_code_entity_preview(&store, &candidate, revision)` tek read'ten tüm preview alanlarını
üretir. Convenience method `execute_query()` sarmalar. Ayrı store okuması YOK.

Kabul kriterine ek: "Resolution preview candidate, identity key, target ve revision'ı tek
`read_validated_store()` sonucundan üretir; ayrı store okumaları yapmaz."

### P1-5 — Wizard reason sırası informed-review sözleşmesini bozuyor (zorunlu)

**Bulgu:** Tur 2 wizard `resolve <candidate> <reason>` komutu önerdi — operator reason'u exact target
preview görmeden önce yazmış olur. Mevcut wizard sözleşmesi: basis göster → confirmation → reason sor →
mutation ("operator görmediği basis'e gerekçe yazmasın").

**Çözüm:** wizard komutu `resolve <candidate>` (reason yok); akış: minimal preview → exact target
confirmation → reason prompt → mutation (preview'dan digest + expected target ile).
Tur 1 review karar #3 (per-command session) korunur; wizard UX sözleşmesi hizalanır.

### P2-1 — "Policy mismatch type-level engellenir" iddiası daraltılacak

**Bulgu:** `AnalysisIdentityContext` alanları public + `CanonicalCodeIdentity::new(path, policy)` ayrı
policy alır + drift test forged context kurar → mismatch type-level imkânsız DEĞİL; runtime fail-closed
yakalanır.

**Düzeltme:** iddia daraltılır — "Scheme + policy tek propagation value altında gruplanır; accidental
parameter divergence azaltılır. `CanonicalizationDrift` runtime guard'ı kalan mismatch'i fail-closed
yakalar." Context alanları private + constructor eklenir; nihai koruma runtime drift check kalır.

### P2-2 — `reuse→create` target drift testi core policy ile uyumsuz

**Bulgu + doğrulama:** Core policy (`compute_resolution_target`, store.rs:838-845): inactive entity
Create'e düşmez → `EntityNotLiveForResolution`. Geçerli transition'larla Reuse→Create drift oluşmaz.
`resolve_rejects_reuse_to_create_target_drift` testi imkânsız.

**Düzeltme:** test seti revize:
```rust
resolve_rejects_create_to_reuse_target_drift            # confirmation Create, mutation Reuse (başka süreç live entity oluşturur)
resolve_rejects_reuse_entity_digest_drift               # same entity_id, different entity_digest (content drift)
resolve_rejects_reuse_target_becoming_inactive          # confirmation Reuse, mutation-time EntityNotLiveForResolution
preview_and_mutation_use_same_expected_target
```
Ek doğrulama: `EntityNotLiveForResolution` `ResolutionError` (SessionError yolundan) veya `StoreError`
olarak gelebilir; her iki yol da map'lenmeli.

### P2-3 — Test sayısı 44 değil 48

**Bulgu:** Grup toplamları 8+4+5+4+10+3+4+4+6 = 48; plan `+44` diyor.
**Düzeltme:** başlık `~48`, run-metadata `+48`. Ek not: "48 planned assertions/tests; net-new test
function count implementation sırasında netleşir."

### P2-4 — Text output enum'u `Debug` ile yazmamalı

**Bulgu:** `println!("... ({:?}, ...)", output.mutation.outcome)` text'te `Created`/`Reused` (PascalCase)
üretir; JSON `created`/`reused` (snake_case). Terminoloji hizasız.
**Düzeltme:** `ResolutionOutcomeView::as_str()` → `"created"`/`"reused"`; text output `as_str()` kullanır.

### Preview modeli sadeleştirme (tur 3 ek — review önerisi)

`to_expected_target()` `Result` döner (hex parse). Supersede preview benzeri infallible yapılabilir:
```rust
pub fn expected_target(&self) -> ExpectedResolutionTarget
```
`entity_digest_hex` application'ın kendi `{:016x}` çıktısı → `.expect("...")` infallible. Application
read model'in `anyhow` bağımlılığı kalmaz; `anyhow` yalnız CLI adapter'da. Tur 3'te adoption.

## Özet

## Tur 2 review kararları (2 P0 bloklayıcı + 4 P1 zorunlu + 2 P2 iyileştirme)

Tur 1 planı implementation-ready değildi; iki bloklayıcı mimari problem ve sözleşme açıkları vardı.
Tüm bulgular core koda karşı doğrulandı (explore agent — `candidate_query` filter, `ResolutionError`
varyant shape'leri, `apply_resolution` reachable `StoreError` seti, `resolution_basis_view` Accepted
gate). Doğrulama sonuçları tur 2 düzeltmelerine yansıdı.

### P0-1 — `candidate_query()` Accepted candidate bulamaz (bloklayıcı)

**Bulgu:** Tur 1 `apply_resolution` taslağı `store.candidate_query()` üzerinden candidate arıyordu.
`candidate_query` **yalnız `DecisionStatus::Candidate`** döndürür (store.rs:1786-1797 — `matches!(n.decision_status, DecisionStatus::Candidate)`). Accepted `CodeEntityCandidate` bu sorgudan dışarıdadır → mutlu yol daima `NotFound`.

**Çözüm:** `apply_resolution` `candidate_query()` KULLANMAZ. Tek yol `PresentedResolutionBasis::compile(store, &candidate)` — bu `resolution_basis_view` (store.rs:1696-1728) çağırır, ki o:
1. `graph.node(candidate)` ile candidate'ı status'tan bağımsız bulur (NodeNotFound aksi).
2. `decision_status != Accepted → NotPromotableFrom` (store.rs:1707 — Accepted gate canonical).
3. `node_kind != CodeEntityCandidate → BindingWrongKind` (R3 store-side defense).
4. `code_identity_bindings.get(candidate) → MissingResolutionIdentityBinding` (tur 3 P1-A).
5. `compute_resolution_target` → Create/Reuse target (store policy).

Yani Accepted gate, kind check, binding check, target computation **tek `compile` çağrısında** canonical.
CLI erken `candidate_query()` kontrolü YOK — tüm domain doğrulaması `compile` + store transition.

**Digest kontrol sırası (tur 2 düzeltme):** `compile` basis'i üretir → `basis.candidate_digest()`
↔ `cmd.expected_candidate_digest` karşılaştır → mismatch `StaleResolutionBasis`. `compile` öncesi
değil SONRASI (compile başarılı → digest comparison; compile başarısız → typed error map).

### P0-2 — Target pinning operator presentation sınırına taşınmadı (bloklayıcı)

**Bulgu:** Tur 1 yalnız candidate digest pinliyordu. Core `StaleResolutionTarget` yalnız "compile sonrası
apply'e kadar" drift yakalar; **operator confirmation → mutation-time compile arası** drift'i yakalamaz.

Senaryo: confirmation `Create(CodeEntity:X)` gösterir → başka süreç aynı identity için live entity oluşturur
→ candidate unchanged → candidate digest aynı → mutation lock → basis RE-compile → `Reuse(CodeEntity:X)`
→ operator ek onay olmadan uygulanır.

**Çözüm:** `ResolveCodeEntityCommand` candidate digest + **expected target** taşır:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedResolutionTarget {
    Create { proposed_entity_id: ConceptNodeId },
    Reuse { entity_id: ConceptNodeId, entity_digest: NodeDigest },
}

pub struct ResolveCodeEntityCommand {
    pub candidate: ConceptNodeId,
    pub expected_candidate_digest: NodeDigest,
    pub expected_target: ExpectedResolutionTarget,   // YENİ (tur 2 P0-2)
    pub reason: String,
}
```

Mutation lock altında:
```rust
basis.candidate_digest() == cmd.expected_candidate_digest  // mismatch → StaleResolutionBasis
basis.target() == cmd.expected_target                       // mismatch → StaleResolutionTarget
```

Böylece CLI, core target-pinning semantiğini **operator presentation sınırına** taşır. Confirmation
operator'e tam target'ı gösterir (Create proposed_entity_id / Reuse entity_id + digest).

**Minimal canonical preview V1'de zorunlu (tur 2 P0-2 doğal sonucu):** target reveal confirmation'ın
parçası. Rich diagnostic preview (lineage, blockers) future-work.

### P1-A — `map_resolution_error` context taşımıyor (zorunlu)

**Bulgu:** Tur 1 imzası `fn map_resolution_error(e: ResolutionError) -> ReviewError`. Fakat core'un
bazı varyantları ID taşımıyor: `MissingIdentityBinding` (unit), `AlreadyResolved` (unit),
`CandidateNotAccepted { current }` (status var, id YOK), `WrongCandidateKind` (unit), `WrongFamily`
(unit). CLI varyantları ID istiyor (`AlreadyResolved(String)`, `MissingIdentityBinding(String)`,
`CandidateNotAccepted { id, status }`).

Ek olarak `CandidateNotFound(ConceptNodeId)` tuple — tur 1 `RE::CandidateNotFound => ...` pattern'i
derlenmez (payload var).

**Doğrulama sonucu düzeltme:** review `MissingIdentityBinding`/`AlreadyResolved` "carries data" demişti —
yanlış; ikisi de **unit variant**. Ama imza düzeltmesi yine de geçerli çünkü CLI varyantları context ister.

**Çözüm:** imza `candidate_id` context alır:
```rust
fn map_resolution_error(
    candidate_id: &ConceptNodeId,
    error: ResolutionError,
) -> ReviewError {
    RE::CandidateNotFound(id) => ReviewError::NotFound(id.0),
    RE::CandidateNotAccepted { current } => ReviewError::CandidateNotAccepted {
        id: candidate_id.0.clone(),
        status: format!("{current:?}"),
    },
    RE::MissingIdentityBinding => ReviewError::MissingIdentityBinding(candidate_id.0.clone()),
    RE::AlreadyResolved => ReviewError::AlreadyResolved(candidate_id.0.clone()),
    // ... diğer varyantlar
}
```

### P1-B — Store error mapping resolution yüzeyine daraltılacak (zorunlu)

**Bulgu:** Tur 1 mapper seeding-reachable varyantları (`BindingNodeNotFound`, `DuplicateBinding`,
`BindingWrongKind`) resolution mapper'a eklemişti. `DuplicateBinding → AlreadyResolved` semantik
yanlış (duplicate binding ≠ already resolved).

**Doğrulama:** `apply_resolution` reachable `StoreError` seti (13 distinct — seeding set'inden ayrı):
`ResolutionBasisCandidateMismatch`, `NodeNotFound`, `NotPromotableFrom`, `BindingWrongFamily`,
`StaleResolutionBasis`, `MissingResolutionIdentityBinding`, `AlreadyResolved`, `ReuseTargetIncompatible`,
`EntityNotLiveForResolution`, `EntityIdentityCollision`, `AuditSequenceExhausted`,
`DuplicateLiveCodeEntityIdentity`, `StaleResolutionTarget`.

3 varyant her iki yoldan reachable: `EntityIdentityCollision`, `DuplicateLiveCodeEntityIdentity`,
`BindingWrongFamily`.

**Çözüm:** mapper yalnız resolution-reachable set'i içerir. Seeding-reachable-only varyantlar
(`BindingNodeNotFound`, `DuplicateBinding`, `BindingWrongKind`) `graph init` hata yolunda ele alınır
(anyhow message), resolution mapper'da YOK.

### P1-C — CLI/core identity string eşitliği enforcement'sız (zorunlu)

**Bulgu:** Tur 1 "`identity.identity_key()` ile `CodeIdentityKey.key` aynı string" iddia etti. Fakat
mapper `(&identity, scheme, policy)` ayrı parametre alır — caller yanlışlıkla identity'yi CaseSensitive
üretip mapper'a AsciiCaseInsensitive verirse core ikinci canonicalization'da lowercase yapar →
candidate NodeId bir key'den, binding başka key'den. Type-level invariant YOK.

**Çözüm:** explicit drift kontrolü:
```rust
let core_key = CodeIdentityKey::new(core_scheme, identity.identity_key())?;
if core_key.canonical_key() != identity.identity_key() {
    return Err(IdentityBridgeError::CanonicalizationDrift {
        cli_key: identity.identity_key().to_owned(),
        core_key: core_key.canonical_key().to_owned(),
    });
}
```

Daha yapısal çözüm: `AnalysisIdentityContext { scheme, path_case_policy }` tek context value — hem
`CanonicalCodeIdentity::new` hem core mapping aynı context'i tüketir. Tur 2'de context value adoption
önerilir (parametre sayısı azalır, drift type-level).

### P1-D — Empty forged-identity testi kurulamaz (zorunlu)

**Bulgu:** `CanonicalCodeIdentity` private fields + public constructor empty path'i önceden reddeder
(canonical_identity.rs:62). Sibling modül testinde "forged empty identity_key" üretilemez.

**Çözüm:** empty test core'a bırakılır (mevcut `CodeIdentityKey` test'leri). Yerine daha değerli test:
```rust
to_core_identity_key_rejects_policy_canonicalization_drift
```
Test-only unchecked constructor EKLENMEZ (production invariant'ını deler — PR D pattern).

### P2-A — `ResolutionOutcome` string yerine typed CLI enum (iyileştirme)

**Çözüm:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionOutcomeView { Created, Reused }
```
`ResolveCodeEntityMutation.outcome: ResolutionOutcomeView` (String değil). Anti-corruption CLI projection.

### P2-B — `bindings_seeded` report alanı yanlış zamanda (iyileştirme)

**Bulgu:** `BridgeRunReport` analysis projection sırasında oluşur; store seeding sonra. `bindings_seeded`
alanı başarılı store mutation gerçekleşmeden "seeded" iddia eder.

**Çözüm:** iki seçenek ayrımı —
- `BridgeRunReport.projected_identity_bindings: usize` (projection sayısı — doğru ad).
- Başarılı `seed_code_identity_bindings_trusted` sonrası ayrı stderr: `identity bindings seeded: N`.

Tur 2: ikincisi tercih edilir (Bridge report projection'ı, graph command store mutation'ı anlatır —
epistemik dürüst ayrım).

### Diğer (tur 1 tasarım sorularına review'in yanıtları — kabul edildi)

| # | Tur 1 sorusu | Review kararı | Tur 2 durumu |
|---|---|---|---|
| 1 | Binding üretim yeri | `project_candidate_nodes` boundary içinde; **fakat** `AnalysisCandidateSeed::try_new` sonrasında validated/sorted candidate'lardan (raw loop'ta değil) | Kabul edildi (B bölümü güncellendi) |
| 2 | Rich preview V1 mi | Minimal exact-target preview V1; rich diagnostic future-work | Kabul edildi (P0-2 ile birleşti) |
| 3 | Wizard session lifetime | Per-command domain session; session-spanning V2 batch | Kabul edildi (H bölümü netleştirildi) |
| 4 | `ReviewError::NotFound` reuse | Mevcut `NotFound` reuse; resolution-specific typed varyantlar ayrı | Kabul edildi (D bölümü korundu) |

## Özet

PR E core canonical identity scheme ekledi (`CodeIdentityKey` + `CodeIdentityScheme` +
`CodeIdentityBinding` store katmanı + `CodeEntityResolutionSession` + INV-C16 atomic
transition). PR E2 bu core contract'ı CLI'ya **adopte eder** — iki köprü açar:

1. **`graph init --analyze` binding seeding:** analysis'ten üretilen her `CodeEntityCandidate`
   node için `CanonicalCodeIdentity` → core `CodeIdentityKey` mapping + `seed_code_identity_bindings_trusted`
   çağrısı. PR A `project_analysis` node'ları Candidate lane'e taşır ama binding taşımaz; PR E2
   binding katmanını ekler.
2. **`osp review resolve-code-entity <candidate>`:** PR E `CodeEntityResolutionSession`'ın CLI
   yüzeyi. Supersede pattern'i (PR #54) tek-endpoint uyarlaması — operator candidate resolve eder,
   outcome (Created/Reused) store policy'sinden emerge olur (operator SEÇMEZ).

**Dar V1:** binding seeding + one-shot mutation + interactive wizard komutu. Rich preview
(`osp review resolve-code-entity-preview`) future-work (supersede-preview pattern'i). Sadece
CLI yüzeyi; core 0 değişiklik.

## Ontolojik sözleşme (tur 1 — tasarım sabitleme)

### Node identity iki katmanlı (PR A + PR E birleşimi)

```
Analysis Module (analyzer NodeId)
    │  CanonicalCodeIdentity::new(path, policy)     [CLI lexical normalize]
    ▼
CanonicalCodeIdentity { display_path, identity_key }
    │  identity_key → AnalysisIdentityScheme::PathV1.derive_node_id(CodeEntityCandidate, key)
    │  = "CodeEntityCandidate:{identity_key}"         [CLI NodeId derivation]
    ▼
ConceptNode (Candidate lane; NodeId = "CodeEntityCandidate:…")
    │  PR E2: CanonicalCodeIdentity → CodeIdentityKey mapping
    │  + seed_code_identity_bindings_trusted
    ▼
CodeIdentityBinding { node_id: candidate_node_id, identity_key: core_key }
    │  resolution: CodeIdentityKey.derive_entity_id()
    │  = "CodeEntity:{fnv1a_hash}"                     [core FNV-1a derivation]
    ▼
ConceptNode (resolved CodeEntity; NodeId = "CodeEntity:…")
    via ConceptEdgeKind::ResolvesTo (Accepted; explanation zorunlu)
```

**İki ayrık NodeId türetimi kasıtlı:**
- **Candidate NodeId** (`CodeEntityCandidate:{identity_key}`) — CLI-derived, raw-string prefix
  format (PR A `derive_node_id`). Candidate lane'in kalıcı kimliği.
- **Entity NodeId** (`CodeEntity:{fnv1a_hash}`) — core-derived, FNV-1a domain-separated hash
  (PR E `derive_resolved_code_entity_id`). Resolved entity'nin kalıcı kimliği; candidate'den
  BAĞIMSIZ üretilir.

Binding katmanı iki dünyayı köprüler: candidate NodeId ↔ `CodeIdentityKey`. Resolution sırasında
core, key'den entity NodeId türetir; candidate NodeId'ye DOKUNMAZ (R1 — ID immutable).

### Mapping tasarımı (`CanonicalCodeIdentity` → `CodeIdentityKey`)

| CLI (canonical_identity.rs) | Core (identity.rs) | Mapping |
|---|---|---|
| `PathCasePolicy::CaseSensitive` | `CodePathCasePolicy::CaseSensitive` | 1:1 (`From` impl) |
| `PathCasePolicy::AsciiCaseInsensitive` | `CodePathCasePolicy::AsciiCaseInsensitive` | 1:1 |
| `CanonicalCodeIdentity.identity_key()` | `CodeIdentityKey.key` (canonicalize sonrası) | **aynı string** — CLI lexical normalize yapmış; core empty/control-check + re-canonicalize |
| `AnalysisIdentityScheme::PathV1` (parametresiz) | `CodeIdentityScheme::AnalysisPathV1 { case_policy }` | scheme + policy birleştir |

**Duplication bilinçli (PR E core sahiplenir):** CLI `PathCasePolicy` ↔ core `CodePathCasePolicy`
aynı varyantlar ama ayrı tipler. PR E2 mapping katmanı (`identity_bridge.rs`) `From<PathCasePolicy>
for CodePathCasePolicy` impl + `CanonicalCodeIdentity` → `CodeIdentityKey` dönüştürme sağlar.
CLI enum korunur (PR D anti-corruption prensibi).

### Canonicalize çift-katmanı (zararsız, deterministic)

CLI `CanonicalCodeIdentity::new` zaten `AsciiCaseInsensitive → to_ascii_lowercase()` uygular
(`canonical_identity.rs:110-116`). Core `CodeIdentityKey::new` tekrar `canonicalize` eder
(`identity.rs:119-135`). **İkinci canonicalize idempotent** — zaten lowercase string tekrar
lowercase'e geçer. Çift validation empty/control-check için defense-in-depth; behavior değişmez.

### Resolution outcome operator-chosen DEĞİL; target operator-PINNED (tur 2 P0-2)

Supersede'de operator iki-endpoint spesifiye eder (`old → new`). Resolution'da operator **candidate
+ target** spesifiye eder; outcome (Created/Reused) store policy'sinden emerge olur
(`compute_resolution_target`, store.rs:809-848):
- 0 live entity → **Create** (deterministic `CodeEntity:{fnv1a_hash}`)
- 1 live entity → **Reuse** (existing `CodeEntity` node)
- ≥1 inactive entity → `EntityNotLiveForResolution` (Create'e düşmez — tur 4 P2-2)
- >1 live entity → `DuplicateLiveCodeEntityIdentity`

**Target operator-pinned (tur 2 P0-2):** operator confirmation'da GÖRDÜĞÜ tam target'ı command'e
taşır (`ExpectedResolutionTarget::{Create, Reuse}`). Mutation lock altında RE-compile edilen target
↔ expected target karşılaştırılır; mismatch → `StaleResolutionTarget`. Core `StaleResolutionTarget`
garantisi böylece operator presentation sınırına taşınır (P0-2 düzeltmesi).

Confirmation metni tam target'ı gösterir (Create proposed_entity_id / Reuse entity_id + digest),
sadece "outcome emerge olur" kavramını değil. Minimal canonical preview V1'de zorunlu (P0-2 doğal
sonucu); rich diagnostic preview future-work.

## Mimari (tur 1 — implementation-ready)

### Dar V1 kapsamı (tur 2 — preview V1'e alındı)

| Parça | Dahil | Kapsam dışı |
|---|---|---|
| Binding seeding (graph init) | ✓ | — |
| `resolve-code-entity` one-shot (candidate + target pinned) | ✓ | — |
| Interactive wizard komutu (per-command session) | ✓ | — |
| **Minimal canonical preview** (target reveal: Create/Reuse + entity id/digest) | ✓ (P0-2) | — |
| Rich diagnostic preview (lineage, multi-blocker, collision graph) | — | future-work (supersede-preview pattern) |
| Batch resolution (`--from-analysis`) | — | V2 aday (session-spanning) |
| Core değişiklik | — | 0 (PR E contract hazır) |

## osp-cli değişiklikleri (`crates/osp-cli/src/`)

### A. `identity_bridge.rs` (yeni modül — tek mapping boundary)

**Sorumluluk:** CLI `CanonicalCodeIdentity` ↔ core `CodeIdentityKey` dönüşümünün **tek** sahibi.
PR D `evidence_projection.rs` pattern'i (tek conversion boundary + ownership guard).

```rust
//! CLI canonical identity → core code identity köprüsü.
//!
//! Tek mapping boundary: CLI `CanonicalCodeIdentity` (lexical normalize edilmiş display_path +
//! identity_key) → core `CodeIdentityKey` (scheme + canonicalize edilmiş key).
//!
//! Duplication bilinçli (PR E core sahiplenir): CLI `PathCasePolicy` ↔ core `CodePathCasePolicy`
//! aynı varyantlar ama ayrı tipler. Bu modül `From` impl + dönüştürme sağlar; CLI enum korunur.

use osp_core::anchoring::identity::{CodeIdentityKey, CodeIdentityScheme, CodePathCasePolicy};
use crate::canonical_identity::{CanonicalCodeIdentity, PathCasePolicy};

/// Scheme + policy tek context value (tur 2 P1-C). Tur 3 P2-1: private fields + constructor;
/// accidental parameter divergence azaltır. Nihai koruma runtime CanonicalizationDrift check.
/// (Type-level mismatch engeli iddiası DARALTILDI — runtime fail-closed.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisIdentityContext {
    scheme: crate::analysis_bridge::AnalysisIdentityScheme,
    path_case_policy: PathCasePolicy,
}

impl AnalysisIdentityContext {
    pub fn new(
        scheme: crate::analysis_bridge::AnalysisIdentityScheme,
        path_case_policy: PathCasePolicy,
    ) -> Self {
        Self { scheme, path_case_policy }
    }
    pub fn scheme(&self) -> crate::analysis_bridge::AnalysisIdentityScheme { self.scheme }
    pub fn path_case_policy(&self) -> PathCasePolicy { self.path_case_policy }
    pub fn canonical_identity(
        self,
        path: &str,
    ) -> Result<CanonicalCodeIdentity, CanonicalIdentityError> {
        CanonicalCodeIdentity::new(path, self.path_case_policy)
    }
}

impl From<PathCasePolicy> for CodePathCasePolicy {
    fn from(policy: PathCasePolicy) -> Self {
        match policy {
            PathCasePolicy::CaseSensitive => CodePathCasePolicy::CaseSensitive,
            PathCasePolicy::AsciiCaseInsensitive => CodePathCasePolicy::AsciiCaseInsensitive,
        }
    }
}

/// Tur 2 P1-C — mapping drift (caller policy mismatch) typed error.
/// Tur 3 P1-1A: Eq derive ÇIKARILDI (core CodeIdentityKeyError Eq değil → derive hatası).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum IdentityBridgeError {
    #[error("identity key empty/control rejected by core")]
    CoreValidation(#[from] osp_core::anchoring::identity::CodeIdentityKeyError),
    #[error("canonicalization drift: cli key {cli_key:?} != core key {core_key:?} \
             (caller policy mismatch — use AnalysisIdentityContext)")]
    CanonicalizationDrift { cli_key: String, core_key: String },
}

/// CLI `CanonicalCodeIdentity` → core `CodeIdentityKey`.
///
/// Pre-conditions (CLI sağlar — context üzerinden):
/// - `identity_key` lexical normalize edilmiş (`CanonicalCodeIdentity::new` path structural validation).
/// - `path_case_policy` identity üretimiyle AYNI (context value enforce).
///
/// Post-conditions (core sağlar):
/// - `CodeIdentityKey::new` empty/control-check + scheme canonicalize.
/// - `scheme = AnalysisPathV1 { case_policy }` (CLI `AnalysisIdentityScheme::PathV1` tek varyant).
///
/// Tur 2 P1-C: core canonicalize sonrası `canonical_key() != identity.identity_key()` ise
/// `CanonicalizationDrift` — caller policy mismatch'i fail-closed yakalar.
pub fn to_core_identity_key(
    identity: &CanonicalCodeIdentity,
    ctx: AnalysisIdentityContext,
) -> Result<CodeIdentityKey, IdentityBridgeError> {
    let core_scheme = match ctx.scheme {
        crate::analysis_bridge::AnalysisIdentityScheme::PathV1 => {
            CodeIdentityScheme::AnalysisPathV1 { case_policy: ctx.path_case_policy.into() }
        }
    };
    let cli_key = identity.identity_key();
    let core_key = CodeIdentityKey::new(core_scheme, cli_key)?;
    if core_key.canonical_key() != cli_key {
        return Err(IdentityBridgeError::CanonicalizationDrift {
            cli_key: cli_key.to_owned(),
            core_key: core_key.canonical_key().to_owned(),
        });
    }
    Ok(core_key)
}
```

**Tasarım kararları (tur 2):**
- **`AnalysisIdentityContext` (tur 2 P1-C):** scheme + policy tek context value. `CanonicalCodeIdentity::new`
  ve `to_core_identity_key` aynı context'i tüketir → policy mismatch type-level engellenir. Tur 1'in
  `(&identity, scheme, policy)` 3-parametre imzasından düzeltme.
- **`CanonicalizationDrift` check (tur 2 P1-C):** core canonicalize sonrası drift kontrolü. CaseSensitive
  policy'de her zaman pass (lowercase yapılmaz); AsciiCaseInsensitive'de identity zaten lowercase
  üretildiği için idempotent pass. Mismatch yalnız caller bug'ında (policy propagation error).
- **`CanonicalCodeIdentity` borrow (`&`):** mapping consume etmez; candidate + binding aynı
  identity'den üretilir (R1 single-derivation prensibi).
- **`IdentityBridgeError` typed:** `CoreValidation` (`#[from] CodeIdentityKeyError`) +
  `CanonicalizationDrift`. CLI `BridgeError` sarmalar (`From<IdentityBridgeError>`).

**Ownership guard (PR D pattern):** `std::fs` recursive scan — `CodeIdentityKey::new` çağrısı
yalnız bu modülde (source-scan). `graph init --analyze` bu modül dışında `CodeIdentityKey` üretmez.

**Unit test (8 — tur 2 P1-D düzeltme: empty test core'a, drift test eklendi):**
1. Mutlu yol CaseSensitive (identity_key passthrough, drift check pass)
2. Mutlu yol AsciiCaseInsensitive (lowercase canonicalize idempotent, drift check pass)
3. Control char → `CoreValidation(ControlCharacter)` (CLI constructor geçirebilir)
4. `PathCasePolicy::CaseSensitive` → `CodePathCasePolicy::CaseSensitive`
5. `PathCasePolicy::AsciiCaseInsensitive` → `CodePathCasePolicy::AsciiCaseInsensitive`
6. `AnalysisIdentityScheme::PathV1` → `CodeIdentityScheme::AnalysisPathV1 { case_policy }`
7. Determinism (aynı input → aynı key; canonical_key + scheme round-trip)
8. **`to_core_identity_key_rejects_policy_canonicalization_drift`** (tur 2 P1-D — empty yerine;
   forged context ile policy mismatch simüle)

> **Empty test NOT included (tur 2 P1-D):** `CanonicalCodeIdentity` private fields + public constructor
> empty'i önceden reddeder; sibling modül testinde forged empty üretilemez. Empty test core'un
> mevcut `CodeIdentityKey` test'lerine ait. Test-only unchecked constructor EKLENMEZ (production
> invariant deler).

### B. `analysis_bridge.rs` değişiklik (binding seeding surface)

`BridgeRunOutput` + `CandidateProjectionOutput`'a binding çıktısı eklenir. Tur 1 review (karar #1):
binding'ler `AnalysisCandidateSeed::try_new` sonrasında **validated/sorted candidate'lardan** üretilir
(raw analyzer loop'ta değil).

```rust
pub(crate) struct CandidateProjectionOutput {
    pub(crate) candidate_seed: AnalysisCandidateSeed,
    pub(crate) identity_index: AnalysisProjectionIndex,
    pub(crate) code_identity_bindings: Vec<CodeIdentityBinding>,   // YENİ (PR E2)
    pub(crate) graph_report: BridgeRunReport,
}

pub(crate) struct BridgeRunOutput {
    pub(crate) candidate_seed: AnalysisCandidateSeed,
    pub(crate) identity_index: AnalysisProjectionIndex,
    pub(crate) graph_report: BridgeRunReport,
    pub(crate) metric_projection: crate::metric_projection::AnalysisMetricProjection,
    pub(crate) evidence_projection: crate::evidence_projection::EvidenceProjectionOutput,
    pub(crate) code_identity_bindings: Vec<CodeIdentityBinding>,   // YENİ (PR E2)
}
```

`project_candidate_nodes` içindeki akış (tur 1 review karar #1 — validated/sorted sonra):

```rust
// (1) Analyzer node'larından CodeEntityCandidate üret (identity + concept_node_id).
//     R1: concept_node_id tek-pass türetilmiş, entity + index'e thread edilir.
// (2) AnalysisCandidateSeed::try_new — collision/duplicate doğrula + deterministik sırala.
let candidate_seed = AnalysisCandidateSeed::try_new(entities)?;

// (3) Binding'leri VALIDATED/SORTED candidate'lardan türet (raw loop'ta değil — tur 1 review karar #1).
//     İkinci iteration ama ikinci identity derivation pass'i DEĞİL: path yeniden normalize edilmez,
//     ID yeniden türetilmez; yalnız doğrulanmış projection'dan companion binding üretilir.
let ctx = AnalysisIdentityContext { scheme, path_case_policy: policy };
let code_identity_bindings = candidate_seed
    .entities()
    .iter()
    .map(|candidate| {
        let identity_key = crate::identity_bridge::to_core_identity_key(
            candidate.identity(),
            ctx,
        )?;
        Ok(CodeIdentityBinding {
            node_id: candidate.concept_node_id().clone(),
            identity_key,
        })
    })
    .collect::<Result<Vec<_>, IdentityBridgeError>>()?;
```

**Tasarım kararları (tur 2 — review karar #1):**
- **Validated/sorted sonra üretim:** candidate collision validation tamamlanmadan binding üretilmez.
  Candidate ve binding deterministik sırası birlikte tanımlı. `CandidateProjectionOutput` candidate
  projection'ın tüm companion çıktılarını atomik kavramsal paket.
- **Co-derived, store-atomic DEĞİL:** "candidate + binding aynı projection boundary içinde co-derived"
  (store mutation atomik DEĞİL — store ingress `seed_code_identity_bindings_trusted` staged batch
  validation ile atomic assign yapar). Tur 1 "atomik" ifadesi daraltıldı.
- **`AnalysisIdentityContext` tek parametre (tur 2 P1-C):** `to_core_identity_key(&identity, ctx)` —
  scheme + policy context value. Drift type-level engellenir.
- **`IdentityBridgeError` propagate:** `From<IdentityBridgeError> for BridgeError` eklenir. Mapping
  hatası (empty/control core reject + drift) → bridge reject.
- **`into_drafts` unaffected:** binding seed'ten ayrı; `AnalysisCandidateSeed::into_drafts`
  GraphSeedNodeDraft üretir. Binding `CandidateProjectionOutput` → `BridgeRunOutput` üzerinden `graph init`'e akar.

### C. `commands/graph.rs` değişiklik (`--analyze` branch binding seeding)

`graph.rs:175` sonrası (`candidate_seed.into_drafts()` → `GraphSeedBuilder::build` →
`InMemoryAnchorStore::with_seed`), binding seeding eklenir:

```rust
// graph.rs:186 sonrası (with_seed sonrası)
if !bridge_output.code_identity_bindings.is_empty() {
    store.seed_code_identity_bindings_trusted(&bridge_output.code_identity_bindings)
        .map_err(|e| anyhow::anyhow!("identity binding seeding failed: {e}"))?;
}
```

**Tasarım kararları (tur 1):**
- **`with_seed` sonrası:** graph node'ları önce seed'lenir (candidate'ler), sonra binding.
  `seed_code_identity_bindings_trusted` node existence check (step 1) ister; node'lar hazır
  olmalı.
- **Empty skip:** boş analysis için binding çağrısı YOK (PR A empty-warning pattern).
- **Hata → `anyhow`:** store hatası (`StoreError::BindingNodeNotFound` / `BindingWrongKind` /
  `BindingWrongFamily` / `DuplicateBinding` / `DuplicateLiveCodeEntityIdentity`) typed ama CLI
  surface'da user-friendly message. `anyhow` sarmalar.
- **Pre-validation round-trip:** `export_snapshot` → `restore_snapshot` (graph.rs:187-188)
  binding'leri içerir (PR E snapshot v2). INV-C16 snapshot validation binding'leri doğrular.

**Stderr诚实 (PR D pattern — tur 2 P2-B düzeltme):** iki ayrı yüzey, epistemik dürüst ayrım:
- **`BridgeRunReport.projected_identity_bindings: usize`** (alan adı düzeltme — `bindings_seeded` değil;
  report projection'ı anlatır, mutation değil). Deterministik Display.
- **Başarılı `seed_code_identity_bindings_trusted` sonrası ayrı stderr** (graph.rs):
  ```
  identity bindings seeded: {n}
  ```
  Graph command store mutation sonucunu anlatır.

Bridge report projection sayısını, graph command store mutation'ı ifade eder — "seeded" iddiası
ancak başarılı mutation sonrası.

**Unit test (integration, `analyze_bridge_flow.rs`):**
1. Mutlu yol — `graph init --analyze` sonrası store binding içerir (snapshot v2 `code_identity_bindings` non-empty)
2. Binding candidate NodeId ↔ CodeIdentityKey consistency (her candidate için bir binding)
3. Empty analysis — binding YOK, stderr warning
4. Round-trip — export → restore INV-C16 validation geçer (binding'lerle)

### D. `errors.rs` (resolve-code-entity types)

Supersede pattern'inin tek-endpoint uyarlaması + **tur 2 P0-2 target pinning** + **P2-A typed outcome**:

```rust
use osp_core::anchoring::review::ResolutionRecord;

/// Tur 2 P0-2 — operator-pinned target. Confirmation'da görülen tam target command'e taşınır.
/// Mutation lock altında re-compile edilen target ↔ expected karşılaştırılır (StaleResolutionTarget).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedResolutionTarget {
    Create { proposed_entity_id: ConceptNodeId },
    Reuse { entity_id: ConceptNodeId, entity_digest: NodeDigest },
}

/// `osp review resolve-code-entity <candidate>` tek-endpoint mutation command.
#[derive(Debug, Clone)]
pub struct ResolveCodeEntityCommand {
    pub candidate: ConceptNodeId,
    pub expected_candidate_digest: NodeDigest,       // candidate TOCTOU
    pub expected_target: ExpectedResolutionTarget,   // tur 2 P0-2 — target TOCTOU
    pub reason: String,
}

/// Tur 2 P2-A — typed outcome (String değil; anti-corruption CLI projection).
/// Tur 3 P2-4 — as_str() text output ile JSON terminolojiyi hizalar (Debug değil).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionOutcomeView { Created, Reused }

impl ResolutionOutcomeView {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Reused => "reused",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolveCodeEntityMutation {
    pub status: String,                    // "resolved"
    pub candidate_node_id: String,
    pub entity_node_id: String,
    pub outcome: ResolutionOutcomeView,    // tur 2 P2-A — typed enum (String değil)
    pub resolution_sequence: u64,          // record.seq (global audit_seq union)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PersistedResolveCodeEntityOutput {
    pub mutation: ResolveCodeEntityMutation,
    pub revision: u64,
}
```

**`ReviewError` yeni varyantlar (resolution-specific):**

```rust
pub enum ReviewError {
    // ... mevcut ...
    // --- resolution-specific (PR E2) ---
    // NotFound mevcut; reuse edilir (tur 1 review karar #4 — CandidateNotFound açılmaz).
    #[error("candidate is not Accepted: {id} (status: {status})")]
    CandidateNotAccepted { id: String, status: String },
    #[error("stale resolution basis — candidate digest değişmiş")]
    StaleResolutionBasis,
    #[error("candidate already resolved: {0}")]
    AlreadyResolved(String),
    #[error("stale resolution target — outcome drifted (re-run preview)")]
    StaleResolutionTarget,
    #[error("entity not live for resolution: {entity_id} (status: {status})")]
    EntityNotLiveForResolution { entity_id: String, status: String },
    #[error("entity identity collision: {0}")]
    EntityIdentityCollision(String),
    #[error("duplicate live entity for this identity key")]
    DuplicateLiveEntity,
    #[error("missing identity binding for candidate: {0}")]
    MissingIdentityBinding(String),
}
```

**Tasarım kararları (tur 2):**
- **`ExpectedResolutionTarget` (tur 2 P0-2):** operator-pinned target. Create proposed_entity_id /
  Reuse entity_id + digest. Mutation-time compile ↔ expected karşılaştırma.
- **`expected_candidate_digest` rename:** tur 1 `expected_basis_digest` → tur 2 `expected_candidate_digest`
  (target ayrı pinlendiği için basis = candidate digest netleştirme).
- **`ResolutionOutcomeView` typed (tur 2 P2-A):** String değil; `#[serde(rename_all = "snake_case")]`
  → `"created"` / `"reused"`. Core enum expose ETMEZ; CLI anti-corruption projection.
- **`resolution_sequence`:** `record.seq` (global `audit_seq` union — decision + supersede + resolution).
- **`ReviewError::NotFound` reuse (tur 1 review karar #4):** accept/reject ile paylaşılan "node not
  found" semantics. Resolution-specific typed varyantlar ayrı (CandidateNotAccepted, MissingIdentityBinding,
  AlreadyResolved, StaleResolutionTarget, EntityNotLiveForResolution, EntityIdentityCollision,
  DuplicateLiveEntity). Operasyon isimleri enum varyantına gömülmez (ileride machine-readable
  envelope `operation` metadata taşır).

### E. `application/review.rs` (execute + apply_resolution)

**Tur 2 P0-1 düzeltme:** `candidate_query()` KULLANILMAZ. Tek yol `PresentedResolutionBasis::compile`
(`resolution_basis_view` Accepted gate canonical). Tur 2 P0-2: target pinning.

```rust
pub fn execute_resolve_code_entity(
    &self,
    command: ResolveCodeEntityCommand,
    operator: OperatorId,
) -> Result<PersistedResolveCodeEntityOutput, ReviewError> {
    self.repo
        .mutate(|store| {
            let record = apply_resolution(store, &command, operator.clone())?;
            let outcome = match record.outcome {
                ResolutionOutcome::Created { .. } => ResolutionOutcomeView::Created,
                ResolutionOutcome::Reused { .. } => ResolutionOutcomeView::Reused,
            };
            Ok(ResolveCodeEntityMutation {
                status: "resolved".into(),
                candidate_node_id: record.candidate_id.0.clone(),
                entity_node_id: record.entity_id.0.clone(),
                outcome,
                resolution_sequence: record.seq,
            })
        })
        .map(|(mutation, revision)| PersistedResolveCodeEntityOutput { mutation, revision })
}
```

**`apply_resolution` (domain transition under lock — tur 2 P0-1 compile-based):**

```rust
fn apply_resolution(
    store: &mut InMemoryAnchorStore,
    cmd: &ResolveCodeEntityCommand,
    operator: OperatorId,
) -> Result<ResolutionRecord, ReviewError> {
    // (1) Basis compile — canonical Accepted gate + kind + binding + target (resolution_basis_view).
    //     candidate_query() KULLANILMAZ (tur 2 P0-1): Accepted candidate candidate_query dışında.
    let basis = PresentedResolutionBasis::compile(store, &cmd.candidate)
        .map_err(|e| map_resolution_error(&cmd.candidate, e))?;

    // (2) Candidate digest TOCTOU (compile sonrası — compile başarılı → digest comparison).
    if basis.candidate_digest() != cmd.expected_candidate_digest {
        return Err(ReviewError::StaleResolutionBasis);
    }

    // (3) Target TOCTOU (tur 2 P0-2 — operator-pinned target ↔ current target).
    validate_expected_target(&basis, &cmd.expected_target)?;

    // (4) Reason validation.
    let reason = NonEmptyExplanation::new(cmd.reason.clone())
        .map_err(|e| ReviewError::Store(e.to_string()))?;

    // (5) Session + resolve (core INV-C16 atomic 14-step).
    let mut session = CodeEntityResolutionSession::open_for_operator(operator);
    session.resolve(store, &cmd.candidate, basis, reason)
        .map_err(|e| map_resolution_error(&cmd.candidate, e))
}

/// Tur 2 P0-2 — operator-pinned target ↔ current target karşılaştırma.
/// Core StaleResolutionTarget garantisi operator presentation sınırına taşınır.
fn validate_expected_target(
    basis: &PresentedResolutionBasis,
    expected: &ExpectedResolutionTarget,
) -> Result<(), ReviewError> {
    use osp_core::anchoring::review::PresentedResolutionTarget as PRT;
    match (basis.target(), expected) {
        (PRT::Create { proposed_entity_id }, ExpectedResolutionTarget::Create { proposed_entity_id: exp })
            if proposed_entity_id == exp => Ok(()),
        (PRT::Reuse { entity_id, entity_digest }, ExpectedResolutionTarget::Reuse { entity_id: exp, entity_digest: exp_digest })
            if entity_id == exp && entity_digest == exp_digest => Ok(()),
        _ => Err(ReviewError::StaleResolutionTarget),
    }
}
```

**`map_resolution_error` (tur 2 P1-A — context taşır):**

```rust
fn map_resolution_error(
    candidate_id: &ConceptNodeId,
    error: ResolutionError,
) -> ReviewError {
    use osp_core::anchoring::review::ResolutionError as RE;
    match error {
        // CandidateNotFound tuple variant (tur 2 P1-A — derlenmez düzeltme)
        RE::CandidateNotFound(id) => ReviewError::NotFound(id.0),
        RE::WrongCandidateKind => ReviewError::NotPromotable("candidate kind değil CodeEntityCandidate".into()),
        RE::WrongFamily => ReviewError::NotPromotable("candidate family değil PhysicalCode".into()),
        RE::CandidateNotAccepted { current } => ReviewError::CandidateNotAccepted {
            id: candidate_id.0.clone(),
            status: format!("{current:?}"),
        },
        RE::StaleResolutionBasis => ReviewError::StaleResolutionBasis,
        // Unit variants — candidate_id context'ten (tur 2 P1-A)
        RE::MissingIdentityBinding => ReviewError::MissingIdentityBinding(candidate_id.0.clone()),
        RE::AlreadyResolved => ReviewError::AlreadyResolved(candidate_id.0.clone()),
        RE::StaleResolutionTarget => ReviewError::StaleResolutionTarget,
        RE::ReuseTargetIncompatible => ReviewError::NotPromotable("reuse target incompatible".into()),
        RE::EntityNotLiveForResolution { entity_id, status } => {
            ReviewError::EntityNotLiveForResolution {
                entity_id: entity_id.0,
                status: format!("{status:?}"),
            }
        }
        RE::EntityIdentityCollision { entity_id } => ReviewError::EntityIdentityCollision(entity_id.0),
        RE::DuplicateLiveEntity => ReviewError::DuplicateLiveEntity,
        RE::BasisMismatch { .. } | RE::AuditSequenceExhausted | RE::SessionCounterExhausted => {
            ReviewError::Store(error.to_string())
        }
        // Tur 3 P1-3: store mapper da candidate_id context alır (NotPromotableFrom status-aware split)
        RE::Store(source) => map_resolution_store_error(candidate_id, source),
    }
}

/// Tur 2 P1-B + tur 3 P1-2/P1-3 — resolution-reachable StoreError set + candidate context.
/// Tur 3 P1-2: BindingWrongKind EKLENDİ (resolution_basis_view Accepted gate sonrası reachable).
/// Tur 3 P1-3: NotPromotableFrom status-aware split (Candidate/Rejected/Deprecated → CandidateNotAccepted;
///             Accepted + wrong structural → NotPromotable; NotPromotableFrom(Accepted) mümkün).
fn map_resolution_store_error(
    candidate_id: &ConceptNodeId,
    source: Box<dyn std::error::Error + Send + Sync>,
) -> ReviewError {
    use osp_core::anchoring::store::StoreError as SE;
    use osp_core::anchoring::DecisionStatus;
    let Some(store_err) = source.downcast_ref::<SE>() else {
        return ReviewError::Store(source.to_string());
    };
    match store_err {
        // Tur 3 P1-3 — status-aware split (NotPromotableFrom(Accepted) mümkün, yanlış attribution engeli)
        SE::NotPromotableFrom(status) if *status != DecisionStatus::Accepted => ReviewError::CandidateNotAccepted {
            id: candidate_id.0.clone(),
            status: format!("{status:?}"),
        },
        SE::NotPromotableFrom(status) => ReviewError::NotPromotable(
            format!("accepted node is not structurally eligible for resolution (status={status:?})")),
        SE::NodeNotFound(id) => ReviewError::NotFound(id.0),
        // Tur 3 P1-2 — BindingWrongKind reachable (resolution_basis_view store.rs:1711-1715)
        SE::BindingWrongKind { kind } => ReviewError::NotPromotable(
            format!("candidate kind is {kind:?}; expected CodeEntityCandidate")),
        SE::BindingWrongFamily { family } => ReviewError::NotPromotable(
            format!("candidate family is {family:?}; expected PhysicalCode")),
        SE::StaleResolutionBasis { .. } => ReviewError::StaleResolutionBasis,
        SE::MissingResolutionIdentityBinding(id) => ReviewError::MissingIdentityBinding(id.0),
        SE::AlreadyResolved(id) => ReviewError::AlreadyResolved(id.0),
        SE::ReuseTargetIncompatible { entity_id } => ReviewError::NotPromotable(format!("{entity_id}")),
        SE::EntityNotLiveForResolution { entity_id, status } => ReviewError::EntityNotLiveForResolution {
            entity_id: entity_id.0, status: format!("{status:?}"),
        },
        SE::EntityIdentityCollision { entity_id } => ReviewError::EntityIdentityCollision(entity_id.0),
        SE::DuplicateLiveCodeEntityIdentity => ReviewError::DuplicateLiveEntity,
        SE::StaleResolutionTarget => ReviewError::StaleResolutionTarget,
        SE::ResolutionBasisCandidateMismatch { .. } | SE::AuditSequenceExhausted => {
            ReviewError::Store(source.to_string())
        }
        // NOT mapped (seeding-only — graph init handles): BindingNodeNotFound, DuplicateBinding.
        // NOT mapped (unreachable from resolution): supersede/decision-specific variants.
        _ => ReviewError::Store(source.to_string()),
    }
}
```

**Tasarım kararları (tur 3):**
- **`compile` canonical gate (tur 2 P0-1):** `candidate_query()` YOK. `PresentedResolutionBasis::compile`
  → `resolution_basis_view` Accepted gate (store.rs:1707) + kind + binding + target tek çağrıda.
- **Digest kontrol compile SONRASI (tur 2 P0-1):** `compile` başarılı → `basis.candidate_digest()`
  ↔ expected. `compile` başarısız → typed error map.
- **`validate_expected_target` (tur 2 P0-2):** operator-pinned target ↔ current target. Mismatch →
  StaleResolutionTarget.
- **`map_resolution_error` context (tur 2 P1-A):** `candidate_id: &ConceptNodeId` — unit variant'lar
  context'ten ID alır. `CandidateNotFound(id)` tuple pattern.
- **Tur 3 P1-2 `BindingWrongKind` mapper'a eklendi:** `resolution_basis_view` (store.rs:1711-1715)
  Accepted gate sonrası wrong-kind durumunda `BindingWrongKind` döner → reachable. (`apply_resolution`
  step 3 `NotPromotableFrom` döner — asymmetrik; her ikisi de map'lenir.)
- **Tur 3 P1-3 `NotPromotableFrom` status-aware split:** `candidate_id` context store mapper'a da
  taşınır. `NotPromotableFrom(non-Accepted)` → `CandidateNotAccepted` (Candidate/Rejected/Deprecated);
  `NotPromotableFrom(Accepted)` → `NotPromotable` (wrong structural — yanlış attribution engeli).
- **Tur 2 P1-B seeding-only YOK:** `BindingNodeNotFound`, `DuplicateBinding` graph init hata yolunda.
- **Session pattern:** `CodeEntityResolutionSession::open_for_operator` → `resolve` → (implicit
  close on drop). Per-call session (tur 1 review karar #3).

### F. `commands/review.rs` (one-shot adapter + minimal canonical preview)

**Tur 3 P0:** explicit target flag'leri (colon-free; NodeId colon içerir → split kırılgan).
**Tur 3 P1-1B/C/D:** derleme düzeltmeleri (Serialize derive, Result açma, parse_expected_target).
**Tur 3 P1-4:** preview `ReviewQuery`/`ReviewReadOutput` tek read motoru.
**Tur 3 P2-4:** text output `as_str()` (Debug değil).
**Tur 3 preview sadeleştirme:** `expected_target()` infallible.

```rust
/// Tur 3 P0 — colon-free target outcome enum (NodeId colon içerir).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ResolutionTargetOutcomeArg { Create, Reuse }

#[derive(Args, Debug)]
pub struct ReviewResolveCodeEntityArgs {
    pub candidate: String,                                    // positional (tek-endpoint)
    #[arg(long, default_value = ".osp/anchor-store.json")]
    pub store: PathBuf,
    #[arg(long)]
    pub operator: Option<String>,                            // $OSP_OPERATOR fallback
    #[arg(long)]
    pub reason: String,                                       // required (INV-C7 explanation)
    #[arg(long)]
    pub candidate_digest: Option<String>,                    // hex; required non-TTY/--yes
    // Tur 3 P0 — explicit colon-free target flag'leri (parse_target_flag YOK)
    #[arg(long, value_enum)]
    pub target_outcome: Option<ResolutionTargetOutcomeArg>,
    #[arg(long)]
    pub target_entity_id: Option<String>,
    #[arg(long)]
    pub target_entity_digest: Option<String>,
    #[arg(long)]
    pub yes: bool,                                            // skip confirmation
    #[arg(long, default_value = "text")]
    pub format: String,                                       // "text" | "json"
}

/// Tur 3 P0 — explicit target flag validation matrisi.
/// Create → entity_id zorunlu, digest verilmemeli; Reuse → ikisi zorunlu.
fn parse_expected_target(
    args: &ReviewResolveCodeEntityArgs,
) -> anyhow::Result<ExpectedResolutionTarget> {
    match args.target_outcome {
        Some(ResolutionTargetOutcomeArg::Create) => {
            let id = args.target_entity_id.as_ref().ok_or_else(|| anyhow::anyhow!(
                "--target-entity-id is required when --target-outcome=create"))?;
            if args.target_entity_digest.is_some() {
                anyhow::bail!("--target-entity-digest is not valid when --target-outcome=create");
            }
            Ok(ExpectedResolutionTarget::Create {
                proposed_entity_id: ConceptNodeId(id.clone()),
            })
        }
        Some(ResolutionTargetOutcomeArg::Reuse) => {
            let id = args.target_entity_id.as_ref().ok_or_else(|| anyhow::anyhow!(
                "--target-entity-id is required when --target-outcome=reuse"))?;
            let digest = args.target_entity_digest.as_ref().ok_or_else(|| anyhow::anyhow!(
                "--target-entity-digest is required when --target-outcome=reuse"))?;
            Ok(ExpectedResolutionTarget::Reuse {
                entity_id: ConceptNodeId(id.clone()),
                entity_digest: parse_digest_hex("--target-entity-digest", digest)?,
            })
        }
        None => anyhow::bail!(
            "--target-outcome, --target-entity-id and the applicable target digest \
             are required for non-interactive resolution"),
    }
}

pub fn run_review_resolve_code_entity(args: ReviewResolveCodeEntityArgs) -> anyhow::Result<()> {
    let operator = resolve_operator(args.operator.clone())?;
    let operator_id = OperatorId::new(operator);
    let is_tty = std::io::stdin().is_terminal();
    let (candidate_digest, expected_target) = if args.yes || !is_tty {
        // Non-interactive: --candidate-digest + explicit target flag'leri zorunlu (tur 3 P0).
        let digest = parse_digest_hex("--candidate-digest",
            &args.candidate_digest.clone().ok_or_else(|| anyhow::anyhow!(
                "--candidate-digest <hex> required for non-interactive resolve-code-entity")))?;
        let target = parse_expected_target(&args)?;   // tur 3 P0 — explicit flag'ler
        (digest, target)
    } else {
        confirm_with_resolution(&args)?  // TTY: minimal preview + target reveal + [y/N]
    };
    let command = ResolveCodeEntityCommand {
        candidate: ConceptNodeId(args.candidate.clone()),
        expected_candidate_digest: candidate_digest,
        expected_target,
        reason: args.reason.clone(),
    };
    let repo = FileReviewStore::new(&args.store);
    let service = ReviewApplicationService::new(repo);
    let output = service.execute_resolve_code_entity(command, operator_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let format = OutputFormat::from_str(&args.format);
    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Tur 3 P2-4 — as_str() text/JSON terminolojiyi hizalar (Debug değil)
        println!("✓ resolved {} → {} ({}, record #{}, revision {})",
            output.mutation.candidate_node_id,
            output.mutation.entity_node_id,
            output.mutation.outcome.as_str(),
            output.mutation.resolution_sequence,
            output.revision);
    }
    Ok(())
}

/// Tur 2 P0-2 + tur 3 P1-1C — minimal canonical preview + target reveal.
/// Tur 3 preview sadeleştirme: expected_target() infallible (anyhow YOK).
fn confirm_with_resolution(
    args: &ReviewResolveCodeEntityArgs,
) -> Result<(NodeDigest, ExpectedResolutionTarget), anyhow::Error> {
    let repo = FileReviewStore::new(&args.store);
    let service = ReviewApplicationService::new(repo);
    let preview = service.execute_resolve_code_entity_preview(ConceptNodeId(args.candidate.clone()))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let digest = parse_digest_hex("candidate_digest_hex", &preview.candidate.digest_hex)?;
    // Tur 3 preview sadeleştirme — infallible (tur 3 P1-1C: ? ile açma düzeltmesi)
    let expected_target = preview.expected_target();
    render_resolve_code_entity_preview_text(&mut std::io::stdout(), &preview)?;
    println!("Resolve this exact candidate and target basis? [y/N]");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") && !input.trim().eq_ignore_ascii_case("yes") {
        anyhow::bail!("aborted by operator");
    }
    Ok((digest, expected_target))
}
```

**Minimal canonical preview model (tur 2 P0-2 + tur 3 P1-1B/P1-4/preview sadeleştirme):**

```rust
// Tur 3 P1-4 — ReviewQuery/ReviewReadOutput'a eklendi (tek read motoru).
pub enum ReviewQuery {
    // ... mevcut ...
    ResolveCodeEntityPreview { candidate: ConceptNodeId },
}
pub enum ReviewReadOutput {
    // ... mevcut ...
    ResolveCodeEntityPreview(ResolutionPreviewOutput),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolutionPreviewOutput {
    pub revision: u64,
    pub candidate: ResolutionCandidatePreview,
    pub identity_key: IdentityKeyPreview,
    pub target: ResolutionTargetPreview,
}

// Tur 3 P1-1B — Serialize derive EKLENDİ (tur 2'de yoktu → derive hatası).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ResolutionTargetPreview {
    Create { proposed_entity_id: String },
    Reuse { entity_id: String, entity_digest_hex: String, entity_status: String },
}

impl ResolutionPreviewOutput {
    /// Tur 3 preview sadeleştirme — infallible (supersede preview pattern).
    /// entity_digest_hex application'ın kendi {:016x} çıktısı → expect infallible.
    /// Application read model anyhow bağımlılığı YOK; anyhow yalnız CLI adapter'da.
    pub fn expected_target(&self) -> ExpectedResolutionTarget {
        match &self.target {
            ResolutionTargetPreview::Create { proposed_entity_id } => ExpectedResolutionTarget::Create {
                proposed_entity_id: ConceptNodeId(proposed_entity_id.clone()),
            },
            ResolutionTargetPreview::Reuse { entity_id, entity_digest_hex, .. } => {
                let raw = u64::from_str_radix(entity_digest_hex, 16)
                    .expect("preview entity_digest_hex is our own {:016x} output");
                ExpectedResolutionTarget::Reuse {
                    entity_id: ConceptNodeId(entity_id.clone()),
                    entity_digest: NodeDigest::from_raw(raw),
                }
            }
        }
    }
}
```

**Standalone preview command + builder (tur 3 P1-4 — tek read motoru):**

```rust
#[derive(Args, Debug)]
pub struct ReviewResolveCodeEntityPreviewArgs {
    pub candidate: String,
    #[arg(long, default_value = ".osp/anchor-store.json")]
    pub store: PathBuf,
    #[arg(long, default_value = "text")]
    pub format: String,
}

// ReviewQuery dispatch:
ReviewQuery::ResolveCodeEntityPreview { candidate } => {
    let preview = build_resolve_code_entity_preview(&store, &candidate, revision)?;
    Ok(ReviewReadOutput::ResolveCodeEntityPreview(preview))
}

// Convenience method (execute_query sarmalar — supersede pattern):
pub fn execute_resolve_code_entity_preview(
    &self,
    candidate: ConceptNodeId,
) -> Result<ResolutionPreviewOutput, ReviewError> {
    match self.execute_query(ReviewQuery::ResolveCodeEntityPreview { candidate })? {
        ReviewReadOutput::ResolveCodeEntityPreview(output) => Ok(output),
        _ => unreachable!("query/output variant mismatch"),
    }
}
```

**Confirmation metni (tam target reveal):**
```text
Candidate: CodeEntityCandidate:src/lib.rs
  Digest:   12ab34cd...

Identity key:
  scheme: analysis-path-v1
  policy: ascii-case-insensitive
  key:    src/lib.rs

Resolution target:
  outcome:         reuse
  entity:          CodeEntity:a1b2c3d4e5f6a7b8
  entity digest:   98cd...

Resolve this exact candidate and target basis? [y/N]
```

**Tasarım kararları (tur 3):**
- **Tur 3 P0 explicit target flags:** `--target-outcome` (value_enum) + `--target-entity-id` +
  `--target-entity-digest` ayrı flag'ler. Colon-delimited parse YOK (NodeId colon içerir → kırılgan).
  `parse_expected_target` validation matrisi (Create → id zorunlu digest YOK; Reuse → ikisi zorunlu).
- **Tur 3 P1-1B `ResolutionTargetPreview` Serialize:** `#[serde(tag = "outcome", rename_all = "snake_case")]`.
- **Tur 3 P1-1C `expected_target()` infallible:** tur 2 `to_expected_target() -> Result` düzeltmesi.
  Application read model anyhow YOK; supersede preview pattern.
- **Tur 3 P1-1D `parse_expected_target(&args)`:** tur 2 `parse_target_flag` sözleşme açığı düzeltmesi.
- **Tur 3 P1-4 ReviewQuery/ReviewReadOutput:** preview tek read motoru (`read_validated_store`).
  candidate digest + identity key + target + revision aynı restored snapshot'tan.
- **Tur 3 P2-4 `as_str()`:** text output `output.mutation.outcome.as_str()` (Debug değil).
- **`render_resolve_code_entity_preview_text`:** body-only renderer (üç yüzey tek renderer divergence YOK).
- **Rich diagnostic future-work:** lineage, multi-blocker, collision graph, batch eligibility V2.
- **`--operator` zorunlu + `--format json`:** accept/reject/supersede pattern.

### G. `main.rs` dispatch (tur 2 — preview eklendi)

```rust
enum ReviewAction {
    // ... mevcut ...
    ResolveCodeEntity(commands::review::ReviewResolveCodeEntityArgs),
    ResolveCodeEntityPreview(commands::review::ReviewResolveCodeEntityPreviewArgs),  // tur 2 P0-2
}

// dispatch:
Some(ReviewAction::ResolveCodeEntity(args)) => {
    commands::review::run_review_resolve_code_entity(args)
}
Some(ReviewAction::ResolveCodeEntityPreview(args)) => {
    commands::review::run_review_resolve_code_entity_preview(args)
}
```

### H. Interactive wizard (`review_session.rs` — tur 1 review karar #3 + tur 3 P1-5)

**Tur 3 P1-5 — wizard reason sırası:** `resolve <candidate>` (reason YOK — mevcut wizard informed-review
sözleşmesi: operator görmediği basis'e gerekçe yazmasın). Akış: minimal preview → exact target
confirmation → reason prompt → mutation.

```text
osp> resolve CodeEntityCandidate:src/lib.rs
```
ardından wizard akışı:
```text
Candidate: CodeEntityCandidate:src/lib.rs
  Digest:   12ab34cd...
Resolution target:
  outcome: create
  entity:  CodeEntity:a1b2c3d4e5f6a7b8
Resolve this exact candidate and target basis? [y/N] y
Reason: this is the canonical entity for the lib module
✓ resolved CodeEntityCandidate:src/lib.rs → CodeEntity:a1b2c3d4e5f6a7b8 (created, record #5)
```

Komut `resolve <candidate>` (tur 3 P1-5 — `resolve <candidate> <reason>` DEĞİL). Reason confirmation
sonrası prompt edilir; preview'dan çıkarılan digest + expected target ile mutate edilir.

**Tur 1 review karar #3 — per-command domain session:**
Interactive shell session UI-scoped; `CodeEntityResolutionSession` mutation-command-scoped.
Her mutation kendi lock/reload/revision/save envelope'unda. One-shot ve wizard aynı application
davranışına sahip. Domain session disk store revision'ları boyunca yarı-persist edilmiş yaşamaz.

**Session-spanning V2 batch resolution:**
```bash
osp review resolve-code-entity --from-analysis
```
O zaman tek açık session altında N resolution (ortak session ID, toplu audit correlation,
`close().resolutions`, batch-level summary). V1'de YOK.

V1 ayrımı yazılı sözleşme:
> Interactive shell session is UI-scoped; `CodeEntityResolutionSession` is mutation-command-scoped.
> Batch resolution may introduce an explicitly session-spanning domain lifecycle in V2.

## Kabul kriterleri (~38 — tur 3: +6 CLI sözleşme/derleme/error-mapping)

1. `identity_bridge.rs` tek mapping boundary (`to_core_identity_key`); `AnalysisIdentityContext`
   scheme+policy tek value (tur 2 P1-C).
2. **Tur 3 P2-1:** `AnalysisIdentityContext` private fields + constructor; nihai koruma runtime
   drift check (type-level engel iddiası DARALTILDI).
3. `CanonicalCodeIdentity` → `CodeIdentityKey` mapping deterministic (aynı input → aynı key).
4. **Tur 2 P1-C / tur 3 P2-1:** core canonicalize sonrası drift → `CanonicalizationDrift` runtime
   guard (type-level DEĞİL).
5. **Tur 2 P1-D / tur 3 P1-1A:** `IdentityBridgeError` `Eq` derive YOK (core `CodeIdentityKeyError`
   Eq değil). empty test core'a; `to_core_identity_key_rejects_policy_canonicalization_drift` eklendi.
6. `project_candidate_nodes` binding'leri `AnalysisCandidateSeed::try_new` sonrası validated/sorted
   candidate'lardan üretir (tur 1 review karar #1).
7. `CandidateProjectionOutput.code_identity_bindings` + `BridgeRunOutput.code_identity_bindings`
   non-empty (`--analyze` mutlu yol).
8. `graph init --analyze` `seed_code_identity_bindings_trusted` çağırır (node existence sonrası).
9. `graph init --seed` (legacy) binding üretmez (PR A legacy semantics preserved).
10. Empty analysis → binding YOK + stderr warning (PR A pattern).
11. `graph init --analyze` sonrası `export_snapshot` → `restore_snapshot` INV-C16 validation geçer
    (binding'lerle).
12. Binding candidate NodeId ↔ `CodeIdentityKey` consistency (her binding'in node_id'si candidate
    seed'inde mevcut).
13. **Tur 2 P2-B:** `BridgeRunReport.projected_identity_bindings` (projection sayısı); başarılı
    seeding sonrası ayrı stderr `identity bindings seeded: N`.
14. **Tur 2 P0-1:** `apply_resolution` `candidate_query()` KULLANMAZ — `PresentedResolutionBasis::compile`
    canonical (Accepted gate `resolution_basis_view`'da).
15. **Tur 2 P0-1:** Accepted candidate resolvable (mutlu yol `NotFound` DEĞİL).
16. **Tur 2 P0-2:** `ResolveCodeEntityCommand` candidate digest + `ExpectedResolutionTarget` taşır.
17. **Tur 2 P0-2:** mutation lock altında candidate digest ↔ expected + target ↔ expected
    karşılaştırma; mismatch `StaleResolutionBasis` / `StaleResolutionTarget`.
18. **Tur 2 P0-2 / tur 3 P2-2:** create→reuse target drift reject; reuse entity digest (content)
    drift reject; reuse target becoming inactive → `EntityNotLiveForResolution` (Create'e düşmez).
19. **Tur 2 P0-2 / tur 3 P1-4:** minimal canonical preview `execute_resolve_code_entity_preview`
    target reveal; tek `read_validated_store()` (ReviewQuery motoru; ayrı store okuması YOK).
20. **Tur 2 P0-2:** standalone `osp review resolve-code-entity-preview <candidate>` + `--format json`.
21. **Tur 2 P0-2:** confirmation TTY tam target gösterir + `[y/N]`; `Show` DEĞİL preview-based.
22. **Tur 3 P0:** non-interactive explicit target flag'leri (`--target-outcome` value_enum +
    `--target-entity-id` + `--target-entity-digest`); colon-delimited parse YOK.
23. **Tur 3 P0:** validation matrisi — Create → entity_id zorunlu, digest verilmemeli; Reuse →
    ikisi zorunlu.
24. `--operator` zorunlu (flag → `$OSP_OPERATOR` → error; accept/reject/supersede pattern).
25. `execute_resolve_code_entity` `mutate()` envelope kullanır (lock → reload → validate → op →
    revision+1 → save).
26. **Tur 2 P1-A:** `map_resolution_error(candidate_id, error)` context taşır; `CandidateNotFound(id)`
    tuple pattern; unit variant'lar context'ten ID alır.
27. **Tur 3 P1-2:** `BindingWrongKind` mapper'a DAHİL (`resolution_basis_view` reachable — store.rs:1711).
28. **Tur 3 P1-3:** `map_resolution_store_error(candidate_id, source)` candidate context alır;
    `NotPromotableFrom` status-aware split (Candidate/Rejected/Deprecated → `CandidateNotAccepted`;
    Accepted + wrong structural → `NotPromotable`).
29. **Tur 2 P1-B:** `map_resolution_store_error` seeding-only varyantlar (`BindingNodeNotFound`,
    `DuplicateBinding`) YOK.
30. Outcome (Created/Reused) store policy'sinden emerge; operator SEÇMEZ ama PINLER (tur 2 P0-2).
31. **Tur 2 P2-A / tur 3 P2-4:** `ResolutionOutcomeView` typed enum + `as_str()`; text output
    `as_str()` (Debug değil).
32. **Tur 3 P1-1B:** `ResolutionTargetPreview` `#[derive(Serialize)]` + `#[serde(tag="outcome", rename_all="snake_case")]`.
33. **Tur 3 preview sadeleştirme:** `expected_target()` infallible; application read model anyhow YOK.
34. JSON output `PersistedResolveCodeEntityOutput` serde (`status`, `candidate_node_id`,
    `entity_node_id`, `outcome` typed, `resolution_sequence`, `revision`).
35. **Tur 3 P1-5:** interactive wizard `resolve <candidate>` (reason YOK); preview → confirmation →
    reason prompt → mutation (mevcut informed-review sözleşmesi).
36. Interactive wizard per-command domain session (tur 1 review karar #3).
37. `ReviewAction::ResolveCodeEntity` + `ResolveCodeEntityPreview` dispatch (`main.rs`).
38. Failure durumlarında revision/snapshot/resolution ledger/audit_sequence unchanged (atomic
    persistence envelope pinlenir).
39. 0 regression (`RUSTFLAGS="-D warnings"` temiz; mevcut accept/reject/supersede/preview unaffected).

## Test matrisi (~48 — tur 3: test revizyonu + BindingWrongKind + reuse inactive)

> Tur 3 P2-3: grup toplamı 8+4+5+4+10+3+4+4+6 = 48. 48 planned assertions/tests; net-new test
> function count implementation sırasında netleşir.

### identity_bridge unit (8)
```rust
# Mutlu yol + canonicalize idempotent + drift reject + enum mapping + determinism
to_core_identity_key_case_sensitive_passthrough
to_core_identity_key_ascii_case_insensitive_lowercase_idempotent
to_core_identity_key_control_char_rejects
path_case_policy_to_core_case_sensitive
path_case_policy_to_core_ascii_case_insensitive
analysis_scheme_path_v1_maps_to_core_analysis_path_v1_with_case_policy
to_core_identity_key_deterministic_round_trip
# Tur 2 P1-D / tur 3 P1-1A — empty yerine drift test (CLI constructor empty engeller)
to_core_identity_key_rejects_policy_canonicalization_drift
```

### Binding seeding integration (`analyze_bridge_flow.rs`, +4)
```rust
# graph init --analyze sonrası binding var; consistency; empty skip; round-trip INV-C16
graph_init_analyze_seeds_code_identity_bindings
graph_init_analyze_binding_candidate_consistency
graph_init_analyze_empty_analysis_no_bindings_warning
graph_init_analyze_snapshot_round_trip_with_bindings
```

### Candidate lookup/gate + compile-based (tur 2 P0-1, +5)
```rust
# Tur 2 P0-1 — candidate_query bug düzeltmesi pinlenir
accepted_candidate_is_resolvable_via_compile       # candidate_query DEĞİL compile
candidate_lane_node_rejects_as_not_accepted         # Candidate status → CandidateNotAccepted (tur 3 P1-3)
wrong_kind_rejects                                  # CodeEntityCandidate değilse → BindingWrongKind (tur 3 P1-2)
wrong_family_rejects                                # PhysicalCode değilse
missing_identity_binding_rejects                    # binding yoksa
```

### Target-pinning (tur 2 P0-2 + tur 3 P2-2 revizyon, +4)
```rust
# Tur 2 P0-2 / tur 3 P2-2 — operator-pinned target ↔ current target drift
resolve_rejects_create_to_reuse_target_drift        # confirmation Create, mutation Reuse (başka süreç live entity)
resolve_rejects_reuse_entity_digest_drift           # same entity_id, different entity_digest (content drift)
resolve_rejects_reuse_target_becoming_inactive      # tur 3 P2-2 — EntityNotLiveForResolution (Create'e düşmez)
preview_and_mutation_use_same_expected_target       # preview → expected_target round-trip
```

### resolve-code-entity one-shot unit/integration (`resolution_flow.rs`, +10)
```rust
# Mutlu yol Created + Reused + JSON + stale basis + not accepted + missing + already resolved
resolve_code_entity_created_mutlu_yol
resolve_code_entity_reused_mutlu_yol
resolve_code_entity_created_json_output
resolve_code_entity_candidate_not_accepted_rejects
resolve_code_entity_stale_candidate_digest_rejects
resolve_code_entity_already_resolved_rejects
resolve_code_entity_missing_identity_binding_rejects
# Tur 3 P0 — explicit target flag'leri zorunlu (colon-free)
resolve_code_entity_non_tty_requires_target_outcome_entity_id_digest
resolve_code_entity_operator_env_fallback
resolve_code_entity_confirmation_abort
```

### Minimal preview (tur 2 P0-2 + tur 3 P1-4 tek read motoru, +3)
```rust
# Tur 2 P0-2 / tur 3 P1-4 — execute_resolve_code_entity_preview target reveal (ReviewQuery)
preview_create_target_reveals_proposed_entity_id
preview_reuse_target_reveals_entity_id_and_digest
preview_and_render_use_single_canonical_builder      # üç yüzey tek renderer divergence YOK
```

### Store-policy errors (tur 2 — review eksik test önerisi, +4)
```rust
# Core policy'den emerge olan hata yolları
inactive_entity_rejects_without_create_fallback      # tur 4 P2-2 — inactive Create'e düşmez
duplicate_live_entity_rejects                        # R7 >1 live
entity_identity_collision_rejects                    # hash collision fail-closed
audit_sequence_exhaustion_maps_without_mutation
```

### Atomic persistence (tur 2 — review eksik test önerisi, +4 varyant × 4 assertion)
```rust
# Her failure için (stale basis, not accepted, already resolved, target drift):
# revision unchanged + snapshot unchanged + resolution ledger unchanged + audit_sequence unchanged
# Core no-fallible mutation block + CLI persistence envelope error durumunda dosya değiştirmez
atomic_persistence_stale_basis_unchanged
atomic_persistence_not_accepted_unchanged
atomic_persistence_already_resolved_unchanged
atomic_persistence_target_drift_unchanged
```

### Error mapping unit (tur 2 P1-A + tur 3 P1-2/P1-3, +6)
```rust
# Tur 2 P1-A + tur 3 P1-2 (BindingWrongKind) + tur 3 P1-3 (NotPromotableFrom split)
map_resolution_error_candidate_not_found_tuple
map_resolution_error_candidate_not_accepted_context_id
map_resolution_error_missing_identity_binding_context_id
map_resolution_error_already_resolved_context_id
map_resolution_store_error_binding_wrong_kind_maps_not_promotable    # tur 3 P1-2
map_resolution_store_error_not_promotable_from_status_split          # tur 3 P1-3 (non-Accepted vs Accepted)
```

## Uygulama sırası (tur 3 — explicit target flags + derleme düzeltmeleri + ReviewQuery + wizard)

0. `feat/cli-scheme-adoption` dalı aç (main `06d3a02` üstünde).
1. **A. `identity_bridge.rs`** — `AnalysisIdentityContext` (private fields + constructor — tur 3 P2-1) +
   `IdentityBridgeError` (drift; Eq derive YOK — tur 3 P1-1A) + `to_core_identity_key` +
   `From<PathCasePolicy>` impl + 8 unit test (drift test dahil). `BridgeError::IdentityBridge(#[from])` ekle.
   `main.rs` modül deklare.
2. **B. `analysis_bridge.rs`** — `CandidateProjectionOutput.code_identity_bindings` +
   `BridgeRunOutput.code_identity_bindings` + binding üretimi `try_new` sonrası validated/sorted
   candidate'lardan. `BridgeRunReport.projected_identity_bindings` (tur 2 P2-B).
3. **C. `commands/graph.rs`** — `seed_code_identity_bindings_trusted` çağrısı + ayrı stderr
   `identity bindings seeded: N`. 4 integration test.
4. **D. `errors.rs`** — `ExpectedResolutionTarget` + `ResolveCodeEntityCommand` (target dahil) +
   `ResolutionOutcomeView` (typed + `as_str()` — tur 3 P2-4) + `ResolveCodeEntityMutation` +
   `PersistedResolveCodeEntityOutput` + `ReviewError` yeni varyantlar.
5. **E. `application/review.rs`** — `execute_resolve_code_entity` + `apply_resolution`
   (compile-based) + `validate_expected_target` + `map_resolution_error(candidate_id, error)` +
   `map_resolution_store_error(candidate_id, source)` (tur 3 P1-2 BindingWrongKind + tur 3 P1-3
   NotPromotableFrom split). 6 error-mapping unit test.
6. **F-preview. `application/review.rs` + `commands/review.rs` + `commands/resolve_code_entity_preview_render.rs`**
   — `ResolutionPreviewOutput` + `ResolutionTargetPreview` (Serialize — tur 3 P1-1B) +
   `ReviewQuery::ResolveCodeEntityPreview` + `ReviewReadOutput::ResolveCodeEntityPreview` (tur 3 P1-4
   tek read motoru) + `expected_target()` infallible (tur 3 preview sadeleştirme) +
   `build_resolve_code_entity_preview` + `render_resolve_code_entity_preview_text` (body-only).
   3 preview unit test.
7. **F. `commands/review.rs`** — `ResolutionTargetOutcomeArg` (value_enum) + `ReviewResolveCodeEntityArgs`
   (explicit target flags — tur 3 P0) + `parse_expected_target` (validation matrisi) +
   `run_review_resolve_code_entity` (`as_str()` — tur 3 P2-4) + `confirm_with_resolution` +
   `ReviewResolveCodeEntityPreviewArgs` + `run_review_resolve_code_entity_preview`. 10 one-shot test.
8. **G. `main.rs`** — `ReviewAction::ResolveCodeEntity` + `ResolveCodeEntityPreview` + dispatch.
9. **H. `review_session.rs`** — `resolve <candidate>` interactive komutu (reason YOK — tur 3 P1-5;
   per-command session — tur 1 review karar #3).
10. Workspace `cargo test` (`RUSTFLAGS="-D warnings"`) — 0 regression doğrula.
11. HANDOFF.md + STATUS.md + run-metadata.md test count propagation (pre-commit checklist).
12. PR aç (`feat/cli-scheme-adoption` → `main`).

## HANDOFF bullet'leri (PR E2 sonrası)

- **Rich diagnostic preview future-work:** lineage, multi-blocker list, identity collision açıklama
  grafiği, candidate→entity ilişki geçmişi, batch uygunluk raporu, alternatif target açıklamaları
  (supersede-preview pattern'inin zengin analogu). V1 minimal canonical preview (target reveal) kapandı.
- **Batch resolution V2 aday:** `osp review resolve-code-entity --from-analysis` (tüm Accepted
  candidate'ları tek session'da resolve). Session-spanning lifetime (tur 1 review karar #3).
- **`PathCasePolicy` duplication debt:** CLI ↔ core enum duplication mapping katmanı (`identity_bridge.rs`)
  ile yönetiliyor; future cleanup core adopt (`PathCasePolicy` → `CodePathCasePolicy`).
- **Type-level policy mismatch garantisi (ileride):** `CanonicalCodeIdentity` hangi policy ile üretildiğini
  taşır veya identity + core key tek opaque projection result birlikte üretilir. Tur 3 P2-1 runtime
  guard yeterli; gerçek type-level garanti future-work.
- **Machine-readable CLI error envelope (ileride):** `operation` metadata taşıyan JSON envelope
  (`CandidateNotFound` varyantları yerine ölçeklenebilir — tur 1 review karar #4).
- **PR F evidence identity migration:** `ObservedCodeEvidence.code_entity_id` → `code_identity_key`;
  provider `CodeIdentityKey` merkezi. PR E2 binding seeding PR F'nin önkoşulu (evidence resolved
  entity'ye bağlanır).
- **PR G lineage-aware projection:** `Concept → Candidate → Entity` derived `ImplementedBy`.
  PR E2 `ResolvesTo` edge'leri PR G'nin girdisi.

## run-metadata: current protocol — osp-cli test counts update (+48: 8 identity_bridge + 4 binding
seeding + 5 candidate lookup/gate + 4 target-pinning + 10 resolve-code-entity + 3 preview + 4
store-policy + 4 atomic-persistence + 6 error-mapping); compile-fail unchanged (28); frozen snapshot
untouched (stratum 22).
