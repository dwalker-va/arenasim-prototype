#!/usr/bin/env python3
"""Offline fixture tests for the AS-302 arms script.

`docs/design/balance/sweeps/2026-11-23-as302-arms.py` generated the Paladin
node-arm sweep and prints its verdicts. Two things it does are pinned here:

- **The verdict rule.** A node's margin over its allowance, and that margin's
  z, come from the per-match paired margin
  x_i = 100 * [(A_i - B_i) - k * (V+_i - V0_i)], se = sd(x)/sqrt(n), on
  hand-built CSVs where a quadrature sum of the two deltas' errors gives a
  different z. A node dominates only past its floor AND at a margin z that
  survives Bonferroni.
- **The arms the sweep ran.** Each node comparison is two arms one point
  apart, on the node and on Sacred Duty, read from the script's own `ARMS`
  and `NODE_ARMS`; only the Retribution Aura arm chooses an aura; and the
  script's arms equal `PALADIN_ARMS` in `tests/reference_builds.rs`, which
  `cargo test` resolves against the shipped talents.

Offline by construction: the module's `subprocess` is replaced with a guard.

Run directly, or via `cargo test --test script_fixture_suites`:

    python3 scripts/tests/test_as302_arms.py
"""

from __future__ import annotations

import csv
import importlib.util
import math
import os
import re
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from _harness import assert_file_runs_on_min_python, install_no_subprocess  # noqa: E402

ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
ARMS = os.path.join(ROOT, "docs", "design", "balance", "sweeps", "2026-11-23-as302-arms.py")
RUST_TEST = os.path.join(ROOT, "tests", "reference_builds.rs")
_spec = importlib.util.spec_from_file_location("as302_arms", ARMS)
arms = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(arms)
install_no_subprocess(arms)


# ---------------------------------------------------------------------------
# The verdict rule
# ---------------------------------------------------------------------------

