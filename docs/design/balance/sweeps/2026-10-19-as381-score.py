#!/usr/bin/env python3
"""AS-381 scoring: sight against cover, from the `TeamPlan` healer side.

    2026-10-19-as381-score.py h2h before.csv after.csv
    2026-10-19-as381-score.py frames before.jsonl after.jsonl
    2026-10-19-as381-score.py identical before.csv after.csv

`h2h` reads two `arenasim --batch` runs of one `2026-10-19-as381-gen.py` bank
(rows pair by line) and scores each match from the `TeamPlan` side, which is
the side fielding the bank's one healer: its wins before and after, the
paired flips and McNemar's z on them, sliced by format, seat, healer class and
the number of enemy casters the solve counts (`team_solve::enemy_casters_of`:
every non-melee class plus the Warlock's Felhunter and the Hunter's Spider).

`frames` reads two `tests/healer_cc_exposure.rs` row files of the same bank
and reports, per `TeamPlan` healer-match, the two sides of the trade the card
makes: seconds blind to the solve's anchor and seconds occluded from every
enemy caster, plus caster-seconds of exposure, heal delivered and hard-CC
seconds — over the whole match and over the frames with five or more enemy
casters alive.

`identical` checks two batch CSVs of a uniform bank row for row (winner, end
reason, duration) and prints the non-vacuity counts.
"""
import csv
import json
import math
import sys
from collections import defaultdict

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
MELEE = {"Warrior", "Rogue", "Paladin"}
PET_OWNERS = {"Warlock", "Hunter"}


def casters(team):
    return sum((c not in MELEE) + (c in PET_OWNERS) for c in team)


def bucket(n):
    return "5+" if n >= 5 else str(n) if n >= 3 else "0-2"


def z(lost, gained):
    n = lost + gained
    return 0.0 if n == 0 else (gained - lost) / math.sqrt(n)


def h2h(bp, ap):
    before, after = list(csv.DictReader(open(bp))), list(csv.DictReader(open(ap)))
    assert len(before) == len(after), "the two arms ran different batches"
    cells = defaultdict(lambda: defaultdict(int))
    errors = 0
    for b, a in zip(before, after):
        assert (b["label"], b["seed"]) == (a["label"], a["seed"]), "rows out of step"
        if "error" in (b["winner"], a["winner"]):
            errors += 1
            continue
        seat = b["label"].rsplit("#", 1)[1]
        side, other = ("team1", "team2") if seat == "T1" else ("team2", "team1")
        mine, theirs = b[side].split("+"), b[other].split("+")
        healer = next(c for c in mine if c in HEALERS)
        fmt = f"{len(mine)}v{len(theirs)}"
        wb, wa = b["winner"] == side, a["winner"] == side
        keys = [("all",), ("format", fmt), ("seat", seat), ("healer", healer),
                ("enemy casters", bucket(casters(theirs))),
                ("format x casters", f"{fmt} {bucket(casters(theirs))}")]
        for k in keys:
            c = cells[k]
            c["n"] += 1
            c["wb"] += wb
            c["wa"] += wa
            c["lost"] += wb and not wa
            c["gained"] += wa and not wb
            c["flip"] += (b["winner"], b["duration_secs"]) != (a["winner"], a["duration_secs"])
            c["db"] += b["winner"] == "draw"
            c["da"] += a["winner"] == "draw"
    print(f"{'slice':24} {'n':>6} {'TP wins before':>15} {'after':>7} {'delta':>8} "
          f"{'lost':>5} {'gained':>6} {'z':>6} {'changed':>8} {'draws b/a':>10}")
    for k in sorted(cells, key=lambda k: (k[0] != "all", k)):
        c = cells[k]
        label = " ".join(k)
        n = c["n"]
        print(f"{label:24} {n:>6} {100 * c['wb'] / n:14.1f}% {100 * c['wa'] / n:6.1f}% "
              f"{100 * (c['wa'] - c['wb']) / n:+7.1f}pt {c['lost']:>5} {c['gained']:>6} "
              f"{z(c['lost'], c['gained']):+6.2f} {c['flip']:>8} {c['db']:>5}/{c['da']:<4}")
    print(f"errored rows skipped: {errors}")


