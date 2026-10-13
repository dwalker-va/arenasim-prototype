#!/usr/bin/env python3
"""AS-249: a paired before/after sweep scored from each healer's side.

usage: 2026-10-13-as249-healer-side.py before.csv after.csv

The change reaches every healer on both sides of a mirrored cell, so a
team1-oriented win rate washes it out. This orients each match on a healer
class instead: for healer X, the matches where exactly one side fields X, scored
as X's side winning. Paired at identical seeds; McNemar z on the flips (a draw
counts as a non-win). Pooled over all four healers, a match with healers of two
different classes appears once per class.
"""
import csv
import gzip
import math
import sys

HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]


def opened(path):
    return gzip.open(path, "rt") if path.endswith(".gz") else open(path)


def load(path):
    rows = {}
    for r in csv.DictReader(opened(path)):
        rows[(r["label"], r["seed"])] = r
    return rows


def main(before_path, after_path):
    before, after = load(before_path), load(after_path)
    keys = sorted(set(before) & set(after))
    print(f"paired matches: {len(keys)}")
    print(f"{'healer side':<14}{'n':>6}{'before':>9}{'after':>9}{'delta':>8}{'+flips':>8}{'-flips':>8}{'z':>7}")
    pooled = [0, 0, 0, 0, 0]
    for healer in HEALERS:
        n = wb = wa = up = down = 0
        for k in keys:
            b, a = before[k], after[k]
            t1 = healer in b["team1"].split("+")
            t2 = healer in b["team2"].split("+")
            if t1 == t2:
                continue
            side = "team1" if t1 else "team2"
            won_b = b["winner"] == side
            won_a = a["winner"] == side
            n += 1
            wb += won_b
            wa += won_a
            up += (won_a and not won_b)
            down += (won_b and not won_a)
        z = (up - down) / math.sqrt(up + down) if up + down else 0.0
        print(f"{healer:<14}{n:>6}{100*wb/n:>8.1f}%{100*wa/n:>8.1f}%{100*(wa-wb)/n:>+7.1f}{up:>8}{down:>8}{z:>7.2f}")
        for i, v in enumerate([n, wb, wa, up, down]):
            pooled[i] += v
    n, wb, wa, up, down = pooled
    z = (up - down) / math.sqrt(up + down) if up + down else 0.0
    print(f"{'pooled':<14}{n:>6}{100*wb/n:>8.1f}%{100*wa/n:>8.1f}%{100*(wa-wb)/n:>+7.1f}{up:>8}{down:>8}{z:>7.2f}")


if __name__ == "__main__":
    main(*sys.argv[1:3])
