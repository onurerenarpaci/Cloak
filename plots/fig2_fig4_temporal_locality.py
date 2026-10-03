"""Figures 2 and 4: temporal-locality (reuse-interval) histograms.

Reads the histograms written by experiments/temporal-locality/run.sh
(`results/<dataset>.txt`, default; `results-smoke/` with --smoke) or the
ones used in the paper (`paper-results/<dataset>.txt.xz`, with --paper).
Each file holds one count per line; line x is the number of access pairs to
the same key separated by exactly x intervening accesses (see
experiments/temporal-locality/README.md).

  Figure 2  Ethereum, with the Zipf reference s = 1.1 used in the paper
            (anchored at the x = 0 count). --fit-ethereum uses the fitted
            exponent instead, as for the Figure 4 panels.
  Figure 4  Netflix, Twitter cluster52, Meta KV, Wikipedia text, MSR prxy,
            Alibaba dev 38, each with a least-squares Zipf fit on
            logarithmically binned means of the first 100,000 bins.

Output: plots/out/fig2_temporal_locality.{pdf,png} and
plots/out/fig4_temporal_locality.{pdf,png} (+ .pgf with --pgf). The fitted
exponents are printed to stdout.

    uv run python plots/fig2_fig4_temporal_locality.py            # your results
    uv run python plots/fig2_fig4_temporal_locality.py --paper    # paper data
    uv run python plots/fig2_fig4_temporal_locality.py --smoke    # SMOKE=1 run
"""

import argparse
import lzma
from pathlib import Path

import numpy as np

from plot_common import add_common_args, args_results_dir, save, setup_style

import matplotlib.pyplot as plt  # noqa: E402  (backend chosen by plot_common)

EXPERIMENT = "temporal-locality"
N_BINS = 100_000          # bins plotted and used for the fit (x = 0 .. 99,999)
ETHEREUM_S = 1.1          # Zipf reference exponent drawn in Figure 2

FIG4_PANELS = [
    ("netflix", "Netflix"),
    ("twitter52", "Twitter cluster52"),
    ("metakv", "Meta KV"),
    ("wikit", "Wikipedia text"),
    ("msr-prxy", "MSR prxy"),
    ("alibaba-dev38", "Alibaba dev 38"),
]
XLABEL = "Time Passed Since Last Access (log scale)"
YLABEL = "Frequency (log scale)"


def load_histogram(folder: Path, name: str, n_bins: int = N_BINS):
    """First `n_bins` bins of `<folder>/<name>.txt[.xz]` (zero-padded), or None."""
    for path, opener in ((folder / f"{name}.txt", open),
                         (folder / f"{name}.txt.xz", lzma.open)):
        if path.exists():
            with opener(path, "rt") as f:
                data = [int(line) for _, line in zip(range(n_bins), f)]
            if len(data) < n_bins:  # trailing all-zero bins are not stored
                data += [0] * (n_bins - len(data))
            return np.asarray(data, dtype=float)
    return None


def fit_zipf(counts):
    """Least-squares fit of counts ~ C / g**s on log-binned means.

    g = bin index + 1 is the reuse gap (x intervening accesses -> g = x + 1).
    A raw log-log least squares would be dominated by the many noisy 0/1-count
    bins of the tail; averaging over 64 logarithmically spaced intervals
    weights every decade of gaps equally. Returns (s, C).
    """
    edges = np.unique(np.geomspace(1, len(counts) + 1, 65).astype(int))
    centers, means = [], []
    for lo, hi in zip(edges[:-1], edges[1:]):
        m = counts[lo - 1:hi - 1].mean() if hi > lo else 0.0
        if m > 0:
            centers.append(np.sqrt(lo * (hi - 1)))
            means.append(m)
    slope, intercept = np.polyfit(np.log(centers), np.log(means), 1)
    return -slope, float(np.exp(intercept))


def draw_panel(ax, counts, s, scale, s_label):
    """Empirical histogram (solid) and Zipf reference scale / g**s (dashed).

    The histogram is drawn against its bin index x, so the x = 0 bin falls off
    the log axis, exactly as in the paper figures.
    """
    gaps = np.arange(1, len(counts) + 1)
    ax.loglog(counts, label="Temporal Histogram")
    ax.loglog(gaps, scale / gaps ** s, label=f"Zipf Function s={s_label}",
              linestyle="--")
    ax.grid(True)
    ax.legend(loc="upper right")


def figure2(folder, args):
    counts = load_histogram(folder, "ethereum")
    if counts is None:
        print(f"skipping Figure 2: no ethereum histogram in {folder}")
        return
    s_fit, c_fit = fit_zipf(counts)
    print(f"Figure 2  {'ethereum':14s} fitted s = {s_fit:.2f} "
          f"(paper draws s = {ETHEREUM_S})")
    if args.fit_ethereum:
        s, scale, s_label = s_fit, c_fit, f"{s_fit:.2f}"
    else:
        s, scale, s_label = ETHEREUM_S, counts[0], f"{ETHEREUM_S}"
    fig = plt.figure(figsize=(3.5, 3))
    draw_panel(fig.gca(), counts, s, scale, s_label)
    plt.xlabel(XLABEL)
    plt.ylabel(YLABEL)
    save("fig2_temporal_locality", out_dir=args.out_dir, pgf=args.pgf, fig=fig)
    plt.close(fig)


def figure4(folder, args):
    fig, axes = plt.subplots(2, 3, figsize=(10.5, 6.4), constrained_layout=True)
    found = 0
    for ax, (name, title) in zip(axes.flat, FIG4_PANELS):
        ax.set_title(title)
        counts = load_histogram(folder, name)
        if counts is None:
            print(f"Figure 4  {name:14s} missing in {folder}")
            ax.text(0.5, 0.5, "not computed", ha="center", va="center",
                    transform=ax.transAxes)
            ax.set_xticks([])
            ax.set_yticks([])
            continue
        found += 1
        s, scale = fit_zipf(counts)
        print(f"Figure 4  {name:14s} fitted s = {s:.2f}")
        draw_panel(ax, counts, s, scale, f"{s:.2f}")
    if not found:
        print(f"skipping Figure 4: no histograms in {folder}")
        plt.close(fig)
        return
    fig.supxlabel(XLABEL)
    fig.supylabel(YLABEL)
    save("fig4_temporal_locality", out_dir=args.out_dir, pgf=args.pgf, fig=fig)
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    add_common_args(parser)
    parser.add_argument("--results-dir", type=Path, default=None,
                        help="read histograms from this directory instead of "
                             "experiments/temporal-locality/results "
                             "(or paper-results / results-smoke)")
    parser.add_argument("--fit-ethereum", action="store_true",
                        help="Figure 2: draw the fitted Zipf exponent instead of "
                             f"the paper's s = {ETHEREUM_S}")
    args = parser.parse_args()

    setup_style(usetex=args.usetex or args.pgf, font_size=10)
    # matplotlib's default 10 pt everywhere, as in the paper's versions of these figures
    plt.rcParams.update({"xtick.labelsize": 10, "ytick.labelsize": 10})
    folder = args.results_dir or args_results_dir(EXPERIMENT, args)
    print(f"reading histograms from {folder}")
    figure2(folder, args)
    figure4(folder, args)


if __name__ == "__main__":
    main()
