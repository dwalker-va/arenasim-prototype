//! Sacred Shield in a REAL match (AS-375): every absorb the sim raises wears
//! its gold card for as long as it holds, on the unit holding it, and the
//! card plays its death when the absorb ends (in this match every one is
//! spent by the hits it soaks; the 6 s run-out, a purge and a dying bearer
//! are the bare probes'); and every cast lands the rune disc on the unit it
//! shields.
//!
//! The bare-app probes (`sacred_shield_visual_probes.rs`) drive the shield
//! systems alone. This boots `PlayMatch` through the real `StatesPlugin`
//! schedule, as `--replay` does (the `shield_visual_match_flow.rs` shape),
//! with Bevy's own visibility propagation, so it reads what the match path
//! draws off what the match path raises. Runs with no window or GPU.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{
    ActiveAuras, AuraType, Combatant, PaladinOneShot, PaladinOneShotKind, ShieldCard, ShieldKind,
    ShieldLayer, ShieldPhase, ShieldVisual, SACRED_SHIELD_AURA,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::{
    is_live_shield, AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::HeadlessMatchConfig;

/// One rendered frame at 60 fps.
const FRAME: Duration = Duration::from_micros(16_667);
/// The longest the match is played.
const MAX_FRAMES: usize = 60 * 120;
/// A talented Protection Paladin keeping Sacred Shield up on itself and its
/// Rogue against a Warrior and a Mage (the PR's human-testing replay).
const REPLAY: &str = r#"{"team1":["Paladin","Rogue"],"team2":["Warrior","Mage"],"map":"BasicArena","random_seed":1,"ai_profile":"Legacy","team1_builds":[{"SacredDuty":2,"DivineProtection":1,"Toughness":1,"BlessingOfSacrifice":1,"SacredShield":1},{}]}"#;

/// One unit on one frame.
#[derive(Clone, Debug)]
struct Unit {
    entity: Entity,
    /// Points left on its live Sacred Shield absorb, if it holds one.
    absorb: Option<f32>,
    /// Whether it holds the 30 s buff.
    armed: bool,
    /// Its Sacred Shield card's phase, if it wears one.
    phase: Option<ShieldPhase>,
    /// That card's two client layers, visible, standing within 3 yd of the
    /// unit and lit.
    lit_layers: usize,
}

#[derive(Clone, Debug, Default)]
struct Frame {
    units: Vec<Unit>,
    /// The disc one-shots playing, by the unit they play on.
    discs: Vec<Entity>,
}

#[derive(Resource, Default)]
struct Seen(Vec<Frame>);

fn boot() -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(REPLAY).unwrap();
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
        .add_plugins(bevy::render::view::VisibilityPlugin)
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
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>()
        .add_systems(Last, observe);
    app
}

#[allow(clippy::type_complexity)]
fn observe(
    mut seen: ResMut<Seen>,
    units: Query<(Entity, &GlobalTransform, Option<&ActiveAuras>), With<Combatant>>,
    rigs: Query<(&ShieldVisual, &Children)>,
    cards: Query<(
        &ShieldCard,
        &GlobalTransform,
        &MeshMaterial3d<StandardMaterial>,
        &InheritedVisibility,
    )>,
    discs: Query<&PaladinOneShot>,
    materials: Res<Assets<StandardMaterial>>,
) {
    let mut frame = Frame::default();
    for (entity, at, auras) in units.iter() {
        let list = auras.map(|a| a.auras.as_slice()).unwrap_or(&[]);
        let absorb = list
            .iter()
            .find(|a| is_live_shield(ShieldKind::SacredShield, a))
            .map(|a| a.magnitude);
        let armed = list.iter().any(|a| {
            a.effect_type == AuraType::TriggeredAbsorb && a.ability_name == SACRED_SHIELD_AURA
        });
        let rig = rigs
            .iter()
            .filter(|(r, _)| r.bearer == entity && r.kind == ShieldKind::SacredShield)
            // A dying card and a new one can overlap: the live one is read.
            .min_by_key(|(r, _)| r.phase == ShieldPhase::Death);
        let (phase, lit_layers) = match rig {
            None => (None, 0),
            Some((r, kids)) => {
                let lit = kids
                    .iter()
                    .filter_map(|k| cards.get(k).ok())
                    .filter(|(c, tf, m, vis)| {
                        matches!(c.layer, ShieldLayer::Rim | ShieldLayer::StreakA)
                            && vis.get()
                            && tf.translation().distance(at.translation()) < 3.0
                            && materials.get(&m.0).map_or(0.0, |m| m.base_color.alpha()) > 0.1
                    })
                    .count();
                (Some(r.phase), lit)
            }
        };
        frame.units.push(Unit {
            entity,
            absorb,
            armed,
            phase,
            lit_layers,
        });
    }
    frame.discs = discs
        .iter()
        .filter(|d| d.kind == PaladinOneShotKind::SacredShield)
        .map(|d| d.bearer)
        .collect();
    seen.0.push(frame);
}

