#!/usr/bin/env python3
"""Ethereum transactions (BigQuery public dataset) -> Cloak workload.

Input: CSV export(s) of `ethereum.sql` (columns block_number,
transaction_index, block_timestamp, from_address, to_address; one header row
per file). BigQuery may split an export into several shard files.

Transformation (as used for the paper):
  * every transaction is one read of its recipient, key = to_address
    (contract-creation transactions have an empty to_address; they are kept
    and all map to the same key, as in the original pipeline);
  * recipients that appear exactly once in the window are dropped (an
    element that is never re-accessed carries no temporal locality); all
    other transactions are kept, in order;
  * recipients are relabeled to dense integers in first-appearance order,
    starting at --first-id (default 0).

Request order (--order):
  paper          (default) the exact sequence used in the paper. The paper's
                 workload concatenated the 11 shard files of its BigQuery
                 export in file-name order; BigQuery does not number shards
                 chronologically, so the concatenation is chronological
                 except that two chunks (~20 h and ~25 h) are displaced
                 (see PAPER_SEGMENTS). Rows are sorted by (block_number,
                 transaction_index) and re-cut into those segments, so the
                 paper sequence is reproduced from any export of the same
                 block range, however it is sharded. Rows outside the
                 paper's block range are dropped.
  chronological  strict blockchain execution order (block_number,
                 transaction_index).
  files          rows in the order of the files given on the command line.

The workload used in the paper (17,427,629 requests over 701,313 distinct
recipients) labels keys 1..701,313 in an arbitrary order (Rust HashMap
iteration order); `--order paper --first-id 1` reproduces it up to that
relabeling, i.e. request-for-request with a one-to-one key mapping.

Usage: python3 ethereum.py -o ethereum eth_tx_*.csv
Requires Python >= 3.10; ~3 GB RAM for the paper window (18M transactions).
"""
import argparse
import bisect
import sys
from array import array

# The paper workload's shard sequence. Each entry is the inclusive range
# (block_number, transaction_index) .. (block_number, transaction_index) of
# one export shard, listed in the order the shards were concatenated.
PAPER_SEGMENTS = [
    ((23049662, 0), (23057920, 62)),      # 2025-08-02 00:00:11 .. 08-03 03:42:47 UTC
    ((23057920, 63), (23065999, 171)),    # 2025-08-03 03:42:47 .. 08-04 06:45:59
    ((23071914, 175), (23078115, 133)),   # 2025-08-05 02:35:23 .. 08-05 23:21:47
    ((23085493, 212), (23092507, 145)),   # 2025-08-07 00:05:35 .. 08-07 23:37:23
    ((23065999, 172), (23071914, 174)),   # 2025-08-04 06:45:59 .. 08-05 02:35:23
    ((23092507, 146), (23100560, 83)),    # 2025-08-07 23:37:23 .. 08-09 02:37:23
    ((23100560, 84), (23107414, 224)),    # 2025-08-09 02:37:23 .. 08-10 01:35:23
    ((23107414, 225), (23114961, 253)),   # 2025-08-10 01:35:23 .. 08-11 02:55:11
    ((23114961, 254), (23121859, 19)),    # 2025-08-11 02:55:11 .. 08-12 02:04:11
    ((23121859, 20), (23129291, 227)),    # 2025-08-12 02:04:11 .. 08-13 02:58:11
    ((23078115, 134), (23085493, 211)),   # 2025-08-05 23:21:47 .. 08-07 00:05:35
]
TX_BITS = 20  # sort key = block_number << TX_BITS | transaction_index


def pack(block, tx):
    return (block << TX_BITS) | tx


def read_rows(paths):
    """Return (sort keys, recipient ids) in file order; recipient ids are
    assigned in first-appearance order of the raw rows."""
    keys = array("Q")
    rcpt = array("I")
    addr_ids = {}
    for path in paths:
        with open(path, "rb") as f:
            header = f.readline().rstrip(b"\r\n").split(b",")
            col = {name: i for i, name in enumerate(header)}
            try:
                ib, it, ito = (col[b"block_number"], col[b"transaction_index"],
                               col[b"to_address"])
            except KeyError:
                sys.exit(f"{path}: unexpected header {header}")
            for line in f:
                p = line.rstrip(b"\r\n").split(b",")
                to = p[ito]
                a = addr_ids.get(to)
                if a is None:
                    a = addr_ids[to] = len(addr_ids)
                keys.append(pack(int(p[ib]), int(p[it])))
                rcpt.append(a)
        print(f"  read {path}: {len(keys)} rows so far", file=sys.stderr)
    return keys, rcpt


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("csv", nargs="+", help="BigQuery CSV export file(s)")
    ap.add_argument("-o", "--out", default="ethereum")
    ap.add_argument("--order", choices=["paper", "chronological", "files"],
                    default="paper")
    ap.add_argument("--first-id", type=int, default=0,
                    help="label of the first key (the paper's file uses 1)")
    a = ap.parse_args()

    keys, rcpt = read_rows(a.csv)
    n = len(keys)

    if a.order == "files":
        order = range(n)
    else:
        # Timsort merges the already-sorted shard runs in ~linear time.
        perm = sorted(range(n), key=keys.__getitem__)
        if a.order == "chronological":
            order = perm
        else:
            key = keys.__getitem__
            order = array("I")
            for lo, hi in PAPER_SEGMENTS:
                s = bisect.bisect_left(perm, pack(*lo), key=key)
                e = bisect.bisect_right(perm, pack(*hi), key=key)
                order.extend(perm[s:e])
            del perm
            dropped = n - len(order)
            if dropped:
                print(f"  dropped {dropped} rows outside the paper's block range",
                      file=sys.stderr)

    counts = array("I", bytes(4 * (max(rcpt) + 1 if n else 0)))
    for i in order:
        counts[rcpt[i]] += 1

    labels = {}
    written = 0
    with open(a.out, "w") as o:
        for i in order:
            r = rcpt[i]
            if counts[r] < 2:
                continue
            k = labels.get(r)
            if k is None:
                k = labels[r] = len(labels) + a.first_id
            o.write(f"{k}\n")
            written += 1

    total = len(order)
    one_hit = sum(1 for c in counts if c == 1)
    print(f"transactions   : {total}")
    print(f"recipients     : {sum(1 for c in counts if c > 0)} "
          f"({one_hit} seen once -> dropped)")
    print(f"requests       : {written} (100% reads)")
    print(f"distinct keys  : {len(labels)}")
    print(f"wrote          : {a.out}")


if __name__ == "__main__":
    main()
