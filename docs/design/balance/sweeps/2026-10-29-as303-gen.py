#!/usr/bin/env python3
"""AS-303 banks: the E0 slot-multiplier re-baseline (AUTHORITY).

    2026-10-29-as303-gen.py <bank> <args...> > bank.jsonl

The 0.8 milestone sweep's banks (`2026-10-25-0.8-gen.py`), at the 0.8
sweep's own seeds, so every row has a twin in the 0.8 files at the same cell
and seed, and every label parses with `2026-10-25-0.8-score.py`:

    2v2   block 1: 801-804   block 2: 811-814   block 3: 821-824
    3v3   block 1: 801       block 2: 811       block 3: 821

Banks
-----
uni <map> <profile> 2 <seeds>
    As the 0.8 `uni` bank: the full 36 x 36 2v2 matrix, both teams on
    <profile>. Labels `uni-LL` / `uni-TT`.
uni3 <map> <profile> <seeds>
    The 3v3 standings bank: both teams on <profile>, on the 0.8 sweep's
    600-cell `sample3` (every team fielding one or two healers), not the full
    84 x 84 matrix. Labels `uni-LL` / `uni-TT`.
h2h <map> <seat> <size> <seeds> [sample]
    As the 0.8 `h2h` bank: team <seat> on TeamPlan, the other on Legacy.
    Paired with `uni ... Legacy` at the same seeds.

Both arms (main, and main with E0) run the same files. Every match is
300s-capped, default loadouts.
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("gen08", os.path.join(HERE, "2026-10-25-0.8-gen.py"))
gen08 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen08)


def main(argv):
    if argv[0] == "uni3":
        m, prof, ss = argv[1], argv[2], gen08.seeds(argv[3])
        assert m in gen08.MAPS and prof in ("Legacy", "TeamPlan")
        tag = "LL" if prof == "Legacy" else "TT"
        gen08.emit(gen08.row(f"uni-{tag}", m, a, b, s, ai_profile=prof)
                   for a, b in gen08.sample3() for s in ss)
    elif argv[0] in ("uni", "h2h"):
        gen08.main(argv)
    else:
        sys.exit(f"unknown bank {argv[0]}")


if __name__ == "__main__":
    main(sys.argv[1:])
