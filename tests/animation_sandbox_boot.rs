//! Proves the Animation Sandbox state can actually be ENTERED.
//!
//! ## Why this exists
//!
//! Bevy validates a system's parameters at run time, per state. A system gated
//! on `GameState::AnimationSandbox` that reads a resource only
//! `setup_play_match` inserts compiles cleanly, passes every unit test, and
//! renders perfectly in the kittest snapshot — then panics the instant the
//! state is entered. That has now happened twice on this feature:
//!
//! - `Res<SpellIcons>` was read by the panel but only inserted by
//!   `setup_play_match`, so opening the sandbox died on parameter validation.
//! - Registering the resolution systems a second time made their
//!   `SystemTypeSet`s ambiguous and Bevy rejected the `spawn_projectile_visuals`
//!   ordering, killing the app at schedule construction.
//!
//! Neither is reachable from a pure-function test. Both are caught here, because
//! this builds the real `StatesPlugin`, enters the state, and ticks it.
//!
//! ## What it does and does not cover
//!
//! It covers schedule construction and parameter validation for every system
//! that runs in this state — the "does it open" question. It says nothing about
//! whether anything LOOKS right; `tests/animation_sandbox_snapshot.rs` covers
//! layout, and only a human covers the animations themselves.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin};
use arenasim::states::{GameState, StatesPlugin};

/// Builds the app with everything `StatesPlugin` needs, minus the window and
/// renderer.
///
/// Asset collections are registered directly rather than by pulling in
/// `RenderPlugin`/`PbrPlugin`, which would need a GPU adapter and put this test
/// out of CI's reach — the whole point is that it runs in the default
/// `cargo test`.
fn boot_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .add_plugins(BevyStatesPlugin)
        // Camera input reads keyboard/mouse state.
        .add_plugins(bevy::input::InputPlugin)
        // Window entities are queried by camera and picking systems.
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        // `main.rs` owns this, not `StatesPlugin`.
        .init_state::<GameState>()
        // `EguiContexts` validates against this resource. Registering it alone
        // keeps the egui-drawing systems runnable without pulling in the
        // renderer: they find no `EguiContext` component, so `try_ctx_mut()`
        // returns `None` and each returns early — AFTER its other parameters
        // have been validated, which is exactly the check this test is for.
        .init_resource::<bevy_egui::EguiUserTextures>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        // `spawn_combatant` asks the asset server for weapon glTF scenes. The
        // handles are never resolved here (no loaders), but the asset types
        // must be registered or handle allocation panics.
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
            // Owns the `Keybindings` resource the camera input reads.
            arenasim::settings::SettingsPlugin,
            StatesPlugin,
        ));
    app
}

/// Enter the sandbox and tick it. Any system whose parameters cannot be
/// satisfied in this state panics here.
#[test]
fn the_animation_sandbox_state_can_be_entered() {
    let mut app = boot_app();

    // One update in the default state first, so schedule construction and the
    // state transition are exercised separately — a failure then localises to
    // one or the other.
    app.update();

    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);

    // Several ticks: OnEnter runs on the first, and the Update systems that read
    // what OnEnter inserted run on the ones after.
    for _ in 0..4 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::AnimationSandbox,
        "the sandbox state was left before it could be ticked"
    );
}

/// Leaving the sandbox must not strand resources that would change the next
/// match. `cleanup_sandbox` mirrors `cleanup_play_match`'s removals; this pins
/// the one with teeth.
///
/// `setup_play_match` treats a surviving seeded `GameRng` as "a replay
/// pre-seeded this match" and honours it, so a leak here silently fixes the
/// seed of every match played after visiting the sandbox.
#[test]
fn leaving_the_sandbox_does_not_strand_the_rng() {
    use arenasim::states::play_match::GameRng;

    let mut app = boot_app();
    app.update();

    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::AnimationSandbox);
    for _ in 0..4 {
        app.update();
    }
    assert!(
        app.world().get_resource::<GameRng>().is_some(),
        "the sandbox should have inserted a GameRng for the resolution systems"
    );

    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::MainMenu);
    for _ in 0..2 {
        app.update();
    }

    assert!(
        app.world().get_resource::<GameRng>().is_none(),
        "GameRng survived the sandbox; setup_play_match would honour it as a \
         replay pre-seed and the next match's seed would be fixed by having \
         opened this screen"
    );
}

/// Every dispel entry must actually STRIP something from the staged dummy, or
/// it previews nothing at all — the ribbon and beat are spawned only on a
/// successful dispel.
///
/// This was silently broken: the sandbox staged Arcane Intellect, which
/// `Aura::can_be_dispelled` rejects (it matches magic DEBUFFS only), and fired
/// Purge with no filter where the real Shaman pins one. Every dispel entry
/// resolved, removed nothing, and drew nothing, and no test noticed because
/// the boot test only asked whether the state could be entered. This drives
/// each entry the way the panel does and asks for the ribbon.
#[test]
fn every_dispel_entry_strips_something_in_the_sandbox() {
    use arenasim::states::animation_sandbox::playback::{EntryFamily, SandboxEntry, SandboxPlayback};
    use arenasim::states::animation_sandbox::SandboxConfig;
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::components::DispelRibbon;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    for (ability, family, class) in [
        (AbilityType::DispelMagic, EntryFamily::Component, CharacterClass::Priest),
        (AbilityType::PaladinCleanse, EntryFamily::Component, CharacterClass::Paladin),
        (AbilityType::Purge, EntryFamily::Component, CharacterClass::Shaman),
        (AbilityType::DevourMagic, EntryFamily::Entity, CharacterClass::Warlock),
    ] {
        let mut app = boot_app();
        // The resolution systems run in FixedUpdate; a tight test loop advances
        // real time by microseconds, so drive the clock by hand.
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
        app.update();
        app.insert_resource(SandboxConfig {
            caster_class: class,
            ..Default::default()
        });
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::AnimationSandbox);
        for _ in 0..4 {
            app.update();
        }
        {
            let mut playback = app.world_mut().resource_mut::<SandboxPlayback>();
            playback.select(SandboxEntry::Ability(ability), family);
            playback.restart_requested = true;
        }
        for _ in 0..40 {
            app.update();
        }
        let mut ribbons = app.world_mut().query::<&DispelRibbon>();
        assert!(
            ribbons.iter(app.world()).count() >= 1,
            "{ability:?} ({class:?}) stripped nothing from the staged dummy — \
             nothing to preview"
        );
    }
}