fn play() -> Vec<Frame> {
    let mut app = boot();
    for _ in 0..MAX_FRAMES {
        app.update();
        if app.world().resource::<State<GameState>>().get() != &GameState::PlayMatch {
            break;
        }
    }
    std::mem::take(&mut app.world_mut().resource_mut::<Seen>().0)
}

#[test]
fn every_absorb_in_a_real_match_wears_its_card_and_lets_it_go() {
    let seen = play();
    let unit = |f: &Frame, e: Entity| f.units.iter().find(|u| u.entity == e).cloned();
    let entities: BTreeSet<Entity> = seen
        .iter()
        .flat_map(|f| f.units.iter().map(|u| u.entity))
        .collect();

    let (mut held_frames, mut windows, mut spent, mut ran_out) = (0, 0, 0, 0);
    let mut disc_landings = 0;
    for &e in &entities {
        let mut window_start = None;
        for i in 1..seen.len() {
            let (Some(prev), Some(now)) = (unit(&seen[i - 1], e), unit(&seen[i], e)) else {
                continue;
            };
            // An absorb held since last frame wears its card, held and lit.
            if prev.absorb.is_some() && now.absorb.is_some() {
                held_frames += 1;
                assert!(
                    matches!(now.phase, Some(ShieldPhase::Birth | ShieldPhase::Loop)),
                    "frame {i}: {e:?} holds an absorb but its card is {:?}",
                    now.phase
                );
                assert_eq!(now.lit_layers, 2, "frame {i}: {e:?}'s card is not lit");
            }
            if prev.absorb.is_none() && now.absorb.is_some() {
                window_start = Some(i);
            }
            // An absorb that ended lets its card go: on the next frame it is
            // dying, and it never holds again without a new absorb.
            if prev.absorb.is_some() && now.absorb.is_none() {
                windows += 1;
                let lasted = window_start.map_or(0, |s| i - s);
                // Spent: it ended before its 6 s ran out.
                if (lasted as f32) < 5.5 * 60.0 {
                    spent += 1;
                } else {
                    ran_out += 1;
                }
                if let Some(next) = seen.get(i + 1).and_then(|f| unit(f, e)) {
                    if next.absorb.is_none() {
                        assert!(
                            matches!(next.phase, Some(ShieldPhase::Death) | None),
                            "frame {}: {e:?}'s absorb ended but its card is {:?}",
                            i + 1,
                            next.phase
                        );
                    }
                }
            }
            // The buff's landing plays the disc on the unit it shields.
            let new_disc = seen[i].discs.iter().filter(|b| **b == e).count()
                > seen[i - 1].discs.iter().filter(|b| **b == e).count();
            if new_disc {
                disc_landings += 1;
                assert!(now.armed, "frame {i}: a disc on {e:?}, which holds no buff");
            }
        }
    }
    println!(
        "{held_frames} held frames, {windows} absorbs ended ({spent} spent, {ran_out} ran out), \
         {disc_landings} discs"
    );
    // Not vacuous: the match raised and ended several absorbs, at least one
    // of them spent by the hits it soaked, and cast the buff more than once.
    assert!(held_frames > 60, "{held_frames} frames of a held absorb");
    assert!(windows >= 3, "{windows} absorbs ended");
    assert!(spent >= 1, "no absorb was spent ({ran_out} ran out)");
    assert!(disc_landings >= 2, "{disc_landings} discs landed");
}
