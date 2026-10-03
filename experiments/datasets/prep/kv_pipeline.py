#!/usr/bin/env python3
"""Key-value / CDN trace -> Cloak workload (Twitter 52, Meta KV, WikiT).

These sources are keyed by object ID, so there is no pagination: one trace
request is one Cloak request, and the element size is a benchmark parameter
(1 KB in the paper), not a trace property.

Subsampling is spatial (hash-based) object sampling, as in SHARDS
(Waldspurger et al., FAST '15). A request is kept iff

    int.from_bytes(blake2b(key, digest_size=8, key=b"cloak1").digest(), "big")
        < rate * 2**64

i.e. BLAKE2b-64 keyed with the salt "cloak1", read as a big-endian integer.
Each distinct object is therefore kept with probability `rate` regardless of
its popularity, the decision is stateless and streaming, and thresholds nest:
re-filtering at a smaller rate yields a strict subset. Requests are never
sampled individually; the kept requests stay in trace order and the workload
is a contiguous prefix of the filtered stream.

Subcommands:
  collect   decompressed trace on stdin -> `<op> <key>` sample at a generous
            rate (0 = read, 1 = write); stops at --max-keep kept requests or
            --max-keys distinct objects. Stopping early breaks the upstream
            `curl | zstd` pipe; that is expected.
  regen     sample -> Cloak workload at a tighter (nested) rate: dense keys in
            first-appearance order, `key` = read, `key 1` = write, cut at
            --cap requests / --key-cap distinct keys. --target-keys
            binary-searches the largest rate that stays at or below a
            distinct-key target (how the paper's rates were found).
  stats     workload -> request count, distinct keys, write fraction and
            popularity statistics (Zipf exponent fit, one-hit fraction).

Stdlib only (zstd CLI needed for .zst inputs).
"""
import argparse
import hashlib
import json
import math
import subprocess
import sys

HASH_DENOM = float(2 ** 64)


def zopen(path):
    """Open a sample for binary reading, decompressing .zst on the fly."""
    if path.endswith(".zst"):
        return subprocess.Popen(["zstd", "-dc", path],
                                stdout=subprocess.PIPE).stdout
    return open(path, "rb")


def make_hash(salt):
    saltb = salt.encode()

    def hash64(key):
        h = hashlib.blake2b(key, digest_size=8, key=saltb).digest()
        return int.from_bytes(h, "big")
    return hash64


def make_keep(rate, salt):
    if rate >= 1.0:
        return lambda key: True
    hash64 = make_hash(salt)
    thresh = rate * HASH_DENOM
    return lambda key: hash64(key) < thresh


# ------------------------------------------------------------------- parsers
# Each parser takes a raw line (bytes, newline stripped) and returns
# (op, key, ts) with op 0 = read / 1 = write, or None to drop the line.

def parse_twitter(line, skipped):
    # twitter/cache-trace: ts,key,key_size,value_size,client_id,op,ttl
    # (the anonymized key may itself contain commas)
    p = line.split(b",")
    if len(p) < 7:
        skipped[b"malformed"] += 1
        return None
    op = p[-2]
    key = p[1] if len(p) == 7 else b",".join(p[1:len(p) - 5])
    if op in (b"get", b"gets"):
        return 0, key, p[0]
    if op in (b"set", b"add", b"replace", b"cas", b"append", b"prepend",
              b"incr", b"decr"):
        return 1, key, p[0]
    skipped[op] += 1  # delete
    return None


def parse_metakv(line, skipped):
    # op_time,key,key_size,op,op_count,size,cache_hits,ttl,usecase,sub_usecase
    p = line.split(b",")
    if len(p) < 6:
        skipped[b"malformed"] += 1
        return None
    op = p[3]
    if op in (b"GET", b"GET_LEASE"):
        return 0, p[1], p[0]
    if op in (b"SET", b"SET_LEASE"):
        return 1, p[1], p[0]
    skipped[op] += 1  # DELETE
    return None


def parse_wiki_text(line, skipped):
    # relative_unix \t hashed_host_path_query \t size \t ttfb  (all reads)
    p = line.split(b"\t")
    if len(p) < 4:
        skipped[b"malformed"] += 1
        return None
    return 0, p[1], p[0]


PARSERS = {
    "twitter": parse_twitter,
    "metakv": parse_metakv,
    "wiki-text": parse_wiki_text,
}
HEADER_DATASETS = {"metakv"}  # first line of the stream is a CSV header


