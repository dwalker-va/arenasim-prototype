#!/usr/bin/env python3
"""Head-to-head AI-profile sweep at real statistical power.

This is the measurement methodology from `docs/design/team-level-positioning-ai.md`
("How to measure a step") as a tool instead of folklore:

- HEAD-TO-HEAD, not uniform: each cell sets the two teams' AI profiles
  independently, so the two implementations play each other on the same seed.
  A uniform A/B (both teams switched together) compares two internally
  consistent worlds and cannot answer "is the new AI better".
- BOTH assignments are run (team 1 gets the profile, then team 2), because
  comps are rarely evenly matched and a single assignment confounds the AI
  with the comp. Each side's GAIN is reported against the uniform baseline.
- n=100 per cell by default. Win rate is one bit per match: at n=12 this
  codebase measured a +8pt effect that was really +36, and a -17pt one that
  was really +14. Draws are counted, never credited to a side.
- Wilson 95% intervals and a two-proportion z-test, so "one extra win" cannot
  be mistaken for a result.

Runs via the parallel batch runner (~2 matches/sec on an M-class laptop, so a
3-cell sweep at n=100 is ~3 minutes). Win rate is the CONFIRMATION metric;
for diagnosis prefer the per-frame mechanism metrics in `tests/camp_sweep.rs`
(occlusion-seconds, blocked share), which aggregate thousands of samples per
match instead of one bit.

Example:
    scripts/headtohead_sweep.py --team1 Warrior,Priest --team2 Warlock,Priest
    scripts/headtohead_sweep.py --team1 Hunter,Priest --team2 Rogue,Priest \
        --map PillaredArena --seeds 100 --profile TeamPlan
"""

import argparse
import csv
import json
import math
import subprocess
import sys
import tempfile
from pathlib import Path


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


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--team1", required=True, help="comma-separated classes, e.g. Warrior,Priest")
    ap.add_argument("--team2", required=True)
    ap.add_argument("--map", default="PillaredArena")
    ap.add_argument("--seeds", type=int, default=100, help="seeds per cell (default 100)")
    ap.add_argument("--seed-base", type=int, default=1)
    ap.add_argument("--profile", default="TeamPlan", help="profile under test vs Legacy")
    ap.add_argument("--max-duration", type=float, default=300.0)
    ap.add_argument("--keep", metavar="PREFIX", help="keep the JSONL/CSV at this path prefix")
    args = ap.parse_args(argv)

    t1, t2 = args.team1.split(","), args.team2.split(",")
    cells = [
        ("LL", "Legacy", "Legacy"),
        ("TL", args.profile, "Legacy"),
        ("LT", "Legacy", args.profile),
    ]

    workdir = Path(tempfile.mkdtemp(prefix="headtohead_"))
    jsonl = Path(f"{args.keep}.jsonl") if args.keep else workdir / "sweep.jsonl"
    out = Path(f"{args.keep}.csv") if args.keep else workdir / "sweep.csv"

    with open(jsonl, "w") as f:
        for label, p1, p2 in cells:
            for seed in range(args.seed_base, args.seed_base + args.seeds):
                f.write(json.dumps({
                    "label": label,
                    "team1": t1, "team2": t2,
                    "map": args.map,
                    "max_duration_secs": args.max_duration,
                    "random_seed": seed,
                    "team1_ai_profile": p1,
                    "team2_ai_profile": p2,
                }) + "\n")

    total = len(cells) * args.seeds
    print(f"{total} matches: {args.team1} vs {args.team2} on {args.map}, "
          f"{args.profile} vs Legacy, seeds {args.seed_base}..{args.seed_base + args.seeds - 1}",
          file=sys.stderr)
    subprocess.run(
        ["cargo", "run", "--release", "--quiet", "--",
         "--batch", str(jsonl), "--out", str(out)],
        check=True,
    )

    tally: dict[str, dict[str, int]] = {}
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
    if errors:
        print(f"WARNING: {errors} match(es) errored and are excluded from all rates",
              file=sys.stderr)
    missing = [label for label, _, _ in cells if tally.get(label, {}).get("n", 0) == 0]
    if missing:
        sys.exit(f"no successful matches in cell(s) {missing}; cannot analyze")

    # Every rate below divides by the cell's OWN usable n, never the nominal
    # seed count: errored matches are excluded from the cell, and dividing a
    # real win count by a nominal denominator shrinks the effect toward zero
    # without saying so anywhere in the output.
    print(f"\n{'cell':6} {'n':>4} {'T1':>4} {'T2':>4} {'draw':>5} {'T1%':>5} {'95% CI':>14}")
    for label, _, _ in cells:
        c = tally[label]
        lo, hi = wilson(c["t1"], c["n"])
        print(f"{label:6} {c['n']:>4} {c['t1']:>4} {c['t2']:>4} {c['draw']:>5} "
              f"{100 * c['t1'] / c['n']:>4.0f}% {100 * lo:>6.1f}-{100 * hi:<5.1f}%")

    ll, tl, lt = tally["LL"], tally["TL"], tally["LT"]
    print(f"\n--- {args.profile}'s effect (draws excluded from both sides' win counts) ---")
    for side, k, base, arm in (("team 1", "t1", ll, tl), ("team 2", "t2", ll, lt)):
        gain = 100 * (arm[k] / arm["n"] - base[k] / base["n"])
        print(f"{side} gets it: {base[k]:>3}/{base['n']:<3} -> {arm[k]:>3}/{arm['n']:<3} wins  "
              f"({gain:+.0f}pt, z={ztest(base[k], base['n'], arm[k], arm['n']):+.2f})")
    print("\n|z| >= 1.96 is the conventional 95% bar; near it, run more seeds "
          "(--seeds, --seed-base) rather than re-rolling.", file=sys.stderr)
    if not args.keep:
        print(f"(artifacts in {workdir}; pass --keep PREFIX to keep them elsewhere)",
              file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
