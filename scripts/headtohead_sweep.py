#!/usr/bin/env python3
"""Head-to-head sweep of an AI profile or a gear change, at real statistical power.

This is the measurement methodology from `docs/design/team-level-positioning-ai.md`
("How to measure a step") as a tool instead of folklore. It measures one of two
per-team variants against a baseline:

- an AI PROFILE (`--profile`, default TeamPlan, against Legacy), or
- an EQUIPMENT change (`--team1-equipment` / `--team2-equipment`, against the
  default loadouts or against `--base-team1-equipment` /
  `--base-team2-equipment`). The AI profile is then held fixed on both sides
  (`--ai-profile`), so the gear is the only variable.

The design is the same for both:

- HEAD-TO-HEAD, not uniform: each cell sets the two teams independently, so
  the variant plays the baseline on the same seed. A uniform A/B (both teams
  switched together) compares two internally consistent worlds and cannot
  answer "is the variant better".
- BOTH assignments are run (team 1 gets the variant, then team 2), because
  comps are rarely evenly matched and a single assignment confounds the
  variant with the comp. Each side's GAIN is measured against the uniform
  baseline cell.
- n=100 per cell by default. Win rate is one bit per match: at n=12 this
  codebase measured a +8pt effect that was really +36, and a -17pt one that
  was really +14. Draws are counted, never credited to a side.

The report has two parts, and only the second is the test:

- SINGLE-ARM figures: each cell's win rate with a Wilson 95% interval, and
  each side's gain with an unpaired two-proportion z. They treat the cells as
  independent samples, which they are not, and are printed for comparison
  only.
- PAIRED figures, over the seeds that completed all three cells: the
  variant's side-symmetrised win share against the baseline's at the same
  seats, the effect with a 95% interval from a CLUSTER BOOTSTRAP OVER SEEDS
  (a seed's two seats are not independent, so resampling matches would
  understate the interval), and the discordant seed counts with McNemar's
  test, per seat and over both.

Runs via the parallel batch runner (about 20 matches/sec per `--jobs` worker,
half that for PillaredArena or `TeamPlan`, so a 3-cell sweep at n=100 takes
seconds; see "What a sweep actually costs" in
docs/design/balance/sweep-tiers.md). Win rate is the CONFIRMATION metric;
for diagnosis prefer the per-frame mechanism metrics in `tests/camp_sweep.rs`
(occlusion-seconds, blocked share), which aggregate thousands of samples per
match instead of one bit.

Example:
    scripts/headtohead_sweep.py --team1 Warrior,Priest --team2 Warlock,Priest
    scripts/headtohead_sweep.py --team1 Hunter,Priest --team2 Rogue,Priest \\
        --map PillaredArena --seeds 100 --profile TeamPlan
    # Equipment: a JSON list, one object per team slot, socket -> item id (its
    # `items.ron` key), which is the headless config's `team1_equipment` shape.
    # `{}` leaves a slot on its default loadout.
    scripts/headtohead_sweep.py --team1 Rogue,Priest --team2 Rogue,Priest \\
        --team1-equipment '[{"Trinket2": "DragonspineTrophy"}]' \\
        --team2-equipment '[{"Trinket2": "DragonspineTrophy"}]'
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import random
import subprocess
import sys
import tempfile
from pathlib import Path

# Below this many paired seeds a percentile bootstrap cannot see the tails it
# is estimating, and its interval comes out too narrow. Printed, never hidden.
BOOTSTRAP_MIN_SEEDS = 20


def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    """Wilson score interval for k successes in n trials.

    With no trials the interval is the whole range: n=0 carries no information,
    and returning (0, 0) would make an empty cell read as a confident 0% win
    rate. Bounds are clamped to [0, 1] — the closed form lands a hair below
    zero at k=0, which would print as "-0.0%".
    """
    if n == 0:
        return (0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / d
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, centre - half), min(1.0, centre + half))


def ztest(k1: int, n1: int, k2: int, n2: int) -> float:
    """Two-proportion z, pooled.

    Each arm carries its own n, because a sweep's arms need not be the same
    size: an errored match is excluded from its cell, so the arm it was in has
    fewer usable matches than the nominal seed count. Assuming equal n there
    silently misreports both the effect and its significance. With n1 == n2
    this is the textbook equal-n form.
    """
    if n1 == 0 or n2 == 0:
        return 0.0
    p = (k1 + k2) / (n1 + n2)
    if p in (0.0, 1.0):
        return 0.0
    return (k2 / n2 - k1 / n1) / math.sqrt(p * (1 - p) * (1 / n1 + 1 / n2))


def mcnemar(for_variant: int, against: int) -> float:
    """Continuity-corrected McNemar z over the two discordant counts.

    The same statistic `paired_sweep.py` prints. Unsigned: the direction is in
    the two counts printed beside it. Zero discordant pairs is z=0 (nothing
    flipped, which is a result), and the correction is clamped at zero so a
    one-each-way split does not print as a statistic pointing somewhere.
    """
    d = for_variant + against
    if d == 0:
        return 0.0
    return max(0.0, abs(for_variant - against) - 1) / math.sqrt(d)


def bootstrap_mean_ci(
    columns: list[list[float]],
    resamples: int,
    rng: random.Random,
    level: float = 0.95,
) -> list[tuple[float, float]]:
    """Percentile bootstrap intervals for the means of `columns`, by cluster.

    Every column holds one value per CLUSTER (here: per seed), all in the same
    cluster order, and each resample draws clusters with replacement and takes
    every column's mean over the SAME draw. A cluster is therefore resampled
    whole: its two seats move together, which is what keeps the interval
    honest when they are correlated. Returns one (lo, hi) per column.
    """
    n = len(columns[0]) if columns else 0
    if n == 0:
        return [(0.0, 0.0) for _ in columns]
    idx = range(n)
    means: list[list[float]] = [[] for _ in columns]
    for _ in range(resamples):
        draw = rng.choices(idx, k=n)
        for col, out in zip(columns, means):
            out.append(sum(col[i] for i in draw) / n)
    tail = (1.0 - level) / 2.0
    result = []
    for out in means:
        out.sort()
        lo = out[int(math.floor(tail * (resamples - 1)))]
        hi = out[int(math.ceil((1.0 - tail) * (resamples - 1)))]
        result.append((lo, hi))
    return result


def parse_equipment(text: str | None, flag: str, team_size: int) -> list[dict[str, str]]:
    """Parse one `--*-equipment` value into the config's `teamN_equipment` shape.

    The shape is a JSON list with one object per team slot (in team order),
    each mapping a socket name (`Trinket2`) to an item id (its `items.ron`
    key). Only the shape is checked
    here; whether an item exists and the class may wear it is the headless
    config's own validation, which names the offending socket.
    """
    if text is None:
        return []
    try:
        value = json.loads(text)
    except ValueError as exc:
        sys.exit(f"{flag}: not valid JSON ({exc})")
    if not isinstance(value, list):
        sys.exit(f"{flag}: expected a JSON list with one object per team slot, "
                 f"e.g. '[{{\"Trinket2\": \"DragonspineTrophy\"}}]'")
    if len(value) > team_size:
        sys.exit(f"{flag}: {len(value)} slot entries but the team has only {team_size} members")
    for i, slot in enumerate(value):
        if not isinstance(slot, dict) or not all(
            isinstance(k, str) and isinstance(v, str) for k, v in slot.items()
        ):
            sys.exit(f"{flag}: slot {i} must be an object of socket -> item id strings")
    return value


def padded(equipment: list[dict[str, str]], team_size: int) -> list[dict[str, str]]:
    """`equipment` with an empty override for every slot it leaves unnamed."""
    return list(equipment) + [{}] * (team_size - len(equipment))


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--team1", required=True, help="comma-separated classes, e.g. Warrior,Priest")
    ap.add_argument("--team2", required=True)
    ap.add_argument("--map", default="PillaredArena")
    ap.add_argument("--seeds", type=int, default=100, help="seeds per cell (default 100)")
    ap.add_argument("--seed-base", type=int, default=1)
    ap.add_argument("--profile", help="AI profile under test vs Legacy (default TeamPlan); "
                    "not with an equipment sweep")
    gear = ap.add_argument_group(
        "equipment head-to-head",
        "Each value is a JSON list, one object per team slot, of socket -> item "
        "id: the headless config's teamN_equipment. Both --teamN-equipment are "
        "required, because both assignments must run.")
    gear.add_argument("--team1-equipment", metavar="JSON", help="team 1's gear under test")
    gear.add_argument("--team2-equipment", metavar="JSON", help="team 2's gear under test")
    gear.add_argument("--base-team1-equipment", metavar="JSON",
                      help="team 1's baseline gear (default: its default loadout)")
    gear.add_argument("--base-team2-equipment", metavar="JSON",
                      help="team 2's baseline gear (default: its default loadout)")
    gear.add_argument("--ai-profile", help="the AI profile BOTH sides play in an equipment "
                      "sweep (default: the runner's default)")
    ap.add_argument("--max-duration", type=float, default=300.0)
    ap.add_argument("--bootstrap", type=int, default=10000, metavar="N",
                    help="bootstrap resamples for the paired interval (default 10000)")
    ap.add_argument("--bootstrap-seed", type=int, default=0,
                    help="seed for the bootstrap's resampling, so a report reproduces")
    ap.add_argument("--keep", metavar="PREFIX", help="keep the JSONL/CSV at this path prefix")
    args = ap.parse_args(argv)

    t1, t2 = args.team1.split(","), args.team2.split(",")
    gear_flags = (args.team1_equipment, args.team2_equipment,
                  args.base_team1_equipment, args.base_team2_equipment)

    # Each cell is (label, team 1's config keys, team 2's config keys), with
    # the baseline cell first and then the variant on team 1, then on team 2.
    common: dict[str, object] = {}
    if any(f is not None for f in gear_flags):
        if args.profile is not None:
            sys.exit("--profile names an AI profile under test; an equipment sweep holds "
                     "the AI fixed on both sides, so choose it with --ai-profile")
        if args.team1_equipment is None or args.team2_equipment is None:
            sys.exit("an equipment sweep needs both --team1-equipment and --team2-equipment: "
                     "the variant must be measured from both seats")
        var1 = parse_equipment(args.team1_equipment, "--team1-equipment", len(t1))
        var2 = parse_equipment(args.team2_equipment, "--team2-equipment", len(t2))
        base1 = parse_equipment(args.base_team1_equipment, "--base-team1-equipment", len(t1))
        base2 = parse_equipment(args.base_team2_equipment, "--base-team2-equipment", len(t2))
        for team, var, base, size in ((1, var1, base1, len(t1)), (2, var2, base2, len(t2))):
            if padded(var, size) == padded(base, size):
                sys.exit(f"team {team}'s gear under test is its baseline gear, so its "
                         f"cell would measure nothing")
        if args.ai_profile is not None:
            common["ai_profile"] = args.ai_profile

        def eq(team: int, gear: list[dict[str, str]]) -> dict[str, object]:
            return {f"team{team}_equipment": gear} if gear else {}

        variant = "the gear"
        baseline = "baseline gear" if base1 or base2 else "default loadouts"
        what = (f"gear vs {baseline}, AI "
                f"{args.ai_profile if args.ai_profile else 'the runner default'}")
        cells = [
            ("BB", eq(1, base1), eq(2, base2)),
            ("GB", eq(1, var1), eq(2, base2)),
            ("BG", eq(1, base1), eq(2, var2)),
        ]
    else:
        if args.ai_profile is not None:
            sys.exit("--ai-profile holds the AI fixed for an equipment sweep; "
                     "to measure a profile, use --profile")
        profile = args.profile if args.profile is not None else "TeamPlan"
        variant = profile
        what = f"{profile} vs Legacy"
        cells = [
            ("LL", {"team1_ai_profile": "Legacy"}, {"team2_ai_profile": "Legacy"}),
            ("TL", {"team1_ai_profile": profile}, {"team2_ai_profile": "Legacy"}),
            ("LT", {"team1_ai_profile": "Legacy"}, {"team2_ai_profile": profile}),
        ]
    labels = [label for label, _, _ in cells]
    base_label, var1_label, var2_label = labels

    workdir = Path(tempfile.mkdtemp(prefix="headtohead_"))
    jsonl = Path(f"{args.keep}.jsonl") if args.keep else workdir / "sweep.jsonl"
    out = Path(f"{args.keep}.csv") if args.keep else workdir / "sweep.csv"

    seeds = list(range(args.seed_base, args.seed_base + args.seeds))
    with open(jsonl, "w") as f:
        for label, side1, side2 in cells:
            for seed in seeds:
                cfg: dict[str, object] = {
                    "label": label,
                    "team1": t1, "team2": t2,
                    "map": args.map,
                    "max_duration_secs": args.max_duration,
                    "random_seed": seed,
                }
                cfg.update(common)
                cfg.update(side1)
                cfg.update(side2)
                f.write(json.dumps(cfg) + "\n")

    total = len(cells) * args.seeds
    print(f"{total} matches: {args.team1} vs {args.team2} on {args.map}, "
          f"{what}, seeds {seeds[0]}..{seeds[-1]}",
          file=sys.stderr)
    subprocess.run(
        ["cargo", "run", "--release", "--quiet", "--",
         "--batch", str(jsonl), "--out", str(out)],
        check=True,
    )

    tally: dict[str, dict[str, int]] = {}
    # (label, seed) -> winner, for the paired analysis. Errored matches are
    # left out: a failed match is not a loss, and pairing against one would
    # compare a result to nothing.
    outcome: dict[tuple[str, str], str] = {}
    errors = 0
    with open(out) as f:
        for r in csv.DictReader(f):
            c = tally.setdefault(r["label"], {"t1": 0, "t2": 0, "draw": 0, "n": 0})
            w = r["winner"].strip()
            if w not in ("team1", "team2", "draw"):
                # A failed match is not a draw and must not inflate n — counting
                # it as one would silently bias every rate in the report.
                errors += 1
                continue
            c["n"] += 1
            c["t1" if w == "team1" else "t2" if w == "team2" else "draw"] += 1
            outcome[(r["label"], r["seed"].strip())] = w
    if errors:
        print(f"WARNING: {errors} match(es) errored and are excluded from all rates",
              file=sys.stderr)
    missing = [label for label in labels if tally.get(label, {}).get("n", 0) == 0]
    if missing:
        sys.exit(f"no successful matches in cell(s) {missing}; cannot analyze")

    # Every rate below divides by the cell's OWN usable n, never the nominal
    # seed count: errored matches are excluded from the cell, and dividing a
    # real win count by a nominal denominator shrinks the effect toward zero
    # without saying so anywhere in the output.
    print("single-arm win rates (comparison only: the paired figures below are the test)")
    print(f"{'cell':6} {'n':>4} {'T1':>4} {'T2':>4} {'draw':>5} {'T1%':>5} {'95% CI':>14}")
    for label in labels:
        c = tally[label]
        lo, hi = wilson(c["t1"], c["n"])
        print(f"{label:6} {c['n']:>4} {c['t1']:>4} {c['t2']:>4} {c['draw']:>5} "
              f"{100 * c['t1'] / c['n']:>4.0f}% {100 * lo:>6.1f}-{100 * hi:<5.1f}%")

    base, arm1, arm2 = tally[base_label], tally[var1_label], tally[var2_label]
    print(f"\n--- {variant}'s effect, single-arm (unpaired z; comparison only; "
          f"draws excluded from both sides' win counts) ---")
    for side, k, arm in (("team 1", "t1", arm1), ("team 2", "t2", arm2)):
        gain = 100 * (arm[k] / arm["n"] - base[k] / base["n"])
        print(f"{side} gets it: {base[k]:>3}/{base['n']:<3} -> {arm[k]:>3}/{arm['n']:<3} wins  "
              f"({gain:+.0f}pt, z={ztest(base[k], base['n'], arm[k], arm['n']):+.2f})")

    report_paired(outcome, seeds, labels, variant, args.bootstrap,
                  random.Random(args.bootstrap_seed))

    print("\n|z| >= 1.96 is the conventional 95% bar; near it, run more seeds "
          "(--seeds, --seed-base) rather than re-rolling.", file=sys.stderr)
    if not args.keep:
        print(f"(artifacts in {workdir}; pass --keep PREFIX to keep them elsewhere)",
              file=sys.stderr)
    return 0


def report_paired(
    outcome: dict[tuple[str, str], str],
    seeds: list[int],
    labels: list[str],
    variant: str,
    resamples: int,
    rng: random.Random,
) -> None:
    """Print the paired analysis: the test the single-arm figures are not.

    For each seed that completed all three cells, the variant's seat on team 1
    gains `d1` (1 if it won the variant cell, less 1 if it won the baseline
    cell) and its seat on team 2 gains `d2` likewise; the seed's effect is
    their mean. The side-symmetrised win share is the variant's wins over both
    seats against the baseline's wins in the same two seats, so a comp
    advantage cancels out of it.
    """
    base_label, var1_label, var2_label = labels
    d1: list[float] = []
    d2: list[float] = []
    var_wins = base_wins = 0
    flips = {1: [0, 0], 2: [0, 0]}  # seat -> [to a variant win, to a variant loss]
    seed_net = [0, 0, 0]  # seeds net for the variant, against it, even
    dropped = 0
    for seed in seeds:
        key = str(seed)
        b = outcome.get((base_label, key))
        v1 = outcome.get((var1_label, key))
        v2 = outcome.get((var2_label, key))
        if b is None or v1 is None or v2 is None:
            dropped += 1
            continue
        g1 = int(v1 == "team1") - int(b == "team1")
        g2 = int(v2 == "team2") - int(b == "team2")
        for seat, g in ((1, g1), (2, g2)):
            if g > 0:
                flips[seat][0] += 1
            elif g < 0:
                flips[seat][1] += 1
        net = g1 + g2
        seed_net[0 if net > 0 else 1 if net < 0 else 2] += 1
        var_wins += int(v1 == "team1") + int(v2 == "team2")
        base_wins += int(b == "team1") + int(b == "team2")
        d1.append(g1)
        d2.append(g2)

    n = len(d1)
    print(f"\n--- {variant}'s effect, PAIRED over the {n} seed(s) that completed all three "
          f"cells ---")
    if dropped:
        print(f"{dropped} seed(s) dropped: a match in one of its cells errored")
    if n == 0:
        print("no seed completed all three cells; nothing to pair")
        return
    d = [(a + b) / 2 for a, b in zip(d1, d2)]
    (lo1, hi1), (lo2, hi2), (lo, hi) = bootstrap_mean_ci([d1, d2, d], resamples, rng)

    def pt(x: float) -> str:
        return f"{100 * x:+.1f}pt"

    print(f"side-symmetrised win share: {variant} {var_wins}/{2 * n} = "
          f"{100 * var_wins / (2 * n):.1f}%, baseline at the same seats {base_wins}/{2 * n} = "
          f"{100 * base_wins / (2 * n):.1f}%")
    print(f"effect: {pt(sum(d) / n)}, 95% CI [{pt(lo)}, {pt(hi)}] "
          f"(cluster bootstrap over seeds, {resamples} resamples)")
    for seat, ds, slo, shi in ((1, d1, lo1, hi1), (2, d2, lo2, hi2)):
        up, down = flips[seat]
        print(f"  on team {seat}: {pt(sum(ds) / n)}, 95% CI [{pt(slo)}, {pt(shi)}]; "
              f"discordant seeds {up} to a win, {down} to a loss; "
              f"McNemar z={mcnemar(up, down):.2f}")
    print(f"  both seats: seeds net for {variant} {seed_net[0]}, against {seed_net[1]}, "
          f"even {seed_net[2]}; McNemar z={mcnemar(seed_net[0], seed_net[1]):.2f}")
    if n < BOOTSTRAP_MIN_SEEDS:
        print(f"  (fewer than {BOOTSTRAP_MIN_SEEDS} seeds: the bootstrap interval is too "
              f"narrow at this size; run more seeds)")


if __name__ == "__main__":
    sys.exit(main())
