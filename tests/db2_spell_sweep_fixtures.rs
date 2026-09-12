//! Runs the offline fixture suite for `scripts/db2_spell_sweep.py` as part of
//! `cargo test`.
//!
//! That script is load-bearing for every client-data doc in the project, and
//! everything it guards lives in a REPORTING block — the counts sum, the
//! era-cut partition, the property-4 verdict. A regression in one of those is
//! invisible until a human reads a sweep's output, which is exactly how two
//! reporting bugs shipped in AS-49. The Python suite pins them against
//! hand-built CSV fixtures; this wrapper is what makes the repo's normal check
//! run it.
//!
//! The suite needs no network and no DB2 cache: each case writes its own world
//! into a temporary `--cache-dir`, and the script's `subprocess` is replaced so
//! that an attempted fetch fails the test rather than quietly reaching out to
//! wago.tools.
//!
//! It can also be run directly, which is the faster loop while editing it:
//!
//! ```text
//! python3 scripts/tests/test_db2_spell_sweep.py
//! ```

use std::path::PathBuf;
use std::process::Command;

#[test]
fn db2_spell_sweep_fixture_suite_passes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let suite = root.join("scripts/tests/test_db2_spell_sweep.py");
    assert!(suite.is_file(), "missing fixture suite at {}", suite.display());

    // A missing interpreter FAILS rather than skips. A suite that silently
    // does not run is the same species of false reassurance the script it
    // tests exists to prevent — and python3 is already a hard dependency of
    // scripts/ (db2_spell_sweep.py, headtohead_sweep.py, behaviour_baseline.sh).
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
            "scripts/tests/test_db2_spell_sweep.py failed ({})\n\
             --- stdout ---\n{}\n--- stderr ---\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}
