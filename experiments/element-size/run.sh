#!/usr/bin/env bash
# Element-size sweep -- paper Figure 3, column 4. See README.md.
#
# The size of every stored element grows from 512 B to 32 KB (N = 10^6
# elements, so the server holds up to about 33 GB). Input: the Zipf s = 1.0
# workload (shared with zipf-exponent). Each size runs at its own client
# rate.
#
#   ./run.sh                          full sweep, 5 repetitions, on the cluster
#   SMOKE=1 ./run.sh                  1 repetition, 200k requests per run
#   ONLY='^element-(512|1024)$' ./run.sh
#   LOCAL=1 SMOKE=1 ONLY='^element-512$' ./run.sh   on this machine (mind the RAM)
#   DRY_RUN=1 ./run.sh                print the commands only
set -euo pipefail
source "$(dirname "$0")/../common.sh"

CONFIG="$EXPERIMENT_DIR/configs/element-size.toml"
WORKLOAD_XZ="$CLOAK_ROOT/experiments/zipf-exponent/workloads/s1.0.xz"

# element size (bytes)   client rate (requests/s)
SWEEP="
512      171000
1024     148000
2048     94000
4096     53000
8192     23000
16384    9000
32768    2500
"

for rep in $(seq 1 "$REPS"); do
    cloak_rep "$rep"
    while read -r size rate; do
        [ -n "$size" ] || continue
        label="element-$size"
        cloak_selected "$label" || continue
        workload="$(cloak_workload "$WORKLOAD_XZ")"
        cloak_run "$label" "$CONFIG" "$workload" \
            --client-req-per-sec "$rate" --common-object-size "$size"
    done <<< "$SWEEP"
done
cloak_summary
