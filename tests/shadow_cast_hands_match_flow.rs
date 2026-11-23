//! The Priest's Shadow cast hands observed in a REAL match (AS-449): every
//! Shadow Word: Pain, Vampiric Embrace and Silence lights its Priest's two
//! spell hands exactly ONCE, every Mind Blast cast loops them from its start,
//! and an interrupted Mind Blast's hands are gone on the frame of the
//! interrupt.
//!
//! The three instants have no cast state, so their hands come off their
//! `InstantAbilityFired`; Mind Blast's come off its `CastingState` and end
//! off its `CastEnding`. Which schedule reads which signal, and in what order
//! against the systems that retire it, is a property of `states/mod.rs`'s
//! registration, so this boots `PlayMatch` through the real `StatesPlugin`
//! the way `--replay` does (`shield_cast_hands_match_flow.rs`'s shape) and
//! counts, per cast, the hand rigs its caster grew.
//!
//! Runs with no window or GPU. Visual-only systems never touch the sim, so
//! the matches are the seeded ones, but for one staged fact: team 1's
//! Shadowform Priest is topped up to full health before every frame, so it
//! lives to cast all match. (A Shadowform Priest's death tripped a crash in
//! `shadowform.rs`'s shroud that predates these hands, AS-507.)

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::{
    CastEnding, CastEndingKind, CastingState, Combatant, HealCastHand, HealCastKind, HealCastPhase,
    InstantAbilityFired, LandedCast, PlayMatchEntity,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::systems::CombatSystemPhase;
use arenasim::states::play_match::{
    consume_cast_ending_signals, consume_heal_cast_endings, consume_instant_ability_signals,
    AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};

/// One rendered frame at 60 fps — one sim tick, the client's own cadence.
const FRAME: Duration = Duration::from_micros(16_667);
/// Long enough for the seeded matches below to reach their end.
const MAX_FRAMES: usize = 60 * 90;
/// A cast's hands are spawned on its own frame; this many frames after it is
/// generous, and far shorter than any Priest's next Shadow cast.
const HANDS_WITHIN: usize = 2;

const INSTANTS: [AbilityType; 3] = [
    AbilityType::ShadowWordPain,
    AbilityType::VampiricEmbrace,
    AbilityType::Silence,
];

#[derive(Resource, Default)]
struct Seen {
    frame: usize,
    /// `(frame, caster, ability)` per Shadow instant, read off its marker.
    instants: Vec<(usize, Entity, AbilityType)>,
    /// `(frame, ability)` per Shadow instant marker spawned without
    /// `PlayMatchEntity`, which headless (where nothing consumes it) needs
    /// for the match-end sweep to reclaim it.
    untagged: Vec<(usize, AbilityType)>,
    /// `(frame, caster)` per Mind Blast cast begun.
    blasts: Vec<(usize, Entity)>,
    /// `(frame, caster, kind)` per Mind Blast cast ended this frame:
    /// `Landed` from its `LandedCast`, the rest read off a live loop.
    endings: Vec<(usize, Entity, CastEndingKind)>,
    /// `(frame, caster, phase is loop)` per Shadow hand rig spawned.
    rigs: Vec<(usize, Entity, bool)>,
    /// Endings whose loop was still drawn at the end of their frame.
    survivors: Vec<(usize, Entity, CastEndingKind)>,
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
    app.insert_resource(cfg.to_match_config().unwrap())
        .insert_resource(cfg.ai_profiles().unwrap())
        .insert_resource(GameRng::from_seed(cfg.random_seed.unwrap()))
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>();

    // The marker lives from the Priest AI's cast to
    // `consume_instant_ability_signals` in the same tick; the ending from the
    // cast's resolution to `consume_cast_ending_signals`.
    app.add_systems(
        FixedUpdate,
        (
            observe_fired,
            observe_endings.before(consume_heal_cast_endings),
        )
            .after(CombatSystemPhase::CombatResolution)
            .before(consume_instant_ability_signals)
            .before(consume_cast_ending_signals),
    )
    .add_systems(PreUpdate, top_up_shadow_priest)
    .add_systems(Last, (observe_blasts, observe_rigs, observe_survivors))
    .add_systems(
        Last,
        (|mut seen: ResMut<Seen>| seen.frame += 1)
            .after(observe_rigs)
            .after(observe_survivors),
    );
    app
}

/// The staged fact: team 1's Priest starts every frame at full health.
fn top_up_shadow_priest(mut combatants: Query<&mut Combatant>) {
    for mut c in combatants.iter_mut() {
        if c.team == 1 && c.class == CharacterClass::Priest && c.is_alive() {
            c.current_health = c.max_health;
        }
    }
}

fn observe_fired(
    mut seen: ResMut<Seen>,
    fired: Query<(&InstantAbilityFired, Has<PlayMatchEntity>)>,
) {
    let frame = seen.frame;
    for (f, tagged) in fired.iter() {
        if INSTANTS.contains(&f.ability) {
            seen.instants.push((frame, f.caster, f.ability));
            if !tagged {
                seen.untagged.push((frame, f.ability));
            }
        }
    }
}

/// A Mind Blast ends: a landing names itself; an interrupt or fizzle is
/// known as Mind Blast's by the loop it ends.
fn observe_endings(
    mut seen: ResMut<Seen>,
    endings: Query<(&CastEnding, Option<&LandedCast>)>,
    rigs: Query<&HealCastHand>,
) {
    let frame = seen.frame;
    for (ending, landed) in endings.iter() {
        let blast = match landed {
            Some(cast) => cast.ability == AbilityType::MindBlast,
            None => rigs.iter().any(|r| {
                r.caster == ending.caster
                    && r.kind == HealCastKind::Shadow
                    && r.phase == HealCastPhase::Loop
            }),
        };
        if blast {
            seen.endings.push((frame, ending.caster, ending.kind));
        }
    }
}

fn observe_blasts(
    mut seen: ResMut<Seen>,
    casts: Query<(Entity, &CastingState), Added<CastingState>>,
) {
    let frame = seen.frame;
    for (caster, cast) in casts.iter() {
        if cast.ability == AbilityType::MindBlast && !cast.interrupted {
            seen.blasts.push((frame, caster));
        }
    }
}

fn observe_rigs(mut seen: ResMut<Seen>, rigs: Query<&HealCastHand, Added<HealCastHand>>) {
    let frame = seen.frame;
    for rig in rigs.iter() {
        if rig.kind == HealCastKind::Shadow {
            seen.rigs
                .push((frame, rig.caster, rig.phase == HealCastPhase::Loop));
        }
    }
}

/// At the end of every frame an ending was seen in: a landed Mind Blast's
/// hands play its cast kit, and any other ending's are gone.
fn observe_survivors(mut seen: ResMut<Seen>, rigs: Query<&HealCastHand>) {
    let frame = seen.frame;
    let ended: Vec<_> = seen
        .endings
        .iter()
        .filter(|e| e.0 == frame)
        .cloned()
        .collect();
    for (f, caster, kind) in ended {
        let looping = rigs.iter().any(|r| {
            r.caster == caster && r.kind == HealCastKind::Shadow && r.phase == HealCastPhase::Loop
        });
        let playing = rigs
            .iter()
            .filter(|r| r.caster == caster && r.kind == HealCastKind::Shadow)
            .count();
        let wrong = match kind {
            CastEndingKind::Landed => looping || playing < 2,
            _ => looping,
        };
        if wrong {
            seen.survivors.push((f, caster, kind));
        }
    }
}

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

/// A Shadowform Priest (Vampiric Embrace, Silence) and a Warrior against a
/// Discipline Priest and a Rogue on TwinPillars, under the team AI: both
/// Priests Mind Blast, the Shadow one casts all three instants, and the
/// Warrior and Rogue interrupt.
const BANK: [u64; 2] = [1, 2];

fn config(seed: u64) -> String {
    format!(
        r#"{{"team1":["Priest","Warrior"],"team2":["Priest","Rogue"],
            "team1_builds":[{{"ImprovedMindBlast":2,"ImprovedPsychicScream":1,"Shadowform":1,
                              "VampiricEmbrace":1,"Silence":1}},{{}}],
            "team2_builds":[{{"UnbreakableWill":2,"ImprovedPowerWordShield":1,"InnerFocus":1,
                              "ReflectiveShield":1,"PainSuppression":1,"HolySpecialization":2}},{{}}],
            "team1_priest_forms":["Shadowform","NoForm"],
            "ai_profile":"TeamPlan","map":"TwinPillars","random_seed":{seed}}}"#
    )
}

