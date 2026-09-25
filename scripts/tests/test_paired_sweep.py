#!/usr/bin/env python3
"""Offline fixture tests for `scripts/paired_sweep.py`.

This tool is where a directional sweep becomes a sentence somebody quotes, and
every one of the rules `docs/design/balance/sweep-tiers.md` lays down is a
REPORTING block here -- the control verdict, the resolution floor printed next
to each delta, the slice count, the mirrored-slice caution, the verdict against
the card's stated success condition. A regression in one of those does not
crash; it prints a slightly different true-looking sentence, and it is read
straight into a findings doc. So each is pinned by a case that names the claim.

The statistic is checked against an EXTERNAL worked example rather than against
what this code computes: McNemar's standard 121/59 discordant pair, whose
continuity-corrected chi-square is 20.672.

Everything is file I/O over hand-built CSVs, and the module's `subprocess` is
replaced with a guard, so an offline run is a property of the harness.

Run directly, or via `cargo test --test script_fixture_suites`:

    python3 scripts/tests/test_paired_sweep.py
"""

from __future__ import annotations

import math
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from _harness import assert_runs_on_min_python, install_no_subprocess, run_main  # noqa: E402
from sweep_fixtures import FixtureTestCase  # noqa: E402

import paired_sweep as ps  # noqa: E402

install_no_subprocess(ps)


def arm_rows(label, team1, team2, outcomes, durations=None, seed_base=0):
    """One arm's rows for a cell: `outcomes[i]` is the winner at seed i.

    `durations` defaults to a constant, so a test that cares only about winners
    does not have to spell out a duration per row -- but it is settable,
    because "same winner, different duration" is exactly the control failure
    that a winner-only comparison would miss.
    """
    rows = []
    for i, winner in enumerate(outcomes):
        rows.append({
            "label": label,
            "team1": team1,
            "team2": team2,
            "seed": str(seed_base + i),
            "winner": winner,
            "end_reason": "timeout" if winner == "draw" else "elimination",
            "duration_secs": str(durations[i] if durations else 42.0),
        })
    return rows


def wins(n):
    return ["team1"] * n


def losses(n):
    return ["team2"] * n


class StatisticTests(unittest.TestCase):
    def test_mcnemar_matches_the_standard_worked_example(self):
        """121/59 discordants: continuity-corrected chi-square 20.672.

        z is its square root, so this pins the statistic against a published
        value rather than against this file.
        """
        z = ps.mcnemar(121, 59)
        self.assertAlmostEqual(z * z, 20.672, places=3)
        self.assertAlmostEqual(z, math.sqrt(20.672), places=4)

    def test_mcnemar_is_symmetric_in_the_two_directions(self):
        self.assertEqual(ps.mcnemar(25, 10), ps.mcnemar(10, 25))

    def test_no_discordant_pairs_is_zero_not_a_division_by_zero(self):
        self.assertEqual(ps.mcnemar(0, 0), 0.0)

    def test_a_balanced_pair_of_flips_is_zero_and_never_negative(self):
        """`|b - c| - 1` is negative when the two directions are within one.

        Unclamped, one flip each way -- the most null result the test can
        produce -- prints as z=-0.71, which reads as a statistic pointing
        somewhere.
        """
        self.assertEqual(ps.mcnemar(1, 1), 0.0)
        self.assertEqual(ps.mcnemar(7, 7), 0.0)
        self.assertEqual(ps.mcnemar(8, 7), 0.0)

    def test_two_runs_of_the_SAME_n_resolve_6x_differently(self):
        """The claim the tool's whole honesty rests on, at fixed n.

        n is held constant here on purpose: if the floor tracked sample size
        these two would be equal. They differ 6x, which is why a floor quoted
        from n overstates precision whenever the change flips a lot -- the
        case where someone most wants to cite a magnitude.
        """
        quiet = ps.min_detectable_delta(discordant=50, n=2500)     # 2% flipped
        loud = ps.min_detectable_delta(discordant=2000, n=2500)    # 80% flipped
        self.assertAlmostEqual(quiet, 0.594, places=3)
        self.assertAlmostEqual(loud, 3.546, places=3)
        self.assertGreater(loud / quiet, 5.9)
        # The Wilson half-width on the level at this n is about 1.9pt, which
        # sits between them: it would understate one run and overstate the
        # other.
        self.assertLess(quiet, 1.9)
        self.assertGreater(loud, 1.9)

    def test_nothing_flipped_has_no_resolution_to_state(self):
        self.assertIsNone(ps.min_detectable_delta(0, 500))
        self.assertIsNone(ps.min_detectable_delta(10, 0))

    def test_the_floor_is_the_delta_at_which_mcnemar_would_call_it(self):
        """Derived, not asserted: a run at exactly the floor reaches z=1.96."""
        n, discordant = 1000, 400
        floor = ps.min_detectable_delta(discordant, n)
        imbalance = floor / 100.0 * n          # |b - c| at the floor
        to_win = (discordant + imbalance) / 2
        self.assertAlmostEqual(ps.mcnemar(to_win, discordant - to_win), 1.96, places=6)


