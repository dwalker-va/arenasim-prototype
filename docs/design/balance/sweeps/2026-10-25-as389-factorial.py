#!/usr/bin/env python3
"""AS-389 / AS-392: the AS-385 package as a paired 2x2 factorial per side.

    2026-10-25-as389-factorial.py <off> <t1> <t2> <uni>

Each argument is a `tests/team_sight_cohesion.rs` rows file (plain or
gzipped) for one `2026-10-24-as385-package-gen.py` bank at the same seeds:
`pkg-h2h off` (neither team runs the package), `pkg-h2h t1`, `pkg-h2h t2`
(only that team runs it) and `uniform` (both do). So every side of every
match is played four times, once per (own package, enemy package) cell.

For each slice (map x DPS class, plus all), prints for that class's sides:

- win rate in each cell;
- OWN effect (own on - own off), with the enemy off and with it on;
- ENEMY effect (enemy on - enemy off), with own off and with own on;
- each with its paired +/- flips and McNemar z;
- the class's lining share and hard-CC share per cell (ranged DPS only),
  from the rows' `units`.
"""
import collections
import csv
import gzip
import json
import math
import sys

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}


def load(p):
    """Harness rows (.jsonl), or a `--batch` CSV (wins only, no occupancy)."""
    opener = gzip.open if p.endswith(".gz") else open
    out = {}
    with opener(p, "rt") as f:
        if ".csv" in p:
            for r in csv.DictReader(f):
                out[r["label"]] = {
                    "label": r["label"],
                    "map": r["label"].split(":")[0],
                    "team1": r["team1"],
                    "team2": r["team2"],
                    "winner": {"team1": 1, "team2": 2}.get(r["winner"], 0),
                    "units": [],
                }
            return out
        for line in f:
            if line.strip():
                r = json.loads(line)
                out[r["label"]] = r
    return out


def main(off, t1, t2, uni):
    arms = {k: load(p) for k, p in [("off", off), ("t1", t1), ("t2", t2), ("uni", uni)]}

    def cell_arm(team, own, enemy):
        if own and enemy:
            return "uni"
        if not own and not enemy:
            return "off"
        on_team = team if own else 3 - team
        return f"t{on_team}"

    wins = collections.defaultdict(lambda: collections.defaultdict(list))
    occ = collections.defaultdict(lambda: collections.defaultdict(lambda: [0.0, 0.0, 0.0]))
    for label, base in arms["off"].items():
        mp = "Nagrand" if base["map"] == "PillaredArena" else base["map"]
        for team in (1, 2):
            members = base[f"team{team}"].split("+")
            dps = [m for m in members if m not in HEALERS]
            res = {}
            for own in (False, True):
                for enemy in (False, True):
                    r = arms[cell_arm(team, own, enemy)].get(label)
                    if r is None:
                        break
                    res[(own, enemy)] = 1 if r["winner"] == team else 0
                    for u in r["units"]:
                        if u["team"] == team and u["role"] == "ranged":
                            for key in [(mp, u["class"]), ("both", u["class"])]:
                                o = occ[key][(own, enemy)]
                                o[0] += u["secs"]
                                o[1] += u["lining_target"]
                                o[2] += u["hard_cc"]
            if len(res) != 4:
                continue
            keys = [("both", "ALL"), (mp, "ALL")] + [(s, d) for d in set(dps) for s in ("both", mp)]
            for k in keys:
                for c, v in res.items():
                    wins[k][c].append(v)

    def eff(a, b):
        plus = sum(1 for x, y in zip(a, b) if y > x)
        minus = sum(1 for x, y in zip(a, b) if y < x)
        d = (sum(b) - sum(a)) / max(len(a), 1) * 100
        z = (plus - minus) / math.sqrt(plus + minus) if plus + minus else 0.0
        return f"{d:+5.1f} ({plus}/{minus}, z {z:+.2f})"

    for k in sorted(wins):
        w = wins[k]
        n = len(w[(False, False)])
        rate = {c: 100 * sum(v) / max(n, 1) for c, v in w.items()}
        print(f"\n== {k[0]} {k[1]}  n={n}")
        print("  win: off/off {:.1f}  own-only {:.1f}  enemy-only {:.1f}  both {:.1f}".format(
            rate[(False, False)], rate[(True, False)], rate[(False, True)], rate[(True, True)]))
        print("  own effect:   enemy off " + eff(w[(False, False)], w[(True, False)])
              + "   enemy on " + eff(w[(False, True)], w[(True, True)]))
        print("  enemy effect: own off   " + eff(w[(False, False)], w[(False, True)])
              + "   own on   " + eff(w[(True, False)], w[(True, True)]))
        print("  both vs off:  " + eff(w[(False, False)], w[(True, True)]))
        if k in occ:
            o = occ[k]
            print("  lining / hard-CC %: " + "  ".join(
                f"{'own' if c[0] else '-'}{'+enemy' if c[1] else ''} "
                f"{100 * o[c][1] / max(o[c][0], 1e-9):.1f}/{100 * o[c][2] / max(o[c][0], 1e-9):.1f}"
                for c in [(False, False), (True, False), (False, True), (True, True)]))


if __name__ == "__main__":
    main(*sys.argv[1:5])
