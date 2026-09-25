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

Run directly, or via `cargo test --test script_fixture_suites`:

    python3 scripts/tests/test_gen_sweep.py
"""

from __future__ import annotations

import itertools
import json
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from _harness import assert_runs_on_min_python, install_no_subprocess, run_main  # noqa: E402
from sweep_fixtures import FixtureTestCase  # noqa: E402

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


class AffectsTests(GenTestCase):
    """The directional tier's one lever: cut CELLS, keep the seeds.

    What makes this dangerous rather than merely convenient is that a sweep
    which quietly dropped most of its matrix still produces a perfectly
    plausible aggregate. So the cut is pinned on both sides -- every reachable
    cell kept, exactly the requested number of control cells -- and the counts
    it prints are checked against what it actually wrote.
    """

    def cells(self, run):
        return set((tuple(c["team1"]), tuple(c["team2"])) for c in self.configs(run))

    def test_every_reachable_cell_survives_the_cut(self):
        full = self.assertOk(
            self.gen("--full", "2", "--exclude-double-healer", "--n", "1"))
        cut = self.assertOk(
            self.gen("--full", "2", "--exclude-double-healer", "--n", "1",
                     "--affects", "Shaman"))
        reachable = set(c for c in self.cells(full)
                        if "Shaman" in c[0] or "Shaman" in c[1])
        self.assertTrue(reachable)
        self.assertTrue(reachable <= self.cells(cut))

    def test_the_seeds_are_untouched_by_the_cell_cut(self):
        """The rule the tier rests on: cells go, seeds stay."""
        run = self.assertOk(
            self.gen("--full", "2", "--exclude-double-healer", "--n", "7",
                     "--affects", "Shaman"))
        per_cell = {}
        for cfg in self.configs(run):
            key = (tuple(cfg["team1"]), tuple(cfg["team2"]))
            per_cell.setdefault(key, set()).add(cfg["random_seed"])
        self.assertEqual(set(map(frozenset, per_cell.values())),
                         {frozenset(range(7))})

    def test_the_control_is_exactly_as_many_cells_as_asked_for(self):
        run = self.assertOk(
            self.gen("--full", "2", "--exclude-double-healer", "--n", "1",
                     "--affects", "Shaman", "--control-cells", "5"))
        control = set(c for c in self.cells(run)
                      if "Shaman" not in c[0] and "Shaman" not in c[1])
        self.assertEqual(len(control), 5)
        self.assertErrHas(run, "kept 225 reachable + 5 control of 625 cells")

    def test_the_control_spreads_over_both_team_slots(self):
        """A constant stride aliases against the nested enumeration.

        Even spacing over the 400 Shaman-free 2v2 cells returned eight
        controls sharing two distinct opponents, because the stride was a
        multiple of the inner loop's length. The digest ordering is what fixes
        that, and this is the assertion that would have caught it.

        The selection is pinned member by member rather than by a spread
        floor. A `>=` floor reads as a guard but cannot say which cells were
        chosen, so a reordering that preserved the count while degrading the
        spread would pass it. Changing the digest or the enumeration is
        allowed -- it just has to be a deliberate re-bless of this list.
        """
        run = self.assertOk(
            self.gen("--full", "2", "--exclude-double-healer", "--n", "1",
                     "--affects", "Shaman", "--control-cells", "8"))
        control = [c for c in self.cells(run)
                   if "Shaman" not in c[0] and "Shaman" not in c[1]]
        self.assertEqual(set(control), {
            (("Warrior", "Mage"), ("Mage", "Warlock")),
            (("Warrior", "Warlock"), ("Warrior", "Priest")),
            (("Warrior", "Warlock"), ("Rogue", "Hunter")),
            (("Mage", "Warlock"), ("Warrior", "Mage")),
            (("Rogue", "Priest"), ("Warrior", "Priest")),
            (("Rogue", "Warlock"), ("Warlock", "Paladin")),
            (("Priest", "Hunter"), ("Warlock", "Paladin")),
            (("Warlock", "Paladin"), ("Warrior", "Hunter")),
        })
        # Two claims, not one. The set above pins WHICH cells and needs a
        # human to re-bless it; this pins what a control OWES, so it keeps
        # judging a re-blessed list instead of being re-derived from it. The
        # same duty is asserted for every --affects class, shape and accepted
        # count in ControlCoverageTests below; this is its instance at the
        # call the set pins.
        owed = set(c for c in gen.CLASSES if c != "Shaman")
        for side, label in ((0, "team1"), (1, "opponent")):
            covered = set(x for c in control for x in c[side])
            self.assertEqual(
                covered, owed,
                "a leak into %s would be invisible: the %s side of the control "
                "set never exercises it" % (sorted(owed - covered), label))

    def test_the_same_arguments_regenerate_the_same_control(self):
        """The two arms of a paired run may generate the sweep separately."""
        argv = ["--full", "2", "--exclude-double-healer", "--n", "1",
                "--affects", "Shaman", "--control-cells", "6"]
        self.assertEqual(self.assertOk(self.gen(*argv)).out,
                         self.assertOk(self.gen(*argv)).out)

    def test_asking_for_more_control_cells_than_exist_keeps_them_all(self):
        # 8x8 1v1 cells; 15 hold a Shaman on one side or the other, 49 do not.
        run = self.assertOk(
            self.gen("--t1", "{p}", "--t2-size", "1", "--n", "1",
                     "--affects", "Shaman", "--control-cells", "9999"))
        control = [c for c in self.cells(run)
                   if "Shaman" not in c[0] and "Shaman" not in c[1]]
        self.assertEqual(len(control), 49)
        self.assertErrHas(run, "kept 15 reachable + 49 control of 64 cells")

    def test_dropping_the_control_is_allowed_but_warned_about(self):
        run = self.assertOk(
            self.gen("--full", "2", "--exclude-double-healer", "--n", "1",
                     "--affects", "Shaman", "--control-cells", "0"))
        self.assertEqual(self.cells(run),
                         set(c for c in self.cells(run)
                             if "Shaman" in c[0] or "Shaman" in c[1]))
        self.assertErrHas(run, "WARNING: no control cells")

    def test_a_change_that_reaches_nothing_leaves_a_sweep_measuring_nothing(self):
        """Pinned at the function, because no flag combination reaches it yet.

        Every team2 enumeration spans all eight classes, so a class named in
        --affects always turns up somewhere. The guard is here for the first
        flag that narrows the opponent set: without it the run would emit a
        control-only sweep and read as a clean null.
        """
        cells = [(["Mage"], ["Priest"]), (["Rogue"], ["Warrior"])]
        kept, reachable, control = gen.select_cells(cells, {"Shaman"}, 0)
        self.assertEqual((kept, reachable, control), ([], 0, 0))

    def test_an_unknown_affected_class_is_rejected(self):
        run = self.gen("--full", "2", "--n", "1", "--affects", "Druid")
        self.assertIsInstance(run.code, str)
        self.assertIn("Druid", run.code)

    def test_without_affects_nothing_is_cut(self):
        plain = self.assertOk(
            self.gen("--full", "2", "--exclude-double-healer", "--n", "1"))
        self.assertEqual(len(self.cells(plain)), 625)
        self.assertErrHas(plain, "# wrote 625 match configs")


def product_cells(team1_set, team2_set):
    """Cells the way main() builds them: every team1 against every opponent."""
    return [(list(a), list(b)) for a in team1_set for b in team2_set]


# Every cell shape the generator can build, from its own enumeration functions
# rather than from a list of expected teams. The full matrices at each size,
# with and without the healer exclusions, plus the two template shapes whose
# team1 side is narrower than the roster (a pinned class, and a wildcard).
def _shapes():
    shapes = {}
    for size in (1, 2, 3):
        for edh in (False, True):
            teams = list(gen.enumerate_opponents(size, edh, False))
            shapes["--full %d%s" % (size, " --exclude-double-healer" if edh else "")] = \
                product_cells(teams, teams)
    shapes["--t1 {p} --t2-size 1"] = product_cells(
        gen.expand_t1("{p}"), gen.enumerate_opponents(1, False, True))
    shapes["--t1 Hunter+{p} --t2-size 2 --exclude-double-healer"] = product_cells(
        gen.expand_t1("Hunter+{p}"), gen.enumerate_opponents(2, True, True))
    return shapes


SHAPES = _shapes()


def unreachable(cells, affected):
    return [c for c in cells if not gen.reaches(c[0], c[1], affected)]


def fielded(cells, side):
    return set(x for c in cells for x in c[side])


def brute_force_floor(cells, side):
    """Fewest distinct teams on `side` fielding every class that side fields.

    Independent of the tool's dynamic programme: plain enumeration of team
    subsets, smallest first.
    """
    teams = sorted(set(tuple(c[side]) for c in cells))
    universe = fielded(cells, side)
    for k in range(1, len(teams) + 1):
        for combo in itertools.combinations(teams, k):
            if set(x for t in combo for x in t) == universe:
                return k
    raise AssertionError("no cover at all")


class ControlCoverageTests(unittest.TestCase):
    """AS-143: the control owes every class the change cannot reach, on BOTH sides.

    Controls are the cells the change cannot reach, and paired_sweep.py reads
    them as a bit-exactness claim: if the change leaked, they are what says so.
    A class absent from them on a side is a class a leak into that side would
    be invisible in -- and the control would still pass, quietly, and be cited
    as evidence the change stayed home. The first sampler's 8 controls on 2
    distinct opponents covered 3 of the 7 classes; its digest-ordered successor
    missed Warrior on team1 and Rogue on the opponent side at --affects Warlock
    and the default count, because a uniform 8-cell sample of the 2v2 matrix
    under-covers about 70% of the time and the digest was one draw from that.

    Coverage rather than a count of distinct comps, because a comp count
    states no duty at all -- it drifts with the control count -- so pinning
    one would pin a call rather than the obligation.

    These assert the DUTY, over every --affects class, every shape above and
    every count the tool accepts. What a side owes is derived from the cells
    it could have been given, never from a class list.

    "Every count" is split at K0, the first length at which the plain digest
    prefix covers both sides. Below K0 every count is checked one by one --
    that is where the repair does its work. From K0 up, the tool's result IS
    the prefix, and a longer prefix fields everything a shorter one did, so
    coverage there follows from that equality; the equality is checked at
    every count for 16 past K0 and at a stride to the end of the range (each
    call costs a full digest pass, and the 3v3 matrix has 1,225 unreachable
    cells per class). Above the whole unreachable set a count keeps every cell.
    """

    @classmethod
    def setUpClass(cls):
        # One sampler call per (shape, class, count), shared by the two tests
        # that judge it: each call is a full digest pass over up to 1,225 cells.
        cls.picks = {}
        for shape, klass, un in cls.each_case():
            below, above = cls.counts(un)
            for keep in below + above:
                cls.picks[(shape, klass, keep)] = gen.sample_spread(un, keep)

    @staticmethod
    def each_case():
        for shape, cells in sorted(SHAPES.items()):
            for cls in gen.CLASSES:
                un = unreachable(cells, {cls})
                if un:
                    yield shape, cls, un

    @staticmethod
    def owed(un):
        return (fielded(un, 0), fielded(un, 1))

    @classmethod
    def first_covering_prefix(cls, un):
        order, owed = gen.digest_order(un), cls.owed(un)
        for k in range(1, len(un) + 1):
            chosen = [un[i] for i in order[:k]]
            if (fielded(chosen, 0), fielded(chosen, 1)) == owed:
                return k
        raise AssertionError("the whole set does not cover itself")

    @classmethod
    def counts(cls, un):
        """(counts below K0, counts from K0 up) that the tool accepts."""
        floor, k0, n = gen.control_floor(un)[0], cls.first_covering_prefix(un), len(un)
        above = set(range(k0, min(n, k0 + 16) + 1)) | set(range(k0, n + 1, 17)) | {n}
        return list(range(floor, k0)), sorted(c for c in above if c >= floor)

    def assert_covers(self, shape, cls, un, keep):
        picked = self.picks[(shape, cls, keep)]
        self.assertEqual(len(set(picked)), keep)
        chosen = [un[i] for i in picked]
        for side, label in ((0, "team1"), (1, "opponent")):
            owed = self.owed(un)[side]
            self.assertEqual(
                fielded(chosen, side), owed,
                "%s --affects %s --control-cells %d: a leak into %s would be "
                "invisible, the %s side of the control never fields it"
                % (shape, cls, keep, sorted(owed - fielded(chosen, side)), label))
        return picked

    def test_every_accepted_count_fields_every_unreachable_class_on_both_sides(self):
        judged = set()
        for shape, cls, un in self.each_case():
            below, above = self.counts(un)
            for keep in below + above:
                self.assert_covers(shape, cls, un, keep)
                judged.add((shape, cls, keep))
        # The AS-143 cases by name: Warlock at the default, and at 12.
        for keep in (8, 12):
            self.assertIn(("--full 2 --exclude-double-healer", "Warlock", keep), judged)

    def test_a_control_that_already_covered_is_left_exactly_as_it_was(self):
        """Coverage is repaired, never redrawn.

        Where the plain digest prefix already covered, the result IS that
        prefix, so every invocation whose control already did its job emits
        the sweep it always did -- and a committed JSONL regenerated from its
        own command still pairs with the one it was committed beside.
        """
        preserved, repaired = set(), set()
        for shape, cls, un in self.each_case():
            order = gen.digest_order(un)
            below, above = self.counts(un)
            for keep in below:
                self.assertNotEqual(self.picks[(shape, cls, keep)], sorted(order[:keep]))
                repaired.add((shape, cls, keep))
            for keep in above:
                self.assertEqual(self.picks[(shape, cls, keep)], sorted(order[:keep]),
                                 "%s --affects %s --control-cells %d" % (shape, cls, keep))
                preserved.add((shape, cls, keep))
        # Both branches exercised, by name: the AS-143 case was repaired, and
        # the call test_the_control_spreads_over_both_team_slots pins was not.
        self.assertIn(("--full 2 --exclude-double-healer", "Warlock", 8), repaired)
        self.assertIn(("--full 2 --exclude-double-healer", "Shaman", 8), preserved)

    def test_a_full_matrix_owes_every_class_but_the_affected_one(self):
        """The derived duty, spelled out where the roster is the whole answer."""
        for shape, cls, un in self.each_case():
            if not shape.startswith("--full"):
                continue
            owed = set(gen.CLASSES) - {cls}
            self.assertEqual(fielded(un, 0), owed, shape)
            self.assertEqual(fielded(un, 1), owed, shape)
            chosen = [un[i] for i in gen.sample_spread(un, 8)]
            self.assertEqual((fielded(chosen, 0), fielded(chosen, 1)), (owed, owed),
                             "%s --affects %s at the default count" % (shape, cls))

    def test_a_multi_class_change_is_covered_too(self):
        for shape in ("--full 1", "--full 2 --exclude-double-healer"):
            for pair in itertools.combinations(gen.CLASSES, 2):
                un = unreachable(SHAPES[shape], set(pair))
                floor = gen.control_floor(un)[0]
                for keep in sorted(set((floor, max(floor, 8)))):
                    chosen = [un[i] for i in gen.sample_spread(un, keep)]
                    self.assertEqual(
                        (fielded(chosen, 0), fielded(chosen, 1)),
                        (fielded(un, 0), fielded(un, 1)),
                        "%s --affects %s --control-cells %d" % (shape, ",".join(pair), keep))

    def test_sample_spread_refuses_a_count_below_the_floor(self):
        un = unreachable(SHAPES["--full 1"], {"Shaman"})
        floor = gen.control_floor(un)[0]
        with self.assertRaises(ValueError) as caught:
            gen.sample_spread(un, floor - 1)
        self.assertIn("fewest that can is %d" % floor, str(caught.exception))

    def test_cells_that_are_not_a_product_of_two_sides_are_refused(self):
        """The floor's arithmetic pairs any team1 cover with any opponent cover."""
        un = unreachable(SHAPES["--full 1"], {"Shaman"})
        with self.assertRaises(ValueError) as caught:
            gen.sample_spread(un[:-1], 8)
        self.assertIn("full product", str(caught.exception))


