#!/usr/bin/env python3
"""Offline fixture tests for `scripts/headtohead_sweep.py`.

This is the tool balance decisions are read off. Its Wilson intervals and
z-tests are what turn a pile of match outcomes into "this cell MOVED" -- and a
reporting bug in it is invisible in exactly the way AS-49's two bugs were
invisible: the output looks like an answer, and nobody re-derives it by hand.
The record is not hypothetical. A Tester once had to read `agg_sweep.py:135`
from source to confirm what `--compare` actually flags, because nothing pinned
it; and the AS-44 hand check that per-cell win counts sum to the slice total was
performed with a calculator.

So the statistics are pinned against EXTERNAL values, never against what this
codebase computes:

* the Wilson intervals come from Newcombe (1998), "Two-sided confidence
  intervals for the single proportion: comparison of seven methods",
  *Statistics in Medicine* 17:857-872 -- his four worked examples 81/263,
  15/148, 0/20 and 1/29, whose method-3 (Wilson score) intervals are the
  numbers asserted below;
* the two-proportion z values are worked by hand in the comment above each
  assertion, from the pooled textbook form.

The end-to-end cases drive `main(argv)` with the batch runner replaced by a
fixture that fabricates match outcomes, so a full sweep's reporting path runs
in milliseconds with no `cargo`, no matches and no network. An unexpected
process launch FAILS the test rather than quietly shelling out.

Run directly, or via `cargo test --test script_fixture_suites`:

    python3 scripts/tests/test_headtohead_sweep.py
"""

from __future__ import annotations

import csv
import json
import os
import random
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from _harness import assert_runs_on_min_python, install_no_subprocess, run_main  # noqa: E402
from sweep_fixtures import BATCH_COLUMNS, FixtureTestCase, read_batch_jsonl  # noqa: E402

import headtohead_sweep as h2h  # noqa: E402

# Newcombe's z. He uses the exact 97.5th normal quantile; the script defaults to
# the rounded 1.96, so the published cases pass the exact value explicitly.
Z_EXACT = 1.959963985


class FakeBatchRunner:
    """Stands in for `subprocess`, fabricating the batch runner's output.

    Reads the JSONL the script just wrote and emits the per-match CSV the
    script will read back, assigning winners from a per-label plan. Every
    config in the JSONL becomes exactly one CSV row, so the fixture cannot
    invent or lose matches behind the script's back.
    """

    # The per-seed spelling of a plan: one character per seed, in seed order.
    SEED_WINNERS = {"1": "team1", "2": "team2", "d": "draw", "e": "error"}

    def __init__(self, plan):
        # plan: label -> (team1 wins, team2 wins, draws, errors), consumed in
        # the order the script wrote that label's configs; or label -> a
        # string with one character per seed ("1", "2", "d", "e"), for the
        # paired cases that need to say which seed a result lands on.
        self.plan = plan
        self.calls = []

    def run(self, cmd, **kwargs):
        self.calls.append(cmd)
        jsonl = cmd[cmd.index("--batch") + 1]
        out = cmd[cmd.index("--out") + 1]
        remaining = {label: list(counts) for label, counts in self.plan.items()}
        with open(out, "w", newline="", encoding="utf-8") as f:
            w = csv.DictWriter(f, fieldnames=BATCH_COLUMNS)
            w.writeheader()
            for cfg in read_batch_jsonl(jsonl):
                label = cfg["label"]
                counts = remaining.setdefault(label, [0, 0, 0, 0])
                if isinstance(self.plan.get(label), str):
                    assert counts, "plan for %r ran out of seeds" % label
                    winner = self.SEED_WINNERS[counts.pop(0)]
                else:
                    for idx, winner in enumerate(("team1", "team2", "draw", "error")):
                        if counts[idx] > 0:
                            counts[idx] -= 1
                            break
                    else:  # plan exhausted: anything left is a draw
                        winner = "draw"
                w.writerow(
                    {
                        "label": label,
                        "team1": "+".join(cfg["team1"]),
                        "team2": "+".join(cfg["team2"]),
                        "seed": str(cfg["random_seed"]),
                        "winner": winner,
                        "end_reason": "fixture",
                        "duration_secs": "42.0",
                    }
                )
        for label, counts in remaining.items():
            assert counts in ([0, 0, 0, 0], []), (
                "plan for %r had %r outcomes left over -- more outcomes than "
                "the script generated matches" % (label, counts)
            )
        return None


# ---------------------------------------------------------------------------
# the offline guarantee
# ---------------------------------------------------------------------------


class NoNetworkTests(FixtureTestCase):
    def test_an_unfaked_run_fails_rather_than_shelling_out(self):
        """The offline property belongs to the harness, so pin it."""
        install_no_subprocess(h2h)
        try:
            with self.assertRaises(AssertionError) as caught:
                run_main(h2h.main, ["--team1", "Warrior", "--team2", "Mage", "--seeds", "2"])
            self.assertIn("offline by construction", str(caught.exception))
        finally:
            h2h.subprocess = None


