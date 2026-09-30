//! Healing reduction on heals over time, driven through `process_hot_ticks`.
//!
//! Classic's Mortal Strike cuts ALL healing received, so a heal over time
//! ticking on a Mortal-Striked target heals for the reduced amount, and the
//! same tick on a clean target heals in full. Each probe runs as a pair at
//! identical inputs, so the debuff is the only variable between the halves.
//! The reduction is read from Mortal Strike's own RON aura, never restated.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::auras::process_hot_ticks;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, Combatant, HealingRefused,
};
use arenasim::states::play_match::{AbilityDefinitions, AbilityType, AuraStacks};
use arenasim::CharacterClass;

/// A world holding what `process_hot_ticks` reads, with `dt` seconds on the clock.
fn world(dt: f32) -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(dt));
    world.insert_resource(time);
    world
}

/// The aura `ability` lands on `target` from `caster`, as `process_casting`
/// builds it.
fn aura_of(ability: AbilityType, target: Entity, caster: Entity) -> Aura {
    let defs = AbilityDefinitions::default();
    let mut aura =
        AuraPending::from_ability_scaled(target, caster, defs.get_unchecked(&ability), 100.0)
            .expect("the ability applies an aura")
            .aura;
    aura.caster = Some(caster);
    aura
}

/// Healing Stream's pulse shape: a one-tick heal over time.
fn healing_stream(caster: Entity) -> Aura {
    Aura {
        effect_type: AuraType::HealingOverTime,
        duration: 3.0,
        magnitude: 8.0,
        tick_interval: 2.0,
        caster: Some(caster),
        ability_name: "Healing Stream".to_string(),
        ..Default::default()
    }
}

/// Mortal Strike's multiplier, from its RON aura.
fn mortal_wound_multiplier() -> f32 {
    let defs = AbilityDefinitions::default();
    let aura = defs
        .get_unchecked(&AbilityType::MortalStrike)
        .applies_aura
        .as_ref()
        .expect("Mortal Strike applies Mortal Wound");
    assert_eq!(aura.aura_type, AuraType::HealingReduction);
    assert!(
        aura.magnitude < 1.0,
        "a reduction that cuts nothing proves nothing"
    );
    aura.magnitude
}

/// Tick `hot` once on a hurt ally, with or without Mortal Wound up. Returns
/// the tick's own amount, the healing it landed, and the Mortal Wounds tells
/// spawned on the ally.
fn one_tick(hot: impl Fn(Entity, Entity) -> Aura, mortal_striked: bool) -> (f32, f32, usize) {
    let mut world = world(0.1);
    let healer = world
        .spawn((
            Combatant::new(1, 1, CharacterClass::Druid),
            Transform::default(),
            // `process_hot_ticks` credits casters that carry auras.
            ActiveAuras::default(),
        ))
        .id();
    let warrior = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::default(),
        ))
        .id();
    let mut hurt = Combatant::new(1, 0, CharacterClass::Mage);
    hurt.current_health = 100.0;
    let ally = world.spawn((hurt, Transform::default())).id();

    let mut tick = hot(ally, healer);
    tick.time_until_next_tick = 0.05; // ticks this frame
    let per_tick = tick.tick_amount();
    let mut auras = vec![tick];
    if mortal_striked {
        auras.push(aura_of(AbilityType::MortalStrike, ally, warrior));
    }
    world.entity_mut(ally).insert(ActiveAuras { auras });

    world.run_system_once(process_hot_ticks).unwrap();
    // The healing credited to the healer is the effective heal, exact; the
    // ally's health delta would carry the rounding of `100.0 + heal`.
    let gained = world
        .entity(healer)
        .get::<Combatant>()
        .unwrap()
        .healing_done;
    assert!(
        world
            .entity(ally)
            .get::<Combatant>()
            .unwrap()
            .current_health
            > 100.0,
        "the tick must land on the ally"
    );
    let tells = world
        .query::<&HealingRefused>()
        .iter(&world)
        .filter(|t| t.target == ally)
        .count();
    (per_tick, gained, tells)
}

fn assert_cut_and_clean(name: &str, hot: impl Fn(Entity, Entity) -> Aura + Copy) {
    let multiplier = mortal_wound_multiplier();

    let (per_tick, clean, clean_tells) = one_tick(hot, false);
    assert!(
        per_tick > 0.0,
        "{name}: a tick that heals nothing proves nothing"
    );
    assert_eq!(clean, per_tick, "{name}: a clean tick heals in full");
    assert_eq!(
        clean_tells, 0,
        "{name}: a clean tick shows no Mortal Wounds tell"
    );

    let (_, cut, cut_tells) = one_tick(hot, true);
    assert_eq!(
        cut,
        per_tick * multiplier,
        "{name}: a tick on a Mortal-Striked target heals reduced"
    );
    assert_eq!(
        cut_tells, 1,
        "{name}: the cut tick shows the Mortal Wounds tell"
    );
}

#[test]
fn rejuvenation_ticks_are_cut_by_mortal_strike() {
    assert_cut_and_clean("Rejuvenation", |target, caster| {
        aura_of(AbilityType::Rejuvenation, target, caster)
    });
}

#[test]
fn stacked_lifebloom_ticks_are_cut_by_mortal_strike() {
    assert_cut_and_clean("Lifebloom", |target, caster| {
        let mut aura = aura_of(AbilityType::Lifebloom, target, caster);
        aura.stacks = aura.stacks.map(|s| AuraStacks { count: 3, ..s });
        aura
    });
}

#[test]
fn healing_stream_pulses_are_cut_by_mortal_strike() {
    assert_cut_and_clean("Healing Stream", |_, caster| healing_stream(caster));
}
