#!/usr/bin/env python3
"""Reduce `tests/healer_cc_exposure.rs` rows into the AS-362 tables.

    2026-10-18-as362-reduce.py rows.jsonl [rows2.jsonl ...]

Every figure is a RATE over healer-matches (or over alive seconds), with the
totals it rests on printed beside it, so a table never reads a total as a rate.
"""
import collections
import json
import statistics
import sys

HEAL_RANGE = 40.0
# Reach of each CC ability, as the config declares it (the harness prints the
# same list; repeated here for the "step" column).
REACH = {
    "Frost Nova": 10.0, "Polymorph": 30.0, "Cheap Shot": 2.5, "Kidney Shot": 2.5,
    "Psychic Scream": 8.0, "Fear": 30.0, "Death Coil": 30.0,
    "Hammer of Justice": 10.0, "Freezing Trap": 30.0, "Entangling Roots": 30.0,
    "Cyclone": 20.0,
}
HARD = {"Stun", "Fear", "Polymorph", "Incapacitate", "Cyclone"}

rows = []
for path in sys.argv[1:]:
    with open(path) as f:
        rows += [json.loads(l) for l in f if l.strip()]


def key(r):
    return (r["profile"], r["map"])


groups = collections.defaultdict(list)
for r in rows:
    groups[key(r)].append(r)

print("## Hard CC per healer-match, by healer class")
print()
print("| profile | map | healer | n | hard CC s/match | % of alive time | Stun | Fear | Poly | Incap | Cyclone | Root | Lockout | heal/match | rev/min |")
print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    by_class = collections.defaultdict(list)
    for r in rs:
        by_class[r["class"]].append(r)
    for cls in ["Priest", "Paladin", "Shaman", "Druid", "ALL"]:
        cr = rs if cls == "ALL" else by_class[cls]
        n = len(cr)
        alive = sum(r["alive_secs"] for r in cr)
        hard = sum(r["hard_cc_secs"] for r in cr)
        def t(ty):
            return sum(r["cc_secs"].get(ty, 0.0) for r in cr) / n
        heal = sum(r["heal_to_allies"] for r in cr) / n
        rev = sum(r["reversals"] for r in cr) / (alive / 60.0)
        print(f"| {prof} | {m} | {cls} | {n} | {hard/n:.2f} | {100*hard/alive:.1f}% | "
              f"{t('Stun'):.2f} | {t('Fear'):.2f} | {t('Polymorph'):.2f} | {t('Incapacitate'):.2f} | "
              f"{t('Cyclone'):.2f} | {t('Root'):.2f} | {t('SpellSchoolLockout'):.2f} | {heal:.0f} | {rev:.2f} |")
print()

