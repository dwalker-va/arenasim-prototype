#!/usr/bin/env python3
"""AS-164 (0.7 milestone sweep): the batch inputs that are not plain gen_sweep.py matrices.

The canonical matrices come straight from gen_sweep.py:

    scripts/gen_sweep.py --full 1 --n 100 > canonical_1v1.jsonl   #   8,100
    scripts/gen_sweep.py --full 2 --n 100 > canonical_2v2.jsonl   # 129,600
    scripts/gen_sweep.py --full 3 --n 50  > canonical_3v3.jsonl   # 352,800

This script writes the rest into OUTDIR:

- h2h.jsonl          TeamPlan head-to-head, PillaredArena, seeds 1..300, in the
                     headtohead_sweep.py format (labels <cell>_LL/_TL/_LT).
                     Seeds 1..100 are the 2026-08-06 table's seeds, so those
                     rows pair by seed with 2026-08-06-team-solve-headtohead-n100.csv.
- mech_basic.jsonl   the healer economy panel: <DPS>+<healer> for every DPS and
                     every healer, against every competitive 2v2 comp (at most
                     one healer), both orientations, seeds 0..9. BasicArena.
- mech_nagrand.jsonl the Druid rows of the same panel on PillaredArena.
- mech_hunter.jsonl  Hunter+<any partner> against every one-healer comp, both
                     orientations, seeds 0..9. BasicArena.
- mech_duels.jsonl   1v1 Druid against each healer (Druid included), both
                     orientations, seeds 0..99. BasicArena.
- mech_h2h_hunter.jsonl  the head-to-head's hunter cell, Legacy (LL) and
                     TeamPlan-on-team-2 (LT) arms, seeds 1..100, PillaredArena.

The mech_* files are run through tests/milestone_mechanisms.rs.
"""

import itertools
import json
import sys
from pathlib import Path

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
DPS = [c for c in CLASSES if c not in HEALERS]
CAP = 300.0


def team_order(team):
    return sorted(team, key=CLASSES.index)


def competitive_2v2():
    for combo in itertools.combinations(CLASSES, 2):
        if sum(c in HEALERS for c in combo) <= 1:
            yield list(combo)


def one_healer_2v2():
    for combo in itertools.combinations(CLASSES, 2):
        if sum(c in HEALERS for c in combo) == 1:
            yield list(combo)


def both_orientations(a, b, seeds, label, mp=None):
    for s in seeds:
        for t1, t2, side in ((a, b, "a"), (b, a, "b")):
            cfg = {
                "team1": t1,
                "team2": t2,
                "random_seed": s,
                "max_duration_secs": CAP,
                "label": f"{label}|{side}",
            }
            if mp:
                cfg["map"] = mp
            yield cfg


def h2h():
    cells = [
        ("pillar", ["Warrior", "Priest"], ["Warlock", "Priest"]),
        ("hunter", ["Hunter", "Priest"], ["Rogue", "Priest"]),
        ("wmp", ["Warrior", "Mage", "Priest"], ["Rogue", "Warlock", "Paladin"]),
    ]
    for name, t1, t2 in cells:
        for tag, p1, p2 in (("LL", "Legacy", "Legacy"), ("TL", "TeamPlan", "Legacy"), ("LT", "Legacy", "TeamPlan")):
            for seed in range(1, 301):
                yield {
                    "label": f"{name}_{tag}",
                    "team1": t1,
                    "team2": t2,
                    "map": "PillaredArena",
                    "max_duration_secs": CAP,
                    "random_seed": seed,
                    "team1_ai_profile": p1,
                    "team2_ai_profile": p2,
                }


def mech(healers, mp):
    seeds = range(10)
    for h in healers:
        for x in DPS:
            team = team_order([x, h])
            for opp in competitive_2v2():
                yield from both_orientations(
                    team, opp, seeds, f"{'+'.join(team)}_vs_{'+'.join(opp)}", mp
                )


def mech_hunter():
    seeds = range(10)
    for p in CLASSES:
        if p == "Hunter":
            continue
        team = team_order(["Hunter", p])
        for opp in one_healer_2v2():
            yield from both_orientations(team, opp, seeds, f"{'+'.join(team)}_vs_{'+'.join(opp)}")


def mech_duels():
    for h in HEALERS:
        if h == "Druid":
            for s in range(100):
                yield {"team1": ["Druid"], "team2": ["Druid"], "random_seed": s,
                       "max_duration_secs": CAP, "label": "Druid_vs_Druid|a"}
        else:
            yield from both_orientations(["Druid"], [h], range(100), f"Druid_vs_{h}")


def mech_h2h_hunter():
    for r in h2h():
        if r["label"] in ("hunter_LL", "hunter_LT") and r["random_seed"] <= 100:
            yield r


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    files = {
        "h2h.jsonl": h2h(),
        "mech_basic.jsonl": mech(HEALERS, None),
        "mech_nagrand.jsonl": mech(["Druid"], "PillaredArena"),
        "mech_hunter.jsonl": mech_hunter(),
        "mech_duels.jsonl": mech_duels(),
        "mech_h2h_hunter.jsonl": mech_h2h_hunter(),
    }
    for name, rows in files.items():
        n = 0
        with open(out / name, "w") as f:
            for r in rows:
                f.write(json.dumps(r) + "\n")
                n += 1
        print(f"{name}: {n}", file=sys.stderr)


if __name__ == "__main__":
    main()