def frames(bp, ap):
    def load(p):
        rows = {}
        for line in open(p):
            r = json.loads(line)
            if r["profile"] != "TeamPlan":
                continue
            # The label does not name the map, and one bank holds two.
            key = (r["map"], r["label"], r["seed"], r["side"])
            assert key not in rows, f"duplicate row {key}"
            rows[key] = r
        return rows

    before, after = load(bp), load(ap)
    assert before.keys() == after.keys(), "the two arms ran different banks"
    assert before, "no TeamPlan healer rows"
    fields = ["alive_secs", "blind_secs", "occluded_secs", "exposure", "casters_ge5_secs",
              "blind_ge5_secs", "occluded_ge5_secs", "heal_to_allies", "hard_cc_secs"]
    sums = defaultdict(lambda: defaultdict(float))
    for key, b in before.items():
        a = after[key]
        fmt = f"{len(b['team'].split('+'))}v{len(b['enemy'].split('+'))}"
        cb = bucket(casters(b["enemy"].split("+")))
        for k in [("all",), ("format", fmt), ("enemy casters", cb),
                  ("format x casters", f"{fmt} {cb}"), ("healer", b["class"])]:
            s = sums[k]
            s["n"] += 1
            s["wins b"] += b["result"] == "win"
            s["wins a"] += a["result"] == "win"
            for f in fields:
                s[f + " b"] += b[f]
                s[f + " a"] += a[f]
    cols = ["blind_secs", "occluded_secs", "exposure", "blind_ge5_secs", "occluded_ge5_secs",
            "casters_ge5_secs", "heal_to_allies", "hard_cc_secs", "alive_secs"]
    # The healer-matches the change moved at all, per map: a mean over every
    # row dilutes a rare effect to nothing, so the trade is stated here too.
    print("healer-matches the change moved (any per-frame field or the result differs)")
    for m in sorted({r["map"] for r in before.values()}):
        keys = [k for k in before if before[k]["map"] == m]
        moved = [k for k in keys
                 if any(before[k][f] != after[k][f] for f in fields + ["result", "duration"])]
        print(f"\n{m}: {len(moved)} of {len(keys)} moved")
        for f in cols:
            sb = sum(before[k][f] for k in moved)
            sa = sum(after[k][f] for k in moved)
            print(f"  {f:18} total {sb:10.2f} -> {sa:10.2f}  "
                  f"per moved match {(sa - sb) / max(1, len(moved)):+.3f}")
        results = defaultdict(int)
        for k in moved:
            results[(before[k]["result"], after[k]["result"])] += 1
        print("  result before->after:", dict(sorted(results.items())))
        print("  enemy teams:", ", ".join(sorted({before[k]["enemy"] for k in moved})))
    print()
    print("per TeamPlan healer-match, before -> after (seconds; exposure in caster-seconds)")
    for k in sorted(sums, key=lambda k: (k[0] != "all", k)):
        s = sums[k]
        n = s["n"]
        print(f"\n{' '.join(k)}  (n={int(n)} healer-matches, wins "
              f"{100 * s['wins b'] / n:.1f}% -> {100 * s['wins a'] / n:.1f}%)")
        for f in cols:
            mb, ma = s[f + " b"] / n, s[f + " a"] / n
            print(f"  {f:18} {mb:9.2f} -> {ma:9.2f}  ({ma - mb:+.2f})")


def identical(bp, ap):
    before, after = list(csv.DictReader(open(bp))), list(csv.DictReader(open(ap)))
    assert len(before) == len(after) and before, "empty or mismatched"
    differ = [b["label"] + " " + b["seed"] for b, a in zip(before, after) if b != a]
    kills = sum(b["end_reason"] not in ("timeout", "error") and b["winner"] != "draw" for b in before)
    durations = len({b["duration_secs"] for b in before})
    print(f"{len(before)} rows, {len(differ)} differ; {kills} decided by a kill, "
          f"{durations} distinct durations, "
          f"{sum(b['winner'] == 'draw' for b in before)} draws, "
          f"{sum(b['winner'] == 'error' for b in before)} errors")
    for d in differ[:20]:
        print("  DIFF", d)
    return 1 if differ else 0


if __name__ == "__main__":
    mode = sys.argv[1]
    sys.exit({"h2h": h2h, "frames": frames, "identical": identical}[mode](sys.argv[2], sys.argv[3]) or 0)
