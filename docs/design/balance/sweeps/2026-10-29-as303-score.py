#!/usr/bin/env python3
"""AS-303 scoring: what E0 (TBC slot multipliers) moves, paired.

    2026-10-29-as303-score.py e0 <before csv[.gz]>... -- <after csv[.gz]>...
    2026-10-29-as303-score.py h2h-shift <before csv[.gz]>... -- <after csv[.gz]>...

`h2h-shift` asks how E0 moves the TeamPlan-against-Legacy effect: for each
TeamPlan side of the `h2h-TL` / `h2h-LT` banks, its head-to-head effect (its
win against its all-Legacy twin's) after E0 minus the same before E0, at the
same cell, seed and seat. The mean is in points, with a t-style z from the
per-side spread and each block's mean.

Every uniform row (`uni-LL`, `uni-TT`; `2026-10-29-as303-gen.py`) in the
AFTER files is paired with its twin in the BEFORE files: the same bank, map,
cell and seed, played by main and by main with E0. Both seats of every match
are scored, so each side's delta is its own win (0/1) after against before; a
draw is a non-win. Reported per profile x format x map x class, competitive
(2v2: each team at most one healer; 3v3: one or two), with each seed block's
value (2026-10-25-0.8-score.py's blocks) and the pooled McNemar z; **R** marks
an effect whose three blocks agree in sign and whose |z| >= 1.96.

The after arm's standings, comps, draws and TeamPlan-against-Legacy figures
are the 0.8 scorer's commands, run on the after files alone:

    2026-10-25-0.8-score.py standings|draws|h2h <after files>
    2026-10-25-0.8-score.py comps 2 6 <after files>
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("score08", os.path.join(HERE, "2026-10-25-0.8-score.py"))
sc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sc)

PROFILE = {"uni-LL": "Legacy", "uni-TT": "TeamPlan"}


def cmd_e0(before_paths, after_paths):
    before = sc.load(before_paths)
    after = sc.load(after_paths)
    acc = {}
    n = 0
    changed = 0
    for key, res in after.items():
        bank, m, t1, t2, s = key
        if bank not in PROFILE:
            continue
        twin = before.get(key)
        if twin is None:
            continue
        n += 1
        changed += twin != res
        size = len(t1.split("+"))
        prof = PROFILE[bank]
        comp = sc.competitive(t1, t2)
        blk = sc.block(s, size)
        for seat, team in ((1, t1), (2, t2)):
            bw, aw = sc.won(twin, seat), sc.won(res, seat)
            keys = [f"{size}v{size} all", f"{size}v{size} {prof} all"]
            if comp:
                keys.append(f"{size}v{size} competitive")
                keys.append(f"{size}v{size} {prof} competitive")
                keys.append(f"{size}v{size} {prof} {sc.MAPNAME[m]} competitive")
                for c in sorted(set(team.split("+"))):
                    keys.append(f"{size}v{size} comp {c}")
                    keys.append(f"{size}v{size} {prof} comp {c}")
                    keys.append(f"{size}v{size} {prof} {sc.MAPNAME[m]} comp {c}")
            for k in keys:
                acc.setdefault(k, sc.Paired()).add(bw, aw, blk)
    print(f"paired matches {n}; outcome (winner, reason or duration) changed in {changed} "
          f"({100.0 * changed / max(n, 1):.1f}%)")
    for size in (2, 3):
        print(f"\n## {size}v{size}: all profiles and maps, competitive, by class")
        order = [f"{size}v{size} all", f"{size}v{size} competitive"]
        order += [f"{size}v{size} comp {c}" for c in sc.CLASSES]
        sc.report(acc, order)
        for prof in ("Legacy", "TeamPlan"):
            print(f"\n## {size}v{size} {prof}, competitive, by class (pooled over maps)")
            order = [f"{size}v{size} {prof} all", f"{size}v{size} {prof} competitive"]
            order += [f"{size}v{size} {prof} comp {c}" for c in sc.CLASSES]
            sc.report(acc, order)
            for m in sc.MAPS:
                mn = sc.MAPNAME[m]
                print(f"\n### {size}v{size} {prof} {mn}, competitive, by class")
                order = [f"{size}v{size} {prof} {mn} competitive"]
                order += [f"{size}v{size} {prof} {mn} comp {c}" for c in sc.CLASSES]
                sc.report(acc, order)


class Shift:
    """A per-side difference d in {-2..2}, with its mean, standard error and
    per-block means."""

    def __init__(self):
        self.xs = []
        self.blocks = {}

    def add(self, d, blk):
        self.xs.append(d)
        self.blocks.setdefault(blk, []).append(d)

    def row(self, name):
        n = len(self.xs)
        mean = sum(self.xs) / n
        var = sum((x - mean) ** 2 for x in self.xs) / max(n - 1, 1)
        se = (var / n) ** 0.5
        z = mean / se if se else 0.0
        bl = [sum(v) / len(v) for _, v in sorted(self.blocks.items())]
        rep = len(bl) == 3 and abs(z) >= 1.96 and (all(b > 0 for b in bl) or all(b < 0 for b in bl))
        return (f"| {name} | {n} | {100 * mean:+.1f} | " + " / ".join(f"{100 * b:+.1f}" for b in bl)
                + f" | {z:+.2f}{' **R**' if rep else ''} |")


def cmd_h2h_shift(before_paths, after_paths):
    """How E0 moves the TeamPlan side's head-to-head effect: per TeamPlan side,
    d = (h2h win - LL twin win) after E0, minus the same before E0, at the
    same cell, seed and seat."""
    before = sc.load(before_paths)
    after = sc.load(after_paths)
    acc = {}
    n = 0
    for (bank, m, t1, t2, s), res in after.items():
        seat = {"h2h-TL": 1, "h2h-LT": 2}.get(bank)
        if seat is None:
            continue
        rows = [after.get(("uni-LL", m, t1, t2, s)), before.get((bank, m, t1, t2, s)),
                before.get(("uni-LL", m, t1, t2, s))]
        if any(r is None for r in rows):
            continue
        a_ll, b_h2h, b_ll = rows
        d = (sc.won(res, seat) - sc.won(a_ll, seat)) - (sc.won(b_h2h, seat) - sc.won(b_ll, seat))
        n += 1
        size = len(t1.split("+"))
        team, opp = (t1, t2) if seat == 1 else (t2, t1)
        keys = [f"{size}v{size} all", f"{size}v{size} map={sc.MAPNAME[m]}"]
        if sc.competitive(team, opp):
            keys.append(f"{size}v{size} competitive")
            keys.append(f"{size}v{size} competitive map={sc.MAPNAME[m]}")
            for c in sorted(set(team.split("+"))):
                keys.append(f"{size}v{size} comp has {c}")
        for k in keys:
            acc.setdefault(k, Shift()).add(d, sc.block(s, size))
    print(f"TeamPlan sides with all four twins: {n}")
    print("| slice | n | shift (pt) | per block | z |\n|---|---|---|---|---|")
    for size in (2, 3):
        order = [f"{size}v{size} all", f"{size}v{size} competitive"]
        order += [f"{size}v{size} competitive map={sc.MAPNAME[m]}" for m in sc.MAPS]
        order += [f"{size}v{size} comp has {c}" for c in sc.CLASSES]
        for k in order:
            if k in acc:
                print(acc[k].row(k))


def cmd_h2h(paths):
    """The 0.8 milestone doc's §2 table, from one arm's files: the TeamPlan
    side against its all-Legacy twin, competitive by map, and by class on the
    pillar maps (the 0.8 scorer's `h2h` prints the slices without these)."""
    data = sc.load(paths)
    acc = {}
    n1 = sc.accumulate(acc, sc.pair_sides(data, "h2h-TL", "uni-LL", seat_of=1))
    n2 = sc.accumulate(acc, sc.pair_sides(data, "h2h-LT", "uni-LL", seat_of=2))
    print(f"paired TeamPlan sides: seat 1 {n1}, seat 2 {n2}")
    order = []
    for size in (2, 3):
        order += [f"{size}v{size} competitive"]
        order += [f"{size}v{size} competitive map={sc.MAPNAME[m]}" for m in sc.MAPS]
        order += [f"{size}v{size}", f"{size}v{size} no healer", f"{size}v{size} double healer"]
        order += [f"{size}v{size} double healer map=Nagrand"]
        order += [f"{size}v{size} pillars comp has {c}" for c in sc.CLASSES]
    sc.report(acc, order)


def main(argv):
    if argv and argv[0] == "h2h":
        cmd_h2h(argv[1:])
        return
    if not argv or argv[0] not in ("e0", "h2h-shift") or "--" not in argv:
        sys.exit(__doc__)
    i = argv.index("--")
    {"e0": cmd_e0, "h2h-shift": cmd_h2h_shift}[argv[0]](argv[1:i], argv[i + 1:])


if __name__ == "__main__":
    main(sys.argv[1:])
