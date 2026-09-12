#!/usr/bin/env python3
"""Offline fixture tests for `scripts/db2_spell_sweep.py`.

The sweep script's value is entirely in what it REFUSES to claim: the counts
sum, the era-cut partition, and the property-4 HELD / SPLIT / LOW-PROB /
VACUOUS verdict. Every one of those lives in a reporting block, and until this
file existed the only way to exercise a reporting block was to run the script
against live wago.tools data and read the output by eye. That is how AS-49's
two reporting bugs shipped, and it is why a regression here was invisible.

These tests drive `main(argv)` over hand-built CSV fixtures. They do it without
touching the network at all, by two independent means:

  * the script's own cache is authoritative -- `fetch()` returns a cached CSV
    without a request -- so each test writes its whole world into a temporary
    `--cache-dir` under a fixture `--build`; and
  * the module's `subprocess` is replaced by `_NoNetwork`, so a fetch the
    fixture forgot to provide FAILS the test instead of silently reaching out
    to wago.tools (and then passing only on a machine with network).

Fixtures can also express worlds live data does not contain -- an unnamed spell
that resolves to two visuals is the case AS-57 was written about, unreachable
at build 1.15.9.69547 purely by accident of the data.

Run directly, or via `cargo test --test db2_spell_sweep_fixtures`:

    python3 scripts/tests/test_db2_spell_sweep.py
"""

from __future__ import annotations

import contextlib
import csv
import io
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import db2_spell_sweep as sweep_mod  # noqa: E402

FIXTURE_BUILD = "0.0.0-fixture"

# The default skill line the builder puts spells on, and its display name.
LINE = "900"
LINE_NAME = "Fixture Line"


class _NoNetwork:
    """Stands in for the module's `subprocess`. Any fetch is a test failure."""

    @staticmethod
    def run(cmd, **kwargs):  # pragma: no cover - only runs when a test is wrong
        raise AssertionError(
            "db2_spell_sweep attempted a network fetch (%r) -- the fixture is "
            "missing a table" % (cmd,)
        )


sweep_mod.subprocess = _NoNetwork


# The exact column names the script reads out of each DB2 CSV. Spelled out
# rather than derived, so a fixture that drifts from the real export shape
# fails here instead of quietly agreeing with a broken reader.
COLUMNS = {
    "SkillLine": ["ID", "DisplayName_lang"],
    "SkillLineAbility": ["ID", "SkillLine", "Spell"],
    "SpellName": ["ID", "Name_lang"],
    "SpellXSpellVisual": ["ID", "SpellID", "SpellVisualID", "Probability"],
    "SpellVisual": ["ID"],
    "SpellVisualEvent": [
        "ID",
        "SpellVisualID",
        "TargetType",
        "StartEvent",
        "EndEvent",
        "SpellVisualKitID",
    ],
    "SpellVisualKitEffect": ["ID", "ParentSpellVisualKitID", "EffectType", "Effect"],
    "SpellVisualAnim": ["ID", "LoopAnimID"],
}


