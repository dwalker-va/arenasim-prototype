#!/usr/bin/env python3
"""AS-362 head-to-head, scored from the healer side.

    2026-10-18-as362-h2h.py before.csv after.csv

Both CSVs are `arenasim --batch` runs of one JSONL in which every config sets
the two teams' AI profiles apart: labels ending `#T1` put team 1 on `TeamPlan`
and team 2 on `Legacy`, `#T2` the other way round (both seat assignments). The
change under test reaches only a `TeamPlan` healer, so a match is scored from
the `TeamPlan` side, and only where that side fields a healer. Rows pair by
line: identical config, identical seed, the binary the only difference.

Reports, per seat assignment and in total: the `TeamPlan` side's wins before
and after, the paired flips (McNemar on win / not-win), and the same by the
`TeamPlan` side's healer class; then the CONTROL — matches whose `TeamPlan`
side has no healer — which the change cannot reach and must leave identical.
"""
import csv
import math
import sys
from collections import defaultdict

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}


def load(path):
    with open(path) as f:
        return list(csv.DictReader(f))


def mcnemar(b, c):
    """Two-sided exact-ish McNemar z on discordant pairs (b: lost a win,
    c: gained one)."""
    n = b + c
    if n == 0:
        return 0.0
    return (c - b) / math.sqrt(n)


before, after = load(sys.argv[1]), load(sys.argv[2])
assert len(before) == len(after), "the two arms ran different batches"

cells = defaultdict(lambda: {"n": 0, "wb": 0, "wa": 0, "lost": 0, "gained": 0, "db": 0, "da": 0})
control = {"n": 0, "diff": 0}
for b, a in zip(before, after):
    assert (b["label"], b["seed"]) == (a["label"], a["seed"]), "rows out of step"
    seat = b["label"].rsplit("#", 1)[1]
    side = "team1" if seat == "T1" else "team2"
    tp_team = set(b[side].split("+"))
    healers = tp_team & HEALERS
    if not healers:
        control["n"] += 1
        if (b["winner"], b["duration_secs"]) != (a["winner"], a["duration_secs"]):
            control["diff"] += 1
        continue
    wb, wa = b["winner"] == side, a["winner"] == side
    for key in (("seat", seat), ("healer", next(iter(healers))), ("all", "")):
        c = cells[key]
        c["n"] += 1
        c["wb"] += wb
        c["wa"] += wa
        c["lost"] += wb and not wa
        c["gained"] += wa and not wb
        c["db"] += b["winner"] == "draw"
        c["da"] += a["winner"] == "draw"

print(f"{'slice':16} {'n':>6} {'TeamPlan wins before':>21} {'after':>8} {'delta':>7} "
      f"{'lost':>5} {'gained':>6} {'z':>6} {'draws b/a':>10}")
for key in sorted(cells, key=lambda k: (k[0] != "all", k)):
    c = cells[key]
    label = key[0] if key[0] == "all" else f"{key[0]} {key[1]}"
    d = 100 * (c["wa"] - c["wb"]) / c["n"]
    print(f"{label:16} {c['n']:>6} {c['wb']:>13} ({100 * c['wb'] / c['n']:4.1f}%) "
          f"{100 * c['wa'] / c['n']:7.1f}% {d:+6.1f}pt {c['lost']:>5} {c['gained']:>6} "
          f"{mcnemar(c['lost'], c['gained']):+6.2f} {c['db']:>4}/{c['da']:<4}")
print(f"\ncontrol (TeamPlan side fields no healer): {control['n']} matches, "
      f"{control['diff']} differ")
