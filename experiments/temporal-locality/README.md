# Temporal locality of the real-world traces (Figures 2 and 4)

Figure 2 (Section 6) and Figure 4 (Appendix A) show how temporally skewed the
workloads are. Each panel is a log-log histogram of reuse intervals, drawn
next to a Zipf reference curve: Ethereum is in Figure 2, and the six other
real-world traces (Netflix, Twitter cluster52, Meta KV, Wikipedia text, MSR
prxy, Alibaba dev 38) are in Figure 4. This is offline trace analysis. It runs
on one machine in about a minute and needs neither a cluster nor the Cloak
binaries.

## Histogram definition

The input is a workload file with one request per line (`key` or
`key value`). Only the key is used, so reads and writes count the same. For
each request whose key appeared before, let x be the number of requests
between it and the previous request for that key. Bin x of the histogram
counts these consecutive access pairs, summed over all keys. At x = 10, for
example, the bin holds the number of times a key was accessed and then
accessed again after exactly 10 requests for other keys. A key requested twice
in a row lands in bin 0.

This is the plain position distance, i.e. position difference minus one. It
is not an LRU stack distance: every intervening request counts, including
repeated requests for the same other key. `temporal_histogram.py` computes the
histogram in a single pass with a hash map from each key to its last
position, in O(n) time and O(distinct keys) memory.

On the plots, the x axis ("Time Passed Since Last Access") is the bin index x,
and the y axis ("Frequency") is the bin count. Bin x = 0 cannot be shown on a
log axis, so the solid curve starts at x = 1. The paper figures do the same.
Both figures show the first 100,000 bins.

## Zipf reference curves

**Figure 4.** Each panel's dashed curve is C / g^s over the reuse gap
g = x + 1, with s and C from a least-squares line through log(count) versus
log(g) over the first 100,000 bins. Before the fit, the counts are averaged
within 64 logarithmically spaced intervals of g, so that every decade of gaps
has equal weight and the tens of thousands of noisy 0/1-count bins in the
tail do not dominate. `fit_zipf` in
[`plots/fig2_fig4_temporal_locality.py`](../../plots/fig2_fig4_temporal_locality.py)
has the exact procedure.

**Figure 2.** The Ethereum panel does not use the fitted curve. It draws a
fixed exponent s = 1.1, scaled so that the curve equals the bin-0 count at
x = 1. Applying the Figure 4 fit to Ethereum gives s = 1.06. The plot script
prints this value, and `--fit-ethereum` draws the fitted curve in Figure 2
instead.

## Inputs, fitted exponents, and reproducibility

