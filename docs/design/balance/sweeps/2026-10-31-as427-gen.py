#!/usr/bin/env python3
"""AS-427: put Hand of Justice on every melee swinger in a gen_sweep.py JSONL.

Reads a gen_sweep.py batch on stdin and writes it back with Hand of Justice in
Trinket2 for every member of a class that swings a melee weapon (Warrior,
Rogue, Paladin, Shaman, Druid). Wand users and the Hunter wear their default
kit, so a cell with none of those classes is unchanged between the arms and is
the bit-exactness control.

Every arm plays the SAME JSONL; the arms differ only in what Hand of Justice
is (`2026-10-31-as427-arms.py` builds them), so each match has a twin that
differs only by the extra swing traded for attack power at the declared price.

    scripts/gen_sweep.py --full 2 --exclude-double-healer \\
      --affects Warrior,Rogue,Paladin,Shaman,Druid --control-cells 8 --n 10 \\
      | docs/design/balance/sweeps/2026-10-31-as427-gen.py > sweep.jsonl
"""
import json
import sys

SWINGERS = {"Warrior", "Rogue", "Paladin", "Shaman", "Druid"}


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        cfg = json.loads(line)
        for team in ("team1", "team2"):
            cfg[f"{team}_equipment"] = [
                {"Trinket2": "HandOfJustice"} if cls in SWINGERS else {}
                for cls in cfg[team]
            ]
        print(json.dumps(cfg))


if __name__ == "__main__":
    main()
