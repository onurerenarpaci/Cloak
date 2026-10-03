# Dataset preparation for Table 2

This directory documents how each of the eight Table 2 workloads was
derived from its public source, and contains the scripts to regenerate them.
Seven of them ship in [`../workloads/`](../workloads/) as the exact files
used in the paper, so you do not need to run anything here for those. The
exception is `msr-prxy`, which is not redistributed: download its raw trace
and regenerate it with one command before replaying it (see
[MSR prxy](#msr-prxy)). Use this directory also to audit the workloads, to
regenerate them from the raw data, or to build similar workloads from other
traces.

| File | Purpose |
|---|---|
| `make_workloads.sh` | Driver: regenerates one or all workloads and checks them against the paper's files |
| `netflix.py` | Netflix click-stream CSV -> workload |
| `ethereum.sql`, `ethereum.py` | BigQuery query for the Ethereum transactions; CSV export -> workload |
| `kv_pipeline.py`, `fetch_kv.sh` | Twitter 52, Meta KV, WikiT: streaming hash-sampled collection, then workload generation |
| `block_pipeline.py`, `fetch_alibaba.sh` | MSR prxy, Alibaba device 38: pagination, page hash sampling, workload generation |
| `check_workload.py` | Prints a workload's size and R/W mix and compares it with the paper's file (counts and SHA-256, or equality up to key relabeling) |
| `provenance/<dataset>.txt` | Per-dataset record: source files, time window, mapping, sampling rates/thresholds, intermediate counts, result, commands |

## Summary

All workloads use the same format: one request per line, `key` for a read
or `key value` for a write. Keys are integers: the real traces' keys were
relabeled to dense integers in order of first appearance, except in the
paper's Ethereum file, which labels them 1..701,313 in an arbitrary order
(see [Ethereum](#ethereum)); the synthetic workload uses keys 0..999,999.
The element (value) size is a benchmark parameter (1 KB in the paper),
independent of object or page sizes in the traces.

| Dataset (`../workloads/`) | Source | Window used | Key | Sampling | #Elements (Table 2) | Distinct keys in file | Requests | R/W (%) |
|---|---|---|---|---|---:|---:|---:|---|
| `netflix` | Kaggle Netflix UK movies [17] | 2017-01-01 .. 2019-06-30 (all rows) | movie_id | none | 8,477 | 8,472 | 671,736 | 100/0 |
| `ethereum` | BigQuery `crypto_ethereum.transactions` [12] | blocks 23,049,662 .. 23,129,291 (2025-08-02 00:00 .. 08-13 02:58 UTC) | recipient address | drop recipients seen once | 701,317 | 701,313 | 17,427,629 | 100/0 |
| `synthetic` | generated | - | - | - | 1,000,000 | 173,771 | 1,000,000 | 50/50 |
| `twitter52` | Twitter Twemcache cluster 52 [57] | first hours of the week (prefix of an 8.1 h sample) | object key | object hash, rate 0.04995 | 772,345 | 772,345 | 10,000,000 | 93/7 |
| `metakv` | Meta KV cache 202401 (cacheMon) [6] | first 23.6 h of the 5-day capture | object key | object hash, rate 0.05313 | 988,762 | 988,762 | 7,965,502 | 79/21 |
| `wikit` | Wikimedia text CDN 2019 (cacheMon) [6] | 21 days (10M-request prefix ≈ 18 days) | object key | object hash, rate 0.05469 | 950,243 | 950,243 | 10,000,000 | 100/0 |
| `msr-prxy` | MSR Cambridge, server prxy [40] | full week (2007-02-22 .. 03-01) | 8 KiB page | page hash, 1M pages; first 10M requests | 1,000,000 | 63,296 | 10,000,000 | 66/34 |
| `alibaba-dev38` | Alibaba block traces 2020, device 38 [1] | first 7 days (2020-01-01 .. 01-08, UTC+8) | 4 KiB page | page hash, 1M pages; first 10M requests | 1,000,000 | 246,600 | 10,000,000 | 0/100 |

Bracketed numbers are the paper's references (full citations below).
**#Elements** in Table 2 is the number of elements stored on the server
(`storage_size` in the experiment configs). It equals the distinct keys of
the file for the three key-value traces. It is larger for:

- `msr-prxy` and `alibaba-dev38`: the hash-sampled week-long workloads have
  exactly 1,000,000 pages, and the server stores all of them, but the
  benchmark replays only their first 10M requests, which touch 63,296 and
  246,600 pages respectively;
- `synthetic`: the key space is 1,000,000 keys, of which 1M requests touch
  ~174K;
- `netflix` and `ethereum`: the paper's configurations allocate 5 and 4
  more elements than the files reference; the extra elements are never
  accessed and do not affect the results.

All counts above were measured on the files in `../workloads/`
(`check_workload.py`; for `msr-prxy`, on the regenerated file) and agree
with Table 2.

## Spatial hash sampling

The production traces are far too large to replay whole (Twitter cluster 52
alone has billions of requests), so the key-value and block traces are
subsampled **by object (or page), never by request**, using spatial hash
sampling as in SHARDS [55]. An object with key `k` is kept iff

```python
h = int.from_bytes(hashlib.blake2b(k, digest_size=8, key=b"cloak1").digest(), "big")
keep = h < rate * 2**64          # block traces: h < hash_threshold
```

i.e. a 64-bit BLAKE2b hash keyed with the salt `cloak1`, read big-endian.
For block traces `k` is `b"<stream>:<page>"` (e.g. `b"prxy_0:1234"`,
`b"38:1234"`). Consequences:

- every distinct object survives with probability `rate`, independent of
  its popularity, so the popularity distribution is preserved in
  expectation and reuse distances scale by about `rate`;
- **all** requests to a kept object are kept, in their original order: the
  workload is a contiguous prefix of the filtered stream, so temporal
  locality is preserved;
- thresholds nest: filtering at a smaller rate gives a strict subset, so a
  sample collected at a generous rate can be tightened later without
  re-reading the source.

The rate was chosen to give about one million elements:

- **Key-value traces** (`kv_pipeline.py`): stream the source once at a
  generous rate (stage 1, `collect`; capped at 30M kept requests for
  Twitter 52 and Meta KV, while WikiT is read to the end), then
  re-filter that sample at a tighter, nested rate (stage 2, `regen`) cut at
  10M requests / 1.25M distinct keys. The stage-2 rate was found by binary
  search for 1M distinct keys (`regen --target-keys 1000000`); the exact
  resulting rates are in the provenance files and in `make_workloads.sh`.
  Because the stage-1 cap defines the window for Twitter 52 and Meta KV,
  stage 1 must be rerun with exactly the documented rate and cap.
- **Block traces** (`block_pipeline.py`): a `survey` pass records the hash
  of every distinct page in the window; the threshold is the
  1,000,000-th smallest hash + 1, so the workload has exactly 1M pages. The
  resulting thresholds are documented, so `emit --hash-threshold` can skip
  the survey.

## Datasets

### Netflix

- **Source**: S. Follows and J. E. Camargo-Molina, *Netflix audience
  behaviour - UK movies*, Kaggle, 2021,
  <https://www.kaggle.com/datasets/vodclickstream/netflix-audience-behaviour-uk-movies>
  (CC BY-NC-SA 3.0 IGO; file `vodclickstream_uk_movies_03.csv`; needs a Kaggle
  account). The shipped `netflix.xz` is an adaptation and is distributed under
  the same license, not MIT; see `../workloads/netflix.LICENSE.txt`.
- **Transformation** (`netflix.py`): each click-stream row is a read of its
  `movie_id`; rows are sorted by `datetime` (stable); no sampling.
- **Reproducibility**: exact (byte-identical output).

### Ethereum

- **Source**: Google BigQuery public dataset
  `bigquery-public-data.crypto_ethereum.transactions`; A. Day and
  E. Medvedev, *Ethereum in BigQuery: A public dataset for smart contract
  analytics*, Google Cloud blog, 2018.
- **Window**: all 19,066,487 transactions in blocks 23,049,662 through
  23,129,291 (2025-08-02 00:00:11 to 2025-08-13 02:58:11 UTC).
- **Transformation** (`ethereum.py`): each transaction is a read of its
  recipient (`to_address`); recipients seen only once in the window are
  dropped (1,638,858 of 2,340,171); contract creations (empty `to_address`)
  share one key.
- **Order**: the paper's file concatenates the 11 shards of the original
  BigQuery export in file-name order, which is chronological except that two
  chunks of about 20 h and 25 h are displaced (table in
  `provenance/ethereum.txt`). `ethereum.py --order paper` (the default)
  rebuilds exactly that sequence from any export of the block range;
  `--order chronological` gives strict execution order.
- **Reproducibility**: the original export query was not saved;
  `ethereum.sql` reconstructs it from the exported data (same block range
  and columns). The paper's file labels keys in an arbitrary order (1-based,
  hash-map order); `ethereum.py` output matches it request-for-request up
  to a one-to-one relabeling of keys, which was verified against the
  original export (`check_workload.py --same-as`).

### Synthetic (Zipf s = 1)

- **Generator**: `gen_workload` (`src/bin/gen_workload.rs` in the repository
  root). All N keys are kept in a recency list; for each request a rank r is
  drawn from a Zipf(N, s) distribution, the key at list position r-1 (whose
  current reuse distance is r-1) is queried and moved to the front, and the
  request is a write with probability 0.5 (value = request index).
- **Parameters**: N = 1,000,000 keys, 1,000,000 requests, write ratio 0.5,
  s = 1.0. The same file is the s = 1.0 point of the Zipf-exponent sweep.
- **Reproducibility**: statistical only. The paper's file came from an
  unseeded RNG; a regenerated file has the same length and mix and ~174K
  distinct keys but is not byte-identical.

### Twitter 52

- **Source**: J. Yang, Y. Yue, K. V. Rashmi, *A large scale analysis of
  hundreds of in-memory cache clusters at Twitter*, OSDI '20; traces at
  <https://github.com/twitter/cache-trace> (CC-BY 4.0), file
  `cluster52.sort.zst` from
  <https://ftp.pdl.cmu.edu/pub/datasets/twemcacheWorkload/open_source/>.
- **Mapping**: `get`/`gets` -> read; `set`, `add`, `replace`, `cas`,
  `append`, `prepend`, `incr`, `decr` -> write; `delete` dropped.
- **Sampling**: stage 1 rate 0.05, stopped at 30M kept requests after
  617,289,256 trace lines (trace seconds 0 to 29,318); stage 2 rate
  0.049951171875000006, cut at 10M requests. At this collection rate the
  10M-request window only holds ~772K objects, so the workload has fewer
  than 1M elements.
- **Reproducibility**: exact (deterministic; check with the SHA-256).

### Meta KV

- **Source**: Meta CacheLib key-value cache traces, release 202401, from
  the cacheMon open-source cache dataset,
  <https://github.com/cacheMon/cache_dataset> (CC-BY 4.0):
  `2022_metaKV/kvcache_202401/kvcache_traces_{1..5}.csv.zst`.
- **Mapping**: `GET`/`GET_LEASE` -> read; `SET`/`SET_LEASE` -> write;
  `DELETE` dropped; one CSV row is one request.
- **Sampling**: stage 1 rate 0.2, stopped at 30M kept requests after
  152,409,269 lines (23.6 h of the capture); stage 2 rate
  0.053125000000000006, which consumes the whole stage-1 window (7.97M
  requests).
- **Reproducibility**: exact.

### WikiT

- **Source**: Wikimedia CDN text (page-view) cluster, 2019 21-day trace,
  from the cacheMon dataset (CC-BY 4.0): `2019_wiki/wiki/2019/wiki.txt.2019.zst`.
- **Mapping**: every row is a read of its hashed URL.
- **Sampling**: stage 1 rate 0.25 over the whole trace (207,646,002
  lines); stage 2 rate 0.0546875, cut at 10M requests.
- **Reproducibility**: exact.

### MSR prxy

- **Source**: D. Narayanan, A. Donnelly, A. Rowstron, *Write off-loading:
  Practical power management for enterprise storage*, FAST '08; traces at
  SNIA IOTTA, <http://iotta.snia.org/traces/block-io/388>. Only
  `msr-cambridge1.tar` (3.3 GB) is needed; it holds both `prxy` volumes.
- **Not redistributed**: the MSR distribution carries a Microsoft notice
  that reserves reproduction rights, so this repository does not include
  the workload. Download `msr-cambridge1.tar` from the page above (free,
  after accepting the SNIA Trace Data Files Download License on the web
  form) into `data/msr/`, then run

  ```bash
  ./make_workloads.sh msr-prxy
  ```

  This reads the first ~10M I/Os of the two volumes from the tar, writes
  `out/msr-prxy`, compares it with the paper's workload (SHA-256
  `863c7e4eeef6bb31961b9b6c22cba474e615c3480239a273d4ef3697ecfffbfe` of the
  decompressed file) and, if it matches, installs it as
  `../workloads/msr-prxy.xz` (`xz -T0 -6`; ignored by git), where
  [`../run.sh`](../run.sh) and
  [`../../temporal-locality/run.sh`](../../temporal-locality/run.sh) expect
  it. On a 2-vCPU cloud VM this took 1.5 minutes with a peak memory of
  230 MB (the Python step: 1 minute, 63 MB; the rest is `xz`). Until then,
  both scripts skip `msr-prxy` with a warning.
- **Tenant**: server `prxy` (firewall/web proxy), both volumes `prxy_0` and
  `prxy_1`, merged in timestamp order; the whole one-week trace.
- **Pagination**: 8 KiB pages; an I/O at `[offset, offset+size)` issues one
  request per page it touches; reads -> read, writes -> write. 8 KiB
  maximizes "I/Os that fit in one page minus I/Os that falsely share a page
  with a byte-disjoint I/O" for this trace (4 KiB scores lower).
- **Sampling**: the week has 1,927,322 distinct pages; the 1M-page
  threshold is 9571094887790794229 (rate 0.51885), giving 185,262,952
  requests over the week. The benchmark workload is the first 10M of them.
- **Reproducibility**: exact (deterministic; check with the SHA-256). The
  command above was verified to reproduce the paper's file byte for byte.

### Alibaba device 38

- **Source**: Alibaba block traces 2020, <https://github.com/alibaba/block-traces>
  (CC-BY 4.0), streamed from
  `http://block-traces.oss-cn-beijing.aliyuncs.com/alibaba_block_traces_2020.tar.gz`
  (~194 GB; `fetch_alibaba.sh` keeps only device 38).
- **Tenant/window**: virtual disk 38 (50 GiB, all writes), first 7 days of
  the 31-day trace (timestamps below 1578412800000000 us).
- **Pagination**: 4 KiB pages, as above.
- **Sampling**: the 7 days touch 2,874,345 distinct pages; the 1M-page
  threshold is 6415815174883080656 (rate 0.34780), giving 275,089,919
  requests. The benchmark workload is the first 10M of them (roughly 6 h of
  trace at the average request rate), so a 24-hour capture
  (`./fetch_alibaba.sh data/ali 1577894400000000`) suffices when the
  threshold is given.
- **Reproducibility**: exact, provided the capture is complete (the stream
  cannot be resumed; `fetch_alibaba.sh` retries from scratch).

## Regenerating

Requirements: Python >= 3.10 (standard library only; `PY="uv run python"`
works too), `curl`, `zstd`, `tar`, `grep`, `awk`, `xz`; Cargo for the
synthetic workload. All commands run from this directory. Raw data go to
`data/`, outputs to `out/` (override with `DATA=` / `OUT=`).

Manual downloads first (licenses or accounts required):

```bash
mkdir -p data/netflix data/ethereum data/msr
# Netflix: vodclickstream_uk_movies_03.csv from Kaggle  -> data/netflix/
# Ethereum: run ethereum.sql in BigQuery, export as CSV -> data/ethereum/*.csv
# MSR: msr-cambridge1.tar from SNIA -> data/msr/ (msr-prxy is not shipped)
```

Then, per dataset or all at once:

```bash
./make_workloads.sh netflix twitter52     # or: ./make_workloads.sh all
```

`make_workloads.sh` streams the Twitter, Meta and Wikimedia traces and the
Alibaba tarball itself (`fetch_kv.sh`, `fetch_alibaba.sh`); only the hash
samples and the device-38 capture are stored (several GB in total), never
the raw traces. Every
workload is checked with `check_workload.py` at the end, e.g.:

```bash
python3 check_workload.py out/twitter52 --dataset twitter52
python3 check_workload.py out/ethereum --dataset ethereum --same-as ../workloads/ethereum.xz
```

For `msr-prxy`, a matching file is also installed as
`../workloads/msr-prxy.xz`; the other workloads ship, so their regenerated
copies stay in `out/`. The exact commands for each dataset, including the
full survey-based method for the block traces, are at the end of each
`provenance/<dataset>.txt`.

Cost: the scripts are single-threaded Python and process somewhat under
1M trace lines per second, so the large sources take tens of minutes to a
few hours each (Twitter 52 reads 617M lines; the Alibaba capture streams
the tarball up to the 7-day mark; a block-trace `survey` over the full
week reads 180-230M I/Os); Ethereum needs ~3 GB of RAM. Running on a
cloud VM close to the data is recommended. The Twitter, cacheMon and
Alibaba downloads are hosted by third parties and may move; the SHA-256
values in `check_workload.py` let you confirm that a regenerated workload
is the one used in the paper.

## References

- [1] Alibaba Group. Alibaba block traces. <https://github.com/alibaba/block-traces>, 2020.
- [6] cacheMon. Open-source cache dataset. <https://github.com/cacheMon/cache_dataset>, 2024 (Meta KV and Wikimedia CDN traces).
- [12] A. Day and E. Medvedev. Ethereum in BigQuery: A public dataset for smart contract analytics. Google Cloud blog, 2018.
- [17] S. Follows and J. E. Camargo-Molina. Netflix audience behaviour - UK movies. Kaggle, 2021.
- [40] D. Narayanan, A. Donnelly, and A. Rowstron. Write off-loading: Practical power management for enterprise storage. FAST '08.
- [55] C. A. Waldspurger, N. Park, A. Garthwaite, and I. Ahmad. Efficient MRC construction with SHARDS. FAST '15.
- [57] J. Yang, Y. Yue, and K. V. Rashmi. A large scale analysis of hundreds of in-memory cache clusters at Twitter. OSDI '20.
