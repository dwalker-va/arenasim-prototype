#!/usr/bin/env python3
"""AS-385 package scoring: paired comparisons at identical seeds, McNemar on
the flips. Input CSVs are `--batch` outputs of `2026-10-24-as385-package-gen.py`
banks; matches pair by label.

    2026-10-24-as385-package-score.py seat <base-t1> <arm-t1> <base-t2> <arm-t2>
        The side under test is the seat's team (team 1 in the t1 files, team 2
        in the t2 files): a +flip is that side not winning in base and winning
        in the arm, a -flip the reverse.

    2026-10-24-as385-package-score.py uniform <base> <arm>
        Both sides changed (both teams run the same profile on each binary).
        Every match contributes both sides, so a slice is the win rate of the
        teams in it; the overall "all" row is zero-sum up to draws and is
        printed only for the draw count. Per-class rows are the class standing
        the package moves, not a head-to-head.

Slices: all, map, format, seat, the tested side's healer and each DPS class
in it (per map too), the AS-373 bars (TwinPillars; Shaman), and the camp cell
(Warrior+Priest against Warlock+Priest on Nagrand).
"""
import collections
import csv
import gzip
import math
import sys

HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
CAMP = ("PillaredArena", "Warrior+Priest", "Warlock+Priest")


def load(p):
    opener = gzip.open if p.endswith(".gz") else open
    with opener(p, "rt") as f:
        return {r["label"]: r for r in csv.DictReader(f)}


def slices(label, team, seat, other):
    mp = label.split(":")[0]
    members = team.split("+")
    fmt = "3v3" if len(members) == 3 else "2v2"
    healer = next((h for h in HEALERS if h in members), "none")
    keys = ["all", f"map={mp}", f"fmt={fmt}", f"seat={seat}", f"healer={healer}",
            f"map={mp} healer={healer}"]
    for d in members:
        if d not in HEALERS:
            keys += [f"dps={d}", f"map={mp} dps={d}"]
    if (mp, team, other) == CAMP:
        keys.append("camp: Warrior+Priest v Warlock+Priest, Nagrand")
    if (mp, other, team) == CAMP:
        keys.append("camp: Warlock+Priest v Warrior+Priest, Nagrand")
    return keys


def tally(acc, label, team, seat, other, bw, aw, a_draw, b_draw):
    for key in slices(label, team, seat, other):
        x = acc[key]
        x[0] += 1
        x[1] += bw
        x[2] += aw
        x[3] += aw and not bw
        x[4] += bw and not aw
        x[5] += b_draw
        x[6] += a_draw


def report(acc, title):
    print(f"### {title}\n")
    print("| slice | n | win% base | win% arm | delta | +flips | -flips | z | draws base/arm |")
    print("|---|---|---|---|---|---|---|---|---|")
    order = lambda k: (k != "all", not k.startswith("map=") or " " in k, k.startswith("camp"), k)
    for key in sorted(acc, key=order):
        n, bw, aw, p, m, bd, ad = acc[key]
        z = (p - m) / math.sqrt(p + m) if p + m else 0.0
        print(f"| {key} | {n} | {100*bw/n:.1f} | {100*aw/n:.1f} | {100*(aw-bw)/n:+.1f} "
              f"| {p} | {m} | {z:+.2f} | {bd}/{ad} |")
    print()


def main():
    kind = sys.argv[1]
    acc = collections.defaultdict(lambda: [0] * 7)
    missing = 0
    if kind == "seat":
        files = sys.argv[2:6]
        for seat, (bp, ap) in enumerate([(files[0], files[1]), (files[2], files[3])], start=1):
            base, arm = load(bp), load(ap)
            for label, a in arm.items():
                b = base.get(label)
                if b is None:
                    missing += 1
                    continue
                me, other = f"team{seat}", f"team{3 - seat}"
                tally(acc, label, a[me], seat, a[other],
                      b["winner"] == me, a["winner"] == me,
                      a["winner"] == "draw", b["winner"] == "draw")
        report(acc, "Side under test, both seats")
    elif kind == "uniform":
        base, arm = load(sys.argv[2]), load(sys.argv[3])
        for label, a in arm.items():
            b = base.get(label)
            if b is None:
                missing += 1
                continue
            for seat in (1, 2):
                me, other = f"team{seat}", f"team{3 - seat}"
                tally(acc, label, a[me], seat, a[other],
                      b["winner"] == me, a["winner"] == me,
                      a["winner"] == "draw", b["winner"] == "draw")
        report(acc, "Every side (both teams changed)")
    else:
        sys.exit(f"unknown kind {kind}")
    errors = sum(1 for p in sys.argv[2:] for r in load(p).values() if r["winner"] == "error")
    print(f"unpaired: {missing}; errored rows: {errors}")


main()
