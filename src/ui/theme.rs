//! Design tokens: the ONLY place a UI colour, text size, gap, radius or icon
//! size is chosen.
//!
//! Every value is named by its ROLE, never its look (`TEXT_MUTED`, not
//! `GREY_142`), and belongs to exactly one of three layers that do not borrow
//! from each other:
//!
//! * **Chrome** ([`color`]): menus, panels, buttons. Gold on navy, few hues.
//! * **Data** ([`data`], [`class_color`], [`school_color`]): class, team,
//!   school, damage/healing/taken. Here hue carries meaning.
//! * **HUD** ([`hud`]): in-match only, under its own broadcast rules — labels
//!   stay white, and floating combat text reserves yellow, green and
//!   light-blue.
//!
//! A literal written anywhere else in `src/` is counted by
//! `tests/ui_token_audit.rs`, whose per-file table only ever moves toward
//! zero. Need a value the scale lacks? Add the token here, by role, in the
//! same commit as its first use.
//!
//! The values and their rationale are in
//! `docs/design/proposals/2026-10-11-ui-design-system.md` §2.2; the bench
//! beside it (`docs/design/benches/2026-10-11-ui-design-system-bench.html`)
//! draws every one of them.

use bevy_egui::egui;
use bevy_egui::egui::Color32;

use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::SpellSchool;

/// Chrome and text colours: the encyclopedia's gold-on-navy palette, renamed
/// by role.
pub mod color {
    use super::Color32;

    // Chrome
    /// Screen background.
    pub const BG: Color32 = Color32::from_rgb(20, 20, 30);
    /// Panel and button fill.
    pub const PANEL: Color32 = Color32::from_rgb(30, 30, 42);
    /// Hover, active tab.
    pub const RAISED: Color32 = Color32::from_rgb(37, 37, 56);
    /// Panel stroke, divider.
    pub const LINE: Color32 = Color32::from_rgb(60, 60, 80);
    /// Hover stroke, tooltip and modal edge.
    pub const LINE_STRONG: Color32 = Color32::from_rgb(86, 86, 122);
    /// The current row in a list.
    pub const SELECTED: Color32 = Color32::from_rgb(51, 65, 94);

    // Text
    /// Titles, the active state, the primary action.
    pub const ACCENT: Color32 = Color32::from_rgb(230, 204, 153);
    /// The primary action under the pointer: [`ACCENT`], lifted.
    pub const ACCENT_RAISED: Color32 = Color32::from_rgb(242, 222, 182);
    /// Body text.
    pub const TEXT: Color32 = Color32::from_rgb(230, 217, 191);
    /// Labels, secondary text.
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(142, 142, 166);
    /// Disabled text, hints.
    pub const TEXT_DIM: Color32 = Color32::from_rgb(102, 102, 126);
    /// Navigable names.
    pub const LINK: Color32 = Color32::from_rgb(127, 178, 229);

    // State
    /// Warnings.
    pub const WARN: Color32 = Color32::from_rgb(220, 170, 90);
    /// Destructive actions, the dead.
    pub const DANGER: Color32 = Color32::from_rgb(205, 110, 110);
    /// The idle edge of a destructive control: [`DANGER`] held back until
    /// hover, so a Remove button does not shout from every row.
    pub const DANGER_LINE: Color32 = Color32::from_rgb(90, 52, 52);

    // Surfaces
    /// Text and glyphs drawn ON an [`ACCENT`] fill: the Primary button's
    /// label, an on chip's.
    pub const ON_ACCENT: Color32 = BG;
    /// The dimming laid over a screen behind a modal.
    pub const SCRIM: Color32 = Color32::from_rgba_premultiplied(6, 6, 10, 153);