class SliceTests(unittest.TestCase):
    def test_classes_splits_either_spelling_of_a_team_cell(self):
        self.assertEqual(ps.classes("Mage+Priest"), {"Mage", "Priest"})
        self.assertEqual(ps.classes("Mage|Priest"), {"Mage", "Priest"})
        self.assertEqual(ps.classes("Mage"), {"Mage"})

    def test_the_four_slices_partition_by_which_side_is_affected(self):
        before = {
            ("clean", "0"): ("team1", "Shaman+Mage", "Warrior+Rogue", 1.0),
            ("against", "0"): ("team1", "Warrior+Rogue", "Shaman+Mage", 1.0),
            ("mirror", "0"): ("team1", "Shaman+Mage", "Shaman+Rogue", 1.0),
            ("control", "0"): ("team1", "Warrior+Mage", "Rogue+Priest", 1.0),
        }
        got = ps.build_slices(list(before), before, {"Shaman"})
        self.assertEqual([k[0] for k in got["clean"]], ["clean"])
        self.assertEqual([k[0] for k in got["against"]], ["against"])
        self.assertEqual([k[0] for k in got["mirrored"]], ["mirror"])
        self.assertEqual([k[0] for k in got["control"]], ["control"])

    def test_every_paired_match_lands_in_exactly_one_slice(self):
        before = {
            (str(i), "0"): ("team1", t1, t2, 1.0)
            for i, (t1, t2) in enumerate([
                ("Shaman", "Mage"), ("Mage", "Shaman"), ("Shaman", "Shaman"),
                ("Mage", "Priest"), ("Priest+Shaman", "Mage+Warrior"),
            ])
        }
        got = ps.build_slices(list(before), before, {"Shaman"})
        total = sum(len(v) for v in got.values())
        self.assertEqual(total, len(before))


class PairedTestCase(FixtureTestCase):
    def run_tool(self, before_rows, after_rows, *argv):
        before = self.csv_with(before_rows, name="before.csv")
        after = self.csv_with(after_rows, name="after.csv")
        return run_main(ps.main, [before, after] + list(argv))


