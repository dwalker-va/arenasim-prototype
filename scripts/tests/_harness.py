#!/usr/bin/env python3
"""Scaffolding shared by every offline fixture suite under `scripts/tests/`.

Two suites grew here independently -- the one over `db2_spell_sweep.py` and the
four over the balance sweep tools -- and arrived at the same three pieces. They
live here once:

* **Offline by construction.** `install_no_subprocess` replaces a module's
  `subprocess` with a stand-in that FAILS the test if anything is executed. The
  tools that shell out (`db2_spell_sweep.py` fetches from wago.tools,
  `headtohead_sweep.py` runs `cargo run`) therefore cannot reach the network
  from a fixture run, and the tools that do not shell out are pinned as pure
  file I/O. A suite that needs a real result swaps in its own fake runner
  instead.
* **Driving `main(argv)`, not a subprocess.** `run_main` calls a tool's `main`
  in process with an explicit argv, so a test asserts on the real exit code and
  the real stdout. `run_main_in_temp_dir` does the same with a scoped
  `TemporaryDirectory` around the call, for the tools whose whole world is a
  cache directory: it exists for exactly the length of the run, which is what
  keeps the run offline.
* **The interpreter floor.** `assert_runs_on_min_python` fails on source that
  would not import on the stock system interpreter.

Not a test file: `unittest` discovery ignores the leading underscore, and the
module is imported by `sweep_fixtures.py` and by each `test_*.py` suite. Its
own behaviour is pinned by `test_harness.py`.
"""

from __future__ import annotations

import ast
import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest

TESTS_DIR = os.path.dirname(os.path.abspath(__file__))
SCRIPTS_DIR = os.path.dirname(TESTS_DIR)
REPO_ROOT = os.path.dirname(SCRIPTS_DIR)

# Suites import the tool under test by module name, so `scripts/` has to be
# importable however the suite was started (directly, or by the cargo wrapper).
for _d in (SCRIPTS_DIR, TESTS_DIR):
    if _d not in sys.path:
        sys.path.insert(0, _d)


# ---------------------------------------------------------------------------
# offline guarantee
# ---------------------------------------------------------------------------


class NoSubprocess:
    """Stands in for a module's `subprocess`. Running anything is a failure.

    The message names the module that tried, because the stub is installed per
    module and a suite may install several.
    """

    def __init__(self, owner):
        self.owner = owner

    def run(self, cmd, **kwargs):  # pragma: no cover - only when a test is wrong
        raise AssertionError(
            "%s attempted a network fetch or subprocess launch (%r) -- these "
            "fixture suites are offline by construction, so this means the "
            "fixture is missing something the tool needed" % (self.owner, cmd)
        )

    check_call = run
    check_output = run
    Popen = run


def install_no_subprocess(module):
    """Make any process launch from `module` fail the test. Returns the stub."""
    module.subprocess = NoSubprocess(getattr(module, "__name__", module))
    return module.subprocess


# ---------------------------------------------------------------------------
# driving main(argv)
# ---------------------------------------------------------------------------


class Run:
    """The result of one `main(argv)` call."""

    def __init__(self, code, out, err):
        # `code` is whatever reached the caller: main's return value, or the
        # payload of a SystemExit (an int status, or argparse/sys.exit's
        # message string -- which a test can then assert on directly).
        self.code = code
        self.out = out
        self.err = err

    @property
    def ok(self):
        return self.code in (0, None)

    def __repr__(self):  # pragma: no cover - failure messages only
        return "Run(code=%r)\n--- stdout ---\n%s\n--- stderr ---\n%s" % (
            self.code,
            self.out,
            self.err,
        )


def run_main(main, argv):
    """Call `main(argv)`, capturing stdout, stderr and the exit code."""
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            code = main(argv)
        except SystemExit as exc:
            code = exc.code
    return Run(code, out.getvalue(), err.getvalue())


def run_main_in_temp_dir(main, build_argv, prefix="script-fixture-"):
    """Call `main(argv)` with a temporary directory scoped to the call.

    `build_argv(tmp)` is handed the directory and returns the argv; it is the
    hook that writes the world into it. The directory is created before
    `build_argv`, still on disk for the whole of `main`, and gone by the time
    this returns -- which is deliberate for a tool whose cache IS the fixture:
    the cache being present for the entire run is what keeps the run offline,
    so its lifetime has to enclose `main` rather than merely outlive this call.
    """
    with tempfile.TemporaryDirectory(prefix=prefix) as tmp:
        return run_main(main, build_argv(tmp))


# ---------------------------------------------------------------------------
# interpreter floor
# ---------------------------------------------------------------------------

# The stock macOS `/usr/bin/python3`. These tools are run by hand from a shell
# whose `python3` is often exactly that, so a syntax floor above it breaks the
# tool for its readers, not just for CI.
MIN_PYTHON = (3, 9)


def _has_postponed_annotations(tree):
    return any(
        isinstance(node, ast.ImportFrom)
        and node.module == "__future__"
        and any(a.name == "annotations" for a in node.names)
        for node in tree.body
    )


def _annotations_of(tree):
    """Every annotation expression in `tree`, from anywhere one can appear."""
    found = []
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            a = node.args
            # `vararg`/`kwarg` are NOT in any of the three arg lists, so a
            # `def f(*seeds: int | None)` sails past a walk that forgets them
            # -- and raises at import on 3.9 exactly like a plain argument.
            args = list(a.posonlyargs) + list(a.args) + list(a.kwonlyargs)
            args.extend(x for x in (a.vararg, a.kwarg) if x is not None)
            found.extend(arg.annotation for arg in args if arg.annotation is not None)
            if node.returns is not None:
                found.append(node.returns)
        elif isinstance(node, ast.AnnAssign) and node.annotation is not None:
            found.append(node.annotation)
    return found


