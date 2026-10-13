//! The Keybindings screen: every action's primary and secondary key, and a
//! confirmed reset to the defaults.
//!
//! The Document template: the shared [`screen_header`], whose trailing slot
//! holds Reset to Defaults (a `Danger` button, so it sits on screen at any
//! window size), then a scrolling body of one panel per action category.
//! Clicking a key arms a rebind; the next key pressed takes that slot, except
//! the Back key (Esc by default), which cancels the rebind instead. Reset
//! opens a confirm [`Modal`] and resets nothing until it is confirmed.
//!
//! [`draw_keybindings`] is pure egui, driven by `tests/keybindings_snapshot.rs`;
//! [`keybindings_ui`] is the Bevy wrapper. It owns the two things the draw
//! cannot see — the keyboard, for the rebind, and `GameSettings`, which it
//! writes only on the frame a binding actually changes.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use super::chrome::ScreenNav;
use crate::keybindings::{GameAction, Keybindings};
use crate::settings::GameSettings;
use crate::ui::driver as ui_driver;
use crate::ui::theme::{color, size, space, text};
use crate::ui::widgets::panel::margin_xy;
use crate::ui::widgets::{
    panel, panel_header, screen_header, Button, Look, Modal, Nav, NavAction, Variant,
};

/// The screen's title, in the shared header.
const TITLE: &str = "Keybindings";

/// The widest the binding list grows.
const CONTENT_MAX_W: f32 = 720.0;

/// Every key button's width, so the two key columns line up down the screen.
const KEY_W: f32 = 140.0;

/// The action categories, in the order the screen shows them.
const CATEGORIES: [&str; 4] = ["Navigation", "Camera", "Simulation", "Display"];

/// The reset confirm's egui and `--ui-script` id: its buttons register as
/// `reset-keybindings:confirm`, `reset-keybindings:cancel` and
/// `reset-keybindings:close`.
pub const RESET_MODAL_ID: &str = "reset-keybindings";

/// Which of an action's two keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySlot {
    Primary,
    Secondary,
}

/// A rebind in progress: the next key pressed goes here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rebind {
    pub action: GameAction,
    pub slot: KeySlot,
}

/// The screen's own state, kept by the wrapper between frames.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KeybindingsUiState {
    /// The key slot waiting for a key press, if any.
    pub rebinding: Option<Rebind>,
    /// The Reset to Defaults confirm is open.
    pub confirm_reset: bool,
}

impl KeybindingsUiState {
    /// The screen's own Esc rungs, first to last: cancel a pending rebind,
    /// then close the reset confirm. True when one took the key; once both
    /// are spent, Esc leaves the screen.
    pub fn escape_rung(&mut self) -> bool {
        self.rebinding.take().is_some() || std::mem::take(&mut self.confirm_reset)
    }
}

/// What the screen asks its wrapper to do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingsAction {
    /// The header's exit.
    Exit,
    /// The reset was confirmed: every binding back to its default.
    ResetToDefaults,
}

/// The Keybindings screen's Bevy wrapper.
pub fn keybindings_ui(
    mut contexts: EguiContexts,
    mut nav: ScreenNav,
    mut settings: ResMut<GameSettings>,
    mut state: Local<KeybindingsUiState>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // Esc cancels a pending rebind, then closes the reset confirm, then takes
    // the header's exit. It runs before the rebind reads the keyboard, so the
    // Back key is never captured as a binding. It needs no egui context, so it
    // answers on any frame.
    let escape = nav.escape(false, || state.escape_rung());
    if let Some(NavAction::Exit(to)) = escape {
        *state = KeybindingsUiState::default();
        nav.leave(to);
        return;
    }
    let screen_nav = nav.nav(false).expect("Keybindings has an exit");

    // Use try_ctx_mut to gracefully handle window close (the context dies
    // with the primary window; ctx_mut panics on the final frame).
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    match draw_keybindings(ctx, &settings.keybindings, &mut state, screen_nav) {
        Some(KeybindingsAction::Exit) => {
            *state = KeybindingsUiState::default();
            nav.leave(screen_nav.destination());
            return;
        }
        Some(KeybindingsAction::ResetToDefaults) => {
            settings.keybindings.reset_to_defaults();
            info!("Keybindings reset to defaults");
        }
        None => {}
    }

    // The armed slot takes the first key pressed this frame.
    let Some(rebind) = state.rebinding else {
        return;
    };
    let Some(&new_key) = keyboard.get_just_pressed().next() else {
        return;
    };
    if let Some(conflicting_action) = settings
        .keybindings
        .is_key_bound(new_key, Some(rebind.action))
    {
        info!("Key {new_key:?} is already bound to {conflicting_action:?}");
    }
    if let Some(mut binding) = settings.keybindings.get(rebind.action).cloned() {
        match rebind.slot {
            KeySlot::Primary => binding.primary = new_key,
            KeySlot::Secondary => binding.secondary = Some(new_key),
        }
        settings.keybindings.set(rebind.action, binding);
    }
    state.rebinding = None;
}

