#!/usr/bin/env bash
# Figures 2 and 4: temporal-locality (reuse-interval) histograms.
#
# Offline analysis, runs locally (no cluster, no Cloak binaries). For each of
# the seven real-world traces it reads the workload of the datasets
# experiment (../datasets/workloads/<dataset>.xz, decompressed on the fly),
# writes the histogram to $RESULTS_DIR/<dataset>.txt and finally renders the
# figures with plots/fig2_fig4_temporal_locality.py into plots/out/.
# msr-prxy is not shipped: it is skipped with a warning (its panel is marked
# "not computed") until ../datasets/prep/make_workloads.sh msr-prxy has
# installed it. See README.md for the histogram definition and expected results.
#
#   ./run.sh                          all seven traces (~1 min, < 0.5 GB RAM)
#   SMOKE=1 ./run.sh                  first $SMOKE_REQUESTS requests per trace,
#                                     histograms in ./results-smoke
#   ONLY='msr|alibaba' ./run.sh       only datasets matching the regex
#   DRY_RUN=1 ./run.sh                print what would run
#
# Other knobs: RESULTS_DIR (default ./results, or ./results-smoke with
# SMOKE=1), WORKLOADS_DIR (default ../datasets/workloads), MAX_BINS (default
# 1000000), SMOKE_REQUESTS (default 200000), PLOT=0 (skip the plot step),
# OUT_DIR (figure directory, default plots/out), PYTHON (interpreter command;
# default `uv run --project <Cloak root> python` if uv is installed, else
# python3). SMOKE, DRY_RUN and PLOT accept 1/true/yes (0/false/no/off or
# empty mean off), like the harness in ../common.sh.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
CLOAK_ROOT="$(cd "$HERE/../.." && pwd)"

DATASETS=(netflix ethereum twitter52 metakv wikit msr-prxy alibaba-dev38)

# same truthiness rule as _cloak_true in ../common.sh
is_true() { case "${1:-}" in "" | 0 | false | no | off) return 1 ;; *) return 0 ;; esac; }

SMOKE="${SMOKE:-0}"
if is_true "$SMOKE"; then
  RESULTS_DIR="${RESULTS_DIR:-$HERE/results-smoke}"
else
  RESULTS_DIR="${RESULTS_DIR:-$HERE/results}"
fi
WORKLOADS_DIR="${WORKLOADS_DIR:-${HERE%/*}/datasets/workloads}"
MAX_BINS="${MAX_BINS:-1000000}"
SMOKE_REQUESTS="${SMOKE_REQUESTS:-200000}"
ONLY="${ONLY:-}"
DRY_RUN="${DRY_RUN:-0}"
PLOT="${PLOT:-1}"
OUT_DIR="${OUT_DIR:-}"

if [[ -n "${PYTHON:-}" ]]; then
  read -r -a PY <<<"$PYTHON"
elif command -v uv >/dev/null 2>&1; then
  PY=(uv run --project "$CLOAK_ROOT" python)
else
  PY=(python3)
fi

run() {  # print, and execute unless DRY_RUN is set
  echo "+ $*"
  is_true "$DRY_RUN" || "$@"
}

limit_args=()
if is_true "$SMOKE"; then
  limit_args=(--limit "$SMOKE_REQUESTS")
  echo "SMOKE: using the first $SMOKE_REQUESTS requests of each trace -> $RESULTS_DIR"
fi

is_true "$DRY_RUN" || mkdir -p "$RESULTS_DIR"

for dataset in "${DATASETS[@]}"; do
  [[ -z "$ONLY" || "$dataset" =~ $ONLY ]] || continue
  input="$WORKLOADS_DIR/$dataset.xz"
  [[ -f "$input" || ! -f "$WORKLOADS_DIR/$dataset" ]] || input="$WORKLOADS_DIR/$dataset"
  if [[ ! -f "$input" && "$dataset" == msr-prxy ]]; then
    echo "warning: skipping msr-prxy: $WORKLOADS_DIR/msr-prxy.xz not found; the MSR Cambridge traces are not redistributed, regenerate it with experiments/datasets/prep/make_workloads.sh msr-prxy (see experiments/datasets/prep/README.md, \"MSR prxy\")" >&2
    continue
  fi
  if [[ ! -f "$input" ]]; then
    echo "error: workload $WORKLOADS_DIR/$dataset.xz not found" >&2
    exit 1
  fi
  echo "=== $dataset"
  run "${PY[@]}" "$HERE/temporal_histogram.py" "$input" \
    --max-bins "$MAX_BINS" ${limit_args[@]+"${limit_args[@]}"} \
    -o "$RESULTS_DIR/$dataset.txt"
done

if is_true "$PLOT"; then
  echo "=== plotting"
  plot_args=(--results-dir "$RESULTS_DIR")
  [[ -z "$OUT_DIR" ]] || plot_args+=(--out-dir "$OUT_DIR")
  run "${PY[@]}" "$CLOAK_ROOT/plots/fig2_fig4_temporal_locality.py" "${plot_args[@]}"
fi
