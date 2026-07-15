#!/usr/bin/env python3
"""
OSP Markdown -> LaTeX (Tectonic) converter.

Tek amaci: docs/papers/*.md kaynaklarini arXiv-friendly PDF'lere cevirmek.
LaTeX'i elle yazmiyoruz — bu script, Markdown yapisini LaTeX yapisina mekanik
olarak haritalar. Kapsam: basliklar, paragraflar, tablolar, kod bloklari,
inline kod, kalin/italik, matematik semboller (Unicode -> LaTeX).

Kullanim:
    py scripts/md_to_latex.py docs/papers/paper1-static-space.md --paper-version v2.6 > docs/dist/paper1.tex
    py scripts/md_to_latex.py docs/papers/paper2-agent-trajectory.md --paper-version v1.2 > docs/dist/paper2.tex
    py scripts/md_to_latex.py docs/papers/paper3-concept-anchoring.md --paper-version v1.4 > docs/dist/paper3.tex

Sonra Tectonic ile derle:
    tectonic docs/dist/paper1.tex
    tectonic docs/dist/paper2.tex
    tectonic docs/dist/paper3.tex

--paper-version TEK version kaynagidir. Provenance header (comment satiri olarak
cikti basina eklenir) ve PDF icindeki "OSP Paper N, vX.Y (Preprint)" etiketi
buradan uretilir. Eski kaynakla yeniden uretim yapilirken version flag'i o
kaynaganin yayinlanmis surumuyle uyumlu verilmelidir.
"""
import argparse
import re
import sys
import html
from pathlib import Path


# Longtable (4+ kolon) icin kolon-bazli ragged-right esigi. Bir kolonun en uzun
# hucresi bu degerden KUCUKSE (kisa index/etiket kolonu: INV, Gate no, vb.)
# bare p{} kalir; BUYUKSE (uzun serbest metin) >{\raggedright\arraybackslash}
# prefix eklenir. v1.3 elle-curated tablo davranisini deterministik olarak
# yeniden uretir (Table 1: col1 bare; Table 2: col1 bare; Appendix A: hepsi).
SHORT_COLUMN_MAX_LEN = 6


