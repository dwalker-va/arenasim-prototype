#!/usr/bin/env python3
import csv, sys
from collections import Counter, defaultdict

def main(argv):
    for d in argv:
        rows = list(csv.DictReader(open(f"{d}/traps.csv")))
        print(f"==== {d}: {len(rows)} traps thrown")
        by = defaultdict(list)
        for r in rows:
            by[r["comp"]].append(r)
        tot = Counter()
        for comp, rs in by.items():
            c = Counter()
            for r in rs:
                c["thrown"] += 1
                if r["fate"] != "unsprung":
                    c["sprung"] += 1
                    hit = r["sprung_class"] == r["intended_class"]
                    c["hit" if hit else "miss"] += 1
                c[r["fate"]] += 1
                if r["intended_is_kill_target"] == "True":
                    c["aimed_at_kill_target"] += 1
            tot.update(c)
            vic = Counter(r["sprung_class"] for r in rs if r["sprung_class"])
            aim = Counter(r["intended_class"] for r in rs)
            early = sum(1 for r in rs if float(r["cast_t"]) < 16.0)
            print(f"{comp:12s} thrown={c['thrown']:3d} sprung={c['sprung']:3d} hit={c['hit']:3d} "
                  f"miss={c['miss']:3d} removed={c['removed']:3d} broke={c['broke']:3d} "
                  f"ran_out={c['ran_out']:3d} unsprung={c['unsprung']:3d} aimKT={c['aimed_at_kill_target']:3d} "
                  f"early<16s={early:2d} aim={dict(aim)} victims={dict(vic)}")
        print("TOTAL", dict(tot))


if __name__ == "__main__":
    main(sys.argv[1:])
