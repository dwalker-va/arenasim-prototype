//! UI values come from `ui::theme`: a ratchet on every literal written
//! anywhere else.
//!
//! The design system (`docs/design/proposals/2026-10-11-ui-design-system.md`
//! §3.4a) chooses each colour, text size, gap, radius and icon size once, in
//! `src/ui/theme.rs`, by role. This audit counts what is still chosen
//! somewhere else. It reads source text, needs no GPU, and runs in the default
//! `cargo test`.
//!
//! # What it counts
//!
//! In every `.rs` file under `src/` except `src/ui/theme.rs`, five kinds of
//! literal ([`Kind`]):
//!
//! * **colour** — `Color32::from_*(…)` (`from_rgb`, `from_rgba_*`,
//!   `from_gray`, `from_black_alpha`, `from_white_alpha`, …) and every named
//!   `Color32::` constant except `TRANSPARENT`, which is the absence of a
//!   colour rather than a choice of one;
//! * **size** — a text size given as a number: `.size(<n>…)` and
//!   `FontId::*(<n>…)`;
//! * **space** — a gap given as a number: `add_space(<n>…)`,
//!   `vec2(<n>…)`, `Margin::same|symmetric(<n>…)`, a `Margin { … }` literal
//!   with a numeric field, and `inner_margin`/`outer_margin(<n>…)`;
//! * **radius** — `corner_radius(<n>…)`, `rounding(<n>…)`,
//!   `CornerRadius::same(<n>…)`, and the numeric radius a painter's
//!   `rect`/`rect_filled`/`rect_stroke` takes as its second argument;
//! * **style** — `set_style(`, `set_visuals(` and `style_mut(` anywhere but
//!   inside a `.scope(…)` call, the one sanctioned local variation (egui
//!   restores the style when the scope exits; everything else patches the
//!   context's one sticky global).
//!
//! "A number" means the argument STARTS with a numeric literal, so
//! `.size(14.0 * scale)` counts and `.size(text::BODY * scale)` does not.
//! Bevy `Color` is out of scope by type: world-space effect colours are not
//! egui's.
//!
//! # The ratchet is an EQUALITY
//!
//! [`FROZEN`] records each file's count of each kind. The test fails when a
//! file's count goes up — a new literal — and ALSO when it goes down without
//! the table moving with it, so a migration card that clears literals must
//! lower its rows in the same commit. A `>=` floor would let a row sit above
//! the truth, and that slack would hide the next literal added in the same
//! file. A file whose counts reach zero leaves the table. The last migration
//! card empties it.
//!
//! On a failure the message prints the whole table as it stands, ready to
//! paste — but paste it only for the rows your change was meant to LOWER. A
//! row that went up is a literal to replace with a token, not a number to
//! re-freeze.
//!
//! # Self-reported limitations
//!
//! A lexical scan, so a drift guard, not a proof. Comments and string literals
//! are blanked first, and `#[cfg(test)]` modules are skipped (a test pinning a
//! pixel value is not drawing UI). What it cannot see: a value reached
//! through a local (`let gap = 6.0; ui.add_space(gap)` counts nothing), a
//! colour built some other way (`Color32::from(…)`, `Rgba`), a field
//! assigned a bare number (`ui.spacing_mut().item_spacing.x = 20.0;` — the
//! `vec2(…)` form of the same assignment IS counted), and a `ctx.style_mut`
//! written inside a `ui.scope` closure, which is still global.

mod common;

use common::source_audit::{
    blank_comments_and_strings, blank_test_modules, load_sources, param_list, repo_path,
    split_params, SourceFile, TypeAliases,
};
use regex::Regex;
use std::collections::BTreeMap;
use std::fmt;

/// The one file allowed to write UI literals.
const THEME: &str = "src/ui/theme.rs";

