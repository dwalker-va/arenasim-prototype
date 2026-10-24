#!/usr/bin/env python3
"""AS-385 package banks: every 2v2 healer team against every 2v2 team (no
double healers) and the six one-healer 3v3 teams against each other, on
Nagrand (PillaredArena) and TwinPillars — AS-373's head-to-head cells, on
fresh seeds.

    2026-10-24-as385-package-gen.py <kind> <arm> <seeds> [--variant]

seeds: an inclusive range, e.g. 501-504.

kinds:
  legacy-h2h t1|t2   the named team TeamPlan, the other Legacy. No package
                     fields, so main runs the same file: arm (a), TeamPlan
                     against Legacy before and after the package.
  uniform  -         both teams TeamPlan: arm (b), the package against main
                     directly (run the same file on both binaries).
  pkg-h2h  off|t1|t2 both teams TeamPlan; every team but the named one plays
                     WITHOUT the package (sight yield off, Pillar withheld):
                     the design's §7.1 head-to-head, on the package binary.
  legacy   -         both teams Legacy: the byte-identity bank.

--variant  every team that runs the package leaves its healer's cast out of
           the sight yield's rule 1 (`team{1,2}_yield_healer_cast: false`):
           arm (c). Inert on main and on a team without the package.
"""
import itertools
import json
import sys

DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
MAPS = ["PillaredArena", "TwinPillars"]
THREES = [
    ["Warrior", "Mage", "Priest"],
    ["Rogue", "Warlock", "Druid"],
    ["Warrior", "Hunter", "Paladin"],
    ["Rogue", "Mage", "Shaman"],
    ["Warlock", "Hunter", "Priest"],
    ["Warrior", "Rogue", "Druid"],
]

kind, arm, seeds = sys.argv[1], sys.argv[2], sys.argv[3]
variant = "--variant" in sys.argv[4:]
lo, hi = (int(x) for x in seeds.split("-"))
healer_teams = [[d, h] for d in DPS for h in HEALERS]
all_teams = healer_teams + [list(p) for p in itertools.combinations(DPS, 2)]
cells = []
for m in MAPS:
    for t1 in healer_teams:
        for t2 in all_teams:
            cells.append((m, t1, t2))
    for t1, t2 in itertools.permutations(THREES, 2):
        cells.append((m, t1, t2))


def without_package(row, team):
    row[f"team{team}_sight_yield"] = False
    row[f"team{team}_withheld_plays"] = ["Pillar"]


for m, t1, t2 in cells:
    for s in range(lo, hi + 1):
        row = {
            "team1": t1,
            "team2": t2,
            "map": m,
            "random_seed": s,
            "max_duration_secs": 300,
            "label": f"{m}:{'+'.join(t1)}:{'+'.join(t2)}:{s}",
        }
        package = []
        if kind == "legacy-h2h":
            row["team1_ai_profile"] = "TeamPlan" if arm == "t1" else "Legacy"
            row["team2_ai_profile"] = "TeamPlan" if arm == "t2" else "Legacy"
            package = [1 if arm == "t1" else 2]
        elif kind == "uniform":
            row["ai_profile"] = "TeamPlan"
            package = [1, 2]
        elif kind == "pkg-h2h":
            row["ai_profile"] = "TeamPlan"
            for team in (1, 2):
                if arm == f"t{team}":
                    package.append(team)
                else:
                    without_package(row, team)
        elif kind == "legacy":
            row["ai_profile"] = "Legacy"
        else:
            sys.exit(f"unknown kind {kind}")
        if variant:
            for team in package:
                row[f"team{team}_yield_healer_cast"] = False
        print(json.dumps(row))