# Unicode -> LaTeX matematik sembol haritasi (makalelerde gecenler)
UNICODE_TO_LATEX = [
    # Once en spesifik olanlar (cokluk karsiliklari)
    ('≥', r'\ensuremath{\geq}'),
    ('≤', r'\ensuremath{\leq}'),
    ('≠', r'\ensuremath{\neq}'),
    ('→', r'\ensuremath{\rightarrow}'),
    ('←', r'\ensuremath{\leftarrow}'),
    ('↔', r'\ensuremath{\leftrightarrow}'),
    ('⇒', r'\ensuremath{\Rightarrow}'),
    ('⇐', r'\ensuremath{\Leftarrow}'),
    ('⇔', r'\ensuremath{\Leftrightarrow}'),
    ('θ', r'\ensuremath{\theta}'),
    ('Θ', r'\ensuremath{\Theta}'),
    ('λ', r'\ensuremath{\lambda}'),
    ('Λ', r'\ensuremath{\Lambda}'),
    ('μ', r'\ensuremath{\mu}'),
    ('σ', r'\ensuremath{\sigma}'),
    ('Σ', r'\ensuremath{\Sigma}'),
    ('δ', r'\ensuremath{\delta}'),
    ('Δ', r'\ensuremath{\Delta}'),
    ('ε', r'\ensuremath{\varepsilon}'),
    ('φ', r'\ensuremath{\varphi}'),
    ('Φ', r'\ensuremath{\Phi}'),
    ('ψ', r'\ensuremath{\psi}'),
    ('Ψ', r'\ensuremath{\Psi}'),
    ('ω', r'\ensuremath{\omega}'),
    ('Ω', r'\ensuremath{\Omega}'),
    ('π', r'\ensuremath{\pi}'),
    ('Π', r'\ensuremath{\Pi}'),
    ('α', r'\ensuremath{\alpha}'),
    ('β', r'\ensuremath{\beta}'),
    ('γ', r'\ensuremath{\gamma}'),
    ('Γ', r'\ensuremath{\Gamma}'),
    ('ρ', r'\ensuremath{\rho}'),
    ('τ', r'\ensuremath{\tau}'),
    ('ν', r'\ensuremath{\nu}'),
    ('κ', r'\ensuremath{\kappa}'),
    ('χ', r'\ensuremath{\chi}'),
    ('ξ', r'\ensuremath{\xi}'),
    ('Ξ', r'\ensuremath{\Xi}'),
    ('ζ', r'\ensuremath{\zeta}'),
    ('η', r'\ensuremath{\eta}'),
    ('ι', r'\ensuremath{\iota}'),
    ('∀', r'\ensuremath{\forall}'),
    ('∃', r'\ensuremath{\exists}'),
    ('∈', r'\ensuremath{\in}'),
    ('∉', r'\ensuremath{\notin}'),
    ('∉', r'\ensuremath{\notin}'),
    ('⊂', r'\ensuremath{\subset}'),
    ('⊆', r'\ensuremath{\subseteq}'),
    ('∪', r'\ensuremath{\cup}'),
    ('∩', r'\ensuremath{\cap}'),
    ('∅', r'\ensuremath{\emptyset}'),
    ('∞', r'\ensuremath{\infty}'),
    ('√', r'\ensuremath{\sqrt{}}'),
    ('×', r'\ensuremath{\times}'),
    ('·', r'\ensuremath{\cdot}'),
    ('±', r'\ensuremath{\pm}'),
    ('∓', r'\ensuremath{\mp}'),
    ('≈', r'\ensuremath{\approx}'),
    ('∼', r'\ensuremath{\sim}'),
    ('≡', r'\ensuremath{\equiv}'),
    ('∝', r'\ensuremath{\propto}'),
    ('∂', r'\ensuremath{\partial}'),
    ('∇', r'\ensuremath{\nabla}'),
    # Math letter symbols (blackboard bold etc.)
    ('ℝ', r'\ensuremath{\mathbb{R}}'),
    ('ℕ', r'\ensuremath{\mathbb{N}}'),
    ('ℤ', r'\ensuremath{\mathbb{Z}}'),
    ('ℚ', r'\ensuremath{\mathbb{Q}}'),
    ('ℂ', r'\ensuremath{\mathbb{C}}'),
    ('ℍ', r'\ensuremath{\mathbb{H}}'),
    ('𝔽', r'\ensuremath{\mathbb{F}}'),
    ('ℙ', r'\ensuremath{\mathbb{P}}'),
    # Modifier letters (small caps)
    ('ᴺ', r'\ensuremath{^N}'),
    ('ᴹ', r'\ensuremath{^M}'),
    ('ᴿ', r'\ensuremath{^R}'),
    ('ᵀ', r'\ensuremath{^T}'),
    ('ᴴ', r'\ensuremath{^H}'),
    # Modifier letters (superscript) — Unicode U+1D2x-1D4x range
    ('ᵏ', r'\ensuremath{^k}'),
    ('ᵃ', r'\ensuremath{^a}'),
    ('ᵇ', r'\ensuremath{^b}'),
    ('ᶜ', r'\ensuremath{^c}'),
    ('ᵈ', r'\ensuremath{^d}'),
    ('ᵉ', r'\ensuremath{^e}'),
    ('ᶠ', r'\ensuremath{^f}'),
    ('ᵍ', r'\ensuremath{^g}'),
    ('ʰ', r'\ensuremath{^h}'),
    ('ⁱ', r'\ensuremath{^i}'),
    ('ʲ', r'\ensuremath{^j}'),
    ('ˡ', r'\ensuremath{^l}'),
    ('ᵐ', r'\ensuremath{^m}'),
    ('ⁿ', r'\ensuremath{^n}'),
    ('ᵒ', r'\ensuremath{^o}'),
    ('ᵖ', r'\ensuremath{^p}'),
    ('ʳ', r'\ensuremath{^r}'),
    ('ˢ', r'\ensuremath{^s}'),
    ('ᵗ', r'\ensuremath{^t}'),
    ('ᵘ', r'\ensuremath{^u}'),
    ('ᵛ', r'\ensuremath{^v}'),
    ('ʷ', r'\ensuremath{^w}'),
    ('ˣ', r'\ensuremath{^x}'),
    ('ʸ', r'\ensuremath{^y}'),
    ('ᶻ', r'\ensuremath{^z}'),
    ('…', r'\dots'),
    ('—', '---'),
    ('–', '--'),
    (''', '`'),
    (''', "''"),
    # Sol cift quote (U+201C) -> `` (LaTeX left double quote)
    # Sag cift quote (U+201D) -> '' (LaTeX right double quote)
    ('\u201c', '``'),
    ('\u201d', "''"),
    ('«', r'\guillemotleft{}'),
    ('»', r'\guillemotright{}'),
    ('¿', r'?`'),
    ('¡', r'!`'),
    # Markdown ASCII quotes -> LaTeX curly quotes (SADECE texttt disinda)
    # Bu pattern asagida ozel olarak ele alinacak
    # Turkish characters — T1 fontunda bu glyph'ler yok, LaTeX komutlarına çevir
    # Python string: '\\c{s}' → LaTeX output '\c{s}' (tek backslash)
    ('ı', '{\\i}'),
    ('İ', '{\\I}'),
    ('ğ', '\\u{g}'),
    ('Ğ', '\\u{G}'),
    ('ş', '\\c{s}'),
    ('Ş', '\\c{S}'),
    ('ç', '\\c{c}'),
    ('Ç', '\\c{C}'),
    ('ö', '\\"{o}'),
    ('Ö', '\\"{O}'),
    ('ü', '\\"{u}'),
    ('Ü', '\\"{U}'),
    # Superscripts/subscripts
    ('⁰', r'\ensuremath{^0}'),
    ('¹', r'\ensuremath{^1}'),
    ('²', r'\ensuremath{^2}'),
    ('³', r'\ensuremath{^3}'),
    ('⁴', r'\ensuremath{^4}'),
    ('⁵', r'\ensuremath{^5}'),
    ('⁶', r'\ensuremath{^6}'),
    ('⁷', r'\ensuremath{^7}'),
    ('⁸', r'\ensuremath{^8}'),
    ('⁹', r'\ensuremath{^9}'),
    ('ⁿ', r'\ensuremath{^n}'),
    ('₀', r'\ensuremath{_0}'),
    ('₁', r'\ensuremath{_1}'),
    ('₂', r'\ensuremath{_2}'),
    ('₃', r'\ensuremath{_3}'),
    ('₄', r'\ensuremath{_4}'),
    ('₅', r'\ensuremath{_5}'),
    ('₆', r'\ensuremath{_6}'),
    ('₇', r'\ensuremath{_7}'),
    ('₈', r'\ensuremath{_8}'),
    ('₉', r'\ensuremath{_9}'),
    ('ₐ', r'\ensuremath{_a}'),
    ('ₑ', r'\ensuremath{_e}'),
    ('ₒ', r'\ensuremath{_o}'),
    ('ₓ', r'\ensuremath{_x}'),
    ('ₙ', r'\ensuremath{_n}'),
    ('ₘ', r'\ensuremath{_m}'),
    ('ₖ', r'\ensuremath{_k}'),
    ('ₗ', r'\ensuremath{_l}'),
    ('ₜ', r'\ensuremath{_t}'),
    ('ₚ', r'\ensuremath{_p}'),
    ('ₛ', r'\ensuremath{_s}'),
    ('ₕ', r'\ensuremath{_h}'),
    ('₊', r'\ensuremath{_+}'),
    ('₋', r'\ensuremath{_-}'),
    # Minus sign (U+2212) — normal hyphen'den farkli
    ('−', r'\ensuremath{-}'),
    # Tick isaretleri
    ('✓', r'\checkmark'),
    ('✗', r'$\times$'),
    ('✔', r'\checkmark'),
]


