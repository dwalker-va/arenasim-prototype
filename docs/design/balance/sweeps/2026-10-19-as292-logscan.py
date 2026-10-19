#!/usr/bin/env python3
"""AS-292 round 2: what the TeamPlan Priest suffers in the worst cell.

    2026-10-19-as292-logscan.py <enemy healer> <map> <seeds>

Expects two release binaries beside it, `arenasim-main` and `arenasim-pr`
(main, and the PR merged onto main). Plays every <DPS>+Priest (TeamPlan) v
<DPS>+<enemy healer> (Legacy) cell at seeds 0..<seeds> with each, writes the
match logs to `logs/`, and prints per-match means read off the logs: crowd
control landed on the Priest by kind, interrupts of its casts, healing it
did, and team 1's win rate.
"""
import json, os, re, subprocess, sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

S = os.path.dirname(os.path.abspath(__file__))
DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
enemy_healer = sys.argv[1] if len(sys.argv) > 1 else "Shaman"
mapn = sys.argv[2] if len(sys.argv) > 2 else "BasicArena"
seeds = int(sys.argv[3]) if len(sys.argv) > 3 else 5

jobs = []
for d1 in DPS:
    for d2 in DPS:
        for s in range(seeds):
            jobs.append(([d1, "Priest"], [d2, enemy_healer], s))


def run(binname, job):
    t1, t2, s = job
    tag = f"{binname}_{'+'.join(t1)}_{'+'.join(t2)}_{s}"
    cfgp = f"{S}/logs/{tag}.json"
    outp = f"{S}/logs/{tag}.txt"
    with open(cfgp, "w") as f:
        json.dump({"team1": t1, "team2": t2, "map": mapn, "team1_ai_profile": "TeamPlan",
                   "team2_ai_profile": "Legacy", "random_seed": s, "max_duration_secs": 300}, f)
    subprocess.run([f"{S}/arenasim-{binname}", "--headless", cfgp, "--output", outp],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    txt = open(outp).read()
    c = Counter()
    P = "Team 1 Priest #2"
    for line in txt.splitlines():
        if "[CAST]" in line and f"interrupts {P}" in line:
            c["interrupted"] += 1
        if "[CC]" in line and line.split("] ", 2)[-1].count(P) and " on " + P in line:
            kind = line.split("[CC] ", 1)[1].split(" on ")[0]
            c["cc:" + kind] += 1
        if "Purge" in line and P in line and ("removes" in line or "purges" in line.lower()):
            c["purged"] += 1
        if line.startswith("[") and f"{P}'s" in line and "heals" in line:
            m = re.search(r"for (\d+)", line)
            if m:
                c["heal"] += int(m.group(1))
    m = re.search(r"Winner: (.*)", txt)
    c["win"] += 1 if (m and "Team 1" in m.group(1)) else 0
    return c


os.makedirs(f"{S}/logs", exist_ok=True)
for b in ["main", "pr"]:
    with ThreadPoolExecutor(4) as ex:
        cs = list(ex.map(lambda j: run(b, j), jobs))
    tot = Counter()
    for c in cs:
        tot.update(c)
    n = len(cs)
    print(b, n, {k: round(v / n, 2) for k, v in sorted(tot.items())})
