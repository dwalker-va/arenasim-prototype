//! Every egui kittest harness must install the client's font stack.
//!
//! A harness that renders with egui's default fonts produces baselines of a
//! screen nobody plays: a glyph Rajdhani has no coverage for, or a label that
//! only fits under the defaults' metrics, passes the suite and is wrong in the
//! game. That is not hypothetical — the encyclopedia's Home button shipped a
//! `⌂` candidate that is a tofu box in the default stack, and the suite could
//! not have shown it.
//!
//! So this audit is the drift guard for the one-definition rule: every Rust
//! source file in the crate that builds an `egui_kittest` harness must call
//! `arenasim::ui::fonts::install_game_fonts` at least once per harness it
//! builds. It reads source text, needs no GPU, and runs in the default
//! `cargo test`.
//!
//! # Why the scan covers `src/` as well as `tests/`
//!
//! The rule enforced here is "every kittest harness installs the game fonts",
//! which has no exceptions to remember. The narrower rule — "every harness that
//! *takes a snapshot* installs them" — asks a reader to know which harnesses
//! are which, and a harness that takes no snapshot today may grow a
//! `.snapshot()` call tomorrow, silently baking a baseline of the wrong screen.
//! `#[cfg(test)]` modules inside `src/` build harnesses too
//! (`states/encyclopedia/widget.rs` drives the link click-contract through
//! one), so the scan follows the harnesses rather than the directory. When a
//! `src/` harness trips this audit the fix is to install the fonts in it — one
//! line, and it makes the harness render what the client renders — never an
//! exemption.
//!
//! # Self-reported limitations
//!
//! This is a lexical scan of raw source text, so it is a drift guard and not a
//! proof. Known, accepted gaps:
//!
//! * **Raw source text.** A constructor written inside a comment or a string
//!   literal counts as real, and whitespace spellings rustfmt normalises away
//!   (`Harness :: new (`) do not count at all.
//! * **Counts, not pairing.** The comparison is a per-file COUNT of harnesses
//!   against a count of `install_game_fonts(` call sites, so a two-harness file
//!   whose first closure installed twice and whose second installed not at all
//!   would pass. Real pairing needs a parser: the textual approximation —
//!   charging each install to the nearest preceding constructor — would reject
//!   the legitimate shape where the app closure is a named `fn` defined above
//!   the constructor that takes it. That false positive would block honest
//!   refactors, while this false negative needs two harnesses in one file plus
//!   a doubled install to appear at all. The counts stay.
//! * **Aliased imports.** `use egui_kittest::Harness as H; H::new(..)` is a
//!   silent miss — the pattern matches the type's real name.
//! * **Nested-generic turbofish.** `Harness::<Vec<u8>>::new_state(` is a silent
//!   miss: the turbofish arm stops at the first `>`.

use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