# ------------------------------------------------------------------- collect
def collect(a):
    from collections import defaultdict
    parser = PARSERS[a.dataset]
    hash64 = make_hash(a.salt)
    thresh = a.rate * HASH_DENOM
    skipped = defaultdict(int)
    raw = kept = writes = 0
    distinct = set()  # 64-bit hashes, not raw keys, to bound memory
    first_ts = last_ts = None
    header = None
    stop_reason = "input_exhausted"
    with open(a.out + "-sample", "wb") as out:
        for line in sys.stdin.buffer:
            line = line.rstrip(b"\n")
            if raw == 0 and a.dataset in HEADER_DATASETS:
                header = line.decode(errors="replace")
                raw += 1
                continue
            raw += 1
            rec = parser(line, skipped)
            if rec is None:
                continue
            op, key, ts = rec
            if first_ts is None:
                first_ts = ts
            last_ts = ts
            h = hash64(key)
            if h >= thresh:
                continue
            distinct.add(h)
            if len(distinct) > a.max_keys:
                stop_reason = "max_keys"
                break
            out.write((b"1 " if op else b"0 ") + key + b"\n")
            kept += 1
            writes += op
            if kept >= a.max_keep:
                stop_reason = "max_keep"
                break
    meta = {
        "dataset": a.dataset,
        "rate": a.rate,
        "salt": a.salt,
        "raw_lines": raw,
        "kept": kept,
        "kept_distinct": len(distinct),
        "kept_write_fraction": writes / kept if kept else 0,
        "skipped_ops": {k.decode(errors="replace"): v for k, v in skipped.items()},
        "first_ts": (first_ts or b"").decode(errors="replace"),
        "last_ts": (last_ts or b"").decode(errors="replace"),
        "header": header,
        "stop_reason": stop_reason,
    }
    with open(a.out + "-sample-meta.json", "w") as m:
        json.dump(meta, m, indent=1)
    print(json.dumps(meta, indent=1))


# --------------------------------------------------------------------- regen
def _simulate(sample_path, keep, cap, key_cap):
    """(requests, distinct, stop_reason) for one candidate rate."""
    seen = set()
    n = 0
    with zopen(sample_path) as f:
        for line in f:
            key = line[2:-1]
            if not keep(key):
                continue
            if key not in seen:
                if len(seen) >= key_cap:
                    return n, len(seen), "key_cap"
                seen.add(key)
            n += 1
            if n >= cap:
                return n, len(seen), "request_cap"
    return n, len(seen), "sample_exhausted"


def _emit(sample_path, keep, cap, key_cap, out_path):
    keymap = {}
    n = writes = 0
    stop_reason = "sample_exhausted"
    with zopen(sample_path) as f, open(out_path, "w") as o:
        for line in f:
            op = line[0:1] == b"1"
            key = line[2:-1]
            if not keep(key):
                continue
            k = keymap.get(key)
            if k is None:
                if len(keymap) >= key_cap:
                    stop_reason = "key_cap"
                    break
                k = keymap[key] = len(keymap)
            if op:
                o.write(f"{k} 1\n")
                writes += 1
            else:
                o.write(f"{k}\n")
            n += 1
            if n >= cap:
                stop_reason = "request_cap"
                break
    return n, len(keymap), writes, stop_reason


def regen(a):
    base = a.sample[:-len(".zst")] if a.sample.endswith(".zst") else a.sample
    src_meta = {}
    if base.endswith("-sample"):
        try:
            with open(base + "-meta.json") as f:
                src_meta = json.load(f)
        except FileNotFoundError:
            pass
    r0 = src_meta.get("rate", 1.0)
    salt = src_meta.get("salt", a.salt)
    rate = min(a.rate if a.rate is not None else r0, r0)

    trace = []
    if a.target_keys:
        # distinct-at-cap grows with the rate, so binary search the largest
        # rate whose distinct count stays <= target.
        lo, hi = 0.0, rate
        best = rate
        for _ in range(a.search_iters):
            mid = (lo + hi) / 2
            n, d, why = _simulate(a.sample, make_keep(mid, salt), a.cap,
                                  a.key_cap)
            trace.append({"rate": mid, "requests": n, "distinct": d, "stop": why})
            print(f"  search rate={mid!r}: requests={n} distinct={d} ({why})",
                  flush=True)
            if d > a.target_keys:
                hi = mid
            else:
                best = lo = mid
                if d >= a.target_keys * (1 - a.tolerance):
                    break
        rate = best
    n, d, w, why = _emit(a.sample, make_keep(rate, salt), a.cap, a.key_cap,
                         a.out)
    meta = {
        "sample": a.sample,
        "collect_rate": r0,
        "rate": rate,
        "salt": salt,
        "requests": n,
        "distinct_keys": d,
        "write_fraction": w / n if n else 0,
        "reuse": 1 - d / n if n else 0,
        "stop_reason": why,
        "cap": a.cap,
        "key_cap": a.key_cap,
        "target_keys": a.target_keys,
        "search_trace": trace or None,
    }
    with open(a.out + "-meta.json", "w") as m:
        json.dump(meta, m, indent=1)
    print(json.dumps({k: v for k, v in meta.items() if k != "search_trace"},
                     indent=1))


