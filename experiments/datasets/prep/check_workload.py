#!/usr/bin/env python3
"""Check a (regenerated) Cloak workload against the paper's workload.

  python3 check_workload.py <file> [--dataset NAME] [--same-as <other>]

Prints requests, distinct keys, key range and read/write mix. With
--dataset, also compares against the paper workload's numbers and SHA-256.
With --same-as, checks that both files issue the same request sequence up
to a one-to-one relabeling of keys (same read/write type on every line);
use this for the Ethereum workload, whose paper file labels keys in an
arbitrary order. Files ending in .xz are decompressed on the fly.
"""
import argparse
import hashlib
import lzma
import sys

# dataset -> (requests, distinct keys, writes, sha256 of the decompressed
# paper workload, sha256 of this directory's regeneration if it differs)
PAPER = {
    "netflix": (671736, 8472, 0,
                "348382ca072673a52a17fa0280355faf8e85dab7261ffbc4f2ca97e33b2ad5ff", None),
    "ethereum": (17427629, 701313, 0,
                 "3144c229778e411ab3d302791bf1002cd93a6cf886bc61e01dd47d2040cfd2ff",
                 # ethereum.py --order paper --first-id 0 (first-appearance labels)
                 "74539ed9f15765d7bf0565a58cac5c9fc5afc0e81f3931e87be729b87c491325"),
    "synthetic": (1000000, 173771, 499977,
                  "39f3a9bea6efd591416fb808d908129e30d4679a4b79f0fa58da76b2d32e84e7", None),
    "twitter52": (10000000, 772345, 685120,
                  "75b435c113186a8e46155229e1d5ef6318f36db75715348b47b4e7a214d66b0c", None),
    "metakv": (7965502, 988762, 1691449,
               "4414d88da938b1c47993e67996941b30482580de6f7013a3fe098ea9f07fbeeb", None),
    "wikit": (10000000, 950243, 0,
              "9baf724bf2666be29f692352d1c43c1392d80be20663c64a8571abb698fe26cc", None),
    "msr-prxy": (10000000, 63296, 3420784,
                 "863c7e4eeef6bb31961b9b6c22cba474e615c3480239a273d4ef3697ecfffbfe", None),
    "alibaba-dev38": (10000000, 246600, 10000000,
                      "77abbd5dd92b725795fd7cecc2d9b523eae47a8ee1091c42cba68b6e02f8aca4", None),
}
STOCHASTIC = {"synthetic"}  # random generator: only the shape is reproducible


def open_bin(path):
    return lzma.open(path, "rb") if path.endswith(".xz") else open(path, "rb")


def scan(path):
    sha = hashlib.sha256()
    seen = set()
    n = writes = 0
    lo = hi = None
    with open_bin(path) as f:
        for line in f:
            sha.update(line)
            p = line.split()
            k = int(p[0])
            if k not in seen:
                seen.add(k)
                lo = k if lo is None or k < lo else lo
                hi = k if hi is None or k > hi else hi
            writes += len(p) > 1
            n += 1
    return n, len(seen), writes, lo, hi, sha.hexdigest()


def same_up_to_relabeling(a, b):
    fwd, bwd = {}, {}
    n = 0
    with open_bin(a) as fa, open_bin(b) as fb:
        for la, lb in zip(fa, fb):
            pa, pb = la.split(), lb.split()
            if (len(pa) > 1) != (len(pb) > 1):
                return False, f"read/write type differs at line {n + 1}"
            x, y = pa[0], pb[0]
            if fwd.setdefault(x, y) != y or bwd.setdefault(y, x) != x:
                return False, f"key mapping not one-to-one at line {n + 1}"
            n += 1
        if fa.read(1) or fb.read(1):
            return False, f"different lengths (common prefix {n} lines)"
    return True, f"{n} requests, {len(fwd)} keys, one-to-one key mapping"


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("workload")
    ap.add_argument("--dataset", choices=sorted(PAPER))
    ap.add_argument("--same-as", metavar="OTHER")
    a = ap.parse_args()

    n, d, w, lo, hi, sha = scan(a.workload)
    print(f"requests      : {n}")
    print(f"distinct keys : {d} (key range {lo}..{hi})")
    print(f"R/W           : {100 * (n - w) / n:.1f} / {100 * w / n:.1f}"
          f"  ({w} writes)" if n else "R/W           : -")
    print(f"sha256        : {sha}")

    ok = True
    if a.dataset:
        er, ed, ew, esha, esha_regen = PAPER[a.dataset]
        counts_ok = (n, d, w) == (er, ed, ew)
        verdict = "match" if counts_ok else "DIFFERENT"
        if a.dataset in STOCHASTIC and not counts_ok:
            verdict = "differs (expected, see below)"
        print(f"paper {a.dataset}: requests {er}, distinct {ed}, writes {ew} -> "
              f"{verdict}")
        content_ok = True
        if a.dataset in STOCHASTIC:
            print("  (the paper's synthetic file came from an unseeded RNG: only "
                  "the length, ~50/50 mix and ~174K distinct keys are expected "
                  "to match)")
            counts_ok = n == er
        elif sha == esha:
            print("  byte-identical to the paper workload")
        elif esha_regen and sha == esha_regen:
            print("  identical to this directory's regeneration (paper file "
                  "differs only by key labels; use --same-as to confirm)")
        else:
            print("  content differs from the paper workload")
            content_ok = False
        ok = counts_ok and content_ok

    if a.same_as:
        same, msg = same_up_to_relabeling(a.workload, a.same_as)
        print(f"same as {a.same_as} up to relabeling: "
              f"{'YES' if same else 'NO'} ({msg})")
        ok = ok and same
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
