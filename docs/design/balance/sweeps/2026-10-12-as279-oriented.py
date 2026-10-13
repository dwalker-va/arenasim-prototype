#!/usr/bin/env python3
"""AS-279 directional sweep, scored from the Paladin's side.

usage: 2026-10-12-as279-oriented.py <before.csv[.gz]> <after.csv[.gz]>

The change keeps a Paladin's Hammer of Justice off a target it cannot see. It
reaches only a team that fields a Paladin, so every paired match whose
EXACTLY-ONE side fields a Paladin is scored from that side (win 1, draw 0.5,
loss 0), and the matches whose result moved are tested with an exact two-sided
sign test (McNemar's exact form). Mirrored Paladin cells are counted, not
scored. Rows are keyed by (label, seed).
"""
from __future__ import annotations

import csv
import gzip
import math
import sys


def read(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return {(r["label"], r["seed"]): r for r in csv.DictReader(f)}


def score(row, side):
    if row["winner"] == "draw":
        return 0.5
    return 1.0 if row["winner"] == side else 0.0


def sign_test(up, down):
    n = up + down
    if n == 0:
        return 1.0
    k = min(up, down)
    tail = sum(math.comb(n, i) for i in range(k + 1)) / 2**n
    return min(1.0, 2 * tail)


def main():
    before, after = read(sys.argv[1]), read(sys.argv[2])
    assert before.keys() == after.keys(), "arms are not paired"
    n = up = down = mirrored = 0
    sb = sa = 0.0
    for key, b in before.items():
        a = after[key]
        p1 = "Paladin" in b["team1"].split("+")
        p2 = "Paladin" in b["team2"].split("+")
        if p1 and p2:
            mirrored += 1
            continue
        if not (p1 or p2):
            continue
        side = "team1" if p1 else "team2"
        x, y = score(b, side), score(a, side)
        n += 1
        sb += x
        sa += y
        if y > x:
            up += 1
        elif y < x:
            down += 1
    print(f"Paladin on exactly one side: n={n}  (mirrored, not scored: {mirrored})")
    print(f"Paladin score before {100 * sb / n:.1f}%  after {100 * sa / n:.1f}%  "
          f"delta {100 * (sa - sb) / n:+.2f}pt")
    print(f"moved: {up} better / {down} worse for the Paladin  "
          f"exact sign test p={sign_test(up, down):.3f}  "
          f"(floor: the most lopsided split of {up + down} reaches "
          f"p={sign_test(up + down, 0):.2g})")


if __name__ == "__main__":
    main()
