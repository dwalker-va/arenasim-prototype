#!/usr/bin/env python3
"""AS-303 scoring: what E0 (TBC slot multipliers) moves, paired.

    2026-10-29-as303-score.py e0 <before csv[.gz]>... -- <after csv[.gz]>...

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


def main(argv):
    if not argv or argv[0] != "e0" or "--" not in argv:
        sys.exit(__doc__)
    i = argv.index("--")
    cmd_e0(argv[1:i], argv[i + 1:])


if __name__ == "__main__":
    main(sys.argv[1:])
