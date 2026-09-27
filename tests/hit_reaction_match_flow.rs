//! The victim hit reaction observed in a REAL match — Hunter v Warrior, and a
//! wand-user's match, Priest v Warrior — booted into `PlayMatch` through the
//! real `StatesPlugin` schedule the way `--replay` does, with every graphical
//! system registered where the client registers it.
//!
//! ## Why this exists
//!
//! `tests/hit_reaction_visual_probes.rs` drives the reaction systems in a
//! hand-assembled app — the right place for the geometry, and the reason a
//! flinch that was invisible in every real match passed there. In a match the
//! victim is almost always moving, and the dip used to be ADDED to the walk
//! bob: ±0.10 at a stride's cadence against a 0.10 dip over about the same
//! time. Measured here before the fix, the struck Warrior's body mostly ROSE
//! through its own "dip", and the kiting Hunter's did too under the Warrior's
//! blows. The animation sandbox stages a stationary victim, where the dip
//! reads cleanly, so the sandbox looked right and the match did not.
//!
//! This probe therefore asserts on the body as it is DRAWN — the `VisualBody`
//! child's local Y after every gait writer has run — on victims that are
//! moving when struck, and it covers the whole chain in between: the Shot's
//! swing, its arrow's arrival marker, and the flinch that arrival starts.
//!
//! The wand match pins the same chain for a wand bolt: every wand hit's
//! flinch starts on the frame its bolt ARRIVES, never at the damage — the
//! flinch used to lead its own bolt by up to a second at wand range.
//!
//! Runs with no window or GPU (same shape as `tests/replay_boot_icons.rs`).
//! Visual-only systems never touch the sim, so the match is the seeded one.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::components::{
    AutoAttackKind, AutoAttackSwing, Combatant, HitFlinch, Pet, RangedHitArrival, VisualBody,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::systems::CombatSystemPhase;
use arenasim::states::play_match::{
    cleanup_hit_flinch, consume_hit_reactions, consume_ranged_hit_arrivals, hit_flinch_weight,
    update_cosmetic_arrows, update_fear_run, update_sheep_hop, update_walk_animation,
    update_wand_missiles, AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};

/// One rendered frame at 60 fps — one sim tick, the client's own cadence.
const FRAME: Duration = Duration::from_micros(16_667);
/// Long enough for the seeded matches below to reach their end.
const MAX_FRAMES: usize = 60 * 90;
/// A victim that moved further than this in the frame it was struck was
/// walking, so its gait was bobbing under the flinch.
const MOVING_EPS: f32 = 0.01;

/// What the observers saw. Frames count from the first update.
#[derive(Resource, Default)]
struct Seen {
    frame: usize,
    /// `(frame, attacker, target, kind)` per landed auto.
    swings: Vec<(usize, Entity, Entity, AutoAttackKind)>,
    /// `(frame, target, kind)` per projectile arrival, read before its
    /// consumer.
    arrivals: Vec<(usize, Entity, AutoAttackKind)>,
    /// Per frame, per victim with a live flinch, as drawn.
    samples: Vec<Sample>,
    last_xz: Vec<(Entity, Vec2)>,
    /// The two players, recorded while the match lives — its entities are
    /// despawned when it ends.
    players: Vec<(Entity, CharacterClass)>,
}

#[derive(Clone, Copy)]
struct Sample {
    frame: usize,
    unit: Entity,
    elapsed: f32,
    depth: f32,
    weight: f32,
    /// The body's drawn local Y relative to its rest height.
    body_y: f32,
    /// How far the unit moved on the ground this frame.
    moved: f32,
}

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
    // The `--replay` entrance: straight into the match with the seed set.
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>();

    app.add_systems(
        FixedUpdate,
        observe_swings
            .after(CombatSystemPhase::CombatResolution)
            .before(consume_hit_reactions),
    )
    .add_systems(
        Update,
        observe_arrivals
            .after(update_cosmetic_arrows)
            .after(update_wand_missiles)
            .before(consume_ranged_hit_arrivals),
    )
    // After every gait writer, before the expired flinch is removed: the body
    // exactly as this frame draws it.
    .add_systems(
        Update,
        observe_bodies
            .after(update_walk_animation)
            .after(update_sheep_hop)
            .after(update_fear_run)
            .before(cleanup_hit_flinch),
    )
    .add_systems(Last, |mut seen: ResMut<Seen>| seen.frame += 1);
    app
}

