//! Loadout ordering audit (AS-58)
//!
//! Applying equipment sums float stats across a loadout's entries, and float
//! addition is not associative — so the map's iteration order decides the last
//! ULP of every derived stat. A `HashMap` with the default `RandomState` is
//! seeded PER PROCESS, so that order changes between runs of one unmodified
//! binary. Measured: the same release build, run 40 times, produced Rogue
//! `crit_chance` as three distinct bit patterns (0x3e2e147a / 0x3e2e147b /
//! 0x3e2e147c). Harmless as gameplay, fatal as methodology — this project
//! verifies nearly every sim-adjacent change by headless byte-identity.
//!
//! The fix is the [`Loadout`] alias (`BTreeMap<ItemSlot, ItemId>`), which puts
//! the ordering in the TYPE. `equipment::tests::loadout_is_ordered` guards the
//! alias itself. This audit guards the OTHER direction: new code that declares
//! its own `HashMap` keyed by `ItemSlot` instead of using the alias, which the
//! type assertion could never see.
//!
//! **Why not a repetition test.** Computing the same loadout's stats N times in
//! one process cannot fail — `RandomState` is seeded once per process, so a
//! `HashMap` iterates identically for the whole life of a test binary. The bug
//! only exists across processes, so the guard has to be structural.

use std::fs;
use std::path::{Path, PathBuf};

/// Source roots scanned for `HashMap`s keyed by `ItemSlot`.
const SCAN_ROOTS: &[&str] = &["src", "tests"];

/// This file names the forbidden pattern in its own prose, and would otherwise
/// flag itself.
const SELF: &str = "tests/loadout_order_audit.rs";

#[test]
fn no_hashmap_is_keyed_by_item_slot() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut violations: Vec<String> = Vec::new();
    for root in SCAN_ROOTS {
        let dir = repo_root.join(root);
        let mut files = Vec::new();
        collect_rs_files(&dir, &mut files).expect("failed to walk source tree");
        for path in files {
            let rel = path
                .strip_prefix(&repo_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if rel == SELF {
                continue;
            }
            let src = fs::read_to_string(&path).expect("failed to read source file");
            for (i, line) in src.lines().enumerate() {
                // Both spellings: the bare type and a fully-qualified one.
                if line.contains("HashMap<ItemSlot") || line.contains("HashMap::<ItemSlot") {
                    violations.push(format!("{}:{}: {}", rel, i + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "A HashMap keyed by ItemSlot iterates in per-process-seeded order. Applying \
         equipment sums floats across a loadout, so that order changes derived stats \
         in the last ULP between runs of the same binary — which breaks this project's \
         byte-identity verification protocol (AS-58).\n\n\
         Use the `Loadout` alias (`BTreeMap<ItemSlot, ItemId>`) from \
         `states::play_match::equipment` instead.\n\n\
         Offending lines:\n  {}",
        violations.join("\n  ")
    );
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rs_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}
