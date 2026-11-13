#!/usr/bin/env python3
"""AS-440 banks (DIRECTIONAL): the healer hide rules, head-to-head.

    2026-11-13-as440-gen.py diag <seeds>            > diag.jsonl
    2026-11-13-as440-gen.py uni <rule> <seeds>      > uni.jsonl
    2026-11-13-as440-gen.py h2h <rule> <seeds>      > h2h.jsonl
    2026-11-13-as440-gen.py ctl <seeds>             > ctl.jsonl

Cells: the play banks' 30 x 30 2v2 cells (`2026-10-25-0.8-gen.py play_cells`),
on PillaredArena and TwinPillars, both teams TeamPlan, 300s cap.

- diag: every cell where a healer + DPS team meets a team with a ranged DPS,
  the shipped solve (the diagnosis, `tests/healer_hide_exposure.rs`).
- uni: as diag, both teams on <rule> (the rule's mechanism, traced).
- h2h: every cell whose team 1 (seat 1) or team 2 (seat 2) is a healer + DPS
  team; that team runs <rule> (`team{n}_hide_rule`, or `NoPillar`: the
  Pillar play withheld), the other the shipped solve. Label `h2h-<rule>-t<seat>|map|team1|team2|seed`.
- ctl: the h2h cells with both teams shipped, label `ctl|...` — the twin every
  h2h row is paired with.

Seeds: three blocks, 801 / 811 / 821 (one seed each: DIRECTIONAL).
"""
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("gen08", os.path.join(HERE, "2026-10-25-0.8-gen.py"))
g = importlib.util.module_from_spec(spec)
spec.loader.exec_module(g)

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
RANGED = {"Mage", "Hunter", "Warlock"}
MAPS = ("PillaredArena", "TwinPillars")


def healer_team(t):
    return len(t) == 2 and sum(c in HEALERS for c in t) == 1


def rule_config(rule, seat):
    """A rule's config for team `seat`: a `HideRule`, or `NoPillar` — the
    Pillar play withheld (no pillar stance, so no healer hug). `A+B` sets
    both."""
    out = {}
    for part in rule.split("+"):
        if part == "NoPillar":
            out[f"team{seat}_withheld_plays"] = ["Pillar"]
        else:
            out[f"team{seat}_hide_rule"] = part
    return out


def emit(rows):
    for r in rows:
        print(json.dumps(r))


def main(argv):
    bank = argv[0]
    cells = g.play_cells(2)
    if bank in ("diag", "uni"):
        rule = argv[1] if bank == "uni" else None
        ss = g.seeds(argv[-1])

        def ok(a, b):
            return healer_team(a) and any(c in RANGED for c in b)

        for m in MAPS:
            for a, b in cells:
                if not (ok(a, b) or ok(b, a)):
                    continue
                for s in ss:
                    extra = {} if rule is None else {"team1_hide_rule": rule, "team2_hide_rule": rule}
                    emit([g.row(bank if rule is None else f"uni-{rule}", m, a, b, s,
                                ai_profile="TeamPlan", **extra)])
    elif bank == "h2h":
        rule, ss = argv[1], g.seeds(argv[2])
        for seat in (1, 2):
            for m in MAPS:
                for a, b in cells:
                    if not healer_team((a, b)[seat - 1]):
                        continue
                    for s in ss:
                        emit([g.row(f"h2h-{rule}-t{seat}", m, a, b, s, ai_profile="TeamPlan",
                                    **rule_config(rule, seat))])
    elif bank == "ctl":
        ss = g.seeds(argv[1])
        for m in MAPS:
            for a, b in cells:
                if not (healer_team(a) or healer_team(b)):
                    continue
                for s in ss:
                    emit([g.row("ctl", m, a, b, s, ai_profile="TeamPlan")])
    else:
        sys.exit(f"unknown bank {bank}")


if __name__ == "__main__":
    main(sys.argv[1:])