class World:
    """A hand-built DB2 world, written out as the CSVs the script reads."""

    def __init__(self, skill_lines=None):
        self.skill_lines = dict(skill_lines or {LINE: LINE_NAME})
        # Every table but `SpellVisual`, which `write` DERIVES from the visual
        # ids the spells claim. It is deliberately absent from `rows` so that
        # appending to it raises here instead of being silently discarded at
        # write time.
        self.rows = {name: [] for name in COLUMNS if name != "SpellVisual"}
        for sl, display in self.skill_lines.items():
            self.rows["SkillLine"].append({"ID": sl, "DisplayName_lang": display})
        self._visual_rows: set[int] = set()

    def spell(self, sid, name, visuals=(), line=LINE, dangling=False):
        """Put one spell on a skill line.

        `name=None` means the spell has NO `SpellName` row -- the unnamed-id
        gap. `visuals` is a list of visual ids, or (visual id, Probability)
        pairs; visual id 0 is the client's own "no visual". `dangling=True`
        withholds the `SpellVisual` rows for this spell's visuals.
        """
        self.rows["SkillLineAbility"].append(
            {"ID": str(len(self.rows["SkillLineAbility"]) + 1), "SkillLine": line, "Spell": str(sid)}
        )
        if name is not None:
            self.rows["SpellName"].append({"ID": str(sid), "Name_lang": name})
        for v in visuals:
            vid, prob = v if isinstance(v, tuple) else (v, "1")
            self.rows["SpellXSpellVisual"].append(
                {
                    "ID": str(len(self.rows["SpellXSpellVisual"]) + 1),
                    "SpellID": str(sid),
                    "SpellVisualID": str(vid),
                    "Probability": str(prob),
                }
            )
            if vid and not dangling:
                self._visual_rows.add(int(vid))
        return self

    def event(self, visual, start, end, kit, target="1"):
        self.rows["SpellVisualEvent"].append(
            {
                "ID": str(len(self.rows["SpellVisualEvent"]) + 1),
                "SpellVisualID": str(visual),
                "TargetType": target,
                "StartEvent": str(start),
                "EndEvent": str(end),
                "SpellVisualKitID": str(kit),
            }
        )
        return self

    def kit_anim(self, kit, effect_id, loop_anim, effect_type="6"):
        self.rows["SpellVisualKitEffect"].append(
            {
                "ID": str(len(self.rows["SpellVisualKitEffect"]) + 1),
                "ParentSpellVisualKitID": str(kit),
                "EffectType": effect_type,
                "Effect": str(effect_id),
            }
        )
        self.rows["SpellVisualAnim"].append({"ID": str(effect_id), "LoopAnimID": str(loop_anim)})
        return self

    def write(self, root):
        """Write this world out as CSVs. Idempotent -- the world is unchanged.

        `SpellVisual` is the one table `write` synthesises (from the visual
        ids the spells actually claim), so it is DERIVED here rather than
        appended to `self.rows`. Appending made `write` accumulate: a second
        `run_sweep` over one `World` wrote every `SpellVisual` row twice, and
        a third three times. Nothing in the report would have shown it -- the
        script indexes `SpellVisual` into a SET -- so the next author to reuse
        a world would have been debugging a fixture that had silently changed
        under them.
        """
        build_dir = os.path.join(root, FIXTURE_BUILD)
        os.makedirs(build_dir, exist_ok=True)
        tables = dict(
            self.rows,
            SpellVisual=[{"ID": str(vid)} for vid in sorted(self._visual_rows)],
        )
        for table, cols in COLUMNS.items():
            with open(
                os.path.join(build_dir, table + ".csv"), "w", newline="", encoding="utf-8"
            ) as f:
                w = csv.DictWriter(f, fieldnames=cols)
                w.writeheader()
                w.writerows(tables[table])


def run_sweep(world, *argv, skill_line=(LINE,)):
    """Run `main` over `world`, returning (exit code, stdout).

    A `SystemExit` carrying a message (the empty-inventory bail) comes back as
    that message in the code slot, so a test can assert on it directly.

    `skill_line=()` passes no `--skill-line` at all, for the flags that do not
    take one (`--list-skill-lines`).

    The fixture cache lives for exactly the length of the sweep: the world is
    written inside the `with`, `main` reads it inside the `with`, and the
    directory is gone by the time the caller asserts. That is deliberate --
    the cache being present for the whole run is what keeps the suite offline
    (`fetch()` returns a cached CSV without a request), so its lifetime has to
    enclose `main` rather than merely outlive this call.
    """
    with tempfile.TemporaryDirectory(prefix="db2-sweep-fixture-") as tmp:
        world.write(tmp)
        args = ["--build", FIXTURE_BUILD, "--cache-dir", tmp]
        if skill_line:
            args = ["--skill-line", *skill_line] + args
        args.extend(argv)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            try:
                code = sweep_mod.main(args)
            except SystemExit as exc:
                code = exc.code
        return code, buf.getvalue()