    // The Hero template: chrome laid over a live 3D scene.
    /// The light darkening over the whole scene, so bloom does not fight
    /// the text.
    pub const SCENE_TINT: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 70);
    /// The scene's edges, fading to transparent [`super::size::VIGNETTE`]
    /// inward.
    pub const SCENE_VIGNETTE: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 150);
    /// The centred card behind the wordmark and the menu.
    pub const HERO_CARD: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 120);
    /// A Hero button's fill: [`PANEL`], translucent so the scene shows
    /// through.
    pub const HERO_FILL: Color32 = Color32::from_rgba_premultiplied(24, 24, 33, 200);

    /// The tint an image is drawn with to keep its own colours.
    pub const UNTINTED: Color32 = Color32::WHITE;

    /// The empty part of a chrome bar (the kit's `resource_bar`). The HUD's
    /// bars use [`super::hud::BAR_TRACK`].
    pub const TRACK: Color32 = Color32::from_rgb(17, 17, 26);
}

/// Data colours: hue that carries meaning. One table each.
pub mod data {
    use super::Color32;

    /// Team 1. The HUD pair, used everywhere (decision D7); fills derive from
    /// it by alpha.
    pub const TEAM_1: Color32 = Color32::from_rgb(100, 150, 255);
    /// Team 2. The HUD pair, used everywhere (decision D7).
    pub const TEAM_2: Color32 = Color32::from_rgb(255, 100, 100);

    /// Damage done.
    pub const DAMAGE: Color32 = Color32::from_rgb(255, 150, 100);
    /// Healing done.
    pub const HEALING: Color32 = Color32::from_rgb(110, 215, 130);
    /// Damage taken.
    pub const TAKEN: Color32 = Color32::from_rgb(230, 110, 110);
    /// Killing blows.
    pub const KILLS: Color32 = Color32::from_rgb(255, 205, 90);
    /// Crowd control received, in seconds.
    pub const CC_RECEIVED: Color32 = Color32::from_rgb(180, 100, 255);

    /// A combatant who lived to the end of the match.
    pub const ALIVE: Color32 = Color32::from_rgb(120, 200, 120);
    /// A combatant who died in it.
    pub const DEAD: Color32 = Color32::from_rgb(205, 110, 110);

    /// A beneficial aura.
    pub const BUFF: Color32 = Color32::from_rgb(111, 174, 126);
    /// A harmful aura.
    pub const DEBUFF: Color32 = Color32::from_rgb(208, 106, 91);

    /// What equipment adds: an item tooltip's stat lines, a stat the worn
    /// gear raised, a socket holding a pick other than the class default.
    pub const ITEM_BONUS: Color32 = Color32::from_rgb(100, 255, 100);
}

/// HUD colours: in-match only (decision D6). The HUD keeps its broadcast look
/// and does not take the chrome palette.
pub mod hud {
    use super::Color32;

    /// Every HUD frame's fill: the team frames, the combat log, Time
    /// Controls, the camera window. One translucent near-black.
    /// (13, 13, 20) at alpha 235, premultiplied (egui's unmultiplied
    /// constructor is not `const`); `hud_panel_is_the_team_frame_fill` pins it.
    pub const HUD_PANEL: Color32 = Color32::from_rgba_premultiplied(12, 12, 19, 235);
    /// A team frame's resting edge.
    pub const HUD_PANEL_EDGE: Color32 = Color32::from_rgb(45, 45, 60);

    /// The countdown, Victory, buff borders.
    pub const HUD_GOLD: Color32 = Color32::from_rgb(255, 215, 0);
    /// Every HUD label. White, by the colour-budget rule.
    pub const HUD_LABEL: Color32 = Color32::WHITE;
    /// The outline drawn under a [`HUD_LABEL`] so it reads on any ground.
    pub const HUD_LABEL_OUTLINE: Color32 = Color32::BLACK;

