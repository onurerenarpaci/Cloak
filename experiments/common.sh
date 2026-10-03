# Shared harness for experiments/<name>/run.sh -- source it, do not execute it:
#
#     source "$(dirname "$0")/../common.sh"
#
# Functions
#   cloak_workload <file.xz>    decompress a workload once into
#                               $CLOAK_ROOT/.cache/workloads/ and print the path
#                               of the plain file (with SMOKE=1: of a copy that
#                               holds only the first $SMOKE_REQUESTS requests)
#   cloak_selected <label>      true if <label> matches $ONLY (or ONLY is unset);
#                               lets run.sh skip unneeded decompression
#   cloak_rep <k>               mark the start of repetition k (for logs)
#   cloak_run <label> <config.toml> <workload> [extra binary args...]
#                               run ONE repetition and append one row to
#                               $RESULTS_DIR/<label>-stats.csv and, if the run
#                               produced one, <label>-ratio.csv. A failed run is
#                               logged and the sweep goes on.
#   cloak_skip <reason> <label>...
#                               skip selected labels that cannot run (e.g. a
#                               workload that is not shipped): logs one warning,
#                               cloak_summary lists them; not counted as failed
#   cloak_summary               print a summary; returns 1 if any run failed
#                               or if nothing ran
#
# Environment variables (all optional)
#   REPS=5             repetitions (the outer loop of every run.sh)
#   SMOKE=1            quick check: REPS defaults to 1, workloads are cut to
#                      SMOKE_REQUESTS (default 200000) requests, results go to
#                      results-smoke/ instead of results/
#   ONLY=<regex>       only run labels matching the (extended) regex
#   LOCAL=1            run on this machine (scripts/run-local.sh) instead of
#                      on the cluster via Ansible
#   DRY_RUN=1          print what would run, run nothing
#   RESULTS_DIR=<dir>  default <experiment>/results (results-smoke with SMOKE=1)
#   INVENTORY=<file>   Ansible inventory, default ansible/inventory
#   VERBOSE=1          stream the output of every run (default: per-run log
#                      files in $RESULTS_DIR/logs/, one status line per run)
#   CLOAK_RUN_TIMEOUT  max client run time per repetition in seconds
#                      (default: derived from workload length and rate)
#   CLOAK_SERVER_PORT, CLOAK_PROXY_PORT   ports (default 4000, 5050)
#   ANSIBLE_ARGS       extra ansible-playbook arguments, e.g. "-v"
#
# Works with bash >= 3.2 (macOS and Linux).

CLOAK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXPERIMENT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXPERIMENT="$(basename "$EXPERIMENT_DIR")"
CLOAK_WORKLOAD_CACHE="$CLOAK_ROOT/.cache/workloads"

_cloak_true() { case "${1:-}" in "" | 0 | false | no | off) return 1 ;; *) return 0 ;; esac; }
_cloak_log() { echo "[cloak] $*" >&2; }
_cloak_die() { echo "[cloak] ERROR: $*" >&2; exit 1; }
_cloak_abspath() { (cd "$(dirname "$1")" && printf '%s/%s\n' "$(pwd)" "$(basename "$1")"); }

SMOKE="${SMOKE:-0}"
SMOKE_REQUESTS="${SMOKE_REQUESTS:-200000}"
LOCAL="${LOCAL:-0}"
DRY_RUN="${DRY_RUN:-0}"
VERBOSE="${VERBOSE:-0}"
ONLY="${ONLY:-}"
INVENTORY="${INVENTORY:-$CLOAK_ROOT/ansible/inventory}"
if _cloak_true "$SMOKE"; then
    REPS="${REPS:-1}"
    RESULTS_DIR="${RESULTS_DIR:-$EXPERIMENT_DIR/results-smoke}"
else
    REPS="${REPS:-5}"
    RESULTS_DIR="${RESULTS_DIR:-$EXPERIMENT_DIR/results}"
