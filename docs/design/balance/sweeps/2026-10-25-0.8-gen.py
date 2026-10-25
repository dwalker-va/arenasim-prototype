#!/usr/bin/env python3
"""AS-395 banks: the 0.8 milestone sweep (AUTHORITY).

    2026-10-25-0.8-gen.py <bank> <args...> > bank.jsonl

Every config carries a label `bank|map|team1|team2|seed` (teams joined by
`+`), because the `--batch` CSV does not carry the map or the arm. The scorer
(`2026-10-25-0.8-score.py`) parses it.

Seeds. Every bank draws from three independent seed BLOCKS, fresh for this
card (no earlier card used 801-830 or 841-852):

    2v2   block 1: 801-810   block 2: 811-820   block 3: 821-830
    3v3   block 1: 801       block 2: 811       block 3: 821

The head-to-head and focus banks use the first four seeds of each 2v2 block
(801-804, 811-814, 821-824), and the play and harness banks the first two
(801-802, 811-812, 821-822), so every one of their rows has a twin in the
uniform baselines at the same seed and cell. The 3v3 head-to-head runs on the
`sample3` cells. The Legacy-since-0.7 bank uses
seeds 0-29 / 0-2, the 0.7 canonical CSVs' own seeds, so it pairs with them.

Banks
-----
uni <map> <profile> <size> <seeds>
    The full matrix: every distinct-class team of <size> against every one
    (36 x 36 in 2v2, 84 x 84 in 3v3, mirrors included), both teams on
    <profile> (Legacy | TeamPlan). The class standings.
h2h <map> <seat> <size> <seeds> [competitive|sample]
    Team <seat> on TeamPlan, the other on Legacy; otherwise as `uni`. With
    `competitive`, only cells whose both teams field one healer (2v2) or one
    or two (3v3); with `sample`, the 3v3 `sample3` cells (the 3v3 run).
    Paired with `uni ... Legacy` (LL) at the same seeds.
play <play> <arm> <map> <size> <seeds> [classes]
    Both teams TeamPlan. With [classes] (comma-separated), only the rows
    whose running team holds one of them (the confirmation runs). arm: NN (the play withheld from both), PN (team 1
    runs it), NP (team 2 runs it), PNx / NPx (as PN / NP with the running
    side's exceptions table lifted: `team{n}_exceptions: {"<play>": []}`).
    Cells: 2v2, every team of two distinct classes with at most one healer
    against every such team (30 x 30, AS-274's design); 3v3, a fixed sample
    of 600 cells from the teams with one or two healers (`sample3`).
pillar-h2h <arm> <map> <seeds>
    3v3 sample cells, team 1 TeamPlan against a Legacy team 2 and the mirror
    seat; arm: on (shipped) / off (Pillar withheld from the TeamPlan side).
    The broad replication of AS-391's 3v3 stance effect.
focus-off <map> <size> <seeds>
    As `uni ... TeamPlan`, with the AS-389 target focus off for both teams
    (`team{1,2}_target_focus: false`); 3v3 on the `sample3` cells. Paired with
    the uniform TeamPlan baseline: the focus's uniform (balance) effect.

peel-cand <gate|-|NN> <seeds> <teams>
    The Peel play bank's PN / NP rows whose running team is one of <teams>
    (comma-separated, `Paladin+Hunter`), with the shipped gates plus <gate>
    (`Peeler/Ally`), `-` for the shipped table alone, or `NN` for the play
    withheld from both: a candidate gate's confirmation run.
hunt-bank <shipped|lifted> <seeds>, sight-bank shipped <seeds>
    AS-385's package cells, which AS-394 measured the Hunt exceptions on,
    both teams TeamPlan; `lifted` lifts the Hunt exceptions for both teams
    (`team{1,2}_exceptions: {"Hunt": []}`). Run through the observed
    harnesses, not `--batch`.

Every match is 300s-capped, default loadouts.
"""
import hashlib
import importlib.util
import itertools
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location(
    "gen_sweep", os.path.join(HERE, "..", "..", "..", "..", "scripts", "gen_sweep.py"))
gs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gs)

