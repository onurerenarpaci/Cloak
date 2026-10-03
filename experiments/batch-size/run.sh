#!/usr/bin/env bash
# Batch-size sweep -- paper Figure 3, column 2. See README.md.
#
# The batch size is the sum of the per-reuse-distance-set budgets, which
# Cloak derives from the budget of the first set (--proxy-first-batch-size)
# with a Zipf s = 1.0 allocation. Input: the Zipf s = 1.0 workload (shared
# with zipf-exponent). Each batch size runs at its own client rate.
#
#   ./run.sh                          full sweep, 5 repetitions, on the cluster
#   SMOKE=1 ./run.sh                  1 repetition, 200k requests per run
#   ONLY='^batch-(2000|4000)$' ./run.sh
#   LOCAL=1 SMOKE=1 ./run.sh          on this machine
#   DRY_RUN=1 ./run.sh                print the commands only
set -euo pipefail
source "$(dirname "$0")/../common.sh"

CONFIG="$EXPERIMENT_DIR/configs/batch-size.toml"
WORKLOAD_XZ="$CLOAK_ROOT/experiments/zipf-exponent/workloads/s1.0.xz"

# label         first-set budget   client rate (requests/s)
SWEEP="
batch-2000      121                64000
batch-3000      282                126000
batch-4000      433                149000
batch-5000      583                144000
batch-6000      732                128000
batch-7000      888                113000
batch-8000      1046               108000
batch-9000      1207               105000
batch-10000     1370               94000
batch-11000     1537               92000
"

for rep in $(seq 1 "$REPS"); do
    cloak_rep "$rep"
    while read -r label first rate; do
        [ -n "$label" ] && cloak_selected "$label" || continue
        workload="$(cloak_workload "$WORKLOAD_XZ")"
        cloak_run "$label" "$CONFIG" "$workload" \
            --client-req-per-sec "$rate" --proxy-first-batch-size "$first"
    done <<< "$SWEEP"
done
cloak_summary