class SweepTestCase(unittest.TestCase):
    def temp_dir(self):
        """A fixture cache dir, removed when this case ends.

        For the cases that need the written world to outlive a single
        `run_sweep` -- to delete a table out of it, or to write the same
        world twice and compare. Cleanup is deferred to `addCleanup`, which
        runs after the test method, so the directory is still on disk for
        every assertion the case makes about it.
        """
        tmp = tempfile.TemporaryDirectory(prefix="db2-sweep-fixture-")
        self.addCleanup(tmp.cleanup)
        return tmp.name

    def assertHas(self, out, needle):
        self.assertIn(needle, out, "expected in output:\n  %s\n--- got ---\n%s" % (needle, out))

    def assertLacks(self, out, needle):
        self.assertNotIn(needle, out, "did NOT expect in output:\n  %s\n--- got ---\n%s" % (needle, out))

    def assertUnderHeading(self, out, heading, needle):
        """Assert `needle` appears in the block `heading` introduces.

        A bare substring like `[42]` can be satisfied by any line anywhere in
        the report, so an assertion that means "this section lists it" has to
        say which section. The block is the run of non-blank lines after the
        heading, which is how every section in the report is printed.
        """
        lines = out.splitlines()
        start = next((i for i, ln in enumerate(lines) if heading in ln), None)
        if start is None:
            self.fail("heading not in output:\n  %s\n--- got ---\n%s" % (heading, out))
        body = []
        for ln in lines[start + 1 :]:
            if not ln.strip():
                break
            body.append(ln)
        self.assertIn(
            needle,
            "\n".join(body),
            "expected under %r:\n  %s\n--- section ---\n%s\n--- full output ---\n%s"
            % (heading, needle, "\n".join(body), out),
        )


# --------------------------------------------------------------------------
# the harness itself
# --------------------------------------------------------------------------


class NoNetworkTests(SweepTestCase):
    def test_a_missing_table_fails_rather_than_fetching(self):
        """The offline guarantee is a property of the harness, so pin it."""
        world = World().spell(100, "Fireball", [10])
        tmp = self.temp_dir()
        world.write(tmp)
        os.remove(os.path.join(tmp, FIXTURE_BUILD, "SpellVisual.csv"))
        with self.assertRaises(AssertionError) as ctx:
            with contextlib.redirect_stdout(io.StringIO()):
                sweep_mod.main(
                    ["--skill-line", LINE, "--build", FIXTURE_BUILD, "--cache-dir", tmp]
                )
        self.assertIn("attempted a network fetch", str(ctx.exception))


class WorldTests(SweepTestCase):
    def test_writing_a_world_twice_writes_the_same_world(self):
        """`World.write` must derive its rows, not accumulate them.

        `SpellVisual` is the table `write` synthesises, and it used to APPEND
        to `self.rows`: after two `run_sweep` calls over one `World` the third
        write emitted each visual three times. The script reads `SpellVisual`
        into a SET, so no report would ever have shown it -- the duplication
        is visible only in the fixture on disk, which is where this asserts.
        """
        world = World().spell(100, "Fireball", [10]).spell(101, "Frostbolt", [11])
        first = self.temp_dir()
        world.write(first)
        self.assertEqual(run_sweep(world)[0], 0)
        self.assertEqual(run_sweep(world)[0], 0)
        second = self.temp_dir()
        world.write(second)
        self.assertEqual(self._visual_csv(second), self._visual_csv(first))
        self.assertEqual(self._visual_csv(second), ["10", "11"])

    def test_a_world_sweeps_the_same_way_every_time(self):
        """The reuse a future case will actually reach for."""
        world = World().spell(100, "Holy Nova", [10]).spell(101, "Holy Nova", [20])
        self.assertEqual(run_sweep(world), run_sweep(world))
        strict, permissive = run_sweep(world), run_sweep(world, "--allow-split")
        self.assertEqual(strict[0], 1)
        self.assertEqual(permissive[0], 0)

    def _visual_csv(self, root):
        path = os.path.join(root, FIXTURE_BUILD, "SpellVisual.csv")
        with open(path, newline="", encoding="utf-8") as f:
            return [r["ID"] for r in csv.DictReader(f)]


# --------------------------------------------------------------------------
# property 2 -- the counts
# --------------------------------------------------------------------------


