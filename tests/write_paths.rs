//! Where the game's writable files land.
//!
//! `src/paths.rs` sends installed builds to a per-user data directory. These
//! tests pin the other half of that contract: a build running from a checkout
//! — which is what every balance script and probe drives — must keep writing
//! exactly where it always has.

use std::fs;
use std::path::Path;

use arenasim::combat::log::{CombatLog, MatchMetadata};

fn test_metadata() -> MatchMetadata {
    MatchMetadata {
        arena_name: "Test Arena".to_string(),
        winner: Some(1),
        random_seed: Some(42),
        team1: vec![],
        team2: vec![],
    }
}

/// `scripts/behaviour_baseline.sh` picks up the newest log with
/// `ls -t match_logs/match_*.txt`, so a defaulted save from a checkout has to
/// land under `match_logs/` relative to the checkout.
#[test]
fn a_defaulted_log_lands_under_match_logs_in_the_checkout() {
    let path = CombatLog::default()
        .save_to_file(&test_metadata(), None)
        .expect("save defaulted log");

    assert!(
        path.starts_with("match_logs/match_") && path.ends_with(".txt"),
        "defaulted log should match the match_logs/match_*.txt glob, got {path:?}"
    );
    assert!(
        Path::new(&path).is_file(),
        "defaulted log should exist relative to the checkout, got {path:?}"
    );

    fs::remove_file(&path).expect("clean up the log this test wrote");
}

/// The explicit-path branch is what `--out`, `-o` and `--output` flow through,
/// and what the matrix scripts pass. The seam must not touch it.
#[test]
fn an_explicit_output_path_is_written_verbatim() {
    let dir = tempfile::tempdir().expect("tempdir");
    let requested = dir.path().join("nested/run.txt");
    let requested = requested.to_str().expect("utf-8 temp path");

    let path = CombatLog::default()
        .save_to_file(&test_metadata(), Some(requested))
        .expect("save to explicit path");

    assert_eq!(path, requested, "explicit path should be used verbatim");
    assert!(Path::new(requested).is_file(), "explicit path should exist");
}
