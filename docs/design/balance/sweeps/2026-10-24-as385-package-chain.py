#!/usr/bin/env python3
"""AS-385 package: Chain displacement, from `team_sight_cohesion` rows.

    2026-10-24-as385-package-chain.py <label>=<rows.jsonl[.gz]> ...

Each row carries `chain_window`: per deciding class, its ability decisions
while its team held a lock ending soon (`[all, under Pillar, under Chain]`).
A decision under Pillar inside that window is a hand-off the stance may have
displaced (Pillar sits ahead of Chain in PLAY_ORDER). Printed per arm and
class, with the classes that hold a hand-off first.
"""
import collections
import gzip
import json
import sys

HANDOFF = ["Paladin", "Rogue", "Warlock", "Mage", "Druid"]

print("| arm | class | decisions in a chain window | under Pillar | under Chain |")
print("|---|---|---|---|---|")
for arg in sys.argv[1:]:
    label, path = arg.split("=", 1)
    opener = gzip.open if path.endswith(".gz") else open
    acc = collections.defaultdict(lambda: [0, 0, 0])
    with opener(path, "rt") as f:
        for line in f:
            r = json.loads(line)
            for cls, (a, p, c) in (r.get("chain_window") or {}).items():
                x = acc[cls]
                x[0] += a
                x[1] += p
                x[2] += c
    order = sorted(acc, key=lambda k: (k not in HANDOFF, k))
    for cls in order:
        a, p, c = acc[cls]
        print(f"| {label} | {cls} | {a} | {p} ({100*p/max(a,1):.1f}%) | {c} ({100*c/max(a,1):.1f}%) |")