class CountsTests(SweepTestCase):
    def test_counts_sum_over_the_three_buckets(self):
        world = (
            World()
            .spell(100, "Fireball", [10])
            .spell(101, "Passive Talent", [0])  # only a visual-0 row: nothing to join
            .spell(102, "Helper Effect")  # no SpellXSpellVisual row at all
        )
        code, out = run_sweep(world)
        self.assertEqual(code, 0)
        self.assertHas(out, "inventory names .......................... 3")
        self.assertHas(out, "names with NO SpellXSpellVisual row ...... 2")
        self.assertHas(out, "names with at least one visual ........... 1")
        self.assertHas(out, "joined name x visual rows ................ 1")
        self.assertHas(out, "check: 3 = 2 + 1  OK")

    def test_joined_rows_exceeding_names_is_called_out(self):
        world = World().spell(100, "Fireball", [10, 20], dangling=False)
        _, out = run_sweep(world, "--allow-split")
        self.assertHas(out, "joined name x visual rows ................ 2")
        self.assertHas(out, "note: 2 joined rows > 1 names -- 1 name(s) resolve to more than")

    def test_no_visual_names_get_their_own_section(self):
        world = World().spell(100, "Fireball", [10]).spell(101, "Silent Resolve")
        _, out = run_sweep(world)
        self.assertHas(out, "### names with no visual")
        self.assertHas(out, "NO-VISUAL   Silent Resolve")

    def test_empty_inventory_bails_rather_than_reading_as_clean(self):
        world = World().spell(100, "Fireball", [10])
        code, _ = run_sweep(world, skill_line=("999",))
        self.assertIn("no SkillLineAbility rows for skill line(s) 999", str(code))

    def test_unknown_skill_line_id_is_echoed_back(self):
        world = World().spell(100, "Fireball", [10])
        _, out = run_sweep(world, skill_line=(LINE,))
        self.assertHas(out, "%-6s %s" % (LINE, LINE_NAME))


# --------------------------------------------------------------------------
# property 3 -- the era cut, both branches
# --------------------------------------------------------------------------


class EraCutTests(SweepTestCase):
    def test_all_below_the_cut(self):
        world = World().spell(100, "Fireball", [10]).spell(101, "Frostbolt", [11])
        code, out = run_sweep(world)
        self.assertEqual(code, 0)
        self.assertHas(out, "nothing above the cut -- the inventory is entirely era-side, unaffected.")
        self.assertLacks(out, "partition: max era-side id")

    def test_all_above_the_cut_asserts_nothing(self):
        world = World().spell(403501, "Rune of Fire", [10]).spell(403502, "Rune of Ice", [11])
        code, out = run_sweep(world)
        self.assertEqual(code, 0)
        self.assertHas(out, "EVERY inventory spell is above the cut (2 name(s), ids 403501..403502).")
        self.assertHas(out, "The era-side inventory is EMPTY -- this sweep has nothing to join.")
        # Vacuous, not HELD: an empty era side must never read as a clean book.
        self.assertHas(out, "VACUOUS: no name in scope resolved to a visual")
        self.assertLacks(out, "HELD:")

    def test_clean_partition_is_proven_not_asserted(self):
        world = World().spell(100, "Fireball", [10]).spell(403501, "Rune of Fire", [11])
        code, out = run_sweep(world)
        self.assertEqual(code, 0)
        self.assertHas(out, "partition: max era-side id 100 | min cut-side id 403501 | gap 403401")
        self.assertHas(out, "1 name(s) excluded above the cut: Rune of Fire")

    def test_a_name_on_both_sides_is_named(self):
        world = (
            World()
            .spell(100, "Drain Life", [10])
            .spell(403501, "Drain Life", [11])
        )
        _, out = run_sweep(world)
        self.assertHas(out, "name(s) on BOTH sides of the cut -- era ranks KEPT, cut ids dropped:")
        self.assertHas(out, "Drain Life                   era [100] | cut [403501]")

    def test_cut_zero_disables(self):
        world = World().spell(100, "Fireball", [10]).spell(403501, "Rune of Fire", [11])
        _, out = run_sweep(world, "--era-cut", "0")
        self.assertHas(out, "DISABLED (--era-cut 0): every skill-line spell is in the inventory.")
        self.assertHas(out, "inventory names .......................... 2")

    # ---- B2: a cut that excludes names NARROWS the property-4 claim --------

    def test_held_under_a_cut_carries_its_scope(self):
        """A HELD over an era-side subset must say it is a subset.

        This is AS-49's `--name` fix, reached through a different knob: the
        same Priest book that SPLITs unfiltered prints HELD under a tighter
        `--era-cut`, because the splitting name was excluded.
        """
        world = (
            World()
            .spell(100, "Smite", [10])
            .spell(2652, "Touch of Weakness", [4820])
            .spell(19249, "Touch of Weakness", [183])
        )
        code, out = run_sweep(world, "--era-cut", "2700")
        self.assertEqual(code, 0)
        self.assertHas(out, "HELD:")
        self.assertHas(out, "SCOPE: --era-cut 2700 excluded 1 inventory name(s) above the cut")
        self.assertHas(out, "This covers the era-side")
        # Touch of Weakness straddles this cut exactly as it does in the live
        # Priest book: HELD only because the rank carrying the OTHER visual
        # was excluded. That is precisely the claim the scope line qualifies.
        self.assertHas(out, "Touch of Weakness            era [2652] | cut [19249]")

    def test_unnarrowed_run_prints_no_era_scope(self):
        world = World().spell(100, "Fireball", [10])
        _, out = run_sweep(world)
        self.assertHas(out, "HELD:")
        self.assertLacks(out, "SCOPE: --era-cut")