HEALERS = gs.HEALERS
MAPS = ("BasicArena", "PillaredArena", "TwinPillars")
PLAYS = ("Peel", "TakeCover", "Hunt", "Pillar", "Chain", "Burst")
# `peel::PEEL_GATES` at 40c75048, in the config's "Peeler/Ally" spelling.
PEEL_SHIPPED = ["Mage/Hunter", "Mage/Warlock", "Druid/Hunter", "Druid/Warlock", "Druid/Warrior",
                "Shaman/Warlock", "Rogue/Hunter", "Rogue/Warrior", "Rogue/Priest"]


def seeds(spec_):
    out = []
    for part in spec_.split(","):
        if "-" in part:
            lo, hi = (int(x) for x in part.split("-"))
            out.extend(range(lo, hi + 1))
        else:
            out.append(int(part))
    return out


def teams(size, max_healers=None, min_healers=0):
    out = []
    for t in itertools.combinations(gs.CLASSES, size):
        h = sum(1 for c in t if c in HEALERS)
        if max_healers is not None and h > max_healers:
            continue
        if h < min_healers:
            continue
        out.append(list(t))
    return out


def full_cells(size):
    ts = teams(size)
    return [(a, b) for a in ts for b in ts]


def competitive_cells(size):
    ts = teams(size, max_healers=1, min_healers=1) if size == 2 else teams(size, 2, 1)
    return [(a, b) for a in ts for b in ts]


def play_cells(size):
    if size == 2:
        ts = teams(2, max_healers=1)
        return [(a, b) for a in ts for b in ts]
    return sample3()


def sample3(k=600):
    """A fixed, digest-ordered sample of 3v3 cells whose teams each field one
    or two healers. Order by a stable digest, as `gen_sweep.digest_order`."""
    cells = competitive_cells(3)
    order = gs.digest_order(cells)
    return [cells[i] for i in sorted(order[:k])]


def row(bank, m, t1, t2, s, **extra):
    r = {
        "team1": t1,
        "team2": t2,
        "map": m,
        "random_seed": s,
        "max_duration_secs": 300.0,
        "label": f"{bank}|{m}|{'+'.join(t1)}|{'+'.join(t2)}|{s}",
    }
    r.update(extra)
    return r


def emit(rows):
    n = 0
    for r in rows:
        sys.stdout.write(json.dumps(r) + "\n")
        n += 1
    print(f"# wrote {n}", file=sys.stderr)


