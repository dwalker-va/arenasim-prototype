#!/usr/bin/env python3
"""AS-164: build the TeamPlan bisect table from per-commit batch CSVs.

    bisect-table.py out.csv <commit>=<batch.csv> ...

Each batch CSV is one commit's run of `2026-10-10-as164-bisect.jsonl` (cells
`hunter` and `wmp`, seeds 1-200); the measured commit's rows come from the
head-to-head CSV, cut to the same cells, arms and seeds. Writes one row per match:
commit, cell, arm, side, seed, side_won — where `side` is the comp the
TeamPlan effect is QUOTED for (Rogue+Priest, the team-2 side given TeamPlan
in the hunter cell; Warrior+Mage+Priest, the team-1 side in the 3v3 cell),
and `side_won` is 1 when that comp won. `analyze.py bisect out.csv` prints
the per-commit effects.
"""
import csv
import sys

QUOTED = {"hunter": (2, "Rogue+Priest"), "wmp": (1, "Warrior+Mage+Priest")}
ARMS = {("hunter", "LL"), ("hunter", "LT"), ("wmp", "LL"), ("wmp", "TL")}


def main(argv):
    out = argv[0]
    with open(out, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["commit", "cell", "arm", "side", "seed", "side_won"])
        for spec in argv[1:]:
            commit, path = spec.split("=", 1)
            for r in csv.DictReader(open(path)):
                cell, arm = r["label"].rsplit("_", 1)
                # The head-to-head CSV carries more cells, arms and seeds than
                # the bisect input; keep exactly the bisect's.
                if (cell, arm) not in ARMS or int(r["seed"]) > 200:
                    continue
                side_no, side = QUOTED[cell]
                w.writerow([commit, cell, arm, side, r["seed"], int(r["winner"] == f"team{side_no}")])


if __name__ == "__main__":
    main(sys.argv[1:])
