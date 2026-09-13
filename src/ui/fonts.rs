//! The game's egui font stack.
//!
//! ONE definition of the fonts the player actually sees, shared by the
//! graphical client and by the offscreen `egui_kittest` snapshot harnesses.
//!
//! Before this existed the client installed Rajdhani in a Startup system while
//! the harnesses rendered with egui's default stack, so every baseline PNG
//! pictured a screen nobody plays: a glyph with no Rajdhani coverage, or a
//! label that only fits under the defaults' metrics, could pass the suite and
//! be wrong in the game. Anything that changes the stack belongs here so the
//! harnesses cannot drift from the client again.

use bevy_egui::egui;

/// The client's [`egui::FontDefinitions`]: egui's defaults with Rajdhani Bold
/// and Rajdhani Regular inserted ahead of them in the proportional family.
///
/// Rajdhani leads, so it supplies every glyph it covers; the egui defaults stay
/// behind it as the fallback chain (this is what resolves emoji through the
/// monochrome NotoEmoji face, and what leaves an uncovered codepoint — `⌂`
/// U+2302, say — as a tofu box in the client as well as in a snapshot).
pub fn game_font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "rajdhani_bold".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Rajdhani-Bold.ttf")).into(),
    );
    fonts.font_data.insert(
        "rajdhani_regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Rajdhani-Regular.ttf"))
            .into(),
    );

    // Bold first (headings and the HUD's chrome read as the game's voice),
    // Regular immediately behind it, then egui's defaults.
    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    proportional.insert(0, "rajdhani_bold".to_owned());
    proportional.insert(1, "rajdhani_regular".to_owned());

    fonts
}

/// Install [`game_font_definitions`] on `ctx`.
///
/// Idempotent: `Context::set_fonts` compares against the installed definitions
/// and does nothing when they already match, so a snapshot harness can call
/// this at the top of its per-frame app closure without rebuilding the atlas
/// every frame.
pub fn install_game_fonts(ctx: &egui::Context) {
    ctx.set_fonts(game_font_definitions());
}
