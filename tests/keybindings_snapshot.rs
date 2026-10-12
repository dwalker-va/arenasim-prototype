//! Offscreen visual snapshots of the Keybindings screen and its reset
//! confirm, plus the reset's confirm contract driven through the real draw.
//!
//! ```bash
//! # Render; writes tests/snapshots/keybindings_*.new.png on any diff
//! cargo test --release --test keybindings_snapshot -- --ignored
//! # ...read those PNGs, tweak keybindings_ui.rs, repeat. Then bless:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test keybindings_snapshot -- --ignored
//! ```
//!
//! The snapshots are `#[ignore]`d because they need a GPU adapter. The
//! contract tests are not: they lay the screen out in a bare `egui::Context`
//! with the `--ui-script` registry armed, find each button by its driver id,
//! and click it with raw pointer events, so the default `cargo test` holds
//! them. Everything is drawn at 1280x720, the client's default window.

use bevy_egui::egui;
use egui_kittest::Harness;

use arenasim::keybindings::{GameAction, Keybindings};
use arenasim::states::keybindings_ui::{
    draw_keybindings, KeySlot, KeybindingsAction, KeybindingsUiState, Rebind,
};
use arenasim::states::GameState;
use arenasim::ui::driver::registry;
use arenasim::ui::widgets::Nav;

/// The client's default window, in points.
const DEFAULT_WINDOW: [f32; 2] = [1280.0, 720.0];

fn nav() -> Nav {
    Nav::Return(GameState::Options)
}

/// The default bindings, with one key armed for a rebind so the pressed look
/// is pinned too.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn keybindings_default() {
    snapshot(
        "keybindings_default",
        KeybindingsUiState {
            rebinding: Some(Rebind {
                action: GameAction::Confirm,
                slot: KeySlot::Secondary,
            }),
            confirm_reset: false,
        },
    );
}

/// Reset to Defaults asks first.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn keybindings_reset_confirm() {
    snapshot(
        "keybindings_reset_confirm",
        KeybindingsUiState {
            rebinding: None,
            confirm_reset: true,
        },
    );
}

fn snapshot(name: &str, state: KeybindingsUiState) {
    let keybindings = Keybindings::default();
    let mut state = state;
    let mut harness = Harness::builder()
        .with_size(DEFAULT_WINDOW)
        .build(move |ctx| {
            arenasim::ui::install(ctx);
            let _ = draw_keybindings(ctx, &keybindings, &mut state, nav());
        });
    harness.run();
    harness.snapshot(name);
}

/// The screen in a bare context, one pass at a time, with the registry armed.
struct Screen {
    ctx: egui::Context,
    keybindings: Keybindings,
    state: KeybindingsUiState,
    events: Vec<egui::Event>,
}

impl Screen {
    fn new() -> Self {
        let ctx = egui::Context::default();
        arenasim::ui::install(&ctx);
        let mut screen = Self {
            ctx,
            keybindings: Keybindings::default(),
            state: KeybindingsUiState::default(),
            events: Vec::new(),
        };
        // Let the layout settle before anything is aimed at.
        for _ in 0..3 {
            screen.pass();
        }
        screen
    }

    /// One frame. Returns the action the draw reported.
    fn pass(&mut self) -> Option<KeybindingsAction> {
        registry::arm(&self.ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::from(DEFAULT_WINDOW),
            )),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        let mut action = None;
        let (keybindings, state) = (&self.keybindings, &mut self.state);
        let _ = self.ctx.run(input, |ctx| {
            action = draw_keybindings(ctx, keybindings, state, nav());
        });
        action
    }

    fn frame(&self) -> registry::Frame {
        registry::snapshot(&self.ctx).expect("armed")
    }

    fn is_visible(&self, id: &str) -> bool {
        self.frame().is_visible(id)
    }

    /// Move onto the widget registered as `id`, press and release it, and
    /// return every action reported on the way.
    fn click(&mut self, id: &str) -> Vec<KeybindingsAction> {
        let frame = self.frame();
        let widget = frame
            .widget(id)
            .unwrap_or_else(|| panic!("{id} is not drawn"));
        assert!(widget.visible, "{id} is not on screen");
        let pos = widget.rect.center();
        let mut actions = Vec::new();
        self.events.push(egui::Event::PointerMoved(pos));
        actions.extend(self.pass());
        for pressed in [true, false] {
            self.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            actions.extend(self.pass());
        }
        actions.extend(self.pass());
        actions
    }
}

/// Reset to Defaults sits in the header, on screen at the default window.
#[test]
fn reset_is_on_screen_at_the_default_window_size() {
    let screen = Screen::new();
    assert!(screen.is_visible("keybindings:reset"));
    assert!(screen.is_visible("nav:exit"));
}

/// Reset only opens the confirm; Cancel closes it with nothing reset.
#[test]
fn cancelling_the_reset_resets_nothing() {
    let mut screen = Screen::new();
    assert!(screen.click("keybindings:reset").is_empty());
    assert!(screen.state.confirm_reset, "Reset opens the confirm");
    assert!(screen.is_visible("reset-keybindings:confirm"));
    assert!(screen.is_visible("reset-keybindings:cancel"));

    assert!(
        screen.click("reset-keybindings:cancel").is_empty(),
        "Cancel asks for nothing"
    );
    assert!(!screen.state.confirm_reset, "Cancel closes the confirm");
    assert!(!screen.is_visible("reset-keybindings:confirm"));
}

/// Confirming the reset is what asks for it — exactly once — and closes the
/// confirm.
#[test]
fn confirming_the_reset_asks_for_it_once() {
    let mut screen = Screen::new();
    screen.click("keybindings:reset");
    assert_eq!(
        screen.click("reset-keybindings:confirm"),
        vec![KeybindingsAction::ResetToDefaults]
    );
    assert!(!screen.state.confirm_reset);
}

/// Clicking a key arms that slot for a rebind; opening Reset disarms it.
#[test]
fn a_key_click_arms_its_slot_and_reset_disarms_it() {
    let mut screen = Screen::new();
    screen.click("keybind:Confirm:Primary");
    assert_eq!(
        screen.state.rebinding,
        Some(Rebind {
            action: GameAction::Confirm,
            slot: KeySlot::Primary,
        })
    );
    screen.click("keybindings:reset");
    assert_eq!(screen.state.rebinding, None);
}
