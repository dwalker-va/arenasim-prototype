#!/usr/bin/env python3
"""Fixture builders for the offline suites over `scripts/`' sweep tools.

`agg_sweep.py`, `comp_tiers.py`, `gen_sweep.py` and `headtohead_sweep.py` all
speak the same two file formats -- the batch JSONL that `arenasim --batch`
consumes and the per-match CSV it emits -- so the builders for those live here
once instead of four times.

The scaffolding that is not specific to those two formats -- the no-subprocess
guard, the `main(argv)` driver, the scratch directory, the shared output
assertions, the interpreter floor -- lives in `_harness.py`, which the
`db2_spell_sweep.py` suite shares.

Not a test file: `unittest` discovery ignores it, and it is imported by
`test_agg_sweep.py`, `test_comp_tiers.py`, `test_gen_sweep.py` and
`test_headtohead_sweep.py`.
"""

from __future__ import annotations

import csv
import json
import os

from _harness import ScriptTestCase

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


class FixtureTestCase(ScriptTestCase):
    """The shared base, plus the batch formats these four suites are built on."""

    def setUp(self):
        self.tmp = self.temp_dir(prefix="sweep-fixture-")

    def path(self, name):
        return os.path.join(self.tmp, name)

    def csv_with(self, *rows, name="results.csv"):
        """Write a per-match CSV from one or more `match_rows` runs."""
        flat = []
        for r in rows:
            flat.extend(r)
        return write_batch_csv(self.path(name), flat)

    def assertOk(self, run):
        self.assertTrue(run.ok, "expected success, got %r" % (run,))
        return run
