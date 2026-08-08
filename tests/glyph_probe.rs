//! Offscreen preview of the banter vocabulary.
//!
//! The pool is pictographic — game icons and symbol glyphs, no words — so the
//! only way to judge whether a line READS is to look at one. This renders a
//! representative set the way a speech bubble draws them, on the same white
//! bubble background, so the vocabulary can be iterated without launching the
//! client and driving it to a countdown.
//!
//! Class portraits are drawn as framed placeholder plates here: the real
//! textures are Bevy assets that only exist in a running app, and kittest has
//! no Bevy. Judge the GRAMMAR — the order, spacing, and whether the glyph
//! sequence carries meaning — not the portrait art.
//!
//! ```bash
//! cargo test --release --test glyph_probe -- --ignored
//! UPDATE_SNAPSHOTS=1 cargo test --release --test glyph_probe -- --ignored
//! ```

use bevy_egui::egui;
use egui_kittest::Harness;

use arenasim::states::play_match::banter::vocab::{self, Span};
use arenasim::states::play_match::rendering::effects::draw_symbol;

const SCREEN: [f32; 2] = [720.0, 700.0];
const TEXT: f32 = 18.0;
const ICON: f32 = 20.0;

/// Representative resolved lines, as the renderer would receive them.
const LINES: &[(&str, &str)] = &[
    ("open/generic", "{ability:Mortal Strike} {sym:arrow} {class:Priest:2}"),
    ("open/reply", "{sym:yes}"),
    ("open/healer", "{class:Priest:2} {ability:Flash Heal} {sym:no}"),
    ("open/urgent", "{ability:Mortal Strike} {sym:arrow} {class:Priest:2}!"),
    ("open/paladin", "{ability:Divine Shield} !…"),
    ("open/rogue", "? ?…"),
    ("open/charge", "{ability:Charge} {sym:arrow} {class:Mage:2}"),
    ("open/self", "{class:Warrior:1} {ability:Mortal Strike} {sym:arrow} {class:Mage:2}"),
    ("correct", "{sym:no} {class:Paladin:2} {sym:arrow} {class:Rogue:2}"),
    ("correct/what", "… {class:Paladin:2} ?!"),
    ("switch", "! {sym:arrow} {class:Warlock:2}"),
    ("switch/healer", "{class:Priest:2} {ability:Flash Heal} {sym:no}!"),
    ("unresolved", "{sym:no} ? {sym:arrow} {class:Rogue:2}"),
];

fn team_tint(team: u8) -> egui::Color32 {
    if team == 1 {
        egui::Color32::from_rgb(100, 150, 255)
    } else {
        egui::Color32::from_rgb(255, 100, 100)
    }
}

/// Draw one line the way `render_speech_bubbles` does: measure the spans, size
/// a bubble around them, then lay them out left to right.
fn draw_bubble(ui: &egui::Ui, origin: egui::Pos2, line: &str) -> f32 {
    let font = egui::FontId::proportional(TEXT);
    let ctx = ui.ctx();
    let spans = vocab::parse(line);
    let measured: Vec<(Span, f32)> = spans
        .into_iter()
        .map(|span| {
            let w = match &span {
                Span::Text(t) => ctx
                    .fonts(|f| f.layout_no_wrap(t.clone(), font.clone(), egui::Color32::BLACK))
                    .size()
                    .x,
                _ => ICON,
            };
            (span, w)
        })
        .collect();

    let content_w: f32 = measured.iter().map(|(_, w)| *w).sum();
    let pad = egui::vec2(12.0, 6.0);
    let size = egui::vec2(content_w, ICON.max(TEXT)) + pad * 2.0;
    let rect = egui::Rect::from_min_size(origin, size);
    let painter = ui.painter();

    painter.rect_filled(rect, 6.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 240));
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(2.0, egui::Color32::BLACK),
        egui::StrokeKind::Outside,
    );

    let mut x = rect.min.x + pad.x;
    let mid = rect.center().y;
    for (span, w) in &measured {
        let icon = egui::Rect::from_min_size(
            egui::pos2(x, mid - ICON / 2.0),
            egui::vec2(ICON, ICON),
        );
        match span {
            Span::Text(t) => {
                painter.text(
                    egui::pos2(x, mid),
                    egui::Align2::LEFT_CENTER,
                    t,
                    font.clone(),
                    egui::Color32::BLACK,
                );
            }
            // Stand-in for the real portrait. The dark plate approximates
            // busy icon art; the team-coloured FRAME is what carries ownership
            // and is the part worth judging here. (Tinting the art itself was
            // tried in the client and muddied the class silhouette.)
            Span::Class(class, team) => {
                painter.rect_filled(icon, 3.0, egui::Color32::from_gray(40));
                painter.text(
                    icon.center(),
                    egui::Align2::CENTER_CENTER,
                    &class.name()[0..1],
                    egui::FontId::proportional(13.0),
                    egui::Color32::WHITE,
                );
                painter.rect_stroke(
                    icon,
                    3.0,
                    egui::Stroke::new(2.0, team_tint(*team)),
                    egui::StrokeKind::Inside,
                );
            }
            Span::Ability(_) => {
                painter.rect_filled(icon, 3.0, egui::Color32::from_rgb(70, 60, 40));
                painter.rect_stroke(
                    icon,
                    3.0,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 150, 90)),
                    egui::StrokeKind::Inside,
                );
            }
            // The real drawing routine, so the preview shows exactly the marks
            // the client draws rather than a stand-in.
            Span::Symbol(symbol) => draw_symbol(painter, *symbol, icon),
            Span::Unknown => {
                painter.rect_stroke(
                    icon,
                    3.0,
                    egui::Stroke::new(1.0, egui::Color32::RED),
                    egui::StrokeKind::Inside,
                );
            }
        }
        x += w;
    }
    size.y
}

#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn banter_vocabulary_preview() {
    let mut harness = Harness::builder().with_size(SCREEN).build(|ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(34, 38, 52)))
            .show(ctx, |ui| {
                let mut y = 16.0;
                for (label, line) in LINES {
                    ui.painter().text(
                        egui::pos2(16.0, y + 16.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::monospace(12.0),
                        egui::Color32::from_gray(150),
                    );
                    let h = draw_bubble(ui, egui::pos2(150.0, y), line);
                    y += h + 14.0;
                }
            });
    });
    harness.snapshot("banter_vocabulary");
}
