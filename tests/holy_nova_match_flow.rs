//! Holy Nova's visuals in a REAL match, through the client's own schedule
//! (AS-355): the Priest AI's nova raises its dome at the reach in the
//! Priest's book, each enemy it strikes gets the high starburst, each ally it
//! heals the star fall — and every rig goes, with no command flush meeting a
//! rig despawned under it (AS-507's symptom).
//!
//! `tests/holy_nova_visual_probes.rs` drives the systems in a bare app, in an
//! order it chains itself; only the real `StatesPlugin` schedule decides the
//! order the client runs them in, the router in `FixedUpdate` included. This
//! boots `PlayMatch` the way `--replay` does (`rig_despawn_match_flow.rs`'s
//! shape) and plays a seeded match whose Priest novas: Holy Nova's talent, a
//! Hunter's Spider and a Warrior on it at TwinPillars.

use std::collections::BTreeMap;
use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{Pet, StarburstScale};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    holy_nova_secs, star_fall_secs, starburst_secs, AbilityConfigPlugin, GameRng, HolyNovaDome,
    HolyStarFall, HolyStarburst, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::HeadlessMatchConfig;

/// One rendered frame at 60 fps.
const FRAME: Duration = Duration::from_micros(16_667);
/// The longest the match is played looking for the nova.
const MAX_FRAMES: usize = 60 * 90;

/// A Holy Nova Priest and a Mage against a Hunter (with its Spider) and a
/// Warrior, `TeamPlan`, on TwinPillars: its Priest novas with both enemies
/// on it.
const NOVA_MATCH: &str = r#"{"team1":["Priest","Mage"],"team2":["Hunter","Warrior"],
    "map":"TwinPillars","ai_profile":"TeamPlan","random_seed":11,
    "team1_builds":[{"SpiritualHealing":2,"Inspiration":1,"HolyConcentration":1,"HolyNova":1},{}]}"#;

fn boot(cfg: &str) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(cfg).unwrap();
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
        .add_plugins(bevy::transform::TransformPlugin)
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
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    app
}

fn count(log: &CombatLog, what: &str) -> usize {
    log.entries
        .iter()
        .filter(|e| e.message.contains(what))
        .count()
}

/// Each rig's life in frames: the dome until its last star is spent, the
/// high starburst until its last shower, the fall 2.65 s.
fn life_frames(kind: u8) -> usize {
    let secs = match kind {
        0 => holy_nova_secs(),
        1 => starburst_secs(StarburstScale::High),
        _ => star_fall_secs(),
    };
    (secs / FRAME.as_secs_f32()).ceil() as usize + 1
}

/// The match's novas, in the client: a dome for every nova cast, each at the
/// Priest's book's 10 yards; a high starburst for every enemy unit struck —
/// the Spider among them — and a star fall for every ally healed, rig for
/// logged landing; and every rig gone once its life has played.
#[test]
fn a_real_nova_raises_its_dome_bursts_and_falls_and_they_all_go() {
    let mut app = boot(NOVA_MATCH);
    // Each rig: its kind, and the frames it was first and last seen.
    let mut rigs: BTreeMap<Entity, (u8, usize, usize)> = BTreeMap::new();
    let mut first_nova = None;
    let mut pet_burst = false;
    let mut end = 0;
    for frame in 0..MAX_FRAMES {
        app.update();
        end = frame;
        let world = app.world_mut();
        let mut seen = |e: Entity, kind: u8| {
            rigs.entry(e).or_insert((kind, frame, frame)).2 = frame;
        };
        for (e, d) in world.query::<(Entity, &HolyNovaDome)>().iter(world) {
            assert_eq!(d.reach, 10.0);
            seen(e, 0);
            first_nova.get_or_insert(frame);
        }
        let mut pets = world.query_filtered::<(), With<Pet>>();
        for (e, b) in world.query::<(Entity, &HolyStarburst)>().iter(world) {
            assert_eq!(b.scale, StarburstScale::High);
            pet_burst |= pets.get(world, b.target).is_ok();
            seen(e, 1);
        }
        for (e, _) in world.query::<(Entity, &HolyStarFall)>().iter(world) {
            seen(e, 2);
        }
        // Play on until the first nova's rigs have all had time to go.
        if first_nova.is_some_and(|f| frame > f + life_frames(2) + 30) {
            break;
        }
    }
    assert!(first_nova.is_some(), "the Priest never cast Holy Nova");
    let log = app.world().resource::<CombatLog>();
    let (casts, hits, heals) = (
        count(log, "casts Holy Nova"),
        count(log, "Holy Nova hits") + count(log, "Holy Nova CRITS"),
        count(log, "Holy Nova heals") + count(log, "Holy Nova CRITICALLY heals"),
    );
    let of = |kind: u8| rigs.values().filter(|r| r.0 == kind).count();
    assert!(casts > 0 && hits > 0 && heals > 0, "{casts} {hits} {heals}");
    assert_eq!(of(0), casts, "a dome a nova");
    assert_eq!(of(1), hits, "a starburst a hit");
    assert_eq!(of(2), heals, "a fall a heal");
    assert!(pet_burst, "the Spider was struck and burst");
    let mut gone = 0;
    for (e, (kind, first, last)) in &rigs {
        assert!(
            last - first <= life_frames(*kind),
            "{e:?} outlived its life"
        );
        if end - first > life_frames(*kind) {
            assert!(*last < end, "{e:?} never went");
            gone += 1;
        }
    }
    assert!(gone > 0, "no rig was old enough to have gone");
}
