#!/usr/bin/env python3
"""AS-125: paired Hunter-side win rate PER ENEMY COMP, one row per comp.

usage: by-enemy.py <before.csv> <after.csv> [--min-flips N]

Reads two `arenasim --batch` CSVs run from the same JSONL. Keeps the matches
with a Hunter on exactly one side (a Hunter mirror says nothing about the
Hunter), orients each to the Hunter's side, and groups by the ENEMY comp
(classes sorted, so Priest+Rogue and Rogue+Priest are one row). Per row: n,
Hunter win rate before / after, the flips each way, and McNemar z on the
flips. Pooled buckets hide opposite-signed comps; this does not pool.
Rows are sorted by z; `--min-flips` hides rows with fewer flips (default 0).
"""
import csv, math, sys
from collections import defaultdict

DISPELLERS = {"Priest", "Paladin", "Warlock"}  # frees a trapped teammate today


def load(path):
    return {(r["label"], r["seed"]): r for r in csv.DictReader(open(path))}


def hunter_side(r):
    t1, t2 = r["team1"].split("+"), r["team2"].split("+")
    if ("Hunter" in t1) == ("Hunter" in t2):
        return None
    return ("team1", t2) if "Hunter" in t1 else ("team2", t1)


def main(argv):
    min_flips = 0
    if "--min-flips" in argv:
        i = argv.index("--min-flips")
        min_flips = int(argv[i + 1])
        argv = argv[:i] + argv[i + 2:]
    before, after = load(argv[0]), load(argv[1])
    rows = defaultdict(lambda: [0, 0, 0, 0, 0])  # n, win_b, win_a, gained, lost
    for key, b in before.items():
        a = after.get(key)
        side = hunter_side(b)
        if a is None or side is None:
            continue
        me, enemy = side
        comp = "+".join(sorted(enemy))
        wb, wa = b["winner"] == me, a["winner"] == me
        row = rows[comp]
        row[0] += 1
        row[1] += wb
        row[2] += wa
        row[3] += (wa and not wb)
        row[4] += (wb and not wa)
    out = []
    for comp, (n, wb, wa, g, l) in rows.items():
        z = (g - l) / math.sqrt(g + l) if g + l else 0.0
        out.append((z, comp, n, wb, wa, g, l))
    out.sort()
    print("| enemy comp | dispeller | Rogue | n | Hunter before | after | flips +/- | z |")
    print("|---|---|---|---|---|---|---|---|")
    for z, comp, n, wb, wa, g, l in out:
        if g + l < min_flips:
            continue
        classes = comp.split("+")
        disp = "+".join(c for c in classes if c in DISPELLERS) or "-"
        rogue = "yes" if "Rogue" in classes else ""
        print(f"| {comp} | {disp} | {rogue} | {n} | {100*wb/n:.1f}% | {100*wa/n:.1f}% "
              f"| +{g}/-{l} | {z:+.1f} |")
    n = sum(r[2] for r in out); g = sum(r[5] for r in out); l = sum(r[6] for r in out)
    wb = sum(r[3] for r in out); wa = sum(r[4] for r in out)
    print(f"\nALL: n={n} Hunter {100*wb/n:.1f}% -> {100*wa/n:.1f}% flips +{g}/-{l} "
          f"z={(g-l)/math.sqrt(g+l) if g+l else 0:+.1f}; comps with any flip: "
          f"{sum(1 for r in out if r[5]+r[6])} of {len(out)}")


if __name__ == "__main__":
    main(sys.argv[1:])
