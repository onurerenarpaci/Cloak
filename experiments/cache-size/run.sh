#!/usr/bin/env bash
# Cache-size sweep -- paper Figure 3, column 3. See README.md.
#
# Fixed client rate of 115,000 requests/s (below Cloak's maximum, so the
# cache's effect on latency is not masked by a full request queue) and a
# request queue of 1x the batch size (both set in the config); the proxy's
# cache (mini-moka: TinyLFU admission, LRU eviction) grows from 1,000 to
# 256,000 entries. Input: the Zipf s = 1.0 workload (shared with zipf-exponent).
#
#   ./run.sh                          full sweep, 5 repetitions, on the cluster
#   SMOKE=1 ./run.sh                  1 repetition, 200k requests per run
#   ONLY='^cache-(1000|256000)$' ./run.sh
#   LOCAL=1 SMOKE=1 ./run.sh          on this machine
#   DRY_RUN=1 ./run.sh                print the commands only
set -euo pipefail
source "$(dirname "$0")/../common.sh"

CONFIG="$EXPERIMENT_DIR/configs/cache-size.toml"
WORKLOAD_XZ="$CLOAK_ROOT/experiments/zipf-exponent/workloads/s1.0.xz"

CACHE_SIZES="1000 2000 4000 8000 16000 32000 64000 128000 256000"

for rep in $(seq 1 "$REPS"); do
    cloak_rep "$rep"
    for n in $CACHE_SIZES; do
        label="cache-$n"
        cloak_selected "$label" || continue
        workload="$(cloak_workload "$WORKLOAD_XZ")"
        cloak_run "$label" "$CONFIG" "$workload" --proxy-cache-size "$n"
    done
done
cloak_summary