# ---------------------------------------------------------------------------
# Wilson intervals -- Newcombe (1998) worked examples
# ---------------------------------------------------------------------------


class WilsonTests(unittest.TestCase):
    def assertInterval(self, got, lo, hi, places=4):
        self.assertAlmostEqual(got[0], lo, places=places)
        self.assertAlmostEqual(got[1], hi, places=places)

    def test_newcombe_81_of_263(self):
        self.assertInterval(h2h.wilson(81, 263, Z_EXACT), 0.2553, 0.3662)

    def test_newcombe_15_of_148(self):
        self.assertInterval(h2h.wilson(15, 148, Z_EXACT), 0.0624, 0.1605)

    def test_newcombe_0_of_20(self):
        """All losses. Published bound is 0.0000-0.1611."""
        lo, hi = h2h.wilson(0, 20, Z_EXACT)
        self.assertInterval((lo, hi), 0.0000, 0.1611)
        # And exactly zero, not the -1e-17 the closed form rounds to: a
        # reported "-0.0%" lower bound is nonsense on a win rate.
        self.assertEqual(lo, 0.0)
        self.assertFalse(str(lo).startswith("-"))

    def test_newcombe_1_of_29(self):
        self.assertInterval(h2h.wilson(1, 29, Z_EXACT), 0.0061, 0.1718)

    def test_all_wins_mirrors_all_losses(self):
        """Wilson is symmetric under k -> n-k, so 20/20 is 0/20 reflected."""
        lo0, hi0 = h2h.wilson(0, 20, Z_EXACT)
        lo1, hi1 = h2h.wilson(20, 20, Z_EXACT)
        self.assertAlmostEqual(lo1, 1.0 - hi0, places=12)
        self.assertAlmostEqual(hi1, 1.0 - lo0, places=12)
        self.assertEqual(hi1, 1.0)

    def test_a_single_win_is_not_certainty(self):
        """1/1 is 20.7%-100%: a lower bound, not a measurement.

        Worked from the closed form at z=1.96, p=1, n=1:
          denominator 1 + 1.96^2 = 4.8416; centre numerator 1 + 1.96^2/2 =
          2.9208; half numerator 1.96*sqrt(1.96^2/4) = 1.9208. So
          lo = 1.0000/4.8416 = 0.206543 and hi = 4.8416/4.8416 = 1.
        The point of pinning it is the width: one match cannot rule out a
        coin flip, and this is the sample size a balance claim gets rejected
        for.
        """
        lo, hi = h2h.wilson(1, 1)
        self.assertAlmostEqual(lo, 0.206543, places=6)
        self.assertEqual(hi, 1.0)
        # and its mirror
        lo0, hi0 = h2h.wilson(0, 1)
        self.assertEqual(lo0, 0.0)
        self.assertAlmostEqual(hi0, 0.793457, places=6)

    def test_zero_matches_is_no_information_not_zero_percent(self):
        """n=0 carries no information, so the interval is the whole range.

        Reporting (0, 0) here would make an empty cell read as a confident 0%
        win rate -- the failure mode this suite exists to prevent.
        """
        self.assertEqual(h2h.wilson(0, 0), (0.0, 1.0))

    def test_interval_never_leaves_the_unit_range(self):
        for k, n in [(0, 1), (1, 1), (0, 3), (3, 3), (1, 2), (99, 100), (0, 1000)]:
            lo, hi = h2h.wilson(k, n)
            self.assertGreaterEqual(lo, 0.0, "lo out of range for %d/%d" % (k, n))
            self.assertLessEqual(hi, 1.0, "hi out of range for %d/%d" % (k, n))
            self.assertLessEqual(lo, hi)


# ---------------------------------------------------------------------------
# the two-proportion z-test
# ---------------------------------------------------------------------------


