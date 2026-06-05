#!/usr/bin/env python3
"""Generate batch JSONL for arena balance sweeps (consumed by `arenasim --batch`).

Emits one HeadlessMatchConfig per line: a `team1` template played against an
enumerated set of opposing teams, N seeds each. Output is fed to:

    arenasim --batch sweep.jsonl --out results.csv --jobs 16

Examples
--------
# 1v1: Hunter vs every class, N=100, 300s cap
gen_sweep.py --t1 Hunter --t2-size 1 --n 100 > /tmp/sweep.jsonl

# Full 7x7 1v1 matrix
gen_sweep.py --t1 '{p}' --t2-size 1 --n 100 > /tmp/matrix.jsonl

# 2v2: Hunter + every partner vs every distinct opposing pair (no double-healer)
gen_sweep.py --t1 'Hunter+{p}' --t2-size 2 --n 20 --exclude-double-healer > /tmp/h2v2.jsonl

# 3v3: a fixed comp vs every distinct opposing triple (no all-healer)
gen_sweep.py --t1 'Hunter+Priest+Warrior' --t2-size 3 --n 20 > /tmp/h3v3.jsonl

# Strategy-var sweep: run the generator once per variant with --extra and
# --label-suffix, then concatenate. --extra is merged into every config and the
# suffix keeps the variants distinct when aggregating.
for pet in Spider Boar Bird; do
  gen_sweep.py --t1 Hunter --t2-size 1 --n 100 \
    --extra "{\"team1_hunter_pet_types\":[\"$pet\"]}" --label-suffix "$pet"
done > /tmp/pet_sweep.jsonl

Notes
-----
- `{p}` in --t1 is a wildcard that expands over all 7 classes (skipping any
  expansion that would duplicate a class already in the template).
- Opposing teams are distinct-class unordered combinations of --t2-size.
- The cap defaults to 300s: healer attrition resolves around ~200-240s, so a
  shorter cap silently turns healer wins into draws. Do not lower it without a
  reason.
- `--extra` is shallow-merged into each config (JSON object). Any field of
  HeadlessMatchConfig works: team1_hunter_pet_types, team1_rogue_openers,
  team1_warrior_shouts, team1_mage_armors, team1_paladin_auras, equipment, etc.
"""
import argparse
import itertools
import json
import sys

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter"]
HEALERS = {"Priest", "Paladin"}


def expand_t1(template):
    """Yield concrete team1 lists from a template that may contain one '{p}'."""
    slots = template.split("+")
    if "{p}" not in slots:
        yield slots
        return
    idx = slots.index("{p}")
    fixed = [s for i, s in enumerate(slots) if i != idx]
    for c in CLASSES:
        if c in fixed:
            continue  # no duplicate class on one team
        team = list(slots)
        team[idx] = c
        yield team


def enumerate_opponents(size, exclude_double_healer, exclude_all_healer):
    for combo in itertools.combinations(CLASSES, size):
        healers = sum(1 for c in combo if c in HEALERS)
        if exclude_double_healer and healers >= 2:
            continue
        if exclude_all_healer and size >= 1 and healers == size and size > 1:
            continue
        yield list(combo)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--t1", default=None,
                    help="team1 template, e.g. 'Hunter', 'Hunter+{p}', 'Hunter+Priest+Warrior'")
    ap.add_argument("--full", type=int, default=None, metavar="SIZE",
                    help="complete SIZE-v-SIZE matrix: every distinct-class team of SIZE "
                         "vs every other (both orderings). Ignores --t1/--t2-size.")
    ap.add_argument("--t2-size", type=int, default=None,
                    help="opposing team size (default: same as t1)")
    ap.add_argument("--n", type=int, default=100, help="seeds per matchup (default 100)")
    ap.add_argument("--cap", type=float, default=300.0,
                    help="max_duration_secs (default 300; do not lower without reason)")
    ap.add_argument("--seed-base", type=int, default=0)
    ap.add_argument("--exclude-double-healer", action="store_true",
                    help="drop opposing teams with 2+ healers (Priest/Paladin)")
    ap.add_argument("--include-all-healer", action="store_true",
                    help="keep all-healer opposing teams (excluded by default for size>1)")
    ap.add_argument("--extra", default=None,
                    help="JSON object shallow-merged into every config (strategy vars)")
    ap.add_argument("--label-suffix", default=None,
                    help="appended to each label to keep strategy-var variants distinct")
    args = ap.parse_args()

    extra = json.loads(args.extra) if args.extra else {}

    # team1 set: --full enumerates every distinct-class team of SIZE; otherwise
    # expand the --t1 template.
    if args.full is not None:
        # Full matrix keeps every distinct-class combo on team1 (no all-healer
        # auto-exclusion); --exclude-double-healer still applies if requested.
        team1_set = list(enumerate_opponents(args.full, args.exclude_double_healer, False))
    else:
        if not args.t1:
            ap.error("provide --t1 or --full")
        team1_set = list(expand_t1(args.t1))

    out = sys.stdout
    count = 0
    for team1 in team1_set:
        if args.full is not None:
            t2_size = args.full
            opp_iter = enumerate_opponents(t2_size, args.exclude_double_healer, False)
        else:
            t2_size = args.t2_size if args.t2_size is not None else len(team1)
            opp_iter = enumerate_opponents(t2_size, args.exclude_double_healer,
                                           not args.include_all_healer)
        for opp in opp_iter:
            label = "+".join(team1) + "_vs_" + "+".join(opp)
            if args.label_suffix:
                label += "#" + args.label_suffix
            for s in range(args.n):
                cfg = {
                    "team1": team1,
                    "team2": opp,
                    "random_seed": args.seed_base + s,
                    "max_duration_secs": args.cap,
                    "label": label,
                }
                cfg.update(extra)
                out.write(json.dumps(cfg) + "\n")
                count += 1
    print(f"# wrote {count} match configs", file=sys.stderr)


if __name__ == "__main__":
    main()