# LaTeX ozel karakterler (son halde, unicode donusumunden sonra calisir)
def escape_latex_special(text):
    """LaTeX ozel karakterlerini escape et (ama zaten LaTeX komutu olan yerleri bozma)."""
    # Once & # % _ ~ ^ escape (sira onemli — \ once yapilmamali)
    text = text.replace('&', r'\&')
    text = text.replace('#', r'\#')
    text = text.replace('%', r'\%')
    text = text.replace('_', r'\_')
    # ~ ve ^ artik kullanilmiyor (unicode donusumden sonra)
    return text


def transliterate_to_ascii(text):
    """Code span icindeki Unicode karakterleri LaTeX-safe karsiliklarina cevir.
    Texttt fontu (ec-lmtt10) cogu Unicode desteklemez; LaTeX accent komutlari
    texttt icinde guvenle calisir. Turkce karakterler icin ASCII'ye cevirmek yerine
    LaTeX accent representation kullaniriz — bu, dotted/dotless-I gibi argumanlarin
    gorunurlugunu korur (transliteration argumani bozmaz)."""
    # Turkce — LaTeX accent komutlari (texttt icinde guvenli)
    translit = {
        'ı': r'\i{}',      # dotless i
        'İ': r'\.{I}',     # dotted capital I
        'ğ': r'\u{g}',     # breve g
        'Ğ': r'\u{G}',     # breve G
        'ş': r'\c{s}',     # cedilla s
        'Ş': r'\c{S}',     # cedilla S
        'ç': r'\c{c}',     # cedilla c
        'Ç': r'\c{C}',     # cedilla C
        'ö': r'\"{o}',     # umlaut o
        'Ö': r'\"{O}',     # umlaut O
        'ü': r'\"{u}',     # umlaut u
        'Ü': r'\"{U}',     # umlaut U
        # Yaygin Avrupa dilleri
        'é': r'\'{e}', 'è': r'\`{e}', 'ê': r'\^{e}', 'ë': r'\"{e}',
        'á': r'\'{a}', 'à': r'\`{a}', 'â': r'\^{a}', 'ä': r'\"{a}',
        'ñ': r'\~{n}', 'Ñ': r'\~{N}',
        'ß': 'ss',
        # Matematik semboller (code icinde nadiren gecer — split_code_span_around_math
        # bunlari ayri isler, ama transliteration fallback olarak kalsin)
        '→': '->', '←': '<-', '≥': '>=', '≤': '<=', '≠': '!=',
        '≈': '~', '∼': '~', '…': '...', '—': '--', '–': '-',
        '"': '"', '"': '"', ''': "'", ''': "'",
        '°': 'deg', '×': 'x', '·': '.',
        # Box-drawing ve diagram karakterleri (§3.5/§3.6 ASCII diyagramlari)
        # texttt fontu (ec-lmtt10) bu glyph'leri desteklemez.
        '─': '-',   # U+2500 box drawings light horizontal
        '━': '=',   # U+2501 heavy horizontal
        '│': '|',   # U+2502 light vertical
        '┃': '|',   # U+2503 heavy vertical
        '┌': '+', '┐': '+', '└': '+', '┘': '+',   # corners
        '├': '+', '┤': '+', '┬': '+', '┴': '+', '┼': '+',  # tees
        '↓': 'v',   # U+2193 downwards arrow (diyagram akis)
        '↑': '^',   # U+2191 upwards arrow
    }
    for unicode_char, ascii_eq in translit.items():
        text = text.replace(unicode_char, ascii_eq)
    return text


# Code span icinde hâlâ kalan (transliterate'e girmeyen) Unicode matematik sembolleri.
# Texttt fontu (ec-lmtt10) bunlari gosteremez; bunlari texttt'tan cikarip \ensuremath{} icine gom.
# Ornek: `mainline_query() ∩ {X} = ∅` → \texttt{mainline_query()} $\cap$ \texttt{\{X\} =} $\emptyset$
CODE_SPAN_MATH_SYMBOLS = {
    '∈': r'\in',
    '∉': r'\notin',
    '∩': r'\cap',
    '∪': r'\cup',
    '∅': r'\emptyset',
    '⊆': r'\subseteq',
    '⊂': r'\subset',
    '⊇': r'\supseteq',
    '⊃': r'\supset',
    '∀': r'\forall',
    '∃': r'\exists',
    '∄': r'\nexists',
}


def split_code_span_around_math(code):
    """Code span metnini Unicode matematik sembolleri etrafinda bol.

    Texttt icine giremeyecek (fontun desteklemedigi) sembolleri tespit edip, metni
    onlarin etrafinda parcalara ayirir. Her parca:
      - matematik sembolu ise → ensuremath wrapped (texttt DISINDA)
      - digerleri → texttt'a gidecek (cagiran escape eder)
    Donus: (parcalar listesi) — her parca (tip, deger); tip 'math' veya 'text'.
    Cagiran, 'text' parcalarini \texttt{...} icine koyar, 'math' parcalarini oldugu gibi birakir.
    """
    if not any(sym in code for sym in CODE_SPAN_MATH_SYMBOLS):
        return [('text', code)]
    parts = []
    current = []
    for ch in code:
        if ch in CODE_SPAN_MATH_SYMBOLS:
            if current:
                parts.append(('text', ''.join(current)))
                current = []
            parts.append(('math', CODE_SPAN_MATH_SYMBOLS[ch]))
        else:
            current.append(ch)
    if current:
        parts.append(('text', ''.join(current)))
    return parts