class ControlTests(PairedTestCase):
    """The control is a bit-exactness claim, and it gates the exit code."""

    def test_an_identical_control_slice_passes_and_says_so(self):
        rows = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4) + losses(4))
        reach = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4) + losses(4))
        run = self.run_tool(rows + reach, rows + reach,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertEqual(run.code, 0, run)
        self.assertHas(run, "CONTROL: 8/8")
        self.assertHas(run, "IDENTICAL in winner and duration")

    def test_a_control_that_moves_only_in_DURATION_still_fails(self):
        """The subtle one: same winner, different clock, is still a reach."""
        before = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4),
                          durations=[10.0, 11.0, 12.0, 13.0])
        after = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4),
                         durations=[10.0, 11.0, 12.5, 13.0])
        reach = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        run = self.run_tool(before + reach, after + reach,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "CONTROL FAILED: 1 of 4")
        self.assertEqual(run.code, 1, run)

    def test_a_sweep_with_no_control_slice_says_nothing_bounds_the_diff(self):
        reach = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        run = self.run_tool(reach, reach, "--affects", "Shaman",
                            "--tier", "directional")
        self.assertHas(run, "no matches in which the change can reach NEITHER side")
        self.assertEqual(run.code, 1, run)

    def test_a_control_that_lives_elsewhere_must_be_declared_in_the_report(self):
        """AS-122's Rogue arms all fielded a Rogue; its control was elsewhere.

        The declaration is printed INTO the report a findings doc quotes,
        rather than discharged by an exit code nobody reads.
        """
        reach = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        run = self.run_tool(reach, reach, "--affects", "Shaman",
                            "--tier", "directional",
                            "--control-elsewhere", "the Hunter arm, h.csv")
        self.assertHas(run, "CONTROL: none in this pair, by declaration")
        self.assertHas(run, "the Hunter arm, h.csv")
        self.assertEqual(run.code, 0, run)

    def test_declaring_a_control_elsewhere_does_not_excuse_one_that_moved(self):
        """The flag covers an ABSENT control, never a FAILING one."""
        before = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        after = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue",
                         wins(3) + losses(1))
        reach = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        run = self.run_tool(before + reach, after + reach, "--affects", "Shaman",
                            "--tier", "directional",
                            "--control-elsewhere", "somewhere else")
        self.assertHas(run, "CONTROL FAILED")
        self.assertEqual(run.code, 1, run)


