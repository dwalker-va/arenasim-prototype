//! The victory choreography, observed in world positions: winners who finish
//! a match far apart run together and bounce side by side; a lone winner
//! bounces where it stands.
//!
//! Once a match is decided the sim is frozen, so nothing moves the winners
//! except `play_match::celebration`. The convergence used to be incidental —
//! targetless winners fell into the all-stealth fallback in `move_to_target`
//! and walked to within 5yd of the arena centre, so they only met when the
//! centre happened to lie between them. It is now a deliberate
//! choreography, and these probes pin what a player sees: the distance
//! between winners, and the bounce of their bodies.
//!
//! Booted through the real graphical `StatesPlugin` schedule with no GPU, the
//! `--replay` entrance straight into `PlayMatch` (same shape as
//! `tests/frame_rate_determinism.rs`).

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{
    Celebrating, Combatant, Pet, VictoryCelebration, VisualBody,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::map_geometry::MOVER_RADIUS;
use arenasim::states::play_match::{
    AbilityConfigPlugin, CelebrationMarch, GameRng, MapConfigPlugin, MovementConfigPlugin,
    CONVERGE_SPACING, PET_HEEL_DISTANCE,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::HeadlessMatchConfig;

const FRAME: Duration = Duration::from_micros(16_667);
const MAX_SIM_SECS: f32 = 330.0;

fn boot(cfg_json: &str) -> App {
    let cfg: HeadlessMatchConfig = serde_json::from_str(cfg_json).unwrap();
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
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>()
        .add_systems(FixedPostUpdate, watch_tick)
        .add_systems(Last, watch_frame);
    app
}

/// What one celebration looked like.
#[derive(Resource, Default)]
struct Seen {
    /// Each winner's (slot, position) on the first tick they are marked
    /// winners (by when the choreography has taken at most one step)...
    at_decision: Vec<(u8, Vec3)>,
    /// ...and on the last tick before Results.
    at_end: Vec<(u8, Vec3)>,
    /// Each winning pet's distance from its owner on the first and last
    /// celebration ticks.
    pet_to_owner_at_decision: Vec<f32>,
    pet_to_owner_at_end: Vec<f32>,
    /// The highest any winner's body rose above its rest height.
    max_bounce: f32,
    /// Each winner's planned spot, and whether it had settled, on the last
    /// tick.
    marches_at_end: Vec<(u8, CelebrationMarch)>,
}

fn winners(
    q: &Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&Pet>,
        Has<Celebrating>,
    )>,
) -> Vec<(u8, Vec3)> {
    let mut w: Vec<(u8, Vec3)> = q
        .iter()
        .filter(|(.., celebrating)| *celebrating)
        .map(|(_, c, t, ..)| (c.slot, t.translation))
        .collect();
    w.sort_by_key(|(slot, _)| *slot);
    w
}

fn watch_tick(
    mut seen: ResMut<Seen>,
    state: Res<State<GameState>>,
    celebration: Option<Res<VictoryCelebration>>,
    q: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&Pet>,
        Has<Celebrating>,
    )>,
    marches: Query<(&Combatant, &CelebrationMarch)>,
) {
    if celebration.is_none() || *state.get() != GameState::PlayMatch {
        return;
    }
    let now = winners(&q);
    if now.is_empty() {
        return; // `Celebrating` lands a command-flush after the decision.
    }
    let first = seen.at_decision.is_empty();
    if first {
        seen.at_decision = now.clone();
    }
    seen.at_end = now;
    let pets: Vec<f32> = q
        .iter()
        .filter_map(|(_, c, t, pet, _)| {
            let pet = pet?;
            let (_, _, owner, _, celebrating) = q.get(pet.owner).ok()?;
            (celebrating && c.is_alive()).then(|| {
                Vec2::new(t.translation.x, t.translation.z)
                    .distance(Vec2::new(owner.translation.x, owner.translation.z))
            })
        })
        .collect();
    if first {
        seen.pet_to_owner_at_decision = pets.clone();
    }
    seen.pet_to_owner_at_end = pets;
    let mut marches: Vec<(u8, CelebrationMarch)> =
        marches.iter().map(|(c, m)| (c.slot, *m)).collect();
    marches.sort_by_key(|(slot, _)| *slot);
    seen.marches_at_end = marches;
}

