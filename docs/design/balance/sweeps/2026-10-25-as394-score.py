#!/usr/bin/env python3
"""AS-394 scoring: the lone-healer endgame, read off `tests/hunt_endgame_sweep.rs`
rows, paired at identical seeds.

    2026-10-25-as394-score.py endgame <base.rows> <arm.rows> [seat]
        seat: 1 or 2 for a head-to-head arm (only that team hunts): the
        endgames in which that team is the outnumbering side. Omitted: every
        endgame (a uniform arm).

    2026-10-25-as394-score.py csv <rows>
        The rows as a `--batch` CSV, for `2026-10-24-as385-package-score.py`
        (win rates: `seat` and `uniform`).

The endgame: one team reduced to its lone healer while the other still fields
two or more non-pet units; it lasts to the match's end. Reported per slice:

- the endgames each arm reaches, their mean and median length, how they end
  (the outnumbering side wins / the healer's side wins / a draw), and the
  share of their frames the healer stood in sight of the hunting side
  (`seen`) and of two of its units from bearings 120° or more apart round its
  pillar (`split`);
- PAIRED: the matches that reach the endgame, with the same hider, in both
  arms: the mean of the per-match difference in its length, its standard
  error and t, and the median difference.
"""
import collections
import csv
import gzip
import json
import math
import statistics
import sys

# The classes that can hold a Hunt station: the ranged DPS, and the healers
# `hunt::HUNT_EXCEPTIONS` does not name (the Paladin, Druid and Shaman are
# excepted).
STATIONABLE = {"Mage", "Hunter", "Warlock", "Priest"}


def rows(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return {r["label"]: r for r in (json.loads(l) for l in f if l.strip())}


def slices(r):
    eg = r["endgame"]
    mp = "Nagrand" if r["map"] == "PillaredArena" else r["map"]
    hunters = r[f"team{3 - eg['hider_team']}"].split("+")
    fmt = "3v3" if len(r["team1"].split("+")) == 3 else "2v2"
    staffed = sum(1 for c in hunters if c in STATIONABLE) >= 2
    keys = ["all", f"map={mp}", f"fmt={fmt}", f"hider={eg['hider_class']}",
            f"map={mp} hider={eg['hider_class']}",
            "hunters staffed (2+ stationable)" if staffed else "hunters unstaffed"]
    for c in set(hunters):
        keys.append(f"hunter has {c}")
    return keys


def outcome(r):
    eg = r["endgame"]
    hider = f"team{eg['hider_team']}"
    if r["winner"] == "draw":
        return "draw"
    return "lose" if r["winner"] == hider else "win"


def endgame(base, arm, seat):
    acc = collections.defaultdict(lambda: {"b": [], "a": [], "pairs": [], "bo": collections.Counter(),
                                           "ao": collections.Counter(), "bseen": [], "aseen": [],
                                           "bsplit": [], "asplit": []})
    for label, a in arm.items():
        b = base.get(label)
        if b is None:
            continue
        for tag, r in (("b", b), ("a", a)):
            eg = r["endgame"]
            if eg is None or (seat and eg["hider_team"] != 3 - seat):
                continue
            for k in slices(r):
                x = acc[k]
                x[tag].append(eg["secs"])
                x[tag + "o"][outcome(r)] += 1
                x[tag + "seen"].append(eg["seen"])
                x[tag + "split"].append(eg["split"])
        eb, ea = b["endgame"], a["endgame"]
        if eb and ea and eb["hider_team"] == ea["hider_team"] and (
                not seat or ea["hider_team"] == 3 - seat):
            for k in slices(a):
                acc[k]["pairs"].append(ea["secs"] - eb["secs"])
    title = f"seat {seat} hunts" if seat else "every endgame"
    print(f"### Endgame, {title}\n")
    print("| slice | n base/arm | mean base → arm | median base → arm | win/lose/draw base | win/lose/draw arm "
          "| seen base → arm | split base → arm | paired n | paired mean Δ (t) | paired median Δ |")
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    order = lambda k: (k != "all", not k.startswith("map=") or " " in k, k)
    for k in sorted(acc, key=order):
        x = acc[k]
        if not x["a"] or not x["b"]:
            continue
        p = x["pairs"]
        if len(p) > 1:
            se = statistics.stdev(p) / math.sqrt(len(p))
            t = statistics.mean(p) / se if se else 0.0
            paired = f"{statistics.mean(p):+.1f} ({t:+.1f}) | {statistics.median(p):+.1f}"
        else:
            paired = "- | -"
        fmt_o = lambda c: f"{c['win']}/{c['lose']}/{c['draw']}"
        print(f"| {k} | {len(x['b'])}/{len(x['a'])} | {statistics.mean(x['b']):.1f} → "
              f"{statistics.mean(x['a']):.1f} | {statistics.median(x['b']):.1f} → "
              f"{statistics.median(x['a']):.1f} | {fmt_o(x['bo'])} | {fmt_o(x['ao'])} | "
              f"{statistics.mean(x['bseen']):.2f} → {statistics.mean(x['aseen']):.2f} | "
              f"{statistics.mean(x['bsplit']):.2f} → {statistics.mean(x['asplit']):.2f} | "
              f"{len(p)} | {paired} |")
    print()


def to_csv(path):
    w = csv.writer(sys.stdout)
    w.writerow(["label", "team1", "team2", "seed", "winner", "end_reason", "duration_secs"])
    for r in rows(path).values():
        reason = "cap" if r["winner"] == "draw" and r["duration"] >= 299.9 else "kill"
        w.writerow([r["label"], r["team1"], r["team2"], r["seed"], r["winner"], reason,
                    f"{r['duration']:.2f}"])


def main():
    kind = sys.argv[1]
    if kind == "endgame":
        seat = int(sys.argv[4]) if len(sys.argv) > 4 else 0
        endgame(rows(sys.argv[2]), rows(sys.argv[3]), seat)
    elif kind == "csv":
        to_csv(sys.argv[2])
    else:
        sys.exit(f"unknown kind {kind}")


main()
