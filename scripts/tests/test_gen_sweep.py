#!/usr/bin/env python3
"""Offline fixture tests for `scripts/gen_sweep.py`.

The generator decides what a sweep actually measures. Its failures are silent
by nature: a template that expands wrong, a seed range that is not shared
across matchups, an opponent set quietly missing a class -- none of those look
like errors, they look like results. Worse, they surface hours later as an
aggregated win rate nobody can reproduce.

So the enumeration is pinned by construction: exact team sets, exact opponent
counts, exact seed ranges, and the property that every matchup is played on the
SAME seeds (a paired comparison is the only thing that makes two cells
comparable).

Pure stdout and argv; the module's `subprocess` is replaced with a guard.

Run directly, or via `cargo test --test sweep_script_fixtures`:

    python3 scripts/tests/test_gen_sweep.py
"""

from __future__ import annotations

import json
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from sweep_fixtures import (  # noqa: E402
    FixtureTestCase,
    assert_runs_on_min_python,
    install_no_subprocess,
    run_main,
)

import gen_sweep as gen  # noqa: E402

install_no_subprocess(gen)


class TemplateTests(unittest.TestCase):
    def test_a_concrete_template_is_one_team(self):
        self.assertEqual(list(gen.expand_t1("Hunter+Priest")), [["Hunter", "Priest"]])

    def test_the_wildcard_expands_over_every_class(self):
        teams = list(gen.expand_t1("{p}"))
        self.assertEqual([t[0] for t in teams], gen.CLASSES)
        self.assertEqual(len(teams), 8)

    def test_the_wildcard_skips_a_duplicate_of_the_fixed_slot(self):
        teams = list(gen.expand_t1("Hunter+{p}"))
        self.assertEqual(len(teams), 7)
        self.assertNotIn(["Hunter", "Hunter"], teams)
        self.assertIn(["Hunter", "Priest"], teams)

    def test_the_wildcard_keeps_its_slot_position(self):
        teams = list(gen.expand_t1("{p}+Hunter"))
        self.assertTrue(all(t[1] == "Hunter" for t in teams))

    def test_two_wildcards_are_rejected(self):
        """One is expanded and the other stays a literal '{p}' class name.

        That config reaches the simulator as a team containing a class that
        does not exist -- a whole arm of a sweep, gone, with nothing said.
        """
        with self.assertRaises(ValueError) as caught:
            list(gen.expand_t1("{p}+{p}"))
        self.assertIn("{p}", str(caught.exception))

    def test_an_unknown_class_is_rejected(self):
        with self.assertRaises(ValueError) as caught:
            list(gen.expand_t1("Huntre+Priest"))
        self.assertIn("Huntre", str(caught.exception))


class OpponentEnumerationTests(unittest.TestCase):
    def test_size_one_is_every_class(self):
        opps = list(gen.enumerate_opponents(1, False, False))
        self.assertEqual([o[0] for o in opps], gen.CLASSES)

    def test_size_two_is_every_distinct_unordered_pair(self):
        opps = list(gen.enumerate_opponents(2, False, False))
        self.assertEqual(len(opps), 28)  # C(8,2)
        self.assertEqual(len({tuple(sorted(o)) for o in opps}), 28)
        self.assertTrue(all(len(set(o)) == 2 for o in opps))

    def test_excluding_double_healers_drops_the_healer_pairs(self):
        opps = list(gen.enumerate_opponents(2, True, False))
        self.assertEqual(len(opps), 25)  # 28 - C(3,2)
        for o in opps:
            self.assertLessEqual(sum(1 for c in o if c in gen.HEALERS), 1)

    def test_all_healer_exclusion_needs_a_team(self):
        """A lone healer is a legitimate 1v1 opponent, not an all-healer comp."""
        opps = list(gen.enumerate_opponents(1, False, True))
        self.assertIn(["Priest"], opps)
        triples = list(gen.enumerate_opponents(3, False, True))
        self.assertNotIn(["Priest", "Paladin", "Shaman"], [sorted(t) for t in triples])


class GenTestCase(FixtureTestCase):
    def gen(self, *argv):
        return run_main(gen.main, list(argv))

    def configs(self, run):
        return [json.loads(line) for line in run.out.splitlines() if line.strip()]