class ControlFloorTests(unittest.TestCase):
    def test_the_floor_is_the_true_minimum_cover(self):
        """Checked against plain enumeration, not against the tool's own DP."""
        for shape, cls, un in ControlCoverageTests.each_case():
            f1, f2 = brute_force_floor(un, 0), brute_force_floor(un, 1)
            self.assertEqual(gen.control_floor(un), (max(f1, f2), f1, f2),
                             "%s --affects %s" % (shape, cls))


class ControlFloorCliTests(GenTestCase):
    def test_a_count_below_the_floor_is_refused_and_the_floor_is_named(self):
        floor = gen.control_floor(unreachable(SHAPES["--full 1"], {"Shaman"}))
        run = self.gen("--full", "1", "--n", "1", "--affects", "Shaman",
                       "--control-cells", str(floor[0] - 1))
        self.assertIsInstance(run.code, str)
        self.assertIn("the fewest control cells that can is %d (team1 needs %d, "
                      "the opponent side %d)" % floor, run.code)
        self.assertEqual(run.out, "")

    def test_the_floor_itself_is_accepted(self):
        floor = gen.control_floor(unreachable(SHAPES["--full 1"], {"Shaman"}))[0]
        run = self.assertOk(self.gen("--full", "1", "--n", "1", "--affects", "Shaman",
                                     "--control-cells", str(floor)))
        self.assertErrHas(run, "+ %d control" % floor)