class ZTestTests(unittest.TestCase):
    def test_no_difference_is_zero(self):
        self.assertEqual(h2h.ztest(50, 100, 50, 100), 0.0)

    def test_known_value_equal_arms(self):
        """50/100 vs 70/100.

        Pooled p = 120/200 = 0.6; SE = sqrt(0.6*0.4*(1/100+1/100)) =
        sqrt(0.0048) = 0.0692820; z = 0.20/0.0692820 = 2.8868.
        """
        self.assertAlmostEqual(h2h.ztest(50, 100, 70, 100), 2.8868, places=4)

    def test_sign_follows_the_second_arm(self):
        self.assertAlmostEqual(h2h.ztest(70, 100, 50, 100), -2.8868, places=4)

    def test_known_value_unequal_arms(self):
        """50/100 vs 35/50 -- the case an equal-n formula gets wrong.

        Pooled p = 85/150 = 0.5666667; SE = sqrt(0.5666667*0.4333333*(1/100 +
        1/50)) = sqrt(0.00736667) = 0.0858293; z = 0.20/0.0858293 = 2.3302.
        """
        self.assertAlmostEqual(h2h.ztest(50, 100, 35, 50), 2.3302, places=4)

    def test_the_conventional_bar(self):
        """The 95% bar the script prints: |z| >= 1.96. Pin both sides of it.

        60 vs 74 of 100: pooled 0.67, SE = sqrt(0.67*0.33*0.02) = 0.0665,
        z = 0.14/0.0665 = 2.1053 -- over the bar.
        60 vs 72 of 100: pooled 0.66, SE = sqrt(0.66*0.34*0.02) = 0.0670,
        z = 0.12/0.0670 = 1.7915 -- under it.
        """
        self.assertGreater(h2h.ztest(60, 100, 74, 100), 1.96)
        self.assertLess(h2h.ztest(60, 100, 72, 100), 1.96)

    def test_degenerate_no_variance(self):
        """Every match won by the same side in both arms: no evidence, z=0."""
        self.assertEqual(h2h.ztest(100, 100, 100, 100), 0.0)
        self.assertEqual(h2h.ztest(0, 100, 0, 100), 0.0)

    def test_empty_arm_is_not_a_result(self):
        """An arm with no usable matches must not divide by zero or claim z."""
        self.assertEqual(h2h.ztest(0, 0, 5, 10), 0.0)


# ---------------------------------------------------------------------------
# end to end, through main(argv)
# ---------------------------------------------------------------------------


class SweepTestCase(FixtureTestCase):
    def sweep(self, plan, *argv, seeds=10, keep=True):
        runner = FakeBatchRunner(plan)
        h2h.subprocess = runner
        args = [
            "--team1", "Warrior,Priest",
            "--team2", "Warlock,Priest",
            "--seeds", str(seeds),
            *argv,
        ]
        if keep:
            args += ["--keep", self.path("sweep")]
        run = run_main(h2h.main, args)
        run.runner = runner
        return run

    def tearDown(self):
        h2h.subprocess = None


class CellConstructionTests(SweepTestCase):
    """The head-to-head design itself: three cells, profiles set per team."""

    def test_three_cells_with_per_team_profiles(self):
        self.sweep({"LL": (5, 5, 0, 0), "TL": (7, 3, 0, 0), "LT": (4, 6, 0, 0)}, seeds=10)
        cfgs = read_batch_jsonl(self.path("sweep.jsonl"))
        by_label = {}
        for c in cfgs:
            by_label.setdefault(c["label"], set()).add(
                (c["team1_ai_profile"], c["team2_ai_profile"])
            )
        self.assertEqual(
            by_label,
            {
                "LL": {("Legacy", "Legacy")},
                "TL": {("TeamPlan", "Legacy")},
                "LT": {("Legacy", "TeamPlan")},
            },
        )

    def test_profile_under_test_is_configurable(self):
        self.sweep(
            {"LL": (5, 5, 0, 0), "TL": (5, 5, 0, 0), "LT": (5, 5, 0, 0)},
            "--profile", "Experimental",
            seeds=10,
        )
        cfgs = read_batch_jsonl(self.path("sweep.jsonl"))
        profiles = {(c["team1_ai_profile"], c["team2_ai_profile"]) for c in cfgs}
        self.assertIn(("Experimental", "Legacy"), profiles)
        self.assertIn(("Legacy", "Experimental"), profiles)

    def test_every_cell_runs_the_same_seeds(self):
        """A paired comparison: the AI is the only variable across cells."""
        self.sweep({"LL": (5, 5, 0, 0), "TL": (5, 5, 0, 0), "LT": (5, 5, 0, 0)}, seeds=10)
        seeds = {}
        for c in read_batch_jsonl(self.path("sweep.jsonl")):
            seeds.setdefault(c["label"], []).append(c["random_seed"])
        self.assertEqual(sorted(seeds), ["LL", "LT", "TL"])
        self.assertEqual(seeds["LL"], seeds["TL"])
        self.assertEqual(seeds["LL"], seeds["LT"])
        self.assertEqual(sorted(seeds["LL"]), list(range(1, 11)))

    def test_seed_base_shifts_the_whole_sweep(self):
        self.sweep(
            {"LL": (2, 0, 0, 0), "TL": (2, 0, 0, 0), "LT": (2, 0, 0, 0)},
            "--seed-base", "500",
            seeds=2,
        )
        cfgs = read_batch_jsonl(self.path("sweep.jsonl"))
        self.assertEqual(sorted({c["random_seed"] for c in cfgs}), [500, 501])

    def test_map_and_cap_reach_every_config(self):
        self.sweep(
            {"LL": (1, 0, 0, 0), "TL": (1, 0, 0, 0), "LT": (1, 0, 0, 0)},
            "--map", "TwinPillars", "--max-duration", "120",
            seeds=1,
        )
        for c in read_batch_jsonl(self.path("sweep.jsonl")):
            self.assertEqual(c["map"], "TwinPillars")
            self.assertEqual(c["max_duration_secs"], 120.0)
            self.assertEqual(c["team1"], ["Warrior", "Priest"])
            self.assertEqual(c["team2"], ["Warlock", "Priest"])


