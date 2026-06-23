#!/usr/bin/env bash
#
# OSP Corpus Reproducibility Script
#
# Clones the 15-repo corpus, generates SCIP semantic indices, runs the OSP analyzer,
# and produces a results table matching paper-draft.md Appendix.
#
# Prerequisites:
#   - Rust toolchain (cargo build --release)
#   - Docker (for scip-python)
#   - Node.js 16+ + npm (for scip-typescript: npm i -g @sourcegraph/scip-typescript)
#
# Usage:
#   bash scripts/reproduce-corpus.sh [output-dir]
#
# Output:
#   <output-dir>/results.tsv — tab-separated: repo, lang, nodes, edges, classes, A, I, D, y, coverage

set -euo pipefail

OUTPUT_DIR="${1:-./corpus-results}"
mkdir -p "$OUTPUT_DIR"
CORPUS_DIR="$OUTPUT_DIR/repos"
mkdir -p "$CORPUS_DIR"

# ── Repositories ───────────────────────────────────────────────────────────────

PYTHON_REPOS=(
  "pallets/click"
  "tiangolo/fastapi"
  "django/django"
  "psf/requests"
  "pallets/flask"
  "encode/httpx"
  "Textualize/rich"
  "pydantic/pydantic"
  "pipeposse/worms-supabase"
)

TSJS_REPOS=(
  "date-fns/date-fns"
  "chalk/chalk"
  "sveltejs/svelte"
  "tj/commander.js"
  "lodash/lodash"
  "vitest-dev/vitest"
)

ALL_REPOS=("${PYTHON_REPOS[@]}" "${TSJS_REPOS[@]}")

# ── Helper functions ───────────────────────────────────────────────────────────

clone_repo() {
  local full_name="$1"
  local name="${full_name##*/}"
  local target="$CORPUS_DIR/$name"

  if [ -d "$target" ]; then
    echo "  [skip] $name (already cloned)"
  else
    echo "  [clone] $name..."
    git clone --depth 1 "https://github.com/$full_name.git" "$target" 2>/dev/null
  fi
}

index_python() {
  local name="$1"
  local repo="$CORPUS_DIR/$name"
  echo "  [scip-python] $name..."
  docker run --rm -v "$repo:/repo" -w /repo \
    sourcegraph/scip-python:latest \
    /usr/local/bin/scip-python index . --output index.scip \
    --project-name "$name" --project-version 1.0.0 2>/dev/null
}

index_tsjs() {
  local name="$1"
  local repo="$CORPUS_DIR/$name"
  echo "  [scip-typescript] $name..."
  (cd "$repo" && scip-typescript index --output index.scip --infer-tsconfig 2>/dev/null) || true
}

analyze() {
  local name="$1"
  local repo="$CORPUS_DIR/$name"
  local scip="$repo/index.scip"

  if [ -f "$scip" ]; then
    "$BINARY" --scip "$scip" "$repo" 2>/dev/null | grep "^$name" || true
  else
    "$BINARY" "$repo" 2>/dev/null | grep "^$name" || true
  fi
}

# ── Main ───────────────────────────────────────────────────────────────────────

echo "════════════════════════════════════════════════════════════════"
echo "  OSP Corpus Reproducibility — 15 Repositories"
echo "════════════════════════════════════════════════════════════════"
echo ""

# Build release binary
echo "▶ Building osp-analyze (release)..."
cargo build --release --bin osp-analyze
BINARY="$(pwd)/target/release/osp-analyze"
echo ""

# Clone all repos
echo "▶ Cloning repositories..."
for repo in "${ALL_REPOS[@]}"; do
  clone_repo "$repo"
done
echo ""

# Generate SCIP indices
echo "▶ Generating SCIP indices..."
for repo in "${PYTHON_REPOS[@]}"; do
  name="${repo##*/}"
  index_python "$name"
done
for repo in "${TSJS_REPOS[@]}"; do
  name="${repo##*/}"
  index_tsjs "$name"
done
echo ""

# Analyze all repos
echo "▶ Running OSP analysis..."
echo ""
printf "%-16s %6s %5s %6s %6s %6s %6s %6s %6s\n" "repo" "nodes" "edges" "A" "I" "D" "y" "cov" "lang"

RESULTS_FILE="$OUTPUT_DIR/results.tsv"
echo -e "repo\tlang\tnodes\tedges\tA\tI\tD\ty\tcoverage" > "$RESULTS_FILE"

for repo in "${PYTHON_REPOS[@]}"; do
  name="${repo##*/}"
  result=$(analyze "$name" || echo "")
  if [ -n "$result" ]; then
    # Parse: name nodes edges κ A I D y [cov]
    echo "$result" | awk '{printf "%-16s %6s %5s %6s %6s %6s %6s %6s %6s\n", $1, $2, $3, $5, $6, $7, $8, $9, "Py"}'
    echo -e "$result\tPy" >> "$RESULTS_FILE"
  fi
done

for repo in "${TSJS_REPOS[@]}"; do
  name="${repo##*/}"
  lang="TS"
  case "$name" in
    chalk|lodash|commander*) lang="JS" ;;
  esac
  result=$(analyze "$name" || echo "")
  if [ -n "$result" ]; then
    echo "$result" | awk '{printf "%-16s %6s %5s %6s %6s %6s %6s %6s %6s\n", $1, $2, $3, $5, $6, $7, $8, $9, lang}' lang="$lang"
    echo -e "$result\t$lang" >> "$RESULTS_FILE"
  fi
done

echo ""
echo "▶ Results saved to: $RESULTS_FILE"
echo "▶ Full dataset: docs/scip-cohesion-results.md"
echo ""
echo "Done. Compare with paper-draft.md Appendix."
