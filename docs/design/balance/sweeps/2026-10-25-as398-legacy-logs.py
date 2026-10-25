#!/usr/bin/env python3
"""AS-398: Legacy full-log byte-identity, main against this build.

    legacy_logs.py <main binary> <new binary> <workdir>

Plays every config below under `Legacy` through `--headless` with each
binary, saving each match's `.txt` log, and compares the two logs byte for
byte. Prints one line per config and a summary: identical, differing, and the
decisive events across the identical ones (kills, crits) for non-vacuity.
"""
import itertools
import json
import os
import subprocess
import sys

main_bin, new_bin, work = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(work, exist_ok=True)

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
MAPS = ["BasicArena", "PillaredArena", "TwinPillars"]

# 2v2: a healer + a DPS against a healer + a DPS, plus the no-healer and
# double-healer shapes, every map; 3v3: a fixed handful, every map.
twos = [list(t) for t in itertools.combinations(CLASSES, 2)]
cells = []
for i, a in enumerate(twos):
    b = twos[(i * 7 + 3) % len(twos)]
    cells.append((a, b))
threes = [["Warrior", "Mage", "Priest"], ["Rogue", "Warlock", "Druid"],
          ["Warrior", "Hunter", "Paladin"], ["Rogue", "Mage", "Shaman"],
          ["Warlock", "Hunter", "Priest"], ["Warrior", "Rogue", "Druid"]]
cells += [(a, b) for a, b in itertools.permutations(threes, 2)][:12]

same = differ = 0
kills = crits = 0
for n, (a, b) in enumerate(cells):
    for m in MAPS:
        seed = 900 + n
        logs = []
        for tag, binary in (("main", main_bin), ("new", new_bin)):
            path = os.path.join(work, f"{n:03d}-{m}-{tag}.txt")
            cfg = {"team1": a, "team2": b, "map": m, "random_seed": seed,
                   "ai_profile": "Legacy", "max_duration_secs": 300.0,
                   "output_path": path}
            cfg_path = os.path.join(work, f"{n:03d}-{m}-{tag}.json")
            with open(cfg_path, "w") as f:
                json.dump(cfg, f)
            subprocess.run([binary, "--headless", cfg_path], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            with open(path, "rb") as f:
                logs.append(f.read())
        if logs[0] == logs[1]:
            same += 1
            text = logs[0].decode("utf-8", "replace")
            kills += text.count("has been eliminated")
            crits += text.count("CRIT")
        else:
            differ += 1
            print(f"DIFFER {n:03d} {m} {'+'.join(a)} v {'+'.join(b)} seed {seed}")
print(f"identical {same}, differing {differ}; across the identical: "
      f"{kills} eliminations, {crits} crit lines")
sys.exit(1 if differ else 0)
