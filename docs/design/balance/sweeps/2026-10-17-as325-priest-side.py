#!/usr/bin/env python3
"""AS-325: a paired sweep's slices, scored from the side that fields the Priest.

    2026-10-17-as325-priest-side.py before.csv[.gz] after.csv[.gz]

`scripts/paired_sweep.py` reports team 1's win rate in every slice. In the
AGAINST slice that is the NON-Priest side's rate, and a draw counts for neither
side, so 100 minus that rate is not the Priest's. This script prints, per
slice, the win rate of the Priest's side (team 1 for the MIRROR and CONTROL
slices), how many winners changed, and how many matches changed only their
duration.
"""
import csv
import gzip
import sys
from collections import Counter


def rows(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return list(csv.DictReader(f))


def main(argv):
    if len(argv) != 3:
        print(__doc__)
        return 2
    before, after = rows(argv[1]), rows(argv[2])
    if len(before) != len(after):
        print("the two CSVs differ in length")
        return 1
    stats = {}
    for rb, ra in zip(before, after):
        key = ("label", "team1", "team2", "seed")
        if any(rb[k] != ra[k] for k in key):
            print("unpaired row:", [rb[k] for k in key], [ra[k] for k in key])
            return 1
        p1 = "Priest" in rb["team1"].split("+")
        p2 = "Priest" in rb["team2"].split("+")
        if p1 and p2:
            slice_name = "MIRRORED (team 1)"
        elif p1:
            slice_name = "CLEAN (Priest is team 1)"
        elif p2:
            slice_name = "AGAINST (Priest is team 2)"
        else:
            slice_name = "CONTROL (team 1)"
        side = "team2" if p2 and not p1 else "team1"
        s = stats.setdefault(slice_name, Counter())
        s["n"] += 1
        s["before"] += rb["winner"] == side
        s["after"] += ra["winner"] == side
        s["draws"] += rb["winner"] == "draw"
        if rb["winner"] != ra["winner"]:
            s["winner_changed"] += 1
        elif rb["duration_secs"] != ra["duration_secs"]:
            s["duration_only"] += 1
    for name in sorted(stats):
        s = stats[name]
        n = s["n"]
        print(f"{name}: n={n}  before {100 * s['before'] / n:.1f}%  "
              f"after {100 * s['after'] / n:.1f}%  draws(before) {s['draws']}  "
              f"winners changed {s['winner_changed']}  "
              f"duration-only changes {s['duration_only']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
