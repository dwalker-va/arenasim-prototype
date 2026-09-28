#!/usr/bin/env python3
"""Per-class slices of the AS-167 paired sweep, from each class's own side.

Every class wields a weapon whose speed moved, so no cell is out of the
change's reach and `scripts/paired_sweep.py`'s control/affected split has
nothing to put in the control. This prints the run-wide non-vacuity, then, for
each class, the matches in which exactly one team fields it, scored for that
team: before and after win rate, the paired flips, McNemar z, and the 95%
half-width those flips buy on the delta.

A class's slice is its side's result against every opponent, and every
opponent changed too — it is the net of its own weapon's change and everyone
else's, which is what "how did this class move" means here. It is a
DIRECTIONAL figure (docs/design/balance/sweep-tiers.md): which way and roughly
how far, never a class's standing.

    2026-09-28-as167-slices.py before.csv after.csv
"""
import argparse
import csv
import math

CLASSES = ["Warrior", "Rogue", "Hunter", "Mage", "Priest", "Warlock", "Paladin", "Shaman"]


def load(path):
    with open(path) as f:
        return {(r["label"], r["seed"]): r for r in csv.DictReader(f)}


def slice_line(name, rows):
    n = len(rows)
    wins = sum(b for b, _ in rows)
    up = sum(a and not b for b, a in rows)
    down = sum(b and not a for b, a in rows)
    z = (up - down) / math.sqrt(up + down) if up + down else 0.0
    half = 196 * math.sqrt(up + down) / n if n else 0.0
    return (f"{name:28s} n={n:5d}  before {100 * wins / n:5.1f}%  "
            f"after {100 * (wins + up - down) / n:5.1f}%  "
            f"delta {100 * (up - down) / n:+5.1f}pt (+/-{half:.1f})  "
            f"flips +{up}/-{down}  z={z:+.2f}")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("before")
    ap.add_argument("after")
    args = ap.parse_args(argv)
    before, after = load(args.before), load(args.after)
    assert before.keys() == after.keys(), "the two arms did not run the same configs"

    kills = {arm: sum(r["end_reason"] == "kill" for r in d.values())
             for arm, d in (("before", before), ("after", after))}
    moved = sum(before[k]["winner"] != after[k]["winner"]
                or before[k]["duration_secs"] != after[k]["duration_secs"] for k in before)
    flipped = sum(before[k]["winner"] != after[k]["winner"] for k in before)
    print(f"matches {len(before)} per arm; ended by a kill: before {kills['before']}, "
          f"after {kills['after']}")
    print(f"distinct durations: before {len({r['duration_secs'] for r in before.values()})}, "
          f"after {len({r['duration_secs'] for r in after.values()})}")
    print(f"winner or duration moved in {moved}; winner flipped in {flipped} "
          f"({100 * flipped / len(before):.1f}%)")
    for arm, d in (("before", before), ("after", after)):
        durs = sorted(float(r["duration_secs"]) for r in d.values())
        print(f"median duration {arm}: {durs[len(durs) // 2]:.1f}s")
    print()

    def rows_for(cls, enemy_filter=None):
        rows = []
        for key, rb in before.items():
            ra = after[key]
            t1, t2 = rb["team1"].split("+"), rb["team2"].split("+")
            on1, on2 = cls in t1, cls in t2
            if on1 == on2:
                continue
            side, enemy = ("team1", t2) if on1 else ("team2", t1)
            if enemy_filter is not None and enemy_filter(enemy) is False:
                continue
            rows.append((rb["winner"] == side, ra["winner"] == side))
        return rows

    for cls in CLASSES:
        print(slice_line(cls, rows_for(cls)))

    # The Warrior is the one class whose weapon DPS rose (the two-hander
    # premium) and whose Heroic Strike grew with its swing, so every other
    # class is split by whether it faced one.
    print()
    for cls in CLASSES:
        if cls == "Warrior":
            continue
        print(slice_line(f"{cls} vs an enemy Warrior",
                         rows_for(cls, lambda enemy: "Warrior" in enemy)))
        print(slice_line(f"{cls} vs no Warrior",
                         rows_for(cls, lambda enemy: "Warrior" not in enemy)))


if __name__ == "__main__":
    main()
