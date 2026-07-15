#!/usr/bin/env python3
"""
Paper 3 v1.4 dist — release-claim validator.

Converter (scripts/md_to_latex.py) mekanik donusumden sorumludur; bu validator
Paper 3 v1.4 dist'in *release claim'lerinden* (v1.3'te olmayan v1.4 icerigi
gercekten var, v1.3 icerigi kalmamis) sorumludur. Uc katmanli kapi:

    py scripts/validate_paper3_v14_dist.py source <markdown>
    py scripts/validate_paper3_v14_dist.py tex    <paper3.tex>
    py scripts/validate_paper3_v14_dist.py pdf    <paper3.pdf> [--build-log <log>]

Her katman ayni merkezi manifest'i kullanir (ayrisma yok). Her katman yalnizca
yapabilecegi kontrolu yapar:
  - source: prose + compact marker'lar (Markdown ham metni)
  - tex:    prose + compact marker'lar (LaTeX escape acilarak) + structural
            (Table EI/RP row key'leri, golden column-spec pattern'i)
  - pdf:    prose + compact marker'lar (PyMuPDF text extraction) + missing-glyph /
            undefined-reference log taramasi (build-log verilirse)

Exit code: 0 = PASS, 1 = FAIL. Ayrinti stdout'a.
"""
import argparse
import re
import sys
import unicodedata
from pathlib import Path


# ===========================================================================
# Merkezi manifest — Paper 3 v1.4 release claim'leri (tek kaynak)
# ===========================================================================

POSITIVE_PROSE_MARKERS = [
    "16 core binding-chain",
    "thirteen type-enforced and three runtime",
    "30 cumulative",
    "28 Paper-3-specific",
    "makes six contributions",
    # Bolum basliklari — converter sayisal onrugu soyar (3.5 Evidence-Identity
    # Invariants -> subsection{Evidence-Identity Invariants}), bu yuzden heading
    # govdesini assertions olarak kullaniyoruz.
    "Evidence-Identity Invariants",
    "Derived-Projection Invariants",
]

POSITIVE_COMPACT_MARKERS = [
    "c6_intent_cannot_form_observed_code_evidence",
]

NEGATIVE_PROSE_MARKERS = [
    "15 Paper-3-specific",
    "two runtime-asserted",      # v1.3 framing
    "24 cumulative",
    "makes four contributions",
]

NEGATIVE_COMPACT_MARKERS = [
    "c6_intent_carries_physical_vector",   # eski v1.3 fixture adi
    "v1.3",                                # stale version etiketi
]

# Structural (tex-layer): Table EI 8, Table RP 5 benzersiz ilk-kolon key.
STRUCTURAL = {
    "table_ei_row_keys": ["EI1", "EI2", "EI3", "EI4", "EI5", "EI6", "EI7", "EI8"],
    "table_rp_row_keys": ["RP1", "RP2", "RP3", "RP4-a", "RP4-b"],
}


# ===========================================================================
# Per-layer canonicalization
# (LaTeX escape tuzağı: identifier .tex'te c6\_intent... olur; semantic
#  string aramak icin escape'ler acilmali. PDF/Markdown'ta sorun yok.)
# ===========================================================================

def canonicalize_common(text):
    """NFKC normalize + soft-hyphen strip. Markdown ve PDF icin."""
    return unicodedata.normalize("NFKC", text).replace("\u00ad", "")


def canonicalize_tex(text):
    """LaTeX escape'leri ac: \\_ -> _, \\& -> &, \\# -> #, \\% -> %, \\$ -> $.
    Ardından common canonicalization."""
    text = canonicalize_common(text)
    return re.sub(r"\\([_&#%$])", r"\1", text)


def normalized(text):
    """Tum whitespace'leri tek bosluga indir + strip. Prose marker'lar icin."""
    return re.sub(r"\s+", " ", text).strip()


