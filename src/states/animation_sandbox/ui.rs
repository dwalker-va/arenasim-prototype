//! The sandbox panel: selection, transport, and camera framing.
//!
//! Split the way `results_ui` and `main_menu` are: [`draw_sandbox_ui`] is a pure
//! `egui` function over plain data that returns the actions the user asked for,
//! and [`sandbox_ui`] is the thin Bevy wrapper that gathers the data and applies
//! them. That split is what lets `tests/animation_sandbox_snapshot.rs` render
//! this screen offscreen in a fraction of a second instead of launching the
//! client and driving it into the right state.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use super::super::configure_match_ui::ClassIcons;
use super::super::match_config::CharacterClass;
use super::super::play_match::ability_config::AbilityDefinitions;
use super::super::play_match::components::{CameraController, CameraMode, SpellIcons};
use super::playback::{entries_for_class, EntryFamily, SandboxEntry, SandboxPlayback};
use super::SandboxConfig;

// Palette shared with the Armory and main menu, so the sandbox reads as part of
// the same client rather than a debug panel bolted on.
const BG_COLOR: egui::Color32 = egui::Color32::from_rgb(20, 20, 30);
const TITLE_GOLD: egui::Color32 = egui::Color32::from_rgb(230, 204, 153);
const BUTTON_TEXT: egui::Color32 = egui::Color32::from_rgb(230, 217, 191);
const MUTED_TEXT: egui::Color32 = egui::Color32::from_rgb(102, 102, 102);
const TILE_FRAME: egui::Color32 = egui::Color32::from_rgb(60, 60, 80);
const TILE_BG: egui::Color32 = egui::Color32::from_rgb(30, 30, 42);
const SELECTED_BG: egui::Color32 = egui::Color32::from_rgb(51, 65, 94);
const WARN_AMBER: egui::Color32 = egui::Color32::from_rgb(220, 170, 90);

/// Edge length of an icon in a list row. The Armory renders the same art at
/// 64px, so anything much smaller stops being recognisable as a spell.
const ROW_ICON: f32 = 24.0;
/// Left inset shared by section headings, icon columns and buttons, so the
/// panels have ONE left edge instead of three within 34px.
const PANEL_INSET: f32 = 8.0;

/// Playback speeds offered in the sandbox.
///
/// A match offers 0.5x as its slowest rung, which is not slow enough to read
/// the phases of a fast effect. 0.1 and 0.25 are the reason this control exists
/// at all; 1x is kept so an entry can be judged at real speed.
const SPEEDS: [f32; 4] = [0.1, 0.25, 0.5, 1.0];

/// Where the camera can be placed in one action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraPreset {
    Front,
    ThreeQuarter,
    Side,
    Top,
}

impl CameraPreset {
    const ALL: [CameraPreset; 4] = [
        CameraPreset::Front,
        CameraPreset::ThreeQuarter,
        CameraPreset::Side,
        CameraPreset::Top,
    ];

    fn label(self) -> &'static str {
        match self {
            CameraPreset::Front => "Front",
            CameraPreset::ThreeQuarter => "3/4",
            CameraPreset::Side => "Side",
            CameraPreset::Top => "Top",
        }
    }

    /// Camera position for this preset, relative to the framed point.
    ///
    /// Framed wide enough that the pair sits clear of the edges: the side
    /// panels overlay the 3D view rather than shrinking it, so ~450px of the
    /// window is covered and a distance that looks fine in isolation puts the
    /// combatants under the panels.
    pub(crate) fn offset(self) -> Vec3 {
        match self {
            CameraPreset::Front => Vec3::new(0.0, 5.0, 23.0),
            CameraPreset::ThreeQuarter => Vec3::new(15.0, 9.0, 19.0),
            CameraPreset::Side => Vec3::new(26.0, 4.0, 0.0),
            CameraPreset::Top => Vec3::new(0.0, 30.0, 0.1),
        }
    }
}

/// A preset the user asked for, plus the one currently in effect.
#[derive(Resource, Default)]
pub struct PendingCameraPreset {
    pub requested: Option<CameraPreset>,
    /// Cleared the moment the user orbits the camera by hand, which is honest:
    /// a preset is a starting angle, not a mode.
    pub applied: Option<CameraPreset>,
}

/// One selectable row, flattened for drawing.
pub struct EntryRow {
    pub entry: SandboxEntry,
    pub family: EntryFamily,
    pub label: String,
    /// `None` in the snapshot harness, which has no Bevy textures.
    pub icon: Option<egui::TextureId>,
}

