#!/usr/bin/env python3
"""AS-125: tabulate trapmech.py output, per comp, one arm or two.

usage: trapsumm.py <before_outdir> [<after_outdir>]
Per comp: matches, Hunter-side wins, traps thrown per match, matches with no
trap at all, median first throw (s after the gates), where the traps sprang
(on the intended victim / someone else), and how they ended (removed by an
enemy dispel / broken by damage / ran the full duration / never sprung).
"""
import csv, statistics, sys
from collections import Counter, defaultdict


def load(d):
    traps = defaultdict(list)
    for r in csv.DictReader(open(f"{d}/traps.csv")):
        traps[r["comp"]].append(r)
    matches = defaultdict(list)
    for r in csv.DictReader(open(f"{d}/matches.csv")):
        matches[r["comp"]].append(r)
    return traps, matches


def summarise(traps, matches, comp):
    ms, ts = matches[comp], traps[comp]
    c = Counter(r["fate"] for r in ts)
    hit = sum(1 for r in ts if r["fate"] != "unsprung" and r["sprung_class"] == r["intended_class"])
    firsts = [float(m["first_cast_after_gates"]) for m in ms if m["first_cast_after_gates"]]
    return {
        "n": len(ms),
        "win": sum(1 for m in ms if m["winner"] == "Team 1"),
        "traps": len(ts),
        "per_match": len(ts) / len(ms) if ms else 0.0,
        "zero": sum(1 for m in ms if m["casts"] == "0"),
        "first": statistics.median(firsts) if firsts else None,
        "hit": hit,
        "miss": sum(1 for r in ts if r["fate"] != "unsprung") - hit,
        "removed": c["removed"], "broke": c["broke"], "ran_out": c["ran_out"],
        "unsprung": c["unsprung"],
    }


def fmt(s):
    first = f"{s['first']:5.1f}" if s["first"] is not None else "    -"
    return (f"win {s['win']:2d}/{s['n']:<2d} traps {s['traps']:3d} ({s['per_match']:.2f}/m) "
            f"zero {s['zero']:2d} first {first} hit {s['hit']:3d} miss {s['miss']:3d} "
            f"rem {s['removed']:3d} brk {s['broke']:3d} full {s['ran_out']:3d} uns {s['unsprung']:2d}")


def main(argv):
    arms = [load(d) for d in argv]
    comps = list(dict.fromkeys(c for _, m in arms for c in m))
    totals = [Counter() for _ in arms]
    for comp in comps:
        for i, (t, m) in enumerate(arms):
            s = summarise(t, m, comp)
            label = comp if i == 0 else ""
            print(f"{label:12s} {'before' if i == 0 and len(arms) > 1 else 'after ' if i else '      '} {fmt(s)}")
            totals[i].update({k: v for k, v in s.items() if k not in ("first", "per_match")})
    for i, tot in enumerate(totals):
        print(f"TOTAL arm {i}:", dict(tot))


if __name__ == "__main__":
    main(sys.argv[1:])
