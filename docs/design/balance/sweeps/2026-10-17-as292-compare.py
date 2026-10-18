#!/usr/bin/env python3
"""AS-292: the bank's two arms, main -> branch, per (profile, map).

usage: 2026-10-17-as292-compare.py <main AS292_DIR> <branch AS292_DIR>

Per arm: healer PRESSURED entries per match, healer seconds in PRESSURED per
match (from the trace's posture transitions, to the match's last event),
`ally_in_danger` windows per match, and how many matches differ from the other
arm in result (winner, end reason, duration) and in their whole trace. Legacy
rows are the control: the rule is gated under TeamPlan, so every Legacy match
must be identical in both.
"""
import collections
import glob
import json
import os
import sys

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}


def stats(trace_path):
    entries = 0
    windows = 0
    posture = {}
    since = {}
    pressured_secs = 0.0
    last_t = 0.0
    with open(trace_path) as f:
        for line in f:
            try:
                e = json.loads(line)
            except ValueError:
                continue
            t = float(e.get("sim_time", last_t))
            last_t = max(last_t, t)
            if e.get("kind") == "movement_decision" and e["actor"]["class"] in HEALERS:
                ent = e["actor"]["entity_id"]
                p = e["posture"]
                prev = posture.get(ent)
                if p == "pressured" and prev != "pressured":
                    if e.get("previous_posture"):
                        entries += 1
                    since[ent] = t
                elif p != "pressured" and prev == "pressured":
                    pressured_secs += t - since.pop(ent)
                posture[ent] = p
            elif e.get("kind") == "situation_change" and e.get("name") == "ally_in_danger":
                if e["change"] == "onset":
                    windows += 1
    for ent, t0 in since.items():
        pressured_secs += last_t - t0
    return entries, pressured_secs, windows


def main(a, b):
    agg = collections.defaultdict(lambda: collections.defaultdict(float))
    for tr in sorted(glob.glob(os.path.join(a, "*.trace.jsonl"))):
        name = os.path.basename(tr)
        stem = name[: -len(".trace.jsonl")]
        profile, amap = stem.split("_")[:2]
        c = agg[(profile, amap)]
        c["matches"] += 1
        for arm, d in (("main", a), ("branch", b)):
            e, s, w = stats(os.path.join(d, name))
            c[f"{arm}_entries"] += e
            c[f"{arm}_secs"] += s
            c[f"{arm}_windows"] += w
        ra = open(os.path.join(a, stem + ".result")).read()
        rb = open(os.path.join(b, stem + ".result")).read()
        c["result_differs"] += ra != rb
        c["trace_differs"] += open(tr).read() != open(os.path.join(b, name)).read()
    for key in sorted(agg):
        c = agg[key]
        n = c["matches"]
        print(f"== {key[0]} {key[1]}: {int(n)} matches; result differs in {int(c['result_differs'])}, "
              f"trace differs in {int(c['trace_differs'])}")
        print(f"  healer PRESSURED entries/match: {c['main_entries'] / n:.2f} -> {c['branch_entries'] / n:.2f}")
        print(f"  healer seconds in PRESSURED/match: {c['main_secs'] / n:.1f} -> {c['branch_secs'] / n:.1f}")
        print(f"  ally_in_danger windows/match: {c['main_windows'] / n:.2f} -> {c['branch_windows'] / n:.2f}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
