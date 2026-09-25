#!/usr/bin/env python3
"""Generate batch JSONL for arena balance sweeps (consumed by `arenasim --batch`).

Emits one HeadlessMatchConfig per line: a `team1` template played against an
enumerated set of opposing teams, N seeds each. Output is fed to:

    arenasim --batch sweep.jsonl --out results.csv --jobs 6

`--jobs` keys off how many agents are sweeping right now, not off the core
count; 6 on a quiet box. See "Sizing `--jobs`" in
docs/design/balance/sweep-tiers.md — 16 is too many.

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

# DIRECTIONAL tier: every cell a Shaman change can reach, plus 8 control cells
# it cannot. Cuts the cells, never the seeds -- see the note below.
gen_sweep.py --full 2 --exclude-double-healer --affects Shaman --n 10 \
  > /tmp/shaman_directional.jsonl    # 2,330 matches, not 62,500

# Strategy-var sweep: run the generator once per variant with --extra and
# --label-suffix, then concatenate. --extra is merged into every config and the
# suffix keeps the variants distinct when aggregating.
for pet in Spider Boar Bird; do
  gen_sweep.py --t1 Hunter --t2-size 1 --n 100 \
    --extra "{\"team1_hunter_pet_types\":[\"$pet\"]}" --label-suffix "$pet"
done > /tmp/pet_sweep.jsonl

Notes
-----
- `{p}` in --t1 is a wildcard that expands over all 8 classes (skipping any
  expansion that would duplicate a class already in the template).
- Opposing teams are distinct-class unordered combinations of --t2-size.
- The cap defaults to 300s: healer attrition resolves around ~200-240s, so a
  shorter cap silently turns healer wins into draws. Do not lower it without a
  reason.
- `--extra` is shallow-merged into each config (JSON object). Any field of
  HeadlessMatchConfig works: team1_hunter_pet_types, team1_rogue_openers,
  team1_warrior_shouts, team1_mage_armors, team1_paladin_auras, equipment, etc.
- `--affects` is the DIRECTIONAL tier's one lever: keep the cells the change
  can reach and a sample of the ones it cannot, at the same seeds. It cuts
  CELLS and never seeds, because in a paired design the significance comes
  from the flip count across the whole run rather than from per-cell
  precision. See `docs/design/balance/sweep-tiers.md`.
- The control fields every class the change cannot reach on BOTH sides, by
  construction: a leak into a class it never fields would pass it clean.
  `--control-cells` below the fewest cells that can do that is refused.
"""
import argparse
import hashlib
import itertools
import json
import sys

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman"]
HEALERS = {"Priest", "Paladin", "Shaman"}


# Fields the generator owns. `--extra` overwriting one of these would leave the
# label naming a team the config no longer contains, and the aggregation
# downstream groups by label.
GENERATED_FIELDS = ("team1", "team2", "random_seed", "label")


def expand_t1(template):
    """Yield concrete team1 lists from a template containing at most one '{p}'.

    A class name that is neither a real class nor the wildcard is an error:
    silently generating a sweep for a class the simulator will reject is a
    whole arm of a measurement lost with nothing said.
    """
    slots = template.split("+")
    unknown = [s for s in slots if s != "{p}" and s not in CLASSES]
    if unknown:
        raise ValueError(
            "unknown class(es) %s in --t1 %r; known: %s"
            % (", ".join(unknown), template, ", ".join(CLASSES))
        )
    if slots.count("{p}") > 1:
        raise ValueError(
            "--t1 %r has %d '{p}' wildcards; only one can be expanded, and the "
            "rest would stay in the team as a literal class name"
            % (template, slots.count("{p}"))
        )
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


def reaches(team1, team2, affected):
    """True when the change can touch this cell at all."""
    return bool((set(team1) | set(team2)) & affected)


def _side_masks(cells, side):
    """Each cell's classes on one side as a bitmask, plus the side's universe.

    The universe is every class that side fields anywhere in `cells` -- derived
    from the cells, never from the roster, so a template that pins team1 to
    one class owes that one class and no other.
    """
    universe = sorted(set(c for cell in cells for c in cell[side]))
    bit = dict((c, 1 << i) for i, c in enumerate(universe))
    masks = []
    for cell in cells:
        m = 0
        for c in cell[side]:
            m |= bit[c]
        masks.append(m)
    return masks, (1 << len(universe)) - 1


def _cover_table(masks, full):
    """`table[u]` = fewest of these teams that together field every class in `u`.

    Exact, by dynamic programming over subsets of the side's universe (at most
    eight classes, so 256 states). Whatever team covers the lowest missing
    class is in some optimal cover, so only those teams need trying.
    """
    teams = sorted(set(masks))
    table = [0] * (full + 1)
    for u in range(1, full + 1):
        low = u & -u
        table[u] = 1 + min(table[u & ~t] for t in teams if t & low)
    return table