/// Every `egui_kittest` call that hands back a live `Harness`, matched by
/// SHAPE rather than by name — a hand-maintained list of constructors goes
/// stale silently, and a stale list in this particular audit would wave
/// through exactly the next screen the audit exists to catch.
///
/// Two shapes cover the crate's whole harness-producing surface:
///
/// * `Harness::new*(` — the inherent constructors, with an optional turbofish
///   (`Harness::<State>::new_state(`).
/// * `.build*(` — the terminal `HarnessBuilder` methods. `Harness::builder()`
///   is deliberately NOT matched, since counting it would double-count every
///   `Harness::builder().build(..)` chain, and the leading `\.` is what
///   excludes it: `builder` is an associated function reached by `::`, so it
///   never appears after a dot. (The `_` boundary in `(?:_\w+)?` is belt and
///   braces for the same case — it also declines a hypothetical *method*
///   spelled `.builder(`.)
///
/// Verified exhaustive against **egui_kittest 0.31.1**, whose harness-returning
/// functions are `Harness::{new, new_state, new_ui, new_ui_state, new_eframe}`
/// and `HarnessBuilder::{build, build_state, build_ui, build_ui_state,
/// build_eframe}` — ten names, both shapes. On a version bump the only thing to
/// re-check is whether the crate grew a harness constructor under a THIRD
/// naming convention; anything new called `new*` on `Harness` or `build*` on
/// the builder is already counted without touching this file.
fn harness_constructors() -> Regex {
    Regex::new(
        r"(?x)
          Harness (?: :: < [^>]* > )? :: new \w* \(   # Harness::new(, Harness::<S>::new_state(
        | \. build (?: _ \w+ )? \(                    # .build(, .build_eframe( — not .builder()
        ",
    )
    .expect("harness-constructor pattern must compile")
}

/// Every `.rs` file under `root`, recursively.
fn rust_sources(root: &Path, out: &mut Vec<PathBuf>) {
    let dir =
        fs::read_dir(root).unwrap_or_else(|e| panic!("{} must be readable: {e}", root.display()));
    for entry in dir {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_egui_harness_installs_the_client_fonts() {
    let constructors = harness_constructors();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut sources = Vec::new();
    for root in ["src", "tests"] {
        rust_sources(&manifest.join(root), &mut sources);
    }
    sources.sort();

    let mut checked = 0;
    let mut total_harnesses = 0;
    let mut failures = Vec::new();

    for path in sources {
        // This file names the builders it looks for, so it would flag itself.
        if path.file_name().and_then(|n| n.to_str()) == Some("snapshot_font_audit.rs") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("source must be readable");
        if !src.contains("egui_kittest") {
            continue;
        }
        checked += 1;

        let harnesses = constructors.find_iter(&src).count();
        total_harnesses += harnesses;
        // The `use` line carries no parenthesis, so this counts call sites only.
        let installs = src.matches("install_game_fonts(").count();

        if installs < harnesses {
            let name = path.strip_prefix(manifest).unwrap_or(&path).display();
            failures.push(format!(
                "{name}: builds {harnesses} egui_kittest harness(es) but calls \
                 install_game_fonts {installs} time(s)"
            ));
        }
    }

    assert!(
        checked > 0 && total_harnesses > 0,
        "audit saw {checked} egui_kittest file(s) and {total_harnesses} harness(es) — \
         the detection is broken, not the tree"
    );
    assert!(
        failures.is_empty(),
        "these harnesses render with egui's DEFAULT fonts, so what they measure and picture \
         is a screen the player never sees. Call \
         `arenasim::ui::fonts::install_game_fonts(ctx)` — `install_game_fonts(ui.ctx())` for \
         a `new_ui` harness — as the first statement of each harness's app closure, then \
         re-bless any baselines it owns:\n  {}",
        failures.join("\n  ")
    );
}

/// The audit is only as good as its detector, so pin the detector itself:
/// every harness-returning call egui_kittest 0.31.1 offers must be counted,
/// and the non-terminal `Harness::builder()` must not be.
#[test]
fn the_detector_counts_every_kittest_constructor() {
    let constructors = harness_constructors();

    for call in [
        "Harness::new(|ctx| {})",
        "Harness::new_state(|ctx, s| {}, 0)",
        "Harness::new_ui(|ui| {})",
        "Harness::new_ui_state(|ui, s| {}, 0)",
        "Harness::new_eframe(|cc| App::new(cc))",
        "Harness::<State>::new_state(|ctx, s| {}, 0)",
        "Harness::builder().build(|ctx| {})",
        "Harness::builder().build_state(|ctx, s| {}, 0)",
        "Harness::builder().build_ui(|ui| {})",
        "Harness::builder().build_ui_state(|ui, s| {}, 0)",
        "Harness::builder().build_eframe(|cc| App::new(cc))",
    ] {
        assert_eq!(
            constructors.find_iter(call).count(),
            1,
            "`{call}` must count as exactly one harness"
        );
    }

    // Opening a builder is not building one.
    assert_eq!(constructors.find_iter("Harness::builder()").count(), 0);
    assert_eq!(
        constructors
            .find_iter("let b = Harness::builder();")
            .count(),
        0
    );
}
