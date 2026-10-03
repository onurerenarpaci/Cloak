#!/usr/bin/env python3
"""Block-I/O trace -> Cloak workload (MSR Cambridge prxy, Alibaba device 38).

Each block I/O (offset, length) is split into the fixed-size pages it
touches; every touched page is one Cloak request (read I/O -> `key`, write
I/O -> `key 1`). A page is identified by "<stream>:<page>", where <stream>
is the MSR volume name (e.g. "prxy_0") or the Alibaba device id ("38") and
<page> = byte_offset // page_size. Page sizes: 8192 for MSR prxy, 4096 for
Alibaba device 38 (see README.md).

The window is fixed (MSR: the whole ~1-week trace; Alibaba: the first 7
days) and PAGES are subsampled with the same spatial hash filter as
kv_pipeline.py (SHARDS, Waldspurger et al., FAST '15):

    keep page iff int.from_bytes(blake2b(b"<stream>:<page>", digest_size=8,
                                         key=b"cloak1").digest(), "big")
                  < hash_threshold

Every request to a kept page is emitted, in time order (volumes are merged
by timestamp); requests are never sampled individually and no caps are
applied. Pages are relabeled to dense integers in first-appearance order.

To hit a distinct-page target exactly, `survey` streams the window once
without sampling and records the hash of every distinct page; `emit
--survey ... --target N` then uses hash_threshold = (N-th smallest page
hash) + 1. With a known threshold (README.md / provenance/), `emit
--hash-threshold T` skips the survey, and `--max-requests` stops after the
first N requests (the paper's benchmark workloads are the first 10M
requests of the 1M-page workloads).

Inputs:
  msr  msr-cambridge1.tar and/or msr-cambridge2.tar from SNIA IOTTA
       (http://iotta.snia.org/traces/block-io/388) in --msr-dir; the volumes
       of --server (prxy -> prxy_0, prxy_1) are read straight from the tars
       (both prxy volumes are in msr-cambridge1.tar).
  ali  zstd-compressed device lines of the Alibaba 2020 trace
       (device_id,opcode,offset,length,timestamp_us), as produced by
       fetch_alibaba.sh.

Stdlib only (zstd CLI needed for the Alibaba input).
"""
import argparse
import gzip
import heapq
import json
import os
import subprocess
import sys
import tarfile
import time
from array import array
from hashlib import blake2b

from kv_pipeline import workload_stats

HASH_DENOM = 2 ** 64
SID_SHIFT = 44  # packed page id: stream_index << 44 | page
HERE = os.path.dirname(os.path.abspath(__file__))

# Alibaba trace start = 2020-01-01 00:00:00 UTC+8 = 1577808000 s; +7 days.
ALI_CUTOFF_US = (1577808000 + 7 * 86400) * 1000000


def make_hash(salt):
    saltb = salt.encode()

    def hash64(key):
        h = blake2b(key, digest_size=8, key=saltb).digest()
        return int.from_bytes(h, "big")
    return hash64


# ------------------------------------------------------------- raw iterators
# Records are (ts, stream_index, op, offset, length); op 0 = read, 1 = write.

def msr_volumes(msr_dir, server):
    vols = []
    for tarname in ("msr-cambridge1.tar", "msr-cambridge2.tar"):
        path = os.path.join(msr_dir, tarname)
        if not os.path.exists(path):
            continue
        with tarfile.open(path) as tf:
            for m in tf:
                if not m.name.endswith(".csv.gz"):
                    continue
                vol = os.path.basename(m.name)[: -len(".csv.gz")]
                if vol.rsplit("_", 1)[0] == server:
                    vols.append((vol, path, m.name))
    vols.sort()
    if not vols:
        sys.exit(f"no volumes for server {server!r} in {msr_dir}")
    return vols


def iter_msr_volume(tarpath, member, sid):
    # Timestamp (Windows filetime), Hostname, DiskNumber, Type, Offset,
    # Size, ResponseTime
    tf = tarfile.open(tarpath)
    with gzip.open(tf.extractfile(member), "rb") as gz:
        for line in gz:
            p = line.rstrip(b"\n").split(b",")
            if len(p) < 6:
                continue
            yield (int(p[0]), sid, 0 if p[3] == b"Read" else 1,
                   int(p[4]), int(p[5]))
    tf.close()


def iter_ali_raw(rawpath, device):
    # device_id, opcode (R/W), offset, length, timestamp (us)
    devb = device.encode()
    cmd = ["zstd", "-dc", rawpath] if rawpath.endswith(".zst") else ["cat", rawpath]
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE)
    for line in proc.stdout:
        p = line.rstrip(b"\n").split(b",")
        if len(p) < 5 or p[0] != devb:
            continue
        yield (int(p[4]), 0, 0 if p[1] == b"R" else 1, int(p[2]), int(p[3]))
    proc.stdout.close()
    proc.wait()


