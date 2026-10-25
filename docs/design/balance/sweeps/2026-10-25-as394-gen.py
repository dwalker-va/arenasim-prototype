#!/usr/bin/env python3
"""AS-394 Hunt banks: AS-385's package cells (every 2v2 healer team against
every 2v2 team, no double healers, plus the six one-healer 3v3 teams in both
orders, on Nagrand and TwinPillars), both teams `TeamPlan`.

    2026-10-25-as394-gen.py <arm> <seeds> [nofocus]

seeds: an inclusive range, e.g. 701-704.

arms (one binary runs all four; `team{n}_withheld_plays: ["Hunt"]` withholds
the play from that team):
  off   neither team hunts
  t1    only team 1 does
  t2    only team 2 does
  uni   both do

`nofocus` also switches the AS-389 target focus off for both teams
(`team{n}_target_focus: false`): the hunt measured without the focus.

Run each bank through `tests/hunt_endgame_sweep.rs` (HUNT_IN / HUNT_OUT),
then score with `2026-10-25-as394-score.py`.
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

arm, seeds = sys.argv[1], sys.argv[2]
nofocus = len(sys.argv) > 3 and sys.argv[3] == "nofocus"
if arm not in ("off", "t1", "t2", "uni"):
    sys.exit(f"unknown arm {arm}")
lo, hi = (int(x) for x in seeds.split("-"))
healer_teams = [[d, h] for d in DPS for h in HEALERS]
all_teams = healer_teams + [list(p) for p in itertools.combinations(DPS, 2)]
cells = []
for m in MAPS:
    for t1 in healer_teams:
        for t2 in all_teams:
            cells.append((m, t1, t2))
    for t1, t2 in itertools.permutations(THREES, 2):
        cells.append((m, t1, t2))

on = {"off": (), "t1": (1,), "t2": (2,), "uni": (1, 2)}[arm]
for m, t1, t2 in cells:
    for s in range(lo, hi + 1):
        row = {
            "team1": t1,
            "team2": t2,
            "map": m,
            "random_seed": s,
            "max_duration_secs": 300,
            "label": f"{m}:{'+'.join(t1)}:{'+'.join(t2)}:{s}",
            "ai_profile": "TeamPlan",
        }
        for team in (1, 2):
            if team not in on:
                row[f"team{team}_withheld_plays"] = ["Hunt"]
            if nofocus:
                row[f"team{team}_target_focus"] = False
        print(json.dumps(row))
