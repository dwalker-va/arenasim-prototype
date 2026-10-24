#!/usr/bin/env python3
"""AS-373 healer pillar-play bank: every 2v2 team with a healer against every
2v2 team (no double healers), plus six one-healer 3v3 teams against each other,
on both pillar maps, one profile for both teams.

    2026-10-19-as373-gen.py <profile> <seeds-2v2> <seeds-3v3> > bank.jsonl

Fed to `tests/healer_pillar_modes.rs` (HPM_IN). Both teams run the same profile:
the bank describes each profile's own world, not a head-to-head.
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

profile, s2, s3 = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
healer_teams = [[d, h] for d in DPS for h in HEALERS]
all_teams = healer_teams + [list(p) for p in itertools.combinations(DPS, 2)]


def emit(m, t1, t2, s):
    print(json.dumps({
        "team1": t1, "team2": t2, "map": m,
        "ai_profile": profile, "random_seed": s,
        "max_duration_secs": 300,
        "label": f"{profile}:{m}:{'+'.join(t1)}:{'+'.join(t2)}:{s}",
    }))


for m in MAPS:
    for t1 in healer_teams:
        for t2 in all_teams:
            for s in range(1, s2 + 1):
                emit(m, t1, t2, s)
    for t1, t2 in itertools.permutations(THREES, 2):
        for s in range(1, s3 + 1):
            emit(m, t1, t2, s)
