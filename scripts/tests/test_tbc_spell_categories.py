#!/usr/bin/env python3
"""Offline fixture tests for `scripts/tbc_spell_categories.py`.

The generator writes `tests/data/tbc_spell_categories.csv`, which
`tests/prevention_tbc_audit.rs` holds every ability's `prevention` to. The
audit can only check what the fixture says, so what the generator REFUSES to
write is pinned here: a stale EXPLICIT or EXCLUDED entry, an EXPLICIT spell its
build lacks. So is how it reads the client: ranks joined by (owner, name), a
split written with both values, a missing `SpellCategories` row read as None,
only class and pet skill lines in the inventory.

Each test writes a whole hand-built client into a temporary `--cache-root`
(one directory per build) and drives `main(argv)`; the module's `subprocess`
is the shared harness's stub, so a table the fixture forgot fails the test
rather than reaching wago.tools.

Run directly, or via `cargo test --test script_fixture_suites`:

    python3 scripts/tests/test_tbc_spell_categories.py
"""

from __future__ import annotations

import csv
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from _harness import (  # noqa: E402
    assert_runs_on_min_python,
    install_no_subprocess,
    run_main_in_temp_dir,
)

import db2_spell_sweep  # noqa: E402
import tbc_spell_categories as gen  # noqa: E402

install_no_subprocess(db2_spell_sweep)

TBC, WRATH = gen.TBC_BUILD, gen.WRATH_BUILD
MAGE = str(gen.CLASS_BIT["Mage"])
PRIEST = str(gen.CLASS_BIT["Priest"])
# Skill lines: a Mage line, a Priest line, a pet line, and a profession every
# class can learn from (CategoryID 11).
FROST, DISC, BOAR, ENGINEERING = "6", "613", "211", "202"
LINES = [
    (FROST, "Frost", "7"),
    (DISC, "Discipline", "7"),
    (BOAR, "Pet - Boar", "7"),
    (ENGINEERING, "Engineering", "11"),
]


def write(path, header, rows):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(header)
        w.writerows(rows)


def world(root, abilities, categories, wrath_names=(), wrath_categories=()):
    """`abilities`: (spell, name, line, mask). `categories`: (spell, prevention)."""
    tbc = os.path.join(root, TBC)
    write(
        os.path.join(tbc, "SkillLine.csv"),
        ["ID", "DisplayName_lang", "CategoryID"],
        [(i, n, c) for i, n, c in LINES],
    )
    write(
        os.path.join(tbc, "SkillLineAbility.csv"),
        ["ID", "SkillLine", "Spell", "ClassMask"],
        [(i, line, spell, mask) for i, (spell, _, line, mask) in enumerate(abilities)],
    )
    write(
        os.path.join(tbc, "SpellName.csv"),
        ["ID", "Name_lang"],
        [(spell, name) for spell, name, _, _ in abilities],
    )
    cat_header = ["ID", "DifficultyID", "PreventionType", "SpellID"]
    write(
        os.path.join(tbc, "SpellCategories.csv"),
        cat_header,
        [(i, "0", p, s) for i, (s, p) in enumerate(categories)],
    )
    wrath = os.path.join(root, WRATH)
    write(os.path.join(wrath, "SpellName.csv"), ["ID", "Name_lang"], list(wrath_names))
    write(
        os.path.join(wrath, "SpellCategories.csv"),
        cat_header,
        [(i, "0", p, s) for i, (s, p) in enumerate(wrath_categories)],
    )
    return ["--cache-root", root]


def rows_of(out):
    """(owner, name) -> (source, build, spells, prevention), from stdout."""
    lines = [l for l in out.splitlines() if not l.startswith("#")]
    assert lines[0] == gen.HEADER, lines[0]
    found = {}
    for line in lines[1:]:
        owner, source, build, spells, prevention, name = line.split(",", 5)
        found[(owner, name)] = (source, build, spells, prevention)
    return found


# The plain client every test starts from: Frostbolt in two ranks (one with no
# SpellCategories row), a talent with no class mask on the Priest's own line,
# a boar's Charge, a profession spell, and a name whose ranks split.
BASE = [
    ("116", "Frostbolt", FROST, MAGE),
    ("205", "Frostbolt", FROST, MAGE),
    ("33206", "Pain Suppression", DISC, PRIEST),
    ("10060", "Power Infusion", DISC, "0"),
    ("7371", "Charge", BOAR, "0"),
    ("3918", "Rough Blasting Powder", ENGINEERING, MAGE),
    ("120", "Cone of Cold", FROST, MAGE),
    ("8492", "Cone of Cold", FROST, MAGE),
]
BASE_CATEGORIES = [
    ("116", "1"),
    ("33206", "1"),
    ("10060", "1"),
    ("7371", "2"),
    ("120", "1"),
    ("8492", "0"),
]


