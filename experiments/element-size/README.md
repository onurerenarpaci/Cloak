# Element-size sweep (Figure 3, column 4)

Varies the size of every stored element from 0.5 KB to 32 KB (N = 10^6
elements, so the server stores about 0.5-33 GB). Each batch transfers a fixed
number of elements, so encryption, shuffling and network cost per batch grow
linearly with the element size, while the batching interval stays at 20 ms.

Paper: Section 7.2.2 "Effect of element size", Figure 3 fourth column
(throughput, mean/p99 latency, batch utilization vs. element size).

## Parameter grid

All runs use [`configs/element-size.toml`](configs/element-size.toml) (N =
10^6, batch size 4,000 = first-set budget 433, cache 1,000 entries, queue
capacity 2x batch size, 20 ms batching interval, 16 threads). Per run,
`run.sh` passes `--common-object-size` (to server, proxy and client) and
`--client-req-per-sec` (a target close to the maximum Cloak sustains at that
size, as used for the paper).

| label           | element size (`--common-object-size`, bytes) | client rate (req/s) |
|-----------------|-------:|--------:|
| `element-512`   | 512    | 171,000 |
| `element-1024`  | 1,024  | 148,000 |
| `element-2048`  | 2,048  | 94,000  |
| `element-4096`  | 4,096  | 53,000  |
| `element-8192`  | 8,192  | 23,000  |
| `element-16384` | 16,384 | 9,000   |
| `element-32768` | 32,768 | 2,500   |

7 configurations x 5 repetitions = 35 runs (repetitions are the outer loop).
`element-1024` is the default configuration used by the other experiments.

## Workload

The Zipf s = 1.0 workload (1,000,000 requests over 10^6 keys, 50% writes) is
not stored here: `run.sh` uses `../zipf-exponent/workloads/s1.0.xz`, the same
file as the `s1.0` point of the Zipf sweep.

## Running

```sh
cd experiments/element-size
./run.sh                                   # full sweep: 5 repetitions on the cluster
SMOKE=1 ./run.sh                           # 1 repetition, first 200,000 requests
ONLY='^element-(512|32768)$' ./run.sh      # a subset
LOCAL=1 SMOKE=1 ONLY='^element-512$' ./run.sh   # on this machine
DRY_RUN=1 ./run.sh                         # print the commands only
```

The server keeps all N elements in memory: it needs about N x (element size +
32 B), i.e. ~0.5 GB for `element-512` but ~33 GB for `element-32768`. Keep
`LOCAL=1` runs to the small sizes. See [`../README.md`](../README.md#harness-variables)
for all harness variables.

**Expected runtime** on the paper's hardware (3x AWS m6a.4xlarge, 16 vCPU /
64 GB): the client phase is ~6 s at 0.5 KB but ~110 s at 16 KB and ~400 s at
32 KB (1M requests at 2,490 req/s), ~10 min per repetition in total. The
fixed cost per run is about 20 s at small sizes (more with a remote
controller) and grows with the element size, because the proxy initializes
the whole store on the server: ~2.5 min at 32 KB, where about 33 GB are
written. A repetition takes ~17 min and the full sweep ~1.5 hours.

## Results and plotting

Results go to `results/<label>-stats.csv` and `results/<label>-ratio.csv`
(`results-smoke/` with `SMOKE=1`), one row per repetition; the columns are
described in [`../README.md`](../README.md#result-files). `paper-results/`
holds the 5 repetitions behind Figure 3 in the same format. From the
repository root, `uv run python plots/fig3_ablations.py` draws Figure 3 from
`results/` (`--paper`, `--smoke` and `--results-root` select other data; see
[`../README.md`](../README.md#plotting)). The x axis shows the element size
in KB (label bytes / 1024).
