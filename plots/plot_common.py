"""Shared helpers for the Cloak plot scripts.

Result layout (written by experiments/<name>/run.sh, see experiments/README.md):

    experiments/<name>/results/<label>-stats.csv        one row per repetition
    experiments/<name>/results/<label>-ratio.csv        one row per repetition
    experiments/<name>/paper-results/<label>-*.csv      the rows used in the paper

`load_sweep` averages the repetition rows of every label and joins the stats
and ratio columns into one DataFrame indexed by the sweep parameter.

Output: every figure is written as PDF + PNG to plots/out/ by default; a PGF
copy (for inclusion in LaTeX) is written only with --pgf and needs a local
LaTeX installation, as does --usetex.
"""

import argparse
import re
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import matplotlib.pyplot as plt  # noqa: E402
import matplotlib.ticker as ticker  # noqa: E402
import pandas as pd  # noqa: E402
import tol_colors as tc  # noqa: E402

PLOTS_DIR = Path(__file__).resolve().parent
CLOAK_ROOT = PLOTS_DIR.parent
EXPERIMENTS_DIR = CLOAK_ROOT / "experiments"
DEFAULT_OUT_DIR = PLOTS_DIR / "out"

STATS_COLUMNS = ["request_count", "throughput", "avg_latency", "avg_latency_write",
                 "avg_latency_read", "std_latency", "p99_latency"]
RATIO_COLUMNS = ["batch_count", "mean_batch_util", "median_batch_util",
                 "calculated_batch_size"]


# --------------------------------------------------------------------------
# command line / style
# --------------------------------------------------------------------------

def add_common_args(parser: argparse.ArgumentParser) -> argparse.ArgumentParser:
    """Options shared by all plot scripts."""
    parser.add_argument(
        "--paper", action="store_true",
        help="plot the results shipped with the paper (experiments/*/paper-results) "
             "instead of your own runs (experiments/*/results)")
    parser.add_argument(
        "--smoke", action="store_true",
        help="plot the output of SMOKE=1 runs (experiments/*/results-smoke)")
    parser.add_argument(
        "--out-dir", type=Path, default=DEFAULT_OUT_DIR,
        help=f"output directory (default: {DEFAULT_OUT_DIR.relative_to(CLOAK_ROOT)})")
    parser.add_argument(
        "--pgf", action="store_true",
        help="additionally write a .pgf file (requires LaTeX; implies --usetex)")
    parser.add_argument(
        "--usetex", action="store_true",
        help="render all text with LaTeX, as in the paper (requires LaTeX)")
    return parser


def setup_style(usetex: bool = False, font_size: float = 8) -> None:
    """Paper-like serif style. With usetex=False no LaTeX installation is needed."""
    plt.rcParams.update({
        "font.family": "serif",
        "font.size": font_size,
        "axes.labelsize": font_size,
        "xtick.labelsize": font_size - 1,
        "ytick.labelsize": font_size - 1,
        "legend.fontsize": font_size,
        "text.usetex": usetex,
        "mathtext.fontset": "cm",
        "pgf.texsystem": "pdflatex",
        "pgf.rcfonts": False,
        "pgf.preamble": r"\usepackage{amsmath}",
    })
    if not usetex:
        plt.rcParams["font.serif"] = ["CMU Serif", "Computer Modern Roman",
                                      "DejaVu Serif", "Times New Roman", "serif"]


def setup_pgf() -> None:
    """LaTeX text rendering, as used for the paper figures."""
    setup_style(usetex=True)


def usetex() -> bool:
    return bool(plt.rcParams.get("text.usetex", False))


def tex(text: str) -> str:
    """Escape LaTeX special characters (# % &) only when usetex is active."""
    if not usetex():
        return text
    return re.sub(r"(?<!\\)([#%&])", r"\\\1", text)


def save(name: str, out_dir=None, pgf: bool = False, fig=None, dpi: int = 200) -> list:
    """Write `<out_dir>/<name>.pdf` and `.png` (+ `.pgf` if requested).

    `name` may carry an extension, which is ignored. Returns the written paths.
    """
    fig = fig or plt.gcf()
    out_dir = Path(out_dir) if out_dir is not None else DEFAULT_OUT_DIR
    out_dir.mkdir(parents=True, exist_ok=True)
    stem = Path(name).stem if Path(name).suffix in (".pgf", ".pdf", ".png") else name
    written = []
    for ext in ("pdf", "png") + (("pgf",) if pgf else ()):
        path = out_dir / f"{stem}.{ext}"
        fig.savefig(path, bbox_inches="tight", **({"dpi": dpi} if ext == "png" else {}))
        written.append(path)
        print(f"wrote {path}")
    return written


# --------------------------------------------------------------------------
# result loading
# --------------------------------------------------------------------------

def results_dir(experiment: str, paper: bool = False, smoke: bool = False) -> Path:
    """`experiments/<experiment>/results`, `.../paper-results` or `.../results-smoke`."""
    sub = "paper-results" if paper else ("results-smoke" if smoke else "results")
    return EXPERIMENTS_DIR / experiment / sub


