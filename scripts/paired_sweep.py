#!/usr/bin/env python3
"""Paired before/after analysis for a balance sweep, with the tier stated.

Both arms run the SAME batch JSONL — same comps, same seeds, same map — so
every match has a twin that differs only by the change. The instrument is
therefore McNemar's test on the matches whose outcome FLIPPED, not a
two-sample comparison of two win rates; Wilson intervals are printed for the
LEVEL and are not the test.

Four slices fall out of one fact about the change — which classes it can
reach, given by `--affects`:

    CONTROL    neither side fields an affected class. The change cannot reach
               these matches, so they must be bit-identical in winner AND
               duration. This is a CORRECTNESS check, it saturates at a few
               hundred matches, and it is separate from the delta. It is a
               claim only over the classes it fields, so those are printed
               per side, and a class it could have fielded and did not is a
               blind spot that fails it.
    CLEAN      only team1 fields one. This is the effect, uncontaminated.
    AGAINST    only team2 fields one. The same effect, pointed the other way.
    MIRRORED   both sides field one, so the effect is expected to WASH.

`--affects` names classes, so the slicing is only right for a change that
reaches a CLASS — an ability, a class loadout, a spell coefficient. A change
scoped to ONE SIDE instead (a `team1_equipment` override, a per-team AI
profile) reaches team2's copies of that class not at all, so what this tool
labels MIRRORED is really a second CLEAN slice pointing the same way. AS-60
armed team 1 only and its "mirrored" cells came back +19.7pt for exactly that
reason. Use `scripts/headtohead_sweep.py` for per-side changes, or slice by
hand and say so.

Read `docs/design/balance/sweep-tiers.md` before citing anything this prints.
The two things it will not let you skip:

* **The resolution is printed next to every delta.** A paired run's power comes
  from its DISCORDANT pairs, so the smallest delta it could have resolved is a
  property of the run, not of its nominal n. A "no movement" result means
  nothing without it, which is why `--expect nothing` prints it in the verdict.
* **The slice count is printed alongside any significant slice.** Testing four
  slices and reporting the one that came back is what multiple comparisons
  produce. A significant MIRRORED slice — the one expected to wash — is called
  out as noise-unless-corroborated rather than reported as a finding.

Usage:
    paired_sweep.py before.csv after.csv --affects Shaman --tier directional
    paired_sweep.py before.csv after.csv --affects Mage,Priest \\
        --tier authority --expect move --per-cell
"""

from __future__ import annotations

import argparse
import csv
import math
import sys
from collections import OrderedDict

# Below this, a per-cell win rate is a direction and not a value. 100 paired
# matches buy about +/-9.7pt on a single cell, which cannot rank two comps.
# `--per-cell` prints the half-width on every row regardless, so the number is
# never separated from what it can support.
CELL_N_FOR_A_VALUE = 400


def wilson(k, n, z=1.96):
    """Wilson score interval for k successes in n trials.

    With no trials the interval is the whole range: n=0 carries no information,
    and (0, 0) would make an empty slice read as a confident 0% win rate.
    """
    if n == 0:
        return (0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    centre = (p + z * z / (2 * n)) / d
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, centre - half), min(1.0, centre + half))


def mcnemar(to_win, to_loss):
    """Continuity-corrected McNemar z over the two discordant counts.

    Zero discordant pairs is z=0, not a division by zero: a run in which
    nothing flipped has shown no difference, which is a result.

    The correction is clamped at zero. Uncorrected, `|b - c| - 1` goes NEGATIVE
    whenever the two directions are balanced to within one flip -- one each way
    printed as z=-0.71 -- which reads as a statistic pointing somewhere when it
    is the most perfectly null result the test can produce.
    """
    d = to_win + to_loss
    if d == 0:
        return 0.0
    return max(0.0, abs(to_win - to_loss) - 1) / math.sqrt(d)


