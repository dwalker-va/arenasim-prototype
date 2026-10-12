//! Offscreen visual snapshots of the Options screen, and the check that every
//! control on it is on screen at the client's default window size.
//!
//! The snapshots render the real `draw_options` to a PNG via `egui_kittest`
//! (wgpu, no window, no Bevy app):
//!
//! ```bash
//! # Render; writes tests/snapshots/options_*.new.png on any diff
//! cargo test --release --test options_snapshot -- --ignored
//! # ...read those PNGs, tweak options_ui.rs, repeat. Then bless:
//! UPDATE_SNAPSHOTS=1 cargo test --release --test options_snapshot -- --ignored
//! ```
//!
//! The snapshots are `#[ignore]`d because they need a GPU adapter. The fit
//! check is not: it lays the screen out in a bare `egui::Context` with the
//! `--ui-script` registry armed, so the default `cargo test` holds it.
//!
//! Both are drawn at 1280x720, the size a new player's window opens at
//! (`GameSettings::default()`: Windowed, `HD720`) — the size at which the
//! Keybindings button used to sit below the bottom of the screen.

use bevy_egui::egui;
use egui_kittest::Harness;

use arenasim::settings::{GameSettings, WindowModeOption};
use arenasim::states::options_ui::{draw_options, OptionsData};
use arenasim::states::GameState;
use arenasim::ui::driver::registry;
use arenasim::ui::widgets::Nav;

/// The client's default window, in points.
const DEFAULT_WINDOW: [f32; 2] = [1280.0, 720.0];

fn nav() -> Nav {
    Nav::Return(GameState::MainMenu)
}

/// A fresh install's settings, as the screen first shows them.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn options_default() {
    snapshot("options_default", GameSettings::default(), false);
}

/// A window-mode change waiting on a restart: the notice is the tallest the
/// screen gets, and it still fits.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn options_restart_required() {
    let settings = GameSettings {
        window_mode: WindowModeOption::BorderlessFullscreen,
        vsync: false,
        ..GameSettings::default()
    };
    snapshot("options_restart_required", settings, true);
}

fn snapshot(name: &str, settings: GameSettings, restart_required: bool) {
    let mut harness = Harness::builder()
        .with_size(DEFAULT_WINDOW)
        .build(move |ctx| {
            arenasim::ui::install(ctx);
            let data = OptionsData {
                settings: &settings,
                restart_required,
            };
            let _ = draw_options(ctx, &data, nav());
        });
    harness.run();
    harness.snapshot(name);
}

/// Lay the screen out at `size` with the driver's registry armed, and return
/// what the last frame registered.
fn registered_frame(
    size: [f32; 2],
    settings: &GameSettings,
    restart_required: bool,
) -> registry::Frame {
    let ctx = egui::Context::default();
    arenasim::ui::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::from(size));
    // A few passes, so anything egui sizes from the previous frame settles.
    for _ in 0..3 {
        registry::arm(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            let data = OptionsData {
                settings,
                restart_required,
            };
            let _ = draw_options(ctx, &data, nav());
        });
    }
    registry::snapshot(&ctx).expect("armed")
}

/// Every control on Options, Keybindings above all, is fully on screen in a
/// 1280x720 window — with or without the restart notice — so no player has to
/// scroll to find one.
#[test]
fn every_control_is_on_screen_at_the_default_window_size() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::from(DEFAULT_WINDOW));
    for restart_required in [false, true] {
        let frame = registered_frame(DEFAULT_WINDOW, &GameSettings::default(), restart_required);
        let ids = [
            "nav:exit",
            "options:window:Windowed",
            "options:window:BorderlessFullscreen",
            "options:resolution:HD720",
            "options:resolution:HD1080",
            "options:resolution:QHD1440",
            "options:vsync:on",
            "options:vsync:off",
            "options:aura-icons:on",
            "options:aura-icons:off",
            "options:keybindings",
        ];
        for id in ids {
            let widget = frame.widget(id).unwrap_or_else(|| {
                panic!("{id} is not drawn (restart_required={restart_required})")
            });
            assert!(
                widget.visible && screen.contains_rect(widget.rect),
                "{id} at {:?} is not wholly inside the {screen:?} window \
                 (restart_required={restart_required})",
                widget.rect
            );
        }
    }
}
