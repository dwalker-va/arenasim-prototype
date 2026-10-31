#!/usr/bin/env python3
"""AS-399 scoring: the target focus's balance shift and its tuning options.

    2026-10-31-as399-score.py focus <csv[.gz]>...
    2026-10-31-as399-score.py options <arm>=<csv[.gz]> ...

`focus` is the 0.8 scorer's `focus` command (`2026-10-25-0.8-score.py`):
every `uni-TT` side against its `focus-off` twin, both teams, with the
head-to-head (`fh-t1` seat 1, `fh-t2` seat 2, against `focus-off`) added.

`options` reads one CSV per arm (`main`, and each option played by the
patched binary with `AS399_OPT=<option>` on the SAME bank,
`2026-10-31-as399-gen.py opt`). The arm is the file's, never the label's.
Per arm it prints:

- the CONTROL: its `focus-off` rows against `main`'s (an option must touch
  nothing while the focus is off — every row identical);
- the UNIFORM shift: `uni-TT` against `focus-off`, per class on the pillar
  maps (competitive), and the double-ranged no-healer teams;
- the HEAD-TO-HEAD value: the side running the focus against the same side
  with it off;
- the option against the shipped focus, paired (`uni-TT` arm against `uni-TT`
  main): what the option itself moves in a uniform world.

Every effect carries its three seed blocks (801-80x / 811-81x / 821-82x) and
the pooled McNemar z; **R** marks one whose blocks agree in sign and whose
|z| >= 1.96 (the 0.8 scorer's rule).
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("score08", os.path.join(HERE, "2026-10-25-0.8-score.py"))
sc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sc)

CLASSES = sc.CLASSES
CANARIES = ["Mage+Hunter", "Mage+Warlock", "Warlock+Hunter"]


def h2h_sides(data):
    yield from sc.pair_sides(data, "fh-t1", "focus-off", seat_of=1)
    yield from sc.pair_sides(data, "fh-t2", "focus-off", seat_of=2)


def order_uniform():
    o = ["2v2", "2v2 competitive", "3v3"]
    o += [f"2v2 competitive map={m}" for m in ("Nagrand", "Twin")]
    o += [f"2v2 pillars comp has {c}" for c in CLASSES]
    o += [f"3v3 pillars comp has {c}" for c in CLASSES]
    o += ["2v2 no healer"] + [f"team {t}" for t in CANARIES]
    return o


KINDS = ["H+M", "H+R", "M+R", "R+R", "M+M"]


def kind(team):
    """A 2v2 team's shape: H a healer, M a melee DPS, R a ranged DPS."""
    cls = team.split("+")
    h = sum(c in sc.HEALERS for c in cls)
    m = sum(c in sc.MELEE for c in cls)
    if h >= 2:
        return "H+H"
    if h == 1:
        return "H+M" if m else "H+R"
    return {2: "M+M", 1: "M+R", 0: "R+R"}[m]


def kinds(acc, it):
    """Accumulate 2v2 sides by (side shape, opponent shape)."""
    for m, size, seat, team, opp, s, bw, aw in it:
        if size != 2 or kind(team) == "H+H" or kind(opp) == "H+H":
            continue
        for k in (f"{kind(team)} vs {kind(opp)}", f"{kind(team)} vs any"):
            acc.setdefault(k, sc.Paired()).add(bw, aw, sc.block(s, size))


def order_kinds():
    return [f"{a} vs {b}" for a in KINDS for b in KINDS + ["any"]]


def spread(acc, prefix="2v2 pillars comp has"):
    """How far the class standings sit from 50%: the mean |level - 50| over
    the classes, focus off (base) and on (arm), with and without the Paladin
    (the strongest class whatever the focus does)."""
    rows = []
    for which in ("base", "arm"):
        lv = {c: 100.0 * (acc[f"{prefix} {c}"].b if which == "base" else acc[f"{prefix} {c}"].a)
              / acc[f"{prefix} {c}"].n for c in CLASSES if f"{prefix} {c}" in acc}
        allc = sum(abs(v - 50) for v in lv.values()) / len(lv)
        nop = [abs(v - 50) for c, v in lv.items() if c != "Paladin"]
        rows.append(f"{which}: mean |level - 50| {allc:.2f} (without the Paladin {sum(nop) / len(nop):.2f}); "
                    + ", ".join(f"{c} {v:.1f}" for c, v in lv.items()))
    print("\n".join(rows))


