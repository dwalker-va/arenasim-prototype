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
//! This is a lexical scan of source text, so it is a drift guard and not a
//! proof. The reading is shared with the repo's other lexical audits
//! (`tests/common/source_audit.rs`), which is where three gaps this file used
//! to list were closed: comments and string literals are blanked before
//! anything is counted, `use egui_kittest::Harness as H;` is resolved back to
//! `Harness`, and every pattern below tolerates whitespace a formatter would
//! never leave. What remains:
//!
//! * **Counts, not pairing.** The comparison is a per-file COUNT of harnesses
//!   against a count of `install_game_fonts(` call sites, so a two-harness file
//!   whose first closure installed twice and whose second installed not at all
//!   would pass. Real pairing needs a parser: the textual approximation —
//!   charging each install to the nearest preceding constructor — would reject
//!   the legitimate shape where the app closure is a named `fn` defined above
//!   the constructor that takes it. That false positive would block honest
//!   refactors, while this false negative needs two harnesses in one file plus
//!   a doubled install to appear at all. The counts stay.
//! * **A harness built somewhere else.** A helper in one file that returns a
//!   built `Harness` to another counts against the file that builds it, which
//!   is the file that must install the fonts — but a harness reached through a
//!   trait object or a macro-generated call site is not text this scan can see.

mod common;

use common::source_audit::{load_sources, SourceFile, TypeAliases};
use regex::Regex;

/// Every `egui_kittest` call that hands back a live `Harness`, matched by
/// SHAPE rather than by name — a hand-maintained list of constructors goes
/// stale silently, and a stale list in this particular audit would wave
/// through exactly the next screen the audit exists to catch.
///
/// Two shapes cover the crate's whole harness-producing surface:
///
/// * `Harness::new*(` — the inherent constructors, with an optional turbofish
///   (`Harness::<State>::new_state(`, nested generics included).
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
          Harness \s* (?: :: \s* < (?: [^<>] | < [^<>]* > )* > \s* )? :: \s* new \w* \s* \(
        | \. \s* build (?: _ \w+ )? \s* \(
        ",
    )
    .expect("harness-constructor pattern must compile")
}

/// `install_game_fonts(` as a CALL — the `use` line carries no parenthesis, so
/// an import is not an installation.
fn install_calls() -> Regex {
    Regex::new(r"\binstall_game_fonts\s*\(").expect("install-call pattern must compile")
}

#[test]
fn every_egui_harness_installs_the_client_fonts() {
    let constructors = harness_constructors();
    let installs_re = install_calls();

    let sources = load_sources(&["src", "tests"]).expect("src/ and tests/ must be readable");

    let mut checked = 0;
    let mut total_harnesses = 0;
    let mut failures = Vec::new();

    for file in &sources {
        // This file names the builders it looks for, so it would flag itself.
        if file.path.file_name().and_then(|n| n.to_str()) == Some("snapshot_font_audit.rs") {
            continue;
        }
        // A file that never names the harness crate cannot build a harness, and
        // the full read below is not worth paying for on every file in the tree.
        if !file.code.contains("egui_kittest") {
            continue;
        }
        // Comments and string literals are blanked, and aliases resolved, so a
        // constructor discussed in prose is not counted and one spelled through
        // an alias is.
        let code = readable_code(file);
        checked += 1;

        let harnesses = constructors.find_iter(&code).count();
        total_harnesses += harnesses;
        let installs = installs_re.find_iter(&code).count();

        if installs < harnesses {
            let name = file.rel_display();
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

/// The text this audit counts in: comments and literal contents blanked, type
/// aliases resolved back to what they alias.
fn readable_code(file: &SourceFile) -> String {
    let code = common::source_audit::blank_comments_and_strings(&file.raw);
    let aliases = TypeAliases::from_source(&code);
    aliases.expand(&code)
}

/// The audit is only as good as its detector, so pin the detector itself:
/// every harness-returning call egui_kittest 0.31.1 offers must be counted,
/// the non-terminal `Harness::builder()` must not be, and neither a spelling
/// rustfmt would reject nor an aliased import may slip through.
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
        // Spellings a formatter would never produce, which used to be silent
        // misses: loose whitespace, and a nested-generic turbofish.
        "Harness :: new (|ctx| {})",
        "Harness::<Vec<u8>>::new_state(|ctx, s| {}, vec![])",
        "harness . build (|ctx| {})",
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

/// The reading the count depends on: an aliased import is resolved, and a
/// constructor that appears only in prose or in a string literal is not a
/// harness. Both were silent misses before the reading moved into
/// `tests/common/source_audit.rs`.
#[test]
fn the_reading_resolves_aliases_and_ignores_prose() {
    let constructors = harness_constructors();
    let installs_re = install_calls();

    let aliased = SourceFile {
        path: std::path::PathBuf::from("aliased.rs"),
        raw: r#"
            use egui_kittest::Harness as H;
            fn t() {
                let mut h = H::new(|ctx| { install_game_fonts(ctx); });
            }
        "#
        .to_string(),
        code: String::new(),
    };
    let code = readable_code(&aliased);
    assert_eq!(
        constructors.find_iter(&code).count(),
        1,
        "`H::new(` through `use egui_kittest::Harness as H` is a harness"
    );
    assert_eq!(installs_re.find_iter(&code).count(), 1);

    let prose = SourceFile {
        path: std::path::PathBuf::from("prose.rs"),
        raw: r#"
            //! Talks about Harness::new(|ctx| ..) in a doc comment.
            /* and Harness::new( in a block comment */
            fn t() {
                let sample = "Harness::new(|ctx| {})";
                let _ = sample;
            }
        "#
        .to_string(),
        code: String::new(),
    };
    assert_eq!(
        constructors.find_iter(&readable_code(&prose)).count(),
        0,
        "a constructor in a comment or a string literal is prose, not a harness"
    );
}
