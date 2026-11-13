//! AS-481 — bleeds ignore armor, as in TBC (user ruling, 2026-11-13).
//!
//! A bleed's tick is Physical damage that armor does not reduce; every other
//! step of the damage pipeline applies to it. Pinned twice:
//!
//! - **in the funnel** (`combat_core::apply_damage_with_absorb`): a bleed hit
//!   on armor against the same hit without the mechanic, and the steps a
//!   bleed still meets — immunity, a damage-taken modifier of its school, a
//!   vulnerability, absorbs;
//! - **through the real tick site** (`auras::process_dot_ticks`), in staged
//!   matches: Rend and Rupture land as their abilities land them on a
//!   heavily armored Warrior and tick for their whole magnitude, while the
//!   same aura without its mechanic (a Physical tick that is no bleed) is
//!   halved; and a Physical-only damage-taken reduction reaches the bleed's
//!   tick, which a Shadow-only one does not, so the tick lands Physical.

mod common;

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::combat_core::{
    apply_damage_with_absorb, DamageFollowUps, Hit, HitKind,
};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraDamageMods, AuraPending, AuraType, Combatant, DamageScope, Mechanic,
};
use common::staged::{play, Played};

/// Armor that stops exactly half a Physical hit: `armor / (armor + 5500)`.
const HALF_ARMOR: f32 = 5500.0;

// ---------------------------------------------------------------------------
// In the funnel
// ---------------------------------------------------------------------------

/// A Warrior with 1000 health, `armor`, and no resistance.
fn victim(armor: f32) -> Combatant {
    let mut c = Combatant::new(2, 0, CharacterClass::Warrior);
    c.max_health = 1000.0;
    c.current_health = 1000.0;
    c.armor = armor;
    c
}

