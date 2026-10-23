#!/usr/bin/env python3
"""AS-292 reachable-cell head-to-head (round 2, on top of AS-362).

    2026-10-19-as292-gen.py <seeds per cell> <control seeds per cell> > h2h.jsonl

The rule reaches only a `TeamPlan` team facing an enemy HEALER, and its
effect lands through that team's healer (the PRESSURED posture) and the
`ally_in_danger` window. So every team is one DPS plus a Priest, Shaman or
Druid (the three healers that can stand in the 28-30yd band; the Paladin is
melee), and every such team meets every other, mirrors included: 15 x 15
cells, on BasicArena (`B`) and PillaredArena (`P`).

Labels are `<team1>_vs_<team2>#<map>_<xx>`:

- `TL`: team 1 `TeamPlan`, team 2 `Legacy`;
- `LT`: the reverse;
- `LL`: both `Legacy` — the CONTROL. Nothing `Legacy` reads changed, so
  every `LL` row must be identical between the arms.
"""
import json
import sys

DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
HEALERS = ["Priest", "Shaman", "Druid"]
MAPS = {"B": "BasicArena", "P": "PillaredArena"}
PROFILES = {"TL": ("TeamPlan", "Legacy"), "LT": ("Legacy", "TeamPlan"), "LL": ("Legacy", "Legacy")}


def main(seeds, control_seeds):
    teams = [[d, h] for d in DPS for h in HEALERS]
    for m, map_name in MAPS.items():
        for assign, (p1, p2) in PROFILES.items():
            n = control_seeds if assign == "LL" else seeds
            for t1 in teams:
                for t2 in teams:
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
    main(int(sys.argv[1]), int(sys.argv[2]))
