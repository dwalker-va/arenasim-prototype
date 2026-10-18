#!/usr/bin/env python3
"""AS-360: a paired sweep's slices, scored from the side that fields the Warlock,
with the draw rate before and after.

    2026-10-17-as360-warlock-side.py before.csv[.gz] after.csv[.gz]

The change reaches every non-healer, so `scripts/paired_sweep.py` can only
report one MIRRORED slice. This script splits the matches by where the Warlock
stands and prints, per slice, the Warlock side's win rate (team 1 where no side
or both sides field one), the draw count, how many winners changed (and which
way for the Warlock's side), and how many matches changed only their duration.
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
        w1 = "Warlock" in rb["team1"].split("+")
        w2 = "Warlock" in rb["team2"].split("+")
        if w1 and w2:
            slice_name = "MIRROR (both field one; team 1)"
        elif w1:
            slice_name = "WARLOCK team 1"
        elif w2:
            slice_name = "WARLOCK team 2"
        else:
            slice_name = "NO WARLOCK (team 1)"
        side = "team2" if w2 and not w1 else "team1"
        for name in (slice_name, "ALL"):
            s = stats.setdefault(name, Counter())
            s["n"] += 1
            s["before"] += rb["winner"] == side
            s["after"] += ra["winner"] == side
            s["draws_before"] += rb["winner"] == "draw"
            s["draws_after"] += ra["winner"] == "draw"
            if rb["winner"] != ra["winner"]:
                s["winner_changed"] += 1
                s["gained"] += ra["winner"] == side
                s["lost"] += rb["winner"] == side
            elif rb["duration_secs"] != ra["duration_secs"]:
                s["duration_only"] += 1
    for name in sorted(stats):
        s = stats[name]
        n = s["n"]
        print(f"{name}: n={n}  side win {100 * s['before'] / n:.1f}% -> "
              f"{100 * s['after'] / n:.1f}%  draws {s['draws_before']} -> {s['draws_after']} "
              f"({100 * s['draws_before'] / n:.2f}% -> {100 * s['draws_after'] / n:.2f}%)  "
              f"winners changed {s['winner_changed']} (side +{s['gained']}/-{s['lost']})  "
              f"duration-only changes {s['duration_only']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
