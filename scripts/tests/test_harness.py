#!/usr/bin/env python3
"""Tests for the scaffolding the five fixture suites share (`_harness.py`).

The harness is load-bearing in a way a normal helper is not: every other suite
in this directory inherits its offline guarantee, and a guard that quietly
stopped biting would turn five green suites into five suites that pass on a
machine with network and fail on one without. So the guard is pinned here
rather than assumed.

This file is also where the interpreter floor is enforced REPO-WIDE. Each tool
suite asserts the floor for the one tool it covers -- that is what makes the
floor visible when you run that suite directly, the fast loop while editing --
and this suite asserts it over every `.py` file git tracks, which is what
covers the files no suite owns.

Run directly, or via `cargo test --test script_fixture_suites`:

    python3 scripts/tests/test_harness.py
"""

from __future__ import annotations

import os
import sys
import types
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from _harness import (  # noqa: E402
    MIN_PYTHON,
    REPO_ROOT,
    ScriptTestCase,
    assert_file_runs_on_min_python,
    install_no_subprocess,
    min_python_violations,
    run_main,
    run_main_in_temp_dir,
    tracked_python_files,
)


def _module(name="fake_tool"):
    """A stand-in for a tool module, for installing the guard onto."""
    return types.ModuleType(name)


# ---------------------------------------------------------------------------
# the offline guarantee
# ---------------------------------------------------------------------------


class NoSubprocessTests(unittest.TestCase):
    """The stub has to BITE, on every entry point a tool might reach for."""

    def setUp(self):
        self.mod = _module("pretend_sweep")
        install_no_subprocess(self.mod)

    def test_run_is_a_failure_not_a_launch(self):
        with self.assertRaises(AssertionError) as caught:
            self.mod.subprocess.run(["curl", "https://wago.tools/db2/SpellName/csv"])
        message = str(caught.exception)
        self.assertIn("offline by construction", message)
        self.assertIn("attempted a network fetch", message)

    def test_the_failure_names_the_module_that_tried(self):
        """A suite may guard several modules; the message has to say which."""
        with self.assertRaises(AssertionError) as caught:
            self.mod.subprocess.run(["cargo", "run"])
        self.assertIn("pretend_sweep", str(caught.exception))
        self.assertIn("cargo", str(caught.exception))

    def test_every_launch_entry_point_is_covered(self):
        for name in ("run", "check_call", "check_output", "Popen"):
            with self.subTest(entry=name):
                with self.assertRaises(AssertionError):
                    getattr(self.mod.subprocess, name)(["curl", "-s", "http://example.invalid"])

    def test_two_guarded_modules_do_not_share_a_name(self):
        other = _module("pretend_other")
        install_no_subprocess(other)
        with self.assertRaises(AssertionError) as caught:
            other.subprocess.run(["curl"])
        self.assertIn("pretend_other", str(caught.exception))
        self.assertNotIn("pretend_sweep", str(caught.exception))


# ---------------------------------------------------------------------------
# driving main(argv)
# ---------------------------------------------------------------------------


class RunMainTests(unittest.TestCase):
    def test_it_captures_the_streams_and_the_return_value(self):
        def main(argv):
            print("out:%s" % argv[0])
            print("err", file=sys.stderr)
            return 0

        run = run_main(main, ["alpha"])
        self.assertEqual(run.code, 0)
        self.assertTrue(run.ok)
        self.assertEqual(run.out, "out:alpha\n")
        self.assertEqual(run.err, "err\n")

    def test_a_bare_return_counts_as_success(self):
        run = run_main(lambda argv: None, [])
        self.assertTrue(run.ok)

    def test_a_nonzero_return_is_not_success(self):
        run = run_main(lambda argv: 1, [])
        self.assertFalse(run.ok)
        self.assertEqual(run.code, 1)

    def test_sys_exit_arrives_as_the_code(self):
        def main(argv):
            raise SystemExit(2)

        self.assertEqual(run_main(main, []).code, 2)

    def test_an_argparse_style_message_arrives_in_the_code_slot(self):
        """`p.error(...)`/`sys.exit("msg")` exit with a STRING, not a status."""

        def main(argv):
            raise SystemExit("nothing to sweep")

        run = run_main(main, [])
        self.assertEqual(run.code, "nothing to sweep")
        self.assertFalse(run.ok)

    def test_a_real_error_still_propagates(self):
        """Only SystemExit is swallowed -- the no-subprocess guard must reach
        the test that asserts on it."""

        def main(argv):
            raise AssertionError("offline by construction")

        with self.assertRaises(AssertionError):
            run_main(main, [])

    def test_stdout_is_restored_afterwards(self):
        before = sys.stdout
        run_main(lambda argv: print("noise"), [])
        self.assertIs(sys.stdout, before)


