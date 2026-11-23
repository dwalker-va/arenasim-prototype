#!/usr/bin/env python3
"""Offline fixture tests for `docs/design/balance/sweeps/2026-11-22-as301-arms.py`.

That script prints the claims the AS-301 findings doc quotes, and each claim
below is a sentence a reader takes at face value:

- a node row's MARGIN z (its margin over the stat-rank allowance, over the
  standard error of both), and the Bonferroni mark that rides on it. Checked
  against a worked example small enough to do by hand, not against what the
  code computes;
- which branch of the Shadow Dance gate (`enemy_healer_answers`) admitted a
  Dance, read off a match;
- what counts as a Dance converting: a team member's death (never a pet's)
  up to 2s past the later of the Dance's end and its chain's last stun's end;
- the Vitality twin's counterfactual: the Dance's victim dead by that window
  end in another arm's log.

Pure functions over hand-built configs and logs; the module's `subprocess` is
replaced with a guard, so nothing runs a match.

Run directly, or via `cargo test --test script_fixture_suites`:

    python3 scripts/tests/test_as301_arms.py
"""

from __future__ import annotations

import importlib.util
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from _harness import REPO_ROOT, assert_runs_on_min_python, install_no_subprocess  # noqa: E402

PATH = os.path.join(REPO_ROOT, "docs", "design", "balance", "sweeps", "2026-11-22-as301-arms.py")
_spec = importlib.util.spec_from_file_location("as301_arms", PATH)
arms = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(arms)
install_no_subprocess(arms)


class FakeSlice:
    """What the statistics read off a `paired_sweep.Slice`."""

    def __init__(self, to_win, to_loss, n, floor=1.0):
        self.to_win, self.to_loss, self.n, self.floor = to_win, to_loss, n, floor

    @property
    def delta(self):
        return 100.0 * (self.to_win - self.to_loss) / self.n


class MarginZ(unittest.TestCase):
    """The worked example: a node at one rank's price (11) whose arm flips
    30 matches toward the Rogue side and 10 away in 100, against a Vitality
    that flips 10 toward and none away in 100.

    - node: delta 20pt, se = 100 * sqrt(40 - 400/100) / 100 = 6.0
    - Vitality: delta 10pt, se = 100 * sqrt(10 - 100/100) / 100 = 3.0
    - k = 11/20.4 - 1 = -0.46078; allowance = -4.6078; margin = 24.6078
    - se(margin) = sqrt(6.0^2 + (0.46078 * 3.0)^2) = 6.1573; z = 3.9966
    """

    def test_paired_se_is_the_discordant_pair_formula(self):
        self.assertAlmostEqual(arms.paired_se(FakeSlice(30, 10, 100)), 6.0)
        self.assertAlmostEqual(arms.paired_se(FakeSlice(10, 0, 100)), 3.0)

    def test_margin_z_folds_the_allowance_error_in_quadrature(self):
        allow, margin, z = arms.margin_stats(FakeSlice(30, 10, 100), 11.0, FakeSlice(10, 0, 100))
        self.assertAlmostEqual(allow, -4.6078, places=3)
        self.assertAlmostEqual(margin, 24.6078, places=3)
        self.assertAlmostEqual(z, 3.9966, places=3)

    def test_a_capstone_allowance_is_slightly_positive(self):
        allow, _, _ = arms.margin_stats(FakeSlice(30, 10, 100), 22.0, FakeSlice(10, 0, 100))
        self.assertAlmostEqual(allow, 10.0 * (22.0 / 20.4 - 1.0))

    def test_the_bonferroni_mark_reads_the_margin_z_not_the_raw_delta(self):
        # Raw delta 0: no evidence the node differs from Vitality, but a
        # one-rank node level with Vitality beats a stat rank of its price.
        level = FakeSlice(50, 50, 4000, floor=0.5)  # se 0.25
        vit = FakeSlice(100, 0, 4000)  # +2.5pt, se 0.247
        _, margin, z = arms.margin_stats(level, 11.0, vit)
        self.assertGreater(margin, 1.0)
        cells = arms.node_cells(level, 11.0, vit)
        self.assertGreaterEqual(abs(z), arms.BONFERRONI_Z)
        self.assertIn("BONF", cells)
        # A margin z under the threshold carries no mark.
        small = arms.node_cells(FakeSlice(12, 8, 100), 11.0, FakeSlice(0, 0, 100))
        self.assertNotIn("BONF", small)

    def test_the_threshold_is_bonferroni_over_the_node_rows(self):
        # Phi^-1(1 - 0.025/12) = 2.865, rounded up.
        self.assertEqual(len(arms.NODE_ARMS), 12)
        self.assertAlmostEqual(arms.BONFERRONI_Z, 2.87)


class AllowanceZero(unittest.TestCase):
    """The list of verdicts the price model decides: a one-rank node a bit
    under a full Vitality is within by the model (its allowance is
    negative) and below at allowance 0; one far over it is DOMINATES both
    ways and is not listed."""

    def test_only_a_verdict_the_model_changes_is_listed(self):
        vit = FakeSlice(100, 0, 4000)  # +2.5pt: a one-rank allowance of -1.15
        near = FakeSlice(40, 80, 4000, floor=0.6)  # -1.0pt
        far = FakeSlice(300, 100, 4000, floor=0.6)  # +5.0pt
        self.assertEqual(
            arms.changes_at_allowance_zero([("near", near, 11.0, vit), ("far", far, 11.0, vit)]),
            [("near", "within", "below")])


def log(*lines, duration=60.0):
    return "Duration: %.2fs\n" % duration + "\n".join(lines) + "\n"


