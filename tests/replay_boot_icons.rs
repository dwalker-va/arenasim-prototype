//! The `--replay` boot path, walked cold: straight into `PlayMatch`.
//!
//! ## Why this exists
//!
//! `run_replay_mode` builds the graphical app with
//! `.insert_state(GameState::PlayMatch)`, so a replay never passes through
//! ConfigureMatch — the state whose chain runs `load_class_icons`. Every screen
//! on the replay path that paints a class icon therefore drew nothing: the
//! in-match team frames and the speech bubbles for the whole match, and the
//! Results screen after it (fixed earlier, pinned in
//! `tests/results_encyclopedia_round_trip.rs`).
//!
//! This walks that entrance the way the player does — cold state, no
//! ConfigureMatch — and asserts each loader the match chrome depends on has
//! actually run by the time its consumer does. It is the runtime half of the
//! fix; `tests/icon_loader_registration_audit.rs` is the static half that
//! closes the class for screens nobody has written yet.
//!
//! ## What it pins, honestly stated
//!
//! The test app has no image codec (decoding wants a GPU adapter, which the
//! default `cargo test` has no business requiring), so the textures can never
//! finish registering with egui here. What a loader does on its FIRST run,
//! before any decoding, is request one handle per icon. So these assertions key
//! on the handle request — which is exactly what goes missing when the loader
//! is absent from the state's schedule, and exactly what the defect was.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;

use arenasim::combat::CombatPlugin;
use arenasim::states::configure_match_ui::ClassIconHandles;
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin, SpellIconHandles,
};
use arenasim::states::{GameState, StatesPlugin};

/// The real `StatesPlugin` schedule, minus the window and renderer, so this
/// runs in the default `cargo test`. Same shape as `tests/encyclopedia_boot.rs`
/// and `tests/animation_sandbox_boot.rs`.
fn boot_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        .add_plugins(bevy::input::InputPlugin)
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .init_state::<GameState>()
        // `EguiContexts` validates against this resource. With no `EguiContext`
        // component to find, every egui system's `try_ctx_mut()` returns `None`
        // and returns early — AFTER its other parameters have been validated.
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        .init_asset::<bevy::scene::Scene>()
        .init_asset::<bevy::gltf::Gltf>()
        .init_asset::<bevy::gltf::GltfNode>()
        .init_asset::<bevy::gltf::GltfMesh>()
        .init_asset::<bevy::gltf::GltfPrimitive>()
        .add_plugins((
            AbilityConfigPlugin,
            MovementConfigPlugin,
            MapConfigPlugin,
            EquipmentPlugin,
            CombatPlugin,
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ));
    app
}

/// Boot the way `--replay` does: the very first state the app ever holds is
/// `PlayMatch`. No main menu, no ConfigureMatch, nothing that could have filled
/// an icon resource on the way in.
fn boot_into_play_match() -> App {
    let mut app = boot_app();
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::PlayMatch);
    // OnEnter runs on the first tick; the Update chain that reads what it
    // inserted runs on the ones after.
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::PlayMatch,
        "the match ended before the chrome could be ticked"
    );
    app
}

/// The class portraits on the team frames (`render_team_frames`) and in the
/// speech bubbles (`render_speech_bubbles`) must be loaded during the match
/// itself, not one screen later.
#[test]
fn a_replay_match_loads_the_class_icons() {
    let app = boot_into_play_match();

    let handles = app.world().resource::<ClassIconHandles>();
    assert!(
        !handles.handles.is_empty(),
        "the class icon loader never ran in the PlayMatch state — `--replay` \
         skips ConfigureMatch, so the team frames and speech bubbles paint \
         class-less for the entire match"
    );
}

/// The control: the spell icons the same team-frame system reads have always
/// been loaded here, so "the class icons were missing" is a property of that
/// one loader's registration rather than of the harness failing to tick the
/// match chrome at all.
#[test]
fn a_replay_match_loads_the_spell_icons() {
    let app = boot_into_play_match();

    let handles = app.world().resource::<SpellIconHandles>();
    assert!(
        !handles.handles.is_empty(),
        "the spell icon loader never ran in the PlayMatch state — the aura \
         rows on the team frames would be blank too"
    );
}
