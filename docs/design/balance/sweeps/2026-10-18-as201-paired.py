#!/usr/bin/env python3
"""AS-201 analysis: two arms of the same sweep (before, after), paired by seed.

Usage: 2026-10-18-as201-paired.py before.csv[.gz] after.csv[.gz]

Per cell: each arm's team-1/team-2/draw counts, rows that moved, cap draws.
Then, per comp and side, TeamPlan's head-to-head effect (the side given it,
against the all-Legacy cell) in each arm, and McNemar on the paired flips of
that side's wins between the arms; and the same flips for uniform TeamPlan.
"""
import csv
import gzip
import math
import sys
from collections import defaultdict


def load(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return {(r["label"], int(r["seed"])): r for r in csv.DictReader(f)}


def win(r):
    return r["winner"].strip()


def dur(r):
    return float(r["duration_secs"])


def z2(k1, n1, k2, n2):
    p = (k1 + k2) / (n1 + n2)
    if p in (0, 1):
        return 0.0
    return (k2 / n2 - k1 / n1) / math.sqrt(p * (1 - p) * (1 / n1 + 1 / n2))


def mcnemar(base, patch, keys, side):
    gain = sum(1 for k in keys if win(patch[k]) == side and win(base[k]) != side)
    loss = sum(1 for k in keys if win(base[k]) == side and win(patch[k]) != side)
    z = (gain - loss) / math.sqrt(gain + loss) if gain + loss else 0.0
    return gain, loss, z


def main(argv):
    base, patch = load(argv[0]), load(argv[1])
    labels = sorted({k[0] for k in base})
    comps = sorted({l.rsplit("_", 1)[0] for l in labels})
    keys_of = {l: [k for k in base if k[0] == l] for l in labels}

    print(f"{'cell':10} {'before t1/t2/d':>15} {'after t1/t2/d':>15} {'moved':>6} {'cap-b':>5} {'cap-a':>5}")
    tally = {}
    for l in labels:
        bt, pt = defaultdict(int), defaultdict(int)
        moved = bcap = pcap = 0
        for k in keys_of[l]:
            b, p = base[k], patch[k]
            bt[win(b)] += 1
            pt[win(p)] += 1
            moved += win(b) != win(p) or abs(dur(b) - dur(p)) > 1e-6
            bcap += win(b) == "draw" and dur(b) >= 299
            pcap += win(p) == "draw" and dur(p) >= 299
        tally[l] = (bt, pt, len(keys_of[l]))
        print(f"{l:10} {bt['team1']:>5}/{bt['team2']:>3}/{bt['draw']:>3}   "
              f"{pt['team1']:>5}/{pt['team2']:>3}/{pt['draw']:>3}   {moved:>6} {bcap:>5} {pcap:>5}")

    print("\nTeamPlan head-to-head effect (side given it vs the all-Legacy cell), per arm:")
    for c in comps:
        for side, cell in (("team1", "TL"), ("team2", "LT")):
            out = []
            for arm, idx in (("before", 0), ("after", 1)):
                ll, n_ll = tally[f"{c}_LL"][idx], tally[f"{c}_LL"][2]
                a, n = tally[f"{c}_{cell}"][idx], tally[f"{c}_{cell}"][2]
                out.append(f"{arm}: {ll[side]:>3}->{a[side]:>3} "
                           f"({100 * (a[side] / n - ll[side] / n_ll):+.0f}pt "
                           f"z={z2(ll[side], n_ll, a[side], n):+.1f})")
            g, lo, z = mcnemar(base, patch, keys_of[f"{c}_{cell}"], side)
            print(f"  {c:5} {side} TeamPlan  " + "  |  ".join(out)
                  + f"  | flips +{g}/-{lo} McNemar z={z:+.1f}")

    print("\nUniform TeamPlan (TT), after vs before, team-1 wins:")
    for c in comps:
        g, lo, z = mcnemar(base, patch, keys_of[f"{c}_TT"], "team1")
        bt, pt, n = tally[f"{c}_TT"]
        print(f"  {c:5} {bt['team1']:>3}/{n} -> {pt['team1']:>3}/{n}  flips +{g}/-{lo}  McNemar z={z:+.1f}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
