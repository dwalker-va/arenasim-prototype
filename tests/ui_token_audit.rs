//! UI values come from `ui::theme`: no literal is written anywhere else.
//!
//! The design system (`docs/design/proposals/2026-10-11-ui-design-system.md`
//! §3.4a) chooses each colour, text size, gap, radius and icon size once, in
//! `src/ui/theme.rs`, by role. This audit fails on any value chosen
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
//!   context's one sticky global). `ui::install`'s own `set_style`, which sets
//!   that global once, is exempt by count ([`INSTALL`]).
//!
//! "A number" means the argument STARTS with a numeric literal, so
//! `.size(14.0 * scale)` counts and `.size(text::BODY * scale)` does not.
//! Bevy `Color` is out of scope by type: world-space effect colours are not
//! egui's.
//!
//! # Zero, everywhere
//!
//! Every file outside the theme must count zero of every kind, and there is
//! no per-file allowance. A literal anywhere else is a value to replace with
//! the `ui::theme` token for its role — add one there, by role, if the theme
//! lacks it — never a number to allow.
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

/// The one sanctioned style call: `ui::install` sets the whole `Style` once,
/// for the client and every harness (design doc §3.2). Exempt by COUNT, not
/// by file — exactly this many style calls there, and every other literal in
/// the file still counts.
const INSTALL: (&str, usize) = ("src/ui/mod.rs", 1);

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

/// Every file under `src/` but the theme, counted, with `ui::install`'s own
/// style call taken off its file's count once it is confirmed to be exactly
/// [`INSTALL`]'s.
fn measure() -> BTreeMap<String, Counts> {
    let scanner = Scanner::new();
    let sources: Vec<SourceFile> = load_sources(&["src"]).expect("src/ must be readable");
    let mut measured: BTreeMap<String, Counts> = sources
        .iter()
        .map(|f| (f.rel_display().replace('\\', "/"), f))
        .filter(|(path, _)| path != THEME)
        .map(|(path, f)| (path, scanner.count(&readable_code(&f.raw))))
        .collect();

    let (install_path, install_calls) = INSTALL;
    let install = measured.get_mut(install_path).unwrap_or_else(|| {
        panic!("{install_path} holds `ui::install`, and it does not exist — the exemption is stale")
    });
    assert_eq!(
        install.style, install_calls,
        "{install_path} must hold exactly `ui::install`'s {install_calls} style call(s); \
         a second one there is a patch the install exemption does not cover"
    );
    install.style = 0;
    measured
}

#[test]
fn no_ui_literal_outside_the_theme() {
    assert!(
        repo_path(THEME).is_file(),
        "{THEME} is the one file exempt from this audit, and it does not exist — \
         the exemption is stale"
    );

    let measured = measure();

    // Non-vacuity: the scan must have read the tree, and the scanner must see
    // the literals the theme itself is made of. A scan that reads nothing, or
    // whose patterns stopped matching, would pass every file below.
    assert!(
        measured.len() > 100,
        "the scan read only {} files under src/ — it is not reading the tree",
        measured.len()
    );
    let theme = Scanner::new().count(&readable_code(
        &std::fs::read_to_string(repo_path(THEME)).expect("the theme must be readable"),
    ));
    assert!(
        theme.colour > 0 && theme.space > 0,
        "the scanner counts nothing in {THEME} ({theme:?}), which is made of UI literals — \
         the scanner is broken, not the tree"
    );

    let offenders: Vec<String> = measured
        .iter()
        .filter(|(_, counts)| !counts.is_zero())
        .map(|(path, counts)| format!("{path}: {counts:?}"))
        .collect();
    assert!(
        offenders.is_empty(),
        "UI literals outside {THEME}. Use the `ui::theme` token for each value's role, \
         and if the theme lacks one, add it there by role in this same commit:\n  {}",
        offenders.join("\n  ")
    );
}

/// The audit is only as good as its scanner, so plant one offender of every
/// pattern and require each to be caught exactly once — and the sanctioned
/// and blanked forms to be caught not at all. A regex that stops matching
/// fails here rather than letting the audit pass over nothing.
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

/// Every checkbox is the kit's ([`arenasim::ui::widgets::Checkbox`]), never
/// egui's stock one: the stock box takes its corners from the installed
/// widget style, whose button radius rounds a 16pt box into a circle, so it
/// reads as a radio button. Lexical, like the audit above: it sees the
/// method form (`ui.checkbox(…)`) and the path form (`egui::Checkbox`,
/// `egui::widgets::Checkbox`), not a stock `Checkbox` imported bare.
#[test]
fn every_checkbox_is_the_kits() {
    let stock = stock_checkbox_pattern();
    let offenders: Vec<String> = load_sources(&["src"])
        .expect("src/ must be readable")
        .iter()
        .filter(|f| !f.rel_display().replace('\\', "/").starts_with("src/ui/"))
        .filter_map(|f| {
            let n = stock.find_iter(&readable_code(&f.raw)).count();
            (n > 0).then(|| format!("{} ({n})", f.rel_display()))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "stock egui checkboxes outside src/ui/ — draw `ui::widgets::checkbox` instead, \
         so the box is square: {offenders:?}"
    );
}

fn stock_checkbox_pattern() -> Regex {
    Regex::new(r"\.\s*checkbox\s*\(|\begui\s*::\s*(?:widgets\s*::\s*)?Checkbox\b")
        .expect("checkbox pattern must compile")
}

/// The checkbox guard's own planted offenders, and the kit's forms it must
/// let through.
#[test]
fn the_checkbox_guard_catches_the_stock_forms() {
    let stock = stock_checkbox_pattern();
    for src in [
        "ui.checkbox(&mut on, \"Loop\")",
        "ui.add(egui::Checkbox::new(&mut on, \"Loop\"))",
        "ui.add(egui::widgets::Checkbox::new(&mut on, \"Loop\"))",
    ] {
        assert_eq!(stock.find_iter(&readable_code(src)).count(), 1, "`{src}`");
    }
    for src in [
        "checkbox(ui, &mut on, \"Loop\")",
        "crate::ui::widgets::checkbox(ui, &mut on, \"Loop\")",
        "Checkbox::new(&mut on, \"Loop\").show(ui)",
    ] {
        assert_eq!(stock.find_iter(&readable_code(src)).count(), 0, "`{src}`");
    }
}
