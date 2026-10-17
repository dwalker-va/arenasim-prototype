#!/usr/bin/env python3
"""AS-290 batches: the Peel play's context gates.

usage: gen.py <batch> > batch.jsonl

Every batch is drawn from AS-274's head-to-head (`../2026-10-12-as274-gen.py
h2h`): every 2v2 team of two distinct classes, no double healer, against every
other, on PillaredArena and BasicArena, both teams under TeamPlan, in three
arms per seed — NN (Peel withheld from both), PN (team 1 peels), NP (team 2
peels). The seeds are split three ways, so a gate is never confirmed or
validated on the seeds that picked it:

  selection     seeds 1-4    h2h       the AS-274 design on main (21,600)
                             abl-X     the PN/NP rows whose peeling team has
                                       class X, for X in the 8 classes with a
                                       peel tool; run with X gated
  confirmation  seeds 5-16   conf      PN/NP rows of the 16 teams holding a
                                       candidate context; run ungated and gated
                             tool      PN/NP rows of the 5 teams whose gate has
                                       two tools; run with each tool gated
                             pet       PN/NP rows of the 8 Hunter teams
  validation    seeds 17-20  final     all three arms, the shipped binary
                             lifted    the PN/NP rows with the peeling side's
                                       gates lifted (team{1,2}_lifted_gates)
                             nn        the NN rows alone (main's control)
                             legacy    ../2026-10-12-as274-gen.py legacy 2

The Burst play (AS-275) then landed, and runs in every TeamPlan arm. On that
base the selection was re-run for the contexts the validation flagged, and
the gates validated again on seeds no gate had seen:

  re-selection  seeds 5-16   reselect  PN/NP rows of the 8 Rogue teams and
                                       Warlock+Shaman; run with the shipped
                                       gates, and with the candidates added
                                       (AS290_EXTRA_GATES, `extra-gates.patch`)
  validation    seeds 21-24  final2, lifted2, nn2: as final, lifted, nn

The selection and confirmation arms gate by an environment variable the arm
patch adds (`arms.patch`, against e1e97de): AS290_PEEL_GATES takes
`Peeler@Ally` or `Peeler:Tool@Ally` items separated by `;`, and
AS290_PETPEEL / AS290_CANCEL / AS290_KEEP switch on the three improvement
arms. The validation batches need no patch: the gates ship, and lifting them
is a config field.
"""
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location(
    "as274gen", os.path.join(HERE, "..", "2026-10-12-as274-gen.py"))
as274 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(as274)

CLASSES_WITH_TOOLS = ["Mage", "Warlock", "Paladin", "Hunter", "Rogue", "Druid", "Priest", "Shaman"]
CONF_TEAMS = [
    "Hunter+Mage", "Hunter+Paladin", "Hunter+Priest", "Hunter+Rogue", "Hunter+Shaman",
    "Hunter+Druid", "Hunter+Warrior", "Hunter+Warlock", "Warlock+Mage", "Warlock+Paladin",
    "Warlock+Priest", "Warlock+Rogue", "Warlock+Shaman", "Warlock+Druid", "Warlock+Warrior",
    "Warrior+Druid",
]
TOOL_TEAMS = ["Mage+Hunter", "Mage+Warlock", "Hunter+Druid", "Warlock+Druid", "Warrior+Druid"]
PET_TEAMS = [t for t in CONF_TEAMS if t.startswith("Hunter+")]

# The env gates each arm ran with.
CONF_GATES = ";".join(
    [f"{p}@Hunter" for p in ["Mage", "Warlock", "Paladin", "Rogue", "Druid", "Priest", "Shaman"]]
    + [f"{p}@Warlock" for p in ["Mage", "Paladin", "Hunter", "Rogue", "Druid", "Priest", "Shaman"]]
    + ["Druid@Warrior"])
TOOL_FIRST = "Mage:FrostNova@Hunter;Mage:FrostNova@Warlock;Druid:Cyclone@Hunter;Druid:Cyclone@Warlock;Druid:Cyclone@Warrior"
TOOL_SECOND = ("Mage:Polymorph@Hunter;Mage:Polymorph@Warlock;Druid:EntanglingRoots@Hunter;"
               "Druid:EntanglingRoots@Warlock;Druid:EntanglingRoots@Warrior")


def rows(lo, hi):
    """Every row of the AS-274 head-to-head at seeds lo..hi, as dicts."""
    out = []
    real = sys.stdout

    class Sink:
        def write(self, s):
            for line in s.splitlines():
                if line.strip():
                    out.append(json.loads(line))

    sys.stdout = Sink()
    try:
        as274.main("h2h", hi)
    finally:
        sys.stdout = real
    return [r for r in out if lo <= r["random_seed"] <= hi]


# The validation seeds: 17-20 on the pre-Burst base, 21-24 on the Burst base.
VALIDATION = {False: (17, 20), True: (21, 24)}


def arm(r):
    return r["label"].rsplit("#", 1)[1]


def peeling(r):
    a = arm(r)
    return r["team1"] if a == "PN" else r["team2"] if a == "NP" else None


def of_teams(rs, teams):
    want = {frozenset(t.split("+")) for t in teams}
    return [r for r in rs if peeling(r) and frozenset(peeling(r)) in want]


def main(batch):
    if batch == "h2h":
        sel = rows(1, 4)
    elif batch.startswith("abl-"):
        x = batch[4:]
        sel = [r for r in rows(1, 4) if peeling(r) and x in peeling(r)]
    elif batch == "conf":
        sel = of_teams(rows(5, 16), CONF_TEAMS)
    elif batch == "tool":
        sel = of_teams(rows(5, 16), TOOL_TEAMS)
    elif batch == "pet":
        sel = of_teams(rows(5, 16), PET_TEAMS)
    elif batch == "reselect":
        rs = rows(5, 16)
        sel = [r for r in rs if peeling(r) and ("Rogue" in peeling(r)
                                                or frozenset(peeling(r)) == {"Warlock", "Shaman"})]
    elif batch in ("final", "final2"):
        sel = rows(*VALIDATION[batch[-1] == "2"])
    elif batch in ("nn", "nn2"):
        sel = [r for r in rows(*VALIDATION[batch[-1] == "2"]) if arm(r) == "NN"]
    elif batch in ("lifted", "lifted2"):
        sel = []
        for r in rows(*VALIDATION[batch[-1] == "2"]):
            if arm(r) == "NN":
                continue
            side = "team1" if arm(r) == "PN" else "team2"
            sel.append({**r, f"{side}_lifted_gates": ["Peel"]})
    else:
        sys.exit(f"unknown batch {batch}")
    for r in sel:
        print(json.dumps(r))


if __name__ == "__main__":
    main(sys.argv[1])
