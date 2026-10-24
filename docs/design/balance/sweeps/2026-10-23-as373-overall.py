#!/usr/bin/env python3
"""Overall post-contact rates per map (Paladin excluded): hard CC'd, in CC
reach, blind to anchor, blind to a dying teammate; by posture too.

    2026-10-23-as373-overall.py rows.jsonl [...]
"""
import collections
import json
import sys

rows = [json.loads(l) for p in sys.argv[1:] for l in open(p) if l.strip()]
rows = [r for r in rows if r["class"] != "Paladin"]
acc = collections.defaultdict(collections.Counter)
for r in rows:
    fmt = "3v3" if r["team"].count("+") == 2 else "2v2"
    for k, o in r["occ"].items():
        ph, p, b = k.split("|")
        if ph != "post":
            continue
        for key in [(r["map"], fmt, "all"), (r["map"], fmt, p), (r["map"], "all", "all"), (r["map"], "all", p)]:
            for f in ["secs", "hard_cc", "exposed", "blind_anchor", "dying", "blind_dying"]:
                acc[key][f] += o[f]
print("| map | fmt | posture | hard CC'd | in CC reach | blind anchor | blind dying |")
print("|---|---|---|---|---|---|---|")
for key in sorted(acc):
    if key[2] not in ("all", "free", "pressured"):
        continue
    a = acc[key]
    print(f"| {key[0]} | {key[1]} | {key[2]} | {100*a['hard_cc']/a['secs']:.2f}% | {100*a['exposed']/a['secs']:.1f}% | {100*a['blind_anchor']/a['secs']:.2f}% | {100*a['blind_dying']/max(a['dying'],1e-9):.2f}% |")
