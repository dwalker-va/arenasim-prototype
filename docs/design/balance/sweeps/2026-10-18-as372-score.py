#!/usr/bin/env python3
"""AS-372: the TakeCover projection fix, scored from the TeamPlan side.

usage: 2026-10-18-as372-score.py before.csv[.gz] after.csv[.gz]

Reads the two arms of 2026-10-18-as372-gen.py's batch (`main` and the branch).
Every match is paired with its twin in the other arm (same label and seed).

1. Identity checks, which must be 0: every LL match (Legacy on both sides,
   every map), every BasicArena match (no obstacles) and every TL / LT match
   whose TeamPlan side fields no healer equal their twin in winner, duration
   and end reason. Non-vacuity is printed beside each.
2. The fix's effect, PAIRED before vs after: each TL / LT match on a pillar map
   scored from the TeamPlan side (team 1 in TL, team 2 in LT), only where that
   side fields a healer (the play is a healer's). McNemar on the flips.
3. Each arm's TeamPlan head-to-head gain over its own LL baseline, from the
   TeamPlan side (the headtohead_sweep.py figure), for context.
"""
import collections
import csv
import gzip
import math
import sys

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
SAME = ("winner", "duration_secs", "end_reason")


def opened(path):
    return gzip.open(path, "rt") if path.endswith(".gz") else open(path)


def load(path):
    out = {}
    for r in csv.DictReader(opened(path)):
        out[(r["label"], r["seed"])] = r
    return out


def split(label):
    cell, arm = label.rsplit("#", 1)
    teams, map_name = cell.rsplit("@", 1)
    return teams, map_name, arm


def mcnemar(up, down):
    return (up - down) / math.sqrt(up + down) if up + down else 0.0


def main(before_path, after_path):
    before, after = load(before_path), load(after_path)
    assert before.keys() == after.keys(), "the arms ran different batches"
    keys = [k for k in SAME if k in next(iter(before.values()))]

    ll = collections.Counter()
    basic = collections.Counter()
    fx = collections.defaultdict(lambda: [0, 0, 0, 0, 0])  # n, before, after, up, down
    moved = collections.Counter()
    control = collections.Counter()
    draws = collections.defaultdict(lambda: [0, 0.0])  # without a winner, total duration
    gain = collections.defaultdict(lambda: [0, 0, 0, 0])  # per arm: n, LL wins, T wins
    for (label, seed), b in before.items():
        a = after[(label, seed)]
        teams, map_name, arm = split(label)
        same = all(a[k] == b[k] for k in keys)
        if arm == "LL":
            ll["n"] += 1
            ll["moved"] += not same
            ll["kills"] += b["winner"] in ("team1", "team2")
            continue
        if map_name == "BasicArena":
            basic["n"] += 1
            basic["moved"] += not same
            basic["kills"] += b["winner"] in ("team1", "team2")
            continue
        side = "team1" if arm == "TL" else "team2"
        if not HEALERS & set(b[side].split("+")):
            control["n"] += 1
            control["moved"] += not same
            continue
        moved[map_name] += not same
        for name, r in (("main", b), ("branch", a)):
            d = draws[(name, map_name)]
            d[0] += r["winner"] not in ("team1", "team2")
            d[1] += float(r["duration_secs"])
        won_b, won_a = b["winner"] == side, a["winner"] == side
        for key in (map_name, "all"):
            s = fx[key]
            s[0] += 1
            s[1] += won_b
            s[2] += won_a
            s[3] += won_a and not won_b
            s[4] += won_b and not won_a
        twin = (label.rsplit("#", 1)[0] + "#LL", seed)
        for name, rows in (("main", before), ("branch", after)):
            g = gain[(name, map_name)]
            g[0] += 1
            g[1] += rows[twin]["winner"] == side
            g[2] += rows[(label, seed)]["winner"] == side

    print(f"columns compared: {', '.join(keys)}")
    print(f"LL (Legacy both sides, all maps): {ll['moved']} of {ll['n']} differ (must be 0); "
          f"{ll['kills']} decided by a kill")
    print(f"BasicArena TL/LT (no obstacles): {basic['moved']} of {basic['n']} differ (must be 0); "
          f"{basic['kills']} decided by a kill")
    print(f"control (TeamPlan side with no healer, pillar maps): {control['moved']} of "
          f"{control['n']} differ (must be 0)")
    print("\nPillar maps, TeamPlan side with a healer, main -> branch (paired):")
    print(f"{'map':<15}{'n':>6}{'moved':>7}{'main':>8}{'branch':>8}{'delta':>8}{'+':>5}{'-':>5}{'z':>7}")
    for key in ("PillaredArena", "TwinPillars", "all"):
        n, wb, wa, up, down = fx[key]
        if not n:
            continue
        mv = moved[key] if key != "all" else sum(moved.values())
        print(f"{key:<15}{n:>6}{mv:>7}{100*wb/n:>7.1f}%{100*wa/n:>7.1f}%{100*(wa-wb)/n:>+7.1f}"
              f"{up:>5}{down:>5}{mcnemar(up, down):>7.2f}")
    print("\nPillar maps, TeamPlan side with a healer, matches without a winner / mean duration:")
    for (name, map_name), (nd, total) in sorted(draws.items()):
        n = fx[map_name][0]
        print(f"  {name:<7}{map_name:<15} {nd:>4} without a winner, mean {total / n:.1f}s")
    print("\nTeamPlan head-to-head gain over LL, TeamPlan side with a healer:")
    for (name, map_name), (n, l_wins, t_wins, _) in sorted(gain.items()):
        print(f"  {name:<7}{map_name:<15} n={n:>5}  LL {100*l_wins/n:5.1f}%  "
              f"TeamPlan {100*t_wins/n:5.1f}%  gain {100*(t_wins-l_wins)/n:+5.1f}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