fn observe_swings(mut seen: ResMut<Seen>, swings: Query<&AutoAttackSwing>) {
    let frame = seen.frame;
    for s in swings.iter() {
        seen.swings.push((frame, s.attacker, s.target, s.kind));
    }
}

fn observe_arrivals(mut seen: ResMut<Seen>, arrivals: Query<&RangedHitArrival>) {
    let frame = seen.frame;
    for a in arrivals.iter() {
        seen.arrivals.push((frame, a.target, a.kind));
    }
}

fn observe_bodies(
    mut seen: ResMut<Seen>,
    units: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&HitFlinch>,
        Option<&Pet>,
        &Children,
    )>,
    bodies: Query<(&Transform, &VisualBody)>,
) {
    let frame = seen.frame;
    for (unit, combatant, tf, flinch, pet, children) in units.iter() {
        if pet.is_none() && !seen.players.iter().any(|(e, _)| *e == unit) {
            seen.players.push((unit, combatant.class));
        }
        let xz = tf.translation.xz();
        let moved = match seen.last_xz.iter_mut().find(|(e, _)| *e == unit) {
            Some((_, last)) => {
                let d = last.distance(xz);
                *last = xz;
                d
            }
            None => {
                seen.last_xz.push((unit, xz));
                0.0
            }
        };
        let Some(flinch) = flinch else { continue };
        for child in children.iter() {
            if let Ok((body, vb)) = bodies.get(child) {
                seen.samples.push(Sample {
                    frame,
                    unit,
                    elapsed: flinch.elapsed,
                    depth: flinch.depth,
                    weight: hit_flinch_weight(flinch),
                    body_y: body.translation.y - vb.rest_y,
                    moved,
                });
            }
        }
    }
}

struct Match {
    seen: Seen,
    hunter: Entity,
    warrior: Entity,
}

/// Play the seeded match `cfg` to its end and hand back what was observed.
fn play_seen(cfg: &str) -> Seen {
    let mut app = boot(cfg);
    for _ in 0..MAX_FRAMES {
        app.update();
        if app.world().resource::<State<GameState>>().get() != &GameState::PlayMatch {
            break;
        }
    }
    std::mem::take(&mut *app.world_mut().resource_mut::<Seen>())
}

fn player(seen: &Seen, class: CharacterClass) -> Entity {
    seen.players
        .iter()
        .find(|(_, c)| *c == class)
        .map(|(e, _)| *e)
        .unwrap_or_else(|| panic!("no {class:?} was observed"))
}

