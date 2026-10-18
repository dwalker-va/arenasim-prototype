#!/usr/bin/env python3
"""AS-363: attribute every moved match to a mutual-camp cell.

    2026-10-17-as363-attribute.py before.csv[.gz] after.csv[.gz]

A team camps when it has a melee (the Paladin counts) and a healer
(`team_plan::CompProfile::wants_to_camp`). The change only touches cells where
BOTH teams would camp, so every moved match must be in one and none outside.
Prints the draw counts and side split in those cells, and exits 1 if a match
outside them moved.
"""
import csv
import gzip
import sys

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
MELEE = {"Warrior", "Rogue", "Paladin"}


def camps(team):
    classes = team.split("+")
    return any(c in MELEE for c in classes) and any(c in HEALERS for c in classes)


def load(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return list(csv.DictReader(f))


def main(argv):
    before, after = load(argv[0]), load(argv[1])
    assert len(before) == len(after), "arms differ in length"
    mutual = moved_in = moved_out = 0
    draws = [0, 0]
    sides = {"before": [0, 0], "after": [0, 0]}
    for b, a in zip(before, after):
        assert (b["label"], b["seed"]) == (a["label"], a["seed"]), "arms out of order"
        is_mutual = camps(b["team1"]) and camps(b["team2"])
        key = ("winner", "end_reason", "duration_secs")
        moved = tuple(b[k] for k in key) != tuple(a[k] for k in key)
        if is_mutual:
            mutual += 1
            draws[0] += b["winner"] == "draw"
            draws[1] += a["winner"] == "draw"
            for arm, row in (("before", b), ("after", a)):
                sides[arm][0] += row["winner"] == "team1"
                sides[arm][1] += row["winner"] == "team2"
            moved_in += moved
        else:
            moved_out += moved
    print(f"matches: {len(before)}; in mutual-camp cells: {mutual}")
    print(f"draws in mutual-camp cells: {draws[0]} -> {draws[1]}")
    for arm, (t1, t2) in sides.items():
        print(f"  {arm}: team1 {t1}, team2 {t2}, draw {mutual - t1 - t2}")
    print(f"moved inside mutual-camp cells: {moved_in}; outside: {moved_out}")
    return 1 if moved_out else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