#[test]
fn every_shadow_cast_plays_its_hands_once_in_a_real_match() {
    let mut instants = [0usize; INSTANTS.len()];
    let (mut blasts, mut landed, mut cut) = (0, 0, 0);
    for seed in BANK {
        let seen = play_seen(&config(seed));

        // Every marker the Priest AI spawns is swept with the match.
        assert!(
            seen.untagged.is_empty(),
            "seed {seed}: Shadow markers spawned without PlayMatchEntity: {:?}",
            seen.untagged
        );
        // Each instant: two hands, launched, on its own frame or the next.
        for &(frame, caster, ability) in &seen.instants {
            let rigs = seen
                .rigs
                .iter()
                .filter(|(f, c, looping)| {
                    *c == caster && !looping && (frame..=frame + HANDS_WITHIN).contains(f)
                })
                .count();
            assert_eq!(
                rigs, 2,
                "seed {seed}: the {ability:?} at frame {frame} grew {rigs} hand rigs"
            );
        }
        // Each Mind Blast: two hands, looping from its start.
        for &(frame, caster) in &seen.blasts {
            let rigs = seen
                .rigs
                .iter()
                .filter(|(f, c, looping)| {
                    *c == caster && *looping && (frame..=frame + HANDS_WITHIN).contains(f)
                })
                .count();
            assert_eq!(
                rigs, 2,
                "seed {seed}: the Mind Blast begun at frame {frame} grew {rigs} loop rigs"
            );
        }
        // ...and nothing else: every rig belongs to one of those casts.
        let launched = seen.rigs.iter().filter(|r| !r.2).count();
        let looped = seen.rigs.iter().filter(|r| r.2).count();
        assert_eq!(
            launched,
            2 * seen.instants.len(),
            "seed {seed}: launch rigs"
        );
        assert_eq!(looped, 2 * seen.blasts.len(), "seed {seed}: loop rigs");
        // A landing turns the loop into the cast kit; anything else ends it.
        assert!(
            seen.survivors.is_empty(),
            "seed {seed}: endings whose hands were wrong at the end of their frame: {:?}",
            seen.survivors
        );

        for (i, ability) in INSTANTS.iter().enumerate() {
            instants[i] += seen.instants.iter().filter(|c| c.2 == *ability).count();
        }
        blasts += seen.blasts.len();
        landed += seen
            .endings
            .iter()
            .filter(|e| e.2 == CastEndingKind::Landed)
            .count();
        cut += seen
            .endings
            .iter()
            .filter(|e| e.2 == CastEndingKind::Interrupted)
            .count();
        println!(
            "seed {seed}: {} instants, {} Mind Blasts, endings {:?}",
            seen.instants.len(),
            seen.blasts.len(),
            seen.endings.iter().map(|e| e.2).collect::<Vec<_>>()
        );
    }
    for (ability, n) in INSTANTS.iter().zip(instants) {
        println!("{ability:?}: {n} casts over the bank");
        assert!(n > 0, "vacuous: no {ability:?} over the bank");
    }
    println!("Mind Blast: {blasts} begun, {landed} landed, {cut} interrupted");
    assert!(blasts > 0 && landed > 0, "vacuous: no Mind Blast landed");
    assert!(
        cut > 0,
        "vacuous: no Mind Blast was interrupted over the bank"
    );
}
