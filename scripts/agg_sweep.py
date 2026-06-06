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
A reported winrate of 37.0% +/-3.1 means the true value is ~33.9-40.1% at 95%.
If two runs' intervals overlap heavily, the difference is NOT a finding -- this
is the guard against the symmetric-nerf "wash" trap (a 37.0 vs 37.7 that looks
like a change but isn't). Bump N to tighten the interval on close matchups.
"""
import argparse
import csv
import math
import re
import sys
from collections import OrderedDict


def wilson_halfwidth(wins, n, z=1.96):
    """Half-width of the Wilson score interval for a proportion (95% by default)."""
    if n == 0:
        return 0.0
    p = wins / n
    denom = 1 + z * z / n
    center = (p + z * z / (2 * n)) / denom
    margin = (z / denom) * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    # Return the symmetric-ish half-width around p (use margin; good enough for display).
    return margin


def load(path):
    groups = OrderedDict()
    with open(path) as f:
        for row in csv.DictReader(f):
            key = row.get("label") or f"{row['team1']}|{row['team2']}"
            g = groups.setdefault(key, {"t1": row["team1"], "t2": row["team2"],
                                        "w1": 0, "w2": 0, "dr": 0, "n": 0, "err": 0})
            g["n"] += 1
            w = row["winner"]
            if w == "team1":
                g["w1"] += 1
            elif w == "team2":
                g["w2"] += 1
            elif w == "error":
                g["err"] += 1
            else:
                g["dr"] += 1
    return groups


def winrate(g):
    return g["w1"] / g["n"] if g["n"] else 0.0


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("csv", help="batch results CSV")
    ap.add_argument("--include", help="regex; keep only labels matching")
    ap.add_argument("--exclude", help="regex; drop labels matching")
    ap.add_argument("--group", help="regex with one capture group; sub-aggregate by it")
    ap.add_argument("--overall-only", action="store_true")
    ap.add_argument("--compare", help="baseline CSV for before/after deltas")
    args = ap.parse_args()

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
    hw = wilson_halfwidth(tot_w, tot_n) * 100
    print(f"OVERALL team1 winrate: {100*tot_w/tot_n:.1f}% +/-{hw:.1f}  ({tot_w}/{tot_n})")
    errs = sum(g["err"] for g in groups.values())
    if errs:
        print(f"  WARNING: {errs} matches errored (counted as non-wins)")

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
            hw = wilson_halfwidth(s["w1"], s["n"]) * 100
            print(f"  {key:<16} {100*s['w1']/s['n']:5.1f}% +/-{hw:4.1f}  ({s['w1']}/{s['n']})")

    if args.overall_only:
        return

    print("\nPer matchup (team1 winrate):")
    for k, g in sorted(groups.items(), key=lambda kv: winrate(kv[1])):
        wr = 100 * winrate(g)
        hw = wilson_halfwidth(g["w1"], g["n"]) * 100
        line = f"  {wr:5.1f}% +/-{hw:4.1f}  {k}  (W{g['w1']} L{g['w2']} D{g['dr']})"
        if base and k in base:
            b = base[k]
            bwr = 100 * winrate(b)
            delta = wr - bwr
            bhw = wilson_halfwidth(b["w1"], b["n"]) * 100
            # Flag as real only if the intervals do not overlap.
            real = abs(delta) > (hw + bhw)
            flag = "  <== MOVED" if real else ""
            line += f"   [was {bwr:.1f}%, {delta:+.1f}{flag}]"
        print(line)


if __name__ == "__main__":
    main()