print("## CC landings on healers, by ability")
print()
print("A landing is a `[CC]` log line on the healer. `src` is the nearest living enemy")
print("of the ability's class at that moment; `src-ally` is that source's distance to the")
print("healer's nearest teammate (small = the teammate fights at the source's feet);")
print("`step` = yards the healer stood")
print("inside the reach (reach - src distance), median over landings.")
print()
print("| profile | map | ability | landings | per healer-match where possible | nominal s/landing | median src dist | median src-ally | median step |")
print("|---|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    ev = collections.defaultdict(list)
    # Denominator: healer-matches whose enemy fields the ability's class.
    for r in rs:
        for e in r["cc_events"]:
            # Only the crowd-control abilities: the log's `[CC]` channel also
            # carries slows, Unstable Affliction's silence and a second "lands"
            # line for Fear.
            if e["ability"] in REACH:
                ev[e["ability"]].append((r, e))
    for ab in sorted(ev, key=lambda a: -len(ev[a])):
        es = ev[ab]
        src_cls = es[0][1]["src_class"]
        possible = sum(1 for r in rs if src_cls in r["enemy"].split("+"))
        dists = [e["src_dist"] for _, e in es if e["src_dist"] is not None]
        reach = REACH.get(ab)
        sa = [e["src_ally_dist"] for _, e in es if e["src_ally_dist"] is not None]
        steps = [reach - e["src_dist"] for _, e in es if reach and e["src_dist"] is not None]
        print(f"| {prof} | {m} | {ab} | {len(es)} | {len(es)/max(possible,1):.2f} (n={possible}) | "
              f"{statistics.mean(e['dur'] for _, e in es):.1f} | "
              f"{statistics.median(dists) if dists else float('nan'):.1f} | "
              f"{statistics.median(sa) if sa else float('nan'):.1f} | {statistics.median(steps) if steps else float('nan'):.1f} |")
print()

print("## Nominal CC seconds per healer-match, by family")
print()
print("Sum of the logged (DR-scaled) durations of landings, per healer-match. Nominal:")
print("a fear or polymorph that broke early is counted in full. `chaser` = melee-range")
print("stuns (Kidney Shot, Cheap Shot); `reach` = everything with a reach disc a healer")
print("can stand outside (casters' CC, Psychic Scream, Hammer of Justice, Frost Nova,")
print("Freezing Trap).")
print()
CHASER = {"Kidney Shot", "Cheap Shot"}
print("| profile | map | chaser s | reach s | of which caster CC (Fear, Poly, Death Coil, Cyclone, Roots) | Scream | HoJ | Nova | Trap |")
print("|---|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    n = len(rs)
    tot = collections.Counter()
    for r in rs:
        for e in r["cc_events"]:
            if e["ability"] in REACH:
                tot[e["ability"]] += e["dur"]
    chaser = sum(v for k, v in tot.items() if k in CHASER) / n
    reach = sum(v for k, v in tot.items() if k not in CHASER) / n
    caster = sum(tot[k] for k in ["Fear", "Polymorph", "Death Coil", "Cyclone", "Entangling Roots"]) / n
    print(f"| {prof} | {m} | {chaser:.2f} | {reach:.2f} | {caster:.2f} | {tot['Psychic Scream']/n:.2f} | "
          f"{tot['Hammer of Justice']/n:.2f} | {tot['Frost Nova']/n:.2f} | {tot['Freezing Trap']/n:.2f} |")
print()

print("## Exposure: share of a healer's alive time inside each enemy CC's reach")
print()
print("Reach = range from `AbilityDefinitions`, in line of sight; cooldown ignored.")
print("Denominator: alive seconds of healers whose enemy fields the ability's class.")
print()
print("| profile | map | ability | exposed % | healer-matches |")
print("|---|---|---|---|---|")
owner = {"Frost Nova": "Mage", "Polymorph": "Mage", "Cheap Shot": "Rogue",
         "Kidney Shot": "Rogue", "Psychic Scream": "Priest", "Fear": "Warlock",
         "Death Coil": "Warlock", "Hammer of Justice": "Paladin",
         "Freezing Trap": "Hunter", "Entangling Roots": "Druid", "Cyclone": "Druid"}
for (prof, m), rs in sorted(groups.items()):
    for ab, cls in owner.items():
        cr = [r for r in rs if cls in r["enemy"].split("+")]
        alive = sum(r["alive_secs"] for r in cr)
        exp = sum(r["exposed_secs"].get(ab, 0.0) for r in cr)
        print(f"| {prof} | {m} | {ab} | {100*exp/max(alive,1e-9):.1f}% | {len(cr)} |")
print()

print("## Totals (non-vacuity)")
for (prof, m), rs in sorted(groups.items()):
    nev = sum(len(r["cc_events"]) for r in rs)
    print(f"- {prof} {m}: {len(rs)} healer-matches, {nev} CC landings, "
          f"{sum(r['hard_cc_secs'] for r in rs):.0f}s hard CC, "
          f"{sum(1 for r in rs if r['result']=='win')} wins / "
          f"{sum(1 for r in rs if r['result']=='draw')} draws")