# --------------------------------------------------------------------------
# property 4 -- the assertion
# --------------------------------------------------------------------------


class PropertyFourTests(SweepTestCase):
    def test_held(self):
        world = World().spell(100, "Fireball", [10]).spell(101, "Frostbolt", [11])
        code, out = run_sweep(world)
        self.assertEqual(code, 0)
        self.assertHas(
            out,
            "HELD: all 2 name(s) with a visual resolve to exactly one SpellVisual at Probability 1.",
        )
        self.assertHas(out, "(Rank collapse is intact; a per-name visual is safe to transcribe.)")

    def test_split_is_fatal(self):
        world = World().spell(100, "Holy Nova", [10]).spell(101, "Holy Nova", [20])
        code, out = run_sweep(world)
        self.assertEqual(code, 1)
        self.assertHas(out, "SPLIT   Holy Nova -> 2 visuals: 10, 20")
        self.assertHas(out, "Do not transcribe a single visual for them. Join each rank.")
        self.assertLacks(out, "HELD:")

    def test_low_probability_is_fatal(self):
        world = World().spell(100, "Fireball", [(10, "0.5")])
        code, out = run_sweep(world)
        self.assertEqual(code, 1)
        self.assertHas(out, "LOW-PROB Fireball:")
        self.assertHas(out, "!! spell 100 -> visual 10 at Probability 0.5")

    def test_unparseable_probability_is_fatal(self):
        world = World().spell(100, "Fireball", [(10, "")])
        code, out = run_sweep(world)
        self.assertEqual(code, 1)
        self.assertHas(out, "LOW-PROB Fireball:")

    def test_vacuous_is_not_held(self):
        world = World().spell(100, "Passive Talent", [0]).spell(101, "Helper Effect")
        code, out = run_sweep(world)
        self.assertEqual(code, 0)
        self.assertHas(out, "VACUOUS: no name in scope resolved to a visual -- this run asserts NOTHING.")
        self.assertLacks(out, "HELD:")

    def test_dangling_visual_is_fatal(self):
        world = World().spell(100, "Fireball", [10], dangling=True)
        code, out = run_sweep(world)
        self.assertEqual(code, 1)
        self.assertHas(out, "!! visual 10 has no SpellVisual row")
        self.assertHas(out, "UNRESOLVED Fireball: visual(s) [10] absent from SpellVisual")

    # ---- B1: the guard and its population must agree ----------------------

    def test_unnamed_spell_that_splits_is_fatal(self):
        """An unnamed SpellID resolving to TWO visuals must NOT read as HELD.

        `resolve()`'s marker ladder puts the name-unknown branch above SPLIT,
        so a marker-string fatal check misses this row while the shape-based
        HELD population counts it -- HELD, exit 0, over a row that splits.
        Zero of the 90 unnamed skill-line SpellIDs carry a visual at build
        1.15.9.69547, so live data cannot reach this. A fixture can.
        """
        world = World().spell(42, None, [10, 20])
        code, out = run_sweep(world)
        self.assertEqual(code, 1, "an unnamed split must be fatal:\n%s" % out)
        self.assertHas(out, "SPLIT   <no SpellName row for id 42> -> 2 visuals: 10, 20")
        self.assertLacks(out, "HELD:")

    def test_unnamed_spell_at_low_probability_is_fatal(self):
        world = World().spell(42, None, [(10, "0.25")])
        code, out = run_sweep(world)
        self.assertEqual(code, 1, "an unnamed low-probability row must be fatal:\n%s" % out)
        self.assertHas(out, "LOW-PROB <no SpellName row for id 42>:")

    def test_unnamed_spell_with_one_clean_visual_still_holds(self):
        """Shape selection must not widen the fatal set, only align it.

        An unnamed id is reported, never fatal on its own -- the row's
        RESOLUTION is what property 4 judges, and this one resolves cleanly.
        """
        world = World().spell(42, None, [10]).spell(100, "Fireball", [11])
        code, out = run_sweep(world)
        self.assertEqual(code, 0)
        self.assertHas(out, "HELD: all 2 name(s) with a visual")
        self.assertUnderHeading(
            out, "### UNRESOLVED: skill-line SpellIDs with no SpellName row", "[42]"
        )

    # ---- B3: two adjacent counts must not read as a contradiction ---------

    def test_name_filter_scope_names_its_population(self):
        world = (
            World()
            .spell(100, "Fireball", [10])
            .spell(101, "Fire Blast", [11])
            .spell(102, "Fire Ward")  # filtered in, but carries no visual
            .spell(103, "Frostbolt", [12])
        )
        code, out = run_sweep(world, "--name", "Fire")
        self.assertEqual(code, 0)
        self.assertHas(out, "(filtered by --name; counts are over the filtered subset)")
        # The HELD count (names with a visual) and the SCOPE count (filtered
        # inventory names) are different populations two lines apart. The
        # SCOPE line has to say which one it is.
        self.assertHas(out, "HELD: all 2 name(s) with a visual")
        self.assertHas(out, "SCOPE: --name Fire is in effect. This covers the 3 filtered")
        self.assertHas(out, "inventory name(s)")

    def test_name_filter_matching_nothing_is_vacuous(self):
        world = World().spell(100, "Fireball", [10])
        code, out = run_sweep(world, "--name", "Nonexistent")
        self.assertEqual(code, 0)
        self.assertHas(out, "VACUOUS:")
        self.assertHas(out, "SCOPE: --name Nonexistent is in effect.")

    def test_scope_lines_travel_with_a_failing_run_too(self):
        world = (
            World()
            .spell(100, "Holy Nova", [10])
            .spell(101, "Holy Nova", [20])
            .spell(403501, "Rune of Fire", [11])
        )
        code, out = run_sweep(world, "--name", "Holy")
        self.assertEqual(code, 1)
        self.assertHas(out, "SPLIT   Holy Nova")
        self.assertHas(out, "SCOPE: --name Holy is in effect.")
        self.assertHas(out, "SCOPE: --era-cut 400000")