    /// The empty part of a team frame's resource bar.
    pub const BAR_TRACK: Color32 = Color32::from_rgb(20, 20, 30);
    /// The empty part of a team frame's health bar.
    pub const HEALTH_TRACK: Color32 = Color32::from_rgb(28, 28, 34);
    /// The well behind a team frame's aura icon.
    pub const ICON_WELL: Color32 = Color32::from_rgb(20, 20, 20);

    // Health, by threshold: the team frames and the overhead bar share them.
    /// Above half health.
    pub const HEALTH_HIGH: Color32 = Color32::from_rgb(0, 200, 0);
    /// Above a quarter.
    pub const HEALTH_MID: Color32 = Color32::from_rgb(255, 200, 0);
    /// A quarter or less.
    pub const HEALTH_LOW: Color32 = Color32::from_rgb(200, 0, 0);
    /// A dead combatant's empty bar.
    pub const HEALTH_DEAD: Color32 = Color32::from_rgb(60, 60, 60);

    /// A living combatant's health fill at `fraction` of its maximum: the
    /// one place the thresholds are chosen.
    pub fn health_color(fraction: f32) -> Color32 {
        if fraction > 0.5 {
            HEALTH_HIGH
        } else if fraction > 0.25 {
            HEALTH_MID
        } else {
            HEALTH_LOW
        }
    }

    // Resources.
    pub const MANA: Color32 = Color32::from_rgb(80, 150, 255);
    pub const ENERGY: Color32 = Color32::from_rgb(255, 255, 100);
    pub const RAGE: Color32 = Color32::from_rgb(255, 80, 80);

    // Aura borders on the team frames ([`HUD_GOLD`] for a buff).
    /// A harmful aura.
    pub const AURA_DEBUFF: Color32 = Color32::from_rgb(200, 50, 50);
    /// Hard crowd control: brighter than [`AURA_DEBUFF`].
    pub const AURA_HARD_CC: Color32 = Color32::from_rgb(255, 60, 60);

    // Cast bars.
    /// The empty part of a cast or channel bar.
    pub const CAST_BAR_TRACK: Color32 = Color32::from_rgb(15, 15, 20);
    /// A cast bar's fill, WoW's orange: a cast an interrupt can take.
    pub const CAST_BAR_FILL: Color32 = Color32::from_rgb(255, 180, 50);
    /// A cast bar's edge, beside [`CAST_BAR_FILL`].
    pub const CAST_BAR_EDGE: Color32 = Color32::from_rgb(255, 200, 100);
    /// An UNINTERRUPTIBLE cast's fill (Aimed Shot): WoW's grey bar, the tell
    /// that no interrupt takes it.
    pub const CAST_BAR_UNINTERRUPTIBLE_FILL: Color32 = Color32::from_rgb(140, 140, 146);
    /// An uninterruptible cast's edge, beside [`CAST_BAR_UNINTERRUPTIBLE_FILL`].
    pub const CAST_BAR_UNINTERRUPTIBLE_EDGE: Color32 = Color32::from_rgb(196, 196, 204);
    /// An interrupted cast or channel, while "INTERRUPTED" shows on its bar.
    pub const CAST_BAR_INTERRUPTED_FILL: Color32 = Color32::from_rgb(150, 20, 20);
    /// An interrupted bar's edge, beside [`CAST_BAR_INTERRUPTED_FILL`].
    pub const CAST_BAR_INTERRUPTED_EDGE: Color32 = Color32::from_rgb(220, 50, 50);
    /// A channel bar's fill: teal, distinct from the cast bar's orange. It
    /// drains rather than fills.
    pub const CHANNEL_BAR_FILL: Color32 = Color32::from_rgb(50, 200, 150);
    /// A channel bar's edge, beside [`CHANNEL_BAR_FILL`].
    pub const CHANNEL_BAR_EDGE: Color32 = Color32::from_rgb(100, 220, 180);
}

/// Floating combat text: the ONLY place its hues are written. Yellow, green
/// and light-blue are reserved here for damage, healing and absorbs (the
/// colour-budget rule); nothing else in the game may draw text in them.
pub mod fct {
    use super::Color32;