def compact(text):
    """Tum whitespace'leri sil. Identifier marker'lar icin."""
    return re.sub(r"\s+", "", text)


# ===========================================================================
# Assertion helpers
# ===========================================================================

def _check_markers(raw_text, canonicalize_fn, label):
    """Marker manifest kontrolu. (positive_count, negative_found_list, passed)"""
    canon = canonicalize_fn(raw_text)
    norm = normalized(canon)
    comp = compact(canon)

    failures = []

    # Positive prose: her biri >= 1
    for m in POSITIVE_PROSE_MARKERS:
        m_norm = normalized(m)
        count = norm.count(m_norm)
        if count < 1:
            failures.append(f"POS-PROSE MISSING ({label}): '{m}' (count {count})")

    # Positive compact: her biri >= 1
    for m in POSITIVE_COMPACT_MARKERS:
        m_comp = compact(m)
        count = comp.count(m_comp)
        if count < 1:
            failures.append(f"POS-COMPACT MISSING ({label}): '{m}' (count {count})")

    # Negative prose: her biri == 0
    for m in NEGATIVE_PROSE_MARKERS:
        m_norm = normalized(m)
        count = norm.count(m_norm)
        if count != 0:
            failures.append(f"NEG-PROSE PRESENT ({label}): '{m}' (count {count}, expected 0)")

    # Negative compact: her biri == 0
    for m in NEGATIVE_COMPACT_MARKERS:
        m_comp = compact(m)
        count = comp.count(m_comp)
        if count != 0:
            failures.append(f"NEG-COMPACT PRESENT ({label}): '{m}' (count {count}, expected 0)")

    return failures


def _extract_longtable_specs(tex_text):
    """\\begin{longtable}{SPEC} — SPEC'i matching-brace ile cikar (ic ice braces var)."""
    specs = []
    target = "begin{longtable}"
    idx = 0
    while True:
        pos = tex_text.find(target, idx)
        if pos == -1:
            break
        brace_open = tex_text.find("{", pos + len(target))
        if brace_open == -1:
            break
        depth = 0
        end = brace_open
        for j in range(brace_open, min(brace_open + 1000, len(tex_text))):
            if tex_text[j] == "{":
                depth += 1
            elif tex_text[j] == "}":
                depth -= 1
                if depth == 0:
                    end = j
                    break
        specs.append(tex_text[brace_open + 1:end])
        idx = pos + 1
    return specs


def _column_spec_pattern_ok(spec, first_col_prefixed_expected):
    """Golden column-spec *pattern* kontrolu (literal genislik degil, yapi).
    - @{} her iki uc ta (tam birer kez)
    - ilk kolon: first_col_prefixed_expected True ise >{raggedright} ile baslar,
      False ise bare p{ ile baslar
    - uzun kolonlar >{raggedright\arraybackslash}p{} formatinda
    """
    spec_norm = " ".join(spec.split())
    # @{} iki uc
    if not (spec_norm.startswith("@{}") and spec_norm.endswith("@{}")):
        return False, "missing @{} at both ends"
    # Ilk kolon kontrolu (@{} sonrasi). .tex'te tek backslash: >{\raggedright\arraybackslash}
    after_brace = spec_norm[3:].lstrip()
    if first_col_prefixed_expected:
        if not after_brace.startswith(r">{\raggedright\arraybackslash}"):
            return False, "first column should be prefixed but is bare"
    else:
        if not after_brace.startswith("p{"):
            return False, "first column should be bare but is prefixed/other"
    # En az bir raggedright var (uzun kolonlar)
    if "raggedright" not in spec_norm:
        return False, "no raggedright columns found"
    return True, ""


