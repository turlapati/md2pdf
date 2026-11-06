#!/usr/bin/env bash
# Render selected examples to PNG and compare against testdata/golden/.
#
# Usage:
#   bash scripts/golden-test.sh              # compare (fails on mismatch)
#   UPDATE_GOLDEN=1 bash scripts/golden-test.sh   # regenerate references
#
# Requires: md2pdf on PATH or target/debug|release/md2pdf, and pdftoppm (poppler).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GOLDEN_DIR="$ROOT/testdata/golden"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/md2pdf-golden.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

DATE="${GOLDEN_DATE:-2026-01-15}"
DPI="${GOLDEN_DPI:-96}"

find_md2pdf() {
  # Prefer the newest workspace build so an stale target/release binary cannot
  # shadow a freshly built target/debug (or the reverse).
  local candidates=()
  for c in \
    "$ROOT/target/release/md2pdf" \
    "$ROOT/target/release/md2pdf.exe" \
    "$ROOT/target/debug/md2pdf" \
    "$ROOT/target/debug/md2pdf.exe"
  do
    [[ -x "$c" ]] && candidates+=("$c")
  done
  if [[ ${#candidates[@]} -gt 0 ]]; then
    # shellcheck disable=SC2012
    ls -t "${candidates[@]}" | head -n1
    return
  fi
  if command -v md2pdf >/dev/null 2>&1; then
    command -v md2pdf
    return
  fi
  echo "md2pdf binary not found; run: cargo build -p md2pdf" >&2
  exit 1
}

if ! command -v pdftoppm >/dev/null 2>&1; then
  echo "pdftoppm not found (install poppler-utils / Poppler)" >&2
  exit 1
fi

MD2PDF="$(find_md2pdf)"
echo "using $MD2PDF"
mkdir -p "$GOLDEN_DIR"

# name|relative-md|extra-md2pdf-args|max-pages (empty = all)
CASES=(
  "kitchen-sink|examples/kitchen-sink.md|--toc -q|3"
  "math|examples/edge-cases/math.md|-q|"
  "html-block|examples/edge-cases/html-block.md|-q|"
  "unicode|examples/edge-cases/unicode.md|-q|"
  "dup-headings|examples/edge-cases/dup-headings.md|-q|"
  "heading-code|examples/edge-cases/heading-code.md|-q|"
  "table-pipe|examples/edge-cases/table-pipe.md|-q|"
  "typst-injection|examples/edge-cases/typst-injection.md|-q|"
  "code-in-list|examples/edge-cases/code-in-list.md|-q|"
  "quote-in-list|examples/edge-cases/quote-in-list.md|-q|"
)

fail=0

for case in "${CASES[@]}"; do
  IFS='|' read -r name md extra max_pages <<<"$case"
  pdf="$WORK/$name.pdf"
  # shellcheck disable=SC2086
  "$MD2PDF" "$md" -o "$pdf" --date "$DATE" $extra

  prefix="$WORK/$name"
  if [[ -n "$max_pages" ]]; then
    pdftoppm -png -r "$DPI" -f 1 -l "$max_pages" "$pdf" "$prefix"
  else
    pdftoppm -png -r "$DPI" "$pdf" "$prefix"
  fi

  shopt -s nullglob
  pages=("$prefix"-*.png)
  shopt -u nullglob
  if [[ ${#pages[@]} -eq 0 ]]; then
    echo "error: no pages rasterized for $name" >&2
    fail=1
    continue
  fi

  for page in "${pages[@]}"; do
    base="$(basename "$page")"
    ref="$GOLDEN_DIR/$base"
    if [[ "${UPDATE_GOLDEN:-}" == "1" ]]; then
      cp "$page" "$ref"
      echo "updated $base"
      continue
    fi
    if [[ ! -f "$ref" ]]; then
      echo "missing golden: $base (run UPDATE_GOLDEN=1 bash scripts/golden-test.sh)" >&2
      fail=1
      continue
    fi
    if ! cmp -s "$page" "$ref"; then
      echo "MISMATCH $base" >&2
      mkdir -p "$ROOT/testdata/golden-out"
      cp "$page" "$ROOT/testdata/golden-out/$base"
      fail=1
    else
      echo "ok $base"
    fi
  done
done

if [[ "${UPDATE_GOLDEN:-}" == "1" ]]; then
  echo "golden references written to $GOLDEN_DIR"
  exit 0
fi

if [[ "$fail" -ne 0 ]]; then
  echo "golden visual tests failed (actual PNGs in testdata/golden-out/)" >&2
  exit 1
fi
echo "all golden visual tests passed"
