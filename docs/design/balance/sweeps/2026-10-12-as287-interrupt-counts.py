#!/usr/bin/env python3
"""AS-287: what happened to the interrupts, the Aimed Shots and the lockouts.

usage: 2026-10-12-as287-interrupt-counts.py <before-bin> <after-bin> <sweep.jsonl>
                                            <workdir> <out.csv> [--map MAP] [--jobs N]

The batch CSV carries outcomes only. This replays the sweep's matches that
field a Hunter (optionally one map's) on both binaries with the match log on,
and counts, per match and arm, from the log's own lines:

  interrupts_aimed   `<x> interrupts <Hunter>'s Aimed Shot - ...`
  interrupts_other   every other `<x> interrupts <y>'s <spell> - ...`
  physical_lockouts  `... - Physical school locked for ...`
  cc_stopped_aimed   `<Hunter>'s Aimed Shot interrupted by crowd control`
  aimed_casts        `<Hunter> begins casting Aimed Shot` (casts started)

and writes one row per (arm, label, seed), then prints the totals per arm.
The same seed is the same match on both arms until the change first acts, so
the totals compare like with like; they are counts, not rates.
"""
from __future__ import annotations

import argparse
import csv
import json
import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

COUNTS = {
    "interrupts_aimed": re.compile(r" interrupts Team \d \w+ #\d's Aimed Shot - "),
    "interrupts_other": re.compile(r" interrupts .*'s (?!Aimed Shot - ).* - \w+ school locked"),
    "physical_lockouts": re.compile(r" - Physical school locked for "),
    "cc_stopped_aimed": re.compile(r"'s Aimed Shot interrupted by crowd control"),
    "aimed_casts": re.compile(r" begins casting Aimed Shot"),
}


def replay(binary: str, cfg: dict, path: str) -> dict:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    cfg = dict(cfg, output_path=path + ".txt")
    with open(path + ".json", "w") as f:
        json.dump(cfg, f)
    subprocess.run(
        [binary, "--headless", path + ".json"],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    with open(path + ".txt") as f:
        log = f.read()
    os.remove(path + ".txt")
    os.remove(path + ".json")
    return {k: len(rx.findall(log)) for k, rx in COUNTS.items()}


def main(argv=None) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("before")
    p.add_argument("after")
    p.add_argument("sweep")
    p.add_argument("workdir")
    p.add_argument("out")
    p.add_argument("--map", default=None)
    p.add_argument("--jobs", type=int, default=6)
    a = p.parse_args(argv)

    cfgs = []
    with open(a.sweep) as f:
        for line in f:
            if not line.strip() or line.startswith("#"):
                continue
            cfg = json.loads(line)
            if "Hunter" not in cfg["team1"] + cfg["team2"]:
                continue
            if a.map and cfg.get("map") != a.map:
                continue
            cfgs.append(cfg)

    jobs = []
    for arm, binary in (("before", a.before), ("after", a.after)):
        for i, cfg in enumerate(cfgs):
            jobs.append((arm, binary, cfg, os.path.join(a.workdir, arm, str(i))))

    def run(job):
        arm, binary, cfg, path = job
        return arm, cfg, replay(os.path.abspath(binary), cfg, path)

    rows = []
    with ThreadPoolExecutor(a.jobs) as pool:
        for n, (arm, cfg, counts) in enumerate(pool.map(run, jobs), 1):
            rows.append({"arm": arm, "label": cfg["label"], "seed": cfg["random_seed"], **counts})
            if n % 500 == 0:
                print(f"  {n}/{len(jobs)}", file=sys.stderr)

    with open(a.out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=["arm", "label", "seed", *COUNTS])
        w.writeheader()
        w.writerows(rows)

    print(f"{len(cfgs)} matches with a Hunter{' on ' + a.map if a.map else ''}, per arm")
    print(f"{'arm':<8}" + "".join(f"{k:>20}" for k in COUNTS))
    for arm in ("before", "after"):
        mine = [r for r in rows if r["arm"] == arm]
        print(f"{arm:<8}" + "".join(f"{sum(r[k] for r in mine):>20}" for k in COUNTS))
    return 0


if __name__ == "__main__":
    sys.exit(main())
