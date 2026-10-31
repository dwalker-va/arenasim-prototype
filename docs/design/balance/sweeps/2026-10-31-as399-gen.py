#!/usr/bin/env python3
"""AS-399 banks: the AS-389 target focus's balance shift, re-measured on main
after E0, and its tuning options (DIRECTIONAL, replicated).

    2026-10-31-as399-gen.py <bank> <args...> > bank.jsonl

Labels are the 0.8 sweep's `bank|map|team1|team2|seed`
(`2026-10-25-0.8-gen.py`), so `2026-10-25-0.8-score.py` reads them; the
option arms run the SAME files on a patched binary, and the scorer
(`2026-10-31-as399-score.py`) tells the arms apart by the file each row came
from, never by its label.

Seeds: the 0.8 sweep's and E0's blocks, so every row has a twin in their
files at the same cell and seed.

    2v2   block 1: 801-804   block 2: 811-814   block 3: 821-824
    3v3   block 1: 801       block 2: 811       block 3: 821

The option arms play the first two seeds of each 2v2 block (801-802,
811-812, 821-822).

Banks
-----
uni <map> <on|off> <seeds>
    The full 36 x 36 2v2 matrix, both teams TeamPlan; `on` is the shipped
    focus (label `uni-TT`), `off` has it off for both teams
    (`team{1,2}_target_focus: false`, label `focus-off`). The focus's
    uniform (balance) effect is `uni-TT` against `focus-off`.
uni3 <map> <on|off> <seeds>
    As `uni`, on the 0.8 sweep's 600-cell 3v3 `sample3`.
fh <map> <seeds>
    Head-to-head on the 30 x 30 2v2 cells with at most one healer per team
    (the play banks' cells): one team runs the focus and the other does not,
    both seats (labels `fh-t1`, `fh-t2`). Paired with `focus-off`: the
    focus's value to the side that runs it.
opt <map> <seeds>
    The option arms' bank: the 30 x 30 cells, uniform focus on (`uni-TT`),
    focus off (`focus-off`, the control: an option touches nothing while the
    focus is off) and both head-to-head seats (`fh-t1`, `fh-t2`).

Every match is 300s-capped, default loadouts, both teams TeamPlan.
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("gen08", os.path.join(HERE, "2026-10-25-0.8-gen.py"))
gen08 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen08)

OFF = {"team1_target_focus": False, "team2_target_focus": False}


def uniform(m, arm, cells, ss):
    assert arm in ("on", "off")
    bank = "uni-TT" if arm == "on" else "focus-off"
    extra = {} if arm == "on" else OFF
    return [gen08.row(bank, m, a, b, s, ai_profile="TeamPlan", **extra)
            for a, b in cells for s in ss]


def head_to_head(m, cells, ss):
    rows = []
    for seat in (1, 2):
        for a, b in cells:
            for s in ss:
                rows.append(gen08.row(f"fh-t{seat}", m, a, b, s, ai_profile="TeamPlan",
                                      **{f"team{3 - seat}_target_focus": False}))
    return rows


def main(argv):
    bank = argv[0]
    m = argv[1]
    assert m in gen08.MAPS
    if bank == "uni":
        gen08.emit(uniform(m, argv[2], gen08.full_cells(2), gen08.seeds(argv[3])))
    elif bank == "uni3":
        gen08.emit(uniform(m, argv[2], gen08.sample3(), gen08.seeds(argv[3])))
    elif bank == "fh":
        gen08.emit(head_to_head(m, gen08.play_cells(2), gen08.seeds(argv[2])))
    elif bank == "opt":
        cells, ss = gen08.play_cells(2), gen08.seeds(argv[2])
        gen08.emit(uniform(m, "on", cells, ss) + uniform(m, "off", cells, ss)
                   + head_to_head(m, cells, ss))
    else:
        sys.exit(f"unknown bank {bank}")


if __name__ == "__main__":
    main(sys.argv[1:])