The inputs are the datasets experiment's workloads in
`../datasets/workloads/<dataset>.xz`, the same files Table 2 replays (sources
and sampling: [`../datasets/prep/README.md`](../datasets/prep/README.md)).
`msr-prxy` is not shipped, because the MSR Cambridge traces are not
redistributed; regenerate it with `../datasets/prep/make_workloads.sh msr-prxy`
([details](../datasets/prep/README.md#msr-prxy)). Until then `run.sh` skips
it with a warning and its Figure 4 panel is marked "not computed". The
paper's histogram, `paper-results/msr-prxy.txt.xz`, is included.

| Panel | Dataset | Trace used for the paper figure | Requests | Distinct keys | s (paper) | Recomputed by `run.sh` |
|---|---|---|---:|---:|---:|---|
| Fig. 2 | `ethereum` | shipped workload (full) | 17,427,629 | 701,313 | 1.1 (fixed; fit gives 1.06) | identical histogram |
| Fig. 4 | `netflix` | shipped workload (full) | 671,736 | 8,472 | 1.13 | identical histogram |
| Fig. 4 | `twitter52` | shipped workload (full) | 10,000,000 | 772,345 | 1.06 | identical histogram |
| Fig. 4 | `metakv` | shipped workload (full) | 7,965,502 | 988,762 | 0.30 | identical histogram |
| Fig. 4 | `wikit` | shipped workload (full) | 10,000,000 | 950,243 | 0.73 | identical histogram |
| Fig. 4 | `msr-prxy` | **full one-week sample, not shipped** | 185,262,952 | 1,000,000 | 1.10 | same shape, s = 1.17 |
| Fig. 4 | `alibaba-dev38` | **full one-week sample, not shipped** | 275,089,919 | 1,000,000 | 0.83 | same shape, s = 0.81 |

The five panels computed from shipped workloads reproduce the paper's
histogram files byte for byte, and rendering `paper-results/` with `--pgf`
reproduces the paper's figures exactly.

**MSR prxy and Alibaba dev 38.** These two panels were computed from the
complete one-week, hash-sampled block traces: every request to a sampled set
of 1,000,000 pages over the whole 7-day window. Those files are several GB
each and are not in this repository; `paper-results/` has their week-long
histograms. Table 2 replays the first 10,000,000 requests of the same sampled
traces (shipped for Alibaba dev 38, regenerated for MSR prxy), which cover
about 5% (MSR prxy) and 4% (Alibaba dev 38) of the week and touch 63,296 and
246,600 distinct pages. `run.sh` recomputes the histograms from these
prefixes. Their shape is qualitatively the same: MSR prxy keeps the peak at
x ≈ 4, the secondary bump near x ≈ 30, the flat middle and the spike at
x ≈ 2×10⁴ (narrower in the prefix); Alibaba dev 38 keeps the steep head, the
trough near x ≈ 3×10³ and the periodic hump at 10⁴ with the spike at
about 2×10⁴. Counts are about 18 (MSR prxy) and 28 (Alibaba dev 38) times
lower, in proportion to the request counts, and the tails above x ≈ 10³ are
noisier. The fitted exponents are s = 1.17 instead of 1.10 and s = 0.81
instead of 0.83.

## How to reproduce

From this directory:

```bash
./run.sh                      # all seven histograms (six without msr-prxy) -> results/, figures -> ../../plots/out/
SMOKE=1 ./run.sh              # first 200,000 requests per trace -> results-smoke/ (a few seconds)
ONLY='msr|alibaba' ./run.sh   # a subset; panels missing from results/ are marked "not computed"
DRY_RUN=1 ./run.sh            # print the commands only
```

`run.sh` also reads `RESULTS_DIR` (default `results/`, or `results-smoke/`
with `SMOKE=1`), `WORKLOADS_DIR` (default `../datasets/workloads`), `MAX_BINS`
(default 1,000,000), `SMOKE_REQUESTS` (default 200,000), `PLOT=0` (skip the
figures), `OUT_DIR` (figure directory) and `PYTHON` (interpreter command,
default `uv run --project <repo> python`, or `python3` without uv). `SMOKE`,
`DRY_RUN` and `PLOT` accept `1`/`true`/`yes` and `0`/`false`/`no`, as in the
harness of the cluster experiments.

To plot only, run these from the repository root:

```bash
uv run python plots/fig2_fig4_temporal_locality.py            # from experiments/temporal-locality/results
uv run python plots/fig2_fig4_temporal_locality.py --paper    # from paper-results (the paper's data)
uv run python plots/fig2_fig4_temporal_locality.py --smoke    # from results-smoke
uv run python plots/fig2_fig4_temporal_locality.py --paper --pgf   # also .pgf; needs LaTeX (implies --usetex)
```

The output files are `plots/out/fig2_temporal_locality.{pdf,png}` and
`plots/out/fig4_temporal_locality.{pdf,png}`; the script also prints the
fitted exponent of each panel. Without `--usetex`, text is rendered with
matplotlib's own fonts, so the layout differs slightly from the paper while
the data and curves stay the same.

The histogram tool works on any workload file (plain or `.xz`) and needs only
the Python standard library; the plot script needs numpy and matplotlib, which
are in the repository's `pyproject.toml`:

```bash
mkdir -p results
python3 temporal_histogram.py ../datasets/workloads/netflix.xz -o results/netflix.txt [--limit N] [--max-bins N]
```

**Runtime and memory** (a laptop, one core): the histograms take about 0.5 µs
per request including xz decompression, i.e. 4–7 s for each 10M-request
trace and about 10 s for Ethereum (17.4M requests); the whole `run.sh` takes
about 40 s including the plots. Peak memory stays under 200 MB per trace,
dominated by the key map (≈ 150 bytes per distinct key); rendering takes
about 3 s and up to about 400 MB. A week-long block trace like the unshipped
MSR/Alibaba inputs (2–3×10⁸ requests, 10⁶ keys) would take a few minutes and
well under 1 GB.

## File formats

`results/<dataset>.txt` (written by `run.sh`) and `paper-results/<dataset>.txt.xz`
(xz-compressed) use the same format: one non-negative integer per line, and
line x (0-based) is the number of access pairs with exactly x intervening
accesses. Only bins x < 1,000,000 are stored. Pairs farther apart are counted
in the tool's summary line but not written, and trailing zero bins are
dropped. The figures read only the first 100,000 lines. The paper's files
hold 1,000,000 bins for `ethereum` (its full histogram reaches x ≈ 1.7×10⁷;
the 702,570 pairs with x ≥ 10⁶ are omitted), `twitter52` and
`alibaba-dev38`; 657,544 for `netflix` (complete); 999,991 for `metakv`;
999,997 for `wikit`; and 999,974 for `msr-prxy`.

Compare a rerun with the paper data:
`xz -dc paper-results/netflix.txt.xz | cmp - results/netflix.txt`.
