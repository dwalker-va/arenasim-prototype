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
use crate::states::chrome::ScreenNav;
use crate::ui::theme::{class_color, color, space, text};
use crate::ui::widgets::panel::margin_xy;
use crate::ui::widgets::{
    checkbox, list_row, panel, panel_header, resource_bar, screen_header, section_label, stat_rows,
    Button, Chip, ListIcon, ListRow, Nav, NavAction, Variant,
};

/// The screen title.
const TITLE: &str = "Animations";
/// The Workbench's side panels. The 3D stage shows between them, so they are
/// as narrow as their contents allow: the left one fits a class name in a
/// list row and the camera chips on one line, the right one an ability name
/// beside its `needs dummy` note.
const STAGE_PANEL_WIDTH: f32 = 220.0;
const ENTRY_PANEL_WIDTH: f32 = 260.0;
/// The Pause/Resume button's width. Fixed, because "Resume" is wider than
/// "Pause", and letting the bar reflow slid Step and the speed chips out
/// from under the cursor every time you paused.
const PAUSE_WIDTH: f32 = 76.0;
/// The progress track's length: the pass and its loop tail together.
const TRACK_WIDTH: f32 = 160.0;

/// Playback speeds offered in the sandbox.
///
/// A match offers 0.5x as its slowest rung, which is not slow enough to read
/// the phases of a fast effect. 0.1 and 0.25 are the reason this control exists
/// at all; 1x is kept so an entry can be judged at real speed.
///
/// `pub` because `SandboxAction::SetSpeed` only ever carries a member of this
/// array, so a `SandboxView::speed` outside it highlights NO chip — a state the
/// running panel cannot reach. The snapshot fixture picks its rung from here
/// rather than naming one, so it cannot pin that state by accident.
pub const SPEEDS: [f32; 4] = [0.1, 0.25, 0.5, 1.0];

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
    /// Relational entry that needs a staged dummy — disabled dummy-off (AE3).
    pub needs_dummy: bool,
}

/// Everything the panel draws from. No Bevy types, so the snapshot harness can
/// build one by hand.
pub struct SandboxView {
    /// The screen's exit: back to whatever opened it.
    pub nav: Nav,
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
    /// The exit: leave for whatever opened the sandbox.
    Exit,
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

/// A side panel's or the transport's frame: the screen background, so the
/// panels read as one surface with the header, and the 3D stage as the thing
/// framed between them.
fn workbench_frame(x: f32, y: f32) -> egui::Frame {
    egui::Frame::new()
        .fill(color::BG)
        .inner_margin(margin_xy(x, y))
}

/// A section caption, spaced off whatever came before it.
fn section(ui: &mut egui::Ui, label: &str) {
    ui.add_space(space::LG);
    section_label(ui, label);
}

/// Progress through the current pass, and the loop tail after it.
///
/// Two bars rather than one: the tail is its own dimmer segment, because
/// during it the numeric readout is pinned at the pass duration, which on its
/// own reads as a hang.
///
/// Display-only, deliberately: these effects are spawn/update/cleanup entity
/// pipelines with no retained keyframe timeline, so there is nothing to seek
/// backwards through. Loop plus frame-step covers the inspection need a scrub
/// would have served.
fn progress_track(ui: &mut egui::Ui, view: &SandboxView) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XXS;
        let span = view.duration + view.loop_tail;
        if view.duration <= 0.0 || span <= 0.0 {
            resource_bar(ui, 0.0, color::ACCENT, TRACK_WIDTH);
            return;
        }
        let usable = TRACK_WIDTH - space::XXS;
        let pass_width = usable * view.duration / span;
        resource_bar(ui, view.elapsed / view.duration, color::ACCENT, pass_width);
        if view.loop_tail > 0.0 {
            resource_bar(
                ui,
                (view.elapsed - view.duration) / view.loop_tail,
                color::TEXT_DIM,
                usable - pass_width,
            );
        }
    });
}