class TallyTests(SweepTestCase):
    """The check AS-44's Tester did by hand: counts sum to the slice total."""

    def _table(self, run):
        """Parse the per-cell table back out of stdout."""
        rows = {}
        for line in run.out.splitlines():
            parts = line.split()
            if parts and parts[0] in ("LL", "TL", "LT"):
                rows[parts[0]] = [int(p) for p in parts[1:5]]
        return rows

    def test_per_cell_counts_sum_to_the_cell_total(self):
        run = self.sweep(
            {"LL": (5, 3, 2, 0), "TL": (7, 2, 1, 0), "LT": (4, 4, 2, 0)}, seeds=10
        )
        self.assertOk(run)
        table = self._table(run)
        self.assertEqual(set(table), {"LL", "TL", "LT"})
        for label, (n, t1, t2, draw) in table.items():
            self.assertEqual(n, 10, "cell %s lost or invented matches" % label)
            self.assertEqual(
                t1 + t2 + draw, n, "cell %s: %d+%d+%d != %d" % (label, t1, t2, draw, n)
            )
        self.assertEqual(table["LL"][1:], [5, 3, 2])
        self.assertEqual(table["TL"][1:], [7, 2, 1])

    def test_draws_are_not_credited_to_either_side(self):
        run = self.sweep(
            {"LL": (5, 0, 5, 0), "TL": (5, 0, 5, 0), "LT": (5, 0, 5, 0)}, seeds=10
        )
        # 5 wins out of 10 played, NOT 5 out of the 5 decisive matches.
        self.assertHas(run, "50%")
        self.assertEqual(self._table(run)["LL"], [10, 5, 0, 5])

    def test_the_sum_holds_when_matches_error(self):
        """Errored matches leave the tally, so the remaining counts still sum."""
        run = self.sweep(
            {"LL": (5, 5, 0, 0), "TL": (6, 2, 0, 2), "LT": (5, 5, 0, 0)}, seeds=10
        )
        self.assertOk(run)
        table = self._table(run)
        self.assertEqual(table["TL"], [8, 6, 2, 0])
        self.assertEqual(sum(table["TL"][1:]), table["TL"][0])
        self.assertErrHas(run, "2 match(es) errored")


class EffectReportingTests(SweepTestCase):
    def test_gain_is_the_paired_difference(self):
        run = self.sweep(
            {"LL": (50, 50, 0, 0), "TL": (62, 38, 0, 0), "LT": (44, 56, 0, 0)}, seeds=100
        )
        self.assertOk(run)
        # team 1: 50% -> 62% = +12pt. team 2: 50% -> 56% = +6pt.
        self.assertHas(run, "+12pt")
        self.assertHas(run, "+6pt")

    def test_a_loss_is_reported_as_a_loss(self):
        run = self.sweep(
            {"LL": (50, 50, 0, 0), "TL": (38, 62, 0, 0), "LT": (50, 50, 0, 0)}, seeds=100
        )
        self.assertHas(run, "-12pt")

    def test_errors_are_excluded_from_the_effect_denominator(self):
        """The bug this case was written for.

        The header warns that errored matches are "excluded from all rates".
        With 10 of 100 TL matches errored, team 1's TeamPlan arm is 62 wins out
        of the 90 that ran = 68.9%, against a 50.0% baseline: +19pt. Dividing by
        the nominal 100 seeds instead reports +12pt -- a third of the effect
        silently eaten by matches that never happened.
        """
        run = self.sweep(
            {"LL": (50, 50, 0, 0), "TL": (62, 28, 0, 10), "LT": (50, 50, 0, 0)}, seeds=100
        )
        self.assertOk(run)
        self.assertHas(run, "+19pt")
        self.assertLacks(run, "+12pt")

    def test_z_survives_unequal_arms(self):
        """Same setup: z must use each arm's own n, not the nominal seeds."""
        run = self.sweep(
            {"LL": (50, 50, 0, 0), "TL": (62, 28, 0, 10), "LT": (50, 50, 0, 0)}, seeds=100
        )
        # 50/100 vs 62/90. Pooled p = 112/190 = 0.5894737; SE =
        # sqrt(0.5894737*0.4105263*(1/100 + 1/90)) = sqrt(0.00510938) =
        # 0.0714799; z = 0.1888889/0.0714799 = 2.6425.
        self.assertHas(run, "z=+2.64")
        # The equal-n form on the nominal 100 seeds says z=+1.71 -- under the
        # 1.96 bar, i.e. the opposite verdict from the same matches.
        self.assertLacks(run, "z=+1.71")

    def test_a_clean_run_reports_the_conventional_z(self):
        run = self.sweep(
            {"LL": (50, 100 - 50, 0, 0), "TL": (70, 30, 0, 0), "LT": (50, 50, 0, 0)},
            seeds=100,
        )
        # The hand-worked 50/100 vs 70/100 case: z = 2.8868.
        self.assertHas(run, "z=+2.89")

    def test_one_match_per_cell_is_not_a_finding(self):
        """n=1 must widen the interval, not read as a confident 0/100%."""
        run = self.sweep(
            {"LL": (0, 1, 0, 0), "TL": (1, 0, 0, 0), "LT": (0, 1, 0, 0)}, seeds=1
        )
        self.assertOk(run)
        self.assertLacks(run, "100.0-100.0")
        self.assertLacks(run, "0.0-0.0")
        self.assertHas(run, "20.7-100.0")  # the 1/1 Wilson interval
        self.assertHas(run, "0.0-79.3")  # and the 0/1 one