    /// Ability damage, DoT ticks included.
    pub const DAMAGE: Color32 = Color32::from_rgb(255, 255, 0);
    /// Auto-attack damage.
    pub const AUTO: Color32 = Color32::WHITE;
    /// Every heal: direct, over time, bloom.
    pub const HEAL: Color32 = Color32::from_rgb(100, 255, 100);
    /// Damage an absorb shield took.
    pub const ABSORB: Color32 = Color32::from_rgb(100, 180, 255);
    /// A hit or effect the target was immune to. Shares the damage yellow:
    /// it reads as the damage that did not land.
    pub const IMMUNE: Color32 = DAMAGE;
    /// A named state the combatant entered: Travel Form, Berserker Rage.
    pub const STATE: Color32 = Color32::WHITE;
    /// Divine Shield, the one invulnerability: the HUD's gold.
    pub const INVULNERABLE: Color32 = super::hud::HUD_GOLD;
    /// The outline drawn around every floating number.
    pub const OUTLINE: Color32 = Color32::BLACK;
}

/// The type scale, in points. Rajdhani Bold unless a call site says otherwise.
///
/// The HUD scales these by its zoom factor (`hud.rs` `ui_scale`); it uses the
/// same steps multiplied, never sizes of its own.
pub mod text {
    /// The main-menu logo, and nothing else.
    pub const WORDMARK: f32 = 72.0;
    /// VS, the victory banner.
    pub const DISPLAY: f32 = 42.0;
    /// A screen title.
    pub const TITLE: f32 = 30.0;
    /// A panel title.
    pub const H2: f32 = 20.0;
    /// A section or row title.
    pub const H3: f32 = 16.0;
    /// Body text and buttons.
    pub const BODY: f32 = 14.0;
    /// Labels, stat rows.
    pub const SMALL: f32 = 12.0;
    /// Column headers, badges.
    pub const MICRO: f32 = 10.0;
}

/// The spacing scale, in points. A screen's margin is [`XL`](space::XL)
/// horizontally and [`LG`](space::LG) vertically, on every screen.
pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
    pub const XXXL: f32 = 48.0;
}

/// Corner radii (egui's `CornerRadius` unit).
pub mod radius {
    /// Chips, rows, icons.
    pub const SM: u8 = 3;
    /// Panels, buttons, tooltips.
    pub const MD: u8 = 6;
    /// Modals, team columns.
    pub const LG: u8 = 10;
}

/// Icon edge lengths, in points.
pub mod icon {
    /// Inline glyph-sized icons.
    pub const XS: f32 = 16.0;
    /// Results rows, team-frame auras.
    pub const SM: f32 = 20.0;
    /// List rows, the timeline.
    pub const MD: f32 = 24.0;
    /// Index tiles.
    pub const LG: f32 = 36.0;
    /// Choice tiles, the Configure class icon.
    pub const XL: f32 = 48.0;
    /// A detail page's header icon.
    pub const HERO: f32 = 56.0;
}

/// Stroke widths, in points.
pub mod stroke {
    /// Every panel, row, button and tooltip edge.
    pub const HAIRLINE: f32 = 1.0;
    /// One dash of a dashed edge (the empty state's outline).
    pub const DASH: f32 = 4.0;
    /// The gap between two dashes of a dashed edge.
    pub const DASH_GAP: f32 = 3.0;
    /// A glyph drawn as strokes: the checkbox's tick.
    pub const MARK: f32 = 2.0;
}

