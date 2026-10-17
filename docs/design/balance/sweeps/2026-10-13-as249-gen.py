#!/usr/bin/env python3
"""AS-249 sweep batches.

usage: 2026-10-13-as249-gen.py cover <seeds> > cover.jsonl

cover — the TakeCover play head-to-head (the AS-275 three-arm design). Every
2v2 team of two distinct classes (no double healer: 30 teams) against every
other, 900 ordered cells, on PillaredArena and BasicArena, BOTH teams under
TeamPlan, in three arms per seed:

  NN  the play withheld from both teams (the paired baseline)
  CN  team 1 takes cover, team 2 does not
  NC  team 2 takes cover, team 1 does not

A label is `<team1>_vs_<team2>@<map>#<arm>`; the seeds are shared by the arms,
so each CN / NC match has a twin in NN that differs only by one team's play.
The analysis (2026-10-13-as249-cover.py) scores each from the COVERING side.
BasicArena has nothing to hide behind, so there every arm must equal NN.

The Legacy before/after sweep (the onset gap, the closing rung and the Druid's
Mark) is `scripts/gen_sweep.py --full 2 --exclude-double-healer --affects
Priest,Paladin,Shaman,Druid --control-cells 8 --n 10`, once plain and once with
`--extra '{"map":"PillaredArena"}' --label-suffix nagrand`, scored from each
healer's side by 2026-10-13-as249-healer-side.py.
"""
import itertools
import json
import sys

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
MAPS = ["PillaredArena", "BasicArena"]


def teams():
    for pair in itertools.combinations(CLASSES, 2):
        if not (pair[0] in HEALERS and pair[1] in HEALERS):
            yield list(pair)


def main(mode, seeds):
    assert mode == "cover", mode
    seeds = int(seeds)
    for map_name in MAPS:
        for t1 in teams():
            for t2 in teams():
                base = f"{'+'.join(t1)}_vs_{'+'.join(t2)}@{map_name}"
                for seed in range(1, seeds + 1):
                    for arm, w1, w2 in [("NN", True, True), ("CN", False, True), ("NC", True, False)]:
                        print(json.dumps({
                            "team1": t1,
                            "team2": t2,
                            "map": map_name,
                            "random_seed": seed,
                            "max_duration_secs": 300.0,
                            "label": f"{base}#{arm}",
                            "ai_profile": "TeamPlan",
                            "team1_withheld_plays": ["TakeCover"] if w1 else [],
                            "team2_withheld_plays": ["TakeCover"] if w2 else [],
                        }))


if __name__ == "__main__":
    main(*sys.argv[1:3])
