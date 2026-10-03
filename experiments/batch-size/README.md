# Batch-size sweep (Figure 3, column 2)

Fixes the input to the Zipf s = 1.0 workload (matching Cloak's anticipated
distribution) and varies the batch size from 2,000 to 11,000. Small batches
leave too few budget slots for real requests; large batches cannot be filled
and processed within the fixed 20 ms batching interval.

Paper: Section 7.2.2 "Effect of batch size", Figure 3 second column
(throughput, mean/p99 latency, batch utilization vs. batch size).

## Parameter grid

The batch size is the sum of the budgets of all reuse-distance sets. Cloak
derives the budgets from the budget of the first (most recent) set,
`first_batch_size`, with a Zipf s = 1.0 allocation (paper Algorithm 3), so the
sweep sets `--proxy-first-batch-size`. The label is the resulting batch size
as plotted on the paper's x axis; the exact sum of budgets is reported by
every run as `calculated_batch_size` in the ratio file.

All runs use [`configs/batch-size.toml`](configs/batch-size.toml) (N = 10^6
elements of 1 KB, cache 1,000 entries, queue capacity 2x batch size, 20 ms
batching interval, 16 threads). Per run, `run.sh` passes
`--proxy-first-batch-size` and `--client-req-per-sec` (a target close to the
maximum Cloak sustains at that batch size, as used for the paper).

| label         | `--proxy-first-batch-size` | sum of budgets (`calculated_batch_size`) | client rate (req/s) |
|---------------|------:|-------:|--------:|
| `batch-2000`  | 121   | 2,008  | 64,000  |
| `batch-3000`  | 282   | 2,999  | 126,000 |
| `batch-4000`  | 433   | 3,999  | 149,000 |
| `batch-5000`  | 583   | 5,001  | 144,000 |
| `batch-6000`  | 732   | 5,996  | 128,000 |
| `batch-7000`  | 888   | 6,998  | 113,000 |
| `batch-8000`  | 1046  | 7,999  | 108,000 |
| `batch-9000`  | 1207  | 9,002  | 105,000 |
| `batch-10000` | 1370  | 9,998  | 94,000  |
| `batch-11000` | 1537  | 11,002 | 92,000  |

10 configurations x 5 repetitions = 50 runs (repetitions are the outer loop).
`batch-4000` is the default configuration used by the other experiments.

## Workload

The Zipf s = 1.0 workload (1,000,000 requests over 10^6 keys, 50% writes) is
not stored here: `run.sh` uses `../zipf-exponent/workloads/s1.0.xz`, the same
file as the `s1.0` point of the Zipf sweep (and the cache-size and
element-size sweeps).

## Running

```sh
cd experiments/batch-size
./run.sh                                  # full sweep: 5 repetitions on the cluster
SMOKE=1 ./run.sh                          # 1 repetition, first 200,000 requests
ONLY='^batch-(2000|4000|11000)$' ./run.sh # a subset
LOCAL=1 SMOKE=1 ONLY='^batch-4000$' ./run.sh   # on this machine (~1.1 GB RAM)
DRY_RUN=1 ./run.sh                        # print the commands only
```

See [`../README.md`](../README.md#harness-variables) for all harness variables.

**Expected runtime** on the paper's hardware (3x AWS m6a.4xlarge, 16 vCPU /
64 GB): the client phase of a repetition takes ~100 s in total (7-16 s per
run), plus about 20 s of fixed cost per run (orchestration, server start,
storage initialization; more with a remote controller): ~5 min per
repetition, ~25 min for the full sweep.

## Results and plotting

Results go to `results/<label>-stats.csv` and `results/<label>-ratio.csv`
(`results-smoke/` with `SMOKE=1`), one row per repetition; the columns are
described in [`../README.md`](../README.md#result-files). `paper-results/`
holds the 5 repetitions behind Figure 3 in the same format. From the
repository root, `uv run python plots/fig3_ablations.py` draws Figure 3 from
`results/` (`--paper`, `--smoke` and `--results-root` select other data; see
[`../README.md`](../README.md#plotting)). The x axis uses the batch size from
the label.