/// Everything the panel draws from. No Bevy types, so the snapshot harness can
/// build one by hand.
pub struct SandboxView {
    pub caster_class: CharacterClass,
    pub class_icons: Vec<(CharacterClass, Option<egui::TextureId>)>,
    pub dummy_enabled: bool,
    pub dummy_class: CharacterClass,
    pub rows: Vec<EntryRow>,
    pub selected: Option<SandboxEntry>,
    /// Label of the selected entry, and its data as key/value pairs — the
    /// numbers the animation is usually being checked against.
    pub selected_label: Option<String>,
    pub selected_details: Vec<(String, String)>,
    pub applied_preset: Option<CameraPreset>,
    pub looping: bool,
    pub paused: bool,
    pub speed: f32,
    pub elapsed: f32,
    pub duration: f32,
    /// Tail held after a pass before a loop restarts, drawn as a distinct
    /// segment so the eye can tell "pass over" from "hung".
    pub loop_tail: f32,
}

/// What the user asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SandboxAction {
    Back,
    SetCaster(CharacterClass),
    SetDummyEnabled(bool),
    SetDummyClass(CharacterClass),
    Preset(CameraPreset),
    Select(SandboxEntry, EntryFamily),
    Play,
    SetLooping(bool),
    TogglePause,
    Step,
    SetSpeed(f32),
}

/// Applies the house palette to egui's widget visuals.
///
/// Without this every button, checkbox and toggle renders in egui's defaults —
/// `rgb(60,60,60)` fills and a cyan accent — inside a carefully themed panel,
/// which puts TWO different "selected" colours on screen at once.
fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.panel_fill = BG_COLOR;
    v.window_fill = BG_COLOR;
    v.selection.bg_fill = SELECTED_BG;
    v.selection.stroke = egui::Stroke::new(1.0, TITLE_GOLD);

    v.widgets.inactive.weak_bg_fill = TILE_BG;
    v.widgets.inactive.bg_fill = TILE_BG;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, TILE_FRAME);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, BUTTON_TEXT);

    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(42, 42, 58);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(42, 42, 58);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, TITLE_GOLD);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, BUTTON_TEXT);

    v.widgets.active.weak_bg_fill = SELECTED_BG;
    v.widgets.active.bg_fill = SELECTED_BG;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, TITLE_GOLD);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, BUTTON_TEXT);
    ctx.set_style(style);
}

/// Section heading, matching the Armory's small-caps gold rules.
fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(text)
            .color(TITLE_GOLD)
            .size(12.0)
            .strong(),
    );
    ui.add_space(2.0);
}

/// Draws an icon into `rect`, or a framed empty slot when there is no texture.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, icon: Option<egui::TextureId>, dim: bool) {
    match icon {
        Some(id) => {
            painter.image(
                id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                if dim {
                    egui::Color32::from_gray(110)
                } else {
                    egui::Color32::WHITE
                },
            );
        }
        None => {
            // Draw NOTHING, but keep the space reserved so labels stay on one
            // column. A framed empty square reads as an unchecked checkbox —
            // the wrong affordance for a row that fires an action — and the
            // body animations have no icon by nature, not by failure, so there
            // is nothing there to stand in for.
        }
    }
}

/// A list row: icon, label, optional right-aligned tag, selection highlight.
fn icon_row(
    ui: &mut egui::Ui,
    icon: Option<egui::TextureId>,
    label: &str,
    tag: Option<&str>,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let height = ROW_ICON + 6.0;
    // Inset on BOTH sides, so the left panel's highlight does not bleed into
    // the panel divider while the right panel's sits 8px clear of it.
    let width = (ui.available_width() - PANEL_INSET).max(0.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, height),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );

    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 3.0, SELECTED_BG);
        painter.rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0, TITLE_GOLD),
            egui::StrokeKind::Inside,
        );
    } else if enabled && response.hovered() {
        painter.rect_filled(rect, 3.0, TILE_BG);
    }

    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - ROW_ICON / 2.0),
        egui::vec2(ROW_ICON, ROW_ICON),
    );
    paint_icon(painter, icon_rect, icon, !enabled);

    painter.text(
        egui::pos2(icon_rect.right() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        if !enabled {
            MUTED_TEXT
        } else if selected {
            egui::Color32::WHITE
        } else {
            BUTTON_TEXT
        },
    );

    if let Some(tag) = tag {
        let pill = egui::Rect::from_min_max(
            egui::pos2(rect.right() - 54.0, rect.center().y - 8.0),
            egui::pos2(rect.right() - 2.0, rect.center().y + 8.0),
        );
        painter.rect_filled(pill, 8.0, TILE_BG);
        painter.text(
            pill.center(),
            egui::Align2::CENTER_CENTER,
            tag,
            egui::FontId::proportional(10.0),
            MUTED_TEXT,
        );
    }

    response
}