def convert_unicode(text):
    """Unicode karakterleri LaTeX karsiliklarina cevir."""
    # Once compound pattern'leri ele al (ℝ^5, ℝ^(5+N), ℝ^N vb.)
    # Bu pattern'ler Unicode donusumunden ONCE matches
    # ℝ^(5+N) -> \ensuremath{\mathbb{R}^{5+N}}
    text = re.sub(r'ℤ\^\(([^)]+)\)', r'\\ensuremath{\\mathbb{Z}^{\1}}', text)
    text = re.sub(r'ℝ\^\(([^)]+)\)', r'\\ensuremath{\\mathbb{R}^{\1}}', text)
    text = re.sub(r'ℕ\^\(([^)]+)\)', r'\\ensuremath{\\mathbb{N}^{\1}}', text)
    text = re.sub(r'ℂ\^\(([^)]+)\)', r'\\ensuremath{\\mathbb{C}^{\1}}', text)
    # ℝ^5, ℝ^N (tek karakter) -> \ensuremath{\mathbb{R}^{5}}
    text = re.sub(r'ℝ\^(\w)', r'\\ensuremath{\\mathbb{R}^{\1}}', text)
    text = re.sub(r'ℤ\^(\w)', r'\\ensuremath{\\mathbb{Z}^{\1}}', text)
    text = re.sub(r'ℕ\^(\w)', r'\\ensuremath{\\mathbb{N}^{\1}}', text)
    text = re.sub(r'ℂ\^(\w)', r'\\ensuremath{\\mathbb{C}^{\1}}', text)

    for unicode_char, latex_seq in UNICODE_TO_LATEX:
        text = text.replace(unicode_char, latex_seq)
    return text


def process_inline(text):
    """Inline formatlamayi isle: bold, italic, code, links."""
    # Once LaTeX ozel karakterleri (ama code bloklarini koru)
    # Strateji: once code span'leri ayir, sonra escape, sonra geri koy

    # Code span'leri gecici olarak isaretle (control char KULLANMA — LaTeX invalid char)
    code_spans = []
    def save_code(m):
        code_spans.append(m.group(1))
        return f'ZZCODEMARKER{len(code_spans)-1}ZZ'

    text = re.sub(r'`([^`]+)`', save_code, text)

    # LaTeX escape
    text = escape_latex_special(text)

    # Unicode -> LaTeX
    text = convert_unicode(text)

    # Markdown inline -> LaTeX
    # Once \\ escape'lerini koru (\\* vb.) — bunlari gecici isaretle
    escaped_stars = []
    def save_escape(m):
        escaped_stars.append(m.group(0))
        return f'\x01ESC{len(escaped_stars)-1}\x01'

    # \\* \\_ \\# gibi escape'leri koru
    text = re.sub(r'\\[\*_\#]', save_escape, text)

    # Bold-italic once (***text***)
    text = re.sub(r'\*\*\*([^*]+)\*\*\*', r'\\textbf{\\textit{\1}}', text)
    # Bold (**text**) — satir sonuna kadar veya bir sonraki **'e kadar
    text = re.sub(r'\*\*([^*]+?)\*\*', r'\\textbf{\1}', text)
    # Italic (*text*) — dikkat: _ ile degil sadece * ile
    text = re.sub(r'(?<!\*)\*([^*\n]+?)\*(?!\*)', r'\\emph{\1}', text)
    # Strikethrough (~~text~~)
    text = re.sub(r'~~([^~]+?)~~', r'\\sout{\1}', text)

    # Markdown link [text](url) -> \href{url}{text}
    text = re.sub(r'\[([^\]]+)\]\(([^)]+)\)', r'\\href{\2}{\1}', text)

    # ASCII quotes -> LaTeX curly quotes (code span'ler zaten korundu)
    # "text" -> ``text''  (cift)
    text = re.sub(r'"([^"]+?)"', '``\\1\'\'', text)
    # 'text' -> `text'  (tek) — sadece kelime ortasi degilse
    text = re.sub(r"(?<!\w)'([^']+?)'(?!\w)", "`\\1'", text)

    # Escape'leri geri yukle
    def restore_escape(m):
        idx = int(m.group(1))
        return escaped_stars[idx]
    text = re.sub(r'\x01ESC(\d+)\x01', restore_escape, text)

    # Code span'leri geri koy — uzun identifier'ları kelimelere böl (her "break point"
    # sonrası boşluk bırakarak) ve her parçayı ayrı \texttt{} içine koy.
    # Bu, LaTeX'in boşluk noktalarında satır kırmasına izin verir.
    def restore_code(m):
        idx = int(m.group(1))
        code = code_spans[idx]
        # Escape LaTeX özel karakterleri ONCE (backslash dahil) — raw code'a uygula.
        # Sonra Turkce karakterleri LaTeX accent'e cevir; accent komutlarindaki \
        # bozulmaz cunku esc() zaten calisti.
        def esc(s):
            s = s.replace('\\', r'\textbackslash{}')
            s = s.replace('_', r'\_')
            s = s.replace('#', r'\#')
            s = s.replace('$', r'\$')
            s = s.replace('%', r'\%')
            s = s.replace('&', r'\&')
            s = s.replace('~', r'\textasciitilde{}')
            s = s.replace('^', r'\textasciicircum{}')
            return s
        code_escaped = esc(code)
        code_clean = transliterate_to_ascii(code_escaped)

        # Code span icinde hâlâ kalan (fontun gosteremedigi) Unicode matematik sembolleri
        # varsa, metni onlarin etrafinda bol: matematik kismi \ensuremath{}, digerleri texttt.
        segments = split_code_span_around_math(code_clean)

        # code_clean zaten esc() + transliterate() gecmis; tekrar escape ETME.
        # Identifier rendering kategorize (review §8): blanket >12 bolme yerine.
        def render_text_segment(seg):
            """Text segmenti icin kategori-bazli render:
            - Dosya yolu (/ icerir) → \\path{} (her karakterde kirilir)
            - Rust path (:: icerir) → tek \\texttt{}, :: sonrasi \\allowbreak
            - Kisa CamelCase / genel → tek \\texttt{} (parcalama yok)
            Uzun snake_case method isimleri (mainline_query vb.) \\texttt{} ile kalir;
            \\nolinkurl escape sorunlu (\\_ gibi LaTeX escape'leri URL baglaminda gecersiz).
            """
            if '/' in seg and len(seg) > 12:
                return r'\path{' + seg + '}'
            if '::' in seg:
                # Rust path: tek texttt, :: sonrasi allowbreak (taşarsa kir)
                parts = seg.split('::')
                if len(parts) == 2:
                    return r'\texttt{' + parts[0] + r'::\allowbreak ' + parts[1] + '}'
                return r'\texttt{' + seg + '}'
            return r'\texttt{' + seg + '}'

        # Hic matematik sembolu yoksa tek segment
        if len(segments) == 1 and segments[0][0] == 'text':
            return render_text_segment(segments[0][1])

        # Matematik sembolu var: her segmenti tipine gore isle.
        out_parts = []
        for seg_type, seg_val in segments:
            if seg_type == 'math':
                out_parts.append(r'\ensuremath{' + seg_val + '}')
            elif seg_val:
                out_parts.append(render_text_segment(seg_val))
        return ' '.join(out_parts)

    text = re.sub(r'ZZCODEMARKER(\d+)ZZ', restore_code, text)

    return text


