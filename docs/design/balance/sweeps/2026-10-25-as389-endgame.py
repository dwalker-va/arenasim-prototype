#!/usr/bin/env python3
"""AS-389: the lone-healer endgame — one team reduced to its healer alone
while the other still fields two or more — read off the deaths in
`tests/team_sight_cohesion.rs` rows.

    2026-10-25-as389-endgame.py rows.jsonl[.gz] ...

Per map, format and the surviving healer's class: how many matches reach
the endgame, how long it lasts (to the match's end), how it ends (the
outnumbering side wins, the healer's side wins, a draw), and how much of
it falls after dampening's onset (75s after the gates).
"""
import collections
import gzip
import json
import statistics
import sys

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
DAMP = 75.0


def rows(paths):
    for p in paths:
        opener = gzip.open if p.endswith(".gz") else open
        with opener(p, "rt") as f:
            for line in f:
                if line.strip():
                    yield json.loads(line)


def main(paths):
    acc = collections.defaultdict(lambda: {"n": 0, "eg": 0, "secs": [], "win": 0, "lose": 0,
                                           "draw": 0, "post_damp": 0.0, "dur": []})
    for r in rows(paths):
        mp = "Nagrand" if r["map"] == "PillaredArena" else r["map"]
        comps = {1: r["team1"].split("+"), 2: r["team2"].split("+")}
        fmt = f"{len(comps[1])}v{len(comps[2])}"
        alive = {t: len(comps[t]) for t in (1, 2)}
        dead = {1: [], 2: []}
        onset = None
        for d in sorted(r["deaths"], key=lambda d: d["t"]):
            alive[d["team"]] -= 1
            dead[d["team"]].append(d["class"])
            if onset is None:
                for t in (1, 2):
                    living = [c for c in comps[t] if c not in dead[t] or
                              dead[t].count(c) < comps[t].count(c)]
                    if (alive[t] == 1 and alive[3 - t] >= 2 and len(living) == 1
                            and living[0] in HEALERS):
                        onset = (d["t"], t, living[0])
        keys = [("all", fmt, "-"), (mp, fmt, "-")]
        if onset:
            keys += [("all", fmt, onset[2]), (mp, fmt, onset[2])]
        for k in keys:
            a = acc[k]
            a["n"] += 1
            a["dur"].append(r["duration"])
            if onset and (k[2] == "-" or k[2] == onset[2]):
                t0, hteam, _ = onset
                a["eg"] += 1
                end = r["duration"]  # match_time, from the gates, as the deaths
                secs = max(end - t0, 0.0)
                a["secs"].append(secs)
                a["post_damp"] += max(end - max(t0, DAMP), 0.0)
                if r["winner"] == 3 - hteam:
                    a["win"] += 1
                elif r["winner"] == hteam:
                    a["lose"] += 1
                else:
                    a["draw"] += 1
    for k in sorted(acc):
        a = acc[k]
        if not a["eg"]:
            continue
        s = a["secs"]
        print(f"{k[0]:12} {k[1]} {k[2]:8} matches {a['n']:5}  endgame {a['eg']:4} "
              f"({100 * a['eg'] / a['n']:.0f}%)  secs median {statistics.median(s):5.1f} "
              f"mean {statistics.mean(s):5.1f}  post-damp share "
              f"{100 * a['post_damp'] / max(sum(s), 1e-9):.0f}%  "
              f"outnumbering side wins {100 * a['win'] / a['eg']:.1f}%  healer wins "
              f"{100 * a['lose'] / a['eg']:.1f}%  draw {100 * a['draw'] / a['eg']:.1f}%  "
              f"endgame share of all match time {100 * sum(s) / max(sum(a['dur']), 1e-9):.0f}%")


if __name__ == "__main__":
    main(sys.argv[1:])