fn watch_frame(
    mut seen: ResMut<Seen>,
    bodies: Query<(&ChildOf, &Transform, &VisualBody)>,
    celebrating: Query<(), With<Celebrating>>,
) {
    for (parent, transform, body) in &bodies {
        if celebrating.get(parent.parent()).is_ok() {
            seen.max_bounce = seen.max_bounce.max(transform.translation.y - body.rest_y);
        }
    }
}

fn run(cfg: &str) -> Seen {
    let mut app = boot(cfg);
    let frames = (MAX_SIM_SECS / FRAME.as_secs_f32()) as usize;
    for _ in 0..frames {
        app.update();
        if *app.world().resource::<State<GameState>>().get() == GameState::Results {
            return std::mem::take(&mut *app.world_mut().resource_mut::<Seen>());
        }
    }
    panic!("{cfg} never reached Results");
}

fn apart(w: &[(u8, Vec3)]) -> f32 {
    Vec2::new(w[0].1.x, w[0].1.z).distance(Vec2::new(w[1].1.x, w[1].1.z))
}

/// Two winners who finished the match far apart. Mage and Paladin win on
/// TwinPillars a pillar's width apart.
const FAR_APART: &str = r#"{"team1":["Hunter","Rogue"],"team2":["Mage","Paladin"],"map":"TwinPillars","random_seed":5}"#;

/// One winner left standing.
const LONE_WINNER: &str =
    r#"{"team1":["Warrior"],"team2":["Hunter"],"map":"BasicArena","random_seed":11}"#;

#[test]
fn winners_far_apart_run_together_and_bounce() {
    let seen = run(FAR_APART);
    assert_eq!(
        seen.at_decision.len(),
        2,
        "two winners: {:?}",
        seen.at_decision
    );
    let before = apart(&seen.at_decision);
    let after = apart(&seen.at_end);
    eprintln!(
        "winners {before:.2}yd -> {after:.2}yd apart, bounce {:.2}: {:?} -> {:?}",
        seen.max_bounce, seen.at_decision, seen.at_end
    );
    // Non-vacuity: they really did finish far apart.
    assert!(before > 10.0, "winners finished only {before:.1}yd apart");
    // They meet side by side, CONVERGE_SPACING apart.
    assert!(
        (after - CONVERGE_SPACING).abs() < 0.05,
        "winners {before:.1}yd apart at the decision ended {after:.1}yd apart"
    );
    assert!(seen.max_bounce > 0.3, "no bounce: {}", seen.max_bounce);
}

#[test]
fn a_lone_winner_bounces_where_it_stands() {
    let seen = run(LONE_WINNER);
    assert_eq!(seen.at_decision.len(), 1, "{:?}", seen.at_decision);
    let (start, end) = (seen.at_decision[0].1, seen.at_end[0].1);
    eprintln!(
        "lone winner {start} -> {end}, bounce {:.2}, pet {:?} -> {:?}",
        seen.max_bounce, seen.pet_to_owner_at_decision, seen.pet_to_owner_at_end
    );
    assert_eq!(start, end, "the lone winner moved");
    assert!(seen.max_bounce > 0.3, "no bounce: {}", seen.max_bounce);
    // The Hunter's pet survived with it, finished the match across the arena,
    // and comes back to heel at its side.
    assert_eq!(seen.pet_to_owner_at_end.len(), 1);
    assert!(seen.pet_to_owner_at_decision[0] > 2.0 * PET_HEEL_DISTANCE);
    assert!(
        seen.pet_to_owner_at_end[0] <= PET_HEEL_DISTANCE + 0.01,
        "pet ended {:.2}yd from its owner",
        seen.pet_to_owner_at_end[0]
    );
}