def sorted_stream(records, window=65536):
    """Smooth local timestamp disorder with a sliding min-heap."""
    heap = []
    push, pop = heapq.heappush, heapq.heappop
    for rec in records:
        push(heap, rec)
        if len(heap) > window:
            yield pop(heap)
    while heap:
        yield pop(heap)


def open_source(a, for_emit):
    """-> (stream names, record iterator, cutoff_ts, ts_divisor, label)."""
    if a.source == "msr":
        vols = msr_volumes(a.msr_dir, a.server)
        names = [v[0] for v in vols]
        if for_emit:  # merged time order across volumes
            recs = heapq.merge(*[sorted_stream(iter_msr_volume(p, m, sid))
                                 for sid, (_, p, m) in enumerate(vols)])
        else:  # order is irrelevant for the survey
            recs = (r for sid, (_, p, m) in enumerate(vols)
                    for r in iter_msr_volume(p, m, sid))
        return names, recs, None, 1e7, f"msr:{a.server}:" + ",".join(names)
    recs = iter_ali_raw(a.raw, a.device)
    if for_emit:
        recs = sorted_stream(recs)
    return [a.device], recs, a.cutoff_ts, 1e6, f"alibaba:dev{a.device}"


def pages(off, ln, P):
    if ln <= 0:
        ln = 1
    return range(off // P, (off + ln - 1) // P + 1)


# --------------------------------------------------------------------- survey
def survey(a):
    names, records, cutoff_ts, ts_div, label = open_source(a, for_emit=False)
    P = a.page_size
    hash64 = make_hash(a.salt)
    names_b = [n.encode() for n in names]
    seen = set()
    hashes = array("Q")
    ios = requests = wreqs = 0
    first_ts = last_ts = None
    t0 = time.time()
    for ts, sid, op, off, ln in records:
        if cutoff_ts is not None and ts > cutoff_ts:
            continue
        pr = pages(off, ln, P)
        requests += len(pr)
        if op:
            wreqs += len(pr)
        base = sid << SID_SHIFT
        for pp in pr:
            ki = base | pp
            if ki not in seen:
                seen.add(ki)
                hashes.append(hash64(b"%s:%d" % (names_b[sid], pp)))
        first_ts = ts if first_ts is None or ts < first_ts else first_ts
        last_ts = ts if last_ts is None or ts > last_ts else last_ts
        ios += 1
        if ios % 10000000 == 0:
            print(f"  ios={ios} requests={requests} distinct={len(seen)} "
                  f"elapsed={time.time() - t0:.0f}s", file=sys.stderr, flush=True)
    with open(a.out + "-keyhash.bin", "wb") as f:
        hashes.tofile(f)
    meta = {
        "source": label,
        "page_size": P,
        "salt": a.salt,
        "streams": dict(enumerate(names)),
        "ios": ios,
        "requests": requests,
        "distinct_pages": len(seen),
        "write_req_fraction": wreqs / requests if requests else 0,
        "first_ts": first_ts,
        "last_ts": last_ts,
        "span_seconds": (last_ts - first_ts) / ts_div if ios else 0,
        "cutoff_ts": cutoff_ts,
    }
    with open(a.out + "-survey-meta.json", "w") as f:
        json.dump(meta, f, indent=1)
    print(json.dumps(meta, indent=1))


def threshold_from_survey(survey_prefix, target):
    h = array("Q")
    with open(survey_prefix + "-keyhash.bin", "rb") as f:
        h.frombytes(f.read())
    if target >= len(h):
        return HASH_DENOM, len(h)
    return sorted(h)[target - 1] + 1, len(h)


# ----------------------------------------------------------------------- emit
def emit(a):
    survey_distinct = None
    if a.hash_threshold is not None:
        thresh = a.hash_threshold
    else:
        thresh, survey_distinct = threshold_from_survey(a.survey, a.target)
    print(f"hash_threshold={thresh} rate={thresh / HASH_DENOM:.10g}",
          file=sys.stderr, flush=True)

    names, records, cutoff_ts, ts_div, label = open_source(a, for_emit=True)
    P = a.page_size
    hash64 = make_hash(a.salt)
    names_b = [n.encode() for n in names]
    page_hash = {}   # packed page id -> 64-bit hash (memoized)
    keymap = {}      # kept packed page id -> dense key
    limit = a.max_requests
    requests = wreqs = ios = 0
    first_ts = last_ts = None
    stop_reason = "window_exhausted"
    t0 = time.time()
    with open(a.out, "w") as out:
        for ts, sid, op, off, ln in records:
            if cutoff_ts is not None and ts > cutoff_ts:
                continue
            ios += 1
            base = sid << SID_SHIFT
            for pp in pages(off, ln, P):
                ki = base | pp
                h = page_hash.get(ki)
                if h is None:
                    h = page_hash[ki] = hash64(b"%s:%d" % (names_b[sid], pp))
                if h >= thresh:
                    continue
                k = keymap.get(ki)
                if k is None:
                    k = keymap[ki] = len(keymap)
                if op:
                    out.write(f"{k} 1\n")
                    wreqs += 1
                else:
                    out.write(f"{k}\n")
                requests += 1
                if first_ts is None:
                    first_ts = ts
                last_ts = ts
                if limit is not None and requests >= limit:
                    break
            if limit is not None and requests >= limit:
                stop_reason = "max_requests"
                break
            if ios % 10000000 == 0:
                print(f"  ios={ios} requests={requests} distinct={len(keymap)} "
                      f"elapsed={time.time() - t0:.0f}s", file=sys.stderr,
                      flush=True)
    meta = {
        "source": label,
        "sampling": "keep page iff blake2b('<stream>:<page>', key=salt, "
                    "digest_size=8) < hash_threshold",
        "salt": a.salt,
        "page_size": P,
        "hash_threshold": thresh,
        "rate": thresh / HASH_DENOM,
        "target_keys": a.target,
        "survey_distinct_pages": survey_distinct,
        "raw_ios_consumed": ios,
        "requests": requests,
        "distinct_pages": len(keymap),
        "write_fraction": wreqs / requests if requests else 0,
        "first_kept_ts": first_ts,
        "last_kept_ts": last_ts,
        "span_seconds": (last_ts - first_ts) / ts_div if requests else 0,
        "stop_reason": stop_reason,
    }
    with open(a.out + "-meta.json", "w") as f:
        json.dump(meta, f, indent=1)
    print(json.dumps(meta, indent=1))


def stats(a):
    out = workload_stats(a.workload, a.fit_ranks)
    print(json.dumps(out, indent=1))
    if a.out:
        with open(a.out, "w") as f:
            json.dump(out, f, indent=1)


# ----------------------------------------------------------------------- main
def add_source_args(p):
    p.add_argument("source", choices=["msr", "ali"])
    p.add_argument("--page-size", type=int, required=True)
    p.add_argument("--salt", default="cloak1")
    p.add_argument("--server", default="prxy", help="msr: server name")
    p.add_argument("--msr-dir", default=os.path.join(HERE, "data", "msr"),
                   help="msr: directory holding msr-cambridge{1,2}.tar")
    p.add_argument("--raw", help="ali: device capture from fetch_alibaba.sh")
    p.add_argument("--device", default="38", help="ali: device id")
    p.add_argument("--cutoff-ts", type=int, default=ALI_CUTOFF_US,
                   help="ali: drop I/Os after this timestamp (us); default "
                        "= trace start + 7 days")


def main():
    ap = argparse.ArgumentParser(
        description="Block trace -> Cloak workload (page hash sampling).")
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("survey", help="unsampled pass: distinct-page hashes")
    add_source_args(s)
    s.add_argument("--out", required=True,
                   help="prefix: writes -keyhash.bin, -survey-meta.json")
    e = sub.add_parser("emit", help="write the hash-sampled workload")
    add_source_args(e)
    g = e.add_mutually_exclusive_group(required=True)
    g.add_argument("--hash-threshold", type=int,
                   help="keep pages whose hash is below this value")
    g.add_argument("--survey", help="survey prefix (use with --target)")
    e.add_argument("--target", type=int, default=None,
                   help="distinct-page target, with --survey")
    e.add_argument("--max-requests", type=int, default=None,
                   help="stop after this many requests (prefix)")
    e.add_argument("--out", required=True,
                   help="workload file (also writes <out>-meta.json)")
    st = sub.add_parser("stats", help="workload statistics")
    st.add_argument("workload")
    st.add_argument("--fit-ranks", type=int, default=10000)
    st.add_argument("--out", default=None)
    a = ap.parse_args()
    if a.cmd in ("survey", "emit") and a.source == "ali" and not a.raw:
        ap.error("ali needs --raw")
    if a.cmd == "emit" and a.survey and a.target is None:
        ap.error("--survey needs --target")
    {"survey": survey, "emit": emit, "stats": stats}[a.cmd](a)


if __name__ == "__main__":
    main()
