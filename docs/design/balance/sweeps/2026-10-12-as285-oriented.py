#!/usr/bin/env python3
"""AS-285 directional sweep, oriented to the side with a CC partner.

usage: 2026-10-12-as285-oriented.py <before.csv[.gz]> <after.csv[.gz]>

The change stops a hard-CC chooser casting onto an enemy its own team already
holds. It can only help a team that HOLDS enemies with one unit while another
unit chooses hard CC: a CC partner (Mage or Druid) beside a Warlock, Rogue,
Paladin or Hunter. Both sides of a 2v2 can field such a pair, so a team-1 win
rate pooled over every cell nets the effect out. Each paired match whose
exactly-one side fields a pair is therefore scored from THAT side (win > draw >
loss), and the discordant pairs are tested with an exact two-sided sign test
(McNemar's exact form), with the resolution floor beside it: the smallest p the
most lopsided split of that many discordant pairs could reach.

It also prints the control (rows whose label fields no class the change can
reach, compared on winner and duration) and how many rows moved at all.
Rows are keyed by (label, seed); the label carries the map suffix.
"""
from __future__ import annotations

import csv
import gzip
import math
import sys

# Every class with a hard-CC chooser the change touches.
AFFECTED = ("Warlock", "Rogue", "Paladin", "Hunter", "Mage", "Druid")
PARTNER = ("Mage", "Druid")
CHOOSER = ("Warlock", "Rogue", "Paladin", "Hunter")


def read(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return {(r["label"], r["seed"]): r for r in csv.DictReader(f)}


def has_pair(team):
    classes = team.split("+")
    return any(c in PARTNER for c in classes) and any(c in CHOOSER for c in classes)


def score(row, side):
    if row["winner"] == side:
        return 1
    if row["winner"] == "draw":
        return 0
    return -1


def sign_test(plus, minus):
    n = plus + minus
    if n == 0:
        return 1.0
    k = min(plus, minus)
    tail = sum(math.comb(n, i) for i in range(k + 1)) / 2.0**n
    return min(1.0, 2 * tail)


def report(name, pairs):
    plus = minus = 0
    wins_b = wins_a = 0
    for b, a, side in pairs:
        sb, sa = score(b, side), score(a, side)
        wins_b += sb == 1
        wins_a += sa == 1
        if sa > sb:
            plus += 1
        elif sa < sb:
            minus += 1
    n = plus + minus
    floor = sign_test(n, 0) if n else 1.0
    print(
        f"  {name:<14} n={len(pairs):>5}  pair-side wins {wins_b} -> {wins_a}  "
        f"discordant {n} (after+ {plus} / after- {minus})  exact p={sign_test(plus, minus):.3f}  "
        f"floor p={floor:.3g}"
    )


def main(argv):
    before, after = read(argv[1]), read(argv[2])
    assert before.keys() == after.keys(), "the arms ran different batches"
    keys = sorted(before)

    control = [k for k in keys if not any(c in k[0] for c in AFFECTED)]
    same = sum(
        before[k]["winner"] == after[k]["winner"]
        and before[k]["duration_secs"] == after[k]["duration_secs"]
        for k in control
    )
    print(f"CONTROL: {same}/{len(control)} rows with no affected class identical in winner and duration")

    moved = [
        k
        for k in keys
        if (before[k]["winner"], before[k]["duration_secs"])
        != (after[k]["winner"], after[k]["duration_secs"])
    ]
    flipped = [k for k in keys if before[k]["winner"] != after[k]["winner"]]
    decisive = sum(after[k]["winner"] != "draw" for k in keys)
    print(
        f"paired matches: {len(keys)}; decisive (after): {decisive}; "
        f"moved in winner or duration: {len(moved)}; winner changed: {len(flipped)}"
    )

    by_map = {}
    for k in keys:
        b = before[k]
        p1, p2 = has_pair(b["team1"]), has_pair(b["team2"])
        if p1 == p2:
            continue
        side = "team1" if p1 else "team2"
        m = k[0].rsplit("#", 1)[1] if "#" in k[0] else "BasicArena"
        by_map.setdefault(m, []).append((b, after[k], side))
    print("Pair-side result, before -> after (cells where exactly one side fields a CC pair):")
    allp = []
    for m in sorted(by_map):
        report(m, by_map[m])
        allp += by_map[m]
    report("ALL", allp)


if __name__ == "__main__":
    main(sys.argv)