class DegenerateInputTests(SweepTestCase):
    def test_a_cell_with_no_usable_matches_bails(self):
        """An empty cell must stop the report, not read as a 0% win rate."""
        run = self.sweep(
            {"LL": (5, 5, 0, 0), "TL": (0, 0, 0, 10), "LT": (5, 5, 0, 0)}, seeds=10
        )
        self.assertIsInstance(run.code, str)
        self.assertIn("TL", run.code)
        self.assertIn("cannot analyze", run.code)
        self.assertLacks(run, "0%")

    def test_all_wins_and_all_losses(self):
        run = self.sweep(
            {"LL": (0, 20, 0, 0), "TL": (20, 0, 0, 0), "LT": (0, 20, 0, 0)}, seeds=20
        )
        self.assertOk(run)
        # 0/20 and 20/20 are Newcombe's bounds reflected: 0.0-16.1 and 83.9-100.
        self.assertHas(run, "0.0-16.1")
        self.assertHas(run, "83.9-100.0")
        self.assertHas(run, "+100pt")

    def test_every_match_a_draw(self):
        """No wins anywhere is a legitimate (if useless) sweep, not a crash."""
        run = self.sweep(
            {"LL": (0, 0, 10, 0), "TL": (0, 0, 10, 0), "LT": (0, 0, 10, 0)}, seeds=10
        )
        self.assertOk(run)
        self.assertHas(run, "+0pt")
        self.assertHas(run, "z=+0.00")


class ArtifactTests(SweepTestCase):
    def test_keep_writes_both_artifacts(self):
        self.sweep({"LL": (1, 0, 0, 0), "TL": (1, 0, 0, 0), "LT": (1, 0, 0, 0)}, seeds=1)
        self.assertTrue(os.path.isfile(self.path("sweep.jsonl")))
        self.assertTrue(os.path.isfile(self.path("sweep.csv")))

    def test_the_runner_is_invoked_once_over_the_whole_sweep(self):
        run = self.sweep(
            {"LL": (2, 0, 0, 0), "TL": (2, 0, 0, 0), "LT": (2, 0, 0, 0)}, seeds=2
        )
        self.assertEqual(len(run.runner.calls), 1)
        cmd = run.runner.calls[0]
        self.assertIn("--batch", cmd)
        self.assertIn("--out", cmd)

    def test_without_keep_nothing_lands_in_the_repo(self):
        run = self.sweep(
            {"LL": (1, 0, 0, 0), "TL": (1, 0, 0, 0), "LT": (1, 0, 0, 0)},
            seeds=1,
            keep=False,
        )
        self.assertOk(run)
        self.assertErrHas(run, "artifacts in")
        for name in os.listdir(os.getcwd()):
            self.assertFalse(name.startswith("sweep."), "leaked %s into cwd" % name)


# ---------------------------------------------------------------------------
# McNemar and the cluster bootstrap
# ---------------------------------------------------------------------------


class McNemarTests(unittest.TestCase):
    def test_known_values(self):
        """Continuity-corrected, (|b - c| - 1)/sqrt(b + c).

        12 vs 0: 11/sqrt(12) = 11/3.4641016 = 3.1754.
        6 vs 0: 5/sqrt(6) = 5/2.4494897 = 2.0412.
        30 vs 10: 19/sqrt(40) = 19/6.3245553 = 3.0042.
        """
        self.assertAlmostEqual(h2h.mcnemar(12, 0), 3.1754, places=4)
        self.assertAlmostEqual(h2h.mcnemar(6, 0), 2.0412, places=4)
        self.assertAlmostEqual(h2h.mcnemar(30, 10), 3.0042, places=4)

    def test_unsigned_so_the_counts_carry_the_direction(self):
        self.assertEqual(h2h.mcnemar(0, 12), h2h.mcnemar(12, 0))

    def test_nothing_flipped_and_a_balanced_split_are_null(self):
        self.assertEqual(h2h.mcnemar(0, 0), 0.0)
        self.assertEqual(h2h.mcnemar(1, 1), 0.0)
        self.assertEqual(h2h.mcnemar(1, 0), 0.0)


