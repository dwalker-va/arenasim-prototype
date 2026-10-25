#!/usr/bin/env python3
"""AS-398: the Peel play's 3v3 format gates, measured on main (9050d193).

    2026-10-25-as398-peel3-gen.py <arm> <seeds> > bank.jsonl
    ONLY=Mage,Druid 2026-10-25-as398-peel3-gen.py - 871-876 > bank.jsonl

arm: `-` (the shipped Peel gates), or a peeling class (`Mage`) whose every
peel is added to the running side's table (`Mage/<every class>`) -- in a
3v3-only bank exactly "the class does not peel on a team of three". Rows: the
0.8 sweep's 600-cell `sample3`, PN (team 1 runs Peel, team 2 has it
withheld) and NP (the mirror), all three maps, both teams TeamPlan. With
`ONLY` set, only the rows whose running team holds one of those classes (the
rest play the same in every arm). Label `peel3-<arm>-<PN|NP>|map|t1|t2|seed`,
scored by `2026-10-25-0.8-score.py pair`.

Selection ran seeds 801,811,821 (the 0.8 sweep's 3v3 blocks); confirmation
871-876 (two per block).
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("g08", os.path.join(HERE, "2026-10-25-0.8-gen.py"))
g = importlib.util.module_from_spec(spec)
spec.loader.exec_module(g)

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]


def main():
    arm, ss = sys.argv[1], g.seeds(sys.argv[2])
    table = list(g.PEEL_SHIPPED)
    if arm != "-":
        assert arm in CLASSES
        table += [f"{arm}/{c}" for c in CLASSES if f"{arm}/{c}" not in table]
    only = set(os.environ.get("ONLY", "").split(",")) - {""}
    rows = []
    for a, b in g.sample3():
        for s in ss:
            for tag, runs in (("PN", 1), ("NP", 2)):
                if only and not only & set(a if runs == 1 else b):
                    continue
                for m in g.MAPS:
                    extra = {"ai_profile": "TeamPlan",
                             f"team{3 - runs}_withheld_plays": ["Peel"],
                             f"team{runs}_exceptions": {"Peel": table}}
                    rows.append(g.row(f"peel3-{arm}-{tag}", m, a, b, s, **extra))
    g.emit(rows)


if __name__ == "__main__":
    main()
