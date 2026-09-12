//! Runs the offline fixture suites for the balance sweep tools in `scripts/`
//! as part of `cargo test`.
//!
//! `gen_sweep.py` decides what a sweep measures, `agg_sweep.py` and
//! `comp_tiers.py` decide what it says, and `headtohead_sweep.py` decides
//! whether the difference is real. Everything they guard lives in a REPORTING
//! block: an interval, a MOVED flag, a z, a denominator. A regression in one
//! of those is invisible until a human re-derives the number by hand, which
//! nobody does — balance decisions get read straight off the output. These
//! suites pin the statistics against published values (Newcombe 1998 for the
//! Wilson intervals) and hand-worked z's, never against what the code itself
//! computes.
//!
//! Every suite is offline by construction: hand-built CSVs and JSONL, and each
//! module's `subprocess` replaced with a stand-in that fails the test rather
//! than launching anything. `headtohead_sweep.py` really does shell out to
//! `cargo run`, so its suite swaps in a fixture batch runner — no matches are
//! simulated and no network is touched.
//!
//! The suites can also be run directly, which is the faster loop while editing
//! them:
//!
//! ```text
//! python3 scripts/tests/test_headtohead_sweep.py
//! ```
//!
//! `scripts/tests/sweep_fixtures.py` holds the scaffolding they share. It is
//! deliberately separate from `scripts/tests/test_db2_spell_sweep.py`, whose
//! near-identical harness predates it.

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
    // scripts/ (gen_sweep.py, agg_sweep.py, headtohead_sweep.py and the
    // balance-sweep skill all shell into it).
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

/// The serious one: the Wilson intervals and z-tests balance decisions are
/// read off.
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
