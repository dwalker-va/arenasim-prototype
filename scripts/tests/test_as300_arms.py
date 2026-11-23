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
[ 11.60s] [DMG] Team 2 Priest #1's Mind Flay ticks on Team 1 Priest #1 for 10 damage
[ 11.80s] [DEATH] Team 1 Mage #2 has been eliminated
[ 13.02s] [CAST] Team 1 Priest #1 begins casting Flash Heal on Team 1 Priest #1
[ 14.52s] [HEAL] Team 1 Priest #1's Flash Heal heals Team 1 Priest #1 for 100
[ 15.00s] [HEAL] Team 1 Priest #1's Flash Heal CRITICALLY heals Team 1 Priest #1 for 150
[ 15.10s] [HEAL] Team 2 Paladin #2's Flash of Light heals Team 2 Warrior #1 for 80
[ 15.20s] [HEAL] [SAVED] Guardian Spirit heals Team 1 Priest #1 for 120
[ 16.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Warrior #1
[ 17.00s] [CAST] Team 2 Warrior #1 interrupts Team 1 Priest #1's Mind Flay - Shadow school locked for 4.0s
[ 17.20s] [CAST] Team 1 Priest #1 casts Power Word: Shield on Team 1 Priest #1
[ 20.00s] [DMG] Team 1 Priest #1's Mind Blast hits Team 2 Warrior #1 for 60 damage
[ 21.00s] [DMG] Team 1 Priest #1's Mind Blast CRITS Team 2 Warrior #1 for 95 damage
[ 22.00s] [DMG] Team 1 Priest #1's Shadow Word: Pain ticks for 10 damage on Team 2 Warrior #1
[ 23.00s] [DMG] Team 2 Mage #1's Frostbolt hits Team 2 Warrior #1 for 50 damage
[ 30.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Warrior #1
[ 30.50s] [CAST] Team 2 Warrior #1 interrupts Team 1 Priest #1's Mind Flay - Shadow school locked for 4.0s
[ 35.00s] [CAST] Team 1 Priest #1 casts Power Word: Shield on Team 1 Priest #1
[ 40.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Warrior #1
[ 41.00s] [CC] Team 1 Priest #1's Mind Flay interrupted by crowd control
[ 45.00s] [CAST] Team 1 Priest #1 casts Psychic Scream
[ 50.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Warrior #1
[ 52.00s] [CC] Team 1 Priest #1's Mind Flay interrupted by Silence
[ 55.00s] [CAST] Team 1 Priest #1 casts Power Word: Shield on Team 1 Priest #1
[ 60.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Rogue #2
[ 61.50s] [DEATH] Team 2 Rogue #2 has been eliminated
[ 65.00s] [CAST] Team 1 Priest #1 casts Power Word: Shield on Team 1 Priest #1
[ 70.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Warrior #1
[ 71.00s] [DMG] Team 1 Priest #1's Mind Flay ticks on Team 2 Warrior #1 for 10 damage
"""
# A channel its own Priest ends early with a cast, as a separate log: LOG's
# last channel has no later line, so it runs to the log's end.
OWN_CAST = """\
[ 80.00s] [CAST] Team 1 Priest #1 begins channeling Mind Flay on Team 2 Warrior #1
[ 81.00s] [CAST] Team 1 Priest #1 casts Power Word: Shield on Team 1 Priest #1
"""
LINES = LOG.splitlines()


def lines(first, last):
    """LOG's lines `first` to `last`, 1-based and inclusive, as a log."""
    return "\n".join(LINES[first - 1:last])


class ChannelEndTests(unittest.TestCase):
    """A channel ends at its real end: the first of an interrupt of it, crowd
    control or a silence cutting it, its target's or caster's death, its
    caster's next cast, its full 3s, or the log's end."""

    def ended(self, text):
        c = arms.mechanism_counts(text)
        return c["ended"], round(c["channel_secs"], 2)

    def test_a_channel_its_priest_next_casts_after_runs_its_full_three_seconds(self):
        # another unit's death mid-channel (the Mage, 11.80) does not end it
        self.assertEqual(self.ended(lines(1, 9)), ({"full": 1}, 3.0))

    def test_an_interrupt_ends_a_channel_before_the_next_cast(self):
        self.assertEqual(self.ended(lines(14, 16)), ({"interrupt": 1}, 1.0))

    def test_an_interrupted_channel_with_no_cast_for_three_seconds_is_not_full(self):
        self.assertEqual(self.ended(lines(21, 23)), ({"interrupt": 1}, 0.5))

    def test_crowd_control_and_a_silence_cut_a_channel_short(self):
        self.assertEqual(self.ended(lines(24, 26)), ({"crowd control or silence": 1}, 1.0))
        self.assertEqual(self.ended(lines(27, 29)), ({"crowd control or silence": 1}, 2.0))

    def test_its_targets_death_ends_a_channel(self):
        self.assertEqual(self.ended(lines(30, 32)), ({"death": 1}, 1.5))

    def test_its_priests_own_cast_before_three_seconds_cuts_it(self):
        self.assertEqual(self.ended(OWN_CAST), ({"own cast": 1}, 1.0))

    def test_a_log_that_ends_mid_channel_ends_it_there(self):
        self.assertEqual(self.ended(lines(33, 34)), ({"match end": 1}, 1.0))

    def test_the_whole_log(self):
        c = arms.mechanism_counts(LOG)
        self.assertEqual(c["channels"], 7)
        self.assertEqual(c["ended"], {"full": 1, "interrupt": 2,
                                      "crowd control or silence": 2, "death": 1,
                                      "match end": 1})
        self.assertAlmostEqual(c["channel_secs"], 10.0)
        # six channels are followed by a cast of their Priest; one of those
        # ran its full 3s
        self.assertEqual((c["with_next_cast"], c["full_with_next_cast"]), (6, 1))


class MechanismCountTests(unittest.TestCase):
    def setUp(self):
        self.c = arms.mechanism_counts(LOG)

    def test_a_priests_damage_is_summed_by_spell_hits_and_crits_and_ticks(self):
        # Mind Flay: two ticks of Team 1's Priest, one of Team 2's
        self.assertEqual(self.c["damage"],
                         {"Mind Flay": 30, "Mind Blast": 155, "Shadow Word: Pain": 10})
        self.assertEqual(self.c["mind_blast_hits"], 2)

    def test_healing_counts_a_priests_heals_and_crits_and_nobody_elses(self):
        self.assertEqual(self.c["healing"], 250)

    def test_damage_taken_counts_direct_damage_on_a_priest_but_not_periodic_ticks(self):
        # the hit, the crit and the channelled tick (40 + 90 + 10); not the
        # Rend tick, and not the hit on the Mage
        self.assertEqual(self.c["damage_taken"], 140)

    def test_counts_merge_across_matches(self):
        total = arms.merge_counts([self.c, self.c])
        self.assertEqual(total["channels"], 14)
        self.assertEqual(total["damage"]["Mind Blast"], 310)
        self.assertEqual(total["ended"]["interrupt"], 4)
        self.assertEqual((total["with_next_cast"], total["full_with_next_cast"]), (12, 2))
        self.assertAlmostEqual(total["channel_secs"], 20.0)
        self.assertEqual(total["damage_taken"], 280)

    def test_a_log_with_no_mind_flay_counts_none(self):
        c = arms.mechanism_counts(lines(17, 20))
        self.assertEqual((c["channels"], c["ended"], c["channel_secs"]), (0, {}, 0.0))
        self.assertEqual(c["mind_blast_hits"], 2)


class MinPythonTests(unittest.TestCase):
    def test_the_arms_script_runs_on_the_floor_interpreter(self):
        assert_file_runs_on_min_python(self, ARMS)


if __name__ == "__main__":
    unittest.main()