def min_detectable_delta(discordant, n):
    """The smallest |delta| this run could have called significant, in points.

    McNemar reaches z=1.96 when |b - c| >= 1.96*sqrt(b+c) + 1, and the delta a
    given imbalance produces is (b - c)/n. So the floor is a property of the
    DISCORDANCE the run actually produced and not of its nominal n: at
    n=2,500 a change that flips 2% of the sweep resolves 0.6pt and one that
    flips 80% resolves 3.5pt. A floor quoted from n alone is wrong in both
    directions, and wrong in the direction that overstates precision exactly
    when the change flips a lot.

    Returns None when nothing flipped — there is no resolution to state, and
    a number here would read as one.
    """
    if n == 0 or discordant == 0:
        return None
    return 100.0 * (1.96 * math.sqrt(discordant) + 1) / n


def classes(team):
    """The set of class names in a team cell, however the CSV spelled it."""
    return set(c for c in team.replace("|", "+").split("+") if c)


def load(path):
    """Per-match rows keyed by (label, seed), plus the error count.

    Errored matches are dropped rather than counted: a failed match is not a
    loss, and pairing against one would silently compare a result to nothing.
    """
    rows, errors = OrderedDict(), 0
    try:
        handle = open(path, newline="", encoding="utf-8")
    except OSError as exc:
        sys.exit("cannot read %s: %s" % (path, exc))
    with handle:
        reader = csv.DictReader(handle)
        wanted = ("label", "seed", "winner", "team1", "team2", "duration_secs")
        missing = [c for c in wanted if c not in (reader.fieldnames or [])]
        if missing:
            sys.exit("%s is not a batch results CSV (missing %s)"
                     % (path, ", ".join(missing)))
        for row in reader:
            winner = row["winner"].strip()
            if winner == "error":
                errors += 1
                continue
            rows[(row["label"], row["seed"])] = (
                winner, row["team1"], row["team2"], float(row["duration_secs"]))
    return rows, errors


class Slice:
    """One named subset of the paired matches, and what it measured."""

    def __init__(self, name, keys, before, after, expected_to_wash=False,
                 aggregate=False):
        self.name = name
        self.n = len(keys)
        self.expected_to_wash = expected_to_wash
        # An aggregate is the headline, not a comparison. Counting it among
        # the slices tested would inflate every multiple-comparisons
        # denominator by one and make "one significant slice of two" the
        # normal reading of a single real effect.
        self.aggregate = aggregate
        self.before_wins = sum(1 for k in keys if before[k][0] == "team1")
        self.after_wins = sum(1 for k in keys if after[k][0] == "team1")
        self.to_win = sum(1 for k in keys
                          if before[k][0] != "team1" and after[k][0] == "team1")
        self.to_loss = sum(1 for k in keys
                           if before[k][0] == "team1" and after[k][0] != "team1")
        self.moved = sum(1 for k in keys if before[k][0] != after[k][0])
        self.durations_moved = sum(1 for k in keys if before[k][3] != after[k][3])
        self.z = mcnemar(self.to_win, self.to_loss)
        self.floor = min_detectable_delta(self.to_win + self.to_loss, self.n)

    @property
    def delta(self):
        return 100.0 * (self.after_wins - self.before_wins) / self.n if self.n else 0.0

    @property
    def significant(self):
        return self.n > 0 and self.z >= 1.96

    def line(self):
        if self.n == 0:
            return "%-34s (no matches in this slice)" % self.name
        blo, bhi = wilson(self.before_wins, self.n)
        alo, ahi = wilson(self.after_wins, self.n)
        floor = ("resolves >=%.1fpt" % self.floor if self.floor is not None
                 else "no flips: unresolvable")
        return ("%-34s n=%-5d before %5.1f%% [%.1f-%.1f]  after %5.1f%% [%.1f-%.1f]  "
                "delta %+5.1fpt  flips %d (+%d/-%d) z=%.2f %s  %s"
                % (self.name, self.n,
                   100 * self.before_wins / self.n, 100 * blo, 100 * bhi,
                   100 * self.after_wins / self.n, 100 * alo, 100 * ahi,
                   self.delta, self.moved, self.to_win, self.to_loss, self.z,
                   "SIG" if self.significant else "ns ", floor))


