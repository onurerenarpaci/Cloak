#!/usr/bin/env bash
# Local smoke test: build Cloak, run one tiny repetition (server + proxy + client) on
# localhost and print the results. Extra args are passed to the binaries as config
# overrides, e.g. `local-test/run.sh --proxy-is-unsafe true` for the unsafe baseline.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLOAK_ROOT="$(cd "$HERE/.." && pwd)"
RESULTS="$HERE/results"

echo "== Building (cargo build --release)"
(cd "$CLOAK_ROOT" && cargo build --release)

echo "== Running one repetition on localhost"
mkdir -p "$RESULTS"
rm -f "$RESULTS/stats.csv" "$RESULTS/ratio.csv"
"$CLOAK_ROOT/scripts/run-local.sh" "$HERE/config.toml" "$HERE/workload.txt" "$RESULTS" "$@"

echo
echo "== $RESULTS/stats.csv"
cat "$RESULTS/stats.csv"
if [ -f "$RESULTS/ratio.csv" ]; then
    echo "== $RESULTS/ratio.csv"
    cat "$RESULTS/ratio.csv"
fi
echo

expected=$(grep -c . "$HERE/workload.txt")
summary=$(awk -F, -v n="$expected" 'NR == 2 {
    printf "%d/%d requests answered, throughput %.0f req/s, mean latency %.1f ms (std %.1f, p99 %s ms)",
        $1, n, $2, $3, $6, $7 }' "$RESULTS/stats.csv")
if [ -f "$RESULTS/ratio.csv" ]; then
    summary="$summary$(awk -F, 'NR == 2 {
        printf ", %d batches of size %d, mean batch utilization %.2f", $1, $4, $2 }' "$RESULTS/ratio.csv")"
fi
echo "OK: $summary"