fn aura(kind: AuraType, magnitude: f32, scope: Option<DamageScope>) -> Aura {
    Aura {
        effect_type: kind,
        duration: 30.0,
        magnitude,
        break_on_damage_threshold: -1.0,
        damage_mods: AuraDamageMods {
            scope,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A periodic Physical hit of 100, a bleed's when `mechanic` is.
fn tick(victim: Entity, mechanic: Option<Mechanic>) -> Hit<'static> {
    Hit::new(100.0, SpellSchool::Physical, "Probe", None, None, victim)
        .of_kind(HitKind::Periodic)
        .mechanic(mechanic)
}

/// What `hit` puts on a `target` wearing `auras`: (to health, absorbed).
fn land(hit: Hit<'_>, target: &mut Combatant, auras: Vec<Aura>) -> (f32, f32) {
    let mut auras = ActiveAuras { auras };
    apply_damage_with_absorb(
        hit,
        target,
        Some(&mut auras),
        &mut DamageFollowUps::default(),
        &mut CombatLog::default(),
    )
}

fn entity() -> Entity {
    World::new().spawn_empty().id()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn armor_does_not_reduce_a_bleed() {
    let e = entity();
    let mut armored = victim(HALF_ARMOR);
    let (bleed, _) = land(tick(e, Some(Mechanic::Bleed)), &mut armored, vec![]);
    assert!(near(bleed, 100.0), "the bleed: {bleed}");
    assert!(near(armored.damage_mitigated_by_armor, 0.0));
    // The control: the same Physical tick, no bleed, is halved.
    let mut armored = victim(HALF_ARMOR);
    let (plain, _) = land(tick(e, None), &mut armored, vec![]);
    assert!(near(plain, 50.0), "the control: {plain}");
    assert!(near(armored.damage_mitigated_by_armor, 50.0));
}

#[test]
fn a_bleed_meets_every_other_step() {
    let e = entity();
    let bleed = || tick(e, Some(Mechanic::Bleed));
    // Immunity blocks it.
    let (to_health, absorbed) = land(
        bleed(),
        &mut victim(HALF_ARMOR),
        vec![aura(AuraType::DamageImmunity, 0.0, None)],
    );
    assert_eq!((to_health, absorbed), (0.0, 0.0), "immunity");
    // A Physical damage-taken reduction reaches it (Shadowform's)...
    let physical = Some(DamageScope::School(SpellSchool::Physical));
    let (to_health, _) = land(
        bleed(),
        &mut victim(HALF_ARMOR),
        vec![aura(AuraType::DamageTakenReduction, 0.15, physical)],
    );
    assert!(near(to_health, 85.0), "a Physical reduction: {to_health}");
    // ...a spell-only one does not.
    let (to_health, _) = land(
        bleed(),
        &mut victim(HALF_ARMOR),
        vec![aura(
            AuraType::DamageTakenReduction,
            0.15,
            Some(DamageScope::Spells),
        )],
    );
    assert!(
        near(to_health, 100.0),
        "a spell-only reduction: {to_health}"
    );
    // A vulnerability raises it (Improved Kidney Shot's).
    let (to_health, _) = land(
        bleed(),
        &mut victim(HALF_ARMOR),
        vec![aura(AuraType::DamageTakenIncrease, 0.10, None)],
    );
    assert!(near(to_health, 110.0), "a vulnerability: {to_health}");
    // An absorb soaks it.
    let (to_health, absorbed) = land(
        bleed(),
        &mut victim(HALF_ARMOR),
        vec![aura(AuraType::Absorb, 30.0, None)],
    );
    assert!(
        near(to_health, 70.0) && near(absorbed, 30.0),
        "an absorb: {to_health}, {absorbed}"
    );
}

// ---------------------------------------------------------------------------
// Through the tick site, in staged matches
// ---------------------------------------------------------------------------

const T1: Unit = Unit::new(1, 0);
const T2: Unit = Unit::new(2, 0);

fn config(team1: &str, seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec![team1.into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(seed),
        max_duration_secs: 20.0,
        ..Default::default()
    }
}

/// Both units held in a long stun, apart and stripped of their pre-match
/// buffs, the victim (team 2's Warrior) in `armor` and wearing `extra`: the
/// staged bleed's ticks are the only damage either takes.
fn held(armor: f32, extra: Vec<Aura>) -> Stage {
    let stun = || aura(AuraType::Stun, 600.0, None);
    let mut stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T2, 40.0, 0.0)
        .clear_auras(T1)
        .clear_auras(T2)
        .aura(T1, stun())
        .aura(T2, stun())
        .combatant(T2, move |c| c.armor = armor);
    for a in extra {
        stage = stage.aura(T2, a);
    }
    stage
}

/// `ability`'s aura as its ability lands it on T2 from T1, with `edit` made
/// to it first.
fn land_edited(
    stage: Stage,
    ability: AbilityType,
    edit: impl FnOnce(&mut Aura) + Send + Sync + 'static,
) -> Stage {
    stage.world(move |world| {
        let (t, c) = (T2.entity(world), T1.entity(world));
        let def = world
            .resource::<AbilityDefinitions>()
            .get_unchecked(&ability)
            .clone();
        let mut pending = AuraPending::from_ability(t, c, &def).expect("a damage over time");
        edit(&mut pending.aura);
        world.spawn(pending);
    })
}

/// Every tick the log gives `actor`'s `ability`, in order.
fn ticks(played: &Played, actor: &str, ability: &str) -> Vec<f32> {
    let tick = format!("{actor}'s {ability} ticks for ");
    played
        .log
        .lines()
        .filter_map(|line| {
            let rest = &line[line.find(&tick)? + tick.len()..];
            rest.split(' ').next()?.parse().ok()
        })
        .collect()
}

/// The ticks, asserted to be there (a staged aura that never ticked would
/// pass any per-tick claim).
fn ticked(played: &Played, actor: &str, ability: &str) -> Vec<f32> {
    let t = ticks(played, actor, ability);
    assert!(
        t.len() >= 3,
        "{actor}'s {ability} ticked {} times:\n{}",
        t.len(),
        played.log
    );
    t
}

/// (team 1's class, its bleed, the bleed's tick, its actor in the log).
const BLEEDS: [(&str, AbilityType, f32, &str, &str); 2] = [
    (
        "Warrior",
        AbilityType::Rend,
        8.0,
        "Team 1 Warrior #1",
        "Rend",
    ),
    (
        "Rogue",
        AbilityType::Rupture,
        10.0,
        "Team 1 Rogue #1",
        "Rupture",
    ),
];

#[test]
fn a_bleed_ticks_through_armor_and_a_physical_tick_does_not() {
    for (class, ability, magnitude, actor, name) in BLEEDS {
        // The bleed, landed as its ability lands it.
        let bleed = play(
            config(class, 11),
            held(HALF_ARMOR, vec![]).land(T2, ability, T1),
        );
        let got = ticked(&bleed, actor, name);
        assert!(
            got.iter().all(|t| *t == magnitude),
            "{name} on half armor: {got:?}"
        );
        // The control: the same aura as a Physical tick that is no bleed.
        let plain = play(
            config(class, 11),
            land_edited(held(HALF_ARMOR, vec![]), ability, |a| {
                a.spell_school = Some(SpellSchool::Physical);
                a.damage_mods.mechanic = None;
            }),
        );
        let got = ticked(&plain, actor, name);
        assert!(
            got.iter().all(|t| *t == magnitude / 2.0),
            "{name} without its mechanic on half armor: {got:?}"
        );
    }
}

#[test]
fn a_bleed_tick_lands_physical() {
    let reduction = |school| {
        aura(
            AuraType::DamageTakenReduction,
            0.5,
            Some(DamageScope::School(school)),
        )
    };
    for (class, ability, magnitude, actor, name) in BLEEDS {
        let physical = play(
            config(class, 11),
            held(0.0, vec![reduction(SpellSchool::Physical)]).land(T2, ability, T1),
        );
        let got = ticked(&physical, actor, name);
        assert!(
            got.iter().all(|t| *t == magnitude / 2.0),
            "{name} under a Physical reduction: {got:?}"
        );
        // The control: a Shadow reduction does not reach it.
        let shadow = play(
            config(class, 11),
            held(0.0, vec![reduction(SpellSchool::Shadow)]).land(T2, ability, T1),
        );
        let got = ticked(&shadow, actor, name);
        assert!(
            got.iter().all(|t| *t == magnitude),
            "{name} under a Shadow reduction: {got:?}"
        );
    }
}
