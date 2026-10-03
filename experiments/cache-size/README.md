# Cache-size sweep (Figure 3, column 3)

Varies the number of entries in the proxy's cache (mini-moka: TinyLFU
admission, LRU eviction) from 1,000 to 256,000. Queries served from the cache skip the batch round-trip to the server, so mean
latency drops and fewer real requests enter the batches. Because the proxy
stops accepting queries when its request queue is full, a saturated client
would mask this effect; the sweep therefore runs the client at a fixed
115,000 requests/s, below Cloak's maximum, and uses a request queue of 1x the
batch size.

Paper: Section 7.2.2 "Effect of cache size", Figure 3 third column (the
throughput panel is flat because the rate is fixed).

## Parameter grid

All runs use [`configs/cache-size.toml`](configs/cache-size.toml): N = 10^6
elements of 1 KB, batch size 4,000 (first-set budget 433), **queue capacity
1x batch size** (`queue_coefficient = 1`), **client rate 115,000 req/s**
(`req_per_sec`), 20 ms batching interval, 16 threads, and the client's
per-query output log enabled, exactly as for the paper. Per run, `run.sh`
only passes `--proxy-cache-size`.

| label          | `--proxy-cache-size` |
|----------------|--------:|
| `cache-1000`   | 1,000   |
| `cache-2000`   | 2,000   |
| `cache-4000`   | 4,000   |
| `cache-8000`   | 8,000   |
| `cache-16000`  | 16,000  |
| `cache-32000`  | 32,000  |
| `cache-64000`  | 64,000  |
| `cache-128000` | 128,000 |
| `cache-256000` | 256,000 |

9 configurations x 5 repetitions = 45 runs (repetitions are the outer loop).

## Workload

The Zipf s = 1.0 workload (1,000,000 requests over 10^6 keys, 50% writes) is
not stored here: `run.sh` uses `../zipf-exponent/workloads/s1.0.xz`, the same
file as the `s1.0` point of the Zipf sweep.

## Running

```sh
cd experiments/cache-size
./run.sh                                   # full sweep: 5 repetitions on the cluster
SMOKE=1 ./run.sh                           # 1 repetition, first 200,000 requests
ONLY='^cache-(1000|256000)$' ./run.sh      # a subset
LOCAL=1 SMOKE=1 ONLY='^cache-1000$' ./run.sh   # on this machine (~1.1 GB RAM)
DRY_RUN=1 ./run.sh                         # print the commands only
```

See [`../README.md`](../README.md#harness-variables) for all harness variables.

**Expected runtime** on the paper's hardware (3x AWS m6a.4xlarge, 16 vCPU /
64 GB): each run's client phase takes ~9 s (1M requests at ~111,000 req/s
achieved), plus about 20 s of fixed cost per run (orchestration, server
start, storage initialization; more with a remote controller): ~4.5 min per
repetition, ~20 min for the full sweep.

## Results and plotting

Results go to `results/<label>-stats.csv` and `results/<label>-ratio.csv`
(`results-smoke/` with `SMOKE=1`), one row per repetition; the columns are
described in [`../README.md`](../README.md#result-files). `paper-results/`
holds the 5 repetitions behind Figure 3 in the same format. From the
repository root, `uv run python plots/fig3_ablations.py` draws Figure 3 from
`results/` (`--paper`, `--smoke` and `--results-root` select other data; see
[`../README.md`](../README.md#plotting)).
