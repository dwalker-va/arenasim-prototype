#!/usr/bin/env python3
"""AS-281: attribute every BasicArena match the change moved to an immune target.

For each paired row whose winner or duration differs, replay the match on both
instrumented binaries (`--headless`, match log on), find the first line where
the two match logs part, and the first decision either binary made that the
immunity filter changes:

  before: WASTED_CONC (Concussive Shot fired at an immune unit),
          TRAP_AT_IMMUNE (a trap thrown at an immune unit)
  after:  CONC_REDIRECT (Concussive Shot passed an immune melee for another),
          CONC_HELD_IMMUNE, TRAP_HELD_IMMUNE (held for an immune target),
          FROST_REDIRECT_THROWN (Frost Trap peel passed an immune anchor)

A match is attributed when such a decision comes no later than the first
divergent log line. Any other BasicArena divergence is a failure: on an
obstacle-free map the sight gate cannot act.

usage: attribute.py <before-bin> <after-bin> <sweep.jsonl> <before.csv> <after.csv> <workdir>
"""
import csv, json, os, re, subprocess, sys, pathlib

BEFORE_BIN, AFTER_BIN, JSONL, BCSV, ACSV, WORK = [os.path.abspath(a) for a in sys.argv[1:7]]
WORK = pathlib.Path(WORK)
WORK.mkdir(parents=True, exist_ok=True)
CAUSES = {"WASTED_CONC", "TRAP_AT_IMMUNE", "CONC_REDIRECT", "CONC_HELD_IMMUNE",
          "TRAP_HELD_IMMUNE", "FROST_REDIRECT_THROWN"}
LINE_T = re.compile(r"^\[\s*([0-9.]+)s\]")

configs = [l for l in open(JSONL) if l.strip()]
before = list(csv.DictReader(open(BCSV)))
after = list(csv.DictReader(open(ACSV)))
assert len(configs) == len(before) == len(after)


def run(binary, cfg_path, tag):
    d = WORK / tag
    d.mkdir(exist_ok=True)
    for f in (d / "match_logs").glob("*") if (d / "match_logs").exists() else []:
        f.unlink()
    env = {**os.environ, "AS281_LANE": "0"}
    env.pop("AS281_EXEMPT", None)
    r = subprocess.run([binary, "--headless", str(cfg_path)], cwd=d, capture_output=True,
                       text=True, env=env)
    log = next((d / "match_logs").glob("match_*.txt")).read_text().splitlines()
    events = []
    for l in r.stderr.splitlines():
        parts = l.split()
        if parts and parts[0] == "AS281" and parts[2] in CAUSES:
            events.append((float(parts[1]), parts[2]))
    return log, events


rows = []
moved = [i for i in range(len(configs))
         if (before[i]["winner"], before[i]["duration_secs"]) != (after[i]["winner"], after[i]["duration_secs"])]
for i in moved:
    cfg_path = WORK / f"cfg_{i}.json"
    cfg_path.write_text(configs[i])
    blog, bev = run(BEFORE_BIN, cfg_path, "b")
    alog, aev = run(AFTER_BIN, cfg_path, "a")
    # The event log is the timed lines; the report header carries end-state.
    bl = [l for l in blog if LINE_T.match(l)]
    al = [l for l in alog if LINE_T.match(l)]
    div = next((k for k in range(min(len(bl), len(al))) if bl[k] != al[k]), None)
    div_t = float(LINE_T.match(bl[div]).group(1)) if div is not None else None
    first = sorted([(t, "before:" + e) for t, e in bev] + [(t, "after:" + e) for t, e in aev])
    first_cause = first[0] if first else None
    rows.append({
        "row": i, "label": before[i]["label"], "seed": before[i]["seed"],
        "before": f'{before[i]["winner"]} {before[i]["duration_secs"]}',
        "after": f'{after[i]["winner"]} {after[i]["duration_secs"]}',
        "first_divergent_log_t": div_t,
        "first_cause_t": first_cause[0] if first_cause else None,
        "first_cause": first_cause[1] if first_cause else None,
        "causes_before": sorted({e for _, e in bev}),
        "causes_after": sorted({e for _, e in aev}),
    })
    print(json.dumps(rows[-1]), flush=True)

w = csv.DictWriter(open(WORK / "attribution.csv", "w"), fieldnames=list(rows[0].keys()))
w.writeheader()
w.writerows(rows)
print(f"moved {len(moved)}; attributed {sum(1 for r in rows if r['first_cause'])}")
