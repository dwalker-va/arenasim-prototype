#!/usr/bin/env python3
"""Control equivalence on main: a play withheld from both teams against the
same play excepted everywhere on both, over a probe's bank.

    ctrl_gen.py <play> <withheld|excepted> > bank.jsonl
"""
import json
import sys

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
BANKS = {
    "Chain": ([
        (["Mage", "Paladin"], ["Warrior", "Priest"]),
        (["Rogue", "Paladin"], ["Mage", "Shaman"]),
        (["Warlock", "Druid"], ["Rogue", "Priest"]),
        (["Mage", "Druid"], ["Hunter", "Paladin"]),
        (["Rogue", "Mage"], ["Warlock", "Priest"]),
        (["Warlock", "Paladin"], ["Mage", "Druid"]),
        (["Rogue", "Mage", "Priest"], ["Warrior", "Warlock", "Paladin"]),
        (["Warlock", "Mage", "Druid"], ["Rogue", "Hunter", "Priest"]),
    ], ["BasicArena", "PillaredArena"], range(1, 25)),
    "Peel": ([
        (["Mage", "Priest"], ["Warrior", "Druid"]),
        (["Warlock", "Shaman"], ["Rogue", "Priest"]),
        (["Hunter", "Paladin"], ["Warrior", "Mage"]),
        (["Rogue", "Druid"], ["Hunter", "Priest"]),
        (["Warrior", "Shaman"], ["Rogue", "Warlock"]),
        (["Paladin", "Mage"], ["Warrior", "Hunter"]),
    ], ["PillaredArena", "BasicArena"], range(1, 17)),
}

BANKS["Burst"] = BANKS["Chain"]
BANKS["Hunt"] = BANKS["Chain"]
play, arm = sys.argv[1], sys.argv[2]
comps, maps, seeds = BANKS[play]
if play == "Peel":
    every = [f"{c}/{x}" for c in CLASSES for x in CLASSES]
else:
    every = list(CLASSES)
for t1, t2 in comps:
    for m in maps:
        for s in seeds:
            row = {"team1": t1, "team2": t2, "map": m, "random_seed": s,
                   "ai_profile": "TeamPlan",
                   "label": f"ctrl|{m}|{'+'.join(t1)}|{'+'.join(t2)}|{s}"}
            if arm == "withheld":
                row["team1_withheld_plays"] = [play]
                row["team2_withheld_plays"] = [play]
            else:
                row["team1_exceptions"] = {play: every}
                row["team2_exceptions"] = {play: every}
            print(json.dumps(row))
