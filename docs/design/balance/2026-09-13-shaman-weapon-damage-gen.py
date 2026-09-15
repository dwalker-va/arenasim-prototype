#!/usr/bin/env python3
"""Generate the AS-97 paired sweep input.

Three slices, all 2v2-with-healer on BasicArena, seeds 0..N-1 per cell:

  control  - no Shaman on either side. The change must be a NO-OP here, so
             these rows must come out byte-identical between the two binaries.
             If they differ, the change reached something it should not have.
  clean    - team1's healer is the Shaman, team2's is a Priest, DPS mirrored.
             The delta slice: the enemy has no Shaman, so a team1 win-rate move
             is attributable to the Shaman's own weapon.
  mirrored - BOTH sides field a Shaman. The buff applies to both, so this asks
             whether it is net-neutral when neither side has the edge.

`--batch`'s CSV has no map column (src/headless/batch.rs), so the map is
encoded in the label instead of being lost.
"""
import json
import sys

MAP = "BasicArena"
N = int(sys.argv[1]) if len(sys.argv) > 1 else 100
OUT = sys.argv[2]

CONTROL = [
    (["Warrior", "Priest"], ["Mage", "Priest"]),
    (["Rogue", "Priest"], ["Warlock", "Priest"]),
    (["Hunter", "Priest"], ["Warrior", "Priest"]),
    (["Mage", "Priest"], ["Rogue", "Priest"]),
    (["Warlock", "Priest"], ["Hunter", "Priest"]),
]
CLEAN = [
    (["Warrior", "Shaman"], ["Warrior", "Priest"]),
    (["Mage", "Shaman"], ["Mage", "Priest"]),
    (["Rogue", "Shaman"], ["Rogue", "Priest"]),
    (["Hunter", "Shaman"], ["Hunter", "Priest"]),
    (["Warlock", "Shaman"], ["Warlock", "Priest"]),
]
MIRRORED = [
    (["Warrior", "Shaman"], ["Mage", "Shaman"]),
    (["Rogue", "Shaman"], ["Hunter", "Shaman"]),
    (["Warlock", "Shaman"], ["Warrior", "Shaman"]),
    (["Mage", "Shaman"], ["Rogue", "Shaman"]),
]

with open(OUT, "w") as f:
    for slice_name, cells in (
        ("control", CONTROL),
        ("clean", CLEAN),
        ("mirrored", MIRRORED),
    ):
        for t1, t2 in cells:
            cell = "%s_vs_%s" % ("+".join(t1), "+".join(t2))
            for seed in range(N):
                f.write(
                    json.dumps(
                        {
                            "team1": t1,
                            "team2": t2,
                            "map": MAP,
                            "random_seed": seed,
                            "max_duration_secs": 300,
                            "label": "%s|%s|%s" % (MAP, slice_name, cell),
                        }
                    )
                    + "\n"
                )
print("wrote %s" % OUT)
