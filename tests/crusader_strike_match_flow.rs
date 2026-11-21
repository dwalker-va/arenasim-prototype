//! Crusader Strike's visual in a REAL match (AS-379): every strike the
//! Retribution Paladin makes swings Mortal Strike's stroke and lays the red
//! trail on the frame it is made; a landed one lands `strike_impact_chest` on
//! its victim, and a dodged one lands nothing.
//!
//! ## Why this exists
//!
//! The sim spawns no `InstantAbilityFired` for an avoided strike, so the
//! dodged stroke is read off the sim's own `MeleeAvoided`, which lives only
//! from the class AI (Phase 2) to `resolve_melee_avoidances` (Phase 3). The
//! bare-app probes in `crusader_strike_visual_probes.rs` hand their systems
//! that marker directly; only the real `StatesPlugin` schedule can show that
//! `note_avoided_strikes` sits inside that window. So this boots `PlayMatch`
//! the way `--replay` does (the `shield_cast_hands_match_flow.rs` shape).
//!
//! Runs with no window or GPU. Visual-only systems never touch the sim, so
//! the match is the seeded one, but for the staged facts of two arms: in the
//! dodge arm, on the frame the gates open the Rogue is given a certain dodge,
//! so every strike that meets it standing is dodged (one it takes stunned
//! still lands); in the same-tick arm, every dodged strike's tick also lands
//! the Paladin's auto-attack, so the two strokes contend for one socket.

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin as BevyStatesPlugin;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::CombatPlugin;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::combat_core::MeleeAvoided;
use arenasim::states::play_match::components::{
    AutoAttackSwing, Combatant, InstantAbilityFired, MatchCountdown, SchoolImpact, SwingStyle,
    WeaponHand, WeaponSocket,
};
use arenasim::states::play_match::equipment::EquipmentPlugin;
use arenasim::states::play_match::systems::CombatSystemPhase;
use arenasim::states::play_match::{
    consume_swing_signals, AbilityConfigPlugin, GameRng, MapConfigPlugin, MovementConfigPlugin,
    WeaponTrail,
};
use arenasim::states::{GameState, StatesPlugin};
use arenasim::{CharacterClass, HeadlessMatchConfig};

/// One rendered frame at 60 fps — one sim tick, the client's own cadence.
const FRAME: Duration = Duration::from_micros(16_667);
/// Long enough for the strikes below; the match need not end.
const FRAMES: usize = 60 * 45;

/// A Retribution Paladin (Crusader Strike, AS-422's deep build) against a
/// Rogue, on BasicArena: no one else on the field lays a weapon trail.
const CONFIG: &str = r#"{"team1":["Paladin"],"team2":["Rogue"],
    "team1_builds":[{"Conviction":2,"TwoHandedWeaponSpecialization":1,"Vengeance":2,
                     "TheArtOfWar":1,"Repentance":1,"CrusaderStrike":1}],
    "random_seed":7}"#;

#[derive(Resource, Default)]
struct Seen {
    frame: usize,
    /// Frames on which a Crusader Strike was dodged or parried.
    avoided: Vec<usize>,
    /// Frames on which a Crusader Strike landed (the drain's marker).
    landed: Vec<usize>,
    /// Frames on which a weapon trail was laid.
    trails: Vec<usize>,
    /// Frames on which a Crusader Strike landing was spawned.
    impacts: Vec<usize>,
    /// Frames on which the Paladin's main hand was swinging Mortal Strike's
    /// stroke from the top.
    strokes: Vec<usize>,
    /// Frames on which the Paladin's auto-attack landed.
    autos: Vec<usize>,
}

#[derive(Resource)]
struct DodgeEverything;

/// The same-tick arm: on the tick a strike is dodged, the Paladin's next
/// auto-attack is due and the Rogue does not dodge it.
#[derive(Resource)]
struct AutoOnTheStrikesTick;

fn boot(dodge: bool, same_tick_auto: bool) -> App {
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
        .insert_state(GameState::PlayMatch)
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Seen>();
    if dodge {
        app.insert_resource(DodgeEverything);
    }
    if same_tick_auto {
        app.insert_resource(AutoOnTheStrikesTick);
    }
    // Both markers live only inside this window: `MeleeAvoided` from the
    // class AI to Phase 3, the drain's marker from Phase 2 to its consumer.
    app.add_systems(PreUpdate, stage_dodge)
        .add_systems(
            FixedUpdate,
            observe_strikes
                .after(CombatSystemPhase::CombatAndMovement)
                .before(CombatSystemPhase::CombatResolution),
        )
        .add_systems(
            FixedUpdate,
            auto_on_the_strikes_tick
                .after(observe_strikes)
                .before(CombatSystemPhase::CombatResolution),
        )
        .add_systems(
            FixedUpdate,
            (observe_autos, restore_dodge)
                .after(CombatSystemPhase::CombatResolution)
                .before(consume_swing_signals),
        )
        .add_systems(Last, observe_drawn)
        .add_systems(
            Last,
            (|mut seen: ResMut<Seen>| seen.frame += 1).after(observe_drawn),
        );
    app
}

/// The dodge arm's staged fact: from the gates, the Rogue dodges every
/// melee attack it meets able to act.
fn stage_dodge(
    mut done: Local<bool>,
    dodge: Option<Res<DodgeEverything>>,
    countdown: Option<Res<MatchCountdown>>,
    mut combatants: Query<&mut Combatant>,
) {
    if *done || dodge.is_none() || !countdown.is_some_and(|c| c.gates_opened) {
        return;
    }
    for mut c in combatants.iter_mut() {
        if c.team == 2 && c.class == CharacterClass::Rogue {
            c.dodge_chance = 1.0;
            *done = true;
        }
    }
    assert!(*done, "no Rogue to stage");
}