fi
case "$RESULTS_DIR" in /*) ;; *) RESULTS_DIR="$(pwd)/$RESULTS_DIR" ;; esac
case "$INVENTORY" in /*) ;; *) INVENTORY="$(pwd)/$INVENTORY" ;; esac

_CLOAK_OK=0
_CLOAK_FAILED=0
_CLOAK_PLANNED=0
_CLOAK_SKIPPED=""
CLOAK_REP="${CLOAK_REP:-1}"

# ---------------------------------------------------------------------------
# sanity checks (fail loudly before anything runs)
# ---------------------------------------------------------------------------
case "$REPS" in '' | *[!0-9]* | 0) _cloak_die "REPS must be a positive integer (got '$REPS')" ;; esac
case "$SMOKE_REQUESTS" in '' | *[!0-9]* | 0) _cloak_die "SMOKE_REQUESTS must be a positive integer" ;; esac
for _v in CLOAK_RUN_TIMEOUT CLOAK_SERVER_PORT CLOAK_PROXY_PORT; do
    case "${!_v:-1}" in '' | *[!0-9]*) _cloak_die "$_v must be a positive integer" ;; esac
done
if [ -n "$ONLY" ]; then
    _rc=0
    [[ "" =~ $ONLY ]] || _rc=$?
    [ "$_rc" -ne 2 ] || _cloak_die "ONLY is not a valid extended regex: '$ONLY'"
fi
command -v xz >/dev/null 2>&1 || _cloak_die "xz is required to decompress the workloads"

if _cloak_true "$LOCAL"; then
    _CLOAK_MODE=local
    [ -z "${CLOAK_RUN_TIMEOUT:-}" ] || export CLOAK_CLIENT_TIMEOUT="$CLOAK_RUN_TIMEOUT"
    if ! _cloak_true "$DRY_RUN"; then
        [ -x "$CLOAK_ROOT/scripts/run-local.sh" ] || _cloak_die "$CLOAK_ROOT/scripts/run-local.sh not found"
        for _b in server proxy client budget_ratio; do
            [ -x "$CLOAK_ROOT/target/release/$_b" ] ||
                _cloak_die "target/release/$_b missing; run 'cargo build --release' in $CLOAK_ROOT first"
        done
    fi
else
    _CLOAK_MODE=ansible
    export ANSIBLE_CONFIG="${ANSIBLE_CONFIG:-$CLOAK_ROOT/ansible/ansible.cfg}"
    if ! _cloak_true "$DRY_RUN"; then
        command -v ansible-playbook >/dev/null 2>&1 ||
            _cloak_die "ansible-playbook not found (install Ansible, or use LOCAL=1 to run on this machine)"
        [ -f "$INVENTORY" ] ||
            _cloak_die "inventory '$INVENTORY' not found: copy ansible/inventory.example to ansible/inventory and fill in your machines, or set INVENTORY=... (LOCAL=1 runs on this machine instead)"
    elif [ ! -f "$INVENTORY" ]; then
        _cloak_log "note: inventory '$INVENTORY' does not exist (fine for DRY_RUN)"
    fi
fi

# One sweep at a time per cluster (or per local port pair): two concurrent
# sweeps would kill each other's processes.
_CLOAK_OUTDIR=""   # temporary result directory of the run in progress
_CLOAK_CURRENT=()  # (stamp label rep start args) of the run in progress
_cloak_unlock() {
    [ -z "${_CLOAK_OUTDIR:-}" ] || rm -rf "$_CLOAK_OUTDIR"
    [ -z "${_CLOAK_LOCK:-}" ] || rm -rf "$_CLOAK_LOCK"
}
_cloak_interrupted() {
    if [ -n "${_CLOAK_CURRENT:-}" ]; then
        printf '%s\t%s\trep=%s\tINTERRUPTED\t%ss\tmode=%s\targs=%s\n' "${_CLOAK_CURRENT[0]}" \
            "${_CLOAK_CURRENT[1]}" "${_CLOAK_CURRENT[2]}" "$(($(date +%s) - _CLOAK_CURRENT[3]))" \
            "$_CLOAK_MODE" "${_CLOAK_CURRENT[4]}" >> "$RESULTS_DIR/runs.log"
    fi
    if [ "$_CLOAK_MODE" = local ]; then
        _cloak_log "interrupted"
    else
        _cloak_log "interrupted. Leftover Cloak processes are stopped at the start of the next run (or now: cd \"$CLOAK_ROOT/ansible\" && ansible-playbook -i \"$INVENTORY\" stop.yml)"
    fi
    exit 130
}
if ! _cloak_true "$DRY_RUN"; then
    if [ "$_CLOAK_MODE" = local ]; then
        _key="local:${CLOAK_SERVER_PORT:-4000}:${CLOAK_PROXY_PORT:-5050}"
    else
        _key="ansible:$INVENTORY"
    fi
    _key="$(printf '%s' "$_key" | cksum | awk '{print $1}')"
    mkdir -p "$CLOAK_ROOT/.cache/locks"
    _CLOAK_LOCK="$CLOAK_ROOT/.cache/locks/$_key"
    if ! mkdir "$_CLOAK_LOCK" 2>/dev/null; then
        _pid="$(cat "$_CLOAK_LOCK/pid" 2>/dev/null || true)"
        if [ -n "$_pid" ] && kill -0 "$_pid" 2>/dev/null; then
            _CLOAK_LOCK=""
            _cloak_die "another sweep (pid $_pid) is already using this cluster; wait for it to finish"
        fi
        rm -rf "$_CLOAK_LOCK"
        mkdir "$_CLOAK_LOCK" || _cloak_die "cannot create lock $_CLOAK_LOCK"
    fi
    echo $$ > "$_CLOAK_LOCK/pid"
    trap _cloak_unlock EXIT
    trap _cloak_interrupted INT TERM
fi

_cloak_log "experiment $EXPERIMENT: mode=$_CLOAK_MODE reps=$REPS$(_cloak_true "$SMOKE" && echo " smoke($SMOKE_REQUESTS requests)")${ONLY:+ only=/$ONLY/}$(_cloak_true "$DRY_RUN" && echo " DRY RUN")"
_cloak_log "results -> $RESULTS_DIR"

# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
_cloak_json_str() {
    local s="$1"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    printf '"%s"' "$s"
}

# print a command so that it can be copy-pasted into a shell
_cloak_print_cmd() {
    local a out="" sq="'" esc="'\\''"
    for a in "$@"; do
        case "$a" in
            '' | *[!A-Za-z0-9_./:=@,+%-]*) out="$out '${a//$sq/$esc}'" ;;
            *) out="$out $a" ;;
        esac
    done
    echo "  ${out# }"
}

# _cloak_append <src.csv> <dest.csv>: append the data rows of src to dest,
# writing the header first if dest is new. Fails on a header mismatch.
_cloak_append() {
    local src="$1" dst="$2" header
    header="$(head -n 1 "$src")"
    if [ -s "$dst" ]; then
        if [ "$(head -n 1 "$dst")" != "$header" ]; then
            _cloak_log "header of $src does not match $dst; not appending"
            return 1
        fi
        # never glue a row onto a last line without newline
        [ -z "$(tail -c 1 "$dst")" ] || printf '\n' >> "$dst"
    else
        printf '%s\n' "$header" > "$dst"
    fi
    tail -n +2 "$src" | awk 'NF' >> "$dst"
}

# ---------------------------------------------------------------------------
# public functions
# ---------------------------------------------------------------------------
cloak_workload() {
    local xzfile="$1" rel full smoke
    if [ ! -f "$xzfile" ]; then
        _cloak_log "ERROR: workload $xzfile not found"
        return 1
    fi
    xzfile="$(_cloak_abspath "$xzfile")"
    case "$xzfile" in
        "$CLOAK_ROOT"/*) rel="${xzfile#"$CLOAK_ROOT"/}" ;;
        *) rel="external/$(basename "$xzfile")" ;;
    esac
    full="$CLOAK_WORKLOAD_CACHE/${rel%.xz}"
    if ! _cloak_true "$DRY_RUN" && { [ ! -s "$full" ] || [ "$xzfile" -nt "$full" ]; }; then
        mkdir -p "$(dirname "$full")"
        _cloak_log "decompressing ${rel}"
        if ! xz -dc "$xzfile" > "$full.tmp.$$"; then
            rm -f "$full.tmp.$$"
            _cloak_log "ERROR: cannot decompress $xzfile"
            return 1
        fi
        mv "$full.tmp.$$" "$full"
    fi
    if _cloak_true "$SMOKE"; then
        smoke="$full.first$SMOKE_REQUESTS"
        if ! _cloak_true "$DRY_RUN" && { [ ! -s "$smoke" ] || [ "$full" -nt "$smoke" ]; }; then
            head -n "$SMOKE_REQUESTS" "$full" > "$smoke.tmp.$$" && mv "$smoke.tmp.$$" "$smoke"
        fi
        echo "$smoke"
    else
        echo "$full"
    fi
}

cloak_selected() {
    [ -z "$ONLY" ] || [[ "$1" =~ $ONLY ]]
}

cloak_rep() {
    CLOAK_REP="$1"
    _cloak_log "=== repetition $CLOAK_REP/$REPS ==="
}

cloak_run() {
    if [ $# -lt 3 ]; then
        _cloak_die "usage: cloak_run <label> <config.toml> <workload> [extra binary args...]"
    fi
    local label="$1" config="$2" workload="$3"
    shift 3
    cloak_selected "$label" || return 0
    local a
    for a in "$@"; do
        case "${a%%=*}" in
            --config | --client-input-file | --common-server-addr | --common-proxy-addr | \
                --common-server-addr-bind | --common-proxy-addr-bind)
                _cloak_die "cloak_run $label: '$a' is set by the harness and must not be passed" ;;
        esac
    done
    [ -f "$config" ] || _cloak_die "config $config not found"
    config="$(_cloak_abspath "$config")"
    _CLOAK_PLANNED=$((_CLOAK_PLANNED + 1))

    local tag="[rep $CLOAK_REP/$REPS] $label"
    local outdir vars cmd
    if _cloak_true "$DRY_RUN"; then
        outdir="<tmp-dir>"
    else
        [ -f "$workload" ] || _cloak_die "workload $workload not found"
        workload="$(_cloak_abspath "$workload")"
        mkdir -p "$RESULTS_DIR/logs"
        local tmp="${TMPDIR:-/tmp}"
        outdir="$(mktemp -d "${tmp%/}/cloak-out.XXXXXX")"
        _CLOAK_OUTDIR="$outdir"
    fi

    if [ "$_CLOAK_MODE" = local ]; then
        cmd=("$CLOAK_ROOT/scripts/run-local.sh" "$config" "$workload" "$outdir" "$@")
    else
        vars="{\"config_file\": $(_cloak_json_str "$config"), \"workload_file\": $(_cloak_json_str "$workload"), \"out_dir\": $(_cloak_json_str "$outdir"), \"extra_args\": ["
        local first=1
        for a in "$@"; do
            [ $first = 1 ] || vars="$vars, "
            vars="$vars$(_cloak_json_str "$a")"
            first=0
        done
        vars="$vars]"
        [ -z "${CLOAK_RUN_TIMEOUT:-}" ] || vars="$vars, \"run_timeout\": $CLOAK_RUN_TIMEOUT"
        [ -z "${CLOAK_SERVER_PORT:-}" ] || vars="$vars, \"server_port\": $CLOAK_SERVER_PORT"
        [ -z "${CLOAK_PROXY_PORT:-}" ] || vars="$vars, \"proxy_port\": $CLOAK_PROXY_PORT"
        vars="$vars}"
        # shellcheck disable=SC2206  # ANSIBLE_ARGS is meant to be word-split
        cmd=(ansible-playbook -i "$INVENTORY" "$CLOAK_ROOT/ansible/run.yml" -e "$vars" ${ANSIBLE_ARGS:-})
    fi

    if _cloak_true "$DRY_RUN"; then
        echo "$tag"
        _cloak_print_cmd "${cmd[@]}"
        return 0
    fi

    local stamp log start rc dur status f
    stamp="$(date -u +%Y%m%dT%H%M%SZ)"
    log="$RESULTS_DIR/logs/${label}-rep${CLOAK_REP}-${stamp}.log"
    {
        echo "# experiment: $EXPERIMENT   label: $label   repetition: $CLOAK_REP/$REPS"
        echo "# config:     $config"
        echo "# workload:   $workload ($(wc -l < "$workload" | tr -d ' ') requests)"
        echo "# extra args: $*"
        echo "# command:"
        _cloak_print_cmd "${cmd[@]}"
        echo
    } > "$log"

    _cloak_log "$tag: running ($_CLOAK_MODE) ..."
    start="$(date +%s)"
    _CLOAK_CURRENT=("$stamp" "$label" "$CLOAK_REP" "$start" "$*")
    if _cloak_true "$VERBOSE"; then
        {
            if "${cmd[@]}" < /dev/null; then
                echo 0 > "$outdir/.rc"
            else
                echo $? > "$outdir/.rc"
            fi
        } 2>&1 | tee -a "$log"
        rc="$(cat "$outdir/.rc" 2>/dev/null || echo 1)"
    else
        if "${cmd[@]}" < /dev/null >> "$log" 2>&1; then
            rc=0
        else
            rc=$?
        fi
    fi
    dur=$(($(date +%s) - start))
    _CLOAK_CURRENT=()

    # keep the binaries' logs (fetched by run.yml) with the run log
    for f in server proxy client; do
        if [ -s "$outdir/$f.log" ]; then
            { echo; echo "----- $f.log -----"; cat "$outdir/$f.log"; } >> "$log"
        fi
    done

    if [ "$rc" -ne 0 ]; then
        status="FAILED (exit $rc)"
    elif [ ! -s "$outdir/stats.csv" ]; then
        status="FAILED (no stats.csv)"
    elif ! _cloak_append "$outdir/stats.csv" "$RESULTS_DIR/$label-stats.csv"; then
        status="FAILED (stats header mismatch)"
    elif [ -s "$outdir/ratio.csv" ] && ! _cloak_append "$outdir/ratio.csv" "$RESULTS_DIR/$label-ratio.csv"; then
        status="FAILED (ratio header mismatch)"
    else
        status="ok"
    fi
    # keep the raw result files of a failed run next to its log
    if [ "$status" != ok ]; then
        for f in stats ratio; do
            [ ! -s "$outdir/$f.csv" ] || cp "$outdir/$f.csv" "${log%.log}-$f.csv"
        done
    fi
    rm -rf "$outdir"
    _CLOAK_OUTDIR=""

    printf '%s\t%s\trep=%s\t%s\t%ss\tmode=%s\targs=%s\n' "$stamp" "$label" "$CLOAK_REP" \
        "$status" "$dur" "$_CLOAK_MODE" "$*" >> "$RESULTS_DIR/runs.log"
    if [ "$status" = ok ]; then
        _CLOAK_OK=$((_CLOAK_OK + 1))
        _cloak_log "$tag: ok (${dur}s)"
    else
        _CLOAK_FAILED=$((_CLOAK_FAILED + 1))
        _cloak_log "$tag: $status after ${dur}s -- log: $log"
        _cloak_true "$VERBOSE" || tail -n 25 "$log" | sed 's/^/    /' >&2
    fi
    return 0
}

cloak_skip() {
    local reason="$1"
    shift
    [ $# -gt 0 ] || return 0
    _CLOAK_SKIPPED="${_CLOAK_SKIPPED:+$_CLOAK_SKIPPED }$*"
    _cloak_log "WARNING: skipping $*: $reason"
}

cloak_summary() {
    local skipped=""
    [ -z "$_CLOAK_SKIPPED" ] || skipped=", skipped: $_CLOAK_SKIPPED"
    if _cloak_true "$DRY_RUN"; then
        _cloak_log "dry run: $_CLOAK_PLANNED run(s) would be executed$skipped"
        return 0
    fi
    if [ "$_CLOAK_PLANNED" -eq 0 ]; then
        if [ -n "$_CLOAK_SKIPPED" ]; then
            _cloak_log "nothing ran: every selected label was skipped ($_CLOAK_SKIPPED)"
        else
            _cloak_log "nothing ran${ONLY:+ (ONLY=/$ONLY/ matched no label)}"
        fi
        return 1
    fi
    _cloak_log "done: $_CLOAK_OK run(s) ok, $_CLOAK_FAILED failed$skipped -- results in $RESULTS_DIR (log: runs.log)"
    [ "$_CLOAK_FAILED" -eq 0 ]
}
