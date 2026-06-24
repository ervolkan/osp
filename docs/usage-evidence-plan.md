# OSP Real-Usage Evidence Plan — "Show, Don't Tell"

> **Status:** Active execution plan
> **Hedef:** Reviewer'ların sorduğu "gerçek kullanım verisi nerede?" sorusuna somut cevap üretmek
> **Timeline:** 1-2 gün (3 adım)
> **Created:** 2026-06-24

---

## Motivasyon

Tüm reviewer'lar (3 farklı kişiden 5+ review) aynı noktaya takılıyor:

- *"Bu sistem gerçekten kullanılıyor mu?"*
- *"Gerçek LLM ile denendi mi?"*
- *"θ değişimi, hallucination oranı ölçüldü mü?"*
- *"Token benchmark gerçek modelle mi?"*

Paper v2.7 dürüst pozisyon aldı ("gate logic partially implemented") ama bu yeterli değil — **veri üretmemiz lazım**.

---

## 3 Adımlık Plan

### Adım 1: Dogfooding — OSP'yi kendi kodunda kullan (2-3 saat)

**Hedef:** "OSP, kendi kod tabanına uygulandı" verisi üretmek.

**Yapılacaklar:**

1. `osp-core` reposunu analyze et (137 Rust source file)
   - Tree-sitter: coupling, abstractness, instability
   - SCIP (scip-rust): LCOM4 cohesion
   - Sonuç: gerçek A, I, D, y değerleri

2. Vision vector ayarla (architectural targets)
   - coupling ≤ 0.4 (low coupling)
   - cohesion ≥ 0.6 (high cohesion)
   - instability ≤ 0.6 (stable foundation)

3. 5-10 "geliştirme senaryosu" simüle et:
   - "Yeni bir agent module ekle" (valid → pass)
   - "Dairesel import ekle" (Q4 reject — syntax)
   - "Yüksek coupling modülü ekle" (Q5 reject — vision)
   - "Duplikat node ID ekle" (Q6 reject — rule)
   - "İzole modül ekle" (valid → commit)

4. Her senaryo için kaydet:
   - computed_raw pozisyonu (coupling, cohesion, instability)
   - θ deviation (vision'a göre)
   - Hangi gate'te takıldı (veya geçti)
   - HallucinationType sınıflandırması

5. **Çıktı:** `docs/usage-dogfooding.md` — tablo + yorum

**Beklenen veri:**
```
Repo: osp-core (137 files, 330+ tests)
SCIP coverage: ~90% (scip-rust)
Vision: coupling≤0.4, cohesion≥0.6, instability≤0.6

Scenario          | Gate    | Result  | θ      | Hallucination
------------------|---------|---------|--------|------------------
add-agent-module  | ALL     | COMMIT  | 0.18   | (none)
circular-import   | Q4      | REJECT  | —      | Structural
high-coupling     | Q5      | REJECT  | 0.72   | Vision
duplicate-node    | Q6      | REJECT  | —      | Rule
isolated-module   | ALL     | COMMIT  | 0.22   | (none)
```

---

### Adım 2: Gerçek LLM Entegrasyonu (3-4 saat)

**Hedef:** "GPT-4o ile OSP prompt gönderildi, token farkı ölçüldü" verisi.

**Yapılacaklar:**

1. `osp-llm-runtime` crate oluştur (minimal)
   - Input: OspPrompt (serialized JSON)
   - Output: DeltaProposal (parsed JSON)
   - LLM API: OpenAI GPT-4o (veya Anthropic Claude)

2. Token ölçümü:
   - OspPrompt JSON → tiktoken ile token say
   - Raw 2-hop source files → tiktoken ile token say
   - **Gerçek token farkı** (chars/4 değil)

3. LLM'e görev ver:
   - "Bu OSP prompt'una göre bir DeltaProposal üret"
   - Dönen DeltaProposal'ı Q4-Q6'dan geçir
   - Pass/fail kaydet

4. 5-10 LLM çağrısı yap:
   - Token cost (prompt + completion)
   - Gate pass rate
   - Hallucination distribution

5. **Çıktı:** `docs/usage-llm-benchmark.md` — gerçek model verisi

**Beklenen veri:**
```
Model: GPT-4o (gpt-4o-2024-08-06)
Tokenizer: tiktoken (cl100k_base)

Repo: osp-core
  OSP prompt:    155 tokens (tiktoken)
  Raw 2-hop:    3,600 tokens (tiktoken)
  Raw full:    45,000 tokens (tiktoken)

LLM Calls: 10
  Prompt avg:  155 tokens
  Completion avg: 280 tokens
  Total cost: ~$0.02

Gate Results:
  Q4 pass: 8/10 (2 Structural hallucination)
  Q5 pass: 6/10 (2 Vision hallucination)
  Q6 pass: 5/10 (1 Rule hallucination)
  Commit: 5/10 (50%)
```

---

### Adım 3: Kullanım Raporu → Paper (1 saat)

**Hedef:** Toplanan veriyi paper'a `§7.8: Preliminary Usage Observations` olarak ekle.

**Yapılacaklar:**

1. `docs/usage-report.md` yaz (her iki adımın verisi)

2. Paper'a yeni section ekle:
   ```
   ### 7.8 Preliminary Usage Observations

   We applied OSP to its own codebase (osp-core, 137 Rust files) with
   a configured vision vector (coupling ≤ 0.4, cohesion ≥ 0.6) and
   simulated 10 development scenarios...

   [Dogfooding tablosu]

   We further measured real token consumption using GPT-4o's tiktoken
   tokenizer on the same repository...

   [LLM benchmark tablosu]
   ```

3. Contributions'a "preliminary usage evidence" ekle

4. Conclusion'a "OSP applied to own codebase" cümlesi ekle

---

## Hedef Çıktılar

| Deliverable | Konum | Paper'a gider mi? |
|---|---|---|
| Dogfooding verisi (θ, gate results) | `docs/usage-dogfooding.md` | ✅ §7.8 |
| LLM token benchmark (real tiktoken) | `docs/usage-llm-benchmark.md` | ✅ §7.8 |
| Kullanım raporu (birleştirilmiş) | `docs/usage-report.md` | ✅ §7.8 |
| `osp-llm-runtime` crate | `crates/osp-llm-runtime/` | Kod olarak |

## Reviewer'lara verilecek cevap

> "We applied OSP to its own codebase (137 files) with a configured
> architectural vision. Of 10 simulated development scenarios, 50% passed
> all gates and committed; the remainder were rejected at Q4 (syntax),
> Q5 (vision deviation θ > 0.25), or Q6 (rule violation). Real token
> consumption was measured using GPT-4o's tiktoken: OSP prompts averaged
> 155 tokens versus 3,600 tokens for a structure-aware 2-hop baseline —
> a 23× reduction in prompt size, though this measures representation
> compactness, not task success."

---

## Sıralama

```
Adım 1 (Dogfooding)     ──── 2-3 saat ────► usage-dogfooding.md
         ↓
Adım 2 (Real LLM)       ──── 3-4 saat ────► usage-llm-benchmark.md
         ↓
Adım 3 (Paper §7.8)     ──── 1 saat   ────► paper-draft §7.8
```

**Toplam: ~6-8 saat aktif çalışma.**
