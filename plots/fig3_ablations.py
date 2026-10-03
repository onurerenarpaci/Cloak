"""Figure 3: ablation grid (Zipf exponent, batch size, cache size, element size).

Each column is one sweep (experiments/zipf-exponent, batch-size, cache-size,
element-size); the rows show throughput, mean latency (error bars: within-run
standard deviation averaged across repetitions) with p99 latency (dashed,
averaged across repetitions), and mean batch utilization.

    uv run python plots/fig3_ablations.py            # your runs (results/)
    uv run python plots/fig3_ablations.py --paper    # the paper's data
    uv run python plots/fig3_ablations.py --smoke    # SMOKE=1 runs
    uv run python plots/fig3_ablations.py --results-root DIR
        # sweeps run with RESULTS_DIR=DIR/<experiment>, e.g. DIR/zipf-exponent

Writes plots/out/fig3_ablations.{pdf,png} (and .pgf with --pgf). Columns whose
experiment has no results yet are left empty. Axis limits are the paper's
whenever all points fit in them, and are widened otherwise.
"""

import argparse
from pathlib import Path

import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import numpy as np
import tol_colors as tc

from plot_common import (add_common_args, args_results_dir, format_percent,
                         format_thousands, load_sweep, plot_latency_panel, save,
                         setup_style, tex)

parser = add_common_args(argparse.ArgumentParser(description=__doc__.split("\n")[0]))
parser.add_argument(
    "--results-root", type=Path, default=None, metavar="DIR",
    help="read each column from DIR/<experiment>/ (zipf-exponent, batch-size, "
         "cache-size, element-size) instead of experiments/<experiment>/results "
         "(overrides --paper/--smoke); for sweeps run with RESULTS_DIR=DIR/<experiment>")
args = parser.parse_args()
setup_style(usetex=args.usetex or args.pgf, font_size=8)


def load(experiment, pattern, key):
    if args.results_root is not None:
        folder = args.results_root / experiment
    else:
        folder = args_results_dir(experiment, args)
    return load_sweep(folder, pattern, key, min_reps=1, missing_ok=True)


def fit_ylim(ax, values, lo, hi, log=False):
    """Set the paper's y limits (lo, hi) if every value fits, else widen them.

    Returns True if the paper's limits were kept.
    """
    v = values.dropna()
    if v.empty or (v.min() >= lo and v.max() <= hi):
        ax.set_ylim(lo, hi)
        return True
    if log:
        ax.set_ylim(min(lo, v.min() / 1.5), max(hi, v.max() * 1.5))
    else:
        pad = 0.05 * (max(hi, v.max()) - min(lo, v.min()))
        new_lo = lo if v.min() >= lo else max(0, v.min() - pad)
        new_hi = hi if v.max() <= hi else v.max() + pad
        ax.set_ylim(new_lo, new_hi)
    return False


df_zipf = load("zipf-exponent", r"s(\d+(?:\.\d+)?)", float)
df_batch = load("batch-size", r"batch-(\d+)", int)
df_cache = load("cache-size", r"cache-(\d+)", int)
df_elem = load("element-size", r"element-(\d+)", int)
if all(df is None for df in (df_zipf, df_batch, df_cache, df_elem)):
    raise SystemExit("error: no results found; run an experiment first, or use --paper")

LEGEND_LABELS = ["Throughput (ops/s)", "Mean Latency (ms)", "p99 Latency (ms)",
                 "Batch Utilization"]

fig = plt.figure(figsize=(8.3, 2.9))
gs = fig.add_gridspec(3, 4, hspace=0.14, wspace=0.42)


def tput_panel(ax, df):
    (line,) = ax.plot(df.index.tolist(), df["throughput"], color=tc.bright.blue,
                      label="Throughput (ops/s)", linestyle="-", marker="o",
                      markersize=3, markerfacecolor=tc.bright.cyan)
    ax.grid(True)
    return line


def lat_panel(ax, df):
    plot_latency_panel(ax, df, legend=False)


def util_panel(ax, df):
    ax.plot(df.index.tolist(), df["mean_batch_util"], color=tc.bright.green,
            label="Batch Utilization", linestyle="-", marker="o", markersize=3,
            markerfacecolor=tc.bright.yellow)
    ax.yaxis.set_major_formatter(ticker.FuncFormatter(format_percent))
    ax.grid(True)


def column(col, df, xlabel, experiment):
    """Create the three axes of a column; returns None if there is no data."""
    ax = {name: fig.add_subplot(gs[row, col])
          for row, name in enumerate(("tput", "lat", "util"))}
    ax["tput"].xaxis.set_ticklabels([])
    ax["lat"].xaxis.set_ticklabels([])
    ax["util"].set_xlabel(xlabel)
    if df is None:
        for a in ax.values():
            a.set_xticks([])
            a.set_yticks([])
        ax["lat"].text(0.5, 0.5, f"no results\n({experiment})", ha="center",
                       va="center", transform=ax["lat"].transAxes)
        return None
    tput_panel(ax["tput"], df)
    lat_panel(ax["lat"], df)
    util_panel(ax["util"], df)
    return ax


# --- column 0: zipf exponent -------------------------------------------------
ax = column(0, df_zipf, "Zipf Exponent: $s$", "zipf-exponent")
if ax:
    ax["tput"].yaxis.set_major_formatter(ticker.FuncFormatter(format_thousands))
    ax["tput"].set_yticks(np.arange(0, df_zipf["throughput"].max() + 50000, 60000))
    ax["tput"].yaxis.set_minor_locator(ticker.AutoMinorLocator())
    fit_ylim(ax["tput"], df_zipf["throughput"], 0, 200000)
    ax["util"].set_yticks(np.arange(0, df_zipf["mean_batch_util"].max() + 0.20, 0.25))
    ax["util"].yaxis.set_minor_locator(ticker.AutoMinorLocator())
    fit_ylim(ax["util"], df_zipf["mean_batch_util"], 0, 0.85)
    zipf_ticks = np.arange(df_zipf.index.min(), df_zipf.index.max() + 0.1, 0.5)
    for a in ax.values():
        a.set_xticks(zipf_ticks)
    ax["tput"].xaxis.set_ticklabels([])
    ax["lat"].xaxis.set_ticklabels([])

