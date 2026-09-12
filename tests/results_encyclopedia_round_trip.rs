//! The Results ⇄ Encyclopedia round trip, driven through the REAL state machine.
//!
//! ## Why this exists
//!
//! The Results screen's linked icons send the reader into the encyclopedia and
//! promise to bring them back to the same numbers. That promise rests on two
//! facts the type system does not enforce:
//!
//! 1. `MatchResults` is discarded on exactly ONE path — the DONE button. If a
//!    later change discarded it on the way out of the state instead, the reader
//!    would come back to "No match results available" and the screen would be a
//!    trap.
//! 2. The encyclopedia's `return_to` actually points at `Results`.
//!
//! Both are asserted here against a real `App` with the real `StatesPlugin`
//! schedule, so this is a round trip rather than an assertion about intent.
//!
//! It doubles as the Results screen's boot test. Bevy validates a system's
//! parameters at run time, PER STATE: `results_ui` grew four resource
//! parameters when it adopted the linked-icon widget, and a missing one would
//! compile, snapshot perfectly (the snapshot drives the pure draw and never
//! touches the ECS) and then panic the instant a real match ended. Entering the
//! state and ticking it runs that validation. See `tests/encyclopedia_boot.rs`,
//! whose harness this borrows.

use bevy::asset::AssetPlugin;
use bevy::ecs::system::RunSystemOnce as _;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;

use arenasim::combat::CombatPlugin;
use arenasim::states::encyclopedia::{EncyclopediaState, Topic};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    AbilityConfigPlugin, CombatantStats, MapConfigPlugin, MatchResults, MovementConfigPlugin,
};
use arenasim::states::results_ui::{apply_results_action, ResultsAction};
use arenasim::states::{GameState, StatesPlugin};

/// Builds the app with everything `StatesPlugin` needs, minus the window and
/// renderer, so this runs in the default `cargo test` with no GPU adapter.
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
        // and returns early — AFTER its other parameters have been validated,
        // which is exactly the check this file is for.
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

/// A 1v1 result with values distinctive enough that "the same results" means
/// something stronger than "some results".
fn mock_results() -> MatchResults {
    MatchResults {
        winner: Some(2),
        duration_secs: 91.5,
        team1_combatants: vec![CombatantStats {
            class: CharacterClass::Mage,
            slot: 0,
            damage_dealt: 1234.0,
            damage_taken: 2345.0,
            healing_done: 0.0,
            survived: false,
        }],
        team2_combatants: vec![CombatantStats {
            class: CharacterClass::Warrior,
            slot: 0,
            damage_dealt: 2345.0,
            damage_taken: 1234.0,
            healing_done: 0.0,
            survived: true,
        }],
        pet_damage_links: Default::default(),
    }
}

fn enter_results(app: &mut App) {
    app.update();
    app.insert_resource(mock_results());
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::Results);
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::Results,
        "the results state was left before it could be ticked"
    );
}

/// Ticking the Results screen must not panic — the parameter-validation check.
#[test]
fn the_results_state_can_be_entered() {
    let mut app = boot_app();
    enter_results(&mut app);
    assert!(app.world().get_resource::<MatchResults>().is_some());
}

/// Click a linked icon, read the page, come back — and find the same numbers.
#[test]
fn stepping_into_the_encyclopedia_and_back_keeps_the_same_results() {
    let mut app = boot_app();
    enter_results(&mut app);

    // A linked icon was clicked. This is the REAL handler the Bevy wrapper runs.
    app.world_mut()
        .run_system_once(open_mage)
        .expect("open the Mage page");
    for _ in 0..4 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::Encyclopedia
    );
    assert!(
        app.world().get_resource::<MatchResults>().is_some(),
        "the detour into the encyclopedia discarded the results"
    );
    assert_eq!(
        app.world().resource::<EncyclopediaState>().return_to(),
        GameState::Results,
        "the encyclopedia does not know where it was opened from"
    );

    // Leave the encyclopedia the way the reader does — the Back key at the
    // landing topic, which is the root of a deep link.
    let destination = {
        let mut encyclopedia = app.world_mut().resource_mut::<EncyclopediaState>();
        assert!(encyclopedia.back_key(), "Back at a deep link must leave");
        encyclopedia.leave()
    };
    assert_eq!(destination, GameState::Results);
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(destination);
    for _ in 0..4 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::Results
    );
    let results = app
        .world()
        .get_resource::<MatchResults>()
        .expect("the results must survive the round trip");
    let expected = mock_results();
    assert_eq!(results.winner, expected.winner);
    assert_eq!(results.duration_secs, expected.duration_secs);
    assert_eq!(
        results.team1_combatants[0].damage_dealt,
        expected.team1_combatants[0].damage_dealt
    );
    assert_eq!(
        results.team2_combatants[0].damage_dealt,
        expected.team2_combatants[0].damage_dealt
    );
}

/// The control for the test above: DONE is the one path that DOES discard, so
/// "the results survived" is a real property of the encyclopedia detour rather
/// than of a resource nothing ever removes.
#[test]
fn done_is_the_only_path_that_discards_the_results() {
    let mut app = boot_app();
    enter_results(&mut app);

    app.world_mut()
        .run_system_once(press_done)
        .expect("press DONE");
    for _ in 0..4 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::MainMenu
    );
    assert!(
        app.world().get_resource::<MatchResults>().is_none(),
        "DONE must discard the results"
    );
}

fn open_mage(
    mut encyclopedia: ResMut<EncyclopediaState>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    apply_results_action(
        Some(ResultsAction::OpenTopic(Topic::Class(CharacterClass::Mage))),
        &mut encyclopedia,
        &mut next_state,
        &mut commands,
    );
}

fn press_done(
    mut encyclopedia: ResMut<EncyclopediaState>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    apply_results_action(
        Some(ResultsAction::Done),
        &mut encyclopedia,
        &mut next_state,
        &mut commands,
    );
}