class ControlCoverageTests(PairedTestCase):
    """AS-143: an identical control is a claim only over the classes it fields.

    Before this, a control that never fielded a class printed the same PASS as
    one that fielded all of them, so a reader could not tell a discharged
    duty from an undischarged one.
    """

    # The eight controls gen_sweep.py drew for `--full 2 --exclude-double-healer
    # --affects Warlock` at the default count, before AS-143: Warrior only ever
    # an opponent, Rogue only ever on team1.
    AS143_CONTROL = [
        ("Mage+Hunter", "Warrior+Paladin"), ("Mage+Shaman", "Paladin+Hunter"),
        ("Rogue+Priest", "Warrior+Priest"), ("Rogue+Priest", "Warrior+Shaman"),
        ("Rogue+Paladin", "Warrior+Shaman"), ("Rogue+Shaman", "Priest+Hunter"),
        ("Paladin+Hunter", "Warrior+Shaman"), ("Hunter+Shaman", "Mage+Shaman"),
    ]

    def sweep(self, control, reach):
        rows = []
        for i, (t1, t2) in enumerate(control):
            rows += arm_rows("ctl%d" % i, t1, t2, wins(2) + losses(2))
        for i, (t1, t2) in enumerate(reach):
            rows += arm_rows("hit%d" % i, t1, t2, wins(2) + losses(2))
        return rows

    def test_the_as143_control_is_reported_as_a_blind_spot_not_a_pass(self):
        # Reachable cells in which Warrior plays team1 and Rogue plays the
        # opponent side, beside a Warlock: the sweep fields both there, so the
        # control could have fielded them.
        rows = self.sweep(self.AS143_CONTROL, [
            ("Warrior+Mage", "Warlock+Priest"),
            ("Warlock+Mage", "Rogue+Priest"),
        ])
        run = self.run_tool(rows, rows, "--affects", "Warlock", "--tier", "directional")
        self.assertEqual(run.code, 1, run)
        self.assertHas(run, "CONTROL FIELDS on team1: Hunter, Mage, Paladin, Priest, "
                            "Rogue, Shaman (6 of the 7 unaffected classes")
        self.assertHas(run, "CONTROL FIELDS on team2: Hunter, Mage, Paladin, Priest, "
                            "Shaman, Warrior (6 of the 7 unaffected classes")
        self.assertHas(run, "CONTROL BLIND SPOT: the control never fields Warrior on "
                            "team1; nor Rogue on team2.")
        self.assertLacks(run, "bounded by measurement")

    def test_a_control_that_fields_every_class_names_them_and_passes(self):
        rows = self.sweep([("Mage+Priest", "Warrior+Rogue")],
                          [("Shaman+Mage", "Warrior+Rogue"),
                           ("Mage+Priest", "Shaman+Warrior")])
        run = self.run_tool(rows, rows, "--affects", "Shaman", "--tier", "directional")
        self.assertEqual(run.code, 0, run)
        self.assertHas(run, "CONTROL FIELDS on team1: Mage, Priest (2 of the 2")
        self.assertHas(run, "CONTROL FIELDS on team2: Rogue, Warrior (2 of the 2")
        self.assertLacks(run, "BLIND SPOT")
        self.assertHas(run, "bounded by measurement")

    def test_an_unaffected_team_outside_the_control_is_owed_by_it(self):
        """The AGAINST cell fields Warrior+Rogue on team1 with no affected class.

        A control cell could have been built from that team, so the control
        owes it -- the case the Warlock default missed.
        """
        rows = self.sweep([("Mage+Priest", "Warrior+Rogue")],
                          [("Warrior+Rogue", "Shaman+Priest")])
        run = self.run_tool(rows, rows, "--affects", "Shaman", "--tier", "directional")
        self.assertEqual(run.code, 1, run)
        self.assertHas(run, "CONTROL FIELDS on team1: Mage, Priest (2 of the 4")
        self.assertHas(run, "never fields Rogue, Warrior on team1.")

    def test_a_class_that_only_ever_plays_beside_an_affected_one_is_not_owed(self):
        """No control could have fielded it, so its absence is not a blind spot."""
        rows = self.sweep([("Mage+Priest", "Warrior+Rogue")],
                          [("Shaman+Hunter", "Warrior+Rogue")])
        run = self.run_tool(rows, rows, "--affects", "Shaman", "--tier", "directional")
        self.assertEqual(run.code, 0, run)
        self.assertHas(run, "CONTROL FIELDS on team1: Mage, Priest (2 of the 2")
        self.assertLacks(run, "BLIND SPOT")

    def test_the_generator_and_the_report_agree_on_what_a_control_owes(self):
        """gen_sweep.py's control, read by this tool, has no blind spot.

        The two tools derive the duty independently -- the generator from the
        cells it could sample, this report from the teams the sweep fields --
        so this is the check that they are describing the same obligation.
        Every --affects class, at the default count, in the 2v2 and 1v1
        matrices (the second is where the old sampler failed 8 of 8).
        """
        import gen_sweep as gen
        import json
        install_no_subprocess(gen)
        for shape in (["--full", "2", "--exclude-double-healer"], ["--full", "1"]):
            for cls in gen.CLASSES:
                made = run_main(gen.main, shape + ["--n", "1", "--affects", cls])
                self.assertEqual(made.code, 0, made)
                rows = []
                for line in made.out.splitlines():
                    cfg = json.loads(line)
                    rows += arm_rows(cfg["label"], "+".join(cfg["team1"]),
                                     "+".join(cfg["team2"]), wins(1))
                run = self.run_tool(rows, rows, "--affects", cls,
                                    "--tier", "directional")
                self.assertEqual(run.code, 0, "%s --affects %s\n%s" % (shape, cls, run))
                self.assertLacks(run, "BLIND SPOT")
                owed = len(gen.CLASSES) - 1
                self.assertHas(run, "(%d of the %d unaffected classes" % (owed, owed))

    def test_a_blind_spot_covered_elsewhere_must_be_declared_in_the_report(self):
        rows = self.sweep(self.AS143_CONTROL, [("Warrior+Mage", "Warlock+Priest")])
        run = self.run_tool(rows, rows, "--affects", "Warlock", "--tier", "directional",
                            "--control-elsewhere", "a Warrior arm, w.csv")
        self.assertEqual(run.code, 0, run)
        self.assertHas(run, "never fields Warrior on team1.")
        self.assertHas(run, "Covered elsewhere, by declaration — a Warrior arm, w.csv")


