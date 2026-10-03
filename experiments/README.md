# Experiments

One folder per experiment: the Table 2 and Figure 3 cluster experiments of the
paper's evaluation (Section 7), and the offline temporal-locality analysis
behind Figure 2 (Section 6) and Figure 4 (Appendix A). Every folder has a
`README.md` (what is measured, parameter grid, expected runtime), a `run.sh`
and `paper-results/`, the raw results the paper's figure or table was made
from. The cluster experiments also have `configs/`. `datasets/` and
`zipf-exponent/` ship their workloads (xz-compressed, in `workloads/`),
except the MSR prxy workload of `datasets/`, which is not redistributed and
is regenerated from the raw trace with one command
([`datasets/prep/README.md`](datasets/prep/README.md#msr-prxy));
`batch-size/`, `cache-size/` and `element-size/` reuse the Zipf s = 1.0
workload `zipf-exponent/workloads/s1.0.xz`, and `temporal-locality/` reads the
workloads of `datasets/`.

| Folder | Paper item | Needs a cluster |
|---|---|---|
| [`datasets/`](datasets/) | Table 2: Cloak vs. the unsafe baseline on 8 datasets | yes |
| [`zipf-exponent/`](zipf-exponent/) | Figure 3, column 1: workload skew (Zipf s = 0.0 ... 2.0) | yes |
| [`batch-size/`](batch-size/) | Figure 3, column 2: batch size 2,000 ... 11,000 | yes |
| [`cache-size/`](cache-size/) | Figure 3, column 3: proxy cache 1,000 ... 256,000 entries | yes |
| [`element-size/`](element-size/) | Figure 3, column 4: element size 0.5 ... 32 KB | yes |
| [`temporal-locality/`](temporal-locality/) | Figures 2 and 4: reuse-interval histograms of the traces | no (offline analysis) |

## How a run works

`run.sh` loops over repetitions (outer loop) and configurations (inner loop)
and calls the shared harness [`common.sh`](common.sh) once per repetition and
configuration. One repetition of one configuration:

1. the harness decompresses the workload once into `.cache/workloads/` at the
   repository root (with `SMOKE=1`, it uses a copy cut to the first
   `SMOKE_REQUESTS` requests);
2. [`ansible/run.yml`](../ansible/run.yml) stops leftover Cloak processes on
   the three machines, copies the config to all of them and the workload to
   the client (skipped if the client already has an identical copy), starts
   the server, then the proxy (which initializes the encrypted storage on the
   server before it accepts the client), then the client; it waits for the
   client to finish and the proxy to exit, turns the proxy's budget log into
   `ratio.csv` (batch utilization) with `budget_ratio`, fetches `stats.csv`
   and `ratio.csv`, and stops everything again. With `LOCAL=1`,
   [`scripts/run-local.sh`](../scripts/run-local.sh) does the same with all
   three processes on this machine;
3. the harness appends the row of `stats.csv` to
   `results/<label>-stats.csv` and the row of `ratio.csv` to
   `results/<label>-ratio.csv` (writing the CSV header if the file is new).
   Rows are repetitions; re-running an experiment appends more rows, so move
   `results/` away to start from scratch. Configurations without a budget log
   (e.g. the unsafe baseline) produce no ratio file.

On the paper's hardware, with the controller in the same region as the
machines, everything except the client's run takes about 20 s per run (more
with a remote controller, and more for large elements, whose storage takes
longer to initialize). The run-time estimates in each experiment's README are
the client's run time plus this fixed cost.

A failed repetition is recorded and the sweep continues; `run.sh` exits
non-zero at the end if any repetition failed. Every run is listed in
`results/runs.log` (time, label, repetition, status, duration, arguments) and
has its own log in `results/logs/` (command, Ansible or run-local output, and
the server/proxy/client output).

The binaries run with their working directory set to a per-run directory
(`<cloak_dir>/run` on the machines, a temporary directory locally), so the
configs use bare file names (`stats.csv`, `budget_log.txt`, ...). The
addresses in the configs are placeholders: the harness passes the real ones
(`private_ip`, or `ansible_host`, from the inventory) as
`--common-server-addr` / `--common-proxy-addr`. Sweep parameters are passed as
command-line overrides of the config values (e.g. `--proxy-cache-size 4000`),
which every binary accepts for every config key.

## Harness variables

Set them on the command line, e.g. `SMOKE=1 ONLY='^s1\.0$' ./run.sh`. Boolean
variables accept `1`, `true` or `yes`.

| Variable | Default | Meaning |
|---|---|---|
| `REPS` | 5 (1 with `SMOKE=1`) | repetitions |
| `SMOKE` | 0 | `1`: quick end-to-end check; workloads cut to `SMOKE_REQUESTS` requests, results in `results-smoke/` |
| `SMOKE_REQUESTS` | 200000 | requests per run with `SMOKE=1` |
| `ONLY` | (all) | extended regex; run only the labels it matches, e.g. `'^batch-(2000\|4000)$'` |
| `LOCAL` | 0 | `1`: run on this machine with `scripts/run-local.sh` (needs `cargo build --release`) |
| `DRY_RUN` | 0 | `1`: print the commands, run nothing |
| `RESULTS_DIR` | `<experiment>/results` | where result files and logs go |
| `INVENTORY` | `ansible/inventory` | Ansible inventory of the three machines |
| `VERBOSE` | 0 | `1`: show the full output of every run (otherwise only in `results/logs/`) |
| `CLOAK_RUN_TIMEOUT` | derived | max seconds a client may run; on the cluster the default is max(1800, 10 x requests / rate), locally 3600 |
| `CLOAK_SERVER_PORT`, `CLOAK_PROXY_PORT` | 4000, 5050 | TCP ports of server and proxy |
| `ANSIBLE_ARGS` | | extra `ansible-playbook` arguments, e.g. `-v` |

Only one sweep at a time can use a cluster (or the local ports): the harness
takes a lock in `.cache/locks/`. If you interrupt a sweep, the next run stops
any leftover processes first; `cd ansible && ansible-playbook stop.yml` stops
them right away.

## Result files

- `<label>-stats.csv`: `request_count,throughput,avg_latency,avg_latency_write,avg_latency_read,std_latency,p99_latency`,
  written by the client. The client is open-loop: it sends requests at a
  fixed rate (`--client-req-per-sec`) over one connection and records each
  request's latency from send to response, at millisecond resolution.
  `request_count` is the number of responses, `throughput` the responses per
  second over the run, the latencies are in ms: the mean over all requests,
  writes and reads, and the standard deviation and nearest-rank 99th
  percentile over the individual query latencies within the run. Writes are
  acknowledged by the proxy at once, so their latency is close to zero unless
  the proxy applies back-pressure.
- `<label>-ratio.csv`: `batch_count,mean_batch_util,median_batch_util,calculated_batch_size`,
  written by `budget_ratio` from the proxy's budget log. Batch utilization
  is the fraction of each batch's slots used by real client requests rather
  than dummy accesses; `calculated_batch_size` is the batch size, i.e. the
  sum of the budgets.

## Plotting

From the repository root:

```bash
uv run python plots/fig3_ablations.py               # Figure 3 from experiments/*/results/
uv run python plots/fig3_ablations.py --paper       # ... from paper-results/
uv run python plots/fig3_ablations.py --smoke       # ... from results-smoke/
uv run python plots/fig3_ablations.py --results-root DIR   # ... from DIR/<experiment>/
uv run python plots/table2_datasets.py [--paper | --smoke | --results-dir DIR]
uv run python plots/fig2_fig4_temporal_locality.py [--paper | --smoke | --results-dir DIR]
```

`--results-root DIR` is for Figure 3 sweeps run with
`RESULTS_DIR=DIR/<experiment>` (e.g. `DIR/zipf-exponent`); `--results-dir`
names the single folder of Table 2 or Figures 2/4. Output goes to
`plots/out/` (`--out-dir` changes it). Every plotted value is the mean over
the rows (repetitions) of one file. In Figure 3 the error bars are the
within-run standard deviation and the dashed line the p99 latency, both
averaged over the repetitions; the axis limits are the paper's whenever all
points fit in them. Columns or panels without results are left empty.
