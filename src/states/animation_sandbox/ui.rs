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

/// Edge length of an icon in a list row.
const ROW_ICON: f32 = 22.0;

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
    pub(crate) fn offset(self) -> Vec3 {
        match self {
            CameraPreset::Front => Vec3::new(0.0, 4.0, 16.0),
            CameraPreset::ThreeQuarter => Vec3::new(11.0, 7.0, 13.0),
            CameraPreset::Side => Vec3::new(18.0, 3.0, 0.0),
            CameraPreset::Top => Vec3::new(0.0, 20.0, 0.1),
        }
    }
}

/// A preset the user asked for, consumed by [`apply_camera_preset`].
#[derive(Resource, Default)]
pub struct PendingCameraPreset(pub Option<CameraPreset>);

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
    pub looping: bool,
    pub paused: bool,
    pub speed: f32,
    pub elapsed: f32,
    pub duration: f32,
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

/// A list row: optional icon, label, selection highlight.
///
/// Rows carry their icon because a name alone is a poor handle for a spell —
/// the icons already exist in `SpellIcons` and `ClassIcons` and were simply not
/// being used here.
fn icon_row(
    ui: &mut egui::Ui,
    icon: Option<egui::TextureId>,
    label: &str,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let height = ROW_ICON + 6.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );

    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 3.0, SELECTED_BG);
    } else if enabled && response.hovered() {
        painter.rect_filled(rect, 3.0, TILE_BG);
    }

    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 4.0, rect.center().y - ROW_ICON / 2.0),
        egui::vec2(ROW_ICON, ROW_ICON),
    );
    match icon {
        Some(id) => {
            painter.image(
                id,
                icon_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                if enabled {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::from_gray(110)
                },
            );
        }
        None => {
            // The harness has no textures; a framed slot keeps the row's
            // rhythm so layout still reads truthfully in a snapshot.
            painter.rect_filled(icon_rect, 2.0, TILE_BG);
            painter.rect_stroke(
                icon_rect,
                2.0,
                egui::Stroke::new(1.0, TILE_FRAME),
                egui::StrokeKind::Inside,
            );
        }
    }

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

    response
}

/// Draws the sandbox screen. Pure: no Bevy ECS, so the snapshot test can call it.
pub fn draw_sandbox_ui(ctx: &egui::Context, view: &SandboxView) -> Vec<SandboxAction> {
    let mut actions = Vec::new();

    let mut style = (*ctx.style()).clone();
    style.visuals.panel_fill = BG_COLOR;
    style.visuals.window_fill = BG_COLOR;
    ctx.set_style(style);

    egui::SidePanel::left("sandbox_stage_panel")
        .exact_width(200.0)
        .show(ctx, |ui| {
            ui.add_space(8.0);
            if ui.button("\u{2190}  Back to menu").clicked() {
                actions.push(SandboxAction::Back);
            }

            section(ui, "CASTER");
            for (class, icon) in &view.class_icons {
                let selected = view.caster_class == *class;
                if icon_row(ui, *icon, &format!("{class:?}"), selected, true).clicked() && !selected
                {
                    actions.push(SandboxAction::SetCaster(*class));
                }
            }

            section(ui, "TARGET DUMMY");
            let mut staged = view.dummy_enabled;
            if ui.checkbox(&mut staged, "Staged").changed() {
                actions.push(SandboxAction::SetDummyEnabled(staged));
            }
            if view.dummy_enabled {
                for (class, icon) in &view.class_icons {
                    let selected = view.dummy_class == *class;
                    if icon_row(ui, *icon, &format!("{class:?}"), selected, true).clicked()
                        && !selected
                    {
                        actions.push(SandboxAction::SetDummyClass(*class));
                    }
                }
            }

            section(ui, "CAMERA");
            ui.horizontal_wrapped(|ui| {
                for preset in CameraPreset::ALL {
                    if ui.button(preset.label()).clicked() {
                        actions.push(SandboxAction::Preset(preset));
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Drag to orbit \u{00b7} scroll to zoom")
                    .color(MUTED_TEXT)
                    .size(11.0),
            );
        });

    egui::SidePanel::right("sandbox_entry_panel")
        .exact_width(250.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                section(ui, "ABILITIES");
                for row in view.rows.iter().filter(|r| r.family != EntryFamily::Body) {
                    let selected = view.selected == Some(row.entry);
                    let playable = row.family.is_playable();
                    let response = icon_row(ui, row.icon, &row.label, selected, playable);
                    if !playable {
                        // Say why rather than silently greying it out — the gap
                        // is a known phase boundary, not a broken row.
                        response.on_hover_text(
                            "Instant abilities are not playable yet — they are applied \
                             inside class AI code and need the shared application seam \
                             (Phase B).",
                        );
                    } else if response.clicked() {
                        actions.push(SandboxAction::Select(row.entry, row.family));
                    }
                }

                section(ui, "BODY");
                for row in view.rows.iter().filter(|r| r.family == EntryFamily::Body) {
                    let selected = view.selected == Some(row.entry);
                    if icon_row(ui, row.icon, &row.label, selected, true).clicked() {
                        actions.push(SandboxAction::Select(row.entry, row.family));
                    }
                }
            });
        });

    egui::TopBottomPanel::bottom("sandbox_transport")
        .exact_height(48.0)
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let has_selection = view.selected.is_some();

                if ui
                    .add_enabled(has_selection, egui::Button::new("\u{25b6}  Play"))
                    .clicked()
                {
                    actions.push(SandboxAction::Play);
                }

                let mut looping = view.looping;
                if ui.checkbox(&mut looping, "Loop").changed() {
                    actions.push(SandboxAction::SetLooping(looping));
                }

                if ui
                    .button(if view.paused {
                        "\u{25b6}\u{25b6} Resume"
                    } else {
                        "\u{23f8}  Pause"
                    })
                    .clicked()
                {
                    actions.push(SandboxAction::TogglePause);
                }
                if ui
                    .add_enabled(view.paused, egui::Button::new("\u{23ed}  Step"))
                    .on_hover_text("Advance one simulation tick")
                    .clicked()
                {
                    actions.push(SandboxAction::Step);
                }

                ui.separator();
                for speed in SPEEDS {
                    let active = (view.speed - speed).abs() < f32::EPSILON;
                    if ui.selectable_label(active, format!("{speed}x")).clicked() {
                        actions.push(SandboxAction::SetSpeed(speed));
                    }
                }

                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{:.2}s / {:.2}s",
                        view.elapsed.min(view.duration),
                        view.duration
                    ))
                    .color(BUTTON_TEXT)
                    .monospace(),
                );

                if !view.dummy_enabled {
                    ui.separator();
                    ui.label(
                        egui::RichText::new("No dummy staged \u{2014} relational visuals will not read")
                            .color(MUTED_TEXT)
                            .size(11.0),
                    );
                }
            });
        });

    actions
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
        looping: playback.looping,
        paused: virtual_time.relative_speed() == 0.0,
        speed: virtual_time.relative_speed(),
        elapsed: playback.elapsed,
        duration: playback.duration,
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
            SandboxAction::Preset(preset) => pending_preset.0 = Some(preset),
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
    let Some(preset) = pending.0.take() else {
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
