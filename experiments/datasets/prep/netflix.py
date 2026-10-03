#!/usr/bin/env python3
"""Netflix audience behaviour (UK movies) click-stream -> Cloak workload.

Input:  vodclickstream_uk_movies_03.csv from
        https://www.kaggle.com/datasets/vodclickstream/netflix-audience-behaviour-uk-movies
        columns: <row id>,datetime,duration,title,genres,release_date,movie_id,user_id

Transformation:
  * every row is one read of the movie it refers to (key = movie_id);
  * rows are ordered by `datetime` (stable sort, so rows with the same
    timestamp keep their file order);
  * movie_ids are relabeled to dense integers 0..M-1 in first-appearance
    order of the sorted stream.

Output: one key per line (all reads). The workload used in the paper has
671,736 requests over 8,472 distinct keys; this script reproduces it
byte-for-byte from the Kaggle CSV.

Usage: python3 netflix.py vodclickstream_uk_movies_03.csv -o netflix
"""
import argparse
import csv
from datetime import datetime

TS_FORMAT = "%Y-%m-%d %H:%M:%S"
MOVIE_ID_COL = 6


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("csv", help="vodclickstream_uk_movies_03.csv")
    ap.add_argument("-o", "--out", default="netflix", help="output workload file")
    a = ap.parse_args()

    rows = []
    skipped = 0
    with open(a.csv, newline="", encoding="utf-8") as f:
        reader = csv.reader(f)
        next(reader)  # header
        for row in reader:
            try:
                ts = datetime.strptime(row[1].strip(), TS_FORMAT)
                movie = row[MOVIE_ID_COL]
            except (IndexError, ValueError):
                skipped += 1
                continue
            rows.append((ts, movie))

    rows.sort(key=lambda r: r[0])  # stable

    ids = {}
    with open(a.out, "w") as o:
        for _, movie in rows:
            k = ids.get(movie)
            if k is None:
                k = ids[movie] = len(ids)
            o.write(f"{k}\n")

    print(f"rows read      : {len(rows) + skipped} ({skipped} skipped)")
    print(f"requests       : {len(rows)} (100% reads)")
    print(f"distinct keys  : {len(ids)}")
    if rows:
        print(f"time window    : {rows[0][0]} .. {rows[-1][0]}")
    print(f"wrote          : {a.out}")


if __name__ == "__main__":
    main()