/// Draws the sandbox screen on the Workbench template: the header, the stage
/// controls on the left, the entry list on the right, and the transport
/// along the foot of the 3D stage between them. Pure: no Bevy ECS, so the
/// snapshot test can call it.
pub fn draw_sandbox_ui(ctx: &egui::Context, view: &SandboxView) -> Vec<SandboxAction> {
    let mut actions = Vec::new();

    // The screen margin, every screen's: `XL` across, `LG` down. The header's
    // rule is the panel's bottom edge; the side panels open below it.
    let mut header_margin = margin_xy(space::XL, space::LG);
    header_margin.bottom = 0;
    egui::TopBottomPanel::top("sandbox_header")
        .frame(
            egui::Frame::new()
                .fill(color::BG)
                .inner_margin(header_margin),
        )
        .show_separator_line(false)
        .show(ctx, |ui| {
            if let Some(NavAction::Exit(_)) = screen_header(ui, TITLE, view.nav, |_| {}) {
                actions.push(SandboxAction::Exit);
            }
        });

    egui::SidePanel::left("sandbox_stage_panel")
        .exact_width(STAGE_PANEL_WIDTH)
        .resizable(false)
        .frame(workbench_frame(space::LG, 0.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("sandbox_stage_scroll")
                .show(ui, |ui| stage_controls(ui, view, &mut actions));
        });

    egui::SidePanel::right("sandbox_entry_panel")
        .exact_width(ENTRY_PANEL_WIDTH)
        .resizable(false)
        .frame(workbench_frame(space::LG, 0.0))
        .show(ctx, |ui| {
            if let Some(label) = &view.selected_label {
                egui::TopBottomPanel::bottom("sandbox_selected")
                    .frame(egui::Frame::new())
                    .show_separator_line(false)
                    .resizable(false)
                    .show_inside(ui, |ui| selected_details(ui, label, &view.selected_details));
            }
            egui::ScrollArea::vertical()
                .id_salt("sandbox_entry_scroll")
                .show(ui, |ui| entry_list(ui, view, &mut actions));
        });

    egui::TopBottomPanel::bottom("sandbox_transport")
        .resizable(false)
        .frame(workbench_frame(space::LG, space::SM))
        .show(ctx, |ui| transport(ui, view, &mut actions));

    actions
}

/// The left panel: who is staged, and how the camera sees them.
fn stage_controls(ui: &mut egui::Ui, view: &SandboxView, actions: &mut Vec<SandboxAction>) {
    section(ui, "Caster");
    for (class, icon) in &view.class_icons {
        let selected = view.caster_class == *class;
        let row = ListRow::new(class.name())
            .name_color(class_color(*class))
            .icon(ListIcon {
                texture: *icon,
                edge: class_color(*class),
            })
            .selected(selected);
        if list_row(ui, row).clicked() && !selected {
            actions.push(SandboxAction::SetCaster(*class));
        }
    }

    // CAMERA sits ABOVE the dummy section so its position never moves when
    // the dummy checkbox is toggled — the presets are reached for constantly
    // and must stay put.
    section(ui, "Camera");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(space::XS, space::XS);
        for preset in CameraPreset::ALL {
            let active = view.applied_preset == Some(preset);
            if Chip::new(preset.label()).on(active).show(ui).clicked() {
                actions.push(SandboxAction::Preset(preset));
            }
        }
    });
    ui.add_space(space::XS);
    ui.label(
        egui::RichText::new("Drag to orbit \u{00b7} scroll to zoom")
            .size(text::SMALL)
            .color(color::TEXT_DIM),
    );

    section(ui, "Target dummy");
    let mut staged = view.dummy_enabled;
    if checkbox(ui, &mut staged, "Staged").changed() {
        actions.push(SandboxAction::SetDummyEnabled(staged));
    }
    ui.add_space(space::XS);
    if view.dummy_enabled {
        // A secondary axis, so chips rather than a second full-height list:
        // as nine list rows it would push everything below it a third of the
        // screen whenever the dummy was staged.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::XS, space::XS);
            for (class, _) in &view.class_icons {
                let selected = *class == view.dummy_class;
                let chip = Chip::new(class.name())
                    .label_color(class_color(*class))
                    .on(selected);
                if chip.show(ui).clicked() && !selected {
                    actions.push(SandboxAction::SetDummyClass(*class));
                }
            }
        });
    } else {
        // Sits with the control that fixes it, not buried at the far right of
        // the transport bar.
        ui.label(
            egui::RichText::new("Beams, projectiles and impacts will not read without one.")
                .size(text::SMALL)
                .color(color::WARN),
        );
    }
    ui.add_space(space::LG);
}

