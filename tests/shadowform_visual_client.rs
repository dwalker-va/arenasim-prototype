//! The Shadowform look in the REAL graphical schedule (`StatesPlugin`, no GPU,
//! the `--replay` entrance straight into `PlayMatch`).
//!
//! `shadowform_visual_probes.rs` drives the systems in a harness of its own;
//! this proves the client registers them, in an order that holds the look
//! through a played match: a Shadowform Priest takes the form before the
//! gates, and its body is still violet and translucent, with its shroud at its
//! feet, once the fight is on — with the stealth visual running every frame
//! beside it. The healing Priest it fights, in no form, is untouched.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{
    ClientParticle, Combatant, OriginalBodyMaterial, ShadowformLook, ShadowformShroud, VisualBody,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    shadowform_body_color, AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::HeadlessMatchConfig;

/// A Shadowform Priest against a healing one. The form is the Shadowform
/// talent's, chosen as the Priest's Form.
const CONFIG: &str = r#"{
    "team1": ["Priest"],
    "team2": ["Priest"],
    "team1_builds": [{"ImprovedMindBlast": 2, "Shadowform": 1}],
    "team1_priest_forms": ["Shadowform"],
    "map": "BasicArena",
    "random_seed": 3
}"#;

/// A 60Hz display.
const FRAME: Duration = Duration::from_micros(16_667);

/// The Priest's shipped body colour (`class_mesh_color`).
const PRIEST_WHITE: Color = Color::srgb(0.95, 0.95, 0.95);

fn boot() -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(CONFIG).unwrap();
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
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .insert_state(GameState::PlayMatch);
    app
}

/// What one Priest's body shows: its own material (the one Fear or Polymorph
/// displaced, if one holds it), whether the look is marked, and its shroud's
/// live particles (`None` without a shroud).
struct Seen {
    colour: Color,
    alpha_mode: AlphaMode,
    marked: bool,
    shroud_particles: Option<usize>,
}

fn seen(app: &mut App, team: u8) -> Seen {
    let world = app.world_mut();
    let (unit, children) = world
        .query::<(Entity, &Combatant, &Children)>()
        .iter(world)
        .find(|(_, c, _)| c.team == team)
        .map(|(e, _, ch)| (e, ch.iter().collect::<Vec<_>>()))
        .expect("the Priest");
    let body = children
        .into_iter()
        .find(|&c| world.get::<VisualBody>(c).is_some())
        .expect("its body");
    let own = world
        .get::<OriginalBodyMaterial>(body)
        .map(|d| d.0.clone())
        .unwrap_or_else(|| {
            world
                .get::<MeshMaterial3d<StandardMaterial>>(body)
                .unwrap()
                .0
                .clone()
        });
    let material = world
        .resource::<Assets<StandardMaterial>>()
        .get(&own)
        .unwrap()
        .clone();
    let marked = world.get::<ShadowformLook>(body).is_some();
    let rigs: Vec<Entity> = world
        .query::<(Entity, &ShadowformShroud)>()
        .iter(world)
        .filter(|(_, s)| s.owner == unit)
        .map(|(e, _)| e)
        .collect();
    assert!(rigs.len() <= 1, "one shroud a Priest, got {}", rigs.len());
    let shroud_particles = rigs.first().map(|&rig| {
        world.get::<Children>(rig).map_or(0, |c| {
            c.iter()
                .filter(|&p| world.get::<ClientParticle>(p).is_some())
                .count()
        })
    });
    Seen {
        colour: material.base_color,
        alpha_mode: material.alpha_mode,
        marked,
        shroud_particles,
    }
}

#[test]
fn a_shadowform_priest_holds_its_look_through_the_match() {
    let mut app = boot();
    // The 10 s countdown, in which the Priest takes the form, and 5 s of
    // fighting with the stealth visual running every frame.
    for _ in 0..(15 * 60) {
        app.update();
        assert_eq!(
            *app.world().resource::<State<GameState>>().get(),
            GameState::PlayMatch,
            "the match is still on"
        );
    }

    let shadow = seen(&mut app, 1);
    assert!(shadow.marked, "the look is on the Shadowform Priest");
    // Exactly the bench's mix of the Priest's own colour (a duplicate class
    // is drawn darker, so either Priest may be the white one).
    let violet = shadowform_body_color(PRIEST_WHITE);
    let darker = shadowform_body_color(Color::srgb(0.95 * 0.65, 0.95 * 0.65, 0.95 * 0.65));
    assert!(
        shadow.colour == violet || shadow.colour == darker,
        "violet: {:?}",
        shadow.colour
    );
    assert_eq!(shadow.colour.alpha(), 0.65, "translucent");
    assert_eq!(shadow.alpha_mode, AlphaMode::Blend);
    let particles = shadow.shroud_particles.expect("its shroud");
    assert!(particles > 50, "the shroud is emitting: {particles}");

    let healer = seen(&mut app, 2);
    assert!(!healer.marked, "no look on the healing Priest");
    let c = healer.colour.to_srgba();
    assert!(c.red == c.green && c.green == c.blue, "untinted: {c:?}");
    assert_eq!(c.alpha, 1.0, "opaque");
    assert_eq!(healer.shroud_particles, None, "no shroud");
}
