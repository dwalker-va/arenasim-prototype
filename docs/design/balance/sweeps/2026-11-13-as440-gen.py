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

- cover <seeds>: the shipped hand-off cover rule, head-to-head (AS-440
  round 3). Every cell whose team 1 (seat 1) or team 2 (seat 2) is a healer +
  DPS team: that team runs the rule, the other has it off
  (`team{n}_handoff_rule: false`), label `h2h-Cover-t<seat>`; and the twin
  with it off for both, label `ctl|...`.
- h2h-eligible <rule> <seeds>: as h2h, only the sides the pillar stance can
  reach — a Priest, Shaman or Druid side against an enemy that fields a crowd
  control the stance defends against (the nine enemies in `NO_STANCE_ENEMIES`
  never move the stance: zero flips in the 12-seed NoPillar bank). The round-2
  decomposition (`StanceHealerHalf`, `StancePartnerHalf`).
- targeted <seeds> <comp,comp,...> [NoCover]: as h2h NoPillar, but the Pillar
  play is withheld only for the running sides whose comp (`Dps+Healer`) is
  named; every other running side is shipped (with `NoCover`, every running
  side also runs the cover rule).

Seeds: round 1, one seed per block (801 / 811 / 821); round 2, four
(801-804 / 811-814 / 821-824), and fresh blocks 831-834 and 841-844 for
confirmation.
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


NO_STANCE_ENEMIES = {"Warrior+Rogue", "Warrior+Priest", "Warrior+Hunter", "Warrior+Shaman",
                     "Rogue+Priest", "Rogue+Hunter", "Rogue+Shaman", "Priest+Hunter", "Hunter+Shaman"}


def healer_team(t):
    return len(t) == 2 and sum(c in HEALERS for c in t) == 1


def rule_config(rule, seat):
    """A rule's config for team `seat`: `NoPillar`, the Pillar play withheld
    (no pillar stance, so no healer hug). The round-1/2 prototype rules
    (`NoCover`, `NoHandoff`, `Follow`, `StanceHealerHalf`, `StancePartnerHalf`)
    were measurement switches on `team{n}_hide_rule`, removed when the cover
    rule shipped (AS-440 round 3); their banks are the committed CSVs, played
    by the binary at cfbf3a54. The shipped rule's own control is the `cover`
    bank."""
    out = {}
    for part in rule.split("+"):
        if part == "NoPillar":
            out[f"team{seat}_withheld_plays"] = ["Pillar"]
        else:
            sys.exit(f"{part}: a round-1/2 prototype rule, removed (see the docstring)")
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
    elif bank == "cover":
        ss = g.seeds(argv[1])
        off = {"team1_handoff_rule": False, "team2_handoff_rule": False}
        for m in MAPS:
            for a, b in cells:
                if not (healer_team(a) or healer_team(b)):
                    continue
                for s in ss:
                    emit([g.row("ctl", m, a, b, s, ai_profile="TeamPlan", **off)])
        for seat in (1, 2):
            for m in MAPS:
                for a, b in cells:
                    if not healer_team((a, b)[seat - 1]):
                        continue
                    for s in ss:
                        emit([g.row(f"h2h-Cover-t{seat}", m, a, b, s, ai_profile="TeamPlan",
                                    **{f"team{3 - seat}_handoff_rule": False})])
    elif bank in ("h2h-eligible", "targeted"):
        if bank == "h2h-eligible":
            rule, ss, named, nc = argv[1], g.seeds(argv[2]), None, False
        else:
            ss, named, nc = g.seeds(argv[1]), set(argv[2].split(",")), len(argv) > 3
        for seat in (1, 2):
            for m in MAPS:
                for a, b in cells:
                    side, enemy = (a, b)[seat - 1], (a, b)[2 - seat]
                    if not healer_team(side):
                        continue
                    name = "+".join([c for c in side if c not in HEALERS] + [c for c in side if c in HEALERS])
                    for s in ss:
                        if bank == "h2h-eligible":
                            if "Paladin" in side or "+".join(enemy) in NO_STANCE_ENEMIES:
                                continue
                            emit([g.row(f"h2h-{rule}-t{seat}", m, a, b, s, ai_profile="TeamPlan",
                                        **rule_config(rule, seat))])
                        else:
                            extra = {}
                            if name in named:
                                extra[f"team{seat}_withheld_plays"] = ["Pillar"]
                            if nc:
                                extra[f"team{seat}_hide_rule"] = "NoCover"
                            label = "TargetedNC" if nc else "Targeted"
                            emit([g.row(f"h2h-{label}-t{seat}", m, a, b, s, ai_profile="TeamPlan", **extra)])
    else:
        sys.exit(f"unknown bank {bank}")


if __name__ == "__main__":
    main(sys.argv[1:])
