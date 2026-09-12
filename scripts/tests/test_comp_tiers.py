#!/usr/bin/env python3
"""Offline fixture tests for `scripts/comp_tiers.py`.

This is the tool that turns a canonical baseline sweep into the class tier
list, the top/bottom comps and the non-competitive anomaly canary that
`docs/design/balance/canonical_baselines_summary.md` is written from. Every
number in it is an average over sides, and an averaging bug reads as a balance
finding: a class that looks two points strong because its games were
double-counted is indistinguishable, in the output, from a class that is.

So the counting rules are pinned directly -- each match contributes exactly one
game to each side, draws are losses for both, a comp's key is order-insensitive
-- alongside the predicates (`is_competitive`) the canary hangs on.

Offline by construction: hand-built CSVs, and the module's `subprocess` is
replaced with a guard.

Run directly, or via `cargo test --test sweep_script_fixtures`:

    python3 scripts/tests/test_comp_tiers.py
"""

from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from sweep_fixtures import FixtureTestCase, install_no_subprocess, run_main  # noqa: E402

import comp_tiers as ct  # noqa: E402

install_no_subprocess(ct)


def rows(*specs):
    """(team1, team2, winner) triples as the CSV DictReader would yield them."""
    return [
        {"team1": t1, "team2": t2, "winner": w, "label": "", "seed": "0",
         "end_reason": "fixture", "duration_secs": "1.0"}
        for t1, t2, w in specs
    ]


class ClassTierTests(unittest.TestCase):
    def test_each_side_contributes_one_game_per_class(self):
        r = rows(("Warrior+Priest", "Mage+Priest", "team1"))
        tiers = ct.class_tiers(r)
        # Priest played both sides of the one match: one win, one loss.
        self.assertEqual(tiers["Priest"], 50.0)
        self.assertEqual(tiers["Warrior"], 100.0)
        self.assertEqual(tiers["Mage"], 0.0)

    def test_draws_are_losses_for_both_sides(self):
        r = rows(("Warrior", "Mage", "draw"))
        tiers = ct.class_tiers(r)
        self.assertEqual(tiers["Warrior"], 0.0)
        self.assertEqual(tiers["Mage"], 0.0)

    def test_an_errored_match_is_not_a_win_for_anyone(self):
        r = rows(("Warrior", "Mage", "error"))
        tiers = ct.class_tiers(r)
        self.assertEqual(tiers["Warrior"], 0.0)
        self.assertEqual(tiers["Mage"], 0.0)

    def test_a_duplicated_class_on_one_team_counts_once(self):
        """`set(team)` is deliberate: a comp is not two games for one class."""
        r = rows(("Warrior+Warrior", "Mage+Mage", "team1"))
        self.assertEqual(ct.class_tiers(r), {"Warrior": 100.0, "Mage": 0.0})

    def test_winrates_are_percentages_of_that_class_games(self):
        r = rows(
            ("Warrior", "Mage", "team1"),
            ("Warrior", "Rogue", "team1"),
            ("Priest", "Warrior", "team1"),
            ("Priest", "Warrior", "team1"),
        )
        # Warrior: won 2 of 4.
        self.assertEqual(ct.class_tiers(r)["Warrior"], 50.0)


class CompTierTests(unittest.TestCase):
    def test_comp_key_is_order_insensitive(self):
        r = rows(
            ("Warrior+Priest", "Mage+Rogue", "team1"),
            ("Priest+Warrior", "Mage+Rogue", "team2"),
        )
        tiers = ct.comp_tiers(r)
        self.assertIn("Priest+Warrior", tiers)
        self.assertNotIn("Warrior+Priest", tiers)
        self.assertEqual(tiers["Priest+Warrior"], 50.0)

    def test_every_match_is_two_comp_games(self):
        r = rows(
            ("Warrior", "Mage", "team1"),
            ("Warrior", "Rogue", "draw"),
        )
        tiers = ct.comp_tiers(r)
        self.assertEqual(tiers["Warrior"], 50.0)  # 1 win of 2
        self.assertEqual(tiers["Mage"], 0.0)
        self.assertEqual(tiers["Rogue"], 0.0)

    def test_a_mirror_match_gives_the_comp_a_win_and_a_loss(self):
        r = rows(("Warrior+Priest", "Warrior+Priest", "team1"))
        self.assertEqual(ct.comp_tiers(r)["Priest+Warrior"], 50.0)