def main(argv):
    bank = argv[0]
    if bank == "uni":
        m, prof, size, ss = argv[1], argv[2], int(argv[3]), seeds(argv[4])
        assert m in MAPS and prof in ("Legacy", "TeamPlan")
        tag = "LL" if prof == "Legacy" else "TT"
        emit(row(f"uni-{tag}", m, a, b, s, ai_profile=prof)
             for a, b in full_cells(size) for s in ss)
    elif bank == "h2h":
        m, seat, size, ss = argv[1], int(argv[2]), int(argv[3]), seeds(argv[4])
        which = argv[5] if len(argv) > 5 else "full"
        cells = {"full": full_cells, "competitive": competitive_cells,
                 "sample": lambda _size: sample3()}[which](size)
        tag = "TL" if seat == 1 else "LT"
        prof = {1: "TeamPlan" if seat == 1 else "Legacy", 2: "Legacy" if seat == 1 else "TeamPlan"}
        emit(row(f"h2h-{tag}", m, a, b, s, team1_ai_profile=prof[1], team2_ai_profile=prof[2])
             for a, b in cells for s in ss)
    elif bank == "play":
        play, arm, m, size, ss = argv[1], argv[2], argv[3], int(argv[4]), seeds(argv[5])
        assert play in PLAYS and arm in ("NN", "PN", "NP", "PNx", "NPx")
        # Optional: only the rows whose running team (either team, for NN)
        # holds one of these classes.
        only = set(argv[6].split(",")) if len(argv) > 6 else None
        rows = []
        for a, b in play_cells(size):
            if only is not None:
                side = {"PN": [a], "PNx": [a], "NP": [b], "NPx": [b], "NN": [a, b]}[arm]
                if not any(only & set(t) for t in side):
                    continue
            for s in ss:
                extra = {"ai_profile": "TeamPlan"}
                runs = {"NN": (), "PN": (1,), "NP": (2,), "PNx": (1,), "NPx": (2,)}[arm]
                for team in (1, 2):
                    if team not in runs:
                        extra[f"team{team}_withheld_plays"] = [play]
                    elif arm.endswith("x"):
                        extra[f"team{team}_exceptions"] = {play: []}
                rows.append(row(f"play-{play}-{arm}", m, a, b, s, **extra))
        emit(rows)
    elif bank == "pillar-h2h":
        arm, m, ss = argv[1], argv[2], seeds(argv[3])
        assert arm in ("on", "off")
        rows = []
        for a, b in sample3():
            for seat in (1, 2):
                for s in ss:
                    extra = {f"team{seat}_ai_profile": "TeamPlan",
                             f"team{3 - seat}_ai_profile": "Legacy"}
                    if arm == "off":
                        extra[f"team{seat}_withheld_plays"] = ["Pillar"]
                    rows.append(row(f"pillar-h2h-{arm}-s{seat}", m, a, b, s, **extra))
        emit(rows)
    elif bank == "focus-off":
        m, size, ss = argv[1], int(argv[2]), seeds(argv[3])
        cells = full_cells(2) if size == 2 else sample3()
        emit(row("focus-off", m, a, b, s, ai_profile="TeamPlan",
                 team1_target_focus=False, team2_target_focus=False)
             for a, b in cells for s in ss)
    elif bank == "peel-cand":
        # A candidate Peel gate: the play bank's PN / NP rows whose running
        # team is one of <teams>, with the shipped gates plus <gate>
        # ("Peeler/Ally", or "-" for the shipped table alone, or "NN" for
        # the play withheld from both teams).
        gate, ss, want = argv[1], seeds(argv[2]), argv[3].split(",")
        want = {frozenset(t.split("+")) for t in want}
        rows = []
        for a, b in play_cells(2):
            for s in ss:
                for arm, team in (("PN", a), ("NP", b)):
                    if frozenset(team) not in want:
                        continue
                    for m in ("BasicArena", "PillaredArena", "TwinPillars"):
                        extra = {"ai_profile": "TeamPlan"}
                        runs = 1 if arm == "PN" else 2
                        if gate == "NN":
                            extra["team1_withheld_plays"] = ["Peel"]
                            extra["team2_withheld_plays"] = ["Peel"]
                        else:
                            extra[f"team{3 - runs}_withheld_plays"] = ["Peel"]
                            if gate != "-":
                                extra[f"team{runs}_exceptions"] = {"Peel": PEEL_SHIPPED + [gate]}
                        tag = "NN" if gate == "NN" else arm
                        rows.append(row(f"peelcand-{gate}-{tag}", m, a, b, s, **extra))
        emit(rows)
    elif bank in ("hunt-bank", "sight-bank"):
        # AS-385's package cells (AS-394's Hunt bank): every 2v2 team with a
        # healer against every 2v2 team (no double healers), plus the six
        # one-healer 3v3 teams in both orders, on Nagrand and TwinPillars,
        # both teams TeamPlan. For the observed harnesses
        # (`tests/hunt_endgame_sweep.rs`, `tests/team_sight_cohesion.rs`).
        arm, ss = argv[1], seeds(argv[2])
        assert arm in ("shipped", "lifted")
        dps = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
        heal = ["Priest", "Paladin", "Shaman", "Druid"]
        threes = [["Warrior", "Mage", "Priest"], ["Rogue", "Warlock", "Druid"],
                  ["Warrior", "Hunter", "Paladin"], ["Rogue", "Mage", "Shaman"],
                  ["Warlock", "Hunter", "Priest"], ["Warrior", "Rogue", "Druid"]]
        ht = [[d, h] for d in dps for h in heal]
        at = ht + [list(p) for p in itertools.combinations(dps, 2)]
        cells = [(a, b) for a in ht for b in at] + list(itertools.permutations(threes, 2))
        rows = []
        for m in ("PillaredArena", "TwinPillars"):
            for a, b in cells:
                for s in ss:
                    extra = {"ai_profile": "TeamPlan"}
                    if arm == "lifted":
                        extra["team1_exceptions"] = {"Hunt": []}
                        extra["team2_exceptions"] = {"Hunt": []}
                    # One label for both arms: the scorers pair by label.
                    rows.append(row(bank, m, a, b, s, **extra))
        emit(rows)
    else:
        sys.exit(f"unknown bank {bank}")


if __name__ == "__main__":
    main(sys.argv[1:])