def _check_structural_tex(tex_text):
    """Table EI 8 row key + Table RP 5 row key + golden column-spec pattern.
    longtable'lar sirayla: [0]=Terminology(3col), [1]=Table1 INV, [2]=Table EI,
    [3]=Table RP, [4]=Table 2 Gate, [5]=Appendix A."""
    failures = []
    specs = _extract_longtable_specs(tex_text)

    if len(specs) < 6:
        failures.append(f"STRUCTURAL: expected >=6 longtables, found {len(specs)}")
        return failures

    # Table EI (specs[2]) — 4 kolon, ilk kolon INV key (EI1..EI8) bare degil,
    # ama ilk kolon kisa (EI1=3 char <=6) oldugu icin bare olmali. Row key'leri
    # tablo body'sinde ara.
    # Row key'leri .tex'te \textbf{EI1} veya \textbf{EI1} seklinde olabilir;
    # canonicalize_tex sonrasi \textbf{} kalir ama icindeki key acik.
    canon = canonicalize_tex(tex_text)

    # Table EI row keys
    for key in STRUCTURAL["table_ei_row_keys"]:
        # key bold satir basinda: \textbf{EI1} & ...
        pattern = r"\\textbf\{" + re.escape(key) + r"\}"
        if not re.search(pattern, canon):
            failures.append(f"STRUCTURAL EI: row key '{key}' not found as \\textbf{{{key}}}")

    # Table RP row keys (RP4-a, RP4-b tire icerir)
    for key in STRUCTURAL["table_rp_row_keys"]:
        pattern = r"\\textbf\{" + re.escape(key) + r"\}"
        if not re.search(pattern, canon):
            failures.append(f"STRUCTURAL RP: row key '{key}' not found as \\textbf{{{key}}}")

    # Golden column-spec pattern (yapi, literal genislik degil):
    #   [1] Table 1 INV: 4 kolon, ilk bare (C1..C13 kisa index)
    #   [2] Table EI: 4 kolon, ilk bare (EI1 kisa index)
    #   [3] Table RP: 4 kolon, ilk bare (RP1 kisa index)
    #   [4] Table 2 Gate: 6 kolon, ilk bare (1,2,3 kisa gate no)
    #   [5] Appendix A: 6 kolon, ilk PREFIXED (uzun "Sentence" ilk kolon)
    pattern_checks = [
        (1, "Table 1 INV", False),
        (2, "Table EI", False),
        (3, "Table RP", False),
        (4, "Table 2 Gate", False),
        (5, "Appendix A", True),
    ]
    for idx, label, first_prefixed in pattern_checks:
        if idx >= len(specs):
            failures.append(f"STRUCTURAL {label}: longtable index {idx} missing")
            continue
        ok, reason = _column_spec_pattern_ok(specs[idx], first_prefixed)
        if not ok:
            failures.append(f"STRUCTURAL {label} column-spec pattern: {reason}")

    return failures


# ===========================================================================
# Sub-command: source (Markdown)
# ===========================================================================

def cmd_source(args):
    md_path = Path(args.input)
    if not md_path.is_file():
        print(f"FAIL: source file not found: {md_path}", file=sys.stderr)
        return 1
    text = md_path.read_text(encoding="utf-8")
    failures = _check_markers(text, canonicalize_common, "source")
    if failures:
        print(f"FAIL ({md_path}):")
        for f in failures:
            print(f"  - {f}")
        return 1
    print(f"PASS source ({md_path}): {len(POSITIVE_PROSE_MARKERS)}+{len(POSITIVE_COMPACT_MARKERS)} positive, "
          f"{len(NEGATIVE_PROSE_MARKERS)}+{len(NEGATIVE_COMPACT_MARKERS)} negative absent")
    return 0


# ===========================================================================
# Sub-command: tex (LaTeX)
# ===========================================================================

