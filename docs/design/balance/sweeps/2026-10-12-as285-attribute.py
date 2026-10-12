#!/usr/bin/env python3
"""AS-285: attribute every moved match to the decision that first diverged.

usage: 2026-10-12-as285-attribute.py <before-bin> <after-bin> <before.csv> <after.csv>
                                     <sweep.jsonl> <workdir>

For every match whose winner or duration differs between the two arms, the
match is replayed on both binaries with the decision trace on, and the two
traces are walked in step until the first decision that differs (same frame,
same unit, a different choice or target). That decision is classified from the
traces themselves, not by elimination:

- REFUSED   — the base arm cast a hard CC there, and the fixed arm traced it as
              refused because the target was already held (`TargetAlreadyCCd`,
              or the trap's "already held" note), or its chooser passed the
              held unit over and the base arm's target was held then
              (`NoValidTarget` / a different target).
- ALLOWED   — the fixed arm cast a hard CC there that the base arm refused:
              the chain window, or a root that no longer counts as a hold.
- OTHER     — anything else; printed in full so it can be read.

Prints one line per moved match and a tally.
"""
from __future__ import annotations

import csv
import glob
import gzip
import json
import os
import subprocess
import sys
from collections import Counter

HARD_CC = {
    "Polymorph", "Fear", "DeathCoil", "CheapShot", "KidneyShot", "HammerOfJustice",
    "FreezingTrap", "Cyclone", "PsychicScream", "BoarCharge",
}


def read_csv(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return {(r["label"], r["seed"]): r for r in csv.DictReader(f)}


def trace(binary, cfg, workdir):
    """Replay one match with the trace on. Run each arm's binary IN PLACE (in
    its checkout's `target/release/`): a development binary writes its trace
    to `match_logs/` under the working directory (`paths::match_log_dir`),
    here `workdir`; a copied-out one would write to the per-user data dir."""
    os.makedirs(workdir, exist_ok=True)
    logs = os.path.join(workdir, "match_logs")
    for old in glob.glob(os.path.join(logs, "*")):
        os.remove(old)
    path = os.path.abspath(os.path.join(workdir, "cfg.json"))
    with open(path, "w") as f:
        json.dump(cfg, f)
    subprocess.run(
        [binary, "--headless", path, "--trace-mode", "on"],
        cwd=workdir, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True,
    )
    [t] = glob.glob(os.path.join(logs, "*_trace.jsonl"))
    events = []
    with open(t) as f:
        for line in f:
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            if e.get("kind") in ("ability_decision", "pet_decision"):
                events.append(e)
    os.remove(t)
    return events


def key(e):
    out = e.get("outcome") or {}
    return (e.get("sim_time"), e["actor"]["entity_id"], out.get("ability"), out.get("target_id"))


def reason_for(e, ability):
    for c in e.get("candidates") or []:
        if c.get("ability") == ability and c.get("status") == "rejected":
            r = c.get("reason")
            if isinstance(r, dict):
                k = next(iter(r))
                detail = r[k].get("note") or r[k].get("cc_type") if isinstance(r[k], dict) else None
                return f"{k}: {detail}" if detail else k
            return r
    return None


def classify(b, a):
    bo, ao = b.get("outcome") or {}, a.get("outcome") or {}
    ba, aa = bo.get("ability"), ao.get("ability")
    if ba in HARD_CC and (aa != ba or ao.get("target_id") != bo.get("target_id")):
        why = reason_for(a, ba) or ""
        if "TargetAlreadyCCd" in why or "already held" in why or why == "NoValidTarget":
            return "REFUSED", f"{ba} -> {aa} ({why})"
        if aa == ba:
            return "REFUSED", f"{ba} retargeted {bo.get('target_id')} -> {ao.get('target_id')}"
    if aa in HARD_CC and aa != ba:
        return "ALLOWED", f"{ba} -> {aa} (base: {reason_for(b, aa)})"
    return "OTHER", f"{ba} -> {aa}"


def main(argv):
    before_bin, after_bin, before_csv, after_csv, jsonl, workdir = argv[1:7]
    before_bin, after_bin = os.path.abspath(before_bin), os.path.abspath(after_bin)
    before, after = read_csv(before_csv), read_csv(after_csv)
    configs = {}
    with open(jsonl) as f:
        for line in f:
            c = json.loads(line)
            configs[(c["label"], str(c["random_seed"]))] = c
    moved = sorted(
        k for k in before
        if (before[k]["winner"], before[k]["duration_secs"])
        != (after[k]["winner"], after[k]["duration_secs"])
    )
    tally = Counter()
    for k in moved:
        bt = trace(before_bin, configs[k], os.path.join(workdir, "b"))
        at = trace(after_bin, configs[k], os.path.join(workdir, "a"))
        first = next(
            ((b, a) for b, a in zip(bt, at) if key(b) != key(a)),
            None,
        )
        if first is None:
            tally["NO-DIVERGENCE"] += 1
            print(f"{k[0]} seed {k[1]}: no divergent decision")
            continue
        b, a = first
        kind, what = classify(b, a)
        tally[kind] += 1
        print(f"{k[0]} seed {k[1]}: {kind} t={b.get('sim_time')} {b['actor']['class']} {what}")
    print(f"moved: {len(moved)}; " + ", ".join(f"{k} {v}" for k, v in sorted(tally.items())))


if __name__ == "__main__":
    main(sys.argv)
