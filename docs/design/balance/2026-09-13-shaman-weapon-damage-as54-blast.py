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

NOT RUNNABLE AS COMMITTED -- the two paths below point at the authoring
session's scratchpad and will raise FileNotFoundError anywhere else. This file
is a record of the method; edit the paths to re-run it. Only the NEW baseline is
in the working tree. The OLD (pre-AS-54) one was overwritten when the sweep was
re-measured on the new base, and survives as a blob on an earlier commit of this
branch:

    git show 4a902d1:docs/design/balance/2026-09-13-shaman-weapon-damage-before.csv > /tmp/pre.csv

Then set OLD to /tmp/pre.csv and NEW to the tree's
docs/design/balance/2026-09-13-shaman-weapon-damage-before.csv.
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
KNOWN = [("mirrored", "Warrior+Shaman_vs_Mage+Shaman")]

if not bad:
    print("VERDICT: the rule above holds for every cell")
elif sorted(bad) == sorted(KNOWN):
    print("VERDICT: 13 of 14 -- the rule above holds except for the one KNOWN and")
    print("         EXPLAINED cell, %s." % bad[0][1])
    print("         The rule as coded is 'contains a Mage and a melee unit'. The")
    print("         real trigger also needs the melee unit to LAND a hit on the")
    print("         Mage, and here the Warrior connects twice -- both on the")
    print("         enemy Shaman -- and dies. So the chill never fires and the")
    print("         cell is correctly unmoved. See the findings doc, section 5.")
else:
    print("VERDICT: unexpected cells break the rule: %s" % bad)
