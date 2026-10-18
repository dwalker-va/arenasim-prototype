#!/usr/bin/env python3
"""AS-201 sweep input: five comps x four profile cells, PillaredArena.

Usage: 2026-10-18-as201-make-sweep.py SEEDS SEED_BASE OUT.jsonl
The published run is `300 1` (seeds 1-300, 6,000 configs).
"""
import json
import sys

COMPS = [
    ("flag", ["Warrior", "Priest"], ["Warlock", "Priest"]),
    ("hunt", ["Hunter", "Priest"], ["Rogue", "Priest"]),
    ("3v3", ["Warrior", "Mage", "Priest"], ["Rogue", "Warlock", "Paladin"]),
    ("pala", ["Warrior", "Paladin"], ["Warlock", "Priest"]),
    ("drui", ["Warrior", "Druid"], ["Warlock", "Priest"]),
]
CELLS = [
    ("LL", "Legacy", "Legacy"),
    ("TL", "TeamPlan", "Legacy"),
    ("LT", "Legacy", "TeamPlan"),
    ("TT", "TeamPlan", "TeamPlan"),
]


def main(argv):
    seeds, base, path = int(argv[0]), int(argv[1]), argv[2]
    with open(path, "w") as out:
        for name, t1, t2 in COMPS:
            for cell, p1, p2 in CELLS:
                for s in range(base, base + seeds):
                    out.write(json.dumps({
                        "label": f"{name}_{cell}", "team1": t1, "team2": t2,
                        "map": "PillaredArena", "max_duration_secs": 300.0,
                        "random_seed": s, "team1_ai_profile": p1, "team2_ai_profile": p2,
                    }) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
