#!/usr/bin/env python3
"""Is team slot 1 advantaged? (AS-104's settling probe.)

Four balance cards produced a tally in which two showed a significant team-1
gain in the MIRRORED slice -- the slice where the effect should wash. AS-104
proposed settling it with "a mirrored slice on a NULL change, two identical
binaries, same seeds". That probe cannot answer the question, whichever way
determinism goes. Two identical binaries at identical seeds either agree or
they do not. If they agree, every flip count is zero and McNemar's z is 0 --
the run has no way to say anything about slot 1. If they disagree, what it has
found is a determinism defect, which is real and worth knowing and still says
nothing about slot 1. (Determinism here is a property the codebase WORKS TO
MAINTAIN, not an axiom -- see AS-58 and AS-75 under "What a byte-identity
result proves" in CLAUDE.md.) And there is no constructible placebo either --
any change that flips a match flipped it because the change reached it.

What IS answerable is the hypothesis's necessary condition. For a change to
produce a team-1 gain in a slice where both sides carry it, slot 1 must carry
an advantage for the change to amplify. That is a ONE-ARM measurement:

  DIAGONAL    identical comps on both sides. Under exchangeable slots, team1
              wins exactly half the decisive matches. No comp-strength
              confound is possible -- the comps ARE the same.
  SWAP-CLOSED every ordered pair of distinct comps, so each cell's mirror
              image is also in the set. Comp strength cancels across the
              pair; a pooled team1 rate off 50% is a slot effect. Lower
              power per match than the diagonal but far more cells, and it
              is the geometry the cards' mirrored slices actually have.

A two-sided exact binomial test against 0.5 on each. Draws are excluded from
the denominator (they credit neither slot) and reported, because a probe that
drew most of its matches has measured little.

    2026-09-20-as104-slot-symmetry.py diag.csv swap.csv
"""

from __future__ import annotations

import csv
import math
import sys
from collections import Counter
from fractions import Fraction


def wilson(k, n, z=1.96):
    if n == 0:
        return (0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / d
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, centre - half), min(1.0, centre + half))


def binom_two_sided(k, n):
    """Exact two-sided binomial p for k successes in n trials at p0 = 0.5.

    Exact rather than normal-approximated: the whole point of this probe is a
    tight null, and an approximation's tail is the one part of it anyone would
    argue with. Symmetric at p0=0.5, so the two-sided p is twice the smaller
    tail, clamped at 1.

    The ratio is built as a `Fraction` of two exact integers and converted
    once at the end. The obvious `2.0 * total / 2.0 ** n` raises OverflowError
    the moment n passes ~1000 -- which is every run this probe is sized for.
    """
    if n == 0:
        return 1.0
    tail = min(k, n - k)
    total = sum(math.comb(n, i) for i in range(tail + 1))
    return min(1.0, float(Fraction(2 * total, 2 ** n)))


def load(path):
    rows = []
    with open(path, newline="", encoding="utf-8") as f:
        for r in csv.DictReader(f):
            rows.append((r["label"], r["team1"], r["team2"],
                         r["winner"].strip(), float(r["duration_secs"])))
    return rows


def report(name, rows):
    outcomes = Counter(r[3] for r in rows)
    t1, t2 = outcomes["team1"], outcomes["team2"]
    decisive = t1 + t2
    draws = len(rows) - decisive - outcomes["error"]
    lo, hi = wilson(t1, decisive)
    p = binom_two_sided(t1, decisive)
    print("%s" % name)
    print("  matches            %d  (%d decisive, %d draws, %d errors)"
          % (len(rows), decisive, draws, outcomes["error"]))
    print("  team1 wins         %d of %d decisive = %.2f%%  [%.2f-%.2f]"
          % (t1, decisive, 100 * t1 / decisive, 100 * lo, 100 * hi))
    print("  vs 50%%             %+.2fpt   exact two-sided p = %.4f  -> %s"
          % (100 * t1 / decisive - 50.0, p,
             "SLOT ASYMMETRY" if p < 0.05 else "symmetric"))
    print("  resolution         this n rules out a slot effect above "
          "+/-%.2fpt" % (100 * (hi - lo) / 2))
    print("  distinct durations %d (non-vacuity: the sweep is not one match "
          "repeated)" % len(set(r[4] for r in rows)))
    return t1, t2


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    print("SLOT SYMMETRY PROBE (AS-104) -- one arm, one binary, no change "
          "under test.\n")
    diag = load(sys.argv[1])
    swap = load(sys.argv[2])

    report("DIAGONAL  (identical comps both sides)", diag)
    print()
    d1, d2 = report("SWAP-CLOSED  (every ordered pair of distinct comps)", swap)
    print()

    # The swap-closed set's own internal check: cell (A,B) and cell (B,A) are
    # both present, so team1's wins in one are team2's opportunities in the
    # other. Pooling them is the comp-free reading; this states that the set
    # really is closed, rather than assuming it.
    cells = set()
    for label, t1, t2, _, _ in swap:
        cells.add((t1, t2))
    unmirrored = [c for c in cells if (c[1], c[0]) not in cells]
    print("swap-closure check: %d cells, %d without their mirror image"
          % (len(cells), len(unmirrored)))
    if unmirrored:
        print("  FAILED: the set is NOT closed under swapping the sides, so "
              "comp strength does not cancel and the pooled rate above is not "
              "a slot measurement.")

    print()
    combined_t1 = sum(1 for r in diag if r[3] == "team1") + d1
    combined_n = combined_t1 + sum(1 for r in diag if r[3] == "team2") + d2
    lo, hi = wilson(combined_t1, combined_n)
    print("COMBINED: team1 won %d of %d decisive = %.2f%% [%.2f-%.2f], exact "
          "two-sided p = %.4f"
          % (combined_t1, combined_n, 100 * combined_t1 / combined_n,
             100 * lo, 100 * hi, binom_two_sided(combined_t1, combined_n)))

    # Non-zero on an unclosed swap set, matching paired_sweep.py's control:
    # the pooled rate above is not a slot measurement, and a caller running
    # this unattended must not read a printed number as a passing one.
    return 1 if unmirrored else 0


if __name__ == "__main__":
    sys.exit(main())
