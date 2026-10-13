#!/usr/bin/env python3
"""AS-281: the paired result scored from the Hunter's side.

Over every match with a Hunter on exactly one side (paired_sweep's CLEAN and
AGAINST slices together), the Hunter's side's win rate before and after, and
the McNemar flips: + a match the Hunter's side now wins and did not, - one it
now does not win and did.

usage: hunter_side.py <before.csv> <after.csv> [<label>]
"""
import csv, math, sys

b = list(csv.DictReader(open(sys.argv[1])))
a = list(csv.DictReader(open(sys.argv[2])))
label = sys.argv[3] if len(sys.argv) > 3 else ""
assert len(a) == len(b)
n = wb = wa = up = down = 0
for x, y in zip(b, a):
    assert (x["label"], x["seed"]) == (y["label"], y["seed"])
    h1 = "Hunter" in x["team1"].split("+")
    h2 = "Hunter" in x["team2"].split("+")
    if h1 == h2:
        continue
    side = "team1" if h1 else "team2"
    n += 1
    bw, aw = x["winner"] == side, y["winner"] == side
    wb += bw
    wa += aw
    up += aw and not bw
    down += bw and not aw
z = (up - down) / math.sqrt(up + down) if up + down else 0.0
print(f"{label:<28} n={n}  Hunter side {100*wb/n:.1f}% -> {100*wa/n:.1f}%  "
      f"delta {100*(wa-wb)/n:+.1f}pt  flips +{up}/-{down}  z={z:+.2f}")
