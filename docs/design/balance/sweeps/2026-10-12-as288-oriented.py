#!/usr/bin/env python3
"""AS-288 head-to-head, oriented to the TeamPlan side.

usage: 2026-10-12-as288-oriented.py <main.csv[.gz]> <branch.csv[.gz]>

Rows come from `2026-10-12-as288-gen.py`. The debounce changes what the
`TeamPlan`-gated Peel play reads, so its effect falls on the team running
`TeamPlan`: team 1 in the `_TL` rows, team 2 in the `_LT` rows. A team-1 win
rate pooled over both nets that out, so each paired match is scored from the
TeamPlan side (win > draw > loss), and the discordant pairs are tested with an
exact two-sided sign test (McNemar's exact form), with the continuity-corrected
McNemar z and the resolution floor (the best p and z the most lopsided split of
that many pairs could reach) beside it.

The `_LL` rows (both teams `Legacy`) are the CONTROL: nothing `Legacy` reads
changed, so every one must match between the arms in winner, end reason and
duration. The script exits 1 if one does not.
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


def assignment(label):
    return label.rsplit("#", 1)[1]


def tp_team(label):
    return "team1" if assignment(label).endswith("_TL") else "team2"


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


def outcome(row):
    return (row["winner"], row["end_reason"], row["duration_secs"])


def main(before_path, after_path):
    before, after = read(before_path), read(after_path)
    assert before.keys() == after.keys(), "arms are not paired"

    control = [k for k in before if assignment(k[0]).endswith("_LL")]
    same = sum(outcome(before[k]) == outcome(after[k]) for k in control)
    kills = sum(before[k]["end_reason"] == "kill" for k in control)
    durations = len({before[k]["duration_secs"] for k in control})
    print(
        f"CONTROL (Legacy v Legacy): {same}/{len(control)} rows identical in winner, end reason "
        f"and duration ({kills} ended in a kill, {durations} distinct durations)"
    )

    h2h = [k for k in before if not assignment(k[0]).endswith("_LL")]
    moved = sum(outcome(before[k]) != outcome(after[k]) for k in h2h)
    print(f"head-to-head paired matches: {len(h2h)}; moved in winner, end reason or duration: {moved}")

    slices = {}
    for k in h2h:
        for name in (assignment(k[0]), "ALL"):
            slices.setdefault(name, []).append(k)
    print("TeamPlan-side result, main -> branch:")
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
        floor_p = sign_test(n, 0)
        floor_z = (n - 1) / math.sqrt(n) if n else 0.0
        delta = 100.0 * (wins_a - wins_b) / len(keys)
        print(
            f"  {name:5s} n={len(keys):5d}  TeamPlan wins {wins_b} -> {wins_a} ({delta:+.2f}pt)"
            f"  discordant {n} (+{plus} / -{minus})"
            f"  exact p={sign_test(plus, minus):.3f}  corrected z={z:+.2f}"
            f"  floor: best p={floor_p:.3g}, best z={floor_z:.2f}"
        )
    if same != len(control):
        sys.exit(1)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