class DrawRateTests(unittest.TestCase):
    def test_draw_percentage(self):
        r = rows(
            ("Warrior", "Mage", "team1"),
            ("Warrior", "Mage", "draw"),
            ("Warrior", "Mage", "draw"),
            ("Warrior", "Mage", "team2"),
        )
        self.assertEqual(ct.draws(r), 50.0)

    def test_no_rows_is_not_a_zero_draw_rate(self):
        """0 of 0 has no draw rate, and must not be reported as 0.0%."""
        self.assertIsNone(ct.draws([]))


class CompetitivePredicateTests(unittest.TestCase):
    def test_2v2_allows_at_most_one_healer(self):
        self.assertTrue(ct.is_competitive(["Warrior", "Mage"], 2))
        self.assertTrue(ct.is_competitive(["Warrior", "Priest"], 2))
        self.assertFalse(ct.is_competitive(["Priest", "Paladin"], 2))
        self.assertFalse(ct.is_competitive(["Shaman", "Priest"], 2))

    def test_3v3_allows_one_or_two_healers(self):
        self.assertFalse(ct.is_competitive(["Warrior", "Mage", "Rogue"], 3))
        self.assertTrue(ct.is_competitive(["Warrior", "Mage", "Priest"], 3))
        self.assertTrue(ct.is_competitive(["Warrior", "Priest", "Paladin"], 3))
        self.assertFalse(ct.is_competitive(["Priest", "Paladin", "Shaman"], 3))

    def test_all_three_healer_classes_count(self):
        self.assertEqual(ct.HEALERS, {"Priest", "Paladin", "Shaman"})

    def test_competitive_rows_needs_both_teams_competitive(self):
        r = rows(
            ("Warrior+Priest", "Mage+Paladin", "team1"),  # both fine
            ("Priest+Paladin", "Mage+Warrior", "team1"),  # team1 double-healer
            ("Warrior+Mage", "Priest+Shaman", "team1"),  # team2 double-healer
        )
        kept = ct.competitive_rows(r)
        self.assertEqual(len(kept), 1)
        self.assertEqual(kept[0]["team1"], "Warrior+Priest")


class AnomalyCanaryTests(unittest.TestCase):
    def test_only_non_competitive_comps_are_reported(self):
        r = rows(
            ("Priest+Paladin", "Warrior+Mage", "team1"),
            ("Warrior+Priest", "Mage+Rogue", "team1"),
        )
        out = dict((k, (wr, cwr)) for k, wr, cwr in ct.noncompetitive_anomalies(r))
        self.assertEqual(list(out), ["Paladin+Priest"])
        # double-DPS IS competitive at 2v2, so it is not an anomaly candidate
        self.assertNotIn("Mage+Rogue", out)
        self.assertNotIn("Priest+Warrior", out)

    def test_full_field_and_vs_competitive_are_separate_rates(self):
        r = rows(
            # the double-healer beats a competitive comp...
            ("Priest+Paladin", "Warrior+Mage", "team1"),
            # ...and loses to another double-healer (not a competitive opponent)
            ("Priest+Paladin", "Shaman+Priest", "team2"),
        )
        out = {k: (wr, cwr) for k, wr, cwr in ct.noncompetitive_anomalies(r)}
        wr, cwr = out["Paladin+Priest"]
        self.assertAlmostEqual(wr, 50.0)  # 1 of 2 over the full field
        self.assertAlmostEqual(cwr, 100.0)  # 1 of 1 against competitive comps

    def test_no_competitive_opponents_is_none_not_zero(self):
        r = rows(("Priest+Paladin", "Shaman+Priest", "team1"))
        out = {k: (wr, cwr) for k, wr, cwr in ct.noncompetitive_anomalies(r)}
        self.assertIsNone(out["Paladin+Priest"][1])

    def test_the_team_size_comes_from_the_team(self):
        """`competitive_rows` judges a team by its own length; so must this.

        Two predicates over the same word in the same report, disagreeing about
        which team size to apply, is how a canary starts pointing at comps that
        are perfectly ordinary. The bracket the report was invoked for is not
        evidence about a row: a 3v3 row must be judged as the 3v3 it is.
        """
        r = rows(("Warrior+Mage+Rogue", "Warrior+Mage+Priest", "team1"))
        out = {k: wr for k, wr, _ in ct.noncompetitive_anomalies(r)}
        # Triple-DPS is the non-competitive 3v3 shape; the healer comp is fine.
        self.assertIn("Mage+Rogue+Warrior", out)
        self.assertNotIn("Mage+Priest+Warrior", out)


