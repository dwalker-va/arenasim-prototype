#!/usr/bin/env python3
"""AS-372 sweep batch: the TakeCover projection fix, head-to-head.

usage: 2026-10-18-as372-gen.py <seeds-per-pillar-map> <seeds-basicarena> > sweep.jsonl

The `scripts/headtohead_sweep.py` design (LL / TL / LT: the profile under test
on one side, `Legacy` on the other, both assignments) over every 2v2 team of
two distinct classes (no double healer: 30 teams) against every other, 900
ordered cells, on PillaredArena and TwinPillars, plus BasicArena as the
no-obstacle control. Run once per binary (`main` and the branch); every match
has a twin in the other arm at the same label and seed.

  LL  both teams Legacy           — must be byte-identical across binaries
  TL  team 1 TeamPlan, team 2 Legacy
  LT  team 1 Legacy, team 2 TeamPlan

TakeCover is a TeamPlan play, so the fix reaches only TL / LT, and only on a
map with obstacles; BasicArena TL / LT must also be identical across binaries.
The analysis (2026-10-18-as372-score.py) scores each TL / LT match from the
TeamPlan (covering) side, before vs after.
"""
import itertools
import json
import sys

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}


def teams():
    for pair in itertools.combinations(CLASSES, 2):
        if not (pair[0] in HEALERS and pair[1] in HEALERS):
            yield list(pair)


def main(pillar_seeds, basic_seeds):
    plan = [("PillaredArena", int(pillar_seeds)), ("TwinPillars", int(pillar_seeds)),
            ("BasicArena", int(basic_seeds))]
    for map_name, seeds in plan:
        for t1 in teams():
            for t2 in teams():
                base = f"{'+'.join(t1)}_vs_{'+'.join(t2)}@{map_name}"
                for seed in range(1, seeds + 1):
                    for arm, p1, p2 in [("LL", "Legacy", "Legacy"),
                                        ("TL", "TeamPlan", "Legacy"),
                                        ("LT", "Legacy", "TeamPlan")]:
                        print(json.dumps({
                            "team1": t1,
                            "team2": t2,
                            "map": map_name,
                            "random_seed": seed,
                            "max_duration_secs": 300.0,
                            "label": f"{base}#{arm}",
                            "team1_ai_profile": p1,
                            "team2_ai_profile": p2,
                        }))


if __name__ == "__main__":
    main(*sys.argv[1:3])
