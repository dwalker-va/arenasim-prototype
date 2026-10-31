#!/usr/bin/env python3
"""AS-399 spot check: what posture the unit a focus moves off was in.

    2026-10-31-as399-posture.py <arenasim> <scratch dir>

Plays every healer+melee team (team 1) against every healer+ranged team
(team 2, a different healer) on both pillar maps at seed 801, uniform
TeamPlan, trace on, and for every focus onset of team 2 reads the last
`movement_decision` of the team-1 unit the focus moved off: its posture and
its trigger. 96 matches.
"""
import collections
import json
import os
import subprocess
import sys

exe, scratch = sys.argv[1], sys.argv[2]
os.makedirs(scratch, exist_ok=True)
heal = ["Priest", "Shaman", "Druid", "Paladin"]
mel = ["Rogue", "Warrior"]
rng = ["Mage", "Hunter", "Warlock"]
post = collections.Counter()
trig = collections.Counter()
n = 0
for m in ("TwinPillars", "PillaredArena"):
    for h in heal:
        for me in mel:
            for r in rng:
                for h2 in heal:
                    if h2 == h:
                        continue
                    cfg = {"team1": [me, h], "team2": [r, h2], "map": m, "random_seed": 801,
                           "ai_profile": "TeamPlan"}
                    c = os.path.join(scratch, "c.json")
                    with open(c, "w") as f:
                        json.dump(cfg, f)
                    log = os.path.join(scratch, "m.txt")
                    subprocess.run([exe, "--headless", c, "--output", log, "--trace-mode", "on"],
                                   check=True, capture_output=True)
                    last, lastt = {}, {}
                    for line in open(os.path.join(scratch, "m_trace.jsonl")):
                        if '"movement_decision"' in line:
                            e = json.loads(line)
                            a = e["actor"]["entity_id"]
                            last[a] = e.get("posture")
                            lastt[a] = e.get("trigger")
                        elif '"target_focus"' in line and '"situation_change"' in line:
                            e = json.loads(line)
                            if e["team"] == 2 and e["change"] == "onset":
                                f = e.get("from")
                                n += 1
                                post[str(last.get(f))] += 1
                                trig[str(lastt.get(f))] += 1
print("team 2 (H+R) focus onsets off team 1's units:", n)
print("posture of the unit it moved off, at its last movement decision:", post.most_common())
print("that decision's trigger:", trig.most_common(12))
