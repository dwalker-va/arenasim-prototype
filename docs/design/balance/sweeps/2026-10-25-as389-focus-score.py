#!/usr/bin/env python3
"""AS-389 target focus: paired scoring, replicated across seed blocks.

    2026-10-25-as389-focus-score.py <dir> <prefix> <block> [<block> ...]

Reads `<dir>/<prefix>-{off,t1,t2,uni}-<block>.csv` (`--batch` outputs of
`2026-10-25-as389-focus-gen.py`; matches pair by label). For each slice it
prints, per block and pooled:

- HEAD-TO-HEAD: the side running the focus against the same side without it,
  the other team without it, both seats (off -> t1 for team 1, off -> t2 for
  team 2);
- UNIFORM: every side, both teams running it against neither (off -> uni).

Each cell is `delta (+flips/-flips, z)`. A slice is reported as an EFFECT
only when its sign agrees in every block and its pooled McNemar |z| >= 2;
otherwise it is marked `ns` or `unstable` (signs disagree across blocks).
"""
import collections
import csv
import gzip
import math
import sys

HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]


def load(p):
    opener = gzip.open if p.endswith(".gz") else open
    with opener(p, "rt") as f:
        return {r["label"]: r for r in csv.DictReader(f)}


def slices(label, team):
    mp = "Nagrand" if label.startswith("PillaredArena") else "TwinPillars"
    members = team.split("+")
    fmt = "3v3" if len(members) == 3 else "2v2"
    keys = ["all", f"map={mp}", f"fmt={fmt}"]
    healer = next((h for h in HEALERS if h in members), "none")
    keys += [f"healer={healer}"]
    for d in members:
        if d not in HEALERS:
            keys += [f"dps={d}", f"{mp} dps={d}"]
    return keys


def tally(acc, base, arm, seats):
    for label, a in arm.items():
        b = base[label]
        for seat in seats:
            me = f"team{seat}"
            bw, aw = b["winner"] == me, a["winner"] == me
            for key in slices(label, a[me]):
                x = acc[key]
                x[0] += 1
                x[1] += bw
                x[2] += aw
                x[3] += aw and not bw
                x[4] += bw and not aw


def cell(x):
    n, bw, aw, p, m = x
    z = (p - m) / math.sqrt(p + m) if p + m else 0.0
    return 100 * (aw - bw) / max(n, 1), p, m, z


def main(d, prefix, blocks):
    kinds = {"head-to-head": [], "uniform": []}
    for b in blocks:
        f = lambda a: load(f"{d}/{prefix}-{a}-{b}.csv")
        off, t1, t2, uni = f("off"), f("t1"), f("t2"), f("uni")
        h = collections.defaultdict(lambda: [0] * 5)
        tally(h, off, t1, [1])
        tally(h, off, t2, [2])
        u = collections.defaultdict(lambda: [0] * 5)
        tally(u, off, uni, [1, 2])
        kinds["head-to-head"].append(h)
        kinds["uniform"].append(u)
    for kind, per in kinds.items():
        print(f"\n### {kind} ({', '.join(blocks)})\n")
        print("| slice | n pooled | " + " | ".join(blocks) + " | pooled | verdict |")
        print("|---|---|" + "---|" * len(blocks) + "---|---|")
        keys = sorted(set().union(*per), key=lambda k: (k != "all", not k.startswith("map="), k))
        for k in keys:
            cells = [cell(p[k]) for p in per]
            pooled = [sum(p[k][i] for p in per) for i in range(5)]
            dp, pp, pm, pz = cell(pooled)
            signs = {(c[0] > 0) - (c[0] < 0) for c in cells}
            if len(signs - {0}) > 1:
                verdict = "unstable"
            elif abs(pz) >= 2:
                verdict = "EFFECT"
            else:
                verdict = "ns"
            row = " | ".join(f"{c[0]:+.1f} (z {c[3]:+.1f})" for c in cells)
            print(f"| {k} | {pooled[0]} | {row} | {dp:+.1f} ({pp}/{pm}, z {pz:+.2f}) | {verdict} |")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3:])
