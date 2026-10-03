#!/usr/bin/env bash
# Table 2: Cloak vs. the unsafe baseline on eight datasets. See README.md.
#
# For every repetition (outer loop) and dataset (inner loop) this runs Cloak
# and then the unsafe baseline (same config + --proxy-is-unsafe true), each at
# its own client rate from the table below. Results are appended to
# $RESULTS_DIR/<dataset>-cloak-stats.csv and <dataset>-unsafe-stats.csv
# (one row per repetition). The msr-prxy workload is not shipped: until
# prep/make_workloads.sh msr-prxy has installed workloads/msr-prxy.xz, its two
# labels are skipped with a warning (not counted as failed).
#
#   ./run.sh                              full run, 5 repetitions, on the cluster
#   SMOKE=1 ONLY='^netflix-' ./run.sh     quick check on truncated workloads
#   LOCAL=1 SMOKE=1 ./run.sh              on this machine
#   DRY_RUN=1 ./run.sh                    print the commands only
#   PROBE=1 ./run.sh                      one saturation run per mode at
#                                         $PROBE_RATE (default 200000 req/s);
#                                         labels <dataset>-{cloak,unsafe}-probe
set -euo pipefail
source "$(dirname "$0")/../common.sh"

# Client rate (--client-req-per-sec, requests/s) per dataset and mode, as used
# for the paper. Netflix, Ethereum and synthetic use the configs' fixed offered
# rate for both modes; the other datasets drive each mode at its own maximum
# sustainable rate (its saturation-probe throughput rounded down to 1,000).
#   dataset         cloak    unsafe
DATASETS="
netflix          175000   175000
ethereum         170000   170000
synthetic        155000   155000
twitter52        154000   149000
metakv           142000   156000
wikit            104000   159000
msr-prxy         166000   162000
alibaba-dev38     35000   183000
"

PROBE="${PROBE:-0}"
PROBE_RATE="${PROBE_RATE:-200000}"
suffix=""
if _cloak_true "$PROBE"; then
    REPS=1
    suffix="-probe"
fi

# The MSR Cambridge traces are not redistributed (see prep/README.md).
skip_msr=0
if [ ! -f "$EXPERIMENT_DIR/workloads/msr-prxy.xz" ]; then
    skip_msr=1
    labels=""
    for label in "msr-prxy-cloak$suffix" "msr-prxy-unsafe$suffix"; do
        if cloak_selected "$label"; then labels="$labels $label"; fi
    done
    # shellcheck disable=SC2086  # one argument per label
    cloak_skip "workloads/msr-prxy.xz not found; the MSR Cambridge traces are not redistributed, regenerate it with experiments/datasets/prep/make_workloads.sh msr-prxy (see experiments/datasets/prep/README.md, \"MSR prxy\")" $labels
fi

for rep in $(seq 1 "$REPS"); do
    cloak_rep "$rep"
    while read -r dataset cloak_rate unsafe_rate; do
        [ -n "$dataset" ] || continue
        if [ "$dataset" = msr-prxy ] && [ "$skip_msr" = 1 ]; then continue; fi
        cloak_label="$dataset-cloak$suffix"
        unsafe_label="$dataset-unsafe$suffix"
        cloak_selected "$cloak_label" || cloak_selected "$unsafe_label" || continue
        if _cloak_true "$PROBE"; then
            cloak_rate="$PROBE_RATE"
            unsafe_rate="$PROBE_RATE"
        fi
        config="$EXPERIMENT_DIR/configs/$dataset.toml"
        workload="$(cloak_workload "$EXPERIMENT_DIR/workloads/$dataset.xz")"
        cloak_run "$cloak_label" "$config" "$workload" \
            --client-req-per-sec "$cloak_rate"
        cloak_run "$unsafe_label" "$config" "$workload" \
            --proxy-is-unsafe true --client-req-per-sec "$unsafe_rate"
    done <<< "$DATASETS"
done
cloak_summary
