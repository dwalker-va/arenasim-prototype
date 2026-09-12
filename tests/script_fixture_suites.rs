//! Runs the offline fixture suites for the Python tools in `scripts/` as part
//! of `cargo test`.
//!
//! Everything these tools guard lives in a REPORTING block — the counts and
//! the property-4 verdict of `db2_spell_sweep.py`, the Wilson interval, MOVED
//! flag, z and denominator the balance tools print. A regression in one of
//! those is invisible until a human re-derives the number by hand, which
//! nobody does: client-data docs and balance decisions get read straight off
//! the output. That is exactly how AS-49's two reporting bugs shipped. The
//! Python suites pin those claims against hand-built fixtures and published
//! values (Newcombe 1998 for the intervals), never against what the code
//! itself computes; this wrapper is what makes the repo's normal check run
//! them.
//!
//! Every suite is offline by construction: hand-built CSVs and JSONL, and each
//! module's `subprocess` replaced with a stand-in that fails the test rather
//! than launching anything. `db2_spell_sweep.py` would otherwise fetch from
//! wago.tools and `headtohead_sweep.py` would shell out to `cargo run`; under
//! the suites the first reads a temporary `--cache-dir` written by the fixture
//! and the second gets a fake batch runner, so no match is simulated and no
//! request is made.
//!
//! `scripts/tests/_harness.py` holds the scaffolding they all share, and
//! `scripts/tests/test_harness.py` pins the harness itself — including the
//! repo-wide check that every tracked `.py` still imports on the stock system
//! interpreter.
//!
//! A suite can also be run directly, which is the faster loop while editing
//! it:
//!
//! ```text
//! python3 scripts/tests/test_headtohead_sweep.py
//! ```

use std::path::PathBuf;
use std::process::Command;

/// Run one Python fixture suite, failing the test if it fails — or if there is
/// no interpreter to run it with.
fn run_suite(relative: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let suite = root.join(relative);
    assert!(suite.is_file(), "missing fixture suite at {}", suite.display());

    // A missing interpreter FAILS rather than skips. A suite that silently
    // does not run is the same species of false reassurance these scripts
    // exist to prevent — and python3 is already a hard dependency of
    // scripts/ (db2_spell_sweep.py, gen_sweep.py, agg_sweep.py,
    // headtohead_sweep.py, behaviour_baseline.sh and the balance-sweep skill
    // all shell into it).
    let out = Command::new("python3")
        .arg(&suite)
        .current_dir(&root)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "could not run python3 for {}: {e}. The fixture suite is part of \
                 this repo's checks; install python3 rather than skipping it.",
                suite.display()
            )
        });

    if !out.status.success() {
        panic!(
            "{relative} failed ({})\n--- stdout ---\n{}\n--- stderr ---\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

/// The scaffolding the other five suites inherit their offline guarantee from,
/// plus the repo-wide interpreter floor.
#[test]
fn harness_suite_passes() {
    run_suite("scripts/tests/test_harness.py");
}

/// The reporting blocks every client-data doc in the project rests on.
#[test]
fn db2_spell_sweep_fixture_suite_passes() {
    run_suite("scripts/tests/test_db2_spell_sweep.py");
}

/// The serious one on the balance side: the Wilson intervals and z-tests
/// balance decisions are read off.
#[test]
fn headtohead_sweep_fixture_suite_passes() {
    run_suite("scripts/tests/test_headtohead_sweep.py");
}

#[test]
fn agg_sweep_fixture_suite_passes() {
    run_suite("scripts/tests/test_agg_sweep.py");
}

#[test]
fn comp_tiers_fixture_suite_passes() {
    run_suite("scripts/tests/test_comp_tiers.py");
}

#[test]
fn gen_sweep_fixture_suite_passes() {
    run_suite("scripts/tests/test_gen_sweep.py");
}