def process_table(lines, start_idx):
    """Markdown tablosunu LaTeX tabloya cevir. (start_idx, end_idx, latex_str) dondur."""
    # Tablo basligini bul (| ile baslayan ilk satir)
    header_line = lines[start_idx].strip()
    # Separator satirini atla (---)
    end_idx = start_idx + 1
    if end_idx < len(lines) and re.match(r'^\s*\|[\s\-:|]+\|\s*$', lines[end_idx]):
        end_idx += 1

    # Veri satirlarini topla
    rows = []
    while end_idx < len(lines) and lines[end_idx].strip().startswith('|'):
        rows.append(lines[end_idx].strip())
        end_idx += 1

    # Header kolonlarini ayir
    def parse_row(row):
        # Bas ve sondaki | kaldir, split
        row = row.strip()
        if row.startswith('|'):
            row = row[1:]
        if row.endswith('|'):
            row = row[:-1]
        return [c.strip() for c in row.split('|')]

    header_cols = parse_row(header_line)
    n_cols = len(header_cols)

    # Kolon agirliklarini hesapla — SADECE icerik uzunluguna gore (header hariç)
    # Header kısa olabiliyor ama içerik uzun (Appendix A: header "Sentence",
    # içerik "Coupling must not exceed module threshold.")
    # Header'ı saymazsak dar kolonlara uzun içerikler sığmaz
    col_max_lens = []
    for col_idx in range(n_cols):
        # Header'ı minimum genişlik olarak al (bold, sığacak)
        header_len = len(header_cols[col_idx])
        # İçerikteki en uzun hücre
        content_max = 0
        for row in rows:
            cols = parse_row(row)
            if col_idx < len(cols):
                cell = cols[col_idx]
                # ** ve ` işaretlerini çıkarıp gerçek metin uzunluğu
                cell_clean = re.sub(r'[*`]', '', cell)
                content_max = max(content_max, len(cell_clean))
        # Header VE içerikten büyük olanı al
        max_len = max(header_len, content_max)
        col_max_lens.append(max_len)

    # Kolon tipleri: p{width} kullanarak uzun metinleri kaydir
    # Genis tablolar (3+ kolon veya geniş içerik) icin table* (full-width float) kullan
    # İçeriğin en uzun hücresi 30+ karakterse geniş tablo olarak işle
    max_cell_len = max(col_max_lens) if col_max_lens else 0
    use_full_width = n_cols >= 4 or (n_cols >= 3 and max_cell_len > 30)
    width_unit = r'\textwidth' if use_full_width else r'\columnwidth'

    # 6+ kolonlu tablolar için Manuel dengeli ağırlıklar (otomatik hesaplama yerine)
    # Otomatik hesaplama bazen görsel olarak dengesiz sonuç veriyor
    # (örn: uzun Sentence kolonu diğerlerini daraltıyor)
    # 6 kolon: Appendix A için optimal denge
    # Toplam = 0.95 (ufak margin)
    # Sentence (0.24) — uzun cümleler "Coupling must not exceed module threshold."
    # Canonical (0.16) — ModüllerArasıBağımlılık (20 char)
    # Normalized (0.15) — modullerarasibagimlilik (23 char)
    # Rule signal (0.10) — "true (must not)"
    # Ambiguity (0.15) — MultipleCandidates (18 char)
    # Axes (0.15) — [Coupling] (WRONG — physical pipe) (34 char, çok satır)
    if n_cols == 6:
        weights = [0.24, 0.16, 0.15, 0.10, 0.15, 0.15]
    else:
        total_len = sum(col_max_lens) or 1
        weights = [max(0.08, l / total_len) for l in col_max_lens]
        w_sum = sum(weights)
        weights = [w * 0.95 / w_sum for w in weights]

    if n_cols <= 2:
        col_spec = ' '.join([f'p{{0.45{width_unit}}}'] * n_cols)
    else:
        # 3+ kolon: icerik uzunluguna gore agirlikli bol
        cols = [f'p{{{w:.3f}{width_unit}}}' for w in weights]
        # Ragged-right + @{} kurali SADECE 4+ kolonlu longtable'lara uygulanir.
        # 3-kolonlu genis tablolar (Terminology: n_cols>=3 + uzun icerik) v1.3'te
        # bare oldugu icin converter-default'ta birakilir (zero-regression).
        # Kisa index kolonu (INV/Gate no <= SHORT_COLUMN_MAX_LEN) bare; uzun
        # kolonlara >{\raggedright\arraybackslash}; @{} iki uc ta padding kaldirir.
        if use_full_width and n_cols >= 4:
            prefixed = []
            for i, col in enumerate(cols):
                if col_max_lens[i] > SHORT_COLUMN_MAX_LEN:
                    # LaTeX tek backslash: >{\raggedright\arraybackslash}
                    prefixed.append(r'>{\raggedright\arraybackslash}' + col)
                else:
                    prefixed.append(col)
            col_spec = '@{} ' + ' '.join(prefixed) + ' @{}'
        else:
            col_spec = ' '.join(cols)

    # Genis tablolar (4+ kolon veya geniş içerik) icin longtable — sayfalar arasi bolunur.
    # Dar tablolar (<=2 kolon veya kisa icerik) table/tabular kalir.
    if use_full_width:
        # longtable: table/adjustbox ICINE konmaz; caption/label longtable icinde.
        # endfirsthead (ilk sayfa header), endhead (devam header), endfoot (devam alti),
        # endlastfoot (son sayfa alti) — profesyonel cok-sayfali tablo.
        font_cmd = r'\scriptsize'
        latex = []
        latex.append(r'\begingroup')
        latex.append(r'\setlength{\tabcolsep}{3pt}')
        latex.append(font_cmd)
        latex.append(r'\begin{longtable}{' + col_spec + '}')
        # Caption + label (longtable icinde)
        caption_text = ' '.join(process_inline(c) for c in header_cols) if n_cols <= 2 else None
        # Ilk sayfa header
        latex.append(r'\toprule')
        header_processed = [process_inline(c) for c in header_cols]
        latex.append(' & '.join(header_processed) + r' \\')
        latex.append(r'\midrule')
        latex.append(r'\endfirsthead')
        # Devam sayfalarinda header tekrar
        latex.append(r'\toprule')
        latex.append(' & '.join(header_processed) + r' \\')
        latex.append(r'\midrule')
        latex.append(r'\endhead')
        # Devam sayfalarinin alti
        latex.append(r'\midrule')
        latex.append(r'\multicolumn{' + str(n_cols) + r'}{r}{Continued on next page} \\')
        latex.append(r'\endfoot')
        # Son sayfanin alti
        latex.append(r'\bottomrule')
        latex.append(r'\endlastfoot')
        # Rows
        for row in rows:
            cols = parse_row(row)
            while len(cols) < n_cols:
                cols.append('')
            cols = cols[:n_cols]
            cols_processed = [process_inline(c) for c in cols]
            latex.append(' & '.join(cols_processed) + r' \\')
        latex.append(r'\end{longtable}')
        latex.append(r'\endgroup')
    else:
        # Dar tablo — table/tabular (tek sayfa)
        font_cmd = r'\small'
        latex = []
        latex.append(r'\begin{table}[htbp]')
        latex.append(r'\centering')
        latex.append(font_cmd)
        latex.append(r'\begin{tabular}{' + col_spec + '}')
        latex.append(r'\toprule')
        header_processed = [process_inline(c) for c in header_cols]
        latex.append(' & '.join(header_processed) + r' \\')
        latex.append(r'\midrule')
        for row in rows:
            cols = parse_row(row)
            while len(cols) < n_cols:
                cols.append('')
            cols = cols[:n_cols]
            cols_processed = [process_inline(c) for c in cols]
            latex.append(' & '.join(cols_processed) + r' \\')
        latex.append(r'\bottomrule')
        latex.append(r'\end{tabular}')
        latex.append(r'\end{table}')

    return end_idx, '\n'.join(latex)


