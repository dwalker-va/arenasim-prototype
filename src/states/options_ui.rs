//! The Options screen: window, display and interface settings, and the way
//! into Keybindings.
//!
//! The Document template: the shared [`screen_header`], then a scrolling body
//! of panels, one setting per row — its name and a hint on the left, its
//! choices on the right as chips. The whole screen fits a Windowed 1280x720
//! client (the default) without scrolling, so every control, Keybindings
//! included, is on screen at the size a new player starts at;
//! `tests/options_snapshot.rs` pins that at exactly that size.
//!
//! [`draw_options`] is pure egui, driven by the harness; [`options_ui`] is the
//! Bevy wrapper that applies the returned [`OptionsAction`]. The draw only
//! READS the settings: a change comes back as an action, so the wrapper writes
//! `GameSettings` — and its save-on-change system writes `settings.ron` — on
//! the frame something changed, never on a frame that merely drew it.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use super::chrome::ScreenNav;
use super::GameState;
use crate::settings::{GameSettings, PendingSettingsRestart, ResolutionOption, WindowModeOption};
use crate::ui::driver as ui_driver;
use crate::ui::theme::{color, space, text};
use crate::ui::widgets::panel::margin_xy;
use crate::ui::widgets::{
    panel, panel_header, screen_header, Button, Chip, Nav, NavAction, Variant,
};

/// The screen's title, in the shared header.
const TITLE: &str = "Options";

/// The widest the settings column grows: a row's name and hint on the left
/// and its widest set of chips (three resolutions) on the right.
const CONTENT_MAX_W: f32 = 640.0;

/// What the screen asks its wrapper to do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsAction {
    /// The header's exit.
    Exit,
    /// Open the Keybindings screen.
    OpenKeybindings,
    SetWindowMode(WindowModeOption),
    SetResolution(ResolutionOption),
    SetVsync(bool),
    SetAuraIcons(bool),
}

/// Everything [`draw_options`] reads.
pub struct OptionsData<'a> {
    pub settings: &'a GameSettings,
    /// A window mode or resolution change is waiting on a restart.
    pub restart_required: bool,
}

/// The Options screen's Bevy wrapper: Esc and the header exit leave, and every
/// other action is applied to `GameSettings`.
pub fn options_ui(
    mut contexts: EguiContexts,
    mut nav: ScreenNav,
    mut settings: ResMut<GameSettings>,
    pending_restart: Res<PendingSettingsRestart>,
) {
    // Esc takes the exit. It needs no egui context, so it answers on any
    // frame.
    if let Some(NavAction::Exit(to)) = nav.escape(false, || false) {
        nav.leave(to);
        return;
    }
    let screen_nav = nav.nav(false).expect("Options has an exit");

    // Use try_ctx_mut to gracefully handle window close (the context dies
    // with the primary window; ctx_mut panics on the final frame).
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    let data = OptionsData {
        settings: &settings,
        restart_required: pending_restart.restart_required,
    };
    let Some(action) = draw_options(ctx, &data, screen_nav) else {
        return;
    };
    match action {
        OptionsAction::Exit => nav.leave(screen_nav.destination()),
        OptionsAction::OpenKeybindings => nav.open(GameState::Keybindings),
        OptionsAction::SetWindowMode(mode) => settings.window_mode = mode,
        OptionsAction::SetResolution(resolution) => settings.resolution = resolution,
        OptionsAction::SetVsync(on) => {
            settings.vsync = on;
            info!("VSync toggled to: {on}");
        }
        OptionsAction::SetAuraIcons(on) => {
            settings.show_aura_icons = on;
            info!("Show Aura Icons toggled to: {on}");
        }
    }
}

/// Render the Options screen into `ctx`, returning what the user asked for.
///
/// Deliberately free of Bevy ECS system params, so `tests/options_snapshot.rs`
/// can drive it directly.
pub fn draw_options(ctx: &egui::Context, data: &OptionsData, nav: Nav) -> Option<OptionsAction> {
    let mut action = None;
    let settings = data.settings;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(color::BG)
                .inner_margin(margin_xy(space::XL, space::LG)),
        )
        .show(ctx, |ui| {
            if screen_header(ui, TITLE, nav, |_| {}).is_some() {
                action = Some(OptionsAction::Exit);
            }
            ui.add_space(space::LG);

            // What the screen shows, for a script to read back.
            ui_driver::note(
                ui,
                format_args!(
                    "options window={:?} resolution={:?} vsync={} aura_icons={} restart_required={}",
                    settings.window_mode,
                    settings.resolution,
                    settings.vsync,
                    settings.show_aura_icons,
                    data.restart_required,
                ),
            );

            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(CONTENT_MAX_W);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            if let Some(a) = display_panel(ui, data) {
                                action = Some(a);
                            }
                            ui.add_space(space::MD);
                            if let Some(a) = interface_panel(ui, settings) {
                                action = Some(a);
                            }
                            ui.add_space(space::MD);
                            if let Some(a) = controls_panel(ui) {
                                action = Some(a);
                            }
                        });
                    });
                });
        });

    action
}