class ReportTests(PairedTestCase):
    def test_a_delta_is_printed_with_the_floor_that_bounds_it(self):
        before = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                          wins(20) + losses(20))
        after = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                         wins(32) + losses(8))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(before + ctl, after + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "ALL reachable")
        self.assertHas(run, "delta +30.0pt")
        self.assertHas(run, "resolves >=")

    def test_a_slice_in_which_nothing_flipped_says_unresolvable(self):
        rows = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(10))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(rows + ctl, rows + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "no flips: unresolvable")

    def test_non_vacuity_counts_decisive_matches_not_rows(self):
        draws = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", ["draw"] * 6)
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(draws + ctl, draws + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "NON-VACUITY: 4/10 paired matches ended by elimination "
                            "(6 draws)")
        self.assertHas(run, "Nothing moved anywhere")

    def test_distinct_durations_are_reported_so_a_frozen_arm_is_visible(self):
        before = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4),
                          durations=[10.0, 20.0, 30.0, 40.0])
        after = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4),
                         durations=[10.0, 10.0, 10.0, 10.0])
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(2))
        run = self.run_tool(before + ctl, after + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "distinct durations 5 before / 2 after")

    def test_per_cell_is_off_by_default(self):
        rows = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(rows + ctl, rows + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertLacks(run, "Per cell")

    def test_per_cell_marks_a_small_cell_as_direction_only(self):
        rows = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(rows + ctl, rows + ctl, "--affects", "Shaman",
                            "--tier", "directional", "--per-cell")
        self.assertUnderHeading(run, "Per cell", "DIRECTION ONLY")


class MultipleComparisonTests(PairedTestCase):
    def test_the_slice_count_is_printed_whether_or_not_anything_was_significant(self):
        rows = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(rows + ctl, rows + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "SLICES TESTED:")
        self.assertHas(run, "Significant: none.")

    def test_a_significant_MIRRORED_slice_is_called_noise_unless_corroborated(self):
        """The AS-104 tally rule, applied rather than remembered."""
        before = arm_rows("mir", "Shaman+Mage", "Shaman+Rogue",
                          wins(30) + losses(70))
        after = arm_rows("mir", "Shaman+Mage", "Shaman+Rogue",
                         wins(55) + losses(45))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(before + ctl, after + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "NOISE UNLESS CORROBORATED")
        self.assertHas(run, "expected to WASH")
        self.assertHas(run, "add the row to AS-104")

    def test_one_significant_slice_is_reported_with_its_denominator(self):
        """Three disjoint slices tested, one moved: the ratio is the finding."""
        clean_b = arm_rows("cln", "Shaman+Mage", "Warrior+Rogue",
                           wins(30) + losses(70))
        clean_a = arm_rows("cln", "Shaman+Mage", "Warrior+Rogue",
                           wins(55) + losses(45))
        against = arm_rows("agt", "Warrior+Rogue", "Shaman+Mage", wins(20))
        mirror = arm_rows("mir", "Shaman+Mage", "Shaman+Rogue", wins(20))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(clean_b + against + mirror + ctl,
                            clean_a + against + mirror + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "SLICES TESTED: 3.")
        self.assertHas(run, "One significant slice out of 3 tested")

    def test_the_reachable_aggregate_is_not_counted_as_a_comparison(self):
        """Otherwise every single real effect reads as "one of two"."""
        before = arm_rows("cln", "Shaman+Mage", "Warrior+Rogue",
                          wins(30) + losses(70))
        after = arm_rows("cln", "Shaman+Mage", "Warrior+Rogue",
                         wins(55) + losses(45))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(before + ctl, after + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "SLICES TESTED: 1.")
        self.assertLacks(run, "Significant: ALL reachable")


class VerdictTests(PairedTestCase):
    def test_expect_nothing_states_what_the_run_could_have_resolved(self):
        before = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                          wins(50) + losses(50))
        after = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                         (wins(48) + losses(52)))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(before + ctl, after + ctl, "--affects", "Shaman",
                            "--tier", "directional", "--expect", "nothing")
        self.assertHas(run, "VERDICT: consistent with no movement")
        self.assertHas(run, "could have resolved a delta of")
        self.assertHas(run, "says nothing below it")

    def test_expect_nothing_calls_out_a_run_that_did_move(self):
        before = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                          wins(20) + losses(80))
        after = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                         wins(60) + losses(40))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(before + ctl, after + ctl, "--affects", "Shaman",
                            "--tier", "directional", "--expect", "nothing")
        self.assertHas(run, "CONTRADICTS its success condition")

    def test_expect_move_refuses_to_cite_an_unresolvable_magnitude(self):
        before = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                          wins(50) + losses(50))
        after = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                         wins(52) + losses(48))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(before + ctl, after + ctl, "--affects", "Shaman",
                            "--tier", "directional", "--expect", "move")
        self.assertHas(run, "NOT resolvable at this size")
        self.assertHas(run, "do not cite the magnitude")

    def test_an_inert_change_is_distinguished_from_a_small_one(self):
        rows = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(20))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(8))
        run = self.run_tool(rows + ctl, rows + ctl, "--affects", "Shaman",
                            "--tier", "directional", "--expect", "nothing")
        self.assertHas(run, "The change is inert on this sweep")

    def test_omitting_the_success_condition_is_called_out(self):
        """The card's own requirement, enforced in the report."""
        rows = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(rows + ctl, rows + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "NO SUCCESS CONDITION DECLARED")
        self.assertLacks(run, "VERDICT:")

    def test_the_directional_tier_stamps_its_own_limitation(self):
        rows = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(rows + ctl, rows + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "TIER: DIRECTIONAL")
        self.assertHas(run, "may not be cited as a class's standing")

    def test_the_authority_tier_carries_no_such_caveat(self):
        rows = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(rows + ctl, rows + ctl,
                            "--affects", "Shaman", "--tier", "authority")
        self.assertHas(run, "TIER: AUTHORITY")
        self.assertLacks(run, "may not be cited as a class's standing")


