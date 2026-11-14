#!/usr/bin/env python3
"""AS-129: healers dispel pets. Builds the sweep's derived inputs and scores it
from the side the change reaches.

The change reaches a match only through a team holding a dispelling healer
(Priest, Paladin) AND a pet class (Hunter, Warlock): that healer can now free
its pet, and an enemy Hunter's trap AI now counts that pet as freeable. Every
other match must be byte-identical, which `sides` checks first.

    # 2v2 BasicArena / Legacy: every cell an affected class reaches, 8 controls
    scripts/gen_sweep.py --full 2 --exclude-double-healer \\
      --affects Priest,Paladin,Hunter,Warlock --control-cells 8 --n 10 > all2.jsonl
    # 2v2 PillaredArena / TeamPlan: its reachable and control cells
    2026-11-13-as129-pet-dispel.py nagrand all2.jsonl > nag2.jsonl
    # 3v3 BasicArena / Legacy: 300 reachable cells by digest, 5 seeds, + controls
    scripts/gen_sweep.py --full 3 --exclude-double-healer \\
      --affects Priest,Paladin,Hunter,Warlock --control-cells 3 --n 1 > all3.jsonl
    2026-11-13-as129-pet-dispel.py r3 all3.jsonl > r3.jsonl

    <binary> --batch <set>.jsonl --out <set>-<arm>.csv --jobs 4 --trace-mode off
    2026-11-13-as129-pet-dispel.py sides <set>-before.csv <set>-after.csv
"""

import csv
import hashlib
import json
import math
import sys

HEALERS = {"Priest", "Paladin"}
PET_CLASSES = {"Hunter", "Warlock"}


def reached(team):
    """A team the change reaches: a dispelling healer and a pet."""
    return bool(HEALERS & set(team)) and bool(PET_CLASSES & set(team))


def unaffected(cfg):
    return not (set(cfg["team1"]) | set(cfg["team2"])) & (HEALERS | PET_CLASSES)


def cmd_nagrand(path):
    for line in open(path):
        cfg = json.loads(line)
        if reached(cfg["team1"]) or reached(cfg["team2"]) or unaffected(cfg):
            cfg["map"] = "PillaredArena"
            cfg["ai_profile"] = "TeamPlan"
            print(json.dumps(cfg))


def cmd_r3(path):
    cells = {}
    for line in open(path):
        cfg = json.loads(line)
        cells.setdefault(cfg["label"], cfg)
    reach = [c for c in cells.values() if reached(c["team1"]) or reached(c["team2"])]
    control = [c for c in cells.values() if unaffected(c)]
    reach.sort(key=lambda c: hashlib.sha1(c["label"].encode()).hexdigest())
    for cfg in reach[:300] + control:
        for seed in range(5):
            row = dict(cfg)
            row["random_seed"] = 1000 + seed
            print(json.dumps(row))


def wilson(k, n, z=1.96):
    p = k / n
    d = 1 + z * z / n
    c = p + z * z / (2 * n)
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return (c - h) / d * 100, (c + h) / d * 100


def cmd_sides(before_path, after_path):
    def load(p):
        return {(r["label"], r["seed"]): r for r in csv.DictReader(open(p))}

    before, after = load(before_path), load(after_path)
    assert set(before) == set(after), "the arms must hold the same matches"
    keys = sorted(before)

    def teams(row):
        return row["team1"].split("+"), row["team2"].split("+")

    def outcome(row):
        return row["winner"], row["duration_secs"]

    same = moved = 0
    for k in keys:
        t1, t2 = teams(before[k])
        if not reached(t1) and not reached(t2):
            if outcome(before[k]) == outcome(after[k]):
                same += 1
            else:
                moved += 1
    print(f"UNREACHED (no healer+pet team on either side): {same} identical, {moved} moved")
    total = sum(1 for k in keys if outcome(before[k]) != outcome(after[k]))
    print(f"moved overall: {total}/{len(keys)}")

    def sides(keep):
        rows = []
        for k in keys:
            t1, t2 = teams(before[k])
            for seat, me, them in ((1, t1, t2), (2, t2, t1)):
                if keep(me, them):
                    win = f"team{seat}"
                    rows.append((before[k]["winner"] == win, after[k]["winner"] == win))
        return rows

    def report(name, rows):
        n = len(rows)
        if n == 0:
            print(f"{name}: n=0")
            return
        kb = sum(b for b, _ in rows)
        ka = sum(a for _, a in rows)
        up = sum(1 for b, a in rows if a and not b)
        down = sum(1 for b, a in rows if b and not a)
        z = (up - down) / math.sqrt(up + down) if up + down else 0.0
        lb, hb = wilson(kb, n)
        la, ha = wilson(ka, n)
        print(
            f"{name:52s} n={n:5d} before {kb / n * 100:5.1f}% [{lb:.1f}-{hb:.1f}] "
            f"after {ka / n * 100:5.1f}% [{la:.1f}-{ha:.1f}] "
            f"delta {(ka - kb) / n * 100:+5.1f}pt flips {up + down} (+{up}/-{down}) z={z:+.2f}"
        )

    report("healer+pet side v a side without one", sides(lambda me, op: reached(me) and not reached(op)))
    for healer in ("Priest", "Paladin"):
        report(f"  {healer}+pet side", sides(lambda me, op, h=healer: reached(me) and h in me))
    for pet in ("Warlock", "Hunter"):
        report(f"  healer+{pet} side", sides(lambda me, op, p=pet: reached(me) and p in me))
    report(
        "healer+Warlock side v a Hunter team",
        sides(lambda me, op: reached(me) and "Warlock" in me and "Hunter" in op),
    )
    report(
        "healer+Warlock side v a team without a Hunter",
        sides(lambda me, op: reached(me) and "Warlock" in me and "Hunter" not in op),
    )
    report(
        "Hunter side v Rogue+healer",
        sides(lambda me, op: "Hunter" in me and "Rogue" in op and HEALERS & set(op)),
    )


def main(argv):
    if len(argv) == 3 and argv[1] == "nagrand":
        cmd_nagrand(argv[2])
    elif len(argv) == 3 and argv[1] == "r3":
        cmd_r3(argv[2])
    elif len(argv) == 4 and argv[1] == "sides":
        cmd_sides(argv[2], argv[3])
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
