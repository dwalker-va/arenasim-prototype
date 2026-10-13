#!/usr/bin/env python3
"""AS-281: count the instrument's events per arm and map.

Each arm's binary carries the instrument patch committed beside this script
(`2026-10-12-as281-instrument-{before,after}.patch`), which writes one stderr
line per event: `AS281 <sim time> <EVENT> [<trap>] <placement path>`. Run the
sweep with stderr kept, one file per arm and map:

    AS281_LANE=0 <binary> --batch <map>.jsonl --out <arm>_<map>.csv --jobs 6 \\
        2> <arm>_<map>.raw

then `event_counts.py <dir> [<file prefix>]` prints the table and writes event-counts.csv.

Events that are DECISIONS (one per cast or throw): WASTED_CONC, TRAP_AT_IMMUNE,
TRAP_UNSEEN, CONC_REDIRECT, FROST_REDIRECT_THROWN. Events that are HOLDS
(TRAP_HELD_*, CONC_HELD_IMMUNE) fire once per decision tick the throw or shot
was held, so they count ticks (60 a second), not throws.
"""
import collections, csv, pathlib, sys

d = pathlib.Path(sys.argv[1])
prefix = sys.argv[2] if len(sys.argv) > 2 else ""
maps = ["BasicArena", "TwinPillars", "PillaredArena", "PillaredTeamPlan"]
counts = collections.Counter()
for arm in ["before", "after"]:
    for m in maps:
        for line in open(d / f"{prefix}{arm}_{m}.raw"):
            p = line.split()
            if not p or p[0] != "AS281":
                continue
            ev = p[2]
            if ev in ("CONC_NEAREST_IMMUNE", "FROST_ANCHOR_IMMUNE"):
                continue  # per-tick context, not a decision
            trap = p[3] if ev.startswith("TRAP_") and len(p) == 5 else ""
            path = p[-1] if ev.startswith("TRAP_") else ""
            counts[(arm, m, ev, trap, path)] += 1
rows = sorted(counts.items())
with open(d / "event-counts.csv", "w") as f:
    w = csv.writer(f)
    w.writerow(["arm", "map", "event", "trap", "path", "count"])
    for k, n in rows:
        w.writerow([*k, n])
for k, n in rows:
    print(f"{n:>8}  {' '.join(x for x in k if x)}")
