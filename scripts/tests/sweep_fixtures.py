#!/usr/bin/env python3
"""Shared scaffolding for the offline fixture suites over `scripts/`' sweep tools.

`agg_sweep.py`, `comp_tiers.py`, `gen_sweep.py` and `headtohead_sweep.py` all
speak the same two file formats -- the batch JSONL that `arenasim --batch`
consumes and the per-match CSV it emits -- so the fixture builders for those
live here once instead of four times.

Two properties this module exists to guarantee:

* **Offline by construction.** `install_no_subprocess` replaces a module's
  `subprocess` with a stand-in that FAILS the test if anything is executed.
  `headtohead_sweep.py` really does shell out to `cargo run`, so its suite swaps
  in a `FakeBatchRunner` that fabricates results instead; every other suite gets
  the hard no-op guard, which also pins that those tools stay pure file I/O.
* **Driving `main(argv)`, not a subprocess.** Each tool's `main` is called in
  process with an explicit argv, so a test asserts on the real exit code and the
  real stdout rather than on a shell's idea of them.

Not a test file: `unittest` discovery ignores it, and it is imported by
`test_agg_sweep.py`, `test_comp_tiers.py`, `test_gen_sweep.py` and
`test_headtohead_sweep.py`.
"""

from __future__ import annotations

import contextlib
import csv
import io
import json
import os
import sys
import tempfile
import unittest

SCRIPTS_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
if SCRIPTS_DIR not in sys.path:
    sys.path.insert(0, SCRIPTS_DIR)


# ---------------------------------------------------------------------------
# offline guarantee
# ---------------------------------------------------------------------------


class _NoSubprocess:
    """Stands in for a module's `subprocess`. Running anything is a failure."""

    @staticmethod
    def run(cmd, **kwargs):  # pragma: no cover - only reached when a test is wrong
        raise AssertionError(
            "the tool under test tried to execute %r -- these fixtures are "
            "offline by construction" % (cmd,)
        )

    check_call = run
    check_output = run
    Popen = run


def install_no_subprocess(module):
    """Make any process launch from `module` fail the test."""
    module.subprocess = _NoSubprocess


# ---------------------------------------------------------------------------
# the two file formats
# ---------------------------------------------------------------------------

# The per-match CSV written by `arenasim --batch --out`. Spelled out rather than
# derived, so a fixture that drifts from the real export shape fails here
# instead of quietly agreeing with a broken reader.
BATCH_COLUMNS = ["label", "team1", "team2", "seed", "winner", "end_reason", "duration_secs"]


def match_rows(label, team1, team2, t1=0, t2=0, draw=0, error=0, seed_base=0, duration=42.0):
    """Build a run of per-match CSV rows with an exact outcome tally.

    `team1`/`team2` are the '+'-joined comp strings the runner writes. The
    outcome counts are laid down in a fixed order so a test can name a seed.
    """
    rows = []
    seed = seed_base
    for winner, count, reason in (
        ("team1", t1, "elimination"),
        ("team2", t2, "elimination"),
        ("draw", draw, "timeout"),
        ("error", error, "error"),
    ):
        for _ in range(count):
            rows.append(
                {
                    "label": label,
                    "team1": team1,
                    "team2": team2,
                    "seed": str(seed),
                    "winner": winner,
                    "end_reason": reason,
                    "duration_secs": str(duration),
                }
            )
            seed += 1
    return rows


def write_batch_csv(path, rows):
    """Write per-match rows as the CSV `arenasim --batch --out` produces."""
    with open(path, "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=BATCH_COLUMNS)
        w.writeheader()
        w.writerows(rows)
    return path


def read_batch_jsonl(path):
    """Read back a batch JSONL a generator wrote, as a list of configs."""
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


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


class FixtureTestCase(unittest.TestCase):
    """Assertions shared by the four suites."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="sweep-fixture-")

    def path(self, name):
        return os.path.join(self.tmp, name)

    def csv_with(self, *rows, name="results.csv"):
        """Write a per-match CSV from one or more `match_rows` runs."""
        flat = []
        for r in rows:
            flat.extend(r)
        return write_batch_csv(self.path(name), flat)

    def assertHas(self, run, needle):
        self.assertIn(needle, run.out, "expected in stdout:\n  %s\n--- got ---\n%r" % (needle, run))

    def assertLacks(self, run, needle):
        self.assertNotIn(
            needle, run.out, "did NOT expect in stdout:\n  %s\n--- got ---\n%r" % (needle, run)
        )

    def assertErrHas(self, run, needle):
        self.assertIn(needle, run.err, "expected in stderr:\n  %s\n--- got ---\n%r" % (needle, run))

    def assertOk(self, run):
        self.assertTrue(run.ok, "expected success, got %r" % (run,))
        return run