/// The same-tick arm's stage: a dodged strike this tick makes the Paladin's
/// auto due now, and lets it through.
fn auto_on_the_strikes_tick(
    arm: Option<Res<AutoOnTheStrikesTick>>,
    avoided: Query<&MeleeAvoided>,
    mut combatants: Query<&mut Combatant>,
) {
    if arm.is_none() || !avoided.iter().any(|a| a.attack == "Crusader Strike") {
        return;
    }
    for mut c in combatants.iter_mut() {
        match (c.team, c.class) {
            (1, CharacterClass::Paladin) => c.attack_timer = 1000.0,
            (2, CharacterClass::Rogue) => c.dodge_chance = 0.0,
            _ => {}
        }
    }
}

/// ...and the Rogue dodges again from the next tick.
fn restore_dodge(arm: Option<Res<AutoOnTheStrikesTick>>, mut combatants: Query<&mut Combatant>) {
    if arm.is_none() {
        return;
    }
    for mut c in combatants.iter_mut() {
        if c.team == 2 && c.class == CharacterClass::Rogue {
            c.dodge_chance = 1.0;
        }
    }
}

fn observe_autos(
    mut seen: ResMut<Seen>,
    swings: Query<&AutoAttackSwing>,
    combatants: Query<&Combatant>,
) {
    let frame = seen.frame;
    for s in swings.iter() {
        if combatants
            .get(s.attacker)
            .is_ok_and(|c| c.class == CharacterClass::Paladin)
        {
            seen.autos.push(frame);
        }
    }
}

fn observe_strikes(
    mut seen: ResMut<Seen>,
    avoided: Query<&MeleeAvoided>,
    fired: Query<&InstantAbilityFired>,
) {
    let frame = seen.frame;
    for a in avoided.iter() {
        if a.attack == "Crusader Strike" {
            seen.avoided.push(frame);
        }
    }
    for f in fired.iter() {
        if f.ability == AbilityType::CrusaderStrike {
            seen.landed.push(frame);
        }
    }
}

fn observe_drawn(
    mut seen: ResMut<Seen>,
    trails: Query<(), Added<WeaponTrail>>,
    impacts: Query<&SchoolImpact, Added<SchoolImpact>>,
    sockets: Query<&WeaponSocket, Changed<WeaponSocket>>,
    combatants: Query<&Combatant>,
) {
    let frame = seen.frame;
    for _ in trails.iter() {
        seen.trails.push(frame);
    }
    for i in impacts.iter() {
        if i.ability == AbilityType::CrusaderStrike {
            seen.impacts.push(frame);
        }
    }
    for s in sockets.iter() {
        let paladin = combatants
            .get(s.owner)
            .is_ok_and(|c| c.class == CharacterClass::Paladin);
        // The stroke's first frame: released this frame from a full windup.
        if paladin
            && s.hand == WeaponHand::Main
            && s.swing_style == SwingStyle::MortalStrike
            && s.release_t.is_some_and(|t| t < 0.03)
        {
            seen.strokes.push(frame);
        }
    }
}

fn play(dodge: bool, same_tick_auto: bool) -> Seen {
    let mut app = boot(dodge, same_tick_auto);
    for _ in 0..FRAMES {
        app.update();
        if app.world().resource::<State<GameState>>().get() != &GameState::PlayMatch {
            break;
        }
    }
    std::mem::take(&mut *app.world_mut().resource_mut::<Seen>())
}

/// Every strike, landed or dodged, strokes and trails on its own frame; only
/// a landed one lands. With a certain dodge, the dodged strikes draw no
/// landing at all; in the control, every strike lands one.
#[test]
fn every_strike_strokes_and_trails_and_only_a_landed_one_lands() {
    for dodge in [true, false] {
        let seen = play(dodge, false);
        let strikes: Vec<usize> = {
            let mut s = seen.avoided.clone();
            s.extend(&seen.landed);
            s.sort();
            s
        };
        if dodge {
            assert!(
                seen.avoided.len() >= 2,
                "dodged strikes: {:?}",
                seen.avoided
            );
        } else {
            assert!(seen.avoided.is_empty(), "the control dodges nothing");
            assert!(seen.landed.len() >= 2, "landed strikes: {:?}", seen.landed);
        }
        assert_eq!(
            seen.trails, strikes,
            "dodge {dodge}: one trail per strike, on its frame"
        );
        assert_eq!(
            seen.impacts, seen.landed,
            "dodge {dodge}: one landing per LANDED strike, on its frame"
        );
        for f in &strikes {
            assert!(
                seen.strokes.contains(f),
                "dodge {dodge}: no Mortal Strike stroke on the strike's frame {f}: {:?}",
                seen.strokes
            );
        }
    }
}

/// An auto-attack landing on the tick a strike is dodged does not take the
/// socket back: the signature's stroke is started after the auto's, as the
/// landed strikes' is (`consume_avoided_strike_signals` after
/// `consume_swing_signals`).
#[test]
fn a_same_tick_auto_does_not_take_the_socket_from_a_dodged_strike() {
    let seen = play(true, true);
    assert!(
        seen.avoided.len() >= 2,
        "dodged strikes: {:?}",
        seen.avoided
    );
    for f in &seen.avoided {
        assert!(
            seen.autos.contains(f),
            "no auto landed on the dodged strike's frame {f}: {:?}",
            seen.autos
        );
        assert!(
            seen.strokes.contains(f),
            "the auto took the socket on frame {f}: strokes {:?}",
            seen.strokes
        );
    }
}
