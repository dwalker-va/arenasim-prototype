#!/usr/bin/env python3
"""AS-164: the Paladin-side effect of AS-234 (Paladin root Cleanse), by enemy class.

    as234-slices.py before.csv after.csv

before = e4c01dc (AS-234's parent), after = ea63f4f (AS-234 merged); both arms
ran 2026-10-10-as164-as234-paired.jsonl. A match counts once per side that
fields a Paladin and whose opponent fields none (the mirrored matches cancel).
The figure is the Paladin side's win-rate change; McNemar z on its flips.
"""
import csv
import math
import sys
from collections import defaultdict

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]


def load(p):
    return {(r["team1"], r["team2"], r["seed"]): r["winner"] for r in csv.DictReader(open(p))}


def main():
    before, after = load(sys.argv[1]), load(sys.argv[2])
    agg = defaultdict(lambda: [0, 0, 0, 0])  # n, before wins, gained, lost
    for k, wb in before.items():
        wa = after[k]
        t1, t2 = k[0].split("+"), k[1].split("+")
        for side, mine, theirs in ((1, t1, t2), (2, t2, t1)):
            if "Paladin" not in mine or "Paladin" in theirs:
                continue
            b, a = wb == f"team{side}", wa == f"team{side}"
            for key in ["ALL"] + [f"vs {c}" for c in theirs]:
                s = agg[key]
                s[0] += 1
                s[1] += b
                s[2] += a and not b
                s[3] += b and not a
    print("| Paladin side, enemy fields | n | before | after | delta | flips (+/-) | McNemar z |")
    print("|---|---|---|---|---|---|---|")
    for key in ["ALL"] + [f"vs {c}" for c in CLASSES if c != "Paladin"]:
        n, b, g, l = agg[key]
        z = (g - l) / math.sqrt(g + l) if g + l else 0.0
        print(f"| {key} | {n} | {100 * b / n:.1f}% | {100 * (b + g - l) / n:.1f}% | {100 * (g - l) / n:+.1f}pt | {g}/{l} | {z:+.2f} |")


if __name__ == "__main__":
    main()