def build_slices(keys, before, affected):
    """The four standard slices, keyed off which side fields an affected class."""
    buckets = {"clean": [], "against": [], "mirrored": [], "control": []}
    for key in keys:
        on_t1 = bool(classes(before[key][1]) & affected)
        on_t2 = bool(classes(before[key][2]) & affected)
        if on_t1 and on_t2:
            buckets["mirrored"].append(key)
        elif on_t1:
            buckets["clean"].append(key)
        elif on_t2:
            buckets["against"].append(key)
        else:
            buckets["control"].append(key)
    return buckets


def control_coverage(control, keys, before, affected):
    """Per side, (owed, fielded): which unaffected classes the control must field.

    A side OWES every class it fields, anywhere in the sweep, in a team with no
    affected class -- the teams a control cell could have been built from. That
    is derived from the sweep itself, so a template that pins team1 owes only
    what team1 ever fields, and a class that only ever appears beside an
    affected one is owed nowhere: no control could have fielded it.
    """
    sides = []
    for side in (1, 2):
        owed, fielded = set(), set()
        for k in keys:
            team = classes(before[k][side])
            if not team & affected:
                owed |= team
        for k in control:
            fielded |= classes(before[k][side])
        sides.append((owed, fielded))
    return sides


def report_control(keys, before, after, elsewhere=None, coverage=None):
    """The control as the bit-exactness claim it is, not as a win rate.

    A control slice that MOVES is an instrument failure — the change reached a
    match it cannot reach — and is worth more than any delta in the run, so it
    is printed first and stated as pass or fail rather than as a rate.

    An identical control is a claim only over the classes it FIELDS, so which
    those are is printed with it, per side (AS-143). A class the sweep could
    have put in the control and did not is a blind spot: a leak into it would
    pass the control clean. That fails the control just as an absent one does,
    and `--control-elsewhere` excuses it the same way, by a declaration printed
    into the report.
    """
    n = len(keys)
    if n == 0:
        if elsewhere:
            # Legitimately common: AS-122's Rogue arms all field a Rogue, and
            # its control was a separate Hunter sweep. The obligation is still
            # the author's, so the reason is printed INTO the report a findings
            # doc quotes, rather than discharged by an exit code nobody read.
            print("CONTROL: none in this pair, by declaration — %s" % elsewhere)
            return True
        print("CONTROL: no matches in which the change can reach NEITHER side. "
              "Nothing here bounds the diff's blast radius — widen the sweep, "
              "or pass --control-elsewhere with where it lives.")
        return False
    same = sum(1 for k in keys
               if before[k][0] == after[k][0] and before[k][3] == after[k][3])
    blind = []
    if coverage is not None:
        for label, (owed, fielded) in zip(("team1", "team2"), coverage):
            print("CONTROL FIELDS on %s: %s (%d of the %d unaffected classes "
                  "the sweep fields there)"
                  % (label, ", ".join(sorted(fielded)) or "nothing",
                     len(fielded), len(owed)))
            if owed - fielded:
                blind.append("%s on %s" % (", ".join(sorted(owed - fielded)), label))
    if same == n and not blind:
        print("CONTROL: %d/%d matches with no affected class on either side are "
              "IDENTICAL in winner and duration. The diff's blast radius is "
              "bounded by measurement." % (same, n))
        return True
    if same == n:
        print("CONTROL: %d/%d matches with no affected class on either side are "
              "IDENTICAL in winner and duration — but only over the classes "
              "they field." % (same, n))
        print("CONTROL BLIND SPOT: the control never fields %s. A leak into "
              "those would pass it clean, so it does not bound the blast "
              "radius there." % "; nor ".join(blind))
        if elsewhere:
            print("  Covered elsewhere, by declaration — %s" % elsewhere)
            return True
        print("  Regenerate the sweep (gen_sweep.py's control fields every "
              "class on both sides), or pass --control-elsewhere with where those "
              "classes are covered.")
        return False
    print("CONTROL FAILED: %d of %d unreachable matches DIFFER in winner or "
          "duration. The change reached a match it cannot reach — fix the "
          "instrument before reading any delta below." % (n - same, n))
    return False


