#!/usr/bin/env python3
"""AS-381 bank: a `TeamPlan` healer side against a `Legacy` enemy, both seats.

    2026-10-19-as381-gen.py <map> <seeds-2v2> <seeds-3v3> [--uniform PROFILE]

Every config pits a team fielding exactly one healer against an enemy team
(no double healers), with the two teams on different AI profiles: labels
ending `#T1` put team 1 (the healer side) on `TeamPlan` and team 2 on
`Legacy`; `#T2` swap the seats, so the healer side is team 2 and on
`TeamPlan`. Scored from the healer side by `2026-10-18-as362-h2h.py`.

- 2v2: every healer with every DPS, against every 2v2 team without two
  healers.
- 3v3: every healer with five DPS pairs, against eight enemy teams chosen to
  reach five or more enemy casters (a Warlock's Felhunter and a Hunter's
  Spider count, see `team_solve::enemy_casters_of`), plus two melee-heavy
  teams as the low-caster contrast.

`--uniform PROFILE` drops the seat split and puts both teams on PROFILE, one
config per cell and seed (the `Legacy` and BasicArena byte-identity banks).
"""
import argparse
import itertools
import json

HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
ALL = DPS + HEALERS

DPS_PAIRS_3V3 = [
    ["Warrior", "Mage"],
    ["Rogue", "Warlock"],
    ["Hunter", "Mage"],
    ["Warlock", "Hunter"],
    ["Warrior", "Hunter"],
]
ENEMIES_3V3 = [
    ["Warlock", "Hunter", "Mage"],  # 5 casters
    ["Warlock", "Hunter", "Priest"],  # 5
    ["Warlock", "Hunter", "Shaman"],  # 5
    ["Warlock", "Hunter", "Druid"],  # 5
    ["Warlock", "Hunter", "Hunter"],  # 6
    ["Warlock", "Warlock", "Hunter"],  # 6
    ["Mage", "Warlock", "Priest"],  # 4
    ["Hunter", "Mage", "Paladin"],  # 4
    ["Warrior", "Rogue", "Priest"],  # 1, the contrast
    ["Warrior", "Rogue", "Paladin"],  # 0
]


def cells():
    for h, d in itertools.product(HEALERS, DPS):
        for enemy in itertools.combinations_with_replacement(ALL, 2):
            if sum(c in HEALERS for c in enemy) > 1:
                continue
            yield "2v2", [d, h], list(enemy)
    for h, pair in itertools.product(HEALERS, DPS_PAIRS_3V3):
        for enemy in ENEMIES_3V3:
            yield "3v3", pair + [h], enemy


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("map")
    ap.add_argument("seeds_2v2", type=int)
    ap.add_argument("seeds_3v3", type=int)
    ap.add_argument("--uniform", default=None)
    args = ap.parse_args()
    for size, healer_side, enemy in cells():
        n = args.seeds_2v2 if size == "2v2" else args.seeds_3v3
        base = "+".join(healer_side) + "_vs_" + "+".join(enemy)
        seats = (
            [("", healer_side, enemy, args.uniform, args.uniform)]
            if args.uniform
            else [
                ("#T1", healer_side, enemy, "TeamPlan", "Legacy"),
                ("#T2", enemy, healer_side, "Legacy", "TeamPlan"),
            ]
        )
        for seed in range(1, n + 1):
            for suffix, t1, t2, p1, p2 in seats:
                print(json.dumps({
                    "label": base + suffix,
                    "team1": t1,
                    "team2": t2,
                    "map": args.map,
                    "max_duration_secs": 300.0,
                    "random_seed": seed,
                    "team1_ai_profile": p1,
                    "team2_ai_profile": p2,
                }))


if __name__ == "__main__":
    main()