/// Window mode, resolution and VSync, and the restart notice the first two
/// raise.
fn display_panel(ui: &mut egui::Ui, data: &OptionsData) -> Option<OptionsAction> {
    let settings = data.settings;
    let mut action = None;
    panel(None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        panel_header(ui, "Display", color::ACCENT, |_| {});

        setting_row(ui, "Window mode", "Applies after a restart", |ui| {
            for mode in WindowModeOption::all().into_iter().rev() {
                if Chip::new(mode.as_str())
                    .on(settings.window_mode == mode)
                    .driver_id(window_mode_id(mode))
                    .show(ui)
                    .clicked()
                    && settings.window_mode != mode
                {
                    action = Some(OptionsAction::SetWindowMode(mode));
                }
            }
        });
        row_gap(ui);
        setting_row(
            ui,
            "Resolution",
            "Windowed mode only; applies after a restart",
            |ui| {
                for resolution in ResolutionOption::all().into_iter().rev() {
                    if Chip::new(resolution.as_str())
                        .on(settings.resolution == resolution)
                        .driver_id(resolution_id(resolution))
                        .show(ui)
                        .clicked()
                        && settings.resolution != resolution
                    {
                        action = Some(OptionsAction::SetResolution(resolution));
                    }
                }
            },
        );
        row_gap(ui);
        setting_row(
            ui,
            "VSync",
            "Prevents screen tearing; may cost frame rate",
            |ui| {
                if let Some(on) = on_off(ui, settings.vsync, ["options:vsync:on", "options:vsync:off"])
                {
                    action = Some(OptionsAction::SetVsync(on));
                }
            },
        );

        if data.restart_required {
            row_gap(ui);
            ui.label(
                egui::RichText::new(
                    "Restart required: the new window mode and resolution apply when you restart the game.",
                )
                .size(text::SMALL)
                .color(color::WARN),
            );
        }
    });
    action
}

/// Settings for what the match shows.
fn interface_panel(ui: &mut egui::Ui, settings: &GameSettings) -> Option<OptionsAction> {
    let mut action = None;
    panel(None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        panel_header(ui, "Interface", color::ACCENT, |_| {});
        setting_row(
            ui,
            "Aura icons",
            "Buff and debuff icons under health bars; V toggles them in a match",
            |ui| {
                if let Some(on) = on_off(
                    ui,
                    settings.show_aura_icons,
                    ["options:aura-icons:on", "options:aura-icons:off"],
                ) {
                    action = Some(OptionsAction::SetAuraIcons(on));
                }
            },
        );
    });
    action
}

/// The way into Keybindings.
fn controls_panel(ui: &mut egui::Ui) -> Option<OptionsAction> {
    let mut action = None;
    panel(None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        panel_header(ui, "Controls", color::ACCENT, |_| {});
        setting_row(ui, "Keybindings", "Customize keyboard controls", |ui| {
            if Button::new("Configure keybindings", Variant::Secondary)
                .driver_id("options:keybindings")
                .show(ui)
                .clicked()
            {
                action = Some(OptionsAction::OpenKeybindings);
            }
        });
    });
    action
}

/// One setting: its name and a hint on the left, `controls` right-aligned on
/// the same row. `controls` lays out right to left, so it adds its widgets
/// last-first.
fn setting_row(ui: &mut egui::Ui, label: &str, hint: &str, controls: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::XXS;
            ui.label(egui::RichText::new(label).size(text::H3).color(color::TEXT));
            ui.label(
                egui::RichText::new(hint)
                    .size(text::SMALL)
                    .color(color::TEXT_MUTED),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            controls(ui);
        });
    });
}

/// The gap between two rows of one panel.
fn row_gap(ui: &mut egui::Ui) {
    ui.add_space(space::SM);
}

/// An On / Off pair of chips for a boolean setting, drawn in a right-to-left
/// row. Returns the new value when the user picked the one that is not on.
fn on_off(ui: &mut egui::Ui, value: bool, ids: [&str; 2]) -> Option<bool> {
    let [on_id, off_id] = ids;
    let mut picked = None;
    // Right to left: Off first, so the row reads On, Off.
    if Chip::new("Off")
        .on(!value)
        .driver_id(off_id)
        .show(ui)
        .clicked()
        && value
    {
        picked = Some(false);
    }
    if Chip::new("On")
        .on(value)
        .driver_id(on_id)
        .show(ui)
        .clicked()
        && !value
    {
        picked = Some(true);
    }
    picked
}

/// A window-mode chip's `--ui-script` id.
fn window_mode_id(mode: WindowModeOption) -> &'static str {
    match mode {
        WindowModeOption::Windowed => "options:window:Windowed",
        WindowModeOption::BorderlessFullscreen => "options:window:BorderlessFullscreen",
    }
}

/// A resolution chip's `--ui-script` id.
fn resolution_id(resolution: ResolutionOption) -> &'static str {
    match resolution {
        ResolutionOption::HD720 => "options:resolution:HD720",
        ResolutionOption::HD1080 => "options:resolution:HD1080",
        ResolutionOption::QHD1440 => "options:resolution:QHD1440",
    }
}