class ReportTests(FixtureTestCase):
    def _csv(self, *specs):
        return self.csv_with(
            [
                dict(r, seed=str(i))
                for i, r in enumerate(rows(*specs))
            ]
        )

    def report(self, path, *argv):
        return run_main(ct.main, [path, *argv])

    def test_size_2_report(self):
        path = self._csv(
            ("Warrior+Priest", "Mage+Priest", "team1"),
            ("Warrior+Priest", "Rogue+Paladin", "team2"),
            ("Priest+Paladin", "Mage+Warrior", "team1"),
        )
        run = self.assertOk(self.report(path, "--size", "2"))
        self.assertHas(run, "3 matches")
        self.assertHas(run, "## class tiers")
        self.assertHas(run, "## top comps")
        self.assertHas(run, "anomaly canary")

    def test_the_canary_flags_a_winning_non_competitive_comp(self):
        path = self._csv(
            ("Priest+Paladin", "Warrior+Mage", "team1"),
            ("Priest+Paladin", "Rogue+Mage", "team1"),
        )
        run = self.assertOk(self.report(path, "--size", "2"))
        self.assertHas(run, "<< ANOMALY")

    def test_the_canary_stays_quiet_on_a_losing_one(self):
        path = self._csv(
            ("Priest+Paladin", "Warrior+Mage", "team2"),
            ("Priest+Paladin", "Rogue+Mage", "team2"),
        )
        run = self.assertOk(self.report(path, "--size", "2"))
        self.assertLacks(run, "<< ANOMALY")

    def test_size_3_adds_the_dominant_shape_watch(self):
        path = self._csv(
            ("Warrior+Mage+Priest", "Rogue+Warlock+Paladin", "team1"),
            ("Warrior+Priest+Paladin", "Rogue+Warlock+Shaman", "team1"),
        )
        run = self.assertOk(self.report(path, "--size", "3"))
        self.assertHas(run, "dominant-shape watch")
        self.assertHas(run, "/10 top-10 comps are double-healer")

    def test_without_size_the_legacy_report_runs(self):
        path = self._csv(
            ("Warrior", "Mage", "team1"),
            ("Rogue", "Priest", "team2"),
        )
        run = self.assertOk(self.report(path))
        self.assertHas(run, "## class tiers")
        self.assertLacks(run, "anomaly canary")

    def test_draw_rate_is_reported(self):
        path = self._csv(
            ("Warrior+Priest", "Mage+Priest", "draw"),
            ("Warrior+Priest", "Mage+Priest", "team1"),
        )
        run = self.assertOk(self.report(path, "--size", "2"))
        self.assertHas(run, "draws 50.0%")

    def test_an_empty_csv_says_so_instead_of_crashing(self):
        """A sweep that produced nothing must not report 0.0% of anything."""
        path = self.csv_with([])
        run = self.report(path, "--size", "2")
        self.assertIsInstance(run.code, str)
        self.assertIn("no matches", run.code)

    def test_an_empty_csv_in_legacy_mode_too(self):
        path = self.csv_with([])
        run = self.report(path)
        self.assertIsInstance(run.code, str)
        self.assertIn("no matches", run.code)

    def test_a_missing_file_is_reported_not_traced(self):
        run = self.report(self.path("nope.csv"), "--size", "2")
        self.assertIsInstance(run.code, str)
        self.assertIn("nope.csv", run.code)


if __name__ == "__main__":
    unittest.main(verbosity=2)