/// Three winners who finish the match on the same side of the arena would, if
/// each simply stopped on the side it came from, share a spot. Every winner
/// gets a distinct spot, reaches it, and stands clear of the others.
fn assert_three_stand_apart(cfg: &str) -> Seen {
    let seen = run(cfg);
    assert_eq!(seen.at_decision.len(), 3, "{cfg}: {:?}", seen.at_decision);
    assert_eq!(seen.marches_at_end.len(), 3);
    for (slot, march) in &seen.marches_at_end {
        let (_, at) = seen.at_end.iter().find(|(s, _)| s == slot).unwrap();
        let off = Vec2::new(at.x - march.goal.x, at.z - march.goal.z).length();
        assert!(
            march.settled && off < 0.05,
            "{cfg}: slot {slot} ended {off:.2}yd from its spot (settled {})",
            march.settled
        );
    }
    for (i, (_, a)) in seen.at_end.iter().enumerate() {
        for (_, b) in &seen.at_end[i + 1..] {
            let d = Vec2::new(a.x - b.x, a.z - b.z).length();
            eprintln!("{cfg}: pair {d:.2}yd apart");
            assert!(d >= 2.0 * MOVER_RADIUS, "{cfg}: two winners {d:.2}yd apart");
        }
    }
    seen
}

#[test]
fn three_winners_each_reach_their_own_spot() {
    // Found with the Rogue stopping 0.22yd from the Priest...
    assert_three_stand_apart(
        r#"{"team1":["Rogue","Priest","Paladin"],"team2":["Hunter","Shaman","Warlock"],"map":"BasicArena","random_seed":1}"#,
    );
    // ...and, on the pillared Nagrand map, 0.67yd.
    assert_three_stand_apart(
        r#"{"team1":["Hunter","Mage","Priest"],"team2":["Warrior","Rogue","Shaman"],"map":"PillaredArena","random_seed":4}"#,
    );
}

/// A 3v3 on TwinPillars whose winners' ring slot lands inside a pillar: the
/// slot moves to clear ground, and every winner still reaches its spot.
#[test]
fn a_ring_slot_inside_a_pillar_moves_in_a_real_match() {
    let seen = assert_three_stand_apart(
        r#"{"team1":["Warrior","Mage","Priest"],"team2":["Warlock","Paladin","Rogue"],"map":"TwinPillars","random_seed":4}"#,
    );
    // An unmoved slot sits exactly on the ring round the meeting point
    // (`face`); a moved one does not.
    let ring = CONVERGE_SPACING / (2.0 * (std::f32::consts::PI / 3.0).sin());
    let moved = seen
        .marches_at_end
        .iter()
        .filter(|(_, m)| {
            let off = Vec2::new(m.goal.x - m.face.x, m.goal.z - m.face.z).length();
            (off - ring).abs() > 0.01
        })
        .count();
    eprintln!("{moved} of 3 slots moved off the ring");
    assert!(
        moved >= 1,
        "no slot needed moving: {:?}",
        seen.marches_at_end
    );
}

/// Seed scanner for the probes above: prints each candidate's winners and how
/// far apart they finished. `cargo test --release --test victory_choreography
/// -- --ignored --nocapture`
#[test]
#[ignore]
fn scan_for_far_apart_winners() {
    let classes = [
        "Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman",
    ];
    for seed in 1..=6u64 {
        for (i, a) in classes.iter().enumerate() {
            let b = classes[(i + 3) % classes.len()];
            let c = classes[(i + 5) % classes.len()];
            let d = classes[(i + 6) % classes.len()];
            let cfg = format!(
                r#"{{"team1":["{a}","{b}"],"team2":["{c}","{d}"],"map":"TwinPillars","random_seed":{seed}}}"#
            );
            let seen = run(&cfg);
            if seen.at_decision.len() == 2 {
                println!(
                    "{cfg}: {:.1}yd -> {:.1}yd, bounce {:.2}",
                    apart(&seen.at_decision),
                    apart(&seen.at_end),
                    seen.max_bounce
                );
            }
        }
    }
}
