//! The sandbox panel: selection, transport, and camera framing.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use super::super::match_config::CharacterClass;
use super::super::play_match::ability_config::AbilityDefinitions;
use super::playback::{entries_for_class, EntryFamily, SandboxPlayback};
use super::{SandboxConfig, STAGE_SEPARATION};

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
    fn offset(self) -> Vec3 {
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

/// Draws the sandbox panel.
#[allow(clippy::too_many_arguments)]
pub fn sandbox_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<SandboxConfig>,
    mut playback: ResMut<SandboxPlayback>,
    mut pending_preset: ResMut<PendingCameraPreset>,
    mut virtual_time: ResMut<Time<Virtual>>,
    defs: Res<AbilityDefinitions>,
    mut next_state: ResMut<NextState<super::super::GameState>>,
) {
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    egui::SidePanel::left("sandbox_stage_panel")
        .default_width(190.0)
        .show(ctx, |ui| {
            if ui.button("< Back to menu").clicked() {
                next_state.set(super::super::GameState::MainMenu);
            }
            ui.separator();

            ui.label(egui::RichText::new("CASTER").strong());
            for &class in CharacterClass::all() {
                let selected = config.caster_class == class;
                if ui
                    .selectable_label(selected, format!("{class:?}"))
                    .clicked()
                    && !selected
                {
                    config.caster_class = class;
                    // The entry list is per-class, so the previous class's
                    // selection has to go with it — otherwise the transport
                    // keeps `Play` live and casts an ability the newly staged
                    // class does not own.
                    playback.clear_selection();
                }
            }

            ui.add_space(8.0);
            ui.label(egui::RichText::new("TARGET DUMMY").strong());
            let mut dummy_enabled = config.dummy_enabled;
            if ui.checkbox(&mut dummy_enabled, "Staged").changed() {
                config.dummy_enabled = dummy_enabled;
                playback.stop();
            }
            if config.dummy_enabled {
                for &class in CharacterClass::all() {
                    let selected = config.dummy_class == class;
                    if ui
                        .selectable_label(selected, format!("{class:?}"))
                        .clicked()
                        && !selected
                    {
                        config.dummy_class = class;
                        // Restaging despawns the dummy a running cast is aimed
                        // at, which would silently fizzle mid-preview.
                        playback.stop();
                    }
                }
            }

            ui.add_space(8.0);
            ui.label(egui::RichText::new("CAMERA").strong());
            for preset in CameraPreset::ALL {
                if ui.button(preset.label()).clicked() {
                    pending_preset.0 = Some(preset);
                }
            }
        });

    egui::SidePanel::right("sandbox_entry_panel")
        .default_width(230.0)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("ABILITIES").strong());
            egui::ScrollArea::vertical().show(ui, |ui| {
                let listings = entries_for_class(config.caster_class, &defs);

                for listing in listings.iter().filter(|l| l.family != EntryFamily::Body) {
                    let selected = playback.selected == Some(listing.entry);
                    let playable = listing.family.is_playable();

                    let response = ui.add_enabled(
                        playable,
                        egui::SelectableLabel::new(selected, &listing.label),
                    );
                    if !playable {
                        // Say why rather than silently greying it out — the gap
                        // is a known phase boundary, not a broken row.
                        response.on_hover_text(
                            "Instant abilities are not playable yet — they are applied \
                             inside class AI code and need the shared application seam \
                             (Phase B).",
                        );
                    } else if response.clicked() {
                        playback.select(listing.entry, listing.family);
                        playback.restart_requested = true;
                    }
                }

                ui.add_space(10.0);
                ui.label(egui::RichText::new("BODY").strong());
                for listing in listings.iter().filter(|l| l.family == EntryFamily::Body) {
                    let selected = playback.selected == Some(listing.entry);
                    if ui.selectable_label(selected, &listing.label).clicked() {
                        playback.select(listing.entry, listing.family);
                        playback.restart_requested = true;
                    }
                }
            });
        });

    egui::TopBottomPanel::bottom("sandbox_transport").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let has_selection = playback.selected.is_some();

            if ui
                .add_enabled(has_selection, egui::Button::new("Play"))
                .clicked()
            {
                playback.restart_requested = true;
            }
            ui.checkbox(&mut playback.looping, "Loop");

            let paused = virtual_time.relative_speed() == 0.0;
            if ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
                let speed = if paused { 1.0 } else { 0.0 };
                virtual_time.set_relative_speed(speed);
            }
            if ui
                .add_enabled(paused, egui::Button::new("Step"))
                .on_hover_text("Advance one simulation tick")
                .clicked()
            {
                playback.step_requested = true;
            }

            ui.separator();
            for speed in SPEEDS {
                let active = (virtual_time.relative_speed() - speed).abs() < f32::EPSILON;
                if ui
                    .selectable_label(active, format!("{speed}x"))
                    .clicked()
                {
                    virtual_time.set_relative_speed(speed);
                }
            }

            ui.separator();
            ui.label(format!(
                "{:.2}s / {:.2}s",
                playback.elapsed.min(playback.duration),
                playback.duration
            ));

            if !config.dummy_enabled {
                ui.separator();
                ui.label(
                    egui::RichText::new("No dummy staged — relational visuals will not read")
                        .weak(),
                );
            }
        });
    });
}

/// Moves the sandbox camera to a requested preset.
///
/// Sets the transform directly rather than driving a controller, so ordinary
/// camera input afterwards is not snapped back.
pub fn apply_camera_preset(
    mut pending: ResMut<PendingCameraPreset>,
    config: Res<SandboxConfig>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    let Some(preset) = pending.0.take() else {
        return;
    };

    // Frame the caster alone, or the midpoint of the pair when a dummy is
    // staged, so relational visuals sit in the middle of the shot.
    let focus = if config.dummy_enabled {
        Vec3::new(0.0, 1.2, 0.0)
    } else {
        Vec3::new(-STAGE_SEPARATION, 1.2, 0.0)
    };

    for mut transform in cameras.iter_mut() {
        *transform =
            Transform::from_translation(focus + preset.offset()).looking_at(focus, Vec3::Y);
    }
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
            assert!(preset.offset().length() > STAGE_SEPARATION);
        }
    }
}
