#!/usr/bin/env python3
"""Aggregate `arenasim --batch` per-match CSV into matchup winrates.

The batch CSV has columns: label,team1,team2,seed,winner,end_reason,duration_secs
This groups by `label` (falling back to team1|team2) and reports team1's winrate
with a Wilson 95% confidence interval, so you can tell a real difference from
sampling noise.

Examples
--------
# Per-matchup table + overall
agg_sweep.py results.csv

# Overall winrate of one slice (clean slice: enemy has a Mage, ally is not Mage)
agg_sweep.py results.csv --include 'vs_.*Mage' --exclude 'Hunter\\+Mage' --overall-only

# Sub-aggregate by a key pulled from the label (e.g. the partner in 'Hunter+X_vs_...')
agg_sweep.py results.csv --group 'Hunter\\+([A-Za-z]+)_vs'

# Before/after: print per-matchup delta and flag which moved beyond noise
agg_sweep.py after.csv --compare before.csv

Confidence
----------
A reported winrate prints its Wilson 95% interval: `37.0% [33.9-40.1]`. Two
runs whose intervals OVERLAP have not been shown to differ -- this is the
guard against the symmetric-nerf "wash" trap (a 37.0 vs 37.7 that looks like a
change but isn't) -- and `--compare` flags MOVED on exactly that test, over
exactly the intervals it prints, so a reader can check the flag by eye. Bump N
to tighten the interval on close matchups.
"""
import argparse
import csv
import math
import re
import sys
from collections import OrderedDict


def wilson_interval(wins, n, z=1.96):
    """Wilson score interval for a proportion (95% by default).

    With no trials the interval is the whole range: n=0 carries no information,
    and a (0, 0) would make an empty group read as a confident 0% win rate.
    Bounds are clamped to [0, 1]; the closed form lands a hair below zero at
    wins=0, which would print as "-0.0".
    """
    if n == 0:
        return (0.0, 1.0)
    p = wins / n
    denom = 1 + z * z / n
    center = (p + z * z / (2 * n)) / denom
    margin = (z / denom) * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return (max(0.0, center - margin), min(1.0, center + margin))


def separated(a, b):
    """True when two intervals do not overlap -- the MOVED test."""
    return a[0] > b[1] or b[0] > a[1]


def load(path):
    groups = OrderedDict()
    with open(path) as f:
        for row in csv.DictReader(f):
            key = row.get("label") or f"{row['team1']}|{row['team2']}"
            g = groups.setdefault(key, {"t1": row["team1"], "t2": row["team2"],
                                        "w1": 0, "w2": 0, "dr": 0, "n": 0, "err": 0})
            # `.strip()` to match `headtohead_sweep.py`, which reads the same
            # files: stray whitespace must not make one tool see an error
            # where the other sees a draw.
            w = row["winner"].strip()
            if w == "error":
                # A failed match is not a loss. Counting one would inflate n and
                # drag every rate down, with nothing but a one-line warning to
                # show for it, so errors are tallied separately and excluded.
                g["err"] += 1
                continue
            g["n"] += 1
            if w == "team1":
                g["w1"] += 1
            elif w == "team2":
                g["w2"] += 1
            else:
                g["dr"] += 1
    return groups


def winrate(g):
    return g["w1"] / g["n"] if g["n"] else 0.0


def rate_cell(wins, n, width=5):
    """`nn.n% [lo-hi]`, or an explicit n/a when there is nothing to rate.

    Both branches are the same width, so a row with no usable matches does
    not shunt the rest of its column out of alignment.
    """
    if n == 0:
        return f"{'n/a':>{width}}  {'(no matches)':<14}"
    lo, hi = wilson_interval(wins, n)
    span = f"[{100*lo:.1f}-{100*hi:.1f}]"
    return f"{100*wins/n:{width}.1f}% {span:<14}"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("csv", help="batch results CSV")
    ap.add_argument("--include", help="regex; keep only labels matching")
    ap.add_argument("--exclude", help="regex; drop labels matching")
    ap.add_argument("--group", help="regex with one capture group; sub-aggregate by it")
    ap.add_argument("--overall-only", action="store_true")
    ap.add_argument("--compare", help="baseline CSV for before/after deltas")
    args = ap.parse_args(argv)

    groups = load(args.csv)
    if args.include:
        rx = re.compile(args.include)
        groups = OrderedDict((k, v) for k, v in groups.items() if rx.search(k))
    if args.exclude:
        rx = re.compile(args.exclude)
        groups = OrderedDict((k, v) for k, v in groups.items() if not rx.search(k))

    if not groups:
        print("no matching groups", file=sys.stderr)
        sys.exit(1)

    base = load(args.compare) if args.compare else None

    # Overall (pooled) winrate across the selected groups.
    tot_w = sum(g["w1"] for g in groups.values())
    tot_n = sum(g["n"] for g in groups.values())
    errs = sum(g["err"] for g in groups.values())
    if tot_n == 0:
        print(f"no usable matches in {args.csv} ({errs} errored)", file=sys.stderr)
        sys.exit(1)
    print(f"OVERALL team1 winrate: {rate_cell(tot_w, tot_n)}  ({tot_w}/{tot_n})")
    if errs:
        print(f"  WARNING: {errs} match(es) errored and are excluded from all rates")

    if args.group:
        rx = re.compile(args.group)
        sub = OrderedDict()
        for k, g in groups.items():
            m = rx.search(k)
            key = m.group(1) if m else "?"
            s = sub.setdefault(key, {"w1": 0, "n": 0})
            s["w1"] += g["w1"]
            s["n"] += g["n"]
        print("\nBy group:")
        for key, s in sorted(sub.items(), key=lambda kv: -kv[1]["w1"] / max(kv[1]["n"], 1)):
            print(f"  {key:<16} {rate_cell(s['w1'], s['n'])}  ({s['w1']}/{s['n']})")

    if args.overall_only:
        return 0

    print("\nPer matchup (team1 winrate):")
    for k, g in sorted(groups.items(), key=lambda kv: winrate(kv[1])):
        line = f"  {rate_cell(g['w1'], g['n'])}  {k}  (W{g['w1']} L{g['w2']} D{g['dr']})"
        if g["err"]:
            line += f"  ({g['err']} errored)"
        if base and k in base:
            b = base[k]
            was = rate_cell(b["w1"], b["n"]).strip()
            if not g["n"] or not b["n"]:
                # No measurement on one side, so there is no difference to
                # state. A delta here reads as a real drop -- an empty cell
                # against a 50% baseline printed "-50.0" -- when the run
                # simply has nothing to say about this matchup.
                line += f"   [was {was}]"
            else:
                delta = 100 * (winrate(g) - winrate(b))
                # MOVED only when the two Wilson intervals -- the ones printed
                # on this line and in the bracket below -- do not overlap.
                # Comparing `p +/- halfwidth` instead, as this once did, treats
                # intervals as centred on the raw rates and so calls borderline
                # pairs separated when they are not.
                moved = separated(
                    wilson_interval(g["w1"], g["n"]), wilson_interval(b["w1"], b["n"])
                )
                flag = "  <== MOVED" if moved else ""
                line += f"   [was {was}, {delta:+.1f}{flag}]"
        print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
