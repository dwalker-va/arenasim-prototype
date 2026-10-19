"""Two `healer_cc_exposure` row files side by side, per map and healer class:
hard CC seconds, nominal reach-type CC seconds, Psychic Scream seconds, heal to
teammates, heading reversals per minute and wins, all per healer-match.

    2026-10-18-as362-compare.py A.jsonl B.jsonl

AS-362 uses it for TeamPlan on main (A) against TeamPlan with AS-292's rule
merged onto the same main (B).
"""
import json, sys
def load(p): return [json.loads(l) for l in open(p)]
a = load(sys.argv[1]); b = load(sys.argv[2])
REACH = {"Fear","Polymorph","Cyclone","Entangling Roots","Death Coil","Psychic Scream","Frost Nova","Hammer of Justice","Freezing Trap"}
for m in ["BasicArena", "PillaredArena"]:
    for cls in ["Priest", "Paladin", "Shaman", "Druid", "ALL"]:
        out = []
        for rs in (a, b):
            r = [x for x in rs if x["map"] == m and (cls == "ALL" or x["class"] == cls)]
            n = len(r)
            hard = sum(x["hard_cc_secs"] for x in r) / n
            reach = sum(e["dur"] for x in r for e in x["cc_events"] if e["ability"] in REACH) / n
            scream = sum(e["dur"] for x in r for e in x["cc_events"] if e["ability"] == "Psychic Scream") / n
            heal = sum(x["heal_to_allies"] for x in r) / n
            alive = sum(x["alive_secs"] for x in r)
            rev = sum(x["reversals"] for x in r) / (alive / 60)
            wins = sum(x["result"] == "win" for x in r)
            out.append(f"hard {hard:.2f} reach {reach:.2f} scream {scream:.2f} heal {heal:.0f} rev {rev:.1f} win {wins}/{n}")
        print(f"{m:13} {cls:7} | A: {out[0]} | B: {out[1]}")