# --------------------------------------------------------------------- stats
def workload_stats(path, fit_ranks=10000):
    """Popularity stats over a Cloak workload (integer keys)."""
    freq = {}
    n = writes = 0
    with open(path) as f:
        for line in f:
            p = line.split()
            k = int(p[0])
            freq[k] = freq.get(k, 0) + 1
            writes += len(p) > 1
            n += 1
    counts = sorted(freq.values(), reverse=True)
    d = len(counts)
    # Zipf exponent: least-squares fit of log(freq) vs log(rank) over the
    # top ranks (tail ranks are dominated by ties)
    top = counts[:min(d, fit_ranks)]
    xs = [math.log(i + 1) for i in range(len(top))]
    ys = [math.log(c) for c in top]
    mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
    denom = sum((x - mx) ** 2 for x in xs)
    alpha = -sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / denom if denom else 0.0
    one_hit = sum(1 for c in counts if c == 1)
    return {
        "requests": n,
        "distinct_keys": d,
        "max_key": max(freq) if freq else None,
        "write_fraction": writes / n if n else 0,
        "mean_freq": n / d if d else 0,
        "zipf_alpha_fit": round(alpha, 4),
        "fit_ranks": len(top),
        "one_hit_wonder_fraction": round(one_hit / d, 4) if d else 0,
        "top10_request_share": round(sum(counts[:10]) / n, 4) if n else 0,
        "top1pct_request_share": round(sum(counts[:max(1, d // 100)]) / n, 4) if n else 0,
        "max_freq": counts[0] if counts else 0,
    }


def stats(a):
    out = workload_stats(a.workload, a.fit_ranks)
    print(json.dumps(out, indent=1))
    if a.out:
        with open(a.out, "w") as f:
            json.dump(out, f, indent=1)


def main():
    ap = argparse.ArgumentParser(
        description="KV/CDN trace -> Cloak workload (hash-sampled).")
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("collect", help="stdin trace -> hash-sampled sample")
    c.add_argument("--dataset", choices=sorted(PARSERS), required=True)
    c.add_argument("--rate", type=float, required=True)
    c.add_argument("--salt", default="cloak1")
    c.add_argument("--out", required=True,
                   help="prefix: writes <out>-sample and <out>-sample-meta.json")
    c.add_argument("--max-keep", type=int, default=30000000)
    c.add_argument("--max-keys", type=int, default=25000000)
    r = sub.add_parser("regen", help="sample -> workload at a nested rate")
    r.add_argument("sample", help="<prefix>-sample (or .zst) from collect")
    r.add_argument("--rate", type=float, default=None,
                   help="tightened rate (default: the collect rate)")
    r.add_argument("--salt", default="cloak1", help="fallback if no sample meta")
    r.add_argument("--cap", type=int, default=10000000)
    r.add_argument("--key-cap", type=int, default=1250000)
    r.add_argument("--target-keys", type=int, default=None,
                   help="binary-search the rate to land this many distinct keys")
    r.add_argument("--tolerance", type=float, default=0.05)
    r.add_argument("--search-iters", type=int, default=10)
    r.add_argument("--out", required=True,
                   help="workload file (also writes <out>-meta.json)")
    s = sub.add_parser("stats", help="workload statistics")
    s.add_argument("workload")
    s.add_argument("--fit-ranks", type=int, default=10000)
    s.add_argument("--out", default=None)
    a = ap.parse_args()
    {"collect": collect, "regen": regen, "stats": stats}[a.cmd](a)


if __name__ == "__main__":
    main()
