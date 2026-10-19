#!/usr/bin/env python3
"""AS-362 healer CC-exposure bank: every 2v2 team with a healer against every
2v2 team (no double healers), both maps, one profile for both teams.

    2026-10-18-as362-gen.py <profile> <seeds> > bank.jsonl

Fed to `tests/healer_cc_exposure.rs` (CCX_IN). Both teams run the same profile:
the bank describes each profile's own world, not a head-to-head.
"""
import itertools
import json
import sys

DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
MAPS = ["BasicArena", "PillaredArena"]

profile = sys.argv[1]
seeds = int(sys.argv[2])
healer_teams = [[d, h] for d in DPS for h in HEALERS]
all_teams = healer_teams + [list(p) for p in itertools.combinations(DPS, 2)]
for m in MAPS:
    for t1 in healer_teams:
        for t2 in all_teams:
            for s in range(1, seeds + 1):
                print(json.dumps({
                    "team1": t1, "team2": t2, "map": m,
                    "ai_profile": profile, "random_seed": s,
                    "max_duration_secs": 300,
                    "label": f"{profile}:{m}:{'+'.join(t1)}:{'+'.join(t2)}:{s}",
                }))
