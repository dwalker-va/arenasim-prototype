#!/usr/bin/env python3
"""AS-305: the Medallion's declared price, as a node arm (DIRECTIONAL).

Every 2v2 on BasicArena with a wearer class X in team 1 slot 0 and a partner
P, against every distinct enemy pair; no double-healer side (1,800 cells).
Three seed blocks of 4 seeds. The wearer carries the Medallion in Trinket2;
each arm changes ONLY the Medallion's `items.ron` entry, by running a copy of
one release binary with its own `assets/` beside it (`paths::assets_dir`
reads beside the executable first).

    # 1. the cells (same file for every arm; arm 0 runs it without the trinket)
    2026-10-31-as305-medallion.py gen OUT
    # 2. one arm: break | health, its price / health
    2026-10-31-as305-medallion.py arm OUT M18 --break 18 --health 30 --bin target/release/arenasim
    2026-10-31-as305-medallion.py arm OUT H48 --health 48 --bin target/release/arenasim
    2026-10-31-as305-medallion.py arm OUT 0 --none --bin target/release/arenasim
    # 3. the paired analysis of two arms
    2026-10-31-as305-medallion.py analyze OUT M18 H48
    # 4. how often the shipped rule fires and holds (traces, one match at a time)
    2026-10-31-as305-medallion.py fires OUT --bin target/release/arenasim --n 240

Score: the wearer's team, win 1 / draw 0.5 / loss 0, paired at identical seeds.
"""

import argparse
import csv
import glob
import itertools
import json
import math
import os
import random
import re
import shutil
import subprocess
import sys
from collections import Counter, defaultdict

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
BLOCKS, SEEDS = 3, 4
SEED_BASE = 305_000
REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))
ENTRY = re.compile(r"(        MedallionOfTheAlliance: \(\n(?:.*\n)*?        \),\n)")


def configs(with_medallion):
    for block in range(BLOCKS):
        for x in CLASSES:
            for p in CLASSES:
                if p == x or (x in HEALERS and p in HEALERS):
                    continue
                for e in itertools.combinations(CLASSES, 2):
                    if e[0] in HEALERS and e[1] in HEALERS:
                        continue
                    for k in range(SEEDS):
                        cfg = {"team1": [x, p], "team2": list(e),
                               "random_seed": SEED_BASE + block * 1000 + k,
                               "max_duration_secs": 300}
                        if with_medallion:
                            cfg["team1_equipment"] = [{"Trinket2": "MedallionOfTheAlliance"}, {}]
                        yield cfg


def gen(out):
    os.makedirs(out, exist_ok=True)
    for name, med in (("med.jsonl", True), ("none.jsonl", False)):
        with open(os.path.join(out, name), "w") as f:
            for cfg in configs(med):
                f.write(json.dumps(cfg) + "\n")
    print(f"{sum(1 for _ in configs(True))} matches per arm")


def medallion_entry(break_price, health):
    lines = ["        MedallionOfTheAlliance: (",
             '            name: "Medallion of the Alliance",',
             "            item_level: 92,",
             "            item_tier: 2,",
             '            icon: "icons/items/inv_jewelry_trinketpvp_01.jpg",',
             "            slot: Trinket,"]
    if health:
        lines.append(f"            max_health: {health:.1f},")
    if break_price:
        lines += ["            on_use: Some((",
                  "                effect: CcBreak,",
                  "                cooldown: OncePerMatch,",
                  f"                price: Some({break_price:.1f}),",
                  "            )),"]
    lines.append("        ),")
    return "\n".join(lines) + "\n"


def arm(out, name, binary, break_price, health, none, jobs):
    d = os.path.join(out, f"arm{name}")
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    shutil.copy2(binary, os.path.join(d, "arenasim"))
    shutil.copytree(os.path.join(REPO, "assets"), os.path.join(d, "assets"))
    items = os.path.join(d, "assets", "config", "items.ron")
    src = open(items).read()
    assert len(ENTRY.findall(src)) == 1, "the Medallion entry moved"
    if not none:
        src = ENTRY.sub(lambda _: medallion_entry(break_price, health), src)
    open(items, "w").write(src)
    jsonl = os.path.join(out, "none.jsonl" if none else "med.jsonl")
    subprocess.run([os.path.join(d, "arenasim"), "--batch", jsonl, "--out",
                    os.path.join(out, f"arm{name}.csv"), "--jobs", str(jobs),
                    "--trace-mode", "off"], check=True, stdout=subprocess.DEVNULL)


def load(path):
    rows = {}
    for r in csv.DictReader(open(path)):
        key = (r["team1"], r["team2"], int(r["seed"]))
        assert key not in rows, key
        rows[key] = {"team1": 1.0, "team2": 0.0}.get(r["winner"], 0.5)
    return rows


