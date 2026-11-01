//! Which AI a match plays, at each entrance (AS-406).
//!
//! The two entrances default differently, on purpose:
//!
//! - a match the player sets up in the graphical client (Configure Match, then
//!   START MATCH) plays the team AI, `TeamPlan`, on both sides;
//! - a headless config that names no profile plays `Legacy`, so every
//!   baseline, probe and sweep recorded that way keeps its meaning;
//! - `--replay` plays its config's own resolution — the headless rule — so a
//!   replay reproduces its headless run.
//!
//! The client half walks the real `StatesPlugin` schedule (the shape of
//! `tests/replay_boot_icons.rs`): menu, Configure Match, then the match, and
//! reads the `AiProfiles` resource `setup_play_match` inserted. The headless
//! half plays a real match and reads the profile back off its report header.
//! A graphical match played to its end carries its profile into the saved
//! report and the Results screen, which names it.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;

use arenasim::combat::CombatPlugin;
use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::run_headless_match;
use arenasim::states::match_config::{CharacterClass, MatchConfig};
use arenasim::states::play_match::ai_profile::{AiProfile, AiProfiles};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{AbilityConfigPlugin, MapConfigPlugin, MovementConfigPlugin};
use arenasim::states::{GameState, StatesPlugin};

/// The real `StatesPlugin` schedule, minus the window and renderer.
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

fn go(app: &mut App, state: GameState) {
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(state);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().resource::<State<GameState>>().get(), &state);
}

/// The profiles the match that is now running plays.
fn match_profiles(app: &App) -> AiProfiles {
    *app.world()
        .get_resource::<AiProfiles>()
        .expect("setup_play_match inserts AiProfiles")
}

/// Configure Match, then START MATCH: the player's match plays `TeamPlan` on
/// both sides.
#[test]
fn a_match_set_up_in_the_client_plays_team_plan() {
    let mut app = boot_app();
    go(&mut app, GameState::ConfigureMatch);
    {
        let mut config = app.world_mut().resource_mut::<MatchConfig>();
        config.team1 = vec![Some(CharacterClass::Warrior), Some(CharacterClass::Priest)];
        config.team2 = vec![Some(CharacterClass::Mage), Some(CharacterClass::Priest)];
        config.team1_size = 2;
        config.team2_size = 2;
    }
    assert!(
        app.world().get_resource::<AiProfiles>().is_none(),
        "nothing on the way to the match chose a profile"
    );
    go(&mut app, GameState::PlayMatch);
    assert_eq!(
        match_profiles(&app),
        AiProfiles::uniform(AiProfile::TeamPlan)
    );
}

/// The client default is not left behind: the NEXT match the player sets up,
/// after a match played under a replay's profile, plays `TeamPlan` again.
#[test]
fn a_replay_profile_does_not_outlive_its_match() {
    let mut app = boot_app();
    app.insert_resource(AiProfiles::uniform(AiProfile::Legacy));
    go(&mut app, GameState::PlayMatch);
    assert_eq!(match_profiles(&app), AiProfiles::uniform(AiProfile::Legacy));
    go(&mut app, GameState::MainMenu);
    go(&mut app, GameState::ConfigureMatch);
    go(&mut app, GameState::PlayMatch);
    assert_eq!(
        match_profiles(&app),
        AiProfiles::uniform(AiProfile::TeamPlan)
    );
}

fn config(json: &str) -> HeadlessMatchConfig {
    serde_json::from_str(json).expect("config parses")
}

/// `--replay` inserts its config's resolution before the match, and the match
/// keeps it: a config with no profile plays `Legacy`, as headless does, and a
/// head-to-head plays both sides as written.
#[test]
fn a_replay_plays_what_its_config_resolves_to() {
    for (json, want) in [
        (
            r#"{"team1":["Warrior"],"team2":["Mage"],"random_seed":7}"#,
            AiProfiles::uniform(AiProfile::Legacy),
        ),
        (
            r#"{"team1":["Warrior"],"team2":["Mage"],"random_seed":7,"ai_profile":"TeamPlan"}"#,
            AiProfiles::uniform(AiProfile::TeamPlan),
        ),
        (
            r#"{"team1":["Warrior"],"team2":["Mage"],"random_seed":7,"team2_ai_profile":"TeamPlan"}"#,
            AiProfiles {
                team1: AiProfile::Legacy,
                team2: AiProfile::TeamPlan,
            },
        ),
    ] {
        let cfg = config(json);
        let mut app = boot_app();
        app.insert_resource(cfg.to_match_config().unwrap())
            .insert_resource(cfg.ai_profiles().unwrap());
        go(&mut app, GameState::PlayMatch);
        assert_eq!(match_profiles(&app), want, "{json}");
    }
}