def args_results_dir(experiment: str, args) -> Path:
    """Result folder selected by the --paper / --smoke options."""
    return results_dir(experiment, paper=getattr(args, "paper", False),
                       smoke=getattr(args, "smoke", False))


def read_runs(path) -> pd.DataFrame:
    """Read one `<label>-stats.csv` / `<label>-ratio.csv` file (rows = repetitions)."""
    return pd.read_csv(path)


def load_sweep(folder, label_pattern: str, key=float, min_reps: int = 1,
               missing_ok: bool = False):
    """Average the repetitions of every label in `folder`.

    `label_pattern` is a regex matched against the label (the file name without
    the `-stats.csv` / `-ratio.csv` suffix); its first group, converted with
    `key`, becomes the index. Labels that do not match are ignored. Returns the
    mean stats columns joined with the mean ratio columns (ratio columns are NaN
    for labels without a ratio file) plus a `reps` column (number of stats rows).
    With missing_ok=True, a missing folder or one without matching files gives
    None (and a warning) instead of an error.
    """
    folder = Path(folder)

    def missing(msg):
        if missing_ok:
            print(f"warning: {msg}")
            return None
        raise SystemExit(f"error: {msg}")

    if not folder.is_dir():
        return missing(f"result folder {folder} does not exist "
                       "(run the experiment first, or pass --paper)")
    stats, ratio, reps = {}, {}, {}
    regex = re.compile(label_pattern)
    for path in sorted(folder.glob("*.csv")):
        m = re.fullmatch(r"(.+)-(stats|ratio)\.csv", path.name)
        if not m:
            continue
        label, kind = m.groups()
        lm = regex.fullmatch(label)
        if lm is None:
            continue
        df = read_runs(path)
        if df.empty:
            continue
        idx = key(lm.group(1))
        if kind == "stats":
            stats[idx] = df.mean(numeric_only=True)
            reps[idx] = len(df)
        else:
            ratio[idx] = df.mean(numeric_only=True)
    if not stats:
        return missing(f"no '<label>-stats.csv' files matching "
                       f"{label_pattern!r} in {folder}")
    merged = pd.DataFrame.from_dict(stats, orient="index")
    if ratio:
        merged = merged.join(pd.DataFrame.from_dict(ratio, orient="index"), how="left")
    merged["reps"] = pd.Series(reps)
    few = merged.index[merged["reps"] < min_reps].tolist()
    if few:
        print(f"warning: {folder}: fewer than {min_reps} repetitions for {few}")
    return merged.sort_index()


# --------------------------------------------------------------------------
# tick formatters
# --------------------------------------------------------------------------

def format_thousands(value, tick_number=None):
    return f"{int(value / 1000)}k"


def format_percent(value, tick_number=None):
    return f"{int(round(value * 100))}" + ("\\%" if usetex() else "%")


def format_latency(value, tick_number=None):
    return f"{int(value)}"


def format_kb(value, tick_number=None):
    return f"{value / 1024:g}"


# --------------------------------------------------------------------------
# panels
# --------------------------------------------------------------------------

def plot_latency_panel(ax, df, logscale=False, legend=True):
    """Mean latency with error bars (within-run standard deviation, averaged
    across repetitions) plus a dashed p99-latency line (averaged across
    repetitions)."""
    x = df.index.tolist()
    mean = df["avg_latency"]
    has_std = "std_latency" in df.columns and df["std_latency"].notna().any()
    has_p99 = "p99_latency" in df.columns and df["p99_latency"].notna().any()

    if has_std:
        ax.errorbar(x, mean, yerr=df["std_latency"], color=tc.bright.red,
                    label="Mean Latency (ms)", linestyle="-", marker="o",
                    markersize=3, markerfacecolor=tc.bright.purple,
                    capsize=2, elinewidth=0.8, capthick=0.8)
    else:
        ax.plot(x, mean, color=tc.bright.red, label="Mean Latency (ms)",
                linestyle="-", marker="o", markersize=3,
                markerfacecolor=tc.bright.purple)

    top = (mean + (df["std_latency"] if has_std else 0)).max()
    if has_p99:
        ax.plot(x, df["p99_latency"], color=tc.bright.purple,
                label="p99 Latency (ms)", linestyle="--", marker="s",
                markersize=3, markerfacecolor=tc.bright.red)
        top = max(top, df["p99_latency"].max())

    ax.yaxis.set_major_formatter(ticker.FuncFormatter(format_latency))
    if logscale:
        ax.set_yscale("log")
    else:
        ax.set_ylim(0, top * 1.15)
        ax.yaxis.set_major_locator(ticker.MaxNLocator(nbins=5))
        ax.yaxis.set_minor_locator(ticker.AutoMinorLocator())
    ax.grid(True)
    if legend:
        ax.legend()