/// Compact class picker: a grid of icons rather than a second full-height list.
///
/// The dummy list is a secondary axis, and as an 8-row list it cost 224px and
/// pushed everything below it a quarter of the screen whenever it appeared.
fn class_grid(
    ui: &mut egui::Ui,
    classes: &[(CharacterClass, Option<egui::TextureId>)],
    current: CharacterClass,
) -> Option<CharacterClass> {
    const CELL: f32 = 30.0;
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for (class, icon) in classes {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(CELL, CELL), egui::Sense::click());
            let painter = ui.painter();
            let selected = *class == current;
            if selected {
                painter.rect_filled(rect, 3.0, SELECTED_BG);
                painter.rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(1.0, TITLE_GOLD),
                    egui::StrokeKind::Inside,
                );
            } else {
                // A grid cell takes its affordance from the tile itself, not
                // from its contents, so it keeps a frame whether or not the
                // icon resolved. List rows are the opposite — there the frame
                // reads as a checkbox.
                painter.rect_filled(
                    rect,
                    3.0,
                    if response.hovered() {
                        egui::Color32::from_rgb(42, 42, 58)
                    } else {
                        TILE_BG
                    },
                );
                painter.rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(1.0, TILE_FRAME),
                    egui::StrokeKind::Inside,
                );
            }
            paint_icon(painter, rect.shrink(3.0), *icon, false);
            if response.on_hover_text(format!("{class:?}")).clicked() && !selected {
                picked = Some(*class);
            }
        }
    });
    picked
}

/// Progress track for the current pass.
///
/// Display-only, deliberately: these effects are spawn/update/cleanup entity
/// pipelines with no retained keyframe timeline, so there is nothing to seek
/// backwards through. Loop plus frame-step covers the inspection need a scrub
/// would have served.
fn progress_track(ui: &mut egui::Ui, view: &SandboxView) {
    let width = 160.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 8.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, TILE_BG);
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, TILE_FRAME),
        egui::StrokeKind::Inside,
    );

    if view.duration <= 0.0 {
        return;
    }

    let span = view.duration + view.loop_tail;
    // The tail is drawn as its own dimmer segment: during it the numeric
    // readout is pinned at the pass duration, which on its own reads as a hang.
    let tail_start = rect.left() + rect.width() * (view.duration / span);
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(tail_start, rect.top()), rect.right_bottom()),
        0.0,
        egui::Color32::from_rgb(38, 38, 52),
    );

    let progress = (view.elapsed / span).clamp(0.0, 1.0);
    let filled = egui::Rect::from_min_size(
        rect.left_top(),
        egui::vec2(rect.width() * progress, rect.height()),
    );
    painter.rect_filled(filled, 4.0, TITLE_GOLD.gamma_multiply(0.6));
    painter.line_segment(
        [
            egui::pos2(filled.right(), rect.top() - 2.0),
            egui::pos2(filled.right(), rect.bottom() + 2.0),
        ],
        egui::Stroke::new(2.0, TITLE_GOLD),
    );
}

