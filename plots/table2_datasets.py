"""Table 2: throughput and end-to-end latency of Cloak and the unsafe baseline
across the eight datasets.

Reads experiments/datasets/results/<dataset>-{cloak,unsafe}-stats.csv (your
own runs), experiments/datasets/paper-results/ with --paper (the rows used in
the paper), or experiments/datasets/results-smoke/ with --smoke. Every cell is the mean over the repetition rows of one file:
throughput in ops/s, mean and p99 latency in ms (p99 is the within-run 99th
percentile, averaged across repetitions).

Prints a Markdown table to stdout and writes <out-dir>/table2_datasets.csv
(+ table2_datasets.tex with --latex). Needs only pandas.

    uv run python plots/table2_datasets.py --paper
    uv run python plots/table2_datasets.py            # after experiments/datasets/run.sh
"""

import argparse
import sys
import tomllib
from pathlib import Path

import pandas as pd

PLOTS_DIR = Path(__file__).resolve().parent
CLOAK_ROOT = PLOTS_DIR.parent
DATASETS_DIR = CLOAK_ROOT / "experiments" / "datasets"
DEFAULT_OUT_DIR = PLOTS_DIR / "out"

# label -> (name in the paper, citation key in the paper, R/W mix in %)
# The R/W mix is that of the workload file (reads = `key` lines, writes =
# `key value` lines); #Elements is read from the config (storage_size).
DATASETS = {
    "netflix": ("Netflix", "[17]", "100/0"),
    "ethereum": ("Ethereum", "[12]", "100/0"),
    "synthetic": ("Synthetic s = 1", "", "50/50"),
    "twitter52": ("Twitter 52", "[57]", "93/7"),
    "metakv": ("Meta KV", "[6]", "79/21"),
    "wikit": ("WikiT", "[6]", "100/0"),
    "msr-prxy": ("MSR prxy", "[40]", "66/34"),
    "alibaba-dev38": ("Alibaba dev. 38", "[1]", "0/100"),
}
SYSTEMS = ("unsafe", "cloak")
METRICS = {  # output column prefix -> stats CSV column
    "throughput": "throughput",
    "mean_latency_ms": "avg_latency",
    "p99_latency_ms": "p99_latency",
}


def storage_size(dataset: str):
    path = DATASETS_DIR / "configs" / f"{dataset}.toml"
    try:
        with open(path, "rb") as f:
            return tomllib.load(f)["common"]["storage_size"]
    except (OSError, KeyError, tomllib.TOMLDecodeError):
        return None


def load_means(folder: Path, label: str):
    """Mean of the repetition rows of <folder>/<label>-stats.csv, or None."""
    path = folder / f"{label}-stats.csv"
    if not path.is_file():
        return None, 0
    df = pd.read_csv(path)
    if df.empty:
        return None, 0
    return df.mean(numeric_only=True), len(df)


def build_table(folder: Path) -> pd.DataFrame:
    rows, missing = [], []
    for dataset, (name, cite, rw) in DATASETS.items():
        row = {"dataset": dataset, "name": name, "citation": cite,
               "elements": storage_size(dataset), "rw_percent": rw}
        for system in SYSTEMS:
            means, reps = load_means(folder, f"{dataset}-{system}")
            row[f"{system}_reps"] = reps
            if means is None:
                missing.append(f"{dataset}-{system}")
            for col, src in METRICS.items():
                value = None if means is None else means.get(src)
                if value is not None and pd.notna(value):
                    # Same rounding as the paper's table.
                    value = round(value) if col == "throughput" else round(value, 1)
                else:
                    value = None
                row[f"{system}_{col}"] = value
        u, c = row["unsafe_throughput"], row["cloak_throughput"]
        row["cloak_vs_unsafe_throughput"] = round(c / u, 3) if u and c else None
        rows.append(row)
    if missing:
        print(f"note: no results in {folder} for: {', '.join(missing)} (shown as n/a)",
              file=sys.stderr)
    return pd.DataFrame(rows)


def fmt_int(v):
    return "n/a" if v is None or pd.isna(v) else f"{int(v):,}"


def fmt_ms(v):
    return "n/a" if v is None or pd.isna(v) else f"{v:,.1f}"


def fmt_ratio(v):
    return "n/a" if v is None or pd.isna(v) else f"{100 * v:.0f}%"


