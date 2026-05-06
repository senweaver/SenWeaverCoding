#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Bench guard: parses criterion JSON output and fails if p95 latency regression exceeds threshold.
# Usage: ./scripts/bench_guard.sh <benchmark_binary> <base_json> <threshold_pct>
#
# Example:
#   # Capture baseline
#   cargo bench --bench turn_streamed -- --measurement-time=1 > /dev/null 2>&1
#   cp target/criterion/turn_streamed/benchmark.json baseline_turn_streamed.json
#
#   # Compare against baseline (fails if p95 regressed by > 10%)
#   ./scripts/bench_guard.sh turn_streamed baseline_turn_streamed.json 10

set -euo pipefail

BENCHMARK="${1:-}"
BASE_JSON="${2:-}"
THRESHOLD_PCT="${3:-10}"

if [[ -z "$BENCHMARK" || -z "$BASE_JSON" ]]; then
    echo "Usage: $0 <benchmark_name> <baseline_json> [threshold_pct=10]" >&2
    echo "  benchmark_name: name of criterion benchmark (e.g. turn_streamed)" >&2
    echo "  baseline_json:  path to baseline benchmark.json (from criterion --save-baseline)" >&2
    echo "  threshold_pct:  max allowed p95 regression % (default: 10)" >&2
    exit 1
fi

REPORT_DIR="target/criterion/${BENCHMARK}/report"
CURRENT_JSON="${REPORT_DIR}/benchmark.json"

if [[ ! -f "$CURRENT_JSON" ]]; then
    echo "ERROR: No benchmark report found at ${CURRENT_JSON}" >&2
    echo "Run 'cargo bench --bench ${BENCHMARK}' first." >&2
    exit 1
fi

if [[ ! -f "$BASE_JSON" ]]; then
    echo "ERROR: Baseline JSON not found: ${BASE_JSON}" >&2
    exit 1
fi

# Extract p95 mean (nanoseconds) from baseline
BASE_P95=$(jq -r '.statistics[0].percentiles."0.95"' "$BASE_JSON" 2>/dev/null)
if [[ -z "$BASE_P95" || "$BASE_P95" == "null" ]]; then
    # Fallback: try the mean if percentiles not available
    BASE_P95=$(jq -r '.statistics[0].mean' "$BASE_JSON" 2>/dev/null)
fi

CURRENT_P95=$(jq -r '.statistics[0].percentiles."0.95"' "$CURRENT_JSON" 2>/dev/null)
if [[ -z "$CURRENT_P95" || "$CURRENT_P95" == "null" ]]; then
    CURRENT_P95=$(jq -r '.statistics[0].mean' "$CURRENT_JSON" 2>/dev/null)
fi

if [[ -z "$BASE_P95" || "$BASE_P95" == "null" || -z "$CURRENT_P95" || "$CURRENT_P95" == "null" ]]; then
    echo "ERROR: Could not extract p95 values from JSON (checked both percentiles.95 and mean)" >&2
    echo "  Baseline: $BASE_JSON" >&2
    echo "  Current:  $CURRENT_JSON" >&2
    exit 1
fi

# Calculate regression percentage
python3 -c "
import sys
base = float('$BASE_P95')
current = float('$CURRENT_P95')
pct_change = ((current - base) / base) * 100
print(f'base_p95={base:.2f}')
print(f'current_p95={current:.2f}')
print(f'pct_change={pct_change:.2f}%')
print(f'threshold={$THRESHOLD_PCT}%')
if pct_change > $THRESHOLD_PCT:
    print('REGRESSION: p95 latency increased by {:.1f}% (threshold: {}%)'.format(pct_change, $THRESHOLD_PCT))
    sys.exit(1)
elif pct_change < -$THRESHOLD_PCT:
    print('IMPROVEMENT: p95 latency decreased by {:.1f}%'.format(abs(pct_change)))
    sys.exit(0)
else:
    print('OK: p95 within threshold')
    sys.exit(0)
" 2>&1

exit $?
