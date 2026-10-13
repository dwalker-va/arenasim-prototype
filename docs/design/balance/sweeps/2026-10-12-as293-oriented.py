#!/usr/bin/env python3
"""AS-293 directional sweep, scored from the interrupter's side.

usage: 2026-10-12-as293-oriented.py <before.csv[.gz]> <after.csv[.gz]>

The change keeps an interrupt (Kick, Pummel, Wind Shear, the Warlock's
Felhunter's Spell Lock) off a caster its interrupter cannot see. It reaches
only a team that fields an interrupter, so a paired match is scored from the
side that fields one (win 1, draw 0.5, loss 0):

- **any interrupter**: matches where EXACTLY ONE side fields any of the four
  interrupting classes — the clean slice, the other side has no interrupt;
- **per class**: matches where exactly one side fields that class (the other
  side may field a different interrupter, so these rows overlap and are
  confounded by it; read them as where the effect sits, not as standings).

The matches whose result moved are tested with an exact two-sided sign test
(McNemar's exact form), and the floor — the p the most lopsided split of
that many moved matches could reach — is printed beside it. Rows are keyed
by (label, seed).
"""
from __future__ import annotations

import csv
import gzip
import math
import sys

INTERRUPTERS = ("Rogue", "Warrior", "Shaman", "Warlock")


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


def slice_report(name, before, after, fields):
    """`fields(team)` says whether a team (a list of classes) is in the slice."""
    n = up = down = both = 0
    sb = sa = 0.0
    for key, b in before.items():
        a = after[key]
        f1 = fields(b["team1"].split("+"))
        f2 = fields(b["team2"].split("+"))
        if f1 and f2:
            both += 1
            continue
        if not (f1 or f2):
            continue
        side = "team1" if f1 else "team2"
        x, y = score(b, side), score(a, side)
        n += 1
        sb += x
        sa += y
        if y > x:
            up += 1
        elif y < x:
            down += 1
    if n == 0:
        print(f"{name}: no match fields it on exactly one side")
        return
    print(
        f"{name:<15} n={n:5d}  before {100 * sb / n:5.1f}%  after {100 * sa / n:5.1f}%  "
        f"delta {100 * (sa - sb) / n:+.2f}pt  moved {up:3d} better / {down:3d} worse  "
        f"p={sign_test(up, down):.3f}  (floor p={sign_test(up + down, 0):.2g}; "
        f"both sides, not scored: {both})"
    )


def main():
    before, after = read(sys.argv[1]), read(sys.argv[2])
    assert before.keys() == after.keys(), "arms are not paired"
    slice_report(
        "any interrupter",
        before,
        after,
        lambda team: any(c in INTERRUPTERS for c in team),
    )
    for cls in INTERRUPTERS:
        slice_report(cls, before, after, lambda team, cls=cls: cls in team)


if __name__ == "__main__":
    main()