def convert_markdown_to_latex(md_text, paper_num, title, author, orcid, version, date):
    """Ana donusum fonksiyonu."""
    lines = md_text.split('\n')
    output = []
    i = 0
    in_list = False
    in_quote = False
    in_code_block = False
    in_abstract = False
    in_appendix = False
    code_block_lines = []
    code_block_lang = ''

    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # Code block baslangici
        if stripped.startswith('```'):
            if not in_code_block:
                in_code_block = True
                code_block_lang = stripped[3:].strip()
                code_block_lines = []
                # Acik listeyi kapat
                if in_list:
                    output.append(r'\end{itemize}')
                    in_list = False
                if in_quote:
                    output.append(r'\end{quote}')
                    in_quote = False
                i += 1
                continue
            else:
                # Code block sonu
                in_code_block = False
                code_content = '\n'.join(code_block_lines)
                # Kategori A.5: lstlisting icinde escape YAPMA
                # lstlisting zaten raw text isler; _ # % & $ literal kalmali
                # Sadece Unicode karakterleri translit et (lstlisting fontu sinirli)
                code_content_clean = transliterate_to_ascii(code_content)
                output.append(r'\begin{lstlisting}[basicstyle=\ttfamily\footnotesize,breaklines=true,frame=single,backgroundcolor=\color{gray!8}]')
                output.append(code_content_clean)
                output.append(r'\end{lstlisting}')
                i += 1
                continue

        if in_code_block:
            code_block_lines.append(line)
            i += 1
            continue

        # Bos satir
        if not stripped:
            if in_list:
                output.append(r'\end{itemize}')
                in_list = False
            if in_quote:
                output.append(r'\end{quote}')
                in_quote = False
            i += 1
            continue

        # Kategori A.3: Editor notu / revision block'larini atla
        # **Revision:**, **Target:**, **Companion paper:**, **Prior v1**, **Authors:**, **Date:**
        # Bunlar paper body'sinde olmamali (preamble'da title/author/date zaten verildi)
        if re.match(r'^\*\*(Revision|Target|Companion paper|Prior v\d|Authors?|Date)\b', stripped):
            i += 1
            continue
        # "**OSP Paper Draft v2.6** · Target: arXiv" gibi version-only satirlari da atla
        if re.match(r'^\*\*OSP Paper.*Draft', stripped) and 'Target' in stripped:
            i += 1
            continue

        # Basliklar
        m = re.match(r'^(#{1,6})\s+(.*)$', line)
        if m:
            if in_list:
                output.append(r'\end{itemize}')
                in_list = False
            if in_quote:
                output.append(r'\end{quote}')
                in_quote = False
            level = len(m.group(1))
            heading_raw = m.group(2).strip()
            # Kategori A.1: Markdown'daki elle yazilmis numaralari soyma
            # "1. Introduction" -> "Introduction", "5.2 Assumptions" -> "Assumptions"
            # Pattern: ^\d+(\.\d+)*\.?\s+
            heading_clean = re.sub(r'^\d+(\.\d+)*\.?\s+', '', heading_raw)
            heading = process_inline(heading_clean)

            if level == 1:
                # \title{} zaten preamble'da verildi, atla
                i += 1
                continue
            elif level == 2:
                # Kategori A.2: Abstract ozel — \section yerine abstract environment
                if re.match(r'^abstract$', heading_clean, re.IGNORECASE):
                    #一旦 abstract environment aciksa kapat, yoksa ac
                    if in_abstract:
                        output.append(r'\end{abstract}')
                        in_abstract = False
                    else:
                        output.append(r'\begin{abstract}')
                        in_abstract = True
                elif heading_clean.lower().startswith('appendix'):
                    # Appendix — \appendix sonrasi section numaralandirma harf olur
                    if not in_appendix:
                        output.append(r'\appendix')
                        in_appendix = True
                    output.append(r'\section{' + heading + '}')
                else:
                    # Normal section — eger abstract aciksa kapat
                    if in_abstract:
                        output.append(r'\end{abstract}')
                        in_abstract = False
                    output.append(r'\section{' + heading + '}')
            elif level == 3:
                output.append(r'\subsection{' + heading + '}')
            elif level == 4:
                output.append(r'\subsubsection{' + heading + '}')
            else:
                output.append(r'\paragraph{' + heading + '}')
            i += 1
            continue

        # Yatay cizgi (---) -> section break
        if re.match(r'^---+\s*$', stripped) or re.match(r'^\*\*\*+\s*$', stripped):
            if in_list:
                output.append(r'\end{itemize}')
                in_list = False
            if in_quote:
                output.append(r'\end{quote}')
                in_quote = False
            output.append(r'\noindent\rule{\columnwidth}{0.4pt}')
            i += 1
            continue

        # Tablo
        if stripped.startswith('|') and i + 1 < len(lines) and re.match(r'^\s*\|[\s\-:|]+\|\s*$', lines[i+1]):
            if in_list:
                output.append(r'\end{itemize}')
                in_list = False
            if in_quote:
                output.append(r'\end{quote}')
                in_quote = False
            i, table_latex = process_table(lines, i)
            output.append(table_latex)
            continue

        # Block quote
        if stripped.startswith('>'):
            quote_content = stripped[1:].strip()
            if not in_quote:
                output.append(r'\begin{quote}')
                in_quote = True
            output.append(process_inline(quote_content))
            i += 1
            continue

        # List items
        m = re.match(r'^(\s*)([-*+]|\d+\.)\s+(.*)$', line)
        if m:
            if not in_list:
                output.append(r'\begin{itemize}')
                in_list = True
            item_content = process_inline(m.group(3).strip())
            output.append(r'\item ' + item_content)
            i += 1
            continue

        # Normal paragraf
        if in_list:
            output.append(r'\end{itemize}')
            in_list = False
        if in_quote:
            output.append(r'\end{quote}')
            in_quote = False

        # Yasanmis paragraph'i bas
        output.append(process_inline(stripped))
        i += 1

    # Acik kalanlari kapat
    if in_list:
        output.append(r'\end{itemize}')
    if in_quote:
        output.append(r'\end{quote}')
    if in_code_block:
        output.append(r'\end{lstlisting}')

    body = '\n\n'.join(output)

    # Preamble + body
    latex = build_preamble(title, author, orcid, version, date)
    latex += body + '\n\n'
    latex += r'\end{document}' + '\n'

    return latex


