#!/usr/bin/env python3
"""AS-249: the TakeCover head-to-head, scored from the covering side.

usage: 2026-10-13-as249-cover.py cover.csv

Reads the batch CSV of 2026-10-13-as249-gen.py's `cover` mode. Each CN / NC
match is paired with its NN twin (same cell, map and seed) and scored from the
side that runs the play: team 1 in CN, team 2 in NC. Only cells whose covering
side fields a healer can move — the play is a healer's — and those are the
ones scored; the rest are reported as a control and must not move at all.
BasicArena has nothing to hide behind, so every match there must equal its
twin exactly (winner and duration).
"""
import collections
import csv
import gzip
import math
import sys

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}


def split(label):
    cell, arm = label.rsplit("#", 1)
    teams, map_name = cell.rsplit("@", 1)
    return teams, map_name, arm


def opened(path):
    return gzip.open(path, "rt") if path.endswith(".gz") else open(path)


def main(path):
    rows = list(csv.DictReader(opened(path)))
    by = {}
    for r in rows:
        teams, map_name, arm = split(r["label"])
        by[(teams, map_name, r["seed"], arm)] = r
    report = collections.defaultdict(lambda: [0, 0, 0, 0, 0])  # n, before, after, up, down
    control_moved = collections.Counter()
    basic_moved = 0
    basic_n = 0
    for (teams, map_name, seed, arm), r in by.items():
        if arm == "NN":
            continue
        twin = by.get((teams, map_name, seed, "NN"))
        if twin is None:
            continue
        side = "team1" if arm == "CN" else "team2"
        covering = r[side].split("+")
        same = (r["winner"], r["duration_secs"]) == (twin["winner"], twin["duration_secs"])
        if map_name == "BasicArena":
            basic_n += 1
            basic_moved += not same
            continue
        if not HEALERS & set(covering):
            control_moved[map_name] += not same
            continue
        won_b = twin["winner"] == side
        won_a = r["winner"] == side
        for key in ("all", next(iter(HEALERS & set(covering)))):
            s = report[key]
            s[0] += 1
            s[1] += won_b
            s[2] += won_a
            s[3] += won_a and not won_b
            s[4] += won_b and not won_a
    print(f"BasicArena: {basic_moved} of {basic_n} play matches differ from their NN twin (must be 0)")
    print(f"control (no healer on the covering side), PillaredArena: {control_moved['PillaredArena']} moved (must be 0)")
    print(f"\nPillaredArena, scored from the covering side, paired against NN:")
    print(f"{'healer':<10}{'n':>6}{'NN':>8}{'cover':>8}{'delta':>8}{'+':>6}{'-':>6}{'z':>7}")
    for key in ["Priest", "Paladin", "Shaman", "Druid", "all"]:
        n, b, a, up, down = report[key]
        if not n:
            continue
        z = (up - down) / math.sqrt(up + down) if up + down else 0.0
        print(f"{key:<10}{n:>6}{100*b/n:>7.1f}%{100*a/n:>7.1f}%{100*(a-b)/n:>+7.1f}{up:>6}{down:>6}{z:>7.2f}")
    # A lone healer that hides can hold a lost fight to the cap: count the
    # draws (no winner) and the mean duration per arm.
    print("\nPillaredArena, every cell, per arm:")
    for arm in ["NN", "CN", "NC"]:
        arm_rows = [r for (t, m, s_, a_), r in by.items() if m == "PillaredArena" and a_ == arm]
        draws = sum(r["winner"] not in ("team1", "team2") for r in arm_rows)
        mean = sum(float(r["duration_secs"]) for r in arm_rows) / max(len(arm_rows), 1)
        print(f"  {arm}: {len(arm_rows)} matches, {draws} without a winner, mean {mean:.1f}s")


if __name__ == "__main__":
    main(sys.argv[1])
