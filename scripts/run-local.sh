#!/usr/bin/env bash
# Run one Cloak repetition (storage server + proxy + client) on localhost.
#
# Usage: scripts/run-local.sh <config.toml> <workload> <out_dir> [extra binary args...]
#
# The three binaries are started from a fresh temporary run directory (so the bare
# relative file names in the config land there), on 127.0.0.1 ports 4000 (server) and
# 5050 (proxy). Extra args (config overrides such as `--proxy-cache-size 100` or
# `--proxy-is-unsafe true`) are passed to server, proxy and client alike.
# When the client has finished and the proxy has exited, the proxy's budget log (if
# any) is summarized with budget_ratio, and stats.csv (+ ratio.csv if produced) are
# copied into <out_dir>. The server is then stopped. Nothing is left running, also on
# failure or Ctrl-C. Exit status is non-zero if any step fails; in that case nothing is
# copied to <out_dir> and the run directory (with the binaries' logs) is kept.
#
# Requires `cargo build --release` beforehand. Works with bash >= 3.2 (macOS, Linux).
#
# Environment overrides:
#   CLOAK_BIN_DIR             directory with the binaries (default: <repo>/target/release)
#   CLOAK_SERVER_PORT         default 4000
#   CLOAK_PROXY_PORT          default 5050
#   CLOAK_START_TIMEOUT       seconds to wait for server/proxy start-up (default 600;
#                             the proxy initializes the whole storage before listening)
#   CLOAK_CLIENT_TIMEOUT      seconds to wait for the client (default 3600)
#   CLOAK_PROXY_EXIT_TIMEOUT  seconds to wait for the proxy to exit after the client (default 120)
#   CLOAK_KEEP_RUN_DIR=1      keep the temporary run directory also on success

set -euo pipefail

