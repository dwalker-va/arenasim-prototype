#!/usr/bin/env python3
"""Regenerate AS-104's two slot-symmetry probe sweeps.

    2026-09-20-as104-make-probe.py docs/design/balance/sweeps

Writes `2026-09-20-as104-slot-diagonal.jsonl` and
`2026-09-20-as104-slot-swap.jsonl` -- the exact files the published
probe was run over. See this directory's README.
"""
import itertools
import json
import pathlib
import sys

out = pathlib.Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman"]
HEALERS = {"Priest", "Paladin", "Shaman"}
teams2 = [list(c) for c in itertools.combinations(CLASSES, 2)
          if sum(1 for x in c if x in HEALERS) < 2]
assert len(teams2) == 25, len(teams2)


def cfg(t1, t2, seed, label):
    return {"team1": t1, "team2": t2, "random_seed": seed,
            "max_duration_secs": 300.0, "label": label, "map": "BasicArena"}


# (a) self-mirror diagonal: identical comps on both sides.
diag = []
for t in teams2:
    for s in range(80):
        diag.append(cfg(list(t), list(t), s, "+".join(t) + "_vs_" + "+".join(t)))
for c in CLASSES:
    for s in range(80):
        diag.append(cfg([c], [c], s, c + "_vs_" + c))
(out / "2026-09-20-as104-slot-diagonal.jsonl").write_text("".join(json.dumps(x) + "\n" for x in diag))

# (b) swap-closed off-diagonal: every ordered pair of distinct 2v2 teams, so
# the set is closed under swapping the two sides.
off = []
for a in teams2:
    for b in teams2:
        if a == b:
            continue
        for s in range(5):
            off.append(cfg(list(a), list(b), s, "+".join(a) + "_vs_" + "+".join(b)))
(out / "2026-09-20-as104-slot-swap.jsonl").write_text("".join(json.dumps(x) + "\n" for x in off))

print("diag matches:", len(diag), "cells:", len(teams2) + len(CLASSES))
print("swap matches:", len(off), "cells:", len(teams2) * (len(teams2) - 1))