/// Render the Keybindings screen into `ctx`, returning what needs the
/// wrapper.
///
/// Arming a rebind and opening or cancelling the reset confirm are the
/// screen's own business and land in `state`; only the exit and a confirmed
/// reset bubble out. Deliberately free of Bevy ECS system params, so
/// `tests/keybindings_snapshot.rs` can drive it directly.
pub fn draw_keybindings(
    ctx: &egui::Context,
    keybindings: &Keybindings,
    state: &mut KeybindingsUiState,
    nav: Nav,
) -> Option<KeybindingsAction> {
    let mut action = None;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(color::BG)
                .inner_margin(margin_xy(space::XL, space::LG)),
        )
        .show(ctx, |ui| {
            let exit = screen_header(ui, TITLE, nav, |ui| {
                if Button::new("Reset to defaults", Variant::Danger)
                    .driver_id("keybindings:reset")
                    .show(ui)
                    .clicked()
                {
                    state.confirm_reset = true;
                    state.rebinding = None;
                }
            });
            if exit.is_some() {
                action = Some(KeybindingsAction::Exit);
            }
            ui.add_space(space::LG);

            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(CONTENT_MAX_W);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            for (i, category) in CATEGORIES.into_iter().enumerate() {
                                if i > 0 {
                                    ui.add_space(space::MD);
                                }
                                category_panel(ui, category, keybindings, state);
                            }
                        });
                    });
                });
        });

    if state.confirm_reset {
        let mut confirm = false;
        let mut cancel = false;
        let modal = Modal::new(RESET_MODAL_ID, "Reset keybindings?")
            .footer(|ui| {
                confirm = Button::new("Reset", Variant::Danger)
                    .driver_id("reset-keybindings:confirm")
                    .show(ui)
                    .clicked();
                cancel = Button::new("Cancel", Variant::Secondary)
                    .driver_id("reset-keybindings:cancel")
                    .show(ui)
                    .clicked();
            })
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Every action goes back to its default keys. Your custom bindings are lost.",
                    )
                    .size(text::BODY)
                    .color(color::TEXT),
                );
            });
        if confirm {
            state.confirm_reset = false;
            action = action.or(Some(KeybindingsAction::ResetToDefaults));
        } else if cancel || modal.close {
            state.confirm_reset = false;
        }
    }

    action
}

/// One category's panel: a row per action, its primary and secondary key on
/// the right.
fn category_panel(
    ui: &mut egui::Ui,
    category: &str,
    keybindings: &Keybindings,
    state: &mut KeybindingsUiState,
) {
    panel(None).show(ui, |ui| {
        ui.set_width(ui.available_width());
        panel_header(ui, category, color::ACCENT, |ui| {
            // Captions over the two key columns, right to left.
            ui.spacing_mut().item_spacing.x = space::SM;
            ui.add_sized(
                [KEY_W, caption_height(ui)],
                egui::Label::new(caption("Secondary")),
            );
            ui.add_sized(
                [KEY_W, caption_height(ui)],
                egui::Label::new(caption("Primary")),
            );
        });
        let actions = GameAction::all()
            .into_iter()
            .filter(|action| action.category() == category);
        for (i, action) in actions.enumerate() {
            if i > 0 {
                ui.add_space(space::XS);
            }
            binding_row(ui, action, keybindings, state);
        }
    });
}

/// A key-column caption.
fn caption(label: &str) -> egui::RichText {
    egui::RichText::new(label.to_uppercase())
        .size(text::SMALL)
        .color(color::TEXT_MUTED)
}

/// A caption's row height: the interact height, so it centres on the title.
fn caption_height(ui: &egui::Ui) -> f32 {
    ui.spacing().interact_size.y
}

/// One action: its name, then its two key buttons.
fn binding_row(
    ui: &mut egui::Ui,
    action: GameAction,
    keybindings: &Keybindings,
    state: &mut KeybindingsUiState,
) {
    let binding = keybindings.get(action);
    let primary = binding
        .map(|b| Keybindings::key_name(b.primary))
        .unwrap_or("Unbound");
    let secondary = binding
        .and_then(|b| b.secondary)
        .map(Keybindings::key_name)
        .unwrap_or("-");
    // What each slot holds, for a script to read back.
    ui_driver::note(
        ui,
        format_args!("keybind {action:?} primary={primary} secondary={secondary}"),
    );

    // A row as tall as its key buttons, so the name centres on them.
    let row = egui::vec2(ui.available_width(), size::BUTTON);
    let centred = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(row, centred, |ui| {
        ui.label(
            egui::RichText::new(action.description())
                .size(text::BODY)
                .color(color::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            // Right to left: Secondary first, so the row reads Primary,
            // Secondary.
            for (slot, key) in [(KeySlot::Secondary, secondary), (KeySlot::Primary, primary)] {
                let armed = state.rebinding == Some(Rebind { action, slot });
                let label = if armed { "Press a key\u{2026}" } else { key };
                let button = Button::new(label, Variant::Secondary).min_width(KEY_W);
                // An armed slot holds the pressed look until a key lands.
                let button = if armed {
                    button.preview(Look::Pressed)
                } else {
                    button
                };
                let response = button.show(ui);
                ui_driver::mark(
                    ui,
                    response.rect,
                    true,
                    format_args!("keybind:{action:?}:{slot:?}"),
                );
                if response.clicked() {
                    state.rebinding = Some(Rebind { action, slot });
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_cancels_a_pending_rebind_before_anything_else() {
        let mut state = KeybindingsUiState {
            rebinding: Some(Rebind {
                action: GameAction::Confirm,
                slot: KeySlot::Primary,
            }),
            confirm_reset: false,
        };
        assert!(state.escape_rung(), "the first Esc cancels the rebind");
        assert_eq!(state, KeybindingsUiState::default());
        assert!(!state.escape_rung(), "the second Esc is the exit's");
    }

    #[test]
    fn escape_closes_the_reset_confirm_then_leaves() {
        let mut state = KeybindingsUiState {
            rebinding: None,
            confirm_reset: true,
        };
        assert!(state.escape_rung());
        assert_eq!(state, KeybindingsUiState::default());
        assert!(!state.escape_rung());
    }
}
