# Datasets (Table 2): Cloak vs. the unsafe baseline

This experiment produces the Cloak and unsafe-baseline columns of **Table 2**
of the paper: throughput (ops/s), mean end-to-end query latency (ms) and p99
latency (ms) on eight workloads: seven real traces and one synthetic Zipf trace.

## What is measured

Each run replays one workload file through the three binaries (client, proxy
and server, each on its own machine) and writes one stats row (columns and
their definitions: [`../README.md`](../README.md#result-files)). Each Table 2
cell is the mean of a column (`throughput`, `avg_latency`, `p99_latency`) over
the repetitions (rows) of one result file.

Every dataset is run in two modes with the **same config**:

- **cloak**: the full protocol. Each batch is filled according to the fixed
  reuse-distance budgets, with dummy accesses in unused slots. After each
  batch the proxy re-encrypts the accessed elements, shuffles them within the
  batch and writes them back.
- **unsafe**: the same binaries with `--proxy-is-unsafe true`. Requests that
  arrive within one batch interval are coalesced per key, and each batch
  accesses only the requested elements: one round trip to the server that
  writes the batch's writes and one that reads its reads. There are no dummy
  accesses and no reshuffling, and elements never move. The access pattern is
  visible to the server.

## Datasets

Seven of the eight workloads ship as `workloads/<label>.xz` (`xz -T0 -6`,
98 MB in total, 373 MB decompressed); the harness decompresses each one once
into `$CLOAK_ROOT/.cache/workloads/`. Each line is one request: `key` for a
read, `key value` for a write. Every element is 1 KB in all runs
(`object_size = 1024`), including the block traces. Source citations follow
the paper's reference numbers. *#Elements* is the number of elements stored
on the server (`storage_size` in the config); *Requests* is the number of
lines in the workload file.

`msr-prxy` is not included, because the MSR Cambridge traces are not
redistributed. Download the raw trace from SNIA (3.3 GB) and run
`prep/make_workloads.sh msr-prxy` (about 1.5 minutes), which regenerates the
paper's file exactly, checks its SHA-256 and installs it as
`workloads/msr-prxy.xz` (see [`prep/README.md`](prep/README.md#msr-prxy)).
Until then, `run.sh` skips `msr-prxy-cloak` and `msr-prxy-unsafe` with a
warning.

| Label | Source | #Elements | R/W (%) | Requests |
|---|---|---:|---:|---:|
| `netflix` | Netflix audience behaviour, UK movies (Kaggle) [17] | 8,477 | 100/0 | 671,736 |
| `ethereum` | Ethereum transactions, BigQuery public data set [12] | 701,317 | 100/0 | 17,427,629 |
| `synthetic` | Synthetic Zipf reuse-distance trace, s = 1.0 (same file as `../zipf-exponent/workloads/s1.0.xz`) | 1,000,000 | 50/50 | 1,000,000 |
| `twitter52` | Twitter Twemcache cluster 52 [57] | 772,345 | 93/7 | 10,000,000 |
| `metakv` | Meta KV cache trace, cacheMon [6] | 988,762 | 79/21 | 7,965,502 |
| `wikit` | Wikimedia 2019 text CDN (WikiT), cacheMon [6] | 950,243 | 100/0 | 10,000,000 |
| `msr-prxy` | MSR Cambridge block traces, server `prxy` [40] | 1,000,000 | 66/34 | 10,000,000 |
| `alibaba-dev38` | Alibaba block traces 2020, virtual disk 38 [1] | 1,000,000 | 0/100 | 10,000,000 |

The key-value and block traces were subsampled by object (or page) with
spatial hash sampling, which keeps every access to each selected object in
the original order; the block traces were first mapped to pages. How each
workload was derived (time window, mapping, sampling, key labeling), the
licenses and full citations, and scripts that regenerate the workloads from
the public sources are in [`prep/README.md`](prep/README.md), with
per-workload details in `prep/provenance/`.

## Configuration

`configs/<label>.toml` holds the exact parameter values used for the paper.
The server and proxy addresses are placeholders that the harness overrides.
The shared settings are:

- 1 KB elements, AES-128-GCM (32 bytes of overhead per stored element)
- 16 proxy worker threads, 1 client connection
- proxy queue capacity = 2 × batch size (`queue_coefficient = 2`)
- budgets follow a discrete Zipf distribution with s = 1.0 (the paper's
  Algorithm 3), set by `first_batch_size`
- budget log disabled (`enable_budget_log = false`), so these runs produce no
  `-ratio.csv` files

The parameters that differ between datasets:

| Label | `storage_size` | `first_batch_size` | batch size (Σ budgets) | `cache_size` | `batch_interval_ms` |
|---|---:|---:|---:|---:|---:|
| `netflix` | 8,477 | 100 | 520 | 100 | 5 |
| `ethereum` | 701,317 | 433 | 3,744 | 10,000 | 20 |
| `synthetic` | 1,000,000 | 433 | 3,999 | 10,000 | 20 |
| `twitter52` | 772,345 | 433 | 3,810 | 10,000 | 20 |
| `metakv` | 988,762 | 433 | 3,990 | 10,000 | 20 |
| `wikit` | 950,243 | 433 | 3,960 | 10,000 | 20 |
| `msr-prxy` | 1,000,000 | 433 | 3,999 | 10,000 | 20 |
| `alibaba-dev38` | 1,000,000 | 433 | 3,999 | 10,000 | 20 |

Netflix has very few elements, so it uses a smaller batch (520), a smaller
cache (100) and a shorter batch interval (5 ms). All other datasets use
`first_batch_size = 433`, which gives a batch of about 4,000. The exact batch
size is Σ budgets, so it depends slightly on `storage_size`. The Table 2 runs
used a 10,000-entry cache (100 for Netflix), as set in these configs. The
Figure 3 ablations use the 1,000-entry default listed in §7.1 of the paper.

## Client rates

`run.sh` gives each mode its own client rate (requests/s). These are the
rates used for the paper:

| Label | cloak rate | unsafe rate | How the rate was chosen |
|---|---:|---:|---|
| `netflix` | 175,000 | 175,000 | fixed offered rate (config `req_per_sec`) |
| `ethereum` | 170,000 | 170,000 | fixed offered rate (config `req_per_sec`) |
| `synthetic` | 155,000 | 155,000 | fixed offered rate (config `req_per_sec`) |
| `twitter52` | 154,000 | 149,000 | per-mode maximum (probe: 154,794 / 149,931) |
| `metakv` | 142,000 | 156,000 | per-mode maximum (probe: 142,490 / 156,975) |
| `wikit` | 104,000 | 159,000 | per-mode maximum (probe: 104,567 / 159,540) |
| `msr-prxy` | 166,000 | 162,000 | per-mode maximum (probe: 166,826 / 162,068) |
| `alibaba-dev38` | 35,000 | 183,000 | per-mode maximum (probe: 35,851 / 183,122) |

- **Netflix, Ethereum, synthetic:** both modes run at the same offered rate,
  set at or above what either mode can sustain. The reported throughput is
  therefore the achieved (saturated) throughput.
- **The other five datasets:** each mode runs at its own maximum sustainable
  rate. To find it, one probe run per mode drove the client at 200,000 req/s,
  well above saturation. The final runs then used the probe's achieved
  throughput, rounded down to the nearest 1,000 req/s. This way latency is
  measured at, rather than beyond, each mode's capacity. The configs keep the
  200,000 req/s probe rate as `req_per_sec`; `run.sh` overrides it.

Maximum rates depend on the hardware. On machines other than the paper's,
re-probe first and then edit the rate table at the top of `run.sh`:

```bash
PROBE=1 ./run.sh                       # one run per mode at 200,000 req/s
                                       # -> results/<dataset>-{cloak,unsafe}-probe-stats.csv
PROBE=1 PROBE_RATE=250000 ./run.sh     # a different probe rate
```

## Running

Set up the three-machine cluster and the inventory as described in the
top-level [`README.md`](../../README.md); the harness options are documented
in [`../README.md`](../README.md#harness-variables). From this directory:

```bash
./run.sh                               # full run: 8 datasets x 2 modes x REPS=5
REPS=1 ./run.sh                        # one repetition
SMOKE=1 ./run.sh                       # 1 rep on the first 200,000 requests of each workload
ONLY='^wikit-' ./run.sh                # one dataset, both modes
ONLY='-unsafe$' ./run.sh               # only the unsafe baseline
DRY_RUN=1 ./run.sh                     # print what would run
LOCAL=1 SMOKE=1 SMOKE_REQUESTS=20000 ONLY='^netflix-' ./run.sh   # tiny run on localhost
```

Skipped labels (`msr-prxy` while its workload is missing) are listed in the
final summary and do not count as failures: `run.sh` exits 0 if every run
that started succeeded, and non-zero if a run failed or nothing ran (e.g.
`ONLY='^msr-prxy-'` without the workload).

The repetition loop is the outer loop. Within each repetition, every dataset
runs Cloak and then the unsafe baseline. Results are appended to
`results/<label>-stats.csv`, one row per repetition, with labels
`<dataset>-cloak` and `<dataset>-unsafe`. A log of all runs goes to
`results/runs.log`, and per-run logs go to `results/logs/`. SMOKE runs write
to `results-smoke/` instead.

`LOCAL=1` puts all three processes on one machine. It is meant for checking
that everything works, and its numbers are not comparable to Table 2. SMOKE
runs are also too short for meaningful throughput.

**Network path.** Netflix (5 ms batches of 520 elements) is sensitive to how
the machines address each other: on AWS, `netflix-cloak` reaches the paper's
throughput over the instances' public IPs but less than a third of it over
VPC-internal IPs. See "Network path" in the top-level
[`README.md`](../../README.md#1-machines).

**Duration.** On the paper's hardware (3× AWS m6a.4xlarge, 16 vCPUs and 64 GB
RAM each), the client-side run time of one repetition is about 19 minutes for
all 16 runs. The longest are `alibaba-dev38-cloak` (about 4.8 min, 10M
requests at 35k req/s), the two Ethereum runs (about 1.9 min each) and
`wikit-cloak` (about 1.6 min). Orchestration, server start-up and proxy
initialization add about 20 s per run with the controller in the same region:
about 24 minutes per repetition, or about 2 hours for the default 5
repetitions. The first run of each dataset also copies its workload to the
client (430 MB in total); from a remote controller this can take minutes,
and later runs skip the copy.

## Producing the table

From the repository root:

```bash
uv run python plots/table2_datasets.py            # from your results/
uv run python plots/table2_datasets.py --paper    # from paper-results/ (reproduces Table 2)
uv run python plots/table2_datasets.py --smoke    # from results-smoke/
uv run python plots/table2_datasets.py --results-dir DIR   # from another folder
uv run python plots/table2_datasets.py --latex    # also write a LaTeX tabular
```

The script prints a Markdown table and writes `plots/out/table2_datasets.csv`
(and `table2_datasets.tex` with `--latex`). Missing result files are shown as
`n/a`.

## Paper results

`paper-results/<label>-stats.csv` are the raw result rows behind Table 2, in
the 7-column stats format, one row per repetition. Netflix, Ethereum and
synthetic have 4 repetitions per mode; the other five datasets have 5. Each
file was produced with the config in `configs/` and the rate in the table
above. `--proxy-is-unsafe true` was added for the `-unsafe` files. Running
`plots/table2_datasets.py --paper` on these files reproduces every Cloak and
unsafe throughput, mean latency and p99 latency number in Table 2 exactly.
