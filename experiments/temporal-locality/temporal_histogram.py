#!/usr/bin/env python3
"""Temporal-locality (reuse-interval) histogram of a Cloak workload file.

For every request whose key was requested before, count how many requests
to *other* keys lie between it and the previous request to the same key,
and add one to bin x of the histogram. Bin x therefore holds the number of
consecutive access pairs to the same key separated by exactly x intervening
accesses, aggregated over all keys (x = 0 means the key is requested twice
in a row). This is the quantity plotted in Figures 2 and 4 of the paper.

Input: one request per line, `key` (read) or `key value` (write); only the
first whitespace-separated field is used. Plain text or `.xz`-compressed.

Output: one integer per line, line x (0-based) = count for x intervening
accesses. Bins beyond --max-bins are not stored (their pairs are reported
as "dropped"), and trailing all-zero bins are not written.

Runtime is linear in the number of requests (one dictionary lookup per
request, roughly 0.5 s per million requests on a laptop, including xz
decompression); memory is dominated by the key -> last-position map (about
150 bytes per distinct key).
"""

import argparse
import itertools
import lzma
import sys
import time


def open_text(path):
    if path == "-":
        return sys.stdin
    if path.endswith(".xz"):
        return lzma.open(path, "rt")
    return open(path)


def reuse_histogram(lines, max_bins, limit=None):
    """Return (hist, requests, distinct_keys, dropped) for an iterable of lines."""
    hist = [0] * max_bins
    last_seen = {}
    get = last_seen.get
    dropped = 0
    pos = -1
    if limit is not None:
        lines = itertools.islice(lines, limit)
    for pos, line in enumerate(lines):
        fields = line.split(None, 1)
        if not fields:
            raise ValueError(f"empty line at request {pos + 1}")
        key = fields[0]
        prev = get(key)
        if prev is not None:
            x = pos - prev - 1  # number of intervening accesses
            if x < max_bins:
                hist[x] += 1
            else:
                dropped += 1
        last_seen[key] = pos
    return hist, pos + 1, len(last_seen), dropped


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input", help="workload file (plain or .xz), or - for stdin")
    ap.add_argument("-o", "--output", default="-",
                    help="output histogram file (default: stdout)")
    ap.add_argument("--max-bins", type=int, default=1_000_000,
                    help="number of bins to keep, i.e. largest stored x + 1 "
                         "(default 1000000; the figures use the first 100000)")
    ap.add_argument("--limit", type=int, default=None,
                    help="only read the first LIMIT requests")
    args = ap.parse_args()

    t0 = time.time()
    with open_text(args.input) as f:
        hist, requests, keys, dropped = reuse_histogram(f, args.max_bins, args.limit)

    end = len(hist)
    while end > 0 and hist[end - 1] == 0:
        end -= 1
    out = sys.stdout if args.output == "-" else open(args.output, "w")
    try:
        out.writelines(f"{c}\n" for c in hist[:end])
    finally:
        if out is not sys.stdout:
            out.close()

    pairs = sum(hist)
    print(f"{args.input}: {requests} requests, {keys} distinct keys, "
          f"{pairs + dropped} reuse pairs ({dropped} with x >= {args.max_bins} "
          f"not stored), {end} bins written, {time.time() - t0:.1f}s",
          file=sys.stderr)


if __name__ == "__main__":
    main()