/// Fixed control dimensions, in points: the size a component is drawn at,
/// as opposed to the [`space`] between components.
pub mod size {
    /// A Secondary, Ghost or Danger button's height.
    pub const BUTTON: f32 = 32.0;
    /// The Primary button's height: one per screen, and the tallest control
    /// on it.
    pub const BUTTON_PRIMARY: f32 = 44.0;
    /// An Icon button's edge: square, glyph only.
    pub const BUTTON_ICON: f32 = 28.0;
    /// A screen header's navigation buttons.
    pub const NAV: f32 = 26.0;
    /// A tab.
    pub const TAB: f32 = 30.0;
    /// A chip.
    pub const CHIP: f32 = 22.0;
    /// A checkbox's box: square, whatever the label beside it.
    pub const CHECKBOX: f32 = 16.0;
    /// A screen-wide search field: a primary control on a Document screen,
    /// sized to say so.
    pub const SEARCH: f32 = 34.0;
    /// A resource bar's thickness.
    pub const BAR: f32 = 8.0;
    /// The widest a tooltip grows before it wraps.
    pub const TOOLTIP_MAX_WIDTH: f32 = 320.0;
    /// A choice tile's width: an [`super::icon::XL`] icon over a two-line
    /// caption.
    pub const CHOICE_TILE: f32 = 104.0;
    /// The widest a modal grows.
    pub const MODAL_MAX_WIDTH: f32 = 420.0;
    /// The narrowest a Hero button is drawn, so the menu reads as a column.
    pub const HERO_BUTTON_WIDTH: f32 = 280.0;
    /// How far in from a scene's edge its vignette fades out.
    pub const VIGNETTE: f32 = 180.0;
}

/// A class's colour as egui draws it, from the one source,
/// [`CharacterClass::color`].
///
/// Only the UI layer reads this; the 3D bodies keep their own, deliberately
/// distinct mesh palette.
pub fn class_color(class: CharacterClass) -> Color32 {
    let c = class.color().to_srgba();
    Color32::from_rgb(
        (c.red * 255.0) as u8,
        (c.green * 255.0) as u8,
        (c.blue * 255.0) as u8,
    )
}

/// `c` multiplied toward black by `factor` (1.0 = unchanged, below 1 =
/// darker), alpha kept: how a token is drawn receded — the Results screen's
/// defeated team — without choosing a second colour for it.
pub fn darken(c: Color32, factor: f32) -> Color32 {
    let channel = |v: u8| (v as f32 * factor) as u8;
    Color32::from_rgba_premultiplied(channel(c.r()), channel(c.g()), channel(c.b()), c.a())
}

/// `c` moved toward white by `amount` (0.0 = unchanged, 1.0 = white), alpha
/// kept: how a token is drawn brightened — the Hero wordmark's pulse —
/// without choosing a second colour for it.
pub fn lighten(c: Color32, amount: f32) -> Color32 {
    let channel = |v: u8| (v as f32 + (255 - v) as f32 * amount).round() as u8;
    Color32::from_rgba_premultiplied(channel(c.r()), channel(c.g()), channel(c.b()), c.a())
}

