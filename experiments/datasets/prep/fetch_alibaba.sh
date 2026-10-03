#!/usr/bin/env bash
# Capture Alibaba virtual device 38 from the Alibaba block-trace 2020 release.
#
#   ./fetch_alibaba.sh [out_dir] [stop_ts_us]
#
# Streams the release tarball (~194 GB compressed; never stored), keeps only
# device 38's lines, zstd-compresses them to <out_dir>/alibaba-dev38-raw.zst
# and stops once the trace is past stop_ts_us. Default stop_ts_us is the
# first 7 days (trace start 2020-01-01 00:00:00 UTC+8 = 1577808000 s, +7 d)
# plus a 30 s margin; block_pipeline.py trims to the exact 7-day cutoff.
# That is the capture used for the paper (~40 min on a fast datacenter
# link; the tarball is streamed up to the 7-day mark).
#
# Only the first 10M requests of the 1M-page workload are used for the
# benchmark (about 6 hours of trace at the device's average rate). With the
# known hash threshold (README.md) you can stop much earlier, e.g.
#   ./fetch_alibaba.sh data/ali 1577894400000000     # first 24 hours
# and check that `block_pipeline.py emit --max-requests 10000000` reports
# stop_reason "max_requests".
#
# Stop rule (kept from the original capture for byte-identical output): the
# stream is prefiltered to devices 38 and 206 (the pair captured together),
# and the capture ends after 100,000 consecutive prefiltered lines past the
# stop time. Only device 38 is written.
#
# Needs: curl, tar, gzip, grep, awk, zstd. The stream cannot resume, so a
# broken transfer restarts from scratch (up to 3 attempts). "Broken pipe"
# messages from tar/curl after a successful stop are expected.
set -u
URL=http://block-traces.oss-cn-beijing.aliyuncs.com/alibaba_block_traces_2020.tar.gz
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-$HERE/data/ali}"
LIM="${2:-$((1578412800000000 + 30000000))}"
mkdir -p "$OUT"
cd "$OUT"

for attempt in 1 2 3; do
  echo "attempt $attempt: $(date -u +%FT%TZ)"
  rm -f alibaba-dev38-raw.zst
  curl -sS --connect-timeout 30 "$URL" \
    | tar -xzOf - \
    | LC_ALL=C grep -E '^(38|206),' \
    | awk -F, -v lim="$LIM" '
        BEGIN { z38 = "zstd -q -3 -f -o alibaba-dev38-raw.zst -"; past = 0 }
        NF >= 5 && $1 == "38" { print | z38 }
        NF >= 5 {
          ts = $5 + 0
          if (ts > lim) {
            if (++past > 100000) {
              printf "CUTOFF_REACHED nr=%d ts=%.0f\n", NR, ts > "/dev/stderr"
              exit 0
            }
          } else past = 0
        }
        NR % 20000000 == 0 { printf "progress nr=%d ts=%s\n", NR, $5 > "/dev/stderr" }
      ' 2> "fetch_alibaba.$attempt.log"
  if grep -q CUTOFF_REACHED "fetch_alibaba.$attempt.log"; then
    ls -la alibaba-dev38-raw.zst
    exit 0
  fi
  echo "stream ended before the stop time; retrying" >&2
  tail -3 "fetch_alibaba.$attempt.log" >&2
done
exit 1
