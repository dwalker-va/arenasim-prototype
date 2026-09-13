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

use std::fs;
use std::path::Path;

/// The harness constructors `egui_kittest::HarnessBuilder` exposes. Each one
/// starts a harness whose app closure owes us a font install.
const BUILDERS: [&str; 6] = [
    ".build(",
    ".build_ui(",
    ".build_state(",
    ".build_ui_state(",
    ".build_eframe(",
    "Harness::new_ui(",
];

#[test]
fn every_egui_harness_installs_the_client_fonts() {
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

        let harnesses: usize = BUILDERS.iter().map(|b| src.matches(b).count()).sum();
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