DANCE_LINES = (
    "[ 20.00s] [CAST] Team 1 Rogue #1 uses Shadow Dance",
    "[ 20.10s] [CC] Team 1 Rogue #1 uses Cheap Shot on Team 2 Priest #2",
    "[ 20.12s] [CC] Cheap Shot on Team 2 Priest #2 (4.0s, DR: 100%)",
    "[ 23.70s] [CC] Team 1 Rogue #1 uses Cheap Shot on Team 2 Priest #2",
    "[ 23.72s] [CC] Cheap Shot on Team 2 Priest #2 (2.0s, DR: 50%)",
    "[ 25.20s] [CC] Team 1 Rogue #1 uses Kidney Shot on Team 2 Priest #2",
    "[ 25.22s] [CC] Kidney Shot on Team 2 Priest #2 (6.0s, DR: 100%)",
)


class DanceRecords(unittest.TestCase):
    CONFIG = {"team1": ["Rogue", "Warrior"], "team2": ["Warlock", "Priest"]}

    def test_the_chain_and_its_stun_seconds(self):
        (d,) = arms.analyse_dances(log(*DANCE_LINES), self.CONFIG)
        self.assertEqual(d["shape"], "CS4 CS2 KS6")
        self.assertAlmostEqual(d["applied"], 12.0)
        # The window runs to 2s past the later of the Dance's end (28.0)
        # and the last stun's end (25.2 + 6 = 31.2).
        self.assertAlmostEqual(d["window_end"], 33.2)

    def test_the_window_has_the_dance_end_as_its_floor(self):
        (d,) = arms.analyse_dances(log(*DANCE_LINES[:3]), self.CONFIG)
        # One 4s Cheap Shot ends at 24.1, before the Dance's 28.0.
        self.assertAlmostEqual(d["window_end"], 30.0)

    def test_a_pet_death_does_not_convert_a_dance(self):
        pet = "[ 26.00s] [DEATH] Team 2 Felhunter #1 has been eliminated"
        (d,) = arms.analyse_dances(log(*DANCE_LINES, pet), self.CONFIG)
        self.assertFalse(d["converts"])
        member = "[ 32.00s] [DEATH] Team 2 Priest #2 has been eliminated"
        (d,) = arms.analyse_dances(log(*DANCE_LINES, member), self.CONFIG)
        self.assertTrue(d["converts"])
        self.assertTrue(d["converts_victim"])
        self.assertTrue(d["victim_dead"])
        late = "[ 33.50s] [DEATH] Team 2 Priest #2 has been eliminated"
        (d,) = arms.analyse_dances(log(*DANCE_LINES, late), self.CONFIG)
        self.assertFalse(d["converts"])
        self.assertFalse(d["victim_dead"])


def on(victim):
    return (
        "[ 20.00s] [CAST] Team 1 Rogue #1 uses Shadow Dance",
        "[ 20.10s] [CC] Team 1 Rogue #1 uses Cheap Shot on %s" % victim,
        "[ 20.12s] [CC] Cheap Shot on %s (4.0s, DR: 100%%)" % victim,
    )


class Admission(unittest.TestCase):
    """Each branch of `enemy_healer_answers` that lets a Dance go, and the
    one it cannot be read from, set equal to the module's list."""

    def admitted(self, config, *lines):
        (d,) = arms.analyse_dances(log(*lines), config)
        return d["admission"]

    def test_every_branch(self):
        seen = {
            self.admitted({"team1": ["Rogue", "Mage"], "team2": ["Warrior", "Druid"]},
                          *on("Team 2 Druid #2")),
            self.admitted({"team1": ["Rogue", "Mage"], "team2": ["Warrior", "Mage"]},
                          *on("Team 2 Mage #2")),
            self.admitted({"team1": ["Rogue", "Mage"], "team2": ["Warrior", "Paladin"]},
                          "[ 15.00s] [DEATH] Team 2 Paladin #2 has been eliminated",
                          *on("Team 2 Warrior #1")),
            self.admitted({"team1": ["Rogue", "Mage"], "team2": ["Warrior", "Paladin"]},
                          *on("Team 2 Warrior #1")),
            self.admitted({"team1": ["Rogue", "Mage"], "team2": ["Warrior", "Paladin"]},
                          "[ 20.00s] [CAST] Team 1 Rogue #1 uses Shadow Dance"),
        }
        self.assertEqual(seen, set(arms.ADMISSIONS))

    def test_a_healer_dead_after_the_dance_still_stands_at_it(self):
        self.assertEqual(
            self.admitted({"team1": ["Rogue", "Mage"], "team2": ["Warrior", "Paladin"]},
                          *on("Team 2 Warrior #1"),
                          "[ 25.00s] [DEATH] Team 2 Paladin #2 has been eliminated"),
            "a healer stands: its lock or the kill window")


class Twin(unittest.TestCase):
    def test_the_victims_fate_in_another_arms_log_at_the_window_end(self):
        config = {"team1": ["Rogue", "Mage"], "team2": ["Warrior", "Priest"]}
        (d,) = arms.analyse_dances(log(*on("Team 2 Priest #2")), config)  # ends at 30.0
        dead = log("[ 29.00s] [DEATH] Team 2 Priest #2 has been eliminated")
        self.assertEqual(arms.fates(dead, d), (True, True))
        later = log("[ 31.00s] [DEATH] Team 2 Priest #2 has been eliminated")
        self.assertEqual(arms.fates(later, d), (False, False))
        other = log("[ 29.00s] [DEATH] Team 2 Warrior #1 has been eliminated",
                    "[ 29.50s] [DEATH] Team 2 Felhunter #1 has been eliminated")
        self.assertEqual(arms.fates(other, d), (False, True))


class Interpreter(unittest.TestCase):
    def test_runs_on_the_minimum_python(self):
        assert_runs_on_min_python(self, arms)


if __name__ == "__main__":
    unittest.main(verbosity=2)
