#!/usr/bin/env python3
"""AS-391 bank: the Pillar stance against Chain.

    2026-10-24-as391-chain-gen.py <arm> <seeds>

seeds: an inclusive range, e.g. 601-608.

The measured team runs TeamPlan against a Legacy opponent, in both seats:
every 2v2 healer team with a Druid or a Mage (a hand-off holder; a lock
source in itself) against every 2v2 team, and the one-healer 3v3 teams with
a Druid or a Mage against the other AS-385 3v3 teams, on Nagrand
(PillaredArena) and TwinPillars. A DPS-only team never holds a stance, so it
is left out.

arms:
  on    the measured team plays as shipped (Pillar ahead of Chain). Run on
        the order-swap binary (2026-10-24-as391-chain-before-pillar.patch)
        for the swap arm.
  off   the Pillar play is withheld from the measured team.
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


def holds_handoff(team):
    return "Druid" in team or "Mage" in team


arm, seeds = sys.argv[1], sys.argv[2]
assert arm in ("on", "off"), arm
lo, hi = (int(x) for x in seeds.split("-"))
healer_teams = [[d, h] for d in DPS for h in HEALERS]
all_teams = healer_teams + [list(p) for p in itertools.combinations(DPS, 2)]
pairs = []
for me in healer_teams:
    if holds_handoff(me):
        for opp in all_teams:
            pairs.append((me, opp))
for me in THREES:
    if holds_handoff(me):
        for opp in THREES:
            if opp != me:
                pairs.append((me, opp))

for m in MAPS:
    for me, opp in pairs:
        for seat in (1, 2):
            t1, t2 = (me, opp) if seat == 1 else (opp, me)
            for s in range(lo, hi + 1):
                row = {
                    "team1": t1,
                    "team2": t2,
                    "map": m,
                    "random_seed": s,
                    "max_duration_secs": 300,
                    "team1_ai_profile": "TeamPlan" if seat == 1 else "Legacy",
                    "team2_ai_profile": "TeamPlan" if seat == 2 else "Legacy",
                    "label": f"{m}:{'+'.join(t1)}:{'+'.join(t2)}:{s}:t{seat}",
                }
                if arm == "off":
                    row[f"team{seat}_withheld_plays"] = ["Pillar"]
                print(json.dumps(row))
