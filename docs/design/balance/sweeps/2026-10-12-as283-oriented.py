#!/usr/bin/env python3
"""AS-283 head-to-head, oriented to the TeamPlan side.

usage: 2026-10-12-as283-oriented.py <noband.csv[.gz]> <band.csv[.gz]>

The change acts only on the team running TeamPlan, and the sweep puts TeamPlan
on team 1 in half its rows (label suffix `_TL`) and on team 2 in the other half
(`_LT`). A team-1 win rate pooled over both halves nets a per-team change out,
as `paired_sweep.py`'s docstring warns. So each paired match is scored from the
TeamPlan side: a TeamPlan win, a TeamPlan loss, or a draw. The discordant pairs
are matches whose TeamPlan-side result differs between the arms. Then:

- `band+` is the number of discordant pairs where the banded arm did better for
  the TeamPlan side, and `band-` the number where it did worse (win > draw > loss);
- an exact two-sided sign test on band+ against band- (McNemar's exact form,
  the right test at a handful of discordant pairs), and the continuity-corrected
  McNemar z beside it;
- the RESOLUTION FLOOR: the smallest p, and the largest corrected z, that the
  most lopsided split of that many discordant pairs (n-0) could reach. If that p
  is above 0.05, no imbalance among those pairs could be significant.

It prints the result per map and assignment, and overall. It also prints the
control, which is every row whose label carries no affected class, compared on
winner and duration.
"""
from __future__ import annotations

import csv
import gzip
import math
import sys

AFFECTED = ("Warrior", "Priest", "Paladin", "Shaman", "Druid")


def read(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return {(r["label"], r["seed"]): r for r in csv.DictReader(f)}


def tp_team(label):
    suffix = label.rsplit("#", 1)[1]
    return "team1" if suffix.endswith("_TL") else "team2"


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
    tail = sum(math.comb(n, i) for i in range(k + 1)) / 2.0 ** n
    return min(1.0, 2 * tail)


def main(before_path, after_path):
    before, after = read(before_path), read(after_path)
    assert before.keys() == after.keys(), "arms are not paired"
    control = [k for k in before if not any(c in k[0].split("#")[0] for c in AFFECTED)]
    same = sum(
        1
        for k in control
        if (before[k]["winner"], before[k]["duration_secs"])
        == (after[k]["winner"], after[k]["duration_secs"])
    )
    print(f"CONTROL: {same}/{len(control)} rows with no affected class identical in winner and duration")
    moved = sum(
        1
        for k in before
        if (before[k]["winner"], before[k]["duration_secs"])
        != (after[k]["winner"], after[k]["duration_secs"])
    )
    print(f"paired matches: {len(before)}; moved in winner or duration: {moved}")

    slices = {}
    for k in before:
        suffix = k[0].rsplit("#", 1)[1]
        for name in (suffix, "ALL"):
            slices.setdefault(name, []).append(k)
    print("TeamPlan-side result, noband -> band:")
    for name in sorted(slices, key=lambda s: (s == "ALL", s)):
        keys = slices[name]
        wins_b = sum(score(before[k], tp_team(k[0])) == 1 for k in keys)
        wins_a = sum(score(after[k], tp_team(k[0])) == 1 for k in keys)
        plus = minus = 0
        for k in keys:
            side = tp_team(k[0])
            d = score(after[k], side) - score(before[k], side)
            plus += d > 0
            minus += d < 0
        n = plus + minus
        diff = plus - minus
        z = math.copysign(max(abs(diff) - 1, 0) / math.sqrt(n), diff) if n else 0.0
        floor_p = sign_test(n, 0)  # the most lopsided split possible
        floor_z = (n - 1) / math.sqrt(n) if n else 0.0
        verdict = "" if floor_p < 0.05 else " -> no split of these pairs can reach p<0.05"
        print(
            f"  {name:5s} n={len(keys):5d}  TeamPlan wins {wins_b} -> {wins_a}"
            f"  discordant {n} (band+ {plus} / band- {minus})"
            f"  exact p={sign_test(plus, minus):.3f}  corrected z={z:+.2f}"
            f"  floor: best p={floor_p:.3f}, best z={floor_z:.2f}{verdict}"
        )


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