/// Draws the sandbox screen. Pure: no Bevy ECS, so the snapshot test can call it.
pub fn draw_sandbox_ui(ctx: &egui::Context, view: &SandboxView) -> Vec<SandboxAction> {
    let mut actions = Vec::new();
    apply_theme(ctx);

    egui::SidePanel::left("sandbox_stage_panel")
        .exact_width(200.0)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            // U+2190 has no glyph in egui's default font stack and rendered as
            // a tofu box; U+25C0 mirrors the transport's play triangle.
            if ui
                .button(egui::RichText::new("\u{25c0}  BACK").size(15.0))
                .clicked()
            {
                actions.push(SandboxAction::Back);
            }
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("ANIMATIONS")
                    .color(TITLE_GOLD)
                    .size(20.0)
                    .strong(),
            );

            section(ui, "CASTER");
            for (class, icon) in &view.class_icons {
                let selected = view.caster_class == *class;
                if icon_row(ui, *icon, &format!("{class:?}"), None, selected, true).clicked()
                    && !selected
                {
                    actions.push(SandboxAction::SetCaster(*class));
                }
            }

            // CAMERA sits ABOVE the dummy section so its position never moves
            // when the dummy checkbox is toggled — the presets are reached for
            // constantly and must stay put.
            section(ui, "CAMERA");
            ui.horizontal_wrapped(|ui| {
                for preset in CameraPreset::ALL {
                    let active = view.applied_preset == Some(preset);
                    if ui.selectable_label(active, preset.label()).clicked() {
                        actions.push(SandboxAction::Preset(preset));
                    }
                }
            });
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Drag to orbit \u{00b7} scroll to zoom")
                    .color(MUTED_TEXT)
                    .size(11.0),
            );

            section(ui, "TARGET DUMMY");
            let mut staged = view.dummy_enabled;
            if ui.checkbox(&mut staged, "Staged").changed() {
                actions.push(SandboxAction::SetDummyEnabled(staged));
            }
            if view.dummy_enabled {
                ui.add_space(3.0);
                if let Some(class) = class_grid(ui, &view.class_icons, view.dummy_class) {
                    actions.push(SandboxAction::SetDummyClass(class));
                }
            } else {
                // Sits with the control that fixes it, not buried at the far
                // right of the transport bar.
                ui.label(
                    egui::RichText::new("Beams, projectiles and impacts\nwill not read without one.")
                        .color(WARN_AMBER)
                        .size(11.0),
                );
            }
        });

    egui::SidePanel::right("sandbox_entry_panel")
        .exact_width(250.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                section(ui, "ABILITIES");
                for row in view.rows.iter().filter(|r| r.family != EntryFamily::Body) {
                    let selected = view.selected == Some(row.entry);
                    let playable = row.family.is_playable();
                    let tag = (!playable).then_some(match row.family {
                        EntryFamily::Unsupported => "n/a",
                        _ => "soon",
                    });
                    let response = icon_row(ui, row.icon, &row.label, tag, selected, playable);
                    if !playable {
                        response.on_hover_text(match row.family {
                            EntryFamily::Unsupported => {
                                "Not previewable: defined as data but with no application code \
                                 (or no distinct cast visual)."
                            }
                            _ => "This ability's preview mechanism is not wired yet.",
                        });
                    } else if response.clicked() {
                        actions.push(SandboxAction::Select(row.entry, row.family));
                    }
                }

                section(ui, "BODY");
                for row in view.rows.iter().filter(|r| r.family == EntryFamily::Body) {
                    let selected = view.selected == Some(row.entry);
                    if icon_row(ui, row.icon, &row.label, None, selected, true).clicked() {
                        actions.push(SandboxAction::Select(row.entry, row.family));
                    }
                }

                // The question being asked in this screen is usually "does the
                // visual match the data", so the data belongs on it.
                if let Some(label) = &view.selected_label {
                    section(ui, "SELECTED");
                    ui.label(
                        egui::RichText::new(label)
                            .color(BUTTON_TEXT)
                            .size(14.0)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    for (key, value) in &view.selected_details {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(key).color(MUTED_TEXT).size(12.0),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(PANEL_INSET);
                                    ui.label(
                                        egui::RichText::new(value)
                                            .color(BUTTON_TEXT)
                                            .size(12.0),
                                    );
                                },
                            );
                        });
                    }
                }
            });
        });

    egui::TopBottomPanel::bottom("sandbox_transport")
        .exact_height(36.0)
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(PANEL_INSET);
                let has_selection = view.selected.is_some();

                // The one verb on the screen, styled so it is findable without
                // reading — the main menu's button treatment.
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new(
                            egui::RichText::new("\u{25b6}  Play").color(TITLE_GOLD),
                        )
                        .stroke(egui::Stroke::new(1.0, TITLE_GOLD))
                        .min_size(egui::vec2(72.0, 22.0)),
                    )
                    .clicked()
                {
                    actions.push(SandboxAction::Play);
                }

                // Fixed width: "Resume" is wider than "Pause", and letting the
                // bar reflow slid Step and the speed chips out from under the
                // cursor every time you paused.
                if ui
                    .add_enabled(
                        has_selection,
                        egui::Button::new(if view.paused { "Resume" } else { "Pause" })
                            .min_size(egui::vec2(72.0, 22.0)),
                    )
                    .clicked()
                {
                    actions.push(SandboxAction::TogglePause);
                }
                if ui
                    .add_enabled(
                        has_selection && view.paused,
                        egui::Button::new("Step").min_size(egui::vec2(52.0, 22.0)),
                    )
                    .on_hover_text("Advance one simulation tick")
                    .clicked()
                {
                    actions.push(SandboxAction::Step);
                }

                ui.separator();
                let mut looping = view.looping;
                if ui.checkbox(&mut looping, "Loop").changed() {
                    actions.push(SandboxAction::SetLooping(looping));
                }

                ui.separator();
                ui.label(
                    egui::RichText::new("SPEED").color(MUTED_TEXT).size(12.0),
                );
                for speed in SPEEDS {
                    let active = (view.speed - speed).abs() < f32::EPSILON;
                    if ui.selectable_label(active, format!("{speed}x")).clicked() {
                        actions.push(SandboxAction::SetSpeed(speed));
                    }
                }

                ui.separator();
                progress_track(ui, view);
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(if has_selection {
                        format!(
                            "{:.2}s / {:.2}s",
                            view.elapsed.min(view.duration),
                            view.duration
                        )
                    } else {
                        "\u{2014} / \u{2014}".to_string()
                    })
                    .color(if has_selection { BUTTON_TEXT } else { MUTED_TEXT })
                    .monospace(),
                );
            });
        });

    actions
}