def cmd_tex(args):
    tex_path = Path(args.input)
    if not tex_path.is_file():
        print(f"FAIL: tex file not found: {tex_path}", file=sys.stderr)
        return 1
    text = tex_path.read_text(encoding="utf-8")

    # Provenance header kontrolu
    if "GENERATED ARTIFACT" not in text:
        print(f"FAIL ({tex_path}): provenance header missing (GENERATED ARTIFACT)")
        return 1
    if "Paper version: v1.4" not in text:
        print(f"FAIL ({tex_path}): provenance header version mismatch (expected v1.4)")
        return 1

    failures = _check_markers(text, canonicalize_tex, "tex")
    failures += _check_structural_tex(text)

    if failures:
        print(f"FAIL ({tex_path}):")
        for f in failures:
            print(f"  - {f}")
        return 1
    print(f"PASS tex ({tex_path}): markers + structural (EI {len(STRUCTURAL['table_ei_row_keys'])} keys, "
          f"RP {len(STRUCTURAL['table_rp_row_keys'])} keys, golden col-spec patterns)")
    return 0


# ===========================================================================
# Sub-command: pdf (PyMuPDF + log scan)
# ===========================================================================

def cmd_pdf(args):
    try:
        import fitz  # PyMuPDF
    except ImportError:
        print("FAIL: PyMuPDF (fitz) not available — pip install pymupdf", file=sys.stderr)
        return 1

    pdf_path = Path(args.input)
    if not pdf_path.is_file():
        print(f"FAIL: pdf file not found: {pdf_path}", file=sys.stderr)
        return 1

    doc = fitz.open(str(pdf_path))
    page_count = doc.page_count
    if page_count <= 0:
        print(f"FAIL ({pdf_path}): page_count {page_count} (must be > 0)", file=sys.stderr)
        return 1

    # Tum sayfa text'ini cek
    full_text = ""
    for page in doc:
        full_text += page.get_text()
    doc.close()

    failures = _check_markers(full_text, canonicalize_common, "pdf")

    # Build log scan (missing glyph + undefined reference) — waiver'siz FAIL
    if args.build_log:
        log_path = Path(args.build_log)
        if log_path.is_file():
            log_text = log_path.read_text(encoding="utf-8", errors="replace")
            # Missing character
            missing = re.findall(r"Missing character", log_text, re.IGNORECASE)
            if missing:
                failures.append(f"PDF-LOG: {len(missing)} 'Missing character' warning(s) — must be 0")
            # Undefined reference/citation (hedefli pattern)
            undef = re.findall(
                r"undefined references|undefined citations|Citation .* undefined|Reference .* undefined",
                log_text, re.IGNORECASE)
            if undef:
                failures.append(f"PDF-LOG: {len(undef)} undefined reference/citation warning(s) — must be 0")
        else:
            print(f"WARN: build log not found: {log_path}", file=sys.stderr)

    if failures:
        print(f"FAIL ({pdf_path}, {page_count} pages):")
        for f in failures:
            print(f"  - {f}")
        return 1
    print(f"PASS pdf ({pdf_path}, {page_count} pages): markers present/absent"
          + (" + log scan clean" if args.build_log else ""))
    return 0


# ===========================================================================
# CLI
# ===========================================================================

def main():
    parser = argparse.ArgumentParser(
        prog="validate_paper3_v14_dist.py",
        description="Paper 3 v1.4 dist release-claim validator (source/tex/pdf).",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    p_source = sub.add_parser("source", help="Markdown source gate")
    p_source.add_argument("input", help="Markdown source path")

    p_tex = sub.add_parser("tex", help="LaTeX staging gate (markers + structural)")
    p_tex.add_argument("input", help="paper3.tex path")

    p_pdf = sub.add_parser("pdf", help="PDF gate (PyMuPDF + optional log scan)")
    p_pdf.add_argument("input", help="paper3.pdf path")
    p_pdf.add_argument("--build-log", help="Tectonic build log path (missing-glyph/undefined-ref scan)")

    args = parser.parse_args()

    if args.command == "source":
        return cmd_source(args)
    elif args.command == "tex":
        return cmd_tex(args)
    elif args.command == "pdf":
        return cmd_pdf(args)
    return 1


if __name__ == "__main__":
    sys.exit(main())