def write_arm(directory, name, winners):
    """One arm's batch CSV: `winners` is a list of (team1, team2, winner)."""
    with open(os.path.join(directory, name + ".csv"), "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["label", "team1", "team2", "seed", "winner", "end_reason", "duration_secs"])
        for i, (t1, t2, winner) in enumerate(winners):
            w.writerow(["%s_vs_%s#M" % (t1, t2), t1, t2, i, winner, "kill", 30.0 + i])


# Ten one-sided matches (the Paladin on team1 in six, team2 in four), one
# control match and one mirror. Columns: the arm (A), its comparison (B), the
# stem with a Sacred Duty rank (V+) and the stem (V0), each as the winning team.
CLEAN, AGAINST = ("Warrior+Paladin", "Mage+Priest"), ("Rogue+Druid", "Hunter+Paladin")
MATCHES = [
    (CLEAN, ("team1", "team2", "team1", "team2")),
    (CLEAN, ("team1", "team2", "team1", "team1")),
    (CLEAN, ("team1", "team1", "team2", "team1")),
    (CLEAN, ("team1", "team2", "team2", "team2")),
    (CLEAN, ("team2", "team2", "team1", "team2")),
    (CLEAN, ("team1", "team2", "team1", "team2")),
    (AGAINST, ("team2", "team1", "team2", "team1")),
    (AGAINST, ("team2", "team1", "team1", "team1")),
    (AGAINST, ("team2", "team2", "team2", "team1")),
    (AGAINST, ("team1", "team2", "team2", "team2")),
    # control (no Paladin) and mirror (a Paladin each side): never scored
    (("Warrior+Mage", "Rogue+Priest"), ("team1", "team2", "team2", "team1")),
    (("Warrior+Paladin", "Mage+Paladin"), ("team1", "team2", "team2", "team1")),
]
PRICE = 22.0  # a capstone: k = 22 / 13.44 - 1


def hand_xs():
    """The per-match margins, by hand: 1 when the Paladin side wins."""
    k = PRICE / arms.YARDSTICK_PRICE - 1.0
    xs = []
    for (t1, t2), winners in MATCHES[:10]:
        side = "team1" if "Paladin" in t1 else "team2"
        a, b, vp, v0 = (1.0 if w == side else 0.0 for w in winners)
        xs.append(100.0 * ((a - b) - k * (vp - v0)))
    return xs


class MarginTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="as302-margin-")
        for col, name in enumerate(("a", "b", "vp", "v0")):
            write_arm(self.dir, name, [(t1, t2, w[col]) for (t1, t2), w in MATCHES])
        self.saved = arms.STAT_ARMS
        arms.STAT_ARMS = {"s": ("vp", "v0")}

    def tearDown(self):
        arms.STAT_ARMS = self.saved
        for name in os.listdir(self.dir):
            os.remove(os.path.join(self.dir, name))
        os.rmdir(self.dir)

    def test_the_margin_and_its_z_are_the_per_match_paired_margins(self):
        xs = hand_xs()
        n = len(xs)
        mean = sum(xs) / n
        se = math.sqrt(sum((x - mean) ** 2 for x in xs) / (n - 1) / n)
        margin, got_se, z = arms.margin_stats(self.dir, {}, "a", "b", PRICE, "s")
        self.assertAlmostEqual(margin, mean, places=9)
        self.assertAlmostEqual(got_se, se, places=9)
        self.assertAlmostEqual(z, mean / se, places=9)

    def test_the_margin_is_the_delta_less_the_allowance(self):
        delta = arms.compare(self.dir, {}, "a", "b")[0].delta
        v = arms.compare(self.dir, {}, "vp", "v0")[0].delta
        margin, _, _ = arms.margin_stats(self.dir, {}, "a", "b", PRICE, "s")
        self.assertAlmostEqual(margin, delta - arms.allowance(PRICE, v), places=9)

    def test_the_z_is_not_a_quadrature_sum(self):
        """The four arms share each match, so the delta's and the stat rank's
        errors are correlated: summing them in quadrature gives another z."""
        k = PRICE / arms.YARDSTICK_PRICE - 1.0
        xs = hand_xs()
        n = len(xs)
        d = [100.0 * ((1.0 if w[0] == s else 0.0) - (1.0 if w[1] == s else 0.0))
             for (t1, _), w in MATCHES[:10] for s in ["team1" if "Paladin" in t1 else "team2"]]
        v = [100.0 * ((1.0 if w[2] == s else 0.0) - (1.0 if w[3] == s else 0.0))
             for (t1, _), w in MATCHES[:10] for s in ["team1" if "Paladin" in t1 else "team2"]]

        def se(ys):
            m = sum(ys) / n
            return math.sqrt(sum((y - m) ** 2 for y in ys) / (n - 1) / n)

        quadrature_z = (sum(xs) / n) / math.sqrt(se(d) ** 2 + (k * se(v)) ** 2)
        _, _, z = arms.margin_stats(self.dir, {}, "a", "b", PRICE, "s")
        self.assertGreater(abs(z - quadrature_z), 0.05)

    def test_every_margin_equal_is_no_error(self):
        self.assertEqual(arms.paired_mean_z([0.0, 0.0, 0.0]), (0.0, 0.0, 0.0))
        mean, se, z = arms.paired_mean_z([2.0, 2.0])
        self.assertEqual((mean, se), (2.0, 0.0))
        self.assertEqual(z, math.inf)


class VerdictTests(unittest.TestCase):
    T = arms.BONFERRONI_Z

    def test_the_threshold_is_bonferroni_over_the_node_comparisons(self):
        self.assertEqual(len(arms.NODE_ARMS), 12)
        self.assertAlmostEqual(self.T, 2.865, places=3)

    def test_dominates_needs_both_the_floor_and_the_corrected_z(self):
        self.assertEqual(arms.verdict(1.0, 0.6, self.T + 0.01), "DOMINATES")
        self.assertEqual(arms.verdict(1.0, 0.6, self.T - 0.01),
                         "past the floor, not after correction")
        self.assertEqual(arms.verdict(0.5, 0.6, 10.0), "within")

    def test_below_mirrors_it(self):
        self.assertEqual(arms.verdict(-1.0, 0.6, -self.T - 0.01), "below")
        self.assertEqual(arms.verdict(-1.0, 0.6, -self.T + 0.01),
                         "below the floor, not after correction")
        self.assertEqual(arms.verdict(-0.5, 0.6, -10.0), "within")

    def test_no_flips_is_unresolvable(self):
        self.assertEqual(arms.verdict(0.0, None, 0.0), "unresolvable")


# ---------------------------------------------------------------------------
# The arms the sweep ran
# ---------------------------------------------------------------------------

