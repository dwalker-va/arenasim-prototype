//! Offscreen visual snapshot of the Configure Match screen.
//!
//! This is the fast visual-iteration loop for `src/states/configure_match_ui.rs`:
//! it renders the real `draw_configure_match` to a PNG via `egui_kittest`
//! (wgpu, no window, no Bevy ECS) in a fraction of a second.
//!
//! ## Loop
//! ```bash
//! # Render the screen; writes tests/snapshots/configure_match.new.png
//! cargo test --release --test configure_match_snapshot -- --ignored
//! # ...then open / read that PNG, tweak configure_match_ui.rs, repeat.
//!
//! # Once it looks right, bless the baseline (test then passes as a
//! # regression guard; a future pixel change writes a .new.png + .diff.png):
//! UPDATE_SNAPSHOTS=1 cargo test --release --test configure_match_snapshot -- --ignored
//! ```
//!
//! `#[ignore]` keeps it out of the default `cargo test` run because it needs a
//! GPU adapter (wgpu), which CI runners may lack.
//!
//! Fidelity caveats vs the real client: `egui_kittest` has no Bevy textures, so
//! the class icons render as class-color fallback squares and fonts are egui
//! defaults (the app installs Rajdhani via a Startup system). Layout, spacing,
//! and color iterate faithfully; pixel-exact icon/font fidelity still needs the
//! real client.

use egui_kittest::Harness;

use arenasim::states::configure_match_ui::{draw_configure_match, CharacterPickerState, ClassIcons};
use arenasim::states::match_config::{ArenaMap, CharacterClass, MatchConfig};
use arenasim::states::play_match::map_config::MapGeometryConfig;

/// A representative 2v2 config: Team 1 fully filled (Warrior + Priest), Team 2
/// with one filled slot (Mage) and one empty slot — so the baseline exercises
/// all three slot states (filled, empty-active, inactive) in one frame.
fn mock_config() -> MatchConfig {
    let mut config = MatchConfig::default();
    config.set_team1_size(2);
    config.set_team2_size(2);
    config.team1[0] = Some(CharacterClass::Warrior);
    config.team1[1] = Some(CharacterClass::Priest);
    config.team2[0] = Some(CharacterClass::Mage);
    config.team2[1] = None; // empty active slot
    config.map = ArenaMap::PillaredArena;
    config
}

#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn configure_match_2v2() {
    let mut config = mock_config();
    let mut picker = CharacterPickerState::default();
    let icons = ClassIcons::default(); // no textures -> class-color fallback squares
    let map_geometry = MapGeometryConfig::default();

    let mut harness = Harness::builder()
        .with_size([1500.0, 900.0])
        .build(move |ctx| {
            let _ = draw_configure_match(ctx, &mut config, &mut picker, &icons, &map_geometry, None);
        });

    harness.run();
    harness.snapshot("configure_match");
}

/// The character-picker modal open over the base screen, so the picker layout
/// (all eight classes, icons, descriptions) is guarded too.
#[test]
#[ignore = "needs a GPU (wgpu); run explicitly with -- --ignored"]
fn configure_match_picker_open() {
    let mut config = mock_config();
    // Open on Team 2 slot 0 (the Mage) so the "SELECTED" marker is exercised.
    let mut picker = CharacterPickerState {
        active: true,
        team: 2,
        slot: 0,
    };
    let icons = ClassIcons::default();
    let map_geometry = MapGeometryConfig::default();

    let mut harness = Harness::builder()
        .with_size([1500.0, 900.0])
        .build(move |ctx| {
            let _ = draw_configure_match(ctx, &mut config, &mut picker, &icons, &map_geometry, None);
        });

    harness.run();
    harness.snapshot("configure_match_picker_open");
}
