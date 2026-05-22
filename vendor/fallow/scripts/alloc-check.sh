#!/usr/bin/env bash
#
# Allocation regression checker.
#
# Usage:
#   scripts/alloc-check.sh          # Compare against baseline
#   scripts/alloc-check.sh --save   # Run benchmark and save new baseline
#
# The baseline file is stored at alloc-baseline.json in the repo root.

set -euo pipefail

BASELINE_FILE="alloc-baseline.json"
THRESHOLD=10  # percent

# ── Run the allocation benchmark ────────────────────────────────────────
run_bench() {
    cargo bench --bench allocations 2>/dev/null
}

# ── Parse "key: value" lines from benchmark output ─────────────────────
parse_stats() {
    local output="$1"
    alloc_total_bytes=$(echo "$output" | grep '^alloc_total_bytes:' | awk '{print $2}')
    alloc_total_blocks=$(echo "$output" | grep '^alloc_total_blocks:' | awk '{print $2}')
    alloc_max_bytes=$(echo "$output" | grep '^alloc_max_bytes:' | awk '{print $2}')
    alloc_max_blocks=$(echo "$output" | grep '^alloc_max_blocks:' | awk '{print $2}')
}

# ── Save baseline ──────────────────────────────────────────────────────
save_baseline() {
    echo "Running allocation benchmark..."
    local output
    output=$(run_bench)
    parse_stats "$output"

    cat > "$BASELINE_FILE" <<EOF
{
  "alloc_total_bytes": $alloc_total_bytes,
  "alloc_total_blocks": $alloc_total_blocks,
  "alloc_max_bytes": $alloc_max_bytes,
  "alloc_max_blocks": $alloc_max_blocks
}
EOF

    echo "Baseline saved to $BASELINE_FILE"
    echo "  total_bytes:  $alloc_total_bytes"
    echo "  total_blocks: $alloc_total_blocks"
    echo "  max_bytes:    $alloc_max_bytes"
    echo "  max_blocks:   $alloc_max_blocks"
}

# ── Compare against baseline ──────────────────────────────────────────
check_regression() {
    if [ ! -f "$BASELINE_FILE" ]; then
        echo "No baseline file found at $BASELINE_FILE"
        echo "Run with --save to create one."
        exit 1
    fi

    echo "Running allocation benchmark..."
    local output
    output=$(run_bench)
    parse_stats "$output"

    # Read baseline values
    local base_total_bytes base_total_blocks base_max_bytes base_max_blocks
    base_total_bytes=$(python3 -c "import json; print(json.load(open('$BASELINE_FILE'))['alloc_total_bytes'])")
    base_total_blocks=$(python3 -c "import json; print(json.load(open('$BASELINE_FILE'))['alloc_total_blocks'])")
    base_max_bytes=$(python3 -c "import json; print(json.load(open('$BASELINE_FILE'))['alloc_max_bytes'])")
    base_max_blocks=$(python3 -c "import json; print(json.load(open('$BASELINE_FILE'))['alloc_max_blocks'])")

    local failed=0

    compare_metric() {
        local name="$1" current="$2" baseline="$3"
        if [ "$baseline" -eq 0 ]; then
            echo "  $name: $current (no baseline)"
            return
        fi
        local change
        change=$(python3 -c "print(round(($current - $baseline) / $baseline * 100, 1))")
        local sign=""
        if python3 -c "exit(0 if $current > $baseline else 1)" 2>/dev/null; then
            sign="+"
        fi
        echo "  $name: $current (baseline: $baseline, ${sign}${change}%)"

        # Check threshold
        if python3 -c "exit(0 if abs(($current - $baseline) / $baseline * 100) > $THRESHOLD else 1)" 2>/dev/null; then
            if python3 -c "exit(0 if $current > $baseline else 1)" 2>/dev/null; then
                echo "    WARNING: $name increased by more than ${THRESHOLD}%"
                failed=1
            fi
        fi
    }

    echo ""
    echo "Allocation comparison (threshold: ${THRESHOLD}%):"
    compare_metric "total_bytes"  "$alloc_total_bytes"  "$base_total_bytes"
    compare_metric "total_blocks" "$alloc_total_blocks" "$base_total_blocks"
    compare_metric "max_bytes"    "$alloc_max_bytes"    "$base_max_bytes"
    compare_metric "max_blocks"   "$alloc_max_blocks"   "$base_max_blocks"

    if [ "$failed" -ne 0 ]; then
        echo ""
        echo "FAIL: Allocation regression detected."
        echo "If this is expected, run: scripts/alloc-check.sh --save"
        exit 1
    fi

    echo ""
    echo "PASS: No allocation regression detected."
}

# ── Main ──────────────────────────────────────────────────────────────
case "${1:-}" in
    --save)
        save_baseline
        ;;
    *)
        check_regression
        ;;
esac