class BootstrapTests(unittest.TestCase):
    def test_a_constant_column_has_no_spread(self):
        (lo, hi), = h2h.bootstrap_mean_ci([[0.5] * 30], 500, random.Random(0))
        self.assertEqual((lo, hi), (0.5, 0.5))

    def test_a_coin_matches_normal_theory(self):
        """Half ones over n=100: SE = sqrt(0.25/100) = 0.05, so +/-0.098.

        The percentile bootstrap of a mean converges on the normal interval
        at this n; 20000 resamples put its Monte Carlo error near 0.002.
        """
        col = [1.0] * 50 + [0.0] * 50
        (lo, hi), = h2h.bootstrap_mean_ci([col], 20000, random.Random(1))
        self.assertAlmostEqual(lo, 0.5 - 0.098, delta=0.01)
        self.assertAlmostEqual(hi, 0.5 + 0.098, delta=0.01)

    def test_columns_share_each_draw(self):
        """Identical columns get identical intervals: a cluster moves whole."""
        col = [1.0] * 13 + [0.0] * 27
        a, b = h2h.bootstrap_mean_ci([col, list(col)], 2000, random.Random(2))
        self.assertEqual(a, b)

    def test_the_resampling_seed_reproduces(self):
        col = [1.0, 0.0, 0.5, -1.0, 0.0, 1.0] * 5
        self.assertEqual(
            h2h.bootstrap_mean_ci([col], 1000, random.Random(7)),
            h2h.bootstrap_mean_ci([col], 1000, random.Random(7)),
        )


# ---------------------------------------------------------------------------
# the paired report, through main(argv)
# ---------------------------------------------------------------------------

GEAR = '[{"Trinket2": "DragonspineTrophy"}]'
OTHER_GEAR = '[{"Trinket2": "MarkOfTheChampion"}]'


class GearSweepTestCase(SweepTestCase):
    def gear(self, plan, *argv, seeds=10):
        return self.sweep(
            plan,
            "--team1-equipment", GEAR,
            "--team2-equipment", GEAR,
            "--bootstrap", "2000",
            *argv,
            seeds=seeds,
        )