def report_non_vacuity(keys, before, after):
    """Decisive events, not row counts.

    A no-movement claim over a batch that drew every match, or that ran out the
    clock, proves nothing — so the counts that carry the weight are printed
    whether or not anyone asked.
    """
    decisive = sum(1 for k in keys if after[k][0] in ("team1", "team2"))
    moved_any = sum(1 for k in keys
                    if before[k][0] != after[k][0] or before[k][3] != after[k][3])
    print("NON-VACUITY: %d/%d paired matches ended by elimination (%d draws); "
          "%d moved in winner or duration; distinct durations %d before / %d after."
          % (decisive, len(keys), len(keys) - decisive, moved_any,
             len(set(before[k][3] for k in keys)),
             len(set(after[k][3] for k in keys))))
    if moved_any == 0:
        print("  Nothing moved anywhere. Either the change is inert, or it never "
              "reached this sweep — say which, positively, before reporting a null.")


def report_per_cell(keys, before, after):
    """Per-cell deltas, each carrying its own half-width."""
    print("Per cell (direction only below n=%d):" % CELL_N_FOR_A_VALUE)
    cells = OrderedDict()
    for key in keys:
        cells.setdefault(key[0], []).append(key)
    for label, cell_keys in sorted(cells.items()):
        cell = Slice(label, cell_keys, before, after)
        lo, hi = wilson(cell.after_wins, cell.n)
        note = "" if cell.n >= CELL_N_FOR_A_VALUE else "   DIRECTION ONLY"
        print("  %-42s n=%-4d delta %+5.1fpt  +/-%.1fpt%s"
              % (label, cell.n, cell.delta, 100 * (hi - lo) / 2, note))


def report_slice_count(slices):
    """The multiple-comparisons discipline, applied rather than remembered.

    Only the DISJOINT slices count as comparisons; the reachable aggregate is
    the headline and is excluded from the denominator.
    """
    tested = [s for s in slices if s.n > 0 and not s.aggregate]
    sig = [s for s in tested if s.significant]
    print("SLICES TESTED: %d. Significant: %s."
          % (len(tested), ", ".join(s.name for s in sig) if sig else "none"))
    for s in sig:
        if s.expected_to_wash:
            print("  NOISE UNLESS CORROBORATED: %s is the slice where the effect "
                  "is expected to WASH, and it came back significant out of %d "
                  "slices tested. Report it as a count, not as a finding, unless "
                  "a mechanism metric corroborates it. If a THIRD card lands one "
                  "of these, it becomes its own card — add the row to AS-104."
                  % (s.name, len(tested)))
    if len(tested) > 1 and len(sig) == 1 and not sig[0].expected_to_wash:
        print("  One significant slice out of %d tested. State that ratio wherever "
              "this number is cited." % len(tested))


def report_verdict(overall, expect):
    """Answer the success condition the card stated, in its own terms."""
    if expect is None:
        print("NO SUCCESS CONDITION DECLARED (--expect). This report says what "
              "moved; it cannot say whether that is the answer the card wanted. "
              "AS-104 asks the author to state one BEFORE the sweep runs, "
              "because \"I expect this to move nothing\" and \"I need to know "
              "how much\" want different instruments.")
        return
    if expect == "nothing":
        if overall.significant:
            print("VERDICT: the run CONTRADICTS its success condition — expected "
                  "no movement, measured %+.1fpt (z=%.2f) over the reachable "
                  "matches." % (overall.delta, overall.z))
        elif overall.floor is None:
            print("VERDICT: nothing flipped anywhere in the reachable matches. "
                  "The change is inert on this sweep — which is the expected "
                  "answer only if it was expected to be inert, not merely small.")
        else:
            print("VERDICT: consistent with no movement. This run could have "
                  "resolved a delta of %.1fpt or more and measured %+.1fpt, so it "
                  "rules out an effect above that floor and says nothing below it."
                  % (overall.floor, overall.delta))
    elif expect == "move":
        if overall.significant:
            print("VERDICT: %+.1fpt over the reachable matches (z=%.2f), resolved."
                  % (overall.delta, overall.z))
        else:
            print("VERDICT: %+.1fpt over the reachable matches, NOT resolvable at "
                  "this size (%s). Report the direction; do not cite the magnitude."
                  % (overall.delta,
                     "floor %.1fpt" % overall.floor if overall.floor is not None
                     else "nothing flipped"))


