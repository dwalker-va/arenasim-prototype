#!/usr/bin/env python3
"""Paired head-to-head scoring (McNemar on flips).

    2026-10-23-as373-h2h-score.py <base-t1.csv> <arm-t1.csv> <base-t2.csv> <arm-t2.csv>

For seat 1 the side under test is team 1; for seat 2, team 2. Each match in
the arm is paired with the base match of the same label (same seed, comps,
map): a flip is the tested side losing (or drawing) in base and winning in
the arm (+), or the reverse (-). Reported per map, per healer class of the
tested side, per format, and overall: win% base -> arm, flips, z.
"""
import collections
import csv
import math
import sys


def load(p):
    with open(p) as f:
        return {r["label"]: r for r in csv.DictReader(f)}


def side_healer(team):
    for h in ["Priest", "Paladin", "Shaman", "Druid"]:
        if h in team.split("+"):
            return h
    return "none"


acc = collections.defaultdict(lambda: [0, 0, 0, 0, 0, 0])  # n, base_w, arm_w, plus, minus, draws_arm
for seat, (bp, ap) in enumerate([(sys.argv[1], sys.argv[2]), (sys.argv[3], sys.argv[4])], start=1):
    base, arm = load(bp), load(ap)
    for label, a in arm.items():
        b = base.get(label)
        if b is None:
            continue
        me = f"team{seat}"
        team = a[f"team{seat}"]
        mp = label.split(":")[0]
        fmt = "3v3" if team.count("+") == 2 else "2v2"
        bw, aw = b["winner"] == me, a["winner"] == me
        for key in ["all", f"map={mp}", f"healer={side_healer(team)}", f"fmt={fmt}", f"seat={seat}"]:
            x = acc[key]
            x[0] += 1
            x[1] += bw
            x[2] += aw
            x[3] += (aw and not bw)
            x[4] += (bw and not aw)
            x[5] += a["winner"] == "draw"

print("| slice | n | win% base | win% arm | delta | +flips | -flips | z |")
print("|---|---|---|---|---|---|---|---|")
for key in sorted(acc, key=lambda k: (k != "all", k)):
    n, bw, aw, p, m, d = acc[key]
    z = (p - m) / math.sqrt(p + m) if p + m else 0.0
    print(f"| {key} | {n} | {100*bw/n:.1f} | {100*aw/n:.1f} | {100*(aw-bw)/n:+.1f} | {p} | {m} | {z:+.2f} |")