def control_floor(cells):
    """The fewest control cells that field every class on BOTH sides.

    Returns `(floor, team1_floor, opponent_floor)`. Each side's floor is an
    exact minimum set cover of its universe by the teams it fields, and the
    cells form a full product of the two sides' teams (see `sample_spread`),
    so any team1 cover pairs with any opponent cover: the joint floor is the
    larger of the two. At the default roster it is 4 for 2v2, 7 for 1v1 and 3
    for 3v3 with one class affected -- but those are this roster's values, not
    constants; the function is what answers.
    """
    if not cells:
        return (0, 0, 0)
    floors = []
    for side in (0, 1):
        masks, full = _side_masks(cells, side)
        floors.append(_cover_table(masks, full)[full])
    return (max(floors), floors[0], floors[1])


def sample_spread(cells, keep):
    """`keep` indices into `cells`: a control that fields every class on both sides.

    The control's duty follows from how `paired_sweep.py` reads it: the cells
    the change cannot reach, held to bit-exactness. A class the control never
    fields on a side is a class a leak into that side would be invisible in --
    and the control would still PASS, quietly, and be cited as evidence the
    change stayed home. So coverage is a constraint here, not a hoped-for
    property of the sample. A uniform 8-cell sample of the 2v2 matrix misses
    some class on some side about 70% of the time, with no preference for
    either side; the digest order alone got 22 of 24 (class, count) cases
    right by luck, and missed Warlock's at the default count (AS-143).

    ORDER. Cells are ranked by a digest of the cell, not taken from the head
    of the enumeration (all one corner of the space) and not at a constant
    stride: the enumeration is a nested product, so a stride aliases against
    its inner dimension. Measured on the 400 Shaman-free cells of the 2v2
    matrix, a stride of 50 returned eight controls sharing just TWO distinct
    opponents. The digest is `blake2b`, not `hash()`, which is salted per
    process: the two arms of a paired run may generate the sweep separately,
    and a control sampled differently in each would not be a control at all.

    SELECTION. Each pick is the earliest cell in that order that still leaves
    enough picks to finish covering both sides. "Enough" is exact (the cover
    floor of what is still unfielded), so the walk always finishes covered
    when `keep` is at least the floor: a cell pairing the next team of an
    optimal team1 cover with the next of an optimal opponent cover always
    qualifies. When the plain digest prefix already covers, no cell is ever
    passed over and the result is exactly that prefix -- which is why every
    `--affects` invocation whose control already covered emits the same
    sweep it always did.

    Requires `keep` >= `control_floor(cells)[0]` (or `keep` <= 0, which is no
    control at all): a smaller control cannot discharge the duty, and raises
    rather than quietly under-covering. Also requires `cells` to be the full
    product of its two sides' teams, which the unreachable set always is --
    reachability is decided per side -- and on which the floor's arithmetic
    rests.
    """
    if keep <= 0:
        return []
    if keep >= len(cells):
        return list(range(len(cells)))
    teams1 = set(tuple(c[0]) for c in cells)
    teams2 = set(tuple(c[1]) for c in cells)
    if len(set((tuple(c[0]), tuple(c[1])) for c in cells)) != len(teams1) * len(teams2):
        raise ValueError(
            "control cells are not the full product of their two sides' teams, "
            "so the coverage floor does not apply to them")
    floor = control_floor(cells)[0]
    if keep < floor:
        raise ValueError(
            "%d control cells cannot field every class on both sides; the "
            "fewest that can is %d" % (keep, floor))

    order = digest_order(cells)
    m1, full1 = _side_masks(cells, 0)
    m2, full2 = _side_masks(cells, 1)
    t1, t2 = _cover_table(m1, full1), _cover_table(m2, full2)

    picked, taken = [], set()
    left1, left2 = full1, full2           # classes not yet fielded, per side
    while len(picked) < keep and (left1 or left2):
        budget = keep - len(picked) - 1   # picks left AFTER this one
        for i in order:
            if i in taken:
                continue
            n1, n2 = left1 & ~m1[i], left2 & ~m2[i]
            if max(t1[n1], t2[n2]) <= budget:
                picked.append(i)
                taken.add(i)
                left1, left2 = n1, n2
                break
        else:  # unreachable while keep >= floor; see the docstring
            raise AssertionError("coverage walk found no feasible cell")
    # Covered: every remaining cell is feasible, so the rest is plain order.
    picked.extend([i for i in order if i not in taken][:keep - len(picked)])
    return sorted(picked)


def digest_order(cells):
    """Indices into `cells`, ranked by a stable digest of each cell."""
    key = [hashlib.blake2b(repr(c).encode("utf-8"), digest_size=8).digest()
           for c in cells]
    return sorted(range(len(cells)), key=lambda i: key[i])


