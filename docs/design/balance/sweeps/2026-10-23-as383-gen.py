#!/usr/bin/env python3
"""AS-383 reachable-cell head-to-head on FRESH seeds.

    2026-10-23-as383-gen.py <seeds per cell> <control seeds per cell> <seed offset> > h2h.jsonl

`2026-10-19-as292-gen.py`'s cells (every DPS+{Priest,Shaman,Druid} team
against every other, BasicArena and PillaredArena, both seat assignments plus
the Legacy-vs-Legacy control), with every seed moved up by <seed offset> so the
run shares no seed with the AS-383 diagnosis (seeds 0-24). The committed run is
`20 4 1000`; score it with `2026-10-19-as292-h2h.py before.csv after.csv`.
"""
import json
import os
import subprocess
import sys

here = os.path.dirname(os.path.abspath(__file__))
seeds, control, offset = sys.argv[1], sys.argv[2], int(sys.argv[3])
out = subprocess.run([sys.executable, f"{here}/2026-10-19-as292-gen.py", seeds, control],
                     capture_output=True, text=True, check=True).stdout
for line in out.splitlines():
    row = json.loads(line)
    row["random_seed"] += offset
    print(json.dumps(row))