def build_preamble(title, author, orcid, version, date):
    """LaTeX preamble olustur."""
    title_escaped = process_inline(title)
    preamble = r"""\documentclass[11pt,a4paper]{article}

% ===== Paketler =====
\usepackage[T1]{fontenc}
\usepackage[utf8]{inputenc}
\usepackage{lmodern}
\usepackage{microtype}
\usepackage{graphicx}
\usepackage{xcolor}
\usepackage{geometry}
\usepackage{amsmath}
\usepackage{amssymb}
\usepackage{booktabs}
\usepackage{tabularx}
\usepackage{longtable}  % Cok satirli tablolar icin sayfalar arasi bolunur
\usepackage{array}
\usepackage{enumitem}
\usepackage{listings}
\usepackage{tcolorbox}
\usepackage[normalem]{ulem}  % for \sout (strikethrough)
\usepackage{adjustbox}  % Kategori A.6: tablo tasmasini onlemek icin
\usepackage{lscape}     % Landscape orientation for wide tables (sayfa döndürme)
\usepackage{url}        % \path{} ile uzun identifier'lar kirilabilir (her karakter)
\usepackage{hyperref}

\hypersetup{
    colorlinks=true,
    linkcolor=blue!70!black,
    citecolor=blue!70!black,
    urlcolor=blue!70!black,
    bookmarks=true,
    bookmarksnumbered=true,
    unicode=true,
    pdftitle={""" + title_escaped + r"""},
    pdfauthor={""" + author + r"""},
    pdfsubject={OSP Preprint},
    pdfkeywords={concept anchoring, requirements traceability, typestate, provenance, human-in-the-loop, AI coding agents}
}

\geometry{a4paper, top=2.5cm, bottom=2.5cm, left=2.5cm, right=2.5cm}

\setlength{\parindent}{0pt}
\setlength{\parskip}{0.6em}

% Listings (code blocks)
\lstset{
    basicstyle=\ttfamily\footnotesize,
    breaklines=true,
    frame=single,
    backgroundcolor=\color{gray!8},
    keywordstyle=\color{blue!60!black},
    commentstyle=\color{green!50!black},
    stringstyle=\color{red!60!black},
    showstringspaces=false,
    columns=fullflexible,
    keepspaces=true
}

% Reference styling
\renewcommand{\sectionautorefname}{Section}
\renewcommand{\subsectionautorefname}{Section}
\renewcommand{\subsubsectionautorefname}{Section}

% OSP code span
\newcommand{\ospcode}[1]{\texttt{#1}}

% Sloppy mode (uzun code span'lerde satır kırma esnekliği)
\sloppy
\emergencystretch=3em
\hbadness=10000
\hfuzz=2pt

\title{\textbf{\Large """ + title_escaped + r"""}}
\author{""" + author + r""" \\ ORCID: \texttt{""" + orcid + r"""}}
\date{""" + date + r"""}

\begin{document}
\maketitle

\begin{center}
\textit{""" + version + r"""}
\end{center}

"""
    return preamble


