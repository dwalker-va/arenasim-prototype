//! Every egui snapshot harness must install the client's font stack.
//!
//! A harness that renders with egui's default fonts produces baselines of a
//! screen nobody plays: a glyph Rajdhani has no coverage for, or a label that
//! only fits under the defaults' metrics, passes the suite and is wrong in the
//! game. That is not hypothetical — the encyclopedia's Home button shipped a
//! `⌂` candidate that is a tofu box in the default stack, and the suite could
//! not have shown it.
//!
//! So this audit is the drift guard for the one-definition rule: every test
//! file that builds an `egui_kittest` harness must call
//! `arenasim::ui::fonts::install_game_fonts` at least once per harness it
//! builds. It reads source text, needs no GPU, and runs in the default
//! `cargo test`.

use regex::Regex;
use std::fs;
use std::path::Path;

/// Every `egui_kittest` call that hands back a live `Harness`, matched by
/// SHAPE rather than by name — a hand-maintained list of constructors goes
/// stale silently, and a stale list in this particular audit would wave
/// through exactly the next screen the audit exists to catch.
///
/// Two shapes cover the crate's whole harness-producing surface:
///
/// * `Harness::new*(` — the inherent constructors, with an optional turbofish
///   (`Harness::<State>::new_state(`).
/// * `.build*(` — the terminal `HarnessBuilder` methods. `.builder()` is
///   deliberately NOT matched: it opens a builder rather than finishing one,
///   and counting it would double-count every `Harness::builder().build(..)`
///   chain.
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

#[test]
fn every_egui_harness_installs_the_client_fonts() {
    let constructors = harness_constructors();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut checked = 0;
    let mut failures = Vec::new();

    for entry in fs::read_dir(&dir).expect("tests/ must be readable") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("test source must be readable");
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        // This file names the builders it looks for, so it would flag itself.
        if name == "snapshot_font_audit.rs" || !src.contains("egui_kittest") {
            continue;
        }
        checked += 1;

        let harnesses = constructors.find_iter(&src).count();
        // The `use` line carries no parenthesis, so this counts call sites only.
        let installs = src.matches("install_game_fonts(").count();

        if installs < harnesses {
            failures.push(format!(
                "{name}: builds {harnesses} egui_kittest harness(es) but calls \
                 install_game_fonts {installs} time(s)"
            ));
        }
    }

    assert!(
        checked > 0,
        "audit found no egui_kittest harnesses — the detection is broken, not the tree"
    );
    assert!(
        failures.is_empty(),
        "these harnesses render with egui's DEFAULT fonts, so their baselines picture a \
         screen the player never sees. Call `arenasim::ui::fonts::install_game_fonts(ctx)` \
         as the first statement of each harness's app closure, then re-bless:\n  {}",
        failures.join("\n  ")
    );
}

/// The audit is only as good as its detector, so pin the detector itself:
/// every harness-returning call egui_kittest 0.31.1 offers must be counted,
/// and the non-terminal `.builder()` must not be.
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