def markdown(df: pd.DataFrame) -> str:
    head = ["Dataset", "#Elements", "R/W (%)",
            "Tput unsafe", "Tput cloak", "Cloak/unsafe",
            "Mean unsafe", "Mean cloak", "p99 unsafe", "p99 cloak", "Reps (u/c)"]
    lines = ["| " + " | ".join(head) + " |",
             "|" + "|".join(["---"] + ["---:"] * (len(head) - 1)) + "|"]
    for _, r in df.iterrows():
        cells = [r["name"], fmt_int(r["elements"]), r["rw_percent"],
                 fmt_int(r["unsafe_throughput"]), fmt_int(r["cloak_throughput"]),
                 fmt_ratio(r["cloak_vs_unsafe_throughput"]),
                 fmt_ms(r["unsafe_mean_latency_ms"]), fmt_ms(r["cloak_mean_latency_ms"]),
                 fmt_ms(r["unsafe_p99_latency_ms"]), fmt_ms(r["cloak_p99_latency_ms"]),
                 f"{r['unsafe_reps']}/{r['cloak_reps']}"]
        lines.append("| " + " | ".join(str(c) for c in cells) + " |")
    lines.append("")
    lines.append("Throughput in ops/s; mean and p99 latency in ms; "
                 "each cell is the mean over the repetitions.")
    return "\n".join(lines)


def latex(df: pd.DataFrame) -> str:
    def esc(s):
        return str(s).replace("#", r"\#").replace("%", r"\%").replace("&", r"\&")

    lines = [r"\begin{tabular}{lrr rr rr rr}",
             r"\toprule",
             r" & & & \multicolumn{2}{c}{Throughput (ops/s)} & "
             r"\multicolumn{2}{c}{Mean latency (ms)} & "
             r"\multicolumn{2}{c}{p99 latency (ms)} \\",
             r"Dataset & \#Elements & R/W (\%) & unsafe & cloak & unsafe & cloak "
             r"& unsafe & cloak \\",
             r"\midrule"]
    for _, r in df.iterrows():
        name = esc(r["name"]).replace("s = 1", "$s=1$")
        cells = [name, fmt_int(r["elements"]), esc(r["rw_percent"]),
                 fmt_int(r["unsafe_throughput"]), fmt_int(r["cloak_throughput"]),
                 fmt_ms(r["unsafe_mean_latency_ms"]), fmt_ms(r["cloak_mean_latency_ms"]),
                 fmt_ms(r["unsafe_p99_latency_ms"]), fmt_ms(r["cloak_p99_latency_ms"])]
        lines.append(" & ".join(cells) + r" \\")
    lines += [r"\bottomrule", r"\end{tabular}", ""]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--paper", action="store_true",
        help="use the rows shipped with the paper (experiments/datasets/paper-results) "
             "instead of your own runs (experiments/datasets/results)")
    parser.add_argument(
        "--smoke", action="store_true",
        help="use the results of SMOKE=1 runs (experiments/datasets/results-smoke)")
    parser.add_argument(
        "--results-dir", type=Path, default=None,
        help="read <label>-stats.csv files from this folder instead")
    parser.add_argument(
        "--out-dir", type=Path, default=DEFAULT_OUT_DIR,
        help=f"output directory (default: {DEFAULT_OUT_DIR.relative_to(CLOAK_ROOT)})")
    parser.add_argument("--latex", action="store_true",
                        help="also write table2_datasets.tex (LaTeX tabular body)")
    args = parser.parse_args()

    default = "paper-results" if args.paper else "results-smoke" if args.smoke else "results"
    folder = args.results_dir or DATASETS_DIR / default
    if not folder.is_dir():
        raise SystemExit(f"error: result folder {folder} does not exist "
                         "(run experiments/datasets/run.sh first, or pass --paper)")

    df = build_table(folder)
    print(f"Table 2 from {folder}\n")
    print(markdown(df))

    args.out_dir.mkdir(parents=True, exist_ok=True)
    csv_path = args.out_dir / "table2_datasets.csv"
    df.to_csv(csv_path, index=False)
    print(f"\nwrote {csv_path}")
    if args.latex:
        tex_path = args.out_dir / "table2_datasets.tex"
        tex_path.write_text(latex(df))
        print(f"wrote {tex_path}")


if __name__ == "__main__":
    main()