class PairingTests(PairedTestCase):
    def test_rows_with_no_twin_are_excluded_and_warned_about(self):
        before = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(6))
        after = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(before + ctl, after + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "unpaired rows=2")
        self.assertHas(run, "is not paired")

    def test_two_arms_that_share_no_seed_are_refused_outright(self):
        before = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4),
                          seed_base=0)
        after = arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4),
                         seed_base=100)
        run = self.run_tool(before, after, "--affects", "Shaman",
                            "--tier", "directional")
        self.assertIn("did not run the same batch file", str(run.code))

    def test_errored_matches_are_excluded_rather_than_scored_as_losses(self):
        rows = (arm_rows("hit", "Shaman+Mage", "Warrior+Rogue", wins(4))
                + arm_rows("hit", "Shaman+Mage", "Warrior+Rogue",
                           ["error"] * 2, seed_base=4))
        ctl = arm_rows("ctl", "Mage+Priest", "Warrior+Rogue", wins(4))
        run = self.run_tool(rows + ctl, rows + ctl,
                            "--affects", "Shaman", "--tier", "directional")
        self.assertHas(run, "errors before=2 after=2")
        self.assertHas(run, "paired matches: 8")

    def test_a_csv_that_is_not_a_batch_export_is_named_as_such(self):
        path = os.path.join(self.tmp, "wrong.csv")
        with open(path, "w", encoding="utf-8") as f:
            f.write("team1,team2,runs,team1_wins\nMage,Priest,100,51\n")
        run = run_main(ps.main, [path, path, "--affects", "Mage",
                                 "--tier", "directional"])
        self.assertIn("not a batch results CSV", str(run.code))

    def test_affects_must_name_something(self):
        rows = arm_rows("hit", "Shaman", "Mage", wins(2))
        run = self.run_tool(rows, rows, "--affects", " , ",
                            "--tier", "directional")
        self.assertIn("named no classes", str(run.code))


class SourceTests(unittest.TestCase):
    def test_the_tool_imports_on_the_stock_system_interpreter(self):
        assert_runs_on_min_python(self, ps)


if __name__ == "__main__":
    unittest.main(verbosity=2)
