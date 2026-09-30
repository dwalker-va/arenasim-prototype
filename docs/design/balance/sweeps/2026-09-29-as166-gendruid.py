#!/usr/bin/env python3
"""AS-166 Druid no-Rogue control: nothing is hidden, so Flare must never be cast
and every match must be byte-identical. Hunter + partner vs Druid + partner, both
orders, seeds 0-9, with and without kill targets at slot 0; plus 1v1 both orders."""
import json, sys

cells = [
    (["Hunter", "Priest"], ["Druid", "Warrior"]),
    (["Warrior", "Hunter"], ["Mage", "Druid"]),
    (["Hunter", "Druid"], ["Warlock", "Priest"]),
    (["Hunter"], ["Druid"]),
]
for t1, t2 in cells:
    for a, b in ((t1, t2), (t2, t1)):
        for kt in (None, 0):
            for s in range(10):
                c = {"team1": a, "team2": b, "random_seed": s, "max_duration_secs": 300.0,
                     "label": "+".join(a) + "_vs_" + "+".join(b) + ("_kt0" if kt is not None else "")}
                if kt is not None:
                    c["team1_kill_target"] = kt
                    c["team2_kill_target"] = kt
                sys.stdout.write(json.dumps(c) + "\n")