/// `c` at unmultiplied alpha `alpha`: how a token fades (floating combat
/// text in its last half second) without choosing a second colour for it.
pub fn with_alpha(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// A spell school's colour as egui draws it, from the one source,
/// [`SpellSchool::color_rgb8`].
pub fn school_color(school: SpellSchool) -> Color32 {
    let (r, g, b) = school.color_rgb8();
    Color32::from_rgb(r, g, b)
}

/// The whole egui [`egui::Style`] every screen draws under: what stock egui
/// widgets (a `ui.button`, a `selectable_label`, a text field, a window, a
/// hover card) look like when a screen does not paint its own. Not a
/// checkbox: the stock one takes these widgets' [`radius::MD`] and draws a
/// circle, so every checkbox is the kit's ([`crate::ui::widgets::checkbox`]),
/// held there by `tests/ui_token_audit.rs`.
///
/// [`crate::ui::install`] sets it ONCE, for the client and every harness. A
/// screen never patches it — egui keeps one sticky `Style` per context, so a
/// patch leaked into every screen reached after it (design doc §1.2, R1). A
/// genuinely local variation goes in `ui.scope(|ui| { ui.style_mut()… })`,
/// which egui restores on exit.
///
/// The interactive states mirror the kit's Secondary button
/// ([`crate::ui::widgets::Variant::Secondary`]), so a stock button and a kit
/// button side by side read as one control. Popups and tooltips take
/// [`crate::ui::widgets::tooltip_frame`]'s look: [`color::BG`] fill, a
/// [`color::LINE_STRONG`] edge, [`radius::MD`] corners, the same margin.
pub fn style() -> egui::Style {
    let mut style = egui::Style {
        visuals: visuals(),
        ..egui::Style::default()
    };

    // The type scale, for every widget that does not size its own text.
    use egui::{FontFamily, FontId, TextStyle};
    let proportional = |size| FontId::new(size, FontFamily::Proportional);
    style.text_styles = [
        (TextStyle::Small, proportional(text::SMALL)),
        (TextStyle::Body, proportional(text::BODY)),
        (TextStyle::Button, proportional(text::BODY)),
        (TextStyle::Heading, proportional(text::H2)),
        (
            TextStyle::Monospace,
            FontId::new(text::SMALL, FontFamily::Monospace),
        ),
    ]
    .into();

    // Hover cards: the tooltip frame's margin and width.
    style.spacing.menu_margin = egui::Margin::symmetric(space::MD as i8, space::SM as i8);
    style.spacing.tooltip_width = size::TOOLTIP_MAX_WIDTH;

    // One tooltip delay, set once: a linked icon answers the moment it is
    // hovered — that immediacy is the point of the widget.
    style.interaction.tooltip_delay = 0.0;

    style
}

fn visuals() -> egui::Visuals {
    let line = |c| egui::Stroke::new(stroke::HAIRLINE, c);
    let state = |fill, edge, label| egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: line(edge),
        fg_stroke: line(label),
        corner_radius: radius::MD.into(),
        expansion: 0.0,
    };

    let mut v = egui::Visuals::dark();
    v.widgets = egui::style::Widgets {
        noninteractive: state(color::PANEL, color::LINE, color::TEXT),
        inactive: state(color::PANEL, color::LINE, color::TEXT),
        hovered: state(color::RAISED, color::LINE_STRONG, color::ACCENT),
        active: state(color::RAISED, color::ACCENT, color::ACCENT),
        open: state(color::RAISED, color::LINE_STRONG, color::TEXT),
    };
    // Selected: the list row's language, a [`color::SELECTED`] fill with an
    // [`color::ACCENT`] edge and label. Not the chip's gold fill: a stock
    // selectable label often colours its own text (an item's quality gold, a
    // class colour), and that text vanishes on gold. Selected text in a field
    // takes the same pair.
    v.selection.bg_fill = color::SELECTED;
    v.selection.stroke = line(color::ACCENT);

    v.hyperlink_color = color::LINK;
    v.warn_fg_color = color::WARN;
    v.error_fg_color = color::DANGER;

    // Surfaces.
    v.panel_fill = color::BG;
    v.window_fill = color::BG;
    v.window_stroke = line(color::LINE_STRONG);
    v.window_corner_radius = radius::LG.into();
    v.menu_corner_radius = radius::MD.into();
    // Text fields and other wells; striped rows.
    v.extreme_bg_color = color::PANEL;
    v.faint_bg_color = color::PANEL;
    v.code_bg_color = color::PANEL;
    // A hover card is flat, like the kit's tooltip frame.
    v.popup_shadow = egui::Shadow::NONE;

    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `HUD_PANEL` is written premultiplied; it must stay the colour the team
    /// frames were drawn with, (13, 13, 20) at alpha 235.
    #[test]
    fn hud_panel_is_the_team_frame_fill() {
        assert_eq!(
            hud::HUD_PANEL,
            Color32::from_rgba_unmultiplied(13, 13, 20, 235)
        );
    }
}