/// The right panel: the staged class's entries, and the data of the one
/// selected.
fn entry_list(ui: &mut egui::Ui, view: &SandboxView, actions: &mut Vec<SandboxAction>) {
    section(ui, "Abilities");
    for row in view.rows.iter().filter(|r| r.family != EntryFamily::Body) {
        let selected = view.selected == Some(row.entry);
        let mechanism_playable = row.family.is_playable();
        // AE3: a relational entry can't play a complete visual without a
        // second unit — disable it while the dummy is off.
        let dummy_off = row.needs_dummy && !view.dummy_enabled;
        let enabled = mechanism_playable && !dummy_off;
        // The `soon` arm is unreachable today and deliberately kept:
        // `EntryFamily::is_playable` is true for every family except
        // `Unsupported`, so nothing currently lands here. It is held for the
        // next unwired mechanism — a family added to `EntryFamily` but left
        // out of `is_playable` while its start path is built, which is how
        // every mechanism in this panel arrived. Deleting it would label that
        // family `n/a` under a hover claiming it has no application code,
        // which would be false. Keep the two in step: drop this arm only
        // together with the hover below.
        let tag = if !mechanism_playable {
            Some(match row.family {
                EntryFamily::Unsupported => "n/a",
                _ => "soon",
            })
        } else if dummy_off {
            Some("needs dummy")
        } else {
            None
        };
        // An ability always has a spell icon, so its slot is always drawn:
        // the art when it has loaded, the kit's fallback tile until then.
        let mut list_entry = ListRow::new(&row.label)
            .icon(ListIcon {
                texture: row.icon,
                edge: color::LINE,
            })
            .selected(selected)
            .enabled(enabled);
        if let Some(tag) = tag {
            list_entry = list_entry.meta(tag);
        }
        let response = list_row(ui, list_entry);
        if !mechanism_playable {
            response.on_hover_text(match row.family {
                EntryFamily::Unsupported => {
                    "Not previewable: defined as data but with no application code \
                     (or no distinct cast visual)."
                }
                // Pairs with the `soon` tag above: unreachable while
                // `is_playable` excludes only `Unsupported`.
                _ => "This ability's preview mechanism is not wired yet.",
            });
        } else if dummy_off {
            response
                .on_hover_text("This ability needs a target — turn on Target Dummy to preview it.");
        } else if response.clicked() {
            actions.push(SandboxAction::Select(row.entry, row.family));
        }
    }

    // The body animations have no icon by nature, not by failure, so their
    // rows draw none rather than a fallback tile standing in for nothing.
    section(ui, "Body");
    for row in view.rows.iter().filter(|r| r.family == EntryFamily::Body) {
        let selected = view.selected == Some(row.entry);
        if list_row(ui, ListRow::new(&row.label).selected(selected)).clicked() {
            actions.push(SandboxAction::Select(row.entry, row.family));
        }
    }

    ui.add_space(space::LG);
}

/// The selected entry's data, pinned to the foot of the entry panel so it
/// stays in view however far the list above it scrolls. The question being
/// asked in this screen is usually "does the visual match the data", so the
/// data belongs on it.
fn selected_details(ui: &mut egui::Ui, label: &str, details: &[(String, String)]) {
    section(ui, "Selected");
    panel(None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        panel_header(ui, label, color::ACCENT, |_| {});
        stat_rows(
            ui,
            details
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
    });
    ui.add_space(space::LG);
}