usage() {
    echo "usage: $0 <config.toml> <workload> <out_dir> [extra binary args...]" >&2
    exit 2
}
[ $# -ge 3 ] || usage

CLOAK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${CLOAK_BIN_DIR:-$CLOAK_ROOT/target/release}"
SERVER_PORT="${CLOAK_SERVER_PORT:-4000}"
PROXY_PORT="${CLOAK_PROXY_PORT:-5050}"
START_TIMEOUT="${CLOAK_START_TIMEOUT:-600}"
CLIENT_TIMEOUT="${CLOAK_CLIENT_TIMEOUT:-3600}"
PROXY_EXIT_TIMEOUT="${CLOAK_PROXY_EXIT_TIMEOUT:-120}"

log() { echo "[run-local] $*"; }
die() { echo "[run-local] ERROR: $*" >&2; exit 1; }

abspath() { (cd "$(dirname "$1")" && printf '%s/%s\n' "$(pwd)" "$(basename "$1")"); }

[ -f "$1" ] || die "config not found: $1"
[ -f "$2" ] || die "workload not found: $2"
CONFIG="$(abspath "$1")"
WORKLOAD="$(abspath "$2")"
mkdir -p "$3"
OUT_DIR="$(cd "$3" && pwd)"
shift 3
EXTRA=("$@")

for b in server proxy client budget_ratio; do
    [ -x "$BIN_DIR/$b" ] || die "$BIN_DIR/$b not found; run 'cargo build --release' in $CLOAK_ROOT first"
done

# Local addresses, unless the caller overrides them explicitly.
ADDR_ARGS=()
has_arg() {
    local a
    for a in ${EXTRA[@]+"${EXTRA[@]}"}; do
        [ "$a" = "$1" ] && return 0
    done
    return 1
}
has_arg --common-server-addr      || ADDR_ARGS+=(--common-server-addr "127.0.0.1:$SERVER_PORT")
has_arg --common-server-addr-bind || ADDR_ARGS+=(--common-server-addr-bind "127.0.0.1:$SERVER_PORT")
has_arg --common-proxy-addr       || ADDR_ARGS+=(--common-proxy-addr "127.0.0.1:$PROXY_PORT")
has_arg --common-proxy-addr-bind  || ADDR_ARGS+=(--common-proxy-addr-bind "127.0.0.1:$PROXY_PORT")
ARGS=(--config "$CONFIG" ${ADDR_ARGS[@]+"${ADDR_ARGS[@]}"} ${EXTRA[@]+"${EXTRA[@]}"})

# Checks for a listening socket without connecting to it (a connection would be taken
# for a real peer: the server and the proxy each serve the first connection they accept).
port_in_use() {
    if command -v ss >/dev/null 2>&1; then
        ss -lnt 2>/dev/null | awk '{print $4}' | grep -Eq "[:.]$1\$"
    elif command -v lsof >/dev/null 2>&1; then
        lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1
    elif command -v netstat >/dev/null 2>&1; then
        netstat -an 2>/dev/null | grep -i listen | awk '{print $4}' | grep -Eq "[:.]$1\$"
    else
        return 1
    fi
}

SERVER_PID=""
PROXY_PID=""
CLIENT_PID=""
RUN_DIR=""
SUCCESS=0

stop_pid() {
    # stop_pid <pid>: SIGTERM, then SIGKILL after 5 s
    local pid="$1" i
    [ -n "$pid" ] || return 0
    kill -0 "$pid" 2>/dev/null || { wait "$pid" 2>/dev/null || true; return 0; }
    kill -TERM "$pid" 2>/dev/null || true
    for i in $(seq 1 50); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
    done
    kill -0 "$pid" 2>/dev/null && kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

cleanup() {
    local rc=$?
    trap - EXIT INT TERM
    stop_pid "$CLIENT_PID"
    stop_pid "$PROXY_PID"
    stop_pid "$SERVER_PID"
    if [ -n "$RUN_DIR" ] && [ -d "$RUN_DIR" ]; then
        if [ "$SUCCESS" = 1 ] && [ "${CLOAK_KEEP_RUN_DIR:-0}" != 1 ]; then
            rm -rf "$RUN_DIR"
        else
            for f in server.log proxy.log client.log; do
                if [ "$SUCCESS" != 1 ] && [ -s "$RUN_DIR/$f" ]; then
                    echo "----- tail of $f -----" >&2
                    tail -n 15 "$RUN_DIR/$f" >&2
                fi
            done
            echo "[run-local] run directory kept: $RUN_DIR" >&2
        fi
    fi
    exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# wait_for_line <pid> <logfile> <pattern> <name>: until the process logs <pattern>
wait_for_line() {
    local pid="$1" file="$2" pattern="$3" name="$4" start=$SECONDS
    while ! grep -q "$pattern" "$file" 2>/dev/null; do
        kill -0 "$pid" 2>/dev/null || die "$name exited during start-up"
        [ $((SECONDS - start)) -lt "$START_TIMEOUT" ] || die "$name not ready after ${START_TIMEOUT}s"
        sleep 0.1
    done
}

# wait_exit <pid> <timeout_s> <name>: waits for the process, fails on timeout or non-zero exit
wait_exit() {
    local pid="$1" timeout="$2" name="$3" start=$SECONDS rc=0
    while kill -0 "$pid" 2>/dev/null; do
        [ $((SECONDS - start)) -lt "$timeout" ] || die "$name still running after ${timeout}s"
        sleep 0.2
    done
    wait "$pid" || rc=$?
    [ "$rc" -eq 0 ] || die "$name exited with status $rc"
}

for p in "$SERVER_PORT" "$PROXY_PORT"; do
    if port_in_use "$p"; then
        die "port $p is already in use (leftover server/proxy?)"
    fi
done

TMP_BASE="${TMPDIR:-/tmp}"
RUN_DIR="$(mktemp -d "${TMP_BASE%/}/cloak-run.XXXXXX")"
cd "$RUN_DIR"
log "run dir: $RUN_DIR"
log "config: $CONFIG"
log "workload: $WORKLOAD ($(wc -l < "$WORKLOAD" | tr -d ' ') requests)"
[ ${#EXTRA[@]} -eq 0 ] || log "extra args: ${EXTRA[*]}"

"$BIN_DIR/server" "${ARGS[@]}" > server.log 2>&1 &
SERVER_PID=$!
wait_for_line "$SERVER_PID" server.log "Starting server on" server
log "server up (pid $SERVER_PID)"

"$BIN_DIR/proxy" "${ARGS[@]}" > proxy.log 2>&1 &
PROXY_PID=$!
wait_for_line "$PROXY_PID" proxy.log "listening for clients on" proxy
log "proxy up (pid $PROXY_PID)"

"$BIN_DIR/client" "${ARGS[@]}" --client-input-file "$WORKLOAD" > client.log 2>&1 &
CLIENT_PID=$!
log "client started (pid $CLIENT_PID)"
wait_exit "$CLIENT_PID" "$CLIENT_TIMEOUT" client
CLIENT_PID=""
log "client finished"

wait_exit "$PROXY_PID" "$PROXY_EXIT_TIMEOUT" proxy
PROXY_PID=""
log "proxy exited"

[ -s stats.csv ] || die "client did not write stats.csv (the config must set stats_file = \"stats.csv\")"

if [ -f budget_log.txt ]; then
    "$BIN_DIR/budget_ratio" budget_log.txt ratio.csv || die "budget_ratio failed"
else
    log "no budget_log.txt (budget log disabled or unsafe mode): no ratio.csv"
fi

stop_pid "$SERVER_PID"
SERVER_PID=""

cp stats.csv "$OUT_DIR/stats.csv"
[ ! -f ratio.csv ] || cp ratio.csv "$OUT_DIR/ratio.csv"
log "results in $OUT_DIR: $(cd "$OUT_DIR" && ls stats.csv ratio.csv 2>/dev/null | tr '\n' ' ')"
SUCCESS=1
