#!/usr/bin/env python3
"""Offline fixture tests for `scripts/agg_sweep.py`.

`agg_sweep.py --compare` is where a balance change becomes a claim: it prints a
per-matchup delta and flags the cells it considers MOVED. AS-44's Tester had to
read the flagging line out of the source to find out what MOVED even meant,
because nothing pinned it. These cases pin it -- including at the boundary,
where one extra win flips the verdict.

Statistics are checked against EXTERNAL values, never against what this
codebase computes: the intervals are Newcombe's (1998) four worked examples
(*Statistics in Medicine* 17:857-872, method 3), and each MOVED boundary case
names the two intervals it sits between.

Everything here is file I/O over hand-built CSVs, and the module's `subprocess`
is replaced with a guard, so an offline run is a property of the harness rather
than a hope.

Run directly, or via `cargo test --test sweep_script_fixtures`:

    python3 scripts/tests/test_agg_sweep.py
"""

from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from sweep_fixtures import (  # noqa: E402
    FixtureTestCase,
    assert_runs_on_min_python,
    install_no_subprocess,
    match_rows,
    run_main,
)

import agg_sweep as agg  # noqa: E402

install_no_subprocess(agg)

Z_EXACT = 1.959963985


class WilsonTests(unittest.TestCase):
    """Newcombe (1998) worked examples, method 3 (Wilson score)."""

    def assertInterval(self, got, lo, hi):
        self.assertAlmostEqual(got[0], lo, places=4)
        self.assertAlmostEqual(got[1], hi, places=4)

    def test_newcombe_81_of_263(self):
        self.assertInterval(agg.wilson_interval(81, 263, Z_EXACT), 0.2553, 0.3662)

    def test_newcombe_15_of_148(self):
        self.assertInterval(agg.wilson_interval(15, 148, Z_EXACT), 0.0624, 0.1605)

    def test_newcombe_0_of_20(self):
        lo, hi = agg.wilson_interval(0, 20, Z_EXACT)
        self.assertInterval((lo, hi), 0.0000, 0.1611)
        self.assertEqual(lo, 0.0)

    def test_newcombe_1_of_29(self):
        self.assertInterval(agg.wilson_interval(1, 29, Z_EXACT), 0.0061, 0.1718)

    def test_the_interval_is_not_centred_on_the_raw_rate(self):
        """Why `p +/- halfwidth` is not the interval, and never was.

        The Wilson centre is pulled toward 50%, so an interval built by adding
        and subtracting the half-width from the raw rate sits in the wrong
        place -- further from 50% at both ends. That is what made the old MOVED
        rule report a clean separation between two intervals that overlap.
        """
        lo, hi = agg.wilson_interval(20, 100)
        p, centre, half = 0.20, (lo + hi) / 2, (hi - lo) / 2
        self.assertGreater(centre, p + 0.005)
        # The naive interval's lower end is below the real one's by that shift.
        self.assertLess(p - half, lo)

    def test_zero_matches_is_no_information(self):
        """n=0 must not read as a confident 0%."""
        self.assertEqual(agg.wilson_interval(0, 0), (0.0, 1.0))

    def test_bounds_stay_in_range(self):
        for k, n in [(0, 1), (1, 1), (0, 40), (40, 40), (99, 100)]:
            lo, hi = agg.wilson_interval(k, n)
            self.assertGreaterEqual(lo, 0.0)
            self.assertLessEqual(hi, 1.0)


class AggTestCase(FixtureTestCase):
    def agg(self, *argv):
        return run_main(agg.main, list(argv))


class TallyTests(AggTestCase):
    def test_per_matchup_counts_sum_to_the_group_total(self):
        path = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=12, t2=6, draw=2),
            match_rows("A_vs_C", "Warrior", "Rogue", t1=3, t2=17),
        )
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "(W12 L6 D2)")
        self.assertHas(run, "(W3 L17 D0)")

    def test_overall_pools_every_selected_group(self):
        """The AS-44 hand check: per-cell wins sum to the slice total."""
        path = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=12, t2=8),
            match_rows("A_vs_C", "Warrior", "Rogue", t1=3, t2=17),
            match_rows("A_vs_D", "Warrior", "Priest", t1=25, t2=15),
        )
        run = self.assertOk(self.agg(path))
        # 12 + 3 + 25 = 40 wins of 20 + 20 + 40 = 80 matches.
        self.assertHas(run, "(40/80)")
        self.assertHas(run, "50.0%")

    def test_label_falls_back_to_the_team_pair(self):
        rows = match_rows("", "Warrior+Priest", "Mage+Priest", t1=3, t2=1)
        for r in rows:
            r["label"] = ""
        path = self.csv_with(rows)
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "Warrior+Priest|Mage+Priest")

    def test_draws_are_losses_for_team1_but_stay_in_n(self):
        path = self.csv_with(match_rows("A_vs_B", "Warrior", "Mage", t1=5, t2=0, draw=5))
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "(5/10)")
        self.assertHas(run, "(W5 L0 D5)")


