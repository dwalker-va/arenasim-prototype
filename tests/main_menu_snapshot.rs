//! Offscreen visual snapshot of the main menu.
//!
//! This is the fast visual-iteration loop for `src/states/main_menu.rs`:
//! it renders the real `draw_main_menu` to a PNG via `egui_kittest`
//! (wgpu, no window) in a fraction of a second.
//!
//! ## Loop
//! ```bash
//! # Render the screen; writes tests/snapshots/main_menu.new.png
//! cargo test --release --test main_menu_snapshot -- --ignored
//! # ...then open / read that PNG, tweak main_menu.rs, repeat.
//!
//! # Once it looks right, bless the baseline (test then passes as a
//! # regression guard; a future pixel change writes a .new.png + .diff.png):
//! UPDATE_SNAPSHOTS=1 cargo test --release --test main_menu_snapshot -- --ignored
//! ```
//!
//! `#[ignore]` keeps it out of the default `cargo test` run because it needs a
//! GPU adapter (wgpu), which CI runners may lack.
//!
//! Fidelity caveats vs the real client: the harness has no 3D scene behind the
//! panel (the ambient arena backdrop is a Bevy render pass), so the menu draws
//! over egui_kittest's default background — the snapshot guards layout,
//! typography, and the vignette/scrim, not the 3D scene. Fonts are egui
//! defaults (Rajdhani is installed by the app's Startup system).

use egui_kittest::Harness;

use arenasim::states::main_menu::draw_main_menu;

/// Fixed time for a deterministic title-pulse phase: 0.6s puts the
/// sin-driven halo near mid-swing so the glow is visible in the baseline.
const SNAPSHOT_TIME_SECS: f32 = 0.6;

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn main_menu() {
    let mut harness = Harness::builder()
        .with_size([1280.0, 800.0])
        .build(|ctx| {
            let _ = draw_main_menu(ctx, SNAPSHOT_TIME_SECS);
        });

    harness.run();
    harness.snapshot("main_menu");
}
