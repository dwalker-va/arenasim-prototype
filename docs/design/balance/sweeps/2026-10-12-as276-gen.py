#!/usr/bin/env python3
"""AS-276 sweep batches.

usage: 2026-10-12-as276-gen.py h2h <seeds> > h2h.jsonl
       2026-10-12-as276-gen.py legacy <seeds> > legacy.jsonl
       2026-10-12-as276-gen.py teamplan <seeds> > teamplan.jsonl

h2h — the Chain play head-to-head. Every 2v2 team of two distinct classes
(no double healer: 30 teams) against every other, 900 ordered cells, on
PillaredArena and BasicArena, BOTH teams under TeamPlan, in three arms per
seed:

  NN  the play withheld from both teams (the paired baseline)
  CN  team 1 chains, team 2 does not
  NC  team 2 chains, team 1 does not

Every class chains in every arm that chains: `chain_exceptions: []` lifts the
shipped exceptions (`situation::CHAIN_EXCEPTIONS`), so the run measures each
class's chain, including one the play is gated off for. That is how a
milestone sweep re-checks an exception. The Burst and Peel plays run as
shipped in every arm: the only difference between a pair is one team's Chain.

A label is `<team1>_vs_<team2>@<map>#<arm>`; the seeds are shared by the arms,
so each CN / NC match has a twin in NN that differs only by one team's Chain.
The analysis (2026-10-12-as276-oriented.py) scores each from the CHAINING
side.

legacy — the byte-identity batch: the same 900 cells and both maps under
Legacy, for a main-vs-branch comparison.

teamplan — the same cells under plain TeamPlan with no measurement fields:
main's arm for the control against the branch's NN rows (main has no Chain
to withhold, and would refuse the field).
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
                if mode == "teamplan":
                    print(json.dumps({**common, "label": base, "ai_profile": "TeamPlan"}))
                    continue
                for arm, w1, w2 in [("NN", True, True), ("CN", False, True), ("NC", True, False)]:
                    print(json.dumps({
                        **common,
                        "label": f"{base}#{arm}",
                        "ai_profile": "TeamPlan",
                        "team1_withheld_plays": ["Chain"] if w1 else [],
                        "team2_withheld_plays": ["Chain"] if w2 else [],
                        "chain_exceptions": [],
                    }))


if __name__ == "__main__":
    main(*sys.argv[1:])