# --------------------------------------------------------------------------
# B4 -- what --allow-split does and does not cover
# --------------------------------------------------------------------------


class AllowSplitTests(SweepTestCase):
    def test_allow_split_downgrades_a_split(self):
        world = World().spell(100, "Holy Nova", [10]).spell(101, "Holy Nova", [20])
        code, out = run_sweep(world, "--allow-split")
        self.assertEqual(code, 0)
        self.assertHas(out, "SPLIT   Holy Nova")
        self.assertHas(out, "(--allow-split given: exiting 0 anyway.)")

    def test_allow_split_downgrades_a_low_probability_row(self):
        world = World().spell(100, "Fireball", [(10, "0.5")])
        code, out = run_sweep(world, "--allow-split")
        self.assertEqual(code, 0)
        self.assertHas(out, "LOW-PROB Fireball:")

    def test_allow_split_does_not_cover_a_dangling_visual_id(self):
        """A dangling `SpellVisualID` is a different kind of failure.

        `--allow-split` downgrades a verdict a human can overrule by reading
        the rows. A visual id with no `SpellVisual` row has no rows to read,
        so it stays fatal.
        """
        world = World().spell(100, "Fireball", [10], dangling=True)
        code, out = run_sweep(world, "--allow-split")
        self.assertEqual(code, 1, "a dangling visual id must stay fatal:\n%s" % out)
        self.assertHas(out, "not a split")

    def test_allow_split_over_a_split_and_a_dangle_together_stays_fatal(self):
        world = (
            World()
            .spell(100, "Holy Nova", [10])
            .spell(101, "Holy Nova", [20])
            .spell(102, "Fireball", [30], dangling=True)
        )
        code, _ = run_sweep(world, "--allow-split")
        self.assertEqual(code, 1)