class PairedReportTests(GearSweepTestCase):
    def test_the_worked_ten_seed_case(self):
        """Seed by seed, worked by hand.

        BB: team 1 wins seeds 1-5, team 2 seeds 6-10.
        GB: team 1 wins seeds 1-8 -> team 1's seat flips to a win on 6, 7, 8.
        BG: team 2 wins seeds 3-10 -> team 2's seat flips to a win on 3, 4, 5.
        The gear wins 8 + 8 = 16 of its 20 seats; the baseline won 5 + 5 = 10
        of the same seats. Effect (16 - 10)/20 = +30pt. Six seeds net for the
        gear (3-8), none against, four even: McNemar 5/sqrt(6) = 2.04.
        """
        run = self.gear({"BB": "1111122222", "GB": "1111111122", "BG": "1122222222"})
        self.assertOk(run)
        self.assertHas(run, "the gear 16/20 = 80.0%")
        self.assertHas(run, "baseline at the same seats 10/20 = 50.0%")
        self.assertHas(run, "effect: +30.0pt")
        self.assertUnderHeading(run, "PAIRED", "on team 1: +30.0pt")
        self.assertUnderHeading(run, "PAIRED", "on team 2: +30.0pt")
        self.assertHas(run, "discordant seeds 3 to a win, 0 to a loss; McNemar z=1.15")
        self.assertHas(run, "net for the gear 6, against 0, even 4; McNemar z=2.04")

    def test_opposite_flips_within_a_seed_cancel(self):
        """The seed is the unit: a win on one seat and a loss on the other is even.

        BB: team 1 wins both seeds. GB: team 2 wins (the gear's seat on team 1
        LOSES). BG: team 2 wins (the gear's seat on team 2 WINS). Per seat the
        flips are 0/2 and 2/0, but every seed nets to zero.
        """
        run = self.gear({"BB": "11", "GB": "22", "BG": "22"}, seeds=2)
        self.assertOk(run)
        self.assertHas(run, "effect: +0.0pt")
        self.assertHas(run, "on team 1: -100.0pt")
        self.assertHas(run, "on team 2: +100.0pt")
        self.assertHas(run, "net for the gear 0, against 0, even 2; McNemar z=0.00")

    def test_the_interval_is_clustered_by_seed(self):
        """Seats that move together must not buy a narrower interval.

        BB is all draws; on seeds 1-20 the gear wins from BOTH seats, on 21-40
        from neither. Each seed's effect equals each seat's, so the combined
        interval is each seat's interval exactly. Resampling the 80 seat
        results as independent matches would narrow it by about sqrt(2) --
        the understatement the card this option was written for warned of.
        """
        run = self.gear(
            {"BB": "d" * 40, "GB": "1" * 20 + "2" * 20, "BG": "2" * 20 + "1" * 20},
            seeds=40,
        )
        self.assertOk(run)
        lines = run.out.splitlines()
        effect = next(ln for ln in lines if ln.startswith("effect:"))
        seat1 = next(ln for ln in lines if "on team 1:" in ln)
        ci = effect[effect.index("95% CI") : effect.index("]") + 1]
        self.assertIn("+50.0pt", effect)
        self.assertIn(ci, seat1)
        lo, hi = (float(x.strip().rstrip("pt")) for x in ci[len("95% CI [") : -1].split(","))
        # The seed-level interval of 20/40 is about +/-15.5pt (SE 7.9pt); a
        # match-level one would be about +/-11pt.
        self.assertGreater(hi - lo, 27.0)

    def test_draws_are_not_wins_for_either_seat(self):
        run = self.gear({"BB": "dddd", "GB": "1ddd", "BG": "dd2d"}, seeds=4)
        self.assertOk(run)
        self.assertHas(run, "baseline at the same seats 0/8 = 0.0%")
        self.assertHas(run, "the gear 2/8 = 25.0%")
        self.assertHas(run, "effect: +25.0pt")

    def test_a_seed_with_an_errored_match_leaves_the_pairing(self):
        run = self.gear({"BB": "1111", "GB": "11e1", "BG": "2222"}, seeds=4)
        self.assertOk(run)
        self.assertHas(run, "PAIRED over the 3 seed(s)")
        self.assertHas(run, "1 seed(s) dropped")
        self.assertHas(run, "the gear 6/6 = 100.0%")

    def test_the_single_arm_figures_are_labelled_as_such(self):
        run = self.gear({"BB": "1111122222", "GB": "1111111122", "BG": "1122222222"})
        self.assertHas(run, "single-arm win rates (comparison only")
        self.assertHas(run, "single-arm (unpaired z; comparison only")

    def test_a_small_run_says_its_interval_is_too_narrow(self):
        small = self.gear({"BB": "1" * 19, "GB": "1" * 19, "BG": "1" * 19}, seeds=19)
        self.assertHas(small, "fewer than 20 seeds")
        enough = self.gear({"BB": "1" * 20, "GB": "1" * 20, "BG": "1" * 20}, seeds=20)
        self.assertLacks(enough, "fewer than 20 seeds")

    def test_the_report_reproduces(self):
        plan = {"BB": "1212121212", "GB": "1112121221", "BG": "2212122112"}
        self.assertEqual(self.gear(plan).out, self.gear(plan).out)

    def test_a_profile_sweep_gets_the_paired_report_too(self):
        run = self.sweep({"LL": "1111122222", "TL": "1111111122", "LT": "1122222222"})
        self.assertOk(run)
        self.assertHas(run, "TeamPlan's effect, PAIRED over the 10 seed(s)")
        self.assertHas(run, "net for TeamPlan 6, against 0, even 4; McNemar z=2.04")


# ---------------------------------------------------------------------------
# the equipment head-to-head's cells
# ---------------------------------------------------------------------------

EVEN = {"BB": (5, 5, 0, 0), "GB": (5, 5, 0, 0), "BG": (5, 5, 0, 0)}


