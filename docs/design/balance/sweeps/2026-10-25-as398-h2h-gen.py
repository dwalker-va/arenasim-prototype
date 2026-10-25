#!/usr/bin/env python3
"""AS-398 directional bank: this build against main, both teams TeamPlan.

    2026-10-25-as398-h2h-gen.py plain > plain.jsonl   # run on main, and on AS-398
    2026-10-25-as398-h2h-gen.py sim Mage,Druid > sim.jsonl   # run on main

`plain`: every row as is. `sim`: the same rows with AS-398's behaviour change
written in main's own controls -- TakeCover withheld from both teams, and
Peel's exceptions the shipped nine plus Paladin/Hunter and Hunter/Priest
(plus, in 3v3 rows, every peel of each class named, the format gates) for
both teams. On main, `sim` must play row for row what `plain` plays on the
AS-398 build: that is the attribution (graduation itself changes nothing).

Cells: 2v2, every team of two distinct classes with at most one healer
against every such team (30 x 30; both orders, so both seats), 3 maps, seeds
901-902 / 911-912 / 921-922; 3v3, the 0.8 sweep's 600-cell `sample3`, 3 maps,
seeds 901 / 911 / 921. Label `as398|map|t1|t2|seed` (both arms), scored by
`2026-10-25-as398-score.py`.
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("g08", os.path.join(HERE, "2026-10-25-0.8-gen.py"))
g = importlib.util.module_from_spec(spec)
spec.loader.exec_module(g)

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
NEW_GATES = ["Paladin/Hunter", "Hunter/Priest"]


def main():
    arm = sys.argv[1]
    # `sim1` / `sim2`: AS-398's behaviour for team 1 / team 2 only, main's for
    # the other -- the head-to-head, run on main, scored against `plain`.
    assert arm in ("plain", "sim", "sim1", "sim2")
    peel3 = sys.argv[2].split(",") if len(sys.argv) > 2 and sys.argv[2] else []
    teams = {"plain": (), "sim": (1, 2), "sim1": (1,), "sim2": (2,)}[arm]
    rows = []
    for size, seeds in ((2, [901, 902, 911, 912, 921, 922]), (3, [901, 911, 921])):
        for a, b in g.play_cells(size):
            for s in seeds:
                for m in g.MAPS:
                    extra = {"ai_profile": "TeamPlan"}
                    table = g.PEEL_SHIPPED + NEW_GATES
                    if size == 3:
                        for c in peel3:
                            table += [f"{c}/{x}" for x in CLASSES if f"{c}/{x}" not in table]
                    for t in teams:
                        extra[f"team{t}_withheld_plays"] = ["TakeCover"]
                    for t in teams:
                        extra[f"team{t}_exceptions"] = {"Peel": table}
                    rows.append(g.row("as398", m, a, b, s, **extra))
    g.emit(rows)


if __name__ == "__main__":
    main()