class SampleSpreadTests(unittest.TestCase):
    CELLS = unreachable(SHAPES["--full 2 --exclude-double-healer"], {"Shaman"})

    def test_a_non_positive_keep_selects_nothing(self):
        self.assertEqual(gen.sample_spread(self.CELLS, 0), [])
        self.assertEqual(gen.sample_spread(self.CELLS, -4), [])

    def test_indices_come_back_in_enumeration_order(self):
        picked = gen.sample_spread(self.CELLS, 9)
        self.assertEqual(picked, sorted(picked))
        self.assertEqual(len(set(picked)), 9)

    def test_keeping_everything_is_every_index(self):
        self.assertEqual(gen.sample_spread(self.CELLS[:2], 5), [0, 1])


class ReachesTests(unittest.TestCase):
    def test_a_class_on_either_side_makes_the_cell_reachable(self):
        self.assertTrue(gen.reaches(["Shaman"], ["Mage"], {"Shaman"}))
        self.assertTrue(gen.reaches(["Mage"], ["Shaman"], {"Shaman"}))
        self.assertTrue(gen.reaches(["Shaman"], ["Shaman"], {"Shaman"}))

    def test_neither_side_is_unreachable(self):
        self.assertFalse(gen.reaches(["Mage"], ["Priest"], {"Shaman"}))

    def test_any_one_of_several_affected_classes_is_enough(self):
        self.assertTrue(gen.reaches(["Priest"], ["Mage"], {"Shaman", "Priest"}))


class InterpreterFloorTests(unittest.TestCase):
    """`gen_sweep.py` must still import on the stock system interpreter."""

    def test_the_tool_runs_on_the_minimum_interpreter(self):
        assert_runs_on_min_python(self, gen)


if __name__ == "__main__":
    unittest.main(verbosity=2)
