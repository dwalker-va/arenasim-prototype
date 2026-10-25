#!/usr/bin/env python3
"""AS-395: the Legacy movement since 0.7, step by step.

    2026-10-25-0.8-bisect-table.py 2026-10-25-0.8-legacy-bisect.csv.gz 2026-10-25-0.8-legacy07.csv.gz

The bisect CSV holds the Legacy BasicArena 2v2 matrix (36 x 36, seeds 0-9)
played by each checkpoint commit, with a `commit` column; the legacy07 CSV's
seeds 0-9 rows are main (`40c75048`). Each row of the table is one step
between consecutive checkpoints: per class, the change in the competitive
win rate (each side at most one healer) of the sides fielding it, from the
flips (paired at identical seeds), with `*` at |McNemar z| >= 2.5. One seed
block, so this is DIRECTIONAL attribution: it says which merge moved which
class, not by exactly how much.
"""
import csv
import gzip
import math
import sys

HEAL = {"Priest", "Paladin", "Shaman", "Druid"}
CL = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
ORDER = ["ea63f4f0", "8182a332", "79051523", "6a07121c", "0ae067fc", "9ac37952",
         "380af5f3", "a2448a85", "45f7ab6c", "40c75048"]


def nh(t):
    return sum(c in HEAL for c in t.split("+"))


data = {sha: {} for sha in ORDER}
with gzip.open(sys.argv[1], "rt") as f:
    for r in csv.DictReader(f):
        data[r["commit"]][(r["team1"], r["team2"], int(r["seed"]))] = r["winner"]
with gzip.open(sys.argv[2], "rt") as f:
    for r in csv.DictReader(f):
        bank, m, t1, t2, s = r["label"].split("|")
        if m == "BasicArena" and len(t1.split("+")) == 2 and int(s) <= 9:
            data["40c75048"][(t1, t2, int(s))] = r["winner"]

print("| step | " + " | ".join(CL) + " | matches changed |")
print("|---" * (len(CL) + 2) + "|")
for i in range(1, len(ORDER)):
    a, b = data[ORDER[i - 1]], data[ORDER[i]]
    cells = []
    for c in CL:
        up = dn = n = 0
        for k, wa in a.items():
            t1, t2, s = k
            if nh(t1) > 1 or nh(t2) > 1:
                continue
            wb = b[k]
            for seat, team in ((1, t1), (2, t2)):
                if c not in team.split("+"):
                    continue
                x, y = wa == f"team{seat}", wb == f"team{seat}"
                n += 1
                up += y and not x
                dn += x and not y
        z = (up - dn) / math.sqrt(up + dn) if up + dn else 0
        cells.append(f"{100 * (up - dn) / n:+.1f}{'*' if abs(z) >= 2.5 else ''}")
    flips = sum(1 for k in a if a[k] != b[k])
    print(f"| `{ORDER[i - 1][:8]}` -> `{ORDER[i][:8]}` | " + " | ".join(cells) + f" | {flips} |")
