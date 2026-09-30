#!/usr/bin/env python3
"""AS-166 directional sweep input: AS-125's directional JSONLs, cut to the
cells Flare can reach plus controls.

usage: gensweep.py <as125 jsonl> [--every N] > out.jsonl

Keeps, from a `gen_sweep.py --affects Hunter` file:
- every cell with a Hunter on one side and a Rogue on the other (mirrors
  included when a Rogue faces a Hunter) — the cells Flare can reach;
- every Nth (default 7) Hunter cell with no Rogue against it, in label order —
  the "Flare is never wasted" control: nothing is hidden there, so these must
  be byte-identical and cast no Flare;
- every cell with no Hunter at all — the sweep's own control.
All seeds of each kept cell; the kill-target fields are carried as they are.
"""
import json, sys


def kind(c):
    t1, t2 = c["team1"], c["team2"]
    rogue_vs_hunter = ("Hunter" in t1 and "Rogue" in t2) or ("Hunter" in t2 and "Rogue" in t1)
    if rogue_vs_hunter:
        return "reach"
    if "Hunter" in t1 or "Hunter" in t2:
        return "hunter_no_rogue"
    return "control"


def main(argv):
    every = 7
    if "--every" in argv:
        i = argv.index("--every")
        every = int(argv[i + 1])
        argv = argv[:i] + argv[i + 2:]
    cfgs = [json.loads(l) for l in open(argv[0]) if l.strip()]
    no_rogue = sorted({c["label"] for c in cfgs if kind(c) == "hunter_no_rogue"})
    keep_no_rogue = set(no_rogue[::every])
    for c in cfgs:
        k = kind(c)
        if k in ("reach", "control") or c["label"] in keep_no_rogue:
            print(json.dumps(c))


if __name__ == "__main__":
    main(sys.argv[1:])