class FilterTests(AggTestCase):
    def _three(self):
        return self.csv_with(
            match_rows("Hunter+Mage_vs_Warrior", "Hunter+Mage", "Warrior", t1=10, t2=10),
            match_rows("Hunter+Priest_vs_Mage", "Hunter+Priest", "Mage", t1=15, t2=5),
            match_rows("Hunter+Rogue_vs_Mage", "Hunter+Rogue", "Mage", t1=5, t2=15),
        )

    def test_include_keeps_only_matching_labels(self):
        run = self.assertOk(self.agg(self._three(), "--include", "_vs_Mage"))
        self.assertHas(run, "(20/40)")
        self.assertLacks(run, "Hunter+Mage_vs_Warrior")

    def test_exclude_drops_matching_labels(self):
        run = self.assertOk(
            self.agg(self._three(), "--include", "_vs_Mage", "--exclude", r"Hunter\+Rogue")
        )
        self.assertHas(run, "(15/20)")

    def test_a_filter_that_matches_nothing_is_an_error(self):
        run = self.agg(self._three(), "--include", "nothing_matches_this")
        self.assertEqual(run.code, 1)
        self.assertErrHas(run, "no matching groups")

    def test_group_sub_aggregates_by_capture(self):
        run = self.assertOk(self.agg(self._three(), "--group", r"Hunter\+([A-Za-z]+)_vs"))
        self.assertHas(run, "By group:")
        self.assertHas(run, "(15/20)")  # Priest
        self.assertHas(run, "(5/20)")  # Rogue

    def test_overall_only_suppresses_the_per_matchup_table(self):
        run = self.assertOk(self.agg(self._three(), "--overall-only"))
        self.assertLacks(run, "Per matchup")


class MovedBoundaryTests(AggTestCase):
    """What `--compare` flags, pinned at the point where it changes its mind."""

    def _pair(self, before_wins, after_wins, n=100):
        before = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=before_wins, t2=n - before_wins),
            name="before.csv",
        )
        after = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=after_wins, t2=n - after_wins),
            name="after.csv",
        )
        return self.assertOk(self.agg(after, "--compare", before))

    def test_just_separate_is_moved(self):
        """50/100 [40.4-59.6] vs 70/100 [60.4-78.1]: a 0.8pt gap. MOVED."""
        run = self._pair(50, 70)
        self.assertHas(run, "<== MOVED")
        self.assertHas(run, "+20.0")

    def test_just_touching_is_not_moved(self):
        """50/100 [40.4-59.6] vs 69/100 [59.4-77.2]: they overlap by 0.25pt.

        One fewer win than the case above, and the verdict flips. This is the
        boundary a balance claim lives or dies on, and it is the case the old
        `|delta| > halfwidth + halfwidth` rule got wrong: that rule compares
        intervals centred on the raw rates rather than the Wilson centres, so
        it called this pair separated and flagged it MOVED.
        """
        run = self._pair(50, 69)
        self.assertLacks(run, "<== MOVED")
        self.assertHas(run, "+19.0")

    def test_a_drop_moves_too(self):
        run = self._pair(70, 50)
        self.assertHas(run, "<== MOVED")
        self.assertHas(run, "-20.0")

    def test_the_wash_trap_is_not_a_finding(self):
        """The docstring's example: 37.0 vs 37.7 is not a change."""
        before = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=370, t2=630), name="before.csv"
        )
        after = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=377, t2=623), name="after.csv"
        )
        run = self.assertOk(self.agg(after, "--compare", before))
        self.assertLacks(run, "<== MOVED")

    def test_a_huge_shift_at_tiny_n_is_not_a_finding(self):
        """3/5 -> 5/5 is +40pt and means nothing. Sample size, not delta."""
        run = self._pair(3, 5, n=5)
        self.assertHas(run, "+40.0")
        self.assertLacks(run, "<== MOVED")

    def test_a_group_missing_from_the_baseline_is_not_compared(self):
        before = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=50, t2=50), name="before.csv"
        )
        after = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=50, t2=50),
            match_rows("A_vs_NEW", "Warrior", "Rogue", t1=50, t2=50),
            name="after.csv",
        )
        run = self.assertOk(self.agg(after, "--compare", before))
        self.assertHas(run, "A_vs_NEW")
        self.assertEqual(run.out.count("[was "), 1)

    def test_the_printed_interval_is_the_one_moved_is_decided_on(self):
        """A reader must be able to check the flag from the same line."""
        run = self._pair(50, 70)
        self.assertHas(run, "40.4-59.6")
        self.assertHas(run, "60.4-78.1")

    def test_an_unmeasured_cell_prints_no_delta(self):
        """No measurement is not a 50-point drop.

        A cell whose every match errored has nothing to compare against its
        baseline, but the delta was still computed off a 0.0 rate, so the
        line read `n/a ... [was 50.0% [40.4-59.6], -50.0]` -- a finding
        where there is not even a datum.
        """
        before = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=50, t2=50), name="before.csv"
        )
        after = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", error=100),
            match_rows("C_vs_D", "Rogue", "Priest", t1=50, t2=50),
            name="after.csv",
        )
        run = self.assertOk(self.agg(after, "--compare", before))
        self.assertHas(run, "[was 50.0% [40.4-59.6]]")
        self.assertLacks(run, "-50.0")
        self.assertLacks(run, "<== MOVED")

    def test_an_unmeasured_baseline_prints_no_delta_either(self):
        """The suppression is symmetric: a new cell has nothing to move from."""
        before = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", error=100), name="before.csv"
        )
        after = self.csv_with(
            match_rows("A_vs_B", "Warrior", "Mage", t1=80, t2=20), name="after.csv"
        )
        run = self.assertOk(self.agg(after, "--compare", before))
        self.assertHas(run, "[was n/a")
        self.assertLacks(run, "+80.0")
        self.assertLacks(run, "<== MOVED")