fn play() -> Match {
    let seen = play_seen(r#"{"team1":["Hunter"],"team2":["Warrior"],"random_seed":7}"#);
    let (hunter, warrior) = (
        player(&seen, CharacterClass::Hunter),
        player(&seen, CharacterClass::Warrior),
    );
    Match {
        seen,
        hunter,
        warrior,
    }
}

/// The flinch windows that START on `frame` for `unit`: the first sample of a
/// fresh (or refreshed) flinch, and every sample until it next restarts.
fn window_from(seen: &Seen, unit: Entity, frame: usize) -> Vec<Sample> {
    let mut out: Vec<Sample> = Vec::new();
    for s in seen
        .samples
        .iter()
        .filter(|s| s.unit == unit && s.frame >= frame)
    {
        if s.frame != frame + out.len() {
            break;
        }
        if let Some(prev) = out.last() {
            if s.elapsed < prev.elapsed {
                break;
            }
        }
        out.push(*s);
    }
    out
}

/// The body must visibly compress on a victim that is walking when struck:
/// at the top of the envelope it sits at the bottom of the dip, whatever phase
/// its stride was in, and it never rises above rest while the flinch holds
/// most of the gait. Added to the bob instead, it rose to +0.018 at 80% weight
/// in this very match.
fn assert_visible_on_the_move(label: &str, window: &[Sample]) {
    let peak = window
        .iter()
        .max_by(|a, b| a.weight.total_cmp(&b.weight))
        .unwrap();
    assert!(
        peak.body_y <= -0.75 * peak.depth,
        "{label}: at the flinch's peak (weight {:.2}) the body sits at {:+.3}, \
         not at the bottom of a {:.2} dip — the stride is drowning it out",
        peak.weight,
        peak.body_y,
        peak.depth
    );
    for s in window.iter().filter(|s| s.weight >= 0.5) {
        assert!(
            s.body_y <= 0.0,
            "{label}: the struck body ROSE to {:+.3} with the flinch at weight \
             {:.2} (frame {})",
            s.body_y,
            s.weight,
            s.frame
        );
    }
}

#[test]
fn the_flinch_reads_on_moving_victims_in_a_real_match() {
    let m = play();
    let seen = &m.seen;

    // --- The Shot's chain, end to end, in the real schedule -----------------
    let shots: Vec<_> = seen
        .swings
        .iter()
        .filter(|s| s.1 == m.hunter && s.3 == AutoAttackKind::Shot)
        .collect();
    assert!(
        shots.len() >= 3,
        "vacuous: only {} Auto Shots landed in the seeded match",
        shots.len()
    );
    let arrivals: Vec<_> = seen
        .arrivals
        .iter()
        .filter(|a| a.1 == m.warrior && a.2 == AutoAttackKind::Shot)
        .collect();
    assert_eq!(
        arrivals.len(),
        shots.len(),
        "every landed Shot must reach its victim exactly once"
    );
    for (shot, arrival) in shots.iter().zip(&arrivals) {
        assert!(
            arrival.0 > shot.0,
            "an arrow arrives after it is loosed, not with the damage"
        );
        let window = window_from(seen, m.warrior, arrival.0);
        assert!(
            !window.is_empty() && window[0].elapsed <= FRAME.as_secs_f32() + 1e-4,
            "the arrival at frame {} started no flinch on the Warrior that frame",
            arrival.0
        );
    }

    // --- Visibility on the move: the Warrior under arrows -------------------
    let mut moving_shot_windows = 0;
    for arrival in &arrivals {
        let window = window_from(seen, m.warrior, arrival.0);
        if window[0].moved > MOVING_EPS {
            moving_shot_windows += 1;
            assert_visible_on_the_move("Shot on the Warrior", &window);
        }
    }
    assert!(
        moving_shot_windows >= 3,
        "vacuous: only {moving_shot_windows} arrows struck a MOVING Warrior"
    );

    // --- ...and the same flinch under melee: the Warrior's blows on the
    // Hunter, and the pet's on the Warrior. Nothing about this was
    // ranged-specific; it is the flinch itself on anything that walks.
    let mut moving_melee_on_hunter = 0;
    let mut moving_melee_on_warrior = 0;
    for &(frame, _, target, kind) in &seen.swings {
        if kind != AutoAttackKind::Melee {
            continue;
        }
        // A melee reaction starts in the sim tick, so it is drawn this frame.
        let window = window_from(seen, target, frame);
        if window.is_empty() || window[0].elapsed > FRAME.as_secs_f32() + 1e-4 {
            continue; // refreshed a flinch another hit started this frame
        }
        if window[0].moved <= MOVING_EPS {
            continue;
        }
        if target == m.hunter {
            moving_melee_on_hunter += 1;
            assert_visible_on_the_move("Warrior melee on the Hunter", &window);
        } else if target == m.warrior {
            moving_melee_on_warrior += 1;
            assert_visible_on_the_move("pet melee on the Warrior", &window);
        }
    }
    println!(
        "{} Shots / {} arrivals; moving victims: {moving_shot_windows} Shot, \
         {moving_melee_on_hunter} Warrior-on-Hunter, {moving_melee_on_warrior} pet-on-Warrior",
        shots.len(),
        arrivals.len()
    );
    assert!(
        moving_melee_on_hunter >= 1,
        "vacuous: the Warrior never struck a moving Hunter"
    );
    assert!(
        moving_melee_on_warrior >= 3,
        "vacuous: only {moving_melee_on_warrior} pet blows on a moving Warrior"
    );
}

/// The wand-user's match: every wand hit's flinch starts on the frame its bolt
/// arrives — never at the damage, which is the swing's frame and up to a
/// second of flight earlier at wand range.
///
/// Priest v Warrior, because a Warrior takes no other auto-attack damage from
/// a lone Priest: every flinch that starts on it is a wand hit's, so "no
/// flinch before its bolt" can be asserted over EVERY flinch it shows, not
/// only the ones a probe chose to look at.
#[test]
fn a_wand_hit_flinches_when_its_bolt_arrives_in_a_real_match() {
    let seen = play_seen(r#"{"team1":["Priest"],"team2":["Warrior"],"random_seed":1}"#);
    let (priest, warrior) = (
        player(&seen, CharacterClass::Priest),
        player(&seen, CharacterClass::Warrior),
    );

    let wands: Vec<_> = seen
        .swings
        .iter()
        .filter(|s| s.1 == priest && s.3 == AutoAttackKind::Wand)
        .collect();
    assert!(
        wands.len() >= 5,
        "vacuous: only {} wand hits landed in the seeded match",
        wands.len()
    );
    assert!(
        wands.iter().all(|s| s.2 == warrior),
        "a wand hit landed on something other than the Warrior"
    );
    let arrivals: Vec<_> = seen
        .arrivals
        .iter()
        .filter(|a| a.2 == AutoAttackKind::Wand)
        .collect();
    assert_eq!(
        arrivals.len(),
        wands.len(),
        "every landed wand hit must reach its victim exactly once"
    );

    // Each bolt arrives after its hit landed, and its arrival starts the
    // Warrior's flinch that very frame.
    let mut lags = Vec::new();
    for (hit, arrival) in wands.iter().zip(&arrivals) {
        assert_eq!(arrival.1, warrior);
        assert!(
            arrival.0 > hit.0,
            "a bolt arrives after it is thrown, not with the damage"
        );
        lags.push(arrival.0 - hit.0);
        let window = window_from(&seen, warrior, arrival.0);
        assert!(
            !window.is_empty() && window[0].elapsed <= FRAME.as_secs_f32() + 1e-4,
            "the bolt arriving at frame {} started no flinch on the Warrior that frame",
            arrival.0
        );
    }

    // ...and NOTHING starts a flinch on the Warrior at any other frame — in
    // particular not at a wand hit's damage frame, ahead of its bolt.
    let arrival_frames: Vec<usize> = arrivals.iter().map(|a| a.0).collect();
    let starts: Vec<usize> = seen
        .samples
        .iter()
        .filter(|s| s.unit == warrior && s.elapsed <= FRAME.as_secs_f32() + 1e-4)
        .map(|s| s.frame)
        .collect();
    for frame in &starts {
        assert!(
            arrival_frames.contains(frame),
            "a flinch started on the Warrior at frame {frame}, where no bolt arrived"
        );
    }
    assert_eq!(starts.len(), wands.len(), "one flinch per wand hit");

    // Non-vacuity of "held": every bolt spends frames in the air, even the
    // point-blank ones thrown with the Warrior on top of the Priest, and the
    // match includes the defect's worst case — a bolt thrown from across the
    // arena, whose damage-timed flinch would have led it by half a second or
    // more.
    println!(
        "{} wand hits / {} arrivals; flight lag in frames: {lags:?}",
        wands.len(),
        arrivals.len(),
    );
    assert!(
        lags.iter().all(|&l| l >= 2),
        "a bolt arrived within a frame of its damage — nothing was held: {lags:?}"
    );
    assert!(
        lags.iter().any(|&l| l >= 30),
        "vacuous: no bolt in the match flew for half a second or more: {lags:?}"
    );
}
