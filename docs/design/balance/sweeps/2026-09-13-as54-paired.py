#!/usr/bin/env python3
"""Paired before/after analysis for AS-54 (card: Frost Armor chill).

PAIRED design — same seed, comp and map on both sides — so the instrument is
McNemar's test on the matches whose outcome FLIPPED. Wilson 95% intervals are
reported for the level, not used as the test.

A Frost Armor chill exists in a match only when ONE side fields a Mage (every
Mage wears Frost Armor by default) and the OTHER side fields a melee attacker
for it to proc on. Every pet in the game melees, so a Hunter or a Warlock
brings one even though the class itself is ranged. team1 always contains a
Mage here, but team1's PARTNER can be a melee and team2 can field a Mage of its
own, so the chill runs in both directions and the slices have to say which.
"""
import csv
import math
import sys
from collections import OrderedDict

MELEE = {"Warrior", "Rogue", "Paladin"}
PET_OWNERS = {"Hunter", "Warlock"}          # every pet in the game is melee
REMOVERS = {"Warlock", "Hunter"}            # Devour Magic / Master's Call


def wilson(wins, n, z=1.96):
    if n == 0:
        return (0.0, 1.0)
    p = wins / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    m = (z / d) * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return (max(0.0, c - m), min(1.0, c + m))


def load(path):
    rows, errs = OrderedDict(), 0
    with open(path) as f:
        for r in csv.DictReader(f):
            w = r["winner"].strip()
            if w == "error":
                errs += 1
                continue
            rows[(r["label"], r["seed"])] = (w, r["team1"], r["team2"],
                                             float(r["duration_secs"]))
    return rows, errs


def cls(team):
    return set(c for c in team.replace("|", "+").split("+") if c)


def melee(team):
    return bool(cls(team) & MELEE) or bool(cls(team) & PET_OWNERS)


def mage(team):
    return "Mage" in cls(team)


def chills_them(t1, t2):
    """team1's Frost Armor can chill someone on team2."""
    return mage(t1) and melee(t2)


def chills_us(t1, t2):
    """team2's Frost Armor can chill someone on team1."""
    return mage(t2) and melee(t1)


def remover(team):
    return bool(cls(team) & REMOVERS)


def mcnemar(b, c):
    n = b + c
    return (abs(b - c) - 1) / math.sqrt(n) if n > 0 else 0.0


def report(name, keys, before, after):
    n = len(keys)
    if n == 0:
        print(f"{name:<46} (no matches)")
        return
    bw = sum(1 for k in keys if before[k][0] == "team1")
    aw = sum(1 for k in keys if after[k][0] == "team1")
    to_win = sum(1 for k in keys
                 if before[k][0] != "team1" and after[k][0] == "team1")
    to_loss = sum(1 for k in keys
                  if before[k][0] == "team1" and after[k][0] != "team1")
    moved = sum(1 for k in keys if before[k][0] != after[k][0])
    dur = sum(1 for k in keys if before[k][3] != after[k][3])
    blo, bhi = wilson(bw, n)
    alo, ahi = wilson(aw, n)
    z = mcnemar(to_win, to_loss)
    print(f"{name:<46} n={n:<5} "
          f"before {100*bw/n:5.1f}% [{100*blo:.1f}-{100*bhi:.1f}]  "
          f"after {100*aw/n:5.1f}% [{100*alo:.1f}-{100*ahi:.1f}]  "
          f"delta {100*(aw-bw)/n:+5.1f}pt  "
          f"flips {moved:4d} (+{to_win}/-{to_loss}) z={z:4.2f} "
          f"{'sig' if z >= 1.96 else 'ns '}  durations moved {dur}/{n}")


def main():
    before, berr = load(sys.argv[1])
    after, aerr = load(sys.argv[2])
    keys = [k for k in before if k in after]
    print(f"paired matches: {len(keys)}  "
          f"(errors before={berr} after={aerr})")
    print()

    for arm, size in (("2v2", 2), ("3v3", 3)):
        ks = [k for k in keys
              if len(cls(before[k][1])) == size]
        if not ks:
            continue
        t1 = lambda k: before[k][1]
        t2 = lambda k: before[k][2]
        print(f"--- {arm}: team1 = Mage + partner(s), winrate is team1's ---")
        report("ALL opponents", ks, before, after)
        report("our chill can land (enemy has melee)",
               [k for k in ks if chills_them(t1(k), t2(k))], before, after)
        report("  ...enemy can remove it",
               [k for k in ks if chills_them(t1(k), t2(k)) and remover(t2(k))],
               before, after)
        report("  ...enemy cannot remove it",
               [k for k in ks
                if chills_them(t1(k), t2(k)) and not remover(t2(k))],
               before, after)
        report("MIRRORED (enemy Mage chills us too)",
               [k for k in ks if chills_us(t1(k), t2(k))], before, after)
        report("CONTROL: no chill possible either way",
               [k for k in ks
                if not chills_them(t1(k), t2(k))
                and not chills_us(t1(k), t2(k))],
               before, after)
        print()

    # Whole-sweep control, stated as the bit-exactness claim it is.
    ctl = [k for k in keys
           if not chills_them(before[k][1], before[k][2])
           and not chills_us(before[k][1], before[k][2])]
    same = sum(1 for k in ctl
               if before[k][0] == after[k][0] and before[k][3] == after[k][3])
    print(f"CONTROL (whole sweep): {len(ctl)} matches in which no Frost Armor "
          f"chill can exist; {same}/{len(ctl)} are identical in BOTH winner "
          f"and duration.")
    decisive = sum(1 for k in keys if after[k][0] in ("team1", "team2"))
    moved_any = sum(1 for k in keys
                    if before[k][0] != after[k][0] or before[k][3] != after[k][3])
    print(f"non-vacuity: {decisive}/{len(keys)} paired matches ended by "
          f"elimination; {moved_any} matches moved in winner or duration.")


if __name__ == "__main__":
    main()
