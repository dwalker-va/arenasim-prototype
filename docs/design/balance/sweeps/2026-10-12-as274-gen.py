#!/usr/bin/env python3
"""AS-274 sweep batches.

usage: 2026-10-12-as274-gen.py h2h <seeds> > h2h.jsonl
       2026-10-12-as274-gen.py legacy <seeds> > legacy.jsonl

h2h — the Peel play head-to-head. Every 2v2 team of two distinct classes
(no double healer: 30 teams, each with at least one peel tool, since only the
Warrior has none) against every other, 900 ordered cells, on PillaredArena and
BasicArena, BOTH teams under TeamPlan, in three arms per seed:

  NN  the play withheld from both teams (the paired baseline)
  PN  team 1 peels, team 2 does not
  NP  team 2 peels, team 1 does not

A label is `<team1>_vs_<team2>@<map>#<arm>`; the seeds are shared by the arms,
so each PN / NP match has a twin in NN that differs only by one team's Peel.
The analysis (2026-10-12-as274-oriented.py) scores each from the PEELING
side.

legacy — the byte-identity batch: the same 900 cells and both maps under
Legacy, for a main-vs-branch comparison.
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


def cells():
    for t1 in teams():
        for t2 in teams():
            yield t1, t2


def main(mode, seeds):
    seeds = int(seeds)
    for map_name in MAPS:
        for t1, t2 in cells():
            base = f"{'+'.join(t1)}_vs_{'+'.join(t2)}@{map_name}"
            for seed in range(1, seeds + 1):
                common = {
                    "team1": t1,
                    "team2": t2,
                    "map": map_name,
                    "random_seed": seed,
                    "max_duration_secs": 300.0,
                }
                if mode == "legacy":
                    print(json.dumps({**common, "label": base, "ai_profile": "Legacy"}))
                    continue
                for arm, w1, w2 in [("NN", True, True), ("PN", False, True), ("NP", True, False)]:
                    print(json.dumps({
                        **common,
                        "label": f"{base}#{arm}",
                        "ai_profile": "TeamPlan",
                        "team1_withheld_plays": ["Peel"] if w1 else [],
                        "team2_withheld_plays": ["Peel"] if w2 else [],
                    }))


if __name__ == "__main__":
    main(*sys.argv[1:])
