#!/usr/bin/env python3
"""Where did AS-54 actually change outcomes?

Hypothesis: AS-54 makes Frost Armor's chill one debuff. The chill only fires
when a MAGE is being meleed. So AS-54 can only move a cell that contains both a
Mage and a melee attacker. If that is right:

  - clean slice   (X+Shaman vs X+Priest, X in Warrior/Mage/Rogue/Hunter/Warlock)
                  -> no cell has BOTH a Mage and a melee unit, so AS-54 is inert
                     and old-base vs new-base baseline rows must be IDENTICAL.
  - control slice (Warrior+Priest vs Mage+Priest, etc.) -> has both, must DIFFER.
  - mirrored      (Warrior+Shaman vs Mage+Shaman, etc.) -> has both, must DIFFER.

Compares the OLD-base baseline CSV against the NEW-base baseline CSV. Both are
the unmodified-Shaman arm, so the only variable between them is AS-54.
"""
import csv
import os

S = "/private/tmp/claude-501/-Users-dwalker-Projects-arenasim-prototype--claude-worktrees-animations/9e56438a-688e-4700-ae79-5f012de98eea/scratchpad/proof"
OLD = os.path.join(S, "before.csv")      # baseline built on 3c61185 (pre AS-54)
NEW = os.path.join(S, "v2_before.csv")   # baseline built on 323bd93 (post AS-54)
FIELDS = ("winner", "end_reason", "duration_secs")

MELEE = {"Warrior", "Rogue", "Paladin"}


def load(p):
    d = {}
    with open(p) as f:
        for r in csv.DictReader(f):
            d[(r["label"], r["seed"])] = r
    return d


old, new = load(OLD), load(NEW)
assert set(old) == set(new)

cells = {}
for k in old:
    label = k[0]
    sl, cell = label.split("|")[1], label.split("|")[2]
    same = all(old[k][f] == new[k][f] for f in FIELDS)
    d = cells.setdefault((sl, cell), {"n": 0, "diff": 0})
    d["n"] += 1
    d["diff"] += 0 if same else 1

print("%-9s %-42s %5s %6s  %s" % ("slice", "cell", "n", "differ", "has Mage AND melee?"))
for (sl, cell), d in sorted(cells.items()):
    units = set(cell.replace("_vs_", "+").split("+"))
    predicted = "Mage" in units and bool(units & MELEE)
    flag = "yes" if predicted else "no"
    agree = (d["diff"] > 0) == predicted
    print(
        "%-9s %-42s %5d %6d  %-4s %s"
        % (sl, cell, d["n"], d["diff"], flag, "OK" if agree else "<-- MISMATCH")
    )

print()
bad = [
    (sl, cell)
    for (sl, cell), d in cells.items()
    if ((d["diff"] > 0)
        != ("Mage" in set(cell.replace("_vs_", "+").split("+"))
            and bool(set(cell.replace("_vs_", "+").split("+")) & MELEE)))
]
print("VERDICT: %s" % (
    "prediction holds for every cell" if not bad else "prediction fails on %s" % bad))