def main(argv=None):
    ap = argparse.ArgumentParser(
        description=__doc__.splitlines()[0],
        epilog="Full rules: docs/design/balance/sweep-tiers.md")
    ap.add_argument("before", help="batch results CSV for the base arm")
    ap.add_argument("after", help="batch results CSV for the changed arm")
    ap.add_argument("--affects", required=True,
                    help="comma-separated classes the change can reach, e.g. "
                         "'Shaman' or 'Mage,Priest'. Defines all four slices.")
    ap.add_argument("--tier", required=True, choices=("directional", "authority"),
                    help="which instrument this run is. A directional run may "
                         "not be cited as a class's standing.")
    ap.add_argument("--expect", choices=("move", "nothing"), default=None,
                    help="the card's success condition. 'nothing' makes the "
                         "verdict state the resolution the run bought, because "
                         "a null is meaningless without it.")
    ap.add_argument("--control-elsewhere", metavar="WHERE", default=None,
                    help="declare that this pair has no unreachable cells, or "
                         "that its control misses a class it could have "
                         "fielded, and say where that is covered instead (e.g. "
                         "'a separate Hunter arm, <file>'). Printed into the "
                         "report; without it such a run exits non-zero. It "
                         "never excuses a control that moved.")
    ap.add_argument("--per-cell", action="store_true",
                    help="also print per-cell deltas, each with its own "
                         "half-width. Off by default: at directional n a cell "
                         "is a direction, not a value.")
    args = ap.parse_args(argv)

    affected = set(c.strip() for c in args.affects.split(",") if c.strip())
    if not affected:
        sys.exit("--affects named no classes")

    before, before_errors = load(args.before)
    after, after_errors = load(args.after)
    keys = [k for k in before if k in after]
    if not keys:
        sys.exit("no (label, seed) pair appears in both CSVs — the two arms did "
                 "not run the same batch file, so there is nothing to pair")

    unpaired = (len(before) - len(keys)) + (len(after) - len(keys))
    print("TIER: %s%s" % (args.tier.upper(),
                          "    success condition: expected to %s" % args.expect
                          if args.expect else ""))
    print("affected classes: %s" % ", ".join(sorted(affected)))
    print("paired matches: %d  (errors before=%d after=%d, unpaired rows=%d)"
          % (len(keys), before_errors, after_errors, unpaired))
    if unpaired:
        print("  WARNING: %d row(s) had no twin and are excluded. A paired claim "
              "over a partly-paired run is not paired." % unpaired)
    print()

    buckets = build_slices(keys, before, affected)
    control_ok = report_control(
        buckets["control"], before, after, args.control_elsewhere,
        control_coverage(buckets["control"], keys, before, affected))
    print()

    reachable = buckets["clean"] + buckets["against"] + buckets["mirrored"]
    slices = [
        Slice("ALL reachable", reachable, before, after, aggregate=True),
        Slice("CLEAN (only team1 affected)", buckets["clean"], before, after),
        Slice("AGAINST (only team2 affected)", buckets["against"], before, after),
        Slice("MIRRORED (both sides)", buckets["mirrored"], before, after,
              expected_to_wash=True),
    ]
    print("team1 win rate, paired at identical seeds; McNemar z on the flips:")
    for one in slices:
        print("  " + one.line())
    print()

    report_non_vacuity(keys, before, after)
    print()

    if args.per_cell:
        report_per_cell(keys, before, after)
        print()

    report_slice_count(slices)
    print()

    report_verdict(slices[0], args.expect)
    if args.tier == "directional":
        print("Directional tier: this is which-way-and-roughly-how-much. It may "
              "not be cited as a class's standing.")
    return 0 if control_ok else 1


if __name__ == "__main__":
    sys.exit(main())