/// What each file is allowed today, kind by kind. EQUALITY: see the module
/// docs. Sorted by path; a file absent from the table must count zero.
/// One row per file, so a migration card's diff reads as the rows it lowered.
#[rustfmt::skip]
const FROZEN: &[(&str, Counts)] = &[
    ("src/states/animation_sandbox/ui.rs", Counts { colour: 16, size: 11, space: 13, radius: 12, style: 1 }),
    ("src/states/arena_layout_debug.rs", Counts { colour: 14, size: 3, space: 2, radius: 3, style: 0 }),
    ("src/states/configure_match_ui.rs", Counts { colour: 58, size: 21, space: 25, radius: 20, style: 1 }),
    ("src/states/encyclopedia/abilities.rs", Counts { colour: 0, size: 17, space: 11, radius: 0, style: 0 }),
    ("src/states/encyclopedia/auras.rs", Counts { colour: 2, size: 16, space: 13, radius: 1, style: 0 }),
    ("src/states/encyclopedia/classes.rs", Counts { colour: 0, size: 3, space: 4, radius: 0, style: 0 }),
    ("src/states/encyclopedia/items.rs", Counts { colour: 5, size: 14, space: 9, radius: 0, style: 0 }),
    ("src/states/encyclopedia/mod.rs", Counts { colour: 10, size: 7, space: 8, radius: 3, style: 1 }),
    ("src/states/encyclopedia/search.rs", Counts { colour: 0, size: 3, space: 4, radius: 0, style: 0 }),
    ("src/states/encyclopedia/topic.rs", Counts { colour: 1, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/encyclopedia/widget.rs", Counts { colour: 1, size: 13, space: 10, radius: 8, style: 0 }),
    ("src/states/main_menu.rs", Counts { colour: 12, size: 5, space: 11, radius: 3, style: 0 }),
    ("src/states/mod.rs", Counts { colour: 39, size: 27, space: 52, radius: 0, style: 2 }),
    ("src/states/play_match/auras.rs", Counts { colour: 8, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/camera.rs", Counts { colour: 6, size: 5, space: 1, radius: 0, style: 0 }),
    ("src/states/play_match/combat_ai.rs", Counts { colour: 4, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/combat_core/auto_attack.rs", Counts { colour: 2, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/combat_core/casting.rs", Counts { colour: 6, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/effects/berserker_rage.rs", Counts { colour: 1, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/effects/bloom.rs", Counts { colour: 1, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/effects/divine_shield.rs", Counts { colour: 1, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/effects/holy_shock.rs", Counts { colour: 3, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/effects/travel_form.rs", Counts { colour: 1, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/projectiles.rs", Counts { colour: 2, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/rendering/combat_log.rs", Counts { colour: 35, size: 9, space: 9, radius: 3, style: 0 }),
    ("src/states/play_match/rendering/effects/floating_text.rs", Counts { colour: 2, size: 2, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/rendering/effects/speech_bubbles.rs", Counts { colour: 14, size: 1, space: 1, radius: 7, style: 0 }),
    ("src/states/play_match/rendering/hud.rs", Counts { colour: 35, size: 9, space: 6, radius: 1, style: 0 }),
    ("src/states/play_match/rendering/mod.rs", Counts { colour: 1, size: 0, space: 0, radius: 0, style: 0 }),
    ("src/states/play_match/rendering/overlays.rs", Counts { colour: 19, size: 12, space: 0, radius: 2, style: 0 }),
    ("src/states/play_match/rendering/team_frames.rs", Counts { colour: 38, size: 9, space: 2, radius: 14, style: 0 }),
    ("src/states/results_ui.rs", Counts { colour: 27, size: 19, space: 19, radius: 11, style: 1 }),
    ("src/states/view_combatant_ui.rs", Counts { colour: 65, size: 45, space: 48, radius: 15, style: 1 }),
];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Counts {
    colour: usize,
    size: usize,
    space: usize,
    radius: usize,
    style: usize,
}

impl Counts {
    fn is_zero(&self) -> bool {
        *self == Counts::default()
    }

    fn total(&self) -> usize {
        self.colour + self.size + self.space + self.radius + self.style
    }
}

impl fmt::Debug for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Counts {{ colour: {}, size: {}, space: {}, radius: {}, style: {} }}",
            self.colour, self.size, self.space, self.radius, self.style
        )
    }
}

/// The scanner. Patterns compile once; [`Scanner::count`] reads one file's
/// already-blanked code.
struct Scanner {
    colour_ctor: Regex,
    colour_named: Regex,
    size: Regex,
    space: Regex,
    margin_literal: Regex,
    radius: Regex,
    painter_rect: Regex,
    style: Regex,
    scope: Regex,
    numeric_start: Regex,
}

/// An argument list that opens on a numeric literal: `(14.0`, `( -2.`, `(6`.
const NUM_ARG: &str = r"\(\s*-?\s*[0-9]";

impl Scanner {
    fn new() -> Self {
        let re = |s: &str| Regex::new(s).expect("audit pattern must compile");
        Scanner {
            colour_ctor: re(r"\bColor32\s*::\s*from_\w+\s*\("),
            colour_named: re(r"\bColor32\s*::\s*([A-Z][A-Z0-9_]*)\b"),
            size: re(&format!(r"(?:\.\s*size|\bFontId\s*::\s*\w+)\s*{NUM_ARG}")),
            space: re(&format!(
                r"(?:\badd_space|\bvec2|\bMargin\s*::\s*(?:same|symmetric)|\b(?:inner|outer)_margin)\s*{NUM_ARG}"
            )),
            margin_literal: re(r"\bMargin\s*\{[^}]*:\s*-?\s*[0-9]"),
            radius: re(&format!(
                r"(?:\bcorner_radius|\.\s*rounding|\bCornerRadius\s*::\s*same)\s*{NUM_ARG}"
            )),
            painter_rect: re(r"\.\s*rect(?:_filled|_stroke)?\s*\("),
            style: re(r"\b(?:set_style|set_visuals|style_mut)\s*\("),
            scope: re(r"\.\s*scope\w*\s*\("),
            numeric_start: re(r"^\s*-?\s*[0-9]"),
        }
    }

    /// Count `code`, which must already have comments, strings and test
    /// modules blanked.
    fn count(&self, code: &str) -> Counts {
        let colour = self.colour_ctor.find_iter(code).count()
            + self
                .colour_named
                .captures_iter(code)
                .filter(|c| &c[1] != "TRANSPARENT")
                .count();

        let size = self.size.find_iter(code).count();

        let space =
            self.space.find_iter(code).count() + self.margin_literal.find_iter(code).count();

        let painter_radius = self
            .painter_rect
            .find_iter(code)
            .filter(|m| {
                param_list(code, m.end() - 1)
                    .map(|args| {
                        split_params(&args)
                            .get(1)
                            .is_some_and(|radius| self.numeric_start.is_match(radius))
                    })
                    .unwrap_or(false)
            })
            .count();
        let radius = self.radius.find_iter(code).count() + painter_radius;

        // Spans of every `.scope(…)` argument list; a style call inside one is
        // the sanctioned local form.
        let scopes: Vec<(usize, usize)> = self
            .scope
            .find_iter(code)
            .filter_map(|m| {
                let open = m.end() - 1;
                param_list(code, open).map(|args| (open, open + args.len() + 1))
            })
            .collect();
        let style = self
            .style
            .find_iter(code)
            .filter(|m| !scopes.iter().any(|&(a, b)| a < m.start() && m.start() < b))
            .count();

        Counts {
            colour,
            size,
            space,
            radius,
            style,
        }
    }
}

/// The text this audit counts in: comments and literal contents blanked,
/// `#[cfg(test)]` modules blanked, aliases resolved (so `use egui::Color32 as
/// C;` does not hide a `C::from_rgb`).
fn readable_code(raw: &str) -> String {
    let code = blank_test_modules(&blank_comments_and_strings(raw));
    TypeAliases::from_source(&code).expand(&code)
}

/// Every file under `src/` but the theme, counted.
fn measure() -> BTreeMap<String, Counts> {
    let scanner = Scanner::new();
    let sources: Vec<SourceFile> = load_sources(&["src"]).expect("src/ must be readable");
    sources
        .iter()
        .map(|f| (f.rel_display().replace('\\', "/"), f))
        .filter(|(path, _)| path != THEME)
        .map(|(path, f)| (path, scanner.count(&readable_code(&f.raw))))
        .collect()
}

fn render_table(measured: &BTreeMap<String, Counts>) -> String {
    measured
        .iter()
        .filter(|(_, c)| !c.is_zero())
        .map(|(path, c)| format!("    (\"{path}\", {c:?}),"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn ui_literals_match_the_frozen_table() {
    assert!(
        repo_path(THEME).is_file(),
        "{THEME} is the one file exempt from this audit, and it does not exist — \
         the exemption is stale"
    );

    let measured = measure();
    let frozen: BTreeMap<&str, Counts> = FROZEN.iter().copied().collect();
    assert_eq!(frozen.len(), FROZEN.len(), "FROZEN names a file twice");

    let mut failures = Vec::new();
    for (path, counts) in &measured {
        let allowed = frozen.get(path.as_str()).copied().unwrap_or_default();
        if *counts != allowed {
            failures.push(format!(
                "{path}\n      frozen   {allowed:?}\n      measured {counts:?}"
            ));
        }
    }
    for (path, counts) in &frozen {
        if !measured.contains_key(*path) {
            failures.push(format!(
                "{path}: in FROZEN ({counts:?}) but no such file under src/ — remove its row"
            ));
        }
        if counts.is_zero() {
            failures.push(format!("{path}: a row of zeros — remove it"));
        }
    }

    // Non-vacuity: the tree carries hundreds of literals today, and the scan
    // must see them. When the last migration card empties FROZEN, this floor
    // goes with it.
    let total: usize = measured.values().map(Counts::total).sum();
    assert!(
        total > 0 || FROZEN.is_empty(),
        "the scan counted nothing in src/ while FROZEN still allows literals — \
         the scanner is broken, not the tree"
    );

    assert!(
        failures.is_empty(),
        "UI literals outside {THEME} no longer match the frozen table.\n\n\
         A count that went UP is a new literal: use the `ui::theme` token for that role \
         (add one there, by role, if the scale lacks it) instead of re-freezing the row.\n\
         A count that went DOWN is progress: lower the row in FROZEN in this same commit.\n\n  \
         {}\n\nThe table as measured now:\n{}",
        failures.join("\n  "),
        render_table(&measured)
    );
}

/// The audit is only as good as its scanner, so plant one offender of every
/// pattern and require each to be caught exactly once — and the sanctioned
/// and blanked forms to be caught not at all. A regex that stops matching
/// fails here rather than letting the ratchet pass over nothing.
#[test]
fn the_scanner_catches_every_planted_offender() {
    let scanner = Scanner::new();
    let one = |src: &str| scanner.count(&readable_code(src));

    let colour = Counts {
        colour: 1,
        ..Counts::default()
    };
    for src in [
        "let c = Color32::from_rgb(20, 20, 30);",
        "let c = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200);",
        "let c = egui::Color32::from_rgba_premultiplied(0, 0, 0, 200);",
        "let c = Color32::from_gray(90);",
        "let c = Color32::from_black_alpha(150);",
        "let c = Color32::from_white_alpha(40);",
        "let c = Color32::WHITE;",
        "let c = egui::Color32::LIGHT_GRAY;",
        "use bevy_egui::egui::Color32 as C; let c = C::GOLD;",
    ] {
        assert_eq!(one(src), colour, "`{src}`");
    }

    let size = Counts {
        size: 1,
        ..Counts::default()
    };
    for src in [
        "RichText::new(\"x\").size(14.0)",
        "RichText::new(\"x\").size(14.0 * ui_scale)",
        "egui::FontId::proportional(13.5)",
        "egui::FontId::new(12.0, egui::FontFamily::Monospace)",
    ] {
        assert_eq!(one(src), size, "`{src}`");
    }

    let space = Counts {
        space: 1,
        ..Counts::default()
    };
    for src in [
        "ui.add_space(8.0);",
        "ui.add_space(-4.0);",
        "egui::vec2(200.0, 48.0)",
        "egui::Margin::same(14)",
        "egui::Margin::symmetric(16, 12)",
        "egui::Margin { left: 4, right: 4, top: 2, bottom: 2 }",
        "frame.inner_margin(12.0)",
    ] {
        assert_eq!(one(src), space, "`{src}`");
    }

    let radius = Counts {
        radius: 1,
        ..Counts::default()
    };
    for src in [
        "frame.corner_radius(6.0)",
        "egui::CornerRadius::same(6)",
        "painter.rect_filled(rect, 2.0, fill)",
        "painter.rect_stroke(rect.shrink(1.0), 3.0, stroke, egui::StrokeKind::Inside)",
        "painter.rect(rect, 4.0, fill, stroke, kind)",
    ] {
        assert_eq!(one(src), radius, "`{src}`");
    }

    let style = Counts {
        style: 1,
        ..Counts::default()
    };
    for src in [
        "ctx.set_style(style);",
        "ctx.set_visuals(egui::Visuals::dark());",
        "ctx.style_mut(|s| s.interaction.tooltip_delay = 0.0);",
        "ui.style_mut().spacing.item_spacing.y = 2.0;",
    ] {
        assert_eq!(one(src), style, "`{src}`");
    }

    // Not offenders: the sanctioned forms, tokens, and literals in prose.
    for src in [
        "let c = Color32::TRANSPARENT;",
        "ui.scope(|ui| { ui.style_mut().spacing.item_spacing.y = 0.0; });",
        "ui.add_space(theme::space::SM);",
        "RichText::new(\"x\").size(text::BODY * scale)",
        "painter.rect_filled(rect, radius, fill)",
        "painter.rect_filled(rect, egui::CornerRadius::ZERO, fill)",
        "// ui.add_space(8.0); Color32::from_rgb(1, 2, 3)",
        "let s = \"Color32::WHITE .size(12.0) ctx.set_style(s)\";",
        "#[cfg(test)]\nmod tests {\n    fn t() { let _ = Color32::RED; }\n}",
    ] {
        assert_eq!(one(src), Counts::default(), "`{src}` must count nothing");
    }

    // And the five kinds are kept apart when they share a file.
    let mixed = one("let c = Color32::from_rgb(1, 2, 3);\n\
         ui.label(RichText::new(\"x\").size(12.0).color(Color32::WHITE));\n\
         ui.add_space(4.0);\n\
         painter.rect_filled(r, 3.0, c);\n\
         ctx.set_style(s);");
    assert_eq!(
        mixed,
        Counts {
            colour: 2,
            size: 1,
            space: 1,
            radius: 1,
            style: 1,
        }
    );
}
