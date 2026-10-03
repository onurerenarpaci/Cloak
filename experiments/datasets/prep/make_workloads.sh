#!/usr/bin/env bash
# Regenerate the Table 2 workloads from their public sources.
#
#   ./make_workloads.sh <dataset>...      dataset: netflix ethereum synthetic
#                                          twitter52 metakv wikit msr-prxy
#                                          alibaba-dev38 | all
#
# Raw inputs live under $DATA (default: prep/data), workloads are written to
# $OUT/<dataset> (default: prep/out) and then checked against the paper's
# workload with check_workload.py. Sources that need a manual download
# (Kaggle, BigQuery, SNIA) must already be in place; see README.md.
# msr-prxy is not shipped: once its SHA-256 matches, it is also installed
# (xz -T0 -6) as ../workloads/msr-prxy.xz, where the harness expects it.
# Python: plain python3 (>= 3.10, stdlib only); set PY="uv run python" to
# use uv instead.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
CLOAK_ROOT="$(cd "$HERE/../../.." && pwd)"
DATA="${DATA:-$HERE/data}"
OUT="${OUT:-$HERE/out}"
WORKLOADS="$(cd "$HERE/../workloads" && pwd)"
PY="${PY:-python3}"
cd "$HERE"

need() { [ -e "$1" ] || { echo "missing $1 -- $2" >&2; exit 1; }; }

make_one() {
  local ds=$1
  echo "=== $ds ==="
  case "$ds" in
    netflix)
      need "$DATA/netflix/vodclickstream_uk_movies_03.csv" \
        "download it from https://www.kaggle.com/datasets/vodclickstream/netflix-audience-behaviour-uk-movies"
      $PY netflix.py "$DATA/netflix/vodclickstream_uk_movies_03.csv" -o "$OUT/netflix" ;;
    ethereum)
      compgen -G "$DATA/ethereum/*.csv" > /dev/null \
        || { echo "missing $DATA/ethereum/*.csv -- export ethereum.sql from BigQuery" >&2; exit 1; }
      $PY ethereum.py -o "$OUT/ethereum" "$DATA"/ethereum/*.csv ;;
    synthetic)
      need "$CLOAK_ROOT/target/release/gen_workload" "run 'cargo build --release' in $CLOAK_ROOT"
      "$CLOAK_ROOT/target/release/gen_workload" --out-dir "$OUT" \
        --num-keys 1000000 --num-requests 1000000 --write-ratio 0.5 \
        --s-start 1.0 --s-end 1.0 --seed "${SEED:-1}"
      mv "$OUT/s1.0" "$OUT/synthetic" ;;
    twitter52|metakv|wikit)
      local rate
      case "$ds" in
        twitter52) rate=0.049951171875000006 ;;
        metakv)    rate=0.053125000000000006 ;;
        wikit)     rate=0.0546875 ;;
      esac
      [ -s "$DATA/kv/$ds-sample-meta.json" ] || ./fetch_kv.sh "$ds" "$DATA/kv"
      $PY kv_pipeline.py regen "$DATA/kv/$ds-sample" --rate "$rate" \
        --cap 10000000 --key-cap 1250000 --out "$OUT/$ds" ;;
    msr-prxy)
      # both prxy volumes are in msr-cambridge1.tar; msr-cambridge2.tar is not needed
      need "$DATA/msr/msr-cambridge1.tar" "download it from http://iotta.snia.org/traces/block-io/388"
      $PY block_pipeline.py emit msr --server prxy --page-size 8192 \
        --msr-dir "$DATA/msr" --hash-threshold 9571094887790794229 \
        --max-requests 10000000 --out "$OUT/msr-prxy" ;;
    alibaba-dev38)
      [ -s "$DATA/ali/alibaba-dev38-raw.zst" ] || ./fetch_alibaba.sh "$DATA/ali"
      $PY block_pipeline.py emit ali --raw "$DATA/ali/alibaba-dev38-raw.zst" \
        --device 38 --page-size 4096 --hash-threshold 6415815174883080656 \
        --max-requests 10000000 --out "$OUT/alibaba-dev38" ;;
    *) echo "unknown dataset: $ds" >&2; exit 1 ;;
  esac
  local same=()
  [ "$ds" = ethereum ] && same=(--same-as "$HERE/../workloads/ethereum.xz")
  if ! $PY check_workload.py "$OUT/$ds" --dataset "$ds" ${same[@]+"${same[@]}"}; then
    echo "WARNING: $ds does not match the paper workload" >&2
  elif [ "$ds" = msr-prxy ]; then
    # not shipped (see README.md): install the verified file for the harness
    xz -T0 -6 -c "$OUT/$ds" > "$OUT/$ds.xz"
    mv "$OUT/$ds.xz" "$WORKLOADS/$ds.xz"
    echo "installed $WORKLOADS/$ds.xz"
  fi
}

[ $# -ge 1 ] || { sed -n '2,15p' "$0"; exit 1; }
mkdir -p "$DATA" "$OUT"
if [ "$1" = all ]; then
  set -- netflix ethereum synthetic twitter52 metakv wikit msr-prxy alibaba-dev38
fi
for ds in "$@"; do make_one "$ds"; done
