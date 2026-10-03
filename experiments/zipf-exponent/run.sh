#!/usr/bin/env bash
# Zipf-exponent sweep -- paper Figure 3, column 1. See README.md.
#
# Cloak's budgets always anticipate Zipf s = 1.0; the input workload's
# exponent varies from 0.0 (uniform) to 2.0. Each workload runs at its own
# client rate (close to the maximum Cloak sustains for it).
#
#   ./run.sh                          full sweep, 5 repetitions, on the cluster
#   SMOKE=1 ./run.sh                  1 repetition, 200k requests per run
#   ONLY='^s1\.[05]$' ./run.sh        a subset of labels
#   LOCAL=1 SMOKE=1 ./run.sh          on this machine
#   DRY_RUN=1 ./run.sh                print the commands only
set -euo pipefail
source "$(dirname "$0")/../common.sh"

CONFIG="$EXPERIMENT_DIR/configs/zipf-exponent.toml"

# label = workload file (workloads/<label>.xz)   client rate (requests/s)
SWEEP="
s0.0  30000
s0.1  33000
s0.2  34000
s0.3  36000
s0.4  40000
s0.5  44000
s0.6  50000
s0.7  56000
s0.8  76000
s0.9  104000
s1.0  151000
s1.1  168000
s1.2  169000
s1.3  169000
s1.4  169000
s1.5  169000
s1.6  169000
s1.7  169000
s1.8  169000
s1.9  169000
s2.0  169000
"

for rep in $(seq 1 "$REPS"); do
    cloak_rep "$rep"
    while read -r label rate; do
        [ -n "$label" ] && cloak_selected "$label" || continue
        workload="$(cloak_workload "$EXPERIMENT_DIR/workloads/$label.xz")"
        cloak_run "$label" "$CONFIG" "$workload" --client-req-per-sec "$rate"
    done <<< "$SWEEP"
done
cloak_summary
