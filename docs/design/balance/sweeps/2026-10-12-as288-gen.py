#!/usr/bin/env python3
"""AS-288 head-to-head batch: every 2v2 team against every other, no double
healers, on PillaredArena (`P`) and BasicArena (`B`).

usage: 2026-10-12-as288-gen.py <seeds per cell> > h2h.jsonl

Three assignments per map, each labelled `<team1>_vs_<team2>#<map>_<xx>`:

- `TL`: team 1 `TeamPlan`, team 2 `Legacy`;
- `LT`: the reverse;
- `LL`: both `Legacy`, ONE seed per cell — the control. Nothing `Legacy`
  reads changed, so every `LL` row must match between the arms.
"""
import itertools
import json
import sys

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
MAPS = {"P": "PillaredArena", "B": "BasicArena"}
PROFILES = {"TL": ("TeamPlan", "Legacy"), "LT": ("Legacy", "TeamPlan"), "LL": ("Legacy", "Legacy")}


def teams():
    for a, b in itertools.combinations_with_replacement(CLASSES, 2):
        if a in HEALERS and b in HEALERS:
            continue
        yield [a, b]


def main(seeds):
    for m, map_name in MAPS.items():
        for assign, (p1, p2) in PROFILES.items():
            n = 1 if assign == "LL" else seeds
            for t1 in teams():
                for t2 in teams():
                    label = f"{'+'.join(t1)}_vs_{'+'.join(t2)}#{m}_{assign}"
                    for seed in range(n):
                        print(json.dumps({
                            "team1": t1,
                            "team2": t2,
                            "random_seed": seed,
                            "max_duration_secs": 300.0,
                            "label": label,
                            "map": map_name,
                            "team1_ai_profile": p1,
                            "team2_ai_profile": p2,
                        }))


if __name__ == "__main__":
    main(int(sys.argv[1]))
