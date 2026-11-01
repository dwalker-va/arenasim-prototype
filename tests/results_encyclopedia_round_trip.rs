//! The Results ⇄ Encyclopedia round trip, driven through the REAL state machine.
//!
//! ## Why this exists
//!
//! The Results screen's linked icons send the reader into the encyclopedia and
//! promise to bring them back to the same numbers. That promise rests on two
//! facts the type system does not enforce:
//!
//! 1. `MatchResults` is discarded on exactly ONE path — the screen's exit, which
//!    its button and Esc both take. If a later change discarded it on the way
//!    out of the state instead, the reader would come back to "No match
//!    results available" and the screen would be a trap.
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
use arenasim::states::chrome::{ScreenContext, ScreenNav};
use arenasim::states::configure_match_ui::ClassIconHandles;
use arenasim::states::encyclopedia::{EncyclopediaState, Topic};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    AbilityConfigPlugin, CombatantStats, MapConfigPlugin, MatchResults, MovementConfigPlugin,
};
use arenasim::states::results_ui::{apply_results_action, ResultsAction};
use arenasim::states::view_combatant_ui::AbilityIconHandles;
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
            build: Default::default(),
        }],
        team2_combatants: vec![CombatantStats {
            class: CharacterClass::Warrior,
            slot: 0,
            damage_dealt: 2345.0,
            damage_taken: 1234.0,
            healing_done: 0.0,
            survived: true,
            build: Default::default(),
        }],
        pet_damage_links: Default::default(),
        ai_profiles: arenasim::states::play_match::ai_profile::AiProfiles::client_default(),
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

/// The ability icons must be there on a COLD entry — straight from a match,
/// having never opened View Combatant or the encyclopedia.
///
/// Round 1 shipped the Results screen reading `AbilityIcons` through
/// `EncyclopediaData` while `load_ability_icons` was registered only under
/// `ViewCombatant` and `Encyclopedia`. Nothing filled the resource on the way
/// into Results, so every ability bar drew the neutral placeholder tile until
/// the reader clicked through to the encyclopedia — whose loader filled it —
/// and came back. That is the exact path a player takes, and no test walked it.
///
/// WHAT THIS PINS, honestly stated: the test app has no image codec (the
/// renderer's `ImagePlugin` wants a GPU adapter, which the default `cargo test`
/// has no business requiring), so the jpegs never finish decoding here and
/// `AbilityIcons::textures` can never fill. What the loader DOES do on its
/// first run, before any decoding, is request one handle per ability that has
/// an icon. So this asserts the loader RAN IN THE RESULTS STATE, keyed on real
/// `abilities.ron` data — which is precisely the registration bug, and leaves
/// the resource empty when the system is absent from the Results schedule.
#[test]
fn entering_results_cold_loads_the_ability_icons() {
    let mut app = boot_app();
    enter_results(&mut app);

    let handles = app.world().resource::<AbilityIconHandles>();
    assert!(
        !handles.handles.is_empty(),
        "the ability icon loader never ran in the Results state — the bars draw \
         placeholder tiles until some other screen happens to fill AbilityIcons"
    );

    // Keyed on abilities named in the bounced screenshot, so a rename in
    // `abilities.ron` that silently drops an icon fails here rather than in a
    // player's eyeball.
    let names: Vec<&str> = handles.handles.iter().map(|(n, _)| n.as_str()).collect();
    for expected in ["Frost Shock", "Lightning Bolt", "Sinister Strike"] {
        assert!(
            names.contains(&expected),
            "no icon handle requested for {expected} — have {names:?}"
        );
    }
}

/// The CLASS icons must be there on a cold entry too — the second loader the
/// Results chain registers, pinned exactly as strongly as the first.
///
/// `load_class_icons` otherwise runs only under ConfigureMatch, and `--replay`
/// boots straight into PlayMatch, so a player watching a replay reached the
/// results screen having never visited the screen that fills them. This
/// harness never visits ConfigureMatch either, which is what makes it the
/// right place to assert it.
///
/// Same honest scope as the ability-icon test above: with no image codec in
/// the test app the textures cannot decode, so this keys on the HANDLE request
/// the loader makes on its first run — which is precisely what goes missing
/// when the system is absent from the Results schedule.
#[test]
fn entering_results_cold_loads_the_class_icons() {
    let mut app = boot_app();
    enter_results(&mut app);

    let handles = app.world().resource::<ClassIconHandles>();
    assert!(
        !handles.handles.is_empty(),
        "the class icon loader never ran in the Results state — every class \
         portrait draws empty until some other screen happens to fill ClassIcons"
    );
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
        app.world()
            .resource::<ScreenContext>()
            .return_to(GameState::Encyclopedia),
        Some(GameState::Results),
        "the encyclopedia does not know where it was opened from"
    );

    // Leave the encyclopedia the way the reader does — the Back key at the
    // landing topic, which is the root of a deep link.
    press_escape(&mut app);

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

/// The control for the test above: the exit is the one path that DOES
/// discard, so "the results survived" is a real property of the encyclopedia
/// detour rather than of a resource nothing ever removes.
#[test]
fn the_exit_is_the_only_path_that_discards_the_results() {
    let mut app = boot_app();
    enter_results(&mut app);

    app.world_mut()
        .run_system_once(press_exit)
        .expect("press the exit");
    for _ in 0..4 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::MainMenu
    );
    assert!(
        app.world().get_resource::<MatchResults>().is_none(),
        "the exit must discard the results"
    );
}

/// Esc on Results takes the exit's own path: it leaves for whatever started
/// the match AND discards the results, rather than skipping the cleanup the
/// button performs.
#[test]
fn esc_on_results_takes_the_exit_and_discards_the_results() {
    let mut app = boot_app();
    enter_results(&mut app);

    press_escape(&mut app);

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::MainMenu
    );
    assert!(
        app.world().get_resource::<MatchResults>().is_none(),
        "Esc must discard the results, as the exit does"
    );
}

/// Results returns to whatever started the match, not to a fixed main menu:
/// starting a match records its context for both the match and its results.
#[test]
fn results_return_to_whatever_started_the_match() {
    let mut app = boot_app();
    app.update();
    app.world_mut()
        .resource_mut::<ScreenContext>()
        .open_from(GameState::Results, GameState::Encyclopedia);
    enter_results(&mut app);

    press_escape(&mut app);

    assert_eq!(
        app.world().resource::<State<GameState>>().get(),
        &GameState::Encyclopedia,
        "the exit went somewhere other than the recorded opener"
    );
}

/// Press and release the Back key (Esc by default) through Bevy's own input
/// events, then let any transition it asked for land.
fn press_escape(app: &mut App) {
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().send_event(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
    for _ in 0..4 {
        app.update();
    }
}

fn open_mage(
    mut encyclopedia: ResMut<EncyclopediaState>,
    mut nav: ScreenNav,
    mut commands: Commands,
) {
    apply_results_action(
        Some(ResultsAction::OpenTopic(Topic::Class(CharacterClass::Mage))),
        &mut encyclopedia,
        &mut nav,
        &mut commands,
    );
}

fn press_exit(
    mut encyclopedia: ResMut<EncyclopediaState>,
    mut nav: ScreenNav,
    mut commands: Commands,
) {
    apply_results_action(
        Some(ResultsAction::Exit),
        &mut encyclopedia,
        &mut nav,
        &mut commands,
    );
}