class RunMainInTempDirTests(unittest.TestCase):
    def test_the_directory_encloses_the_run_and_is_gone_after(self):
        seen = {}

        def build_argv(tmp):
            seen["at_build"] = os.path.isdir(tmp)
            seen["path"] = tmp
            return ["--cache-dir", tmp]

        def main(argv):
            seen["at_main"] = os.path.isdir(argv[1])
            # The world written before the run is readable during it -- that
            # is what makes the cache authoritative and the run offline.
            with open(os.path.join(argv[1], "SpellName.csv"), "w") as f:
                f.write("ID\n")
            seen["readable"] = os.path.isfile(os.path.join(argv[1], "SpellName.csv"))
            return 0

        run = run_main_in_temp_dir(main, build_argv, prefix="harness-test-")
        self.assertEqual(run.code, 0)
        self.assertTrue(seen["at_build"])
        self.assertTrue(seen["at_main"])
        self.assertTrue(seen["readable"])
        self.assertFalse(os.path.exists(seen["path"]), "the fixture cache outlived the run")

    def test_the_prefix_reaches_the_directory(self):
        seen = {}

        def build_argv(tmp):
            seen["name"] = os.path.basename(tmp)
            return []

        run_main_in_temp_dir(lambda argv: 0, build_argv, prefix="harness-test-")
        self.assertTrue(seen["name"].startswith("harness-test-"), seen["name"])


# ---------------------------------------------------------------------------
# the scratch directory
# ---------------------------------------------------------------------------


class TempDirTests(unittest.TestCase):
    """`temp_dir` has to survive the whole test method and no longer."""

    def test_it_is_removed_after_the_case_it_belongs_to(self):
        recorded = {}

        class Inner(ScriptTestCase):
            def test_writes(inner):
                path = inner.temp_dir(prefix="harness-test-")
                with open(os.path.join(path, "world.csv"), "w") as f:
                    f.write("ID\n")
                recorded["path"] = path
                # Still on disk for every assertion the case makes about it.
                inner.assertTrue(os.path.isfile(os.path.join(path, "world.csv")))

        with open(os.devnull, "w") as quiet:
            result = unittest.TextTestRunner(stream=quiet).run(
                unittest.defaultTestLoader.loadTestsFromTestCase(Inner)
            )
        self.assertTrue(result.wasSuccessful())
        self.assertFalse(os.path.exists(recorded["path"]), "temp_dir was not cleaned up")


class OutputAssertionTests(ScriptTestCase):
    """The shared assertions take a `Run` or a plain string interchangeably."""

    REPORT = "HEADING ONE\n  [42] alpha\n\nHEADING TWO\n  [7] beta\n"

    def test_a_run_and_a_string_are_both_accepted(self):
        run = run_main(lambda argv: print(self.REPORT, end=""), [])
        self.assertHas(run, "[42] alpha")
        self.assertHas(self.REPORT, "[42] alpha")
        self.assertLacks(run, "[99]")
        self.assertUnderHeading(run, "HEADING TWO", "[7] beta")
        self.assertUnderHeading(self.REPORT, "HEADING TWO", "[7] beta")

    def test_a_needle_under_the_wrong_heading_fails(self):
        """The point of the helper: anywhere-in-the-output is not the claim."""
        with self.assertRaises(AssertionError):
            self.assertUnderHeading(self.REPORT, "HEADING TWO", "[42] alpha")

    def test_a_missing_heading_fails(self):
        with self.assertRaises(AssertionError):
            self.assertUnderHeading(self.REPORT, "HEADING THREE", "[7] beta")

    def test_stderr_is_asserted_separately_from_stdout(self):
        def main(argv):
            print("on stdout")
            print("on stderr", file=sys.stderr)

        run = run_main(main, [])
        self.assertErrHas(run, "on stderr")
        self.assertLacks(run, "on stderr")