/// Ability data worth checking an animation against.
fn ability_details(
    entry: SandboxEntry,
    defs: &AbilityDefinitions,
) -> (Option<String>, Vec<(String, String)>) {
    let SandboxEntry::Ability(ability) = entry else {
        return (None, Vec::new());
    };
    let Some(config) = defs.get(&ability) else {
        return (None, Vec::new());
    };
    let mut details = vec![
        ("Cast time".into(), format!("{:.2}s", config.cast_time)),
        ("Range".into(), format!("{:.0} yd", config.range)),
        ("Mana".into(), format!("{:.0}", config.mana_cost)),
        ("Cooldown".into(), format!("{:.0}s", config.cooldown)),
        ("School".into(), format!("{:?}", config.spell_school)),
    ];
    if let Some(speed) = config.projectile_speed {
        details.push(("Projectile".into(), format!("{speed:.0} yd/s")));
    }
    if let Some(aura) = &config.applies_aura {
        details.push(("Aura".into(), format!("{:?}", aura.aura_type)));
    }
    (Some(config.name.clone()), details)
}

/// Bevy wrapper: gathers the view, draws, applies the actions.
#[allow(clippy::too_many_arguments)]
pub fn sandbox_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<SandboxConfig>,
    mut playback: ResMut<SandboxPlayback>,
    mut pending_preset: ResMut<PendingCameraPreset>,
    mut virtual_time: ResMut<Time<Virtual>>,
    defs: Res<AbilityDefinitions>,
    spell_icons: Res<SpellIcons>,
    class_icons: Res<ClassIcons>,
    mut next_state: ResMut<NextState<super::super::GameState>>,
) {
    let rows: Vec<EntryRow> = entries_for_class(config.caster_class, &defs)
        .into_iter()
        .map(|listing| EntryRow {
            icon: spell_icons.textures.get(&listing.label).copied(),
            entry: listing.entry,
            family: listing.family,
            label: listing.label,
        })
        .collect();

    let (selected_label, selected_details) = playback
        .selected
        .map(|entry| ability_details(entry, &defs))
        .unwrap_or((None, Vec::new()));

    let view = SandboxView {
        caster_class: config.caster_class,
        class_icons: CharacterClass::all()
            .iter()
            .map(|c| (*c, class_icons.textures.get(c).copied()))
            .collect(),
        dummy_enabled: config.dummy_enabled,
        dummy_class: config.dummy_class,
        rows,
        selected: playback.selected,
        selected_label,
        selected_details,
        applied_preset: pending_preset.applied,
        looping: playback.looping,
        paused: virtual_time.relative_speed() == 0.0,
        speed: virtual_time.relative_speed(),
        elapsed: playback.elapsed,
        duration: playback.duration,
        loop_tail: super::playback::LOOP_TAIL_SECS,
    };

    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };
    let actions = draw_sandbox_ui(ctx, &view);

    for action in actions {
        match action {
            SandboxAction::Back => next_state.set(super::super::GameState::MainMenu),
            SandboxAction::SetCaster(class) => {
                config.caster_class = class;
                // The entry list is per-class, so the previous class's selection
                // has to go with it — otherwise the transport keeps `Play` live
                // and casts an ability the newly staged class does not own.
                playback.clear_selection();
            }
            SandboxAction::SetDummyEnabled(enabled) => {
                config.dummy_enabled = enabled;
                playback.stop();
            }
            SandboxAction::SetDummyClass(class) => {
                config.dummy_class = class;
                // Restaging despawns the dummy a running cast is aimed at,
                // which would silently fizzle mid-preview.
                playback.stop();
            }
            SandboxAction::Preset(preset) => pending_preset.requested = Some(preset),
            SandboxAction::Select(entry, family) => {
                playback.select(entry, family);
                playback.restart_requested = true;
            }
            SandboxAction::Play => {
                // Play from a paused clock has to lift the pause, or the entry
                // restarts into frozen time and looks like a dead button.
                if virtual_time.relative_speed() == 0.0 {
                    virtual_time.set_relative_speed(playback.resume_speed);
                }
                playback.restart_requested = true;
            }
            SandboxAction::SetLooping(looping) => playback.looping = looping,
            SandboxAction::TogglePause => {
                if virtual_time.relative_speed() == 0.0 {
                    virtual_time.set_relative_speed(playback.resume_speed);
                } else {
                    // Remember the rung so Resume and Play return to the speed
                    // that was being watched, not an assumed 1x.
                    playback.resume_speed = virtual_time.relative_speed();
                    virtual_time.set_relative_speed(0.0);
                }
            }
            SandboxAction::Step => playback.step_requested = true,
            SandboxAction::SetSpeed(speed) => {
                playback.resume_speed = speed;
                virtual_time.set_relative_speed(speed);
            }
        }
    }
}

