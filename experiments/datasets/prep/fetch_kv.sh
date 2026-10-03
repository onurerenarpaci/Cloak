#!/usr/bin/env bash
# Stream one KV/CDN source through the hash-sampling collector.
#
#   ./fetch_kv.sh <twitter52|metakv|wikit> [out_dir]
#
# The raw trace is never stored: curl | zstd -dc | kv_pipeline.py collect.
# Twitter 52 and Meta KV stop after 30M kept requests -- this cap defines
# their sample window, so keep it -- and then close the pipe, so curl/zstd
# exit with a broken-pipe error; that is expected. WikiT is read to the end
# of the trace (52.6M kept requests, below its 60M cap). Success =
# <out_dir>/<name>-sample-meta.json exists.
#
# Needs: curl, zstd, python3 (stdlib only). Network-bound: Twitter 52 reads
# ~617M trace lines before reaching 30M kept requests.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${2:-$HERE/data/kv}"
mkdir -p "$OUT"

S3=https://cache-datasets.s3.amazonaws.com/cache_dataset_txt
PDL=https://ftp.pdl.cmu.edu/pub/datasets/twemcacheWorkload/open_source

collect() { # <parser> <rate> <max-keep> <name>
  python3 "$HERE/kv_pipeline.py" collect --dataset "$1" --rate "$2" \
    --max-keep "$3" --out "$OUT/$4"
  [ -s "$OUT/$4-sample-meta.json" ] || { echo "FAILED: no meta for $4" >&2; exit 1; }
}

case "${1:-}" in
  twitter52)
    curl -s "$PDL/cluster52.sort.zst" | zstd -dc 2>/dev/null \
      | collect twitter 0.05 30000000 twitter52 ;;
  metakv)
    # The 5 files are one 5-day capture, streamed in order; the 30M cap is
    # reached after 152,409,269 lines. Only the first header line is skipped.
    ( for i in 1 2 3 4 5; do
        curl -s "$S3/2022_metaKV/kvcache_202401/kvcache_traces_$i.csv.zst" \
          | zstd -dc 2>/dev/null || exit 0
      done ) | collect metakv 0.2 30000000 metakv ;;
  wikit)
    curl -s "$S3/2019_wiki/wiki/2019/wiki.txt.2019.zst" \
      | zstd -dc 2>/dev/null | collect wiki-text 0.25 60000000 wikit ;;
  *) echo "usage: $0 <twitter52|metakv|wikit> [out_dir]" >&2; exit 1 ;;
esac
echo "DONE $1 -> $OUT/$1-sample"
