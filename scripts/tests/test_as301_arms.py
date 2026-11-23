#!/usr/bin/env python3
"""Offline fixture tests for `docs/design/balance/sweeps/2026-11-22-as301-arms.py`.

That script prints the claims the AS-301 findings doc quotes, and each claim
below is a sentence a reader takes at face value:

- a node row's MARGIN z: the mean over paired matches of the node's margin
  over the stat-rank allowance, over that mean's standard error (the
  per-match form, so the covariance of arms that share matches is in it),
  and the Bonferroni mark that rides on it in either direction. Checked
  against a worked example small enough to do by hand;
- that `replicate` reads each block's allowance from that block's Vitality;
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
    """What `verdict` reads off a `paired_sweep.Slice`."""

    def __init__(self, to_win, to_loss, n, floor=1.0):
        self.to_win, self.to_loss, self.n, self.floor = to_win, to_loss, n, floor

    @property
    def delta(self):
        return 100.0 * (self.to_win - self.to_loss) / self.n


class MarginZ(unittest.TestCase):
    """The worked example, four paired matches and k = -0.5 (1 = the Rogue
    side won):

    - node arm    1 1 0 1      comparison  0 1 0 0
    - stem + Vit  1 0 0 1      stem        0 0 0 1
    - x = 100 * [(node - comparison) - k * (stem+Vit - stem)]
        = 150, 0, 0, 100;  mean 62.5
    - sd (n - 1) = sqrt((87.5^2 + 62.5^2 + 62.5^2 + 37.5^2) / 3) = 75
    - se = 75 / sqrt(4) = 37.5;  z = 62.5 / 37.5 = 1.6667
    """

    def test_the_per_match_worked_example(self):
        margin, z = arms.per_match_margin([1, 1, 0, 1], [0, 1, 0, 0], [1, 0, 0, 1],
                                          [0, 0, 0, 1], -0.5)
        self.assertAlmostEqual(margin, 62.5)
        self.assertAlmostEqual(z, 62.5 / 37.5)

    def test_a_shared_arm_cancels_in_the_per_match_margin(self):
        # The node arm is the stem + Vitality arm and the comparison is the
        # stem, at k = 1: every match's margin is (A - B) - (A - B) = 0, though
        # each delta alone varies match to match. Only the per-match form
        # sees that the two rows' errors cancel.
        a, b = [1, 0, 1, 1, 0, 1], [0, 0, 1, 0, 1, 1]
        self.assertEqual(arms.per_match_margin(a, b, a, b, 1.0), (0.0, 0.0))

    def test_the_bonferroni_mark_reads_the_margin_z_in_either_direction(self):
        s = FakeSlice(50, 50, 4000, floor=0.5)
        self.assertIn("BONF", arms.node_cells(s, -1.0, 1.2, 3.0))
        self.assertIn("BONF", arms.node_cells(s, 1.0, -1.2, -3.0))
        self.assertNotIn("BONF", arms.node_cells(s, -1.0, 1.2, 2.0))
        self.assertNotIn("BONF", arms.node_cells(s, 1.0, -1.2, -2.0))

    def test_the_threshold_is_bonferroni_over_the_node_rows(self):
        # Phi^-1(1 - 0.025/12) = 2.865, rounded up.
        self.assertEqual(len(arms.NODE_ARMS), 12)
        self.assertAlmostEqual(arms.BONFERRONI_Z, 2.87)


CSV_HEADER = "label,team1,team2,seed,winner,end_reason,duration_secs\n"


def write_arm(directory, arm, seeds, rogue_wins):
    """A batch CSV of one-sided matches, Rogue on team 1, that the Rogue side
    wins on the seeds in `rogue_wins`."""
    with open(os.path.join(directory, arm + ".csv"), "w", encoding="utf-8") as f:
        f.write(CSV_HEADER)
        for seed in seeds:
            winner = "team1" if seed in rogue_wins else "team2"
            f.write("Rogue+Priest_vs_Mage+Paladin,Rogue+Priest,Mage+Paladin,%d,%s,kill,30.0\n"
                    % (seed, winner))


class ReplicatePerBlock(unittest.TestCase):
    """`replicate` reads a node row's allowance from the Vitality rank of the
    same block. Block 1's Vitality is worth 0 (5 of 10 either way); block
    2's is worth +40pt (5 -> 9 of 10); pooled it is +20pt."""

    def test_a_block_row_reads_its_own_vitality(self):
        import tempfile
        with tempfile.TemporaryDirectory() as root:
            b1, b2 = os.path.join(root, "b1"), os.path.join(root, "b2")
            os.makedirs(b1)
            os.makedirs(b2)
            s1, s2 = range(0, 10), range(10, 20)
            for d, seeds, vit in ((b1, s1, set(range(0, 5))), (b2, s2, set(range(10, 19)))):
                write_arm(d, "sub_stem", seeds, set(seeds[:5]))
                write_arm(d, "sub_vit", seeds, vit)
                write_arm(d, "sub_cd", seeds, set(seeds[:8]))
            rows = arms.replicate_rows(b1, b2, [("Cheat Death", "sub_cd", "sub_vit")])
        allow = {name: stats[0] for _, name, _, stats in rows}
        k = 11.0 / arms.VITALITY_PRICE - 1.0
        self.assertAlmostEqual(allow["block 1"], 0.0)
        self.assertAlmostEqual(allow["block 2"], k * 40.0)
        self.assertAlmostEqual(allow["pooled"], k * 20.0)


class AllowanceZero(unittest.TestCase):
    """The list of verdicts the price model decides: a one-rank node a bit
    under a full Vitality is within by the model (its allowance is
    negative) and below at allowance 0; one far over it is DOMINATES both
    ways and is not listed."""

    def test_only_a_verdict_the_model_changes_is_listed(self):
        near = FakeSlice(40, 80, 4000, floor=0.6)  # -1.0pt, allowance -1.15
        far = FakeSlice(300, 100, 4000, floor=0.6)  # +5.0pt
        self.assertEqual(
            arms.changes_at_allowance_zero([("near", near, -1.152), ("far", far, -1.152)]),
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
