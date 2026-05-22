#!/usr/bin/env bash
# Duplication Accuracy Baseline — evaluate fallow dupes against ground truth
#
# Runs fallow dupes in all 4 modes + defaults, captures JSON output.
# Then run evaluate-results.py to compute precision/recall.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CORPUS_DIR="$SCRIPT_DIR"
FALLOW_BIN="${FALLOW_BIN:-cargo run --bin fallow --}"
RESULTS_DIR="$SCRIPT_DIR/results"

mkdir -p "$RESULTS_DIR"

echo "=== Fallow Duplication Accuracy Baseline ==="
echo "Corpus: $CORPUS_DIR"
echo ""

run_mode() {
  local mode="$1"
  local min_tokens="${2:-30}"
  local min_lines="${3:-3}"
  local label="${4:-$mode}"
  local output_file="$RESULTS_DIR/dupes-${label}.json"

  echo "--- Running mode: $label ---"

  $FALLOW_BIN dupes \
    --mode "$mode" \
    --min-tokens "$min_tokens" \
    --min-lines "$min_lines" \
    --format json \
    --quiet \
    --no-cache \
    --root "$CORPUS_DIR" \
    > "$output_file" 2>/dev/null || true

  if [ -s "$output_file" ] && command -v python3 &>/dev/null; then
    local groups
    groups=$(python3 -c "import json,sys; print(len(json.load(sys.stdin)['clone_groups']))" < "$output_file" 2>/dev/null || echo "?")
    echo "  Clone groups found: $groups"
  fi
  echo ""
}

# Run all 4 modes with lowered thresholds for benchmark sensitivity
run_mode strict 30 3
run_mode mild 30 3
run_mode weak 30 3
run_mode semantic 30 3

# Also run with default settings for comparison
run_mode mild 50 5 defaults

echo "--- Raw results saved to $RESULTS_DIR ---"
echo ""
echo "=== Done. Run: python3 evaluate-results.py ==="
