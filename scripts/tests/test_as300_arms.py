#!/usr/bin/env python3
"""Offline fixture tests for the AS-300 arms script's `mechanism` counts.

`docs/design/balance/sweeps/2026-11-22-as300-arms.py mechanism` replays Priest
matches and sums what a Priest's Mind Flay did from the match log: its
channels, how long each held the Priest, the Priest's damage by spell, its
healing and the damage it took. The findings doc's Mind Flay paragraph quotes
those sums, so the log reading is pinned here on a hand-built log, line shape
by line shape, with a line of each shape it must NOT count beside it.

Offline by construction: the module's `subprocess` is replaced with a guard.

Run directly, or via `cargo test --test script_fixture_suites`:

    python3 scripts/tests/test_as300_arms.py
"""

from __future__ import annotations

import importlib.util
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from _harness import assert_file_runs_on_min_python, install_no_subprocess  # noqa: E402

ARMS = os.path.normpath(os.path.join(
    HERE, "..", "..", "docs", "design", "balance", "sweeps", "2026-11-22-as300-arms.py"))
_spec = importlib.util.spec_from_file_location("as300_arms", ARMS)
arms = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(arms)
install_no_subprocess(arms)

LOG = """\
[ 10.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Warrior #1
[ 10.50s] [DMG] Team 1 Priest #1's Mind Flay ticks on Team 2 Warrior #1 for 10 damage
[ 11.00s] [DMG] Team 2 Warrior #1's Auto Attack hits Team 1 Priest #1 for 40 damage
[ 11.20s] [DMG] Team 2 Warrior #1's Mortal Strike CRITS Team 1 Priest #1 for 90 damage (12 absorbed)
[ 11.40s] [DMG] Team 2 Warrior #1's Rend ticks for 7 damage on Team 1 Priest #1
[ 11.50s] [DMG] Team 2 Warrior #1's Auto Attack hits Team 1 Mage #2 for 33 damage
[ 13.02s] [CAST] Team 1 Priest #1 begins casting Flash Heal on Team 1 Mage #2
[ 14.52s] [HEAL] Team 1 Priest #1's Flash Heal heals Team 1 Mage #2 for 100
[ 15.00s] [HEAL] Team 1 Priest #1's Flash Heal CRITICALLY heals Team 1 Priest #1 for 150
[ 15.10s] [HEAL] Team 2 Paladin #2's Flash of Light heals Team 2 Warrior #1 for 80
[ 15.20s] [HEAL] [SAVED] Guardian Spirit heals Team 1 Mage #2 for 120
[ 16.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Warrior #1
[ 17.00s] [CAST] Team 2 Warrior #1 interrupts Team 1 Priest #1's Mind Flay - Shadow school locked for 4s
[ 17.20s] [CAST] Team 1 Priest #1 casts Power Word: Shield on Team 1 Priest #1
[ 20.00s] [DMG] Team 1 Priest #1's Mind Blast hits Team 2 Warrior #1 for 60 damage
[ 21.00s] [DMG] Team 1 Priest #1's Mind Blast CRITS Team 2 Warrior #1 for 95 damage
[ 22.00s] [DMG] Team 1 Priest #1's Shadow Word: Pain ticks for 10 damage on Team 2 Warrior #1
[ 23.00s] [DMG] Team 2 Mage #1's Frostbolt hits Team 2 Warrior #1 for 50 damage
"""


class MechanismCountTests(unittest.TestCase):
    def setUp(self):
        self.c = arms.mechanism_counts(LOG)

    def test_each_channel_is_held_until_its_priest_next_casts_capped_at_three_seconds(self):
        self.assertEqual(self.c["channels"], 2)
        # 10.00 -> 13.02 (a full channel, capped at 3.0); 16.00 -> 17.20
        # (cut short by the Pummel: the interrupt line is not the Priest's cast)
        self.assertEqual([round(g, 2) for g in self.c["gaps"]], [3.02, 1.2])
        self.assertAlmostEqual(self.c["channel_secs"], 4.2)
        self.assertEqual(self.c["full_channels"], 1)

    def test_a_priests_damage_is_summed_by_spell_hits_and_crits_and_ticks(self):
        self.assertEqual(self.c["damage"],
                         {"Mind Flay": 10, "Mind Blast": 155, "Shadow Word: Pain": 10})
        self.assertEqual(self.c["mind_blast_hits"], 2)

    def test_healing_counts_a_priests_heals_and_crits_and_nobody_elses(self):
        self.assertEqual(self.c["healing"], 250)

    def test_damage_taken_counts_hits_and_crits_on_a_priest_but_not_periodic_ticks(self):
        # 40 + 90; the Rend tick and the hit on the Mage are not the Priest's
        self.assertEqual(self.c["damage_taken"], 130)

    def test_counts_merge_across_matches(self):
        total = arms.merge_counts([self.c, self.c])
        self.assertEqual(total["channels"], 4)
        self.assertEqual(total["damage"]["Mind Blast"], 310)
        self.assertEqual(len(total["gaps"]), 4)
        self.assertAlmostEqual(total["channel_secs"], 8.4)

    def test_a_log_with_no_mind_flay_counts_none(self):
        c = arms.mechanism_counts("\n".join(LOG.splitlines()[13:]))
        self.assertEqual((c["channels"], c["gaps"], c["channel_secs"]), (0, [], 0.0))
        self.assertEqual(c["mind_blast_hits"], 2)


class MinPythonTests(unittest.TestCase):
    def test_the_arms_script_runs_on_the_floor_interpreter(self):
        assert_file_runs_on_min_python(self, ARMS)


if __name__ == "__main__":
    unittest.main()
