#!/usr/bin/env python3
"""AS-395 cross-check: the observed harness rows against the `--batch` CSVs.

    2026-10-25-0.8-crosscheck.py <hunt-shipped.rows.jsonl[.gz]> <uni-TT csv>...

Every Hunt-harness row whose (map, team1, team2, seed) also appears in the
uniform TeamPlan baseline must carry the same winner and duration: the
observer is read-only, so the two runners must agree row for row.
"""
import csv
import gzip
import json
import sys


def opener(p):
    return gzip.open(p, "rt") if p.endswith(".gz") else open(p)


rows = sys.argv[1]
base = {}
for p in sys.argv[2:]:
    with opener(p) as f:
        for r in csv.DictReader(f):
            bank, m, t1, t2, s = r["label"].split("|")
            if bank == "uni-TT":
                base[(m, t1, t2, int(s))] = (r["winner"], round(float(r["duration_secs"]), 2))
same = diff = 0
with opener(rows) as f:
    for line in f:
        if not line.strip():
            continue
        r = json.loads(line)
        _, m, t1, t2, s = r["label"].split("|")
        b = base.get((m, t1, t2, int(s)))
        if b is None:
            continue
        w = r["winner"]
        d = round(float(r["duration"] if "duration" in r else r["duration_secs"]), 2)
        if (w, d) == b:
            same += 1
        else:
            diff += 1
print(f"harness rows paired with the batch CSV: {same} identical, {diff} different")
