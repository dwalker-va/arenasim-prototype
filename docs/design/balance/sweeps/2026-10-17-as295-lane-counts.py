#!/usr/bin/env python3
"""AS-295: count the lane instrument's events in one or more stderr files.

Each arm's binary carries `2026-10-17-as295-instrument.patch`, which writes
one stderr line per event:

    AS295 TICK seen|hidden_route|hidden_none     one per Hunter decision tick
                                                 with Freezing Trap off
                                                 cooldown and the lane trap
                                                 considered (60 a second)
    AS295 THROW <path>                           one per Freezing Trap thrown
    AS295 OUTCOME <path> sprung|expired|replaced intended|other|- <secs>

`<path>` is lane (the midpoint, seen), lane_unseen (the midpoint, hidden:
only under AS295_MODE=unseen), route (the AS-295 landing), dip or pressure.

usage: as295_lane_counts.py <file.raw> [...]
"""
import collections
import sys

for path in sys.argv[1:]:
    ticks = collections.Counter()
    throws = collections.Counter()
    outcomes = collections.Counter()
    delays = collections.defaultdict(list)
    for line in open(path):
        p = line.split()
        if not p or p[0] != "AS295":
            continue
        if p[1] == "TICK":
            ticks[p[2]] += 1
        elif p[1] == "THROW":
            throws[p[2]] += 1
        elif p[1] == "OUTCOME":
            outcomes[(p[2], p[3], p[4])] += 1
            if p[3] == "sprung":
                delays[p[2]].append(float(p[5]))
    print(f"== {path}")
    hidden = ticks["hidden_route"] + ticks["hidden_none"]
    total = hidden + ticks["seen"]
    if total:
        print(f"ticks: {total} considered, lane seen {ticks['seen']}, hidden {hidden}")
        if hidden:
            share = ticks["hidden_route"] / hidden
            print(f"  hidden with a seen route point: {ticks['hidden_route']} ({share:.1%}),"
                  f" none: {ticks['hidden_none']}")
    for kind in sorted(throws):
        n = throws[kind]
        sprung = sum(v for (k, w, _), v in outcomes.items() if k == kind and w == "sprung")
        intended = outcomes[(kind, "sprung", "intended")]
        expired = sum(v for (k, w, _), v in outcomes.items() if k == kind and w == "expired")
        d = sorted(delays[kind])
        med = d[len(d) // 2] if d else float("nan")
        print(f"throws {kind:12s} {n:6d}  sprung {sprung:6d} ({sprung / n:.1%})"
              f"  on the intended enemy {intended:6d} ({intended / n:.1%})"
              f"  expired {expired:5d}  median secs to spring {med:.2f}")