# --- column 1: batch size ----------------------------------------------------
ax = column(1, df_batch, "Batch Size", "batch-size")
if ax:
    ax["tput"].yaxis.set_major_formatter(ticker.FuncFormatter(format_thousands))
    ax["tput"].set_yticks(np.arange(0, df_batch["throughput"].max() + 20000, 50000))
    ax["tput"].yaxis.set_minor_locator(ticker.AutoMinorLocator())
    fit_ylim(ax["tput"], df_batch["throughput"], 0, 165000)
    ax["util"].set_yticks(np.arange(0, df_batch["mean_batch_util"].max() + 0.2, 0.25))
    ax["util"].yaxis.set_minor_locator(ticker.AutoMinorLocator())
    fit_ylim(ax["util"], df_batch["mean_batch_util"], 0, 0.85)
    for a in ax.values():
        a.set_xticks(np.arange(2000, df_batch.index.max() + 1, 3000))
        a.xaxis.set_major_formatter(ticker.FuncFormatter(format_thousands))
    ax["tput"].xaxis.set_ticklabels([])
    ax["lat"].xaxis.set_ticklabels([])

# --- column 2: cache size ----------------------------------------------------
# The sweep runs at a fixed request rate, so throughput is a flat line; the
# panel is kept so that all four columns share the same 3-panel structure.
ax = column(2, df_cache, tex("Cache Size (# of Elements)"), "cache-size")
if ax:
    ax["tput"].yaxis.set_major_formatter(ticker.FuncFormatter(format_thousands))
    ax["tput"].set_yticks(np.arange(0, df_cache["throughput"].max() * 1.5, 50000))
    ax["tput"].yaxis.set_minor_locator(ticker.AutoMinorLocator())
    ax["tput"].set_ylim(0, df_cache["throughput"].max() * 1.5)
    for a in ax.values():
        a.set_xscale("log")
    if fit_ylim(ax["util"], df_cache["mean_batch_util"], 0.45, 0.67):
        ax["util"].set_yticks(np.arange(0.45, df_cache["mean_batch_util"].max() + 0.1, 0.05))
        ax["util"].set_ylim(0.45, 0.67)
    else:
        ax["util"].yaxis.set_major_locator(ticker.MaxNLocator(nbins=5))
    ax["util"].yaxis.set_minor_locator(ticker.AutoMinorLocator())
    ax["tput"].xaxis.set_ticklabels([])
    ax["lat"].xaxis.set_ticklabels([])
    # errorbar artists inflate log-x autoscale limits; keep the panels aligned
    ax["tput"].set_xlim(ax["util"].get_xlim())
    ax["lat"].set_xlim(ax["util"].get_xlim())

# --- column 3: element size --------------------------------------------------
ax = column(3, df_elem, "Element Size (KB)", "element-size")
if ax:
    ax["tput"].set_yscale("log")
    fit_ylim(ax["tput"], df_elem["throughput"], 500, 300000, log=True)
    ax["util"].set_yticks(np.arange(0, df_elem["mean_batch_util"].max() + 0.2, 0.25))
    ax["util"].yaxis.set_minor_locator(ticker.AutoMinorLocator())
    fit_ylim(ax["util"], df_elem["mean_batch_util"], 0, 0.85)
    for a in ax.values():
        a.set_xscale("log")
        a.set_xticks(df_elem.index.tolist())
    ax["util"].xaxis.set_major_formatter(
        ticker.FuncFormatter(lambda v, _: f"{v / 1024:g}"))
    ax["util"].xaxis.set_minor_formatter(ticker.NullFormatter())
    for a in (ax["tput"], ax["lat"]):
        a.xaxis.set_ticklabels([])
        a.xaxis.set_minor_formatter(ticker.NullFormatter())
    # errorbar artists inflate log-x autoscale limits; keep the panels aligned
    ax["lat"].set_xlim(ax["util"].get_xlim())

# --- shared legend below the grid --------------------------------------------
# take the artists from the first column that has data (so the legend shows the
# error-bar marker exactly as plotted); fall back to plain proxies
handles = None
for col in range(4):
    axes = [fig.axes[3 * col + row] for row in range(3)]
    found = [a.get_legend_handles_labels() for a in axes]
    by_label = {lab: h for hs, labs in found for h, lab in zip(hs, labs)}
    if len(by_label) == 4:
        handles = [by_label[lab] for lab in LEGEND_LABELS]
        break
if handles is None:
    style = dict(marker="o", markersize=3)
    handles = [
        plt.Line2D([], [], color=tc.bright.blue, markerfacecolor=tc.bright.cyan, **style),
        plt.Line2D([], [], color=tc.bright.red, markerfacecolor=tc.bright.purple, **style),
        plt.Line2D([], [], color=tc.bright.purple, markerfacecolor=tc.bright.red,
                   linestyle="--", marker="s", markersize=3),
        plt.Line2D([], [], color=tc.bright.green, markerfacecolor=tc.bright.yellow, **style),
    ]
fig.legend(handles, LEGEND_LABELS, loc="upper center", bbox_to_anchor=(0.5, -0.03),
           ncol=4, columnspacing=1.2, handletextpad=0.5)

save("fig3_ablations", out_dir=args.out_dir, pgf=args.pgf, fig=fig)