class Case(unittest.TestCase):
    def setUp(self):
        self.explicit, self.excluded = gen.EXPLICIT, gen.EXCLUDED
        gen.EXPLICIT, gen.EXCLUDED = {}, {}

    def tearDown(self):
        gen.EXPLICIT, gen.EXCLUDED = self.explicit, self.excluded

    def run_gen(self, abilities=BASE, categories=BASE_CATEGORIES, **wrath):
        return run_main_in_temp_dir(
            gen.main, lambda root: world(root, abilities, categories, **wrath)
        )


class InventoryTests(Case):
    def test_ranks_join_by_owner_and_name(self):
        run = self.run_gen()
        self.assertTrue(run.ok, run)
        rows = rows_of(run.out)
        # 205 has no SpellCategories row, which reads as None: so Frostbolt's
        # ranks disagree, and the row says so.
        self.assertEqual(rows[("Mage", "Frostbolt")], ("name", TBC, "116 205", "None/Silence"))
        self.assertEqual(rows[("Mage", "Cone of Cold")][3], "None/Silence")
        self.assertEqual(rows[("Priest", "Pain Suppression")], ("name", TBC, "33206", "Silence"))
        self.assertEqual(rows[("Pet - Boar", "Charge")], ("name", TBC, "7371", "Pacify"))

    def test_a_talent_with_no_mask_joins_its_line_s_class(self):
        rows = rows_of(self.run_gen().out)
        self.assertEqual(rows[("Priest", "Power Infusion")][3], "Silence")
        self.assertNotIn(("Mage", "Power Infusion"), rows)

    def test_a_profession_is_not_a_class_spell(self):
        rows = rows_of(self.run_gen().out)
        self.assertFalse([k for k in rows if k[1] == "Rough Blasting Powder"], rows)

    def test_the_inventory_is_exactly_the_class_and_pet_rows(self):
        self.assertEqual(
            set(rows_of(self.run_gen().out)),
            {
                ("Mage", "Frostbolt"),
                ("Mage", "Cone of Cold"),
                ("Priest", "Pain Suppression"),
                ("Priest", "Power Infusion"),
                ("Pet - Boar", "Charge"),
            },
        )


class ExplicitTests(Case):
    def test_an_explicit_row_is_read_from_its_build(self):
        gen.EXPLICIT = {("Pet - Boar", "Boar Charge"): (TBC, ["7371"], "why")}
        gen.EXPLICIT[("Paladin", "Sacred Shield")] = (WRATH, ["53601"], "why")
        run = self.run_gen(
            wrath_names=[("53601", "Sacred Shield")], wrath_categories=[("53601", "1")]
        )
        self.assertTrue(run.ok, run)
        rows = rows_of(run.out)
        self.assertEqual(rows[("Pet - Boar", "Boar Charge")], ("explicit", TBC, "7371", "Pacify"))
        self.assertEqual(
            rows[("Paladin", "Sacred Shield")], ("explicit", WRATH, "53601", "Silence")
        )

    def test_an_explicit_name_that_joins_by_name_is_stale(self):
        gen.EXPLICIT = {("Mage", "Frostbolt"): (TBC, ["116"], "why")}
        run = self.run_gen()
        self.assertEqual(run.code, 1, run)
        self.assertIn("EXPLICIT ('Mage', 'Frostbolt') is stale", run.err)
        self.assertEqual(run.out, "", "nothing is written once a check fails")

    def test_an_explicit_spell_its_build_lacks_fails(self):
        gen.EXPLICIT = {("Paladin", "Sacred Shield"): (WRATH, ["53601"], "why")}
        run = self.run_gen()
        self.assertEqual(run.code, 1, run)
        self.assertIn("spells ['53601'] are not in build %s" % WRATH, run.err)
        self.assertEqual(run.out, "")


class ExcludedTests(Case):
    def test_an_excluded_spell_leaves_its_name_s_ranks(self):
        gen.EXCLUDED = {"8492": "why"}
        rows = rows_of(self.run_gen().out)
        self.assertEqual(rows[("Mage", "Cone of Cold")], ("name", TBC, "120", "Silence"))

    def test_an_excluded_spell_no_row_holds_is_stale(self):
        gen.EXCLUDED = {"44416": "why"}
        run = self.run_gen()
        self.assertEqual(run.code, 1, run)
        self.assertIn("EXCLUDED 44416 is stale", run.err)
        self.assertEqual(run.out, "")


class ShippedTablesTests(unittest.TestCase):
    """The shipped tables are well formed, whatever client they meet."""

    def test_every_explicit_row_names_a_known_build_and_spells(self):
        for key, (build, ids, why) in gen.EXPLICIT.items():
            self.assertIn(build, (TBC, WRATH), key)
            self.assertTrue(ids and all(i.isdigit() for i in ids), key)
            self.assertTrue(why, key)


class InterpreterFloorTests(unittest.TestCase):
    def test_the_tool_runs_on_the_minimum_interpreter(self):
        assert_runs_on_min_python(self, gen)


if __name__ == "__main__":
    unittest.main(verbosity=2)
