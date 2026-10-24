#!/usr/bin/env python3
"""AS-373 head-to-head banks. Every 2v2 healer team against every 2v2 team
(no double healers) and the six one-healer 3v3 teams against each other, on
Nagrand and TwinPillars, seeds 1..N.

    2026-10-23-as373-h2h-gen.py modes <arm> N      arm: off | t1 | t2   (both teams TeamPlan;
                                   the named team keeps the pillar modes, the
                                   other plays without them)
    2026-10-23-as373-h2h-gen.py legacy <seat> N    seat: t1 | t2   (the named team TeamPlan,
                                   the other Legacy; no pillar fields, so main
                                   runs it too)
"""
import itertools
import json
import sys

DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
MAPS = ["PillaredArena", "TwinPillars"]
THREES = [
    ["Warrior", "Mage", "Priest"],
    ["Rogue", "Warlock", "Druid"],
    ["Warrior", "Hunter", "Paladin"],
    ["Rogue", "Mage", "Shaman"],
    ["Warlock", "Hunter", "Priest"],
    ["Warrior", "Rogue", "Druid"],
]

kind, arm, n = sys.argv[1], sys.argv[2], int(sys.argv[3])
healer_teams = [[d, h] for d in DPS for h in HEALERS]
all_teams = healer_teams + [list(p) for p in itertools.combinations(DPS, 2)]
cells = []
for m in MAPS:
    for t1 in healer_teams:
        for t2 in all_teams:
            cells.append((m, t1, t2))
    for t1, t2 in itertools.permutations(THREES, 2):
        cells.append((m, t1, t2))

for m, t1, t2 in cells:
    for s in range(1, n + 1):
        row = {
            "team1": t1,
            "team2": t2,
            "map": m,
            "random_seed": s,
            "max_duration_secs": 300,
            "label": f"{m}:{'+'.join(t1)}:{'+'.join(t2)}:{s}",
        }
        if kind == "modes":
            row["ai_profile"] = "TeamPlan"
            row["team1_pillar_modes"] = arm == "t1"
            row["team2_pillar_modes"] = arm == "t2"
        else:
            row["team1_ai_profile"] = "TeamPlan" if arm == "t1" else "Legacy"
            row["team2_ai_profile"] = "TeamPlan" if arm == "t2" else "Legacy"
        print(json.dumps(row))