class DegenerateInputTests(AggTestCase):
    def test_an_empty_csv_is_an_error(self):
        path = self.csv_with([])
        run = self.agg(path)
        self.assertEqual(run.code, 1)
        self.assertErrHas(run, "no matching groups")

    def test_a_single_match_is_not_a_measurement(self):
        path = self.csv_with(match_rows("A_vs_B", "Warrior", "Mage", t1=1))
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "(1/1)")
        self.assertHas(run, "20.7-100.0")  # not 100.0-100.0
        self.assertLacks(run, "100.0-100.0")

    def test_all_wins_and_all_losses(self):
        path = self.csv_with(
            match_rows("ALLWIN", "Warrior", "Mage", t1=20),
            match_rows("ALLLOSS", "Warrior", "Rogue", t2=20),
        )
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "83.9-100.0")  # 20/20, Newcombe's 0/20 reflected
        self.assertHas(run, "0.0-16.1")  # 0/20
        self.assertLacks(run, "-0.0")

    def test_errored_matches_leave_the_denominator(self):
        """An error is not a loss.

        Counting one inflates n and drags the rate down, silently, on a run
        whose only symptom is a one-line warning. The sibling tool
        (`headtohead_sweep.py`) already excludes them; so does this one.
        """
        path = self.csv_with(match_rows("A_vs_B", "Warrior", "Mage", t1=5, t2=5, error=10))
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "(5/10)")  # not 5/20
        self.assertHas(run, "50.0%")
        self.assertLacks(run, "25.0%")
        self.assertHas(run, "10 match(es) errored")

    def test_a_group_with_no_usable_matches_is_not_zero_percent(self):
        path = self.csv_with(
            match_rows("OK", "Warrior", "Mage", t1=5, t2=5),
            match_rows("ALLERR", "Warrior", "Rogue", error=8),
        )
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "ALLERR")
        self.assertHas(run, "n/a")
        # The good group is unaffected by its neighbour's ruin.
        self.assertHas(run, "(5/10)")

    def test_every_match_errored_is_an_error_not_a_report(self):
        path = self.csv_with(match_rows("ALLERR", "Warrior", "Rogue", error=8))
        run = self.agg(path)
        self.assertEqual(run.code, 1)
        self.assertErrHas(run, "no usable matches")

    def test_an_unmeasured_cell_keeps_the_column_aligned(self):
        """`n/a` occupies the same width as a rate, so the table survives it."""
        self.assertEqual(len(agg.rate_cell(0, 0)), len(agg.rate_cell(5, 10)))
        self.assertEqual(len(agg.rate_cell(0, 0)), len(agg.rate_cell(100, 100)))

    def test_a_padded_winner_reads_the_same_here_as_in_headtohead(self):
        """The two tools read the same CSVs; they must agree on the vocabulary.

        `headtohead_sweep.py` strips the field, so " error" is an error there.
        Untrimmed, it fell through to the draw branch here and stayed in n.
        """
        rows = match_rows("A_vs_B", "Warrior", "Mage", t1=5, t2=0, error=5)
        for r in rows:
            if r["winner"] == "error":
                r["winner"] = " error "
        path = self.csv_with(rows)
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "(5/5)")
        self.assertHas(run, "(W5 L0 D0)")
        self.assertHas(run, "5 match(es) errored")

    def test_an_unknown_winner_value_counts_as_a_draw(self):
        """Anything that is not team1/team2/error is a non-win, as today."""
        rows = match_rows("A_vs_B", "Warrior", "Mage", t1=5, t2=0, draw=5)
        for r in rows:
            if r["winner"] == "draw":
                r["winner"] = "timeout"
        path = self.csv_with(rows)
        run = self.assertOk(self.agg(path))
        self.assertHas(run, "(W5 L0 D5)")


class InterpreterFloorTests(unittest.TestCase):
    """`agg_sweep.py` must still import on the stock system interpreter."""

    def test_the_tool_runs_on_the_minimum_interpreter(self):
        assert_runs_on_min_python(self, agg)


if __name__ == "__main__":
    unittest.main(verbosity=2)
