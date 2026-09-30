#!/usr/bin/env python3
"""AS-166: summarise flaremech.py rows, before vs after, per enemy comp and
kill-target setting.

usage: flaresumm.py <before matches.csv> <after matches.csv>

Per row: n; Rogue openers landed before -> after (of n); Flares cast per
match; reveal rate = Flares that revealed the Rogue / Flares cast; matches
whose Rogue was revealed by a Flare; Freezing Traps sprung on the Rogue /
on its partner, before -> after; Hunter-side wins before -> after with the
paired flips.
"""
import csv, sys
from collections import defaultdict


def load(p):
    return {(r["label"], r["seed"], r["kt1"], r["kt2"]): r for r in csv.DictReader(open(p))}


def main(b, a):
    before, after = load(b), load(a)
    groups = defaultdict(list)
    for k, rb in before.items():
        ra = after.get(k)
        if not ra:
            continue
        kt = "kt0" if rb["kt1"] != "" else "none"
        groups[(rb["enemy"], kt)].append((rb, ra))
    print("| enemy | kill targets | n | openers | flares/m | reveal rate | found by Flare | "
          "traps on Rogue | traps on partner | Hunter wins | flips +/- |")
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    tot = defaultdict(int)
    for (enemy, kt), rows in sorted(groups.items(), key=lambda x: (x[0][1], x[0][0])):
        n = len(rows)
        ob = sum(r["opener"] != "" for r, _ in rows)
        oa = sum(r["opener"] != "" for _, r in rows)
        fl = sum(int(r["flares"]) for _, r in rows)
        fr = sum(int(r["flare_reveals"]) for _, r in rows)
        found = sum(r["revealed_by"] == "Flare" for _, r in rows)
        trb = sum(int(r["traps_on_rogue"]) for r, _ in rows)
        tra = sum(int(r["traps_on_rogue"]) for _, r in rows)
        tob = sum(int(r["traps_on_other"]) for r, _ in rows)
        toa = sum(int(r["traps_on_other"]) for _, r in rows)
        wb = sum(int(r["hunter_won"]) for r, _ in rows)
        wa = sum(int(r["hunter_won"]) for _, r in rows)
        g = sum(int(ra["hunter_won"]) and not int(rb["hunter_won"]) for rb, ra in rows)
        l = sum(int(rb["hunter_won"]) and not int(ra["hunter_won"]) for rb, ra in rows)
        rate = f"{fr}/{fl}" if fl else "-"
        print(f"| {enemy} | {kt} | {n} | {ob} -> {oa} | {fl/n:.2f} | {rate} | {found} | "
              f"{trb} -> {tra} | {tob} -> {toa} | {wb} -> {wa} | +{g}/-{l} |")
        for key, v in (("n", n), ("ob", ob), ("oa", oa), ("fl", fl), ("fr", fr), ("found", found),
                       ("wb", wb), ("wa", wa), ("g", g), ("l", l)):
            tot[(kt, key)] += v
    for kt in ("none", "kt0"):
        t = lambda k: tot[(kt, k)]
        if not t("n"):
            continue
        print(f"\n{kt}: n={t('n')} openers {t('ob')} -> {t('oa')}; flares {t('fl')} "
              f"({t('fl')/t('n'):.2f}/m), reveals {t('fr')} ({t('fr')/max(1,t('fl')):.0%}); "
              f"Hunter wins {t('wb')} -> {t('wa')} flips +{t('g')}/-{t('l')}")


if __name__ == "__main__":
    main(*sys.argv[1:3])