# ---------------------------------------------------------------------------
# the interpreter floor
# ---------------------------------------------------------------------------


class MinPythonGuardTests(unittest.TestCase):
    """What the guard must catch, and what it must not flag."""

    def assertFlags(self, source):
        violations = min_python_violations(source, filename="fake_tool.py")
        self.assertTrue(violations, "expected a violation for:\n%s" % source)
        return violations

    def assertClean(self, source):
        self.assertEqual(min_python_violations(source, filename="fake_tool.py"), [])

    def test_a_plain_argument_annotation_is_caught(self):
        self.assertFlags("def f(seed: int | None):\n    pass\n")

    def test_a_vararg_annotation_is_caught(self):
        """`*args` is in `a.vararg`, in none of the three arg lists.

        A walk that iterates only `args`/`posonlyargs`/`kwonlyargs` passes
        `def f(*seeds: int | None)` -- which raises at import on 3.9 exactly
        like a plain argument would.
        """
        self.assertFlags("def f(*seeds: int | None):\n    pass\n")

    def test_a_kwarg_annotation_is_caught(self):
        self.assertFlags("def f(**opts: str | None):\n    pass\n")

    def test_a_keyword_only_annotation_is_caught(self):
        self.assertFlags("def f(*, seed: int | None = None):\n    pass\n")

    def test_a_return_annotation_is_caught(self):
        self.assertFlags("def f():\n    return None\n\n\ndef g() -> int | None:\n    pass\n")

    def test_a_variable_annotation_is_caught(self):
        self.assertFlags("seed: int | None = None\n")

    def test_an_async_def_is_caught(self):
        self.assertFlags("async def f(seed: int | None):\n    pass\n")

    def test_a_nested_def_is_caught(self):
        self.assertFlags("def outer():\n    def inner(x: int | None):\n        pass\n")

    def test_the_future_import_clears_every_annotation(self):
        """The fix the failure message tells you to apply has to work."""
        self.assertClean(
            "from __future__ import annotations\n\n\ndef f(*seeds: int | None) -> str | None:\n"
            "    pass\n"
        )

    def test_a_real_bitwise_or_is_not_an_annotation(self):
        self.assertClean("def f(a, b):\n    return a | b\n")

    def test_syntax_newer_than_the_floor_is_caught(self):
        """Not every floor break is an annotation."""
        violations = self.assertFlags("def f(x):\n    match x:\n        case 1:\n            pass\n")
        self.assertIn("%d.%d" % MIN_PYTHON, violations[0])

    def test_the_message_names_the_file_and_the_line(self):
        violations = self.assertFlags("import os\n\n\ndef f(seed: int | None):\n    pass\n")
        self.assertIn("fake_tool.py", violations[0])
        self.assertIn("line 4", violations[0])


class RepoWideFloorTests(unittest.TestCase):
    """Every `.py` this repo ships must import on the system interpreter.

    The floor is a property of the REPO, not of the four tools that happen to
    have a fixture suite: `packaging/generate_icon.py` has no suite, and the
    next script added will not have one on its first day either.
    """

    def test_every_tracked_python_file_runs_on_the_minimum_interpreter(self):
        files = tracked_python_files()
        # Non-vacuity: a listing that returned nothing would make this pass
        # while checking nothing at all. The count is a coarse tripwire only —
        # it sits below the 14 tracked today so ordinary deletions do not fail
        # it, and the two `assertIn`s below carry the real weight: they name a
        # file that has a fixture suite and one that does not, so the walk is
        # proven to reach past the suites into the rest of the repo.
        self.assertGreaterEqual(len(files), 12, "suspiciously few tracked .py files: %r" % files)
        for path in files:
            self.assertTrue(os.path.isfile(path), path)
        tracked = {os.path.relpath(p, REPO_ROOT) for p in files}
        self.assertIn(os.path.join("scripts", "db2_spell_sweep.py"), tracked)
        self.assertIn(os.path.join("packaging", "generate_icon.py"), tracked)
        for path in files:
            with self.subTest(path=os.path.basename(path)):
                assert_file_runs_on_min_python(self, path)


if __name__ == "__main__":
    unittest.main(verbosity=2)
