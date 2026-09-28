#!/usr/bin/env python3
"""Per-class slices of an AS-115 paired sweep, from the affected side's view.

A match counts when exactly one team fields an affected class, and it is scored
for that team. `--by partner` groups by the affected side's other class (the
Warrior sweep); `--by holder` groups by the affected class itself (the staff
sweep), so a team fielding two affected classes counts under both.

    2026-09-27-as115-slices.py before.csv after.csv --affects Warrior --by partner
    2026-09-27-as115-slices.py pair.csv staff.csv --affects Mage,Priest,Warlock --by holder
"""
import argparse
import csv
import math


def load(path):
    with open(path) as f:
        return {(r["label"], r["seed"]): r for r in csv.DictReader(f)}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("before")
    ap.add_argument("after")
    ap.add_argument("--affects", required=True)
    ap.add_argument("--by", choices=("partner", "holder"), required=True)
    args = ap.parse_args(argv)

    affected = set(args.affects.split(","))
    before, after = load(args.before), load(args.after)
    groups = {}
    for key, rb in before.items():
        ra = after[key]
        t1, t2 = set(rb["team1"].split("+")), set(rb["team2"].split("+"))
        on1, on2 = bool(t1 & affected), bool(t2 & affected)
        if on1 == on2:
            continue
        side, team = ("team1", t1) if on1 else ("team2", t2)
        if args.by == "partner":
            names = sorted(team - affected) or ["(affected pair)"]
        else:
            names = sorted(team & affected)
        won_before = rb["winner"] == side
        won_after = ra["winner"] == side
        for name in names:
            g = groups.setdefault(name, [0, 0, 0, 0])
            g[0] += 1
            g[1] += won_before
            g[2] += won_after and not won_before
            g[3] += won_before and not won_after

    for name, (n, wins, up, down) in sorted(groups.items()):
        z = (up - down) / math.sqrt(up + down) if up + down else 0.0
        print(f"{name:20s} n={n:4d}  before {100 * wins / n:5.1f}%  "
              f"after {100 * (wins + up - down) / n:5.1f}%  "
              f"delta {100 * (up - down) / n:+5.1f}pt  flips +{up}/-{down}  z={z:.2f}")


if __name__ == "__main__":
    main()
