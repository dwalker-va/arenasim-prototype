//! Proves the Encyclopedia state can actually be ENTERED.
//!
//! ## Why this exists
//!
//! Bevy validates a system's parameters at run time, PER STATE. A screen that
//! grows a new `Res<T>` compiles cleanly, passes every unit test, and renders
//! perfectly in the kittest snapshot — because the snapshot drives the pure
//! `draw_encyclopedia` and never touches the ECS — and then panics the instant
//! a player opens the screen, if nothing inserted `T`.
//!
//! The encyclopedia is exactly the shape that goes wrong: its data bundle grows
//! a field per content section (items, then abilities, then auras), and each
//! new field is a new resource the Bevy wrapper has to read. This test enters
//! the state and ticks it, so parameter validation runs against the real
//! `StatesPlugin` schedule.
//!
//! It says nothing about whether anything LOOKS right —
//! `tests/encyclopedia_snapshot.rs` covers that.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin};
use arenasim::states::{GameState, StatesPlugin};

/// Builds the app with everything `StatesPlugin` needs, minus the window and
/// renderer — the same trick `tests/animation_sandbox_boot.rs` uses, so this
/// runs in the default `cargo test` with no GPU adapter.
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
        // `EguiContexts` validates against this resource. With no
        // `EguiContext` component to find, every egui system's `try_ctx_mut()`
        // returns `None` and it returns early — AFTER its other parameters have
        // been validated, which is exactly the check this test is for.
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

/// Enter the encyclopedia and tick it. Any system whose parameters cannot be
/// satisfied in this state — the icon loaders and `encyclopedia_ui` itself —
/// panics here.
#[test]
fn the_encyclopedia_state_can_be_entered() {
    let mut app = boot_app();

    // One update in the default state first, so schedule construction and the
    // state transition are exercised separately.
    app.update();

    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::Encyclopedia);

    for _ in 0..4 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::Encyclopedia,
        "the encyclopedia state was left before it could be ticked"
    );
}

/// The encyclopedia reads the ability registry, which is what makes its class
/// and ability pages derived rather than hand-authored. Pin that the resource
/// the screen needs is actually present in this state — a missing one is the
/// failure this whole file exists to catch.
#[test]
fn the_ability_registry_is_available_to_the_screen() {
    use arenasim::states::play_match::ability_config::AbilityDefinitions;

    let mut app = boot_app();
    app.update();
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::Encyclopedia);
    for _ in 0..4 {
        app.update();
    }

    let abilities = app
        .world()
        .get_resource::<AbilityDefinitions>()
        .expect("AbilityDefinitions must exist wherever the encyclopedia runs");
    assert!(abilities.ability_types().count() > 0);
}

/// The deep-link ROUND TRIP: View Combatant links into the encyclopedia and
/// the way back lands on the SAME combatant.
///
/// `ViewCombatantState` carries the team and slot, and View Combatant REMOVES
/// it on its own Back button — so the invariant is not free, it depends on the
/// encyclopedia link taking a different path. This drives the real app: it
/// enters View Combatant, deep-links exactly as `view_combatant_ui` does
/// (`open_at` + a state transition, no resource removal), ticks the
/// encyclopedia, walks the `Esc`-at-root ladder, and asserts the combatant came
/// back intact.
///
/// The item-page variant is the awkward path the card calls out: a right-click
/// taken MID-PICK. Its resource half is what this covers.
///
/// What it CANNOT cover: the click itself. There is no egui context in this
/// harness (`try_ctx_mut()` returns `None` and every egui system returns
/// early), so the pointer interaction that produces the topic is not
/// exercised — only everything downstream of it. Likewise the picker's
/// open/closed state is a system-`Local`, not a resource, and is out of reach
/// from here.
#[test]
fn a_deep_link_from_view_combatant_returns_to_the_same_combatant() {
    use arenasim::states::encyclopedia::{EncyclopediaState, Topic};
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::equipment::ItemId;
    use arenasim::states::view_combatant_ui::ViewCombatantState;

    for topic in [
        // The kit row: an ability page.
        Topic::Ability(arenasim::states::play_match::AbilityType::AimedShot),
        // The equipment picker's right-click, taken mid-pick.
        Topic::Item(ItemId::WandOfTheInvoker),
    ] {
        let mut app = boot_app();
        app.update();

        app.world_mut().insert_resource(ViewCombatantState {
            class: CharacterClass::Hunter,
            team: 2,
            slot: 1,
        });
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::ViewCombatant);
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<State<GameState>>().get(),
            &GameState::ViewCombatant,
        );

        // The link, exactly as the screen makes it.
        app.world_mut()
            .resource_mut::<EncyclopediaState>()
            .open_at(topic, GameState::ViewCombatant);
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Encyclopedia);
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<State<GameState>>().get(),
            &GameState::Encyclopedia,
        );
        assert!(
            app.world().get_resource::<ViewCombatantState>().is_some(),
            "{:?}: the link dropped the combatant on the way out",
            topic
        );

        // Back at the root — the first press, because `open_at` made the page
        // the root — leaves for wherever the link came from.
        let mut state = app.world_mut().resource_mut::<EncyclopediaState>();
        assert!(state.back_key(), "{:?}: Back did not leave at the root", topic);
        let destination = state.leave();
        assert_eq!(destination, GameState::ViewCombatant);
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(destination);
        for _ in 0..4 {
            app.update();
        }

        assert_eq!(
            app.world().resource::<State<GameState>>().get(),
            &GameState::ViewCombatant,
        );
        let view = app
            .world()
            .get_resource::<ViewCombatantState>()
            .expect("the combatant must survive the round trip");
        assert_eq!(view.team, 2, "{:?}: came back on the wrong team", topic);
        assert_eq!(view.slot, 1, "{:?}: came back on the wrong slot", topic);
        assert_eq!(view.class, CharacterClass::Hunter);
    }
}