def analyze(out, a_name, b_name):
    A = load(os.path.join(out, f"arm{a_name}.csv"))
    B = load(os.path.join(out, f"arm{b_name}.csv"))
    keys = sorted(set(A) & set(B))
    d = [A[k] - B[k] for k in keys]
    mean = sum(d) / len(d)
    sd = math.sqrt(sum((x - mean) ** 2 for x in d) / (len(d) - 1))
    fa = sum(1 for x in d if x > 0)
    fb = sum(1 for x in d if x < 0)
    mc = max((abs(fa - fb) - 1) / math.sqrt(fa + fb), 0.0) if fa + fb else 0.0
    print(f"{a_name} vs {b_name}: {len(keys)} paired matches")
    print(f"  delta {100*mean:+.2f}pt (95% +-{196*sd/math.sqrt(len(d)):.2f}); "
          f"flips {fa} / {fb}; McNemar z={mc:.2f}")
    by = defaultdict(lambda: defaultdict(list))
    for k in keys:
        by[k[0].split("+")[0]][(k[2] - SEED_BASE) // 1000].append(A[k] - B[k])
    for cls in sorted(by):
        allv = [x for b in sorted(by[cls]) for x in by[cls][b]]
        m = sum(allv) / len(allv)
        s = math.sqrt(sum((x - m) ** 2 for x in allv) / (len(allv) - 1))
        parts = " / ".join(f"{100*sum(v)/len(v):+5.1f}" for _, v in sorted(by[cls].items()))
        print(f"    {cls:8} {parts}   pooled {100*m:+5.2f} (+-{196*s/math.sqrt(len(allv)):.2f})")


def fires(out, binary, n):
    """Play a seeded sample of `n` Medallion matches alone with their traces,
    and count the wearer's holds, its uses of the Medallion, and its waits for
    a teammate's dispel: a wait ENDS when its last `waiting for` ask is not
    followed within a frame or two by a use — the dispel landed, or the hold
    ended — and is BROKEN when it is (the grace ran out, or the dispeller
    could no longer dispel)."""
    work = os.path.join(out, "fires")
    os.makedirs(work, exist_ok=True)
    lines = open(os.path.join(out, "med.jsonl")).read().splitlines()
    # A seeded random sample: a fixed stride walks the cell grid in step and
    # can miss a class's CC-heavy cells altogether.
    sample = random.Random(305).sample(lines, min(n, len(lines)))
    held = used = 0
    waits_ended = waits_broken = 0
    per_class = Counter()
    per_class_used = Counter()
    for line in sample:
        cfg = json.loads(line)
        open(os.path.join(work, "c.json"), "w").write(line)
        for f in glob.glob(os.path.join(work, "match_logs", "*")):
            os.remove(f)
        # CARGO_MANIFEST_DIR set: logs land in `work/match_logs`, not the
        # player's data directory (`paths::match_log_dir`).
        subprocess.run([binary, "--headless", "c.json", "--trace-mode", "on"], cwd=work,
                       env={**os.environ, "CARGO_MANIFEST_DIR": work},
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        traces = glob.glob(os.path.join(work, "match_logs", "*_trace.jsonl"))
        if not traces:
            continue
        cls = cfg["team1"][0]
        per_class[cls] += 1
        events = []
        for l in open(traces[0]):
            try:
                e = json.loads(l)
            except ValueError:
                continue
            if e.get("kind") == "item_decision":
                events.append(e)
        if events:
            held += 1
        if any(e["status"] == "chosen" for e in events):
            used += 1
            per_class_used[cls] += 1
        for i, e in enumerate(events):
            if "waiting for" not in json.dumps(e.get("reason")):
                continue
            nxt = events[i + 1] if i + 1 < len(events) else None
            if nxt is not None and nxt["sim_time"] - e["sim_time"] < 0.05:
                if nxt["status"] == "chosen":
                    waits_broken += 1
                continue
            waits_ended += 1
    total = sum(per_class.values())
    print(f"{total} matches: wearer held in {held}, used the Medallion in {used} "
          f"({100*used/total:.0f}%); waits for a dispel: {waits_ended} ended with the "
          f"Medallion kept, {waits_broken} broken after the wait")
    for c in sorted(per_class):
        print(f"    {c:8} used in {per_class_used[c]}/{per_class[c]}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    g = sub.add_parser("gen"); g.add_argument("out")
    a = sub.add_parser("arm"); a.add_argument("out"); a.add_argument("name")
    a.add_argument("--bin", required=True); a.add_argument("--break", dest="brk", type=float, default=0.0)
    a.add_argument("--health", type=float, default=0.0); a.add_argument("--none", action="store_true")
    a.add_argument("--jobs", type=int, default=3)
    z = sub.add_parser("analyze"); z.add_argument("out"); z.add_argument("a"); z.add_argument("b")
    f = sub.add_parser("fires"); f.add_argument("out"); f.add_argument("--bin", required=True)
    f.add_argument("--n", type=int, default=240)
    args = ap.parse_args()
    if args.cmd == "gen":
        gen(args.out)
    elif args.cmd == "arm":
        arm(args.out, args.name, os.path.abspath(args.bin), args.brk, args.health, args.none, args.jobs)
    elif args.cmd == "analyze":
        analyze(args.out, args.a, args.b)
    else:
        fires(args.out, os.path.abspath(args.bin), args.n)


if __name__ == "__main__":
    sys.exit(main())