# --------------------------------------------------------------------------
# the optional chains
# --------------------------------------------------------------------------


class ChainTests(SweepTestCase):
    def _world(self):
        return (
            World()
            .spell(100, "Fireball", [10])
            .event(10, 1, 2, 700)
            .event(10, 3, 13, 701)
            .kit_anim(700, 500, 52)
            .kit_anim(701, 501, 54)
        )

    def test_caster_anims_names_the_animation(self):
        code, out = run_sweep(self._world(), "--caster-anims")
        self.assertEqual(code, 0)
        self.assertHas(out, "precast 700->52 ReadySpellOmni")
        self.assertHas(out, "cast 701->54 SpellCastOmni")

    def test_events_dump(self):
        _, out = run_sweep(self._world(), "--events")
        self.assertHas(out, "event (1,2) precast loop    target 1 kit 700")
        self.assertHas(out, "event (3,13) cast one-shot   target 1 kit 701")

    def test_group_by_anims_requires_caster_anims(self):
        code, _ = run_sweep(self._world(), "--group-by-anims")
        self.assertEqual(code, 2)

    def test_group_by_anims(self):
        code, out = run_sweep(self._world(), "--caster-anims", "--group-by-anims")
        self.assertEqual(code, 0)
        self.assertHas(out, "### grouped by (precast anim, cast anim) -- 1 signatures")
        self.assertHas(out, "--- precast (52,) / cast (54,)   (1)")


# --------------------------------------------------------------------------
# the flags that print no claim
# --------------------------------------------------------------------------


class NonReportingFlagTests(SweepTestCase):
    """`--list-skill-lines` and `--refresh` assert nothing about the data.

    Neither prints a block property 2/3/4 is carried in, so there is no claim
    here to regress. They are covered because they are how a person REACHES
    the claim-bearing runs: the ids every other case hardcodes are discovered
    with the first, and a stale cache -- the failure mode the second exists to
    clear -- would silently sweep the wrong build.
    """

    def test_list_skill_lines_lists_the_ids_and_names(self):
        world = World(skill_lines={LINE: LINE_NAME, "56": "Holy"}).spell(100, "Fireball", [10])
        code, out = run_sweep(world, "--list-skill-lines", skill_line=())
        self.assertEqual(code, 0)
        self.assertHas(out, "    56  Holy")
        self.assertHas(out, "   900  %s" % LINE_NAME)
        # Sorted by id as an integer -- the affordance is scanning the list.
        self.assertLess(out.index("Holy"), out.index(LINE_NAME))
        # A listing, not a sweep: it runs without --skill-line (which the
        # script's docstring recommends) and prints none of the report.
        self.assertLacks(out, "DB2 spell-visual sweep")
        self.assertLacks(out, "HELD:")

    def test_refresh_reaches_past_the_cache(self):
        """--refresh must re-fetch a table that is already cached.

        Offline, a fetch is exactly what the harness forbids -- so the proof
        that --refresh bypassed the cache is that the identical world, which
        sweeps to 0 without the flag, now trips the no-network guard.
        """
        world = World().spell(100, "Fireball", [10])
        self.assertEqual(run_sweep(world)[0], 0)
        with self.assertRaises(AssertionError) as ctx:
            # `fetch()` narrates to stderr on its way to the guard; swallow it
            # so a passing run stays quiet.
            with contextlib.redirect_stderr(io.StringIO()):
                run_sweep(world, "--refresh")
        self.assertIn("attempted a network fetch", str(ctx.exception))


if __name__ == "__main__":
    unittest.main(verbosity=2)