# Each node comparison's label -> the talent its arm holds one more rank of.
EXPECTED_NODES = {
    "Infusion of Light (rank 1)": "InfusionOfLight",
    "Divine Favor": "DivineFavor",
    "Light's Grace": "LightsGrace",
    "Aura Mastery (capstone)": "AuraMastery",
    "Divine Protection": "DivineProtection",
    "Retribution Aura (rank 1, chosen)": "RetributionAura",
    "Blessing of Sacrifice": "BlessingOfSacrifice",
    "Sacred Shield (capstone)": "SacredShield",
    "Vengeance (rank 2)": "Vengeance",
    "The Art of War": "TheArtOfWar",
    "Repentance": "Repentance",
    "Crusader Strike (capstone)": "CrusaderStrike",
}
CAPSTONES = {"AuraMastery", "SacredShield", "CrusaderStrike"}


def spec_of(arm):
    """(reference, remove, add) of a script arm."""
    spec = arms.ARMS[arm][0]
    if isinstance(spec, str):
        return spec, {}, {}
    return spec["reference"], dict(spec.get("remove", {})), dict(spec.get("add", {}))


def net(arm):
    """What an arm moves off its reference: talent -> ranks (+ added)."""
    _, remove, add = spec_of(arm)
    out = {}
    for t, r in add.items():
        out[t] = out.get(t, 0) + r
    for t, r in remove.items():
        out[t] = out.get(t, 0) - r
    return out


class ArmsTests(unittest.TestCase):
    def test_the_node_comparisons_are_the_twelve_card_nodes(self):
        self.assertEqual({label for label, *_ in arms.NODE_ARMS}, set(EXPECTED_NODES))

    def test_each_node_arm_is_its_comparison_with_one_point_moved(self):
        for label, price, arm, base, stem in arms.NODE_ARMS:
            node = EXPECTED_NODES[label]
            self.assertEqual(spec_of(arm)[0], spec_of(base)[0], label)
            a, b = net(arm), net(base)
            diff = {t: a.get(t, 0) - b.get(t, 0) for t in set(a) | set(b)}
            diff = {t: d for t, d in diff.items() if d}
            self.assertEqual(diff, {node: 1, "SacredDuty": -1}, label)
            self.assertEqual(price, (22.0 if node in CAPSTONES else 11.0), label)
            self.assertEqual(arms.STAT_ARMS[stem][1], stem + "_stem", label)
            self.assertTrue(arm.startswith(stem) and base.startswith(stem), label)

    def test_only_the_retribution_aura_arm_chooses_an_aura(self):
        chosen = {name: choices for name, (_, choices) in arms.ARMS.items() if choices}
        self.assertEqual(chosen, {"prot_ra": [("paladin_auras", "Retribution")]})
        ra = [(arm, base) for label, _, arm, base, _ in arms.NODE_ARMS
              if EXPECTED_NODES[label] == "RetributionAura"]
        self.assertEqual(ra, [("prot_ra", "prot_y")])
        self.assertEqual(spec_of("prot_ra_dev"), spec_of("prot_ra"))

    def test_the_script_arms_are_the_rust_tests_arms(self):
        """`PALADIN_ARMS` in tests/reference_builds.rs, which `cargo test`
        resolves against the shipped talents, is the script's `ARMS` but
        `zero`: same names, references, ranks moved and auras."""
        text = open(RUST_TEST, encoding="utf-8").read()
        table = text[text.index("const PALADIN_ARMS"):]
        table = table[:table.index("\n];") + 3]
        entry = re.compile(
            r'arm\(\s*"(\w+)",\s*"(\w+)",\s*&\[([^\]]*)\],\s*&\[([^\]]*)\],?\s*\)')
        pair = re.compile(r"\((\w+),\s*(\d+)\)")
        rust = {}
        for m in entry.finditer(table):
            name, reference, remove, add = m.groups()
            rust[name] = (reference,
                          {t: int(r) for t, r in pair.findall(remove)},
                          {t: int(r) for t, r in pair.findall(add)}, None)
        for aura, name in re.findall(
                r'aura:\s*Some\("(\w+)"\),\s*\.\.arm\(\s*"(\w+)"', table):
            reference, remove, add, _ = rust[name]
            rust[name] = (reference, remove, add, aura)
        script = {}
        for name, (spec, choices) in arms.ARMS.items():
            if spec is None:
                continue
            reference, remove, add = spec_of(name)
            aura = dict(choices).get("paladin_auras")
            script[name] = (reference, remove, add, aura)
        self.assertEqual(rust, script)


class MinPythonTests(unittest.TestCase):
    def test_the_arms_script_runs_on_the_floor_interpreter(self):
        assert_file_runs_on_min_python(self, ARMS)


if __name__ == "__main__":
    unittest.main()
