#!/usr/bin/env python3
"""AS-389 reducer: a ranged DPS's lining time (blind to its own target within
reach, free to act), split by what held it there. Input: rows written by
`tests/team_sight_cohesion.rs` (its `ranged` field), plain or gzipped.

    2026-10-25-as389-reduce.py rows.jsonl[.gz] ...

Prints, per map and class, and per (own package, enemy package) when the
rows mix them:

- lining as a share of post-contact alive time;
- the exclusive cause partition (see `RangedBlind` in the harness), as a
  share of lining, with the endgame (`last:` — the target is the enemy's last
  living non-pet) split out;
- overlapping tallies: target a healer, an alternative in reach and sight,
  own yield, own and enemy stance, the chase's Point goal, moving, the
  nearest regain spot's distance, and whether it keeps the healer in sight;
- lining by the length of the episode it belongs to.
"""
import collections
import gzip
import json
import sys

KEYS = ["tgt_healer", "alt", "own_yield", "own_stance", "enemy_stance",
        "point_goal", "moving", "regain_keeps_healer",
        "regain_near_keeps_healer", "last_enemy", "outnumber"]


def rows(paths):
    for p in paths:
        opener = gzip.open if p.endswith(".gz") else open
        with opener(p, "rt") as f:
            for line in f:
                if line.strip():
                    yield json.loads(line)


def main(paths):
    acc = collections.defaultdict(lambda: collections.defaultdict(float))
    mixed = set()
    for r in rows(paths):
        mp = "Nagrand" if r["map"] == "PillaredArena" else r["map"]
        for u in r.get("ranged", []):
            pk = f"own={'on' if u['own_pkg'] else 'off'} enemy={'on' if u['enemy_pkg'] else 'off'}"
            mixed.add(pk)
            for key in [(mp, u["class"], pk), (mp, "ALL", pk), ("both", u["class"], pk),
                        ("both", "ALL", pk)]:
                a = acc[key]
                a["n"] += 1
                a["secs"] += u["secs"]
                a["lining"] += u["lining"]
                for k in KEYS:
                    a[k] += u.get(k, 0.0)
                for k, v in u["cause"].items():
                    a["cause:" + k] += v
                    a["cause*:" + k.replace("last:", "")] += v
                for k, v in u["regain"].items():
                    a["regain:" + k] += v
                for k, v in u["episode"].items():
                    a["ep:" + k] += v
    causes = ["pressed", "switchable", "yield_blocked", "filling", "beyond_pref",
              "chasing", "pursuing"]
    for key in sorted(acc):
        a = acc[key]
        if a["secs"] == 0:
            continue
        L = a["lining"] or 1e-9
        print(f"\n== {key[0]} {key[1]} {key[2]}  units={int(a['n'])}  "
              f"lining {100 * a['lining'] / a['secs']:.1f}% of {a['secs'] / 3600:.1f}h")
        print("  cause (all / of which endgame):  " + "  ".join(
            f"{c} {100 * a['cause*:' + c] / L:.1f}/{100 * a['cause:last:' + c] / L:.1f}"
            for c in causes if a["cause*:" + c] > 0))
        print("  " + "  ".join(f"{k} {100 * a[k] / L:.1f}" for k in KEYS))
        regs = sorted(k for k in a if k.startswith("regain:"))
        print("  regain dist: " + "  ".join(f"{k[7:]} {100 * a[k] / L:.1f}" for k in regs))
        eps = sorted(k for k in a if k.startswith("ep:"))
        print("  episode len: " + "  ".join(f"{k[3:]} {100 * a[k] / L:.1f}" for k in eps))


if __name__ == "__main__":
    main(sys.argv[1:])