def select_cells(cells, affected, control_cells):
    """Split cells into reachable + a sampled control, for the directional tier.

    Returns the kept cells in enumeration order, plus the two counts, so the
    caller can say on stderr what it cut. A sweep that silently dropped most
    of its matrix is the kind of thing nobody notices until the aggregate
    disagrees with a previous run.

    Selection is by INDEX throughout: a cell is a pair of LISTS, which is not
    hashable, so there is no set of cells to intersect -- and an equality scan
    would be quadratic over the 625-cell matrix for no gain.
    """
    reachable = [i for i, c in enumerate(cells) if reaches(c[0], c[1], affected)]
    unreachable = [i for i, c in enumerate(cells) if not reaches(c[0], c[1], affected)]
    picked = sample_spread([cells[i] for i in unreachable], control_cells)
    keep = set(reachable) | set(unreachable[i] for i in picked)
    return ([c for i, c in enumerate(cells) if i in keep],
            len(reachable), len(keep) - len(reachable))


def main(argv=None):
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
    ap.add_argument("--affects", default=None, metavar="CLASS[,CLASS...]",
                    help="DIRECTIONAL tier: keep only the cells this change can "
                         "reach, plus --control-cells cells it cannot. Cuts "
                         "cells, never seeds.")
    ap.add_argument("--control-cells", type=int, default=8, metavar="N",
                    help="with --affects, how many unreachable cells to keep "
                         "as the bit-exactness control (default 8). The control "
                         "always fields every unaffected class on both sides, "
                         "so a count below the fewest cells that can do that "
                         "is refused; 0 drops the control entirely, which is "
                         "warned about")
    args = ap.parse_args(argv)

    affected = set()
    if args.affects:
        affected = set(c.strip() for c in args.affects.split(",") if c.strip())
        unknown = sorted(affected - set(CLASSES))
        if unknown:
            sys.exit("--affects names unknown class(es) %s; known: %s"
                     % (", ".join(unknown), ", ".join(CLASSES)))
        if args.control_cells < 0:
            sys.exit("--control-cells cannot be negative")

    extra = {}
    if args.extra:
        try:
            extra = json.loads(args.extra)
        except json.JSONDecodeError as e:
            sys.exit(f"--extra is not valid JSON: {e}")
        if not isinstance(extra, dict):
            sys.exit("--extra must be a JSON object, got %s" % type(extra).__name__)
        clobbered = [f for f in GENERATED_FIELDS if f in extra]
        if clobbered:
            sys.exit(
                "--extra may not set %s: the generator owns those, and the label "
                "the aggregation groups by would then name a different team"
                % ", ".join(clobbered)
            )

    # team1 set: --full enumerates every distinct-class team of SIZE; otherwise
    # expand the --t1 template.
    if args.full is not None:
        # Full matrix keeps every distinct-class combo on team1 (no all-healer
        # auto-exclusion); --exclude-double-healer still applies if requested.
        team1_set = list(enumerate_opponents(args.full, args.exclude_double_healer, False))
    else:
        if not args.t1:
            ap.error("provide --t1 or --full")
        try:
            team1_set = list(expand_t1(args.t1))
        except ValueError as e:
            sys.exit(str(e))

    # Materialise the cells before emitting, so --affects can cut them as a
    # set rather than mid-stream.
    cells = []
    for team1 in team1_set:
        if args.full is not None:
            t2_size = args.full
            opp_iter = enumerate_opponents(t2_size, args.exclude_double_healer, False)
        else:
            t2_size = args.t2_size if args.t2_size is not None else len(team1)
            opp_iter = enumerate_opponents(t2_size, args.exclude_double_healer,
                                           not args.include_all_healer)
        for opp in opp_iter:
            cells.append((team1, opp))

    if affected:
        total = len(cells)
        unreachable = [c for c in cells if not reaches(c[0], c[1], affected)]
        floor, floor1, floor2 = control_floor(unreachable)
        if 0 < args.control_cells < floor:
            sys.exit(
                "--control-cells %d cannot field every unaffected class on both "
                "sides; the fewest control cells that can is %d (team1 needs %d, "
                "the opponent side %d). A control that misses a class still "
                "passes, and a leak into that class passes with it. Ask for at "
                "least %d, or 0 to drop the control outright."
                % (args.control_cells, floor, floor1, floor2, floor))
        cells, reachable, control = select_cells(cells, affected, args.control_cells)
        print("# --affects %s: kept %d reachable + %d control of %d cells"
              % (",".join(sorted(affected)), reachable, control, total),
              file=sys.stderr)
        if reachable == 0:
            sys.exit(
                "--affects %s reaches none of the %d cells: the sweep would "
                "measure nothing. Check the class names against the team "
                "selection." % (",".join(sorted(affected)), total))
        if control == 0:
            print("# WARNING: no control cells. Nothing in this sweep bounds "
                  "the change's blast radius, and paired_sweep.py will say so.",
                  file=sys.stderr)

    out = sys.stdout
    count = 0
    for team1, opp in cells:
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
    if count == 0:
        # An empty sweep is silent all the way downstream: the batch runner
        # writes an empty CSV, and the first complaint anyone sees is an
        # aggregation error about a file that looks perfectly fine.
        sys.exit("no match configs generated (check --n and the team selection)")
    print(f"# wrote {count} match configs", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