/// A headless match whose config names no profile plays `Legacy`, and its
/// report header says so.
#[test]
fn a_headless_match_with_no_profile_plays_legacy() {
    let cfg = config(r#"{"team1":["Warrior"],"team2":["Mage"],"random_seed":7}"#);
    assert_eq!(
        cfg.ai_profiles().unwrap(),
        AiProfiles::uniform(AiProfile::Legacy)
    );
    assert_eq!(header_profile(cfg), "Legacy");
}

/// The header records what played, so the line above is not a constant: a
/// TeamPlan config and a head-to-head each write their own.
#[test]
fn the_report_header_names_the_profile_that_played() {
    let teamplan =
        config(r#"{"team1":["Warrior"],"team2":["Mage"],"random_seed":7,"ai_profile":"TeamPlan"}"#);
    assert_eq!(header_profile(teamplan), "TeamPlan");
    let h2h = config(
        r#"{"team1":["Warrior"],"team2":["Mage"],"random_seed":7,"team1_ai_profile":"TeamPlan"}"#,
    );
    assert_eq!(header_profile(h2h), "TeamPlan/Legacy");
}

/// Play `cfg` headless and return the `AI profile:` value from its report's
/// MATCH METADATA block.
fn header_profile(mut cfg: HeadlessMatchConfig) -> String {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    cfg.max_duration_secs = 20.0;
    cfg.output_path = Some(path.to_string_lossy().into_owned());
    run_headless_match(cfg).expect("match runs");
    let report = std::fs::read_to_string(&path).expect("report written");
    let _ = std::fs::remove_file(&path);
    let metadata: Vec<&str> = report
        .lines()
        .skip_while(|l| *l != "MATCH METADATA")
        .take_while(|l| !l.is_empty())
        .collect();
    let lines: Vec<&str> = metadata
        .iter()
        .filter_map(|l| l.strip_prefix("AI profile: "))
        .collect();
    assert_eq!(lines.len(), 1, "one profile line in {metadata:?}");
    lines[0].to_string()
}

/// A graphical match carries the profile it played into the report it saves
/// and the results it hands the Results screen. The match plays a head-to-head
/// so the value read back is neither the client's nor the headless default.
#[test]
fn a_played_match_reports_the_profile_it_played() {
    use arenasim::states::play_match::components::VictoryCelebration;
    use bevy::time::TimeUpdateStrategy;

    const FRAME: std::time::Duration = std::time::Duration::from_micros(16_667);
    let played = AiProfiles {
        team1: AiProfile::TeamPlan,
        team2: AiProfile::Legacy,
    };
    let cfg = config(r#"{"team1":["Warrior"],"team2":["Mage"],"random_seed":7}"#);
    let mut app = boot_app();
    app.add_plugins(bevy::transform::TransformPlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(played);
    go(&mut app, GameState::PlayMatch);
    let mut celebration = None;
    for _ in 0..(330.0 / FRAME.as_secs_f32()) as usize {
        app.update();
        if let Some(c) = app.world().get_resource::<VictoryCelebration>() {
            celebration = Some((c.match_results.ai_profiles, c.match_metadata.ai_profiles));
            break;
        }
    }
    let (results, report) = celebration.expect("the match ended");
    assert_eq!(results, played, "MatchResults");
    assert_eq!(report, played, "the saved report's metadata");
}

/// The Results screen names the profile that played, as text and as the note
/// `tests/ui-scripts/client-plays-team-plan.script` reads.
#[test]
fn the_results_screen_names_the_profile() {
    use arenasim::combat::log::CombatLog;
    use arenasim::states::encyclopedia::EncyclopediaData;
    use arenasim::states::play_match::ability_config::load_ability_definitions;
    use arenasim::states::play_match::equipment::load_item_definitions;
    use arenasim::states::play_match::MatchResults;
    use arenasim::states::results_ui::draw_results_screen;
    use arenasim::ui::driver::registry;
    use arenasim::ui::widgets::Nav;
    use bevy_egui::egui;

    let items = load_item_definitions().expect("items.ron");
    let abilities = load_ability_definitions().expect("abilities.ron");
    let data = EncyclopediaData {
        items: &items,
        abilities: &abilities,
        item_icons: None,
        class_icons: None,
        ability_icons: None,
        talents: arenasim::states::play_match::talents::TalentDefinitions::shipped(),
    };
    for (profiles, want) in [
        (AiProfiles::client_default(), "TeamPlan"),
        (AiProfiles::uniform(AiProfile::Legacy), "Legacy"),
        (
            AiProfiles {
                team1: AiProfile::Legacy,
                team2: AiProfile::TeamPlan,
            },
            "Legacy/TeamPlan",
        ),
    ] {
        let results = MatchResults {
            winner: Some(1),
            duration_secs: 42.0,
            team1_combatants: vec![],
            team2_combatants: vec![],
            pet_damage_links: Default::default(),
            ai_profiles: profiles,
        };
        let ctx = egui::Context::default();
        registry::arm(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            draw_results_screen(
                ctx,
                Some(&results),
                &CombatLog::default(),
                &data,
                Nav::Return(GameState::MainMenu),
            );
        });
        let frame = registry::snapshot(&ctx).expect("armed");
        let notes: Vec<&String> = frame
            .notes
            .iter()
            .filter(|n| n.starts_with("results:ai_profile="))
            .collect();
        assert_eq!(notes, vec![&format!("results:ai_profile={want}")]);
    }
}