def build_provenance_header(input_path, paper_version):
    r"""Uretim provenance header'i — comment satirlari (\documentclass'tan once).
    Repo-relative source path (makine-ozel ciktiyol onler)."""
    # Repo-relative path: mevcut calma dizinine gore input_path'in relative yolunu bul.
    try:
        repo_root = Path.cwd()
        rel = input_path.resolve().relative_to(repo_root)
        source_str = str(rel).replace('\\', '/')
    except ValueError:
        # input_path repo disindaysa mutlak yol fallback
        source_str = str(input_path).replace('\\', '/')
    return (
        "% ============================================================\n"
        "% GENERATED ARTIFACT - do not edit content by hand.\n"
        f"% Source: {source_str}   (repo-relative)\n"
        f"% Paper version: {paper_version}  (derived by scripts/md_to_latex.py)\n"
        f"% Regenerate: py scripts/md_to_latex.py --paper-version {paper_version} <src.md> > docs/dist/paperN.tex\n"
        "% ============================================================\n"
    )


def main():
    parser = argparse.ArgumentParser(
        prog='md_to_latex.py',
        description='OSP Markdown -> LaTeX (Tectonic) converter.',
    )
    parser.add_argument('input_md', help='Markdown source file path')
    parser.add_argument(
        '--paper-version',
        required=True,
        help='Paper version label (e.g. v1.4). Tek version kaynagi; provenance header '
             've PDF etiketi buradan uretilir.',
    )
    args = parser.parse_args()

    input_path = Path(args.input_md)
    if not input_path.is_file():
        print(f"Error: input file not found: {input_path}", file=sys.stderr)
        sys.exit(1)
    md_text = input_path.read_text(encoding='utf-8')

    # Normalize CRLF -> LF
    md_text = md_text.replace('\r\n', '\n').replace('\r', '\n')

    # Paper metadata (path'ten tespit); version artik --paper-version flag'inden.
    name = input_path.name
    if 'paper-draft' in name or 'paper1' in name:
        paper_num = 1
        title = "Ontological Space Protocol: Modeling Software as a Conceptual Space with Epistemological Witnessing"
        date = "June 2026"
    elif 'paper2' in name:
        paper_num = 2
        title = "Architectural Trajectory Navigation: From Target Coordinates to Measurement Predicates"
        date = "July 2026"
    elif 'paper3' in name:
        paper_num = 3
        title = "Concept Anchoring: From Human Sentences to Bound Project Work"
        date = "July 2026"
    else:
        paper_num = 0
        title = "Untitled"
        date = "2026"

    # Version: tek kaynak --paper-version flag'i ("OSP Paper N, vX.Y (Preprint)")
    version = f"OSP Paper {paper_num if paper_num else 'N'}, {args.paper_version} (Preprint)"

    author = "Volkan Er"
    orcid = "0009-0001-3685-4820"

    latex = convert_markdown_to_latex(md_text, paper_num, title, author, orcid, version, date)

    # Provenance header'i cikti basina ekle (\documentclass'tan once - gecerli LaTeX comment).
    print(build_provenance_header(input_path, args.paper_version), end='')
    print(latex)


if __name__ == '__main__':
    main()
