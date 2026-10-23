#!/usr/bin/env python3
"""AS-292 reachable-cell head-to-head, scored from the `TeamPlan` side.

    2026-10-19-as292-h2h.py before.csv[.gz] after.csv[.gz]

Both CSVs are `arenasim --batch` runs of one `2026-10-19-as292-gen.py` JSONL:
before = main, after = the PR merged onto main. Rows pair on (label, seed).
A `_TL` row is scored from team 1, an `_LT` row from team 2: a TeamPlan win
or not. The flips are tested with McNemar's z, (gained - lost) / sqrt(flips).

Slices: everything, per map, per map and seat, per `TeamPlan` healer, per
enemy healer, and per map x `TeamPlan` healer x enemy healer.

The `_LL` rows are the CONTROL: every one must match in winner, end reason
and duration. The script exits 1 if one does not, and prints the control's
decisive-event counts so a clean control is not vacuous.
"""
import csv
import gzip
import math
import sys
from collections import defaultdict

HEALERS = {"Priest", "Shaman", "Druid", "Paladin"}


def read(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return {(r["label"], r["seed"]): r for r in csv.DictReader(f)}


def healer(team):
    return next(c for c in team.split("+") if c in HEALERS)


before, after = read(sys.argv[1]), read(sys.argv[2])
assert before.keys() == after.keys(), "the two arms ran different batches"

cells = defaultdict(lambda: [0, 0, 0, 0, 0])  # n, wins before, after, lost, gained
ctrl = {"n": 0, "diff": 0, "kills": 0, "durations": set()}
for key, b in sorted(before.items()):
    a = after[key]
    label = key[0]
    m, assign = label.rsplit("#", 1)[1].split("_")
    if assign == "LL":
        ctrl["n"] += 1
        same = all(b[f] == a[f] for f in ("winner", "end_reason", "duration_secs"))
        ctrl["diff"] += not same
        ctrl["kills"] += b["end_reason"] == "kill"
        ctrl["durations"].add(b["duration_secs"])
        continue
    side, other = ("team1", "team2") if assign == "TL" else ("team2", "team1")
    wb, wa = b["winner"] == side, a["winner"] == side
    tp, en = healer(b[side]), healer(b[other])
    for k in (
        ("all",),
        ("map", m),
        ("map seat", f"{m} {assign}"),
        ("TeamPlan healer", tp),
        ("enemy healer", en),
        ("map tp x enemy", f"{m} {tp} v {en}"),
    ):
        c = cells[k]
        c[0] += 1
        c[1] += wb
        c[2] += wa
        c[3] += wb and not wa
        c[4] += wa and not wb


def z(lost, gained):
    n = lost + gained
    return 0.0 if n == 0 else (gained - lost) / math.sqrt(n)


print(f"{'slice':34} {'n':>6} {'before':>7} {'after':>7} {'delta':>8} {'lost':>5} {'gained':>6} {'z':>6}")
order = ["all", "map", "map seat", "TeamPlan healer", "enemy healer", "map tp x enemy"]
for k in sorted(cells, key=lambda k: (order.index(k[0]), k)):
    n, wb, wa, lost, gained = cells[k]
    name = k[0] if len(k) == 1 else f"{k[0]}: {k[1]}"
    print(f"{name:34} {n:>6} {100 * wb / n:6.1f}% {100 * wa / n:6.1f}% "
          f"{100 * (wa - wb) / n:+7.2f}pt {lost:>5} {gained:>6} {z(lost, gained):+6.2f}")

print(f"\ncontrol (LL): {ctrl['n']} matches, {ctrl['diff']} differ; "
      f"{ctrl['kills']} ended by a kill, {len(ctrl['durations'])} distinct durations")
sys.exit(1 if ctrl["diff"] else 0)