class OutputTests(GenTestCase):
    def test_one_line_per_matchup_per_seed(self):
        run = self.assertOk(self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "5"))
        cfgs = self.configs(run)
        self.assertEqual(len(cfgs), 8 * 5)
        self.assertErrHas(run, "# wrote 40 match configs")

    def test_the_reported_count_is_the_real_count(self):
        run = self.assertOk(
            self.gen("--t1", "Hunter+{p}", "--t2-size", "2", "--n", "2",
                     "--exclude-double-healer")
        )
        cfgs = self.configs(run)
        self.assertErrHas(run, "# wrote %d match configs" % len(cfgs))
        self.assertEqual(len(cfgs), 7 * 25 * 2)

    def test_every_config_carries_the_fields_the_runner_needs(self):
        run = self.assertOk(self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "1"))
        for cfg in self.configs(run):
            self.assertEqual(cfg["team1"], ["Hunter"])
            self.assertEqual(len(cfg["team2"]), 1)
            self.assertEqual(cfg["max_duration_secs"], 300.0)
            self.assertIn("random_seed", cfg)
            self.assertEqual(cfg["label"], "Hunter_vs_" + cfg["team2"][0])

    def test_every_matchup_is_played_on_the_same_seeds(self):
        run = self.assertOk(
            self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "4", "--seed-base", "7")
        )
        seeds = {}
        for cfg in self.configs(run):
            seeds.setdefault(cfg["label"], []).append(cfg["random_seed"])
        self.assertEqual(len(seeds), 8)
        for label, s in seeds.items():
            self.assertEqual(s, [7, 8, 9, 10], "matchup %s drifted off the seed set" % label)

    def test_cap_is_settable_but_defaults_to_300(self):
        run = self.assertOk(
            self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "1", "--cap", "120")
        )
        self.assertTrue(all(c["max_duration_secs"] == 120.0 for c in self.configs(run)))

    def test_t2_size_defaults_to_the_team1_size(self):
        run = self.assertOk(self.gen("--t1", "Hunter+Priest", "--n", "1"))
        self.assertTrue(all(len(c["team2"]) == 2 for c in self.configs(run)))

    def test_label_suffix_keeps_variants_distinct(self):
        run = self.assertOk(
            self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "1",
                     "--label-suffix", "Spider")
        )
        self.assertTrue(all(c["label"].endswith("#Spider") for c in self.configs(run)))

    def test_extra_is_merged_into_every_config(self):
        run = self.assertOk(
            self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "1",
                     "--extra", '{"team1_hunter_pet_types": ["Spider"]}')
        )
        cfgs = self.configs(run)
        self.assertTrue(all(c["team1_hunter_pet_types"] == ["Spider"] for c in cfgs))
        self.assertTrue(all(c["team1"] == ["Hunter"] for c in cfgs))


class FullMatrixTests(GenTestCase):
    def test_full_1v1_is_the_whole_square(self):
        run = self.assertOk(self.gen("--full", "1", "--n", "1"))
        cfgs = self.configs(run)
        self.assertEqual(len(cfgs), 64)  # 8x8, both orderings and mirrors
        pairs = {(c["team1"][0], c["team2"][0]) for c in cfgs}
        self.assertIn(("Warrior", "Mage"), pairs)
        self.assertIn(("Mage", "Warrior"), pairs)
        self.assertIn(("Mage", "Mage"), pairs)

    def test_full_ignores_t1(self):
        run = self.assertOk(self.gen("--full", "1", "--t1", "Hunter", "--n", "1"))
        self.assertEqual(len(self.configs(run)), 64)

    def test_full_respects_exclude_double_healer(self):
        run = self.assertOk(self.gen("--full", "2", "--n", "1", "--exclude-double-healer"))
        cfgs = self.configs(run)
        self.assertEqual(len(cfgs), 25 * 25)
        for c in cfgs:
            for team in (c["team1"], c["team2"]):
                self.assertLessEqual(sum(1 for x in team if x in gen.HEALERS), 1)


class DegenerateInputTests(GenTestCase):
    def test_neither_t1_nor_full_is_an_error(self):
        run = self.gen("--n", "1")
        self.assertEqual(run.code, 2)

    def test_zero_seeds_generates_nothing_and_says_so(self):
        """An empty sweep must fail here, not three steps downstream.

        `--n 0` writes an empty JSONL; the batch runner then produces an empty
        CSV, and the first thing anyone sees is an aggregation error about a
        file that looks fine.
        """
        run = self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "0")
        self.assertIsInstance(run.code, str)
        self.assertIn("no match configs", run.code)
        self.assertEqual(run.out, "")

    def test_an_extra_that_would_overwrite_the_generated_fields_is_rejected(self):
        """--extra is a shallow merge, so it can silently rewrite a team.

        The label would still name the team the generator chose, and the
        aggregation downstream groups by label: every matchup would report
        under the wrong name.
        """
        run = self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "1",
                       "--extra", '{"team1": ["Mage"]}')
        self.assertIsInstance(run.code, str)
        self.assertIn("team1", run.code)

    def test_a_malformed_extra_is_reported(self):
        run = self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "1", "--extra", "{oops")
        self.assertIsInstance(run.code, str)
        self.assertIn("--extra", run.code)

    def test_an_extra_that_is_not_an_object_is_rejected(self):
        run = self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "1", "--extra", "[1,2]")
        self.assertIsInstance(run.code, str)
        self.assertIn("--extra", run.code)

    def test_a_misspelled_class_is_rejected(self):
        run = self.gen("--t1", "Huntre", "--t2-size", "1", "--n", "1")
        self.assertIsInstance(run.code, str)
        self.assertIn("Huntre", run.code)

    def test_a_single_seed_still_generates(self):
        run = self.assertOk(self.gen("--t1", "Hunter", "--t2-size", "1", "--n", "1"))
        self.assertEqual(len(self.configs(run)), 8)


class InterpreterFloorTests(unittest.TestCase):
    """`gen_sweep.py` must still import on the stock system interpreter."""

    def test_the_tool_runs_on_the_minimum_interpreter(self):
        assert_runs_on_min_python(self, gen)


if __name__ == "__main__":
    unittest.main(verbosity=2)
