#!/usr/bin/env python3
"""AS-303: TBC's item-budget slot modifiers, read from the TBC client.

usage: 2026-10-29-as303-tbc-slot-modifiers.py [--build 2.5.6.69795]
                                              [--cache-dir .db2-cache]

Two readings, both from the TBC Classic client's own tables on wago.tools.

1. The modifiers' VALUES. `RandPropPoints` holds, for every item level and
   quality, the stat budget of five slot groups -- the five columns
   `Epic_0..4` (and `Superior_`, `Good_`). Each column divided by column 0 is
   that group's slot modifier. They are printed for every TBC item level
   (80-164), with their median and spread.

2. Which slot is in which group. The column a slot draws from is chosen in
   code, not data, so it is read off the items instead: every item at a TBC
   required level (61-70) with no item effect (no on-use, equip or proc spell)
   and no socket, whose stats are all primary attributes or ratings, is valued
   as TBC prices an item, V = (sum (stat)^1.5)^(1/1.5), and V is divided by
   column 0 at its item level and quality. The median per InventoryType,
   divided by the median of the full-slot group (head, chest, legs), is the
   slot's measured modifier; the nearest column names its group. The reading
   is noisy (armor and quest rewards are priced loosely) and the script says
   so: it prints the quartiles and the count beside every median.

A trinket cannot be read the second way: every TBC trinket carries a spell,
so none is valued by its stats alone. Its group is a ruling, not a reading.
"""
from __future__ import annotations

import argparse
import os
import statistics
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "scripts"))

import db2_spell_sweep as db2  # noqa: E402

TBC_BUILD = "2.5.6.69795"
QUALITY = {2: "Good", 3: "Superior", 4: "Epic"}
# InventoryType -> name. ROBE (20) is a chest.
SLOTS = {
    1: "Head", 5: "Chest", 20: "Chest", 7: "Legs", 17: "Two-hand",
    3: "Shoulder", 6: "Waist", 8: "Feet", 10: "Hands", 12: "Trinket",
    2: "Neck", 9: "Wrist", 16: "Back", 11: "Finger", 14: "Shield", 23: "Held off-hand",
    13: "One-hand", 21: "Main hand", 22: "Off-hand weapon",
    15: "Ranged", 26: "Ranged (wand/gun)", 25: "Thrown",
}
FULL_SLOT = {"Head", "Chest", "Legs"}
# ItemStatType: 3 Agi, 4 Str, 5 Int, 6 Spi, 7 Sta; 12-37 the ratings.
VALUED = {3, 4, 5, 6, 7} | set(range(12, 38))


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--build", default=TBC_BUILD)
    p.add_argument("--cache-dir", default=os.environ.get("DB2_CACHE_DIR", ".db2-cache"))
    args = p.parse_args(argv)
    cache = os.path.join(args.cache_dir, args.build)

    rpp = {int(r["ID"]): r for r in db2.load("RandPropPoints", args.build, cache)}

    # 1. The five columns as fractions of column 0.
    print(f"RandPropPoints, build {args.build}: each slot-group column / column 0")
    print(f"{'quality':9} {'col':>3} {'median':>7} {'min':>6} {'max':>6}   (item levels 80-164)")
    columns = {}
    for q, name in QUALITY.items():
        for c in range(5):
            xs = [int(r[f"{name}_{c}"]) / int(r[f"{name}_0"])
                  for lvl, r in rpp.items() if 80 <= lvl <= 164 and int(r[f"{name}_0"]) > 0]
            columns[(q, c)] = statistics.median(xs)
            print(f"{name:9} {c:3d} {statistics.median(xs):7.3f} {min(xs):6.3f} {max(xs):6.3f}")
    print()

    # 2. Group membership from the items' own stats.
    has_effect = {int(r["ParentItemID"]) for r in db2.load("ItemEffect", args.build, cache)}
    ratios: dict[str, list[float]] = {}
    for r in db2.load("ItemSparse", args.build, cache):
        q = int(r["OverallQualityID"])
        if q not in QUALITY or not (61 <= int(r["RequiredLevel"]) <= 70):
            continue
        slot = SLOTS.get(int(r["InventoryType"]))
        if slot is None or int(r["ID"]) in has_effect:
            continue
        if any(r[f"SocketType_{i}"] != "0" for i in range(3)):
            continue
        stats = [(int(r[f"StatModifier_bonusStat_{i}"]), int(r[f"StatModifier_bonusAmount_{i}"]))
                 for i in range(10)]
        stats = [(s, a) for s, a in stats if a != 0]
        if not stats or any(s not in VALUED for s, _ in stats):
            continue
        base = rpp.get(int(r["ItemLevel"]))
        full = int(base[f"{QUALITY[q]}_0"]) if base else 0
        if full == 0:
            continue
        value = sum(a ** 1.5 for _, a in stats) ** (1 / 1.5)
        ratios.setdefault(slot, []).append(value / full)

    full_slot = statistics.median([x for s in FULL_SLOT for x in ratios.get(s, [])])
    groups = [columns[(4, c)] for c in range(5)]
    print("Fixed-stat TBC items (required level 61-70, no spell, no socket), valued")
    print("(sum stat^1.5)^(1/1.5) / column 0, normalised to the head/chest/legs median")
    print(f"{'slot':18} {'n':>4} {'q1':>6} {'median':>7} {'q3':>6}  nearest column")
    for slot in sorted(ratios, key=lambda s: -statistics.median(ratios[s])):
        xs = [x / full_slot for x in ratios[slot]]
        if len(xs) < 3:
            print(f"{slot:18} {len(xs):4d}  (too few to read)")
            continue
        q1, med, q3 = statistics.quantiles(xs, n=4)
        med = statistics.median(xs)
        near = min(range(5), key=lambda c: abs(groups[c] - med))
        print(f"{slot:18} {len(xs):4d} {q1:6.3f} {med:7.3f} {q3:6.3f}  {near} ({groups[near]:.3f})")
    if "Trinket" not in ratios:
        print(f"{'Trinket':18} {0:4d}  (every TBC trinket carries a spell; not readable)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