/// Moves the sandbox camera to a requested preset.
///
/// Writes the ORBIT (yaw / pitch / zoom / look-at) rather than the transform.
/// `update_camera_position` rebuilds the transform from the controller every
/// frame, so a preset that set the transform directly would be overwritten
/// before it was ever seen. Going through the controller also means a preset is
/// a starting angle the user can immediately drag away from, rather than a mode
/// that fights their input.
pub fn apply_camera_preset(
    mut pending: ResMut<PendingCameraPreset>,
    config: Res<SandboxConfig>,
    mut controller: ResMut<CameraController>,
) {
    let Some(preset) = pending.requested.take() else {
        return;
    };

    let focus = super::stage_focus(&config);
    let offset = preset.offset();
    let horizontal = offset.xz().length();

    controller.mode = CameraMode::Manual;
    controller.manual_target = focus;
    controller.zoom_distance = offset.length();
    controller.yaw = offset.x.atan2(offset.z);
    controller.pitch = offset.y.atan2(horizontal);
    pending.applied = Some(preset);
}

/// Advances exactly one fixed tick while paused.
///
/// Runs in `First`, AFTER `TimeSystem` and therefore before
/// `RunFixedMainLoop`. That ordering is the whole system: Bevy's frame order is
/// `First -> ... -> RunFixedMainLoop -> Update`, `run_fixed_main_schedule` feeds
/// the fixed accumulator from `Time<Virtual>::delta()`, and `Time::advance_by`
/// OVERWRITES delta rather than adding to it. Registered in `Update` (where the
/// button is pressed) the injected delta was stamped after the fixed loop had
/// already read this frame's, then overwritten by `time_system` at the top of
/// the next one — so the sim never saw it and Step advanced nothing at all.
pub fn apply_step_request(
    mut playback: ResMut<SandboxPlayback>,
    mut virtual_time: ResMut<Time<Virtual>>,
    fixed_time: Res<Time<Fixed>>,
) {
    if !playback.step_requested {
        return;
    }
    playback.step_requested = false;
    // Exactly one timestep, read from the clock that will spend it, so the step
    // stays one tick if the fixed rate is ever retuned.
    virtual_time.advance_by(fixed_time.timestep());
}

/// Restores normal time on the way out, so a paused sandbox cannot leave the
/// next match frozen.
pub fn reset_time_on_exit(mut virtual_time: ResMut<Time<Virtual>>) {
    virtual_time.set_relative_speed(1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sandbox_offers_speeds_below_the_match_minimum() {
        // The match ladder bottoms out at 0.5x, which is the whole reason this
        // control exists.
        assert!(SPEEDS.iter().any(|s| *s < 0.5));
    }

    #[test]
    fn presets_all_look_at_the_stage_from_outside_it() {
        for preset in CameraPreset::ALL {
            assert!(preset.offset().length() > super::super::STAGE_SEPARATION);
        }
    }
}
