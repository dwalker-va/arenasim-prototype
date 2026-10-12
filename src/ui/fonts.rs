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
//!
//! Nothing installs these directly: [`crate::ui::install`] is the one call the
//! client and every harness make.

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

    // The prose family (decision D3): Regular leads, so multi-sentence text
    // gets the lighter weight the proportional chain can never reach (Bold
    // covers every Latin glyph, so Regular behind it draws nothing). The rest
    // of the proportional chain follows as the fallback.
    let mut body = vec!["rajdhani_regular".to_owned()];
    body.extend(
        fonts.families[&egui::FontFamily::Proportional]
            .iter()
            .filter(|name| *name != "rajdhani_regular")
            .cloned(),
    );
    fonts.families.insert(body_family_name(), body);

    fonts
}

/// The name of the prose family: Rajdhani Regular, for descriptions, procs
/// and tooltip bodies. Everything else stays Bold, the game's voice.
const BODY_FAMILY: &str = "Body";

fn body_family_name() -> egui::FontFamily {
    egui::FontFamily::Name(BODY_FAMILY.into())
}

/// The prose family, for [`egui::FontId::new`] or `RichText::family`.
///
/// egui panics on a named family that is not installed, and `set_fonts` only
/// takes effect at the NEXT frame — so on the very first frame after
/// [`crate::ui::install`] (a fresh harness, the client's first paint) the
/// family is not there yet. Until it is, this answers the proportional
/// family, which is what that frame's text is drawn in anyway.
pub fn body_family(ctx: &egui::Context) -> egui::FontFamily {
    let body = body_family_name();
    if ctx.fonts(|f| f.families().contains(&body)) {
        body
    } else {
        egui::FontFamily::Proportional
    }
}