/// The transport along the foot of the stage: play, pause, step, loop, speed,
/// and where the pass is.
fn transport(ui: &mut egui::Ui, view: &SandboxView, actions: &mut Vec<SandboxAction>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        let has_selection = view.selected.is_some();

        // The one verb on the screen: the Primary, at the transport's inline
        // height.
        if Button::new("\u{25b6}  Play", Variant::Primary)
            .compact()
            .enabled(has_selection)
            .show(ui)
            .clicked()
        {
            actions.push(SandboxAction::Play);
        }
        if Button::new(
            if view.paused { "Resume" } else { "Pause" },
            Variant::Secondary,
        )
        .compact()
        .min_width(PAUSE_WIDTH)
        .enabled(has_selection)
        .show(ui)
        .clicked()
        {
            actions.push(SandboxAction::TogglePause);
        }
        if Button::new("Step", Variant::Secondary)
            .compact()
            .enabled(has_selection && view.paused)
            .show(ui)
            .on_hover_text("Advance one simulation tick")
            .clicked()
        {
            actions.push(SandboxAction::Step);
        }

        ui.separator();
        let mut looping = view.looping;
        if checkbox(ui, &mut looping, "Loop").changed() {
            actions.push(SandboxAction::SetLooping(looping));
        }

        ui.separator();
        ui.label(
            egui::RichText::new("SPEED")
                .size(text::SMALL)
                .color(color::TEXT_MUTED),
        );
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            for speed in SPEEDS {
                let active = (view.speed - speed).abs() < f32::EPSILON;
                if Chip::new(&format!("{speed}x"))
                    .on(active)
                    .show(ui)
                    .clicked()
                {
                    actions.push(SandboxAction::SetSpeed(speed));
                }
            }
        });

        ui.separator();
        progress_track(ui, view);
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
            .color(if has_selection {
                color::TEXT
            } else {
                color::TEXT_DIM
            })
            .monospace(),
        );
    });
}

/// Ability data worth checking an animation against.
///
/// `pub` so the snapshot fixture (`tests/animation_sandbox_snapshot.rs`) can
/// DERIVE its SELECTED readout from the same function the panel renders,
/// instead of restating the numbers by hand — which is how the blessed
/// baseline came to pin a Frostbolt with a 30yd range and a 24 mana cost that
/// the shipped panel never produced (AS-50).
pub fn ability_details(
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

/// The panel's entry list for one caster class, flattened for drawing.
///
/// This is the whole row-building step: [`entries_for_class`] for the entries,
/// their family and their label; [`entry_needs_dummy`] for the AE3 disable; and
/// `icon_for` for the texture. The Bevy wrapper below has nothing left to add,
/// which is the point — it means the snapshot fixture can build a REAL panel's
/// rows by calling this same function rather than hand-assembling a list beside
/// it. The fixture passing `|_| None` is the one honest difference: kittest has
/// no Bevy textures, so it is the only input the harness cannot supply.
///
/// Kept here rather than in `playback.rs` because `EntryRow` is a drawing type
/// (it carries an `egui::TextureId`); `entry_needs_dummy` stays `pub(crate)`
/// so its per-ability behaviour keeps being pinned by the in-module unit test
/// that runs in the default `cargo test`, instead of being restated by a
/// fixture behind `#[ignore]`.
pub fn entry_rows(
    class: CharacterClass,
    defs: &AbilityDefinitions,
    icon_for: impl Fn(&str) -> Option<egui::TextureId>,
) -> Vec<EntryRow> {
    entries_for_class(class, defs)
        .into_iter()
        .map(|listing| EntryRow {
            icon: icon_for(&listing.label),
            needs_dummy: super::playback::entry_needs_dummy(listing.entry, defs),
            entry: listing.entry,
            family: listing.family,
            label: listing.label,
        })
        .collect()
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
    mut nav: ScreenNav,
) {
    // Esc takes the exit. It needs no egui context, so it answers on any
    // frame.
    if let Some(NavAction::Exit(to)) = nav.escape(false, || false) {
        nav.leave(to);
        return;
    }

    let rows = entry_rows(config.caster_class, &defs, |label| {
        spell_icons.textures.get(label).copied()
    });

    let (selected_label, selected_details) = playback
        .selected
        .map(|entry| ability_details(entry, &defs))
        .unwrap_or((None, Vec::new()));

    let view = SandboxView {
        nav: nav.nav(false).expect("the sandbox has an exit"),
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
            SandboxAction::Exit => nav.leave(view.nav.destination()),
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
