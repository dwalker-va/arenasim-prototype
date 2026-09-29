#!/usr/bin/env python3
"""Landed auto-attacks per hand, per class, from match logs (non-vacuity).

usage: 2026-09-28-as167-swings.py <log files...>
Counts each player combatant's landed auto-attack lines (Auto Attack / Heroic
Strike / Wand Shot / Auto Shot; pets excluded - their ids are pet names) and
the combat seconds it was alive (gates at 10.0s to its death or the match end).
A dual wielder's lines are split by hand on magnitude: an off-hand swing deals
half of an identical dagger's damage, so per (attacker, target) a non-crit
total below 0.583 x the 90th-percentile total is the off hand (0.583 sits
between the off hand's max, 0.5, and the main hand's min, 0.667, of the main
hand's max for Serpent Fang). Crits are halved first.
"""
import re, sys, collections
LINE = re.compile(r"\[\s*([0-9.]+)s\] \[DMG\] Team (\d) (\w+) #(\d)'s (Auto Attack|Heroic Strike|Wand Shot|Auto Shot) (hits|CRITS) Team (\d) (\w+) #(\d) for (\d+) damage(?: \((\d+) absorbed\))?")
DEATH = re.compile(r"\[\s*([0-9.]+)s\] \[DEATH\] Team (\d) (\w+) #(\d) has been eliminated")
DUR = re.compile(r"Duration: ([0-9.]+)s")
CLASSES = {"Warrior","Mage","Rogue","Priest","Warlock","Paladin","Hunter","Shaman"}


def main(paths):
    landed = collections.Counter()   # (class, kind) -> count
    alive = collections.Counter()    # class -> combat seconds
    for path in paths:
        txt = open(path).read()
        dur = float(DUR.search(txt).group(1))
        deaths = {}
        for m in DEATH.finditer(txt):
            deaths[(m.group(2), m.group(3), m.group(4))] = float(m.group(1))
        members = set(re.findall(r"\[EQUIPMENT\] Team (\d) (\w+) #(\d):", txt))
        for key in members:
            if key[1] in CLASSES:
                alive[key[1]] += max(0.0, deaths.get(key, dur) - 10.0)
        per_pair = collections.defaultdict(list)
        for m in LINE.finditer(txt):
            t, team, cls, slot, kind, verb, tteam, tcls, tslot, dmg, absb = m.groups()
            if cls not in CLASSES:
                continue
            total = int(dmg) + int(absb or 0)
            if verb == "CRITS":
                total /= 2
            per_pair[(team, cls, slot, tteam, tcls, tslot, kind)].append(total)
        for (team, cls, slot, _, _, _, kind), totals in per_pair.items():
            if cls == "Rogue" and kind == "Auto Attack":
                s = sorted(totals)
                p90 = s[min(len(s) - 1, int(0.9 * len(s)))]
                off = sum(1 for x in totals if x < 0.583 * p90)
                landed[(cls, "main hand")] += len(totals) - off
                landed[(cls, "off hand")] += off
            else:
                landed[(cls, kind)] += len(totals)
    print(f"{'class':8} {'swing':14} {'landed':>7} {'per combat-sec':>15}")
    for (cls, kind), n in sorted(landed.items()):
        print(f"{cls:8} {kind:14} {n:7d} {n / max(alive[cls], 1e-9):15.3f}")


if __name__ == "__main__":
    main(sys.argv[1:])