def cmd_focus(paths):
    data = sc.load(paths)
    acc = {}
    sc.accumulate(acc, sc.pair_sides(data, "uni-TT", "focus-off"))
    print("## Uniform: both teams run the focus, against neither")
    sc.report(acc, order_uniform())
    print("\n### standings spread, 2v2 pillar maps, competitive")
    spread(acc)
    print("\n### standings spread, 3v3 pillar maps, competitive")
    spread(acc, "3v3 pillars comp has")
    k = {}
    kinds(k, sc.pair_sides(data, "uni-TT", "focus-off"))
    print("\n## Uniform, 2v2 pillar maps, by team shape (side vs opponent; double healers left out)")
    sc.report(k, order_kinds())
    h = {}
    if sc.accumulate(h, h2h_sides(data)):
        print("\n## Head-to-head: the side that runs the focus, against the same side without it")
        sc.report(h, ["all", "2v2 competitive"] + [f"map={m}" for m in ("Nagrand", "Twin")]
                  + [f"has {c}" for c in CLASSES] + ["no healer"])


def tagged(data, tag):
    return {(tag + "/" + k[0],) + k[1:]: v for k, v in data.items()}


def cmd_options(specs):
    arms = {}
    for spec_ in specs:
        name, path = spec_.split("=", 1)
        arms.setdefault(name, []).append(path)
    data = {name: sc.load(paths) for name, paths in arms.items()}
    base = data.get("main")
    if base is not None and len(data) > 1:
        # `main` is scored on the option bank's rows only, so every arm's
        # figures are over the same cells and seeds.
        keys = set().union(*(set(d) for n, d in data.items() if n != "main"))
        data["main"] = {k: v for k, v in base.items() if k in keys}
    for name, d in data.items():
        print(f"\n# arm `{name}` ({len(d)} matches)")
        if base is not None and name != "main":
            off = [k for k in d if k[0] == "focus-off"]
            same = sum(1 for k in off if base.get(k) == d[k])
            print(f"control: focus-off rows identical to main: {same} / {len(off)}"
                  f" (missing in main: {sum(1 for k in off if k not in base)})")
        acc = {}
        sc.accumulate(acc, sc.pair_sides(d, "uni-TT", "focus-off"))
        print("\n## Uniform shift (uni-TT against focus-off)")
        sc.report(acc, order_uniform())
        print("\n### standings spread (base: focus off; arm: this arm's focus)")
        spread(acc)
        k = {}
        kinds(k, sc.pair_sides(d, "uni-TT", "focus-off"))
        print("\n### by team shape")
        sc.report(k, [x for x in order_kinds() if x.endswith("any") or x.startswith("H+M")])
        h = {}
        sc.accumulate(h, h2h_sides(d))
        print("\n## Head-to-head value (the side running the focus, against it off)")
        sc.report(h, ["all", "2v2 competitive"] + [f"map={m}" for m in ("Nagrand", "Twin")]
                  + [f"has {c}" for c in CLASSES] + ["no healer"])
        if base is not None and name != "main":
            merged = {**tagged(base, "main"), **tagged(d, "arm")}
            p = {}
            sc.accumulate(p, sc.pair_sides(merged, "arm/uni-TT", "main/uni-TT"))
            print(f"\n## `{name}` against the shipped focus, uniform (arm uni-TT against main uni-TT)")
            sc.report(p, order_uniform())


def main(argv):
    if not argv:
        sys.exit(__doc__)
    if argv[0] == "focus":
        cmd_focus(argv[1:])
    elif argv[0] == "options":
        cmd_options(argv[1:])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