class EquipmentCellTests(GearSweepTestCase):
    def cells(self):
        by_label = {}
        for c in read_batch_jsonl(self.path("sweep.jsonl")):
            by_label.setdefault(c["label"], []).append(c)
        return by_label

    def test_three_cells_with_gear_set_per_team(self):
        self.assertOk(self.gear(EVEN))
        gear = [{"Trinket2": "DragonspineTrophy"}]
        seen = {
            label: {
                (json.dumps(c.get("team1_equipment")), json.dumps(c.get("team2_equipment")))
                for c in cfgs
            }
            for label, cfgs in self.cells().items()
        }
        self.assertEqual(
            seen,
            {
                "BB": {("null", "null")},
                "GB": {(json.dumps(gear), "null")},
                "BG": {("null", json.dumps(gear))},
            },
        )

    def test_the_ai_is_not_a_variable(self):
        """No per-team profile anywhere, and none at all unless one is named."""
        self.gear(EVEN)
        for cfgs in self.cells().values():
            for c in cfgs:
                self.assertNotIn("team1_ai_profile", c)
                self.assertNotIn("team2_ai_profile", c)
                self.assertNotIn("ai_profile", c)

    def test_ai_profile_reaches_every_config_uniformly(self):
        self.assertOk(self.gear(EVEN, "--ai-profile", "TeamPlan"))
        for cfgs in self.cells().values():
            for c in cfgs:
                self.assertEqual(c["ai_profile"], "TeamPlan")
                self.assertNotIn("team1_ai_profile", c)

    def test_baseline_gear_sits_on_the_baseline_side_of_every_cell(self):
        self.assertOk(
            self.gear(
                EVEN,
                "--base-team1-equipment", OTHER_GEAR,
                "--base-team2-equipment", OTHER_GEAR,
            )
        )
        gear = [{"Trinket2": "DragonspineTrophy"}]
        base = [{"Trinket2": "MarkOfTheChampion"}]
        for label, (want1, want2) in {
            "BB": (base, base),
            "GB": (gear, base),
            "BG": (base, gear),
        }.items():
            for c in self.cells()[label]:
                self.assertEqual((c["team1_equipment"], c["team2_equipment"]), (want1, want2))

    def test_every_cell_runs_the_same_seeds(self):
        self.gear(EVEN)
        seeds = {label: [c["random_seed"] for c in cfgs] for label, cfgs in self.cells().items()}
        self.assertEqual(sorted(seeds), ["BB", "BG", "GB"])
        self.assertEqual(seeds["BB"], list(range(1, 11)))
        self.assertEqual(seeds["GB"], seeds["BB"])
        self.assertEqual(seeds["BG"], seeds["BB"])

    def test_the_report_names_the_gear_cells(self):
        run = self.gear({"BB": (5, 3, 2, 0), "GB": (7, 2, 1, 0), "BG": (4, 4, 2, 0)})
        self.assertOk(run)
        rows = {ln.split()[0]: ln.split()[1:5] for ln in run.out.splitlines()
                if ln.split() and ln.split()[0] in ("BB", "GB", "BG")}
        self.assertEqual(rows["BB"], ["10", "5", "3", "2"])
        self.assertEqual(rows["GB"], ["10", "7", "2", "1"])
        self.assertEqual(rows["BG"], ["10", "4", "4", "2"])
        self.assertHas(run, "the gear's effect")
        self.assertErrHas(run, "gear vs default loadouts")

    def test_a_profile_sweep_carries_no_equipment(self):
        self.sweep({"LL": (5, 5, 0, 0), "TL": (5, 5, 0, 0), "LT": (5, 5, 0, 0)}, seeds=10)
        for c in read_batch_jsonl(self.path("sweep.jsonl")):
            self.assertNotIn("team1_equipment", c)
            self.assertNotIn("team2_equipment", c)
            self.assertNotIn("ai_profile", c)


class EquipmentRefusalTests(SweepTestCase):
    """Each refusal stops the run before a single match is launched."""

    def refused(self, *argv):
        run = self.sweep(EVEN, *argv)
        self.assertIsInstance(run.code, str, "expected a refusal, got %r" % (run,))
        self.assertEqual(run.runner.calls, [])
        return run.code

    def test_a_profile_under_test_and_gear_together(self):
        msg = self.refused(
            "--profile", "TeamPlan", "--team1-equipment", GEAR, "--team2-equipment", GEAR
        )
        self.assertIn("--ai-profile", msg)

    def test_gear_on_one_seat_only(self):
        self.assertIn("both --team1-equipment and --team2-equipment",
                      self.refused("--team1-equipment", GEAR))
        self.assertIn("both", self.refused("--base-team1-equipment", GEAR))

    def test_gear_under_test_equal_to_the_baseline(self):
        msg = self.refused(
            "--team1-equipment", GEAR, "--team2-equipment", GEAR,
            "--base-team2-equipment", GEAR,
        )
        self.assertIn("team 2's gear under test is its baseline", msg)
        # An empty override is the default loadout, so it measures nothing too.
        msg = self.refused("--team1-equipment", "[{}, {}]", "--team2-equipment", GEAR)
        self.assertIn("team 1's gear under test is its baseline", msg)

    def test_malformed_gear(self):
        self.assertIn("not valid JSON",
                      self.refused("--team1-equipment", "[{", "--team2-equipment", GEAR))
        self.assertIn("expected a JSON list",
                      self.refused("--team1-equipment", '{"Trinket2": "X"}',
                                   "--team2-equipment", GEAR))
        self.assertIn("slot 0 must be an object",
                      self.refused("--team1-equipment", '[{"Trinket2": 3}]',
                                   "--team2-equipment", GEAR))
        self.assertIn("only 2 members",
                      self.refused("--team1-equipment", "[{}, {}, {}]",
                                   "--team2-equipment", GEAR))

    def test_ai_profile_without_gear(self):
        self.assertIn("--profile", self.refused("--ai-profile", "TeamPlan"))


class InterpreterFloorTests(unittest.TestCase):
    """`headtohead_sweep.py` must still import on the stock system interpreter."""

    def test_the_tool_runs_on_the_minimum_interpreter(self):
        assert_runs_on_min_python(self, h2h)


if __name__ == "__main__":
    unittest.main(verbosity=2)