def min_python_violations(source, filename="<source>"):
    """Reasons `source` would not import on `MIN_PYTHON`, as readable strings.

    Two hazards, in the order they would bite:

    * syntax the floor does not have at all (`ast.parse` is told the target
      version, so a `match` statement fails here rather than at import); and
    * a PEP 604 `X | Y` annotation, which is valid syntax on 3.9 but is
      EVALUATED at import, so it raises `TypeError` unless the module carries
      `from __future__ import annotations`. That is exactly how
      `headtohead_sweep.py` briefly acquired a 3.10 floor: green on a pyenv
      3.12, broken at import for anyone on the system interpreter, and
      invisible to a suite running on the same modern interpreter as the bug.

    Checked over the SOURCE rather than by running a second interpreter, so the
    guard holds wherever the suite runs.
    """
    name = os.path.basename(filename)
    try:
        tree = ast.parse(source, filename=filename, feature_version=MIN_PYTHON)
    except SyntaxError as exc:
        return [
            "%s line %s: not valid syntax on Python %d.%d (%s)"
            % (name, exc.lineno, MIN_PYTHON[0], MIN_PYTHON[1], exc.msg)
        ]

    if _has_postponed_annotations(tree):
        return []

    # Only annotations are the hazard -- a real `|` between ints is fine on
    # every version -- so look only inside the annotation expressions.
    violations = []
    for annotation in _annotations_of(tree):
        for sub in ast.walk(annotation):
            if isinstance(sub, ast.BinOp) and isinstance(sub.op, ast.BitOr):
                violations.append(
                    "%s line %d: PEP 604 `X | Y` annotation with no `from "
                    "__future__ import annotations`, so it raises at import on "
                    "Python %d.%d (the system interpreter). Add the import, as "
                    "`scripts/db2_spell_sweep.py` does."
                    % (name, sub.lineno, MIN_PYTHON[0], MIN_PYTHON[1])
                )
    return violations


def assert_file_runs_on_min_python(testcase, path):
    """Fail if the source at `path` would not import on `MIN_PYTHON`."""
    with open(path, encoding="utf-8") as f:
        source = f.read()
    violations = min_python_violations(source, filename=path)
    if violations:
        testcase.fail("\n".join(violations))


def assert_runs_on_min_python(testcase, module):
    """Fail if `module` would not import on `MIN_PYTHON`."""
    assert_file_runs_on_min_python(testcase, module.__file__)


def tracked_python_files():
    """Every `.py` file git tracks, as absolute paths.

    `git ls-files` rather than a filesystem walk, so the check covers exactly
    what the repo ships and a scratch file in the tree cannot fail it.
    """
    try:
        out = subprocess.run(
            ["git", "ls-files", "-z", "--", "*.py"],
            cwd=REPO_ROOT,
            stdout=subprocess.PIPE,
            check=True,
        ).stdout.decode("utf-8")
    except (OSError, subprocess.CalledProcessError) as exc:
        # Loudly, rather than returning an empty list that would make the
        # repo-wide check pass while checking nothing.
        raise AssertionError("could not list tracked files in %s: %s" % (REPO_ROOT, exc))
    return [os.path.join(REPO_ROOT, rel) for rel in out.split("\0") if rel]


# ---------------------------------------------------------------------------
# shared assertions
# ---------------------------------------------------------------------------


def _text(output):
    """The stdout of a `Run`, or a string that already is some output."""
    return output.out if isinstance(output, Run) else output


class ScriptTestCase(unittest.TestCase):
    """Base for every fixture suite: a scratch directory and output assertions.

    The output assertions take either a `Run` (asserting on its stdout) or a
    plain string, so a suite whose driver returns `(code, out)` and one whose
    driver returns a `Run` share them.
    """

    def temp_dir(self, prefix="script-fixture-"):
        """A scratch directory, removed when this case ends.

        Cleanup is deferred to `addCleanup`, which runs after the test method,
        so the directory is still on disk for every assertion the case makes
        about it.
        """
        tmp = tempfile.TemporaryDirectory(prefix=prefix)
        self.addCleanup(tmp.cleanup)
        return tmp.name

    def assertHas(self, output, needle):
        self.assertIn(
            needle, _text(output), "expected in output:\n  %s\n--- got ---\n%s" % (needle, output)
        )

    def assertLacks(self, output, needle):
        self.assertNotIn(
            needle,
            _text(output),
            "did NOT expect in output:\n  %s\n--- got ---\n%s" % (needle, output),
        )

    def assertErrHas(self, run, needle):
        self.assertIn(needle, run.err, "expected in stderr:\n  %s\n--- got ---\n%s" % (needle, run))

    def assertUnderHeading(self, output, heading, needle):
        """Assert `needle` appears in the block `heading` introduces.

        A bare substring like `[42]` can be satisfied by any line anywhere in
        the report, so an assertion that means "this section lists it" has to
        say which section. The block is the run of non-blank lines after the
        heading, which is how every section in these reports is printed.
        """
        text = _text(output)
        lines = text.splitlines()
        start = next((i for i, ln in enumerate(lines) if heading in ln), None)
        if start is None:
            self.fail("heading not in output:\n  %s\n--- got ---\n%s" % (heading, text))
        body = []
        for ln in lines[start + 1 :]:
            if not ln.strip():
                break
            body.append(ln)
        self.assertIn(
            needle,
            "\n".join(body),
            "expected under %r:\n  %s\n--- section ---\n%s\n--- full output ---\n%s"
            % (heading, needle, "\n".join(body), text),
        )
