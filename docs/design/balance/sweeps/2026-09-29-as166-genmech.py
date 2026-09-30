#!/usr/bin/env python3
"""AS-166 mechanism set: Hunter + each partner vs Rogue + each partner, with
and without kill targets at slot 0 (the Rogue), seeds 0-9; plus Hunter vs
Rogue 1v1 in both slot orders, seeds 0-19."""
import json, sys

out = sys.stdout
for p in ["Priest", "Paladin", "Shaman", "Warrior", "Mage", "Warlock", "Rogue"]:
    for x in ["Priest", "Paladin", "Warlock", "Shaman", "Mage", "Warrior"]:
        for kt in (None, 0):
            for s in range(10):
                c = {"team1": ["Hunter", p], "team2": ["Rogue", x], "random_seed": s,
                     "max_duration_secs": 300.0, "label": f"Hunter+{p}_vs_Rogue+{x}"}
                if kt is not None:
                    c["team1_kill_target"] = kt
                    c["team2_kill_target"] = kt
                out.write(json.dumps(c) + "\n")
for s in range(20):
    out.write(json.dumps({"team1": ["Hunter"], "team2": ["Rogue"], "random_seed": s,
                          "max_duration_secs": 300.0, "label": "Hunter_vs_Rogue"}) + "\n")
    out.write(json.dumps({"team1": ["Rogue"], "team2": ["Hunter"], "random_seed": s,
                          "max_duration_secs": 300.0, "label": "Rogue_vs_Hunter"}) + "\n")
