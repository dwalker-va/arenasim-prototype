//! The Druid's rotation, one decision at a time, through the real
//! `decide_druid_action`.
//!
//! Each case builds the scene the decision reads — the roster, the auras on
//! each unit, the Druid's own mana — and asserts what it CHOSE and on whom, as
//! the decision trace records it. Mark of the Wild is pre-applied to everyone
//! so the pre-match buff is out of the way and the rotation proper is what is
//! under test.

use std::collections::BTreeMap;

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::druid::{
    decide_druid_action, DRUID_EMERGENCY_HP, DRUID_INNERVATE_MANA_PCT,
};
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
use arenasim::states::play_match::decision_trace::{AbilityOutcome, DecisionTrace, EventPayload};
use arenasim::states::play_match::{
    AbilityDefinitions, AbilityType, ActiveAuras, Aura, AuraPending, Combatant, CompoundDebuff,
    MovementConfig,
};

fn info(entity: Entity, team: u8, class: CharacterClass, position: Vec3) -> CombatantInfo {
    CombatantInfo {
        entity,
        team,
        slot: 0,
        class,
        current_health: 100.0,
        max_health: 100.0,
        current_mana: 100.0,
        max_mana: 100.0,
        position,
        velocity: Vec3::ZERO,
        is_alive: true,
        stealthed: false,
        target: None,
        is_pet: false,
        casting_ability: None,
        pet_type: None,
        pet: None,
    }
}

fn mark() -> Aura {
    Aura {
        effect_type: arenasim::states::play_match::AuraType::MaxHealthIncrease,
        duration: 600.0,
        ability_name: "Mark of the Wild".to_string(),
        compound: Some(CompoundDebuff::MarkOfTheWild),
        ..Default::default()
    }
}

/// The scene: the Druid, one Warrior ally at `ally_hp` (of 100), and an enemy
/// Warrior in melee with that ally. `ally_rejuvenated` puts the Druid's own
/// Rejuvenation on the ally; `druid_mana` is a fraction of its pool.
struct Scene {
    ally_hp: f32,
    ally_rejuvenated: bool,
    /// A SECOND Druid on the team has its own Rejuvenation on the ally.
    other_druid_rejuvenated: bool,
    druid_mana: f32,
}

/// Run one decision and return what was chosen, on whom ("ally", "self",
/// "enemy"), or `None` when the Druid did nothing.
fn decide(scene: Scene) -> Option<(AbilityType, &'static str)> {
    let mut world = World::new();
    let druid = world.spawn_empty().id();
    let ally = world.spawn_empty().id();
    let enemy = world.spawn_empty().id();
    let abilities = AbilityDefinitions::default();
    let movement = MovementConfig::default();
    let mut combat_log = CombatLog::default();
    let mut trace = DecisionTrace::default();

    let druid_pos = Vec3::new(0.0, 1.0, 0.0);
    let ally_pos = Vec3::new(15.0, 1.0, 0.0);
    let mut combatant = Combatant::new(1, 1, CharacterClass::Druid);
    combatant.current_mana = combatant.max_mana * scene.druid_mana;
    combatant.target = Some(enemy);

    let mut roster = BTreeMap::new();
    let mut me = info(druid, 1, CharacterClass::Druid, druid_pos);
    me.current_mana = combatant.current_mana;
    me.max_mana = combatant.max_mana;
    roster.insert(druid, me);
    let mut warrior = info(ally, 1, CharacterClass::Warrior, ally_pos);
    warrior.current_health = scene.ally_hp;
    roster.insert(ally, warrior);
    let mut attacker = info(enemy, 2, CharacterClass::Warrior, ally_pos + Vec3::X);
    attacker.target = Some(ally);
    roster.insert(enemy, attacker);

    let mut active_auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
    active_auras.insert(druid, vec![mark()]);
    let mut ally_auras = vec![mark()];
    if scene.ally_rejuvenated {
        let def = abilities.get_unchecked(&AbilityType::Rejuvenation);
        ally_auras.push(AuraPending::from_ability(ally, druid, def).unwrap().aura);
    }
    if scene.other_druid_rejuvenated {
        let other_druid = world.spawn_empty().id();
        let def = abilities.get_unchecked(&AbilityType::Rejuvenation);
        ally_auras.push(
            AuraPending::from_ability(ally, other_druid, def)
                .unwrap()
                .aura,
        );
    }
    active_auras.insert(ally, ally_auras);
    let dr = BTreeMap::new();
    let cooldowns = BTreeMap::new();

    let ctx = CombatContext::new(
        druid,
        1,
        &roster,
        &active_auras,
        &dr,
        &cooldowns,
        &[],
        Default::default(),
        Default::default(),
    );
    let self_auras = ActiveAuras {
        auras: active_auras[&druid].clone(),
    };
    let mut queue = CommandQueue::default();
    let mut commands = Commands::new(&mut queue, &world);
    decide_druid_action(
        &mut commands,
        &mut combat_log,
        &abilities,
        druid,
        &mut combatant,
        druid_pos,
        Some(&self_auras),
        &ctx,
        &movement,
        false,
        None,
        true,
        30.0,
        &mut Default::default(),
        &mut trace,
    );

    let chosen = trace.pending_events.drain(..).find_map(|event| {
        let EventPayload::Ability { outcome, .. } = &event.payload else {
            return None;
        };
        let AbilityOutcome::ActionTaken {
            ability, target_id, ..
        } = outcome
        else {
            return None;
        };
        let on = match target_id {
            Some(t) if *t == ally.index() => "ally",
            Some(t) if *t == druid.index() => "self",
            Some(t) if *t == enemy.index() => "enemy",
            _ => "?",
        };
        Some((*ability, on))
    });
    chosen
}

const DYING: f32 = DRUID_EMERGENCY_HP * 100.0 - 10.0;

#[test]
fn a_dying_ally_without_rejuvenation_gets_rejuvenation_to_arm_swiftmend() {
    assert_eq!(
        decide(Scene {
            ally_hp: DYING,
            ally_rejuvenated: false,
            other_druid_rejuvenated: false,
            druid_mana: 1.0,
        }),
        Some((AbilityType::Rejuvenation, "ally"))
    );
}

#[test]
fn a_dying_ally_with_rejuvenation_gets_swiftmend() {
    assert_eq!(
        decide(Scene {
            ally_hp: DYING,
            ally_rejuvenated: true,
            other_druid_rejuvenated: false,
            druid_mana: 1.0,
        }),
        Some((AbilityType::Swiftmend, "ally"))
    );
}

#[test]
fn a_druid_low_on_mana_innervates_itself() {
    let below = DRUID_INNERVATE_MANA_PCT - 0.05;
    assert_eq!(
        decide(Scene {
            ally_hp: 100.0,
            ally_rejuvenated: true,
            other_druid_rejuvenated: false,
            druid_mana: below,
        }),
        Some((AbilityType::Innervate, "self"))
    );
}

#[test]
fn a_druid_with_mana_does_not_innervate() {
    let chosen = decide(Scene {
        ally_hp: 100.0,
        ally_rejuvenated: true,
        other_druid_rejuvenated: false,
        druid_mana: DRUID_INNERVATE_MANA_PCT + 0.3,
    });
    assert_ne!(chosen.map(|(a, _)| a), Some(AbilityType::Innervate));
}

/// A Rejuvenation is the caster's own (`StackScope::PerCaster`): another
/// Druid's on the dying ally does not arm THIS Druid's Swiftmend, so it lays
/// its own Rejuvenation first.
#[test]
fn another_druids_rejuvenation_does_not_arm_swiftmend() {
    assert_eq!(
        decide(Scene {
            ally_hp: DYING,
            ally_rejuvenated: false,
            other_druid_rejuvenated: true,
            druid_mana: 1.0,
        }),
        Some((AbilityType::Rejuvenation, "ally"))
    );
}

/// Two Druids and a Warrior before the gates, nobody marked. Each Druid
/// decides in turn from the SAME aura snapshot, as `decide_abilities` runs
/// them in one frame; `share` says whether they share one same-frame Mark set
/// (the real wiring) or each get a fresh one. Returns the Mark's target per
/// Druid, in decision order.
fn same_frame_marks(share: bool) -> Vec<Option<Entity>> {
    let mut world = World::new();
    let druids = [world.spawn_empty().id(), world.spawn_empty().id()];
    let warrior = world.spawn_empty().id();
    let abilities = AbilityDefinitions::default();
    let movement = MovementConfig::default();
    let mut combat_log = CombatLog::default();

    let mut roster = BTreeMap::new();
    for (i, &druid) in druids.iter().enumerate() {
        let mut me = info(
            druid,
            1,
            CharacterClass::Druid,
            Vec3::new(i as f32, 1.0, 0.0),
        );
        me.slot = i as u8;
        roster.insert(druid, me);
    }
    roster.insert(
        warrior,
        info(
            warrior,
            1,
            CharacterClass::Warrior,
            Vec3::new(5.0, 1.0, 0.0),
        ),
    );
    let active_auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
    let dr = BTreeMap::new();
    let cooldowns = BTreeMap::new();

    let mut shared = std::collections::HashSet::new();
    let mut targets = Vec::new();
    for (i, &druid) in druids.iter().enumerate() {
        let mut combatant = Combatant::new(1, i as u8, CharacterClass::Druid);
        let my_pos = roster[&druid].position;
        let ctx = CombatContext::new(
            druid,
            1,
            &roster,
            &active_auras,
            &dr,
            &cooldowns,
            &[],
            Default::default(),
            Default::default(),
        );
        let mut fresh = std::collections::HashSet::new();
        let marked = if share { &mut shared } else { &mut fresh };
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        decide_druid_action(
            &mut commands,
            &mut combat_log,
            &abilities,
            druid,
            &mut combatant,
            my_pos,
            None,
            &ctx,
            &movement,
            false,
            None,
            false,
            0.0,
            marked,
            &mut trace,
        );
        queue.apply(&mut world);
        let mut pending = world.query::<&AuraPending>();
        let target = pending
            .iter(&world)
            .filter(|p| p.aura.caster == Some(druid))
            .map(|p| p.target)
            .next();
        targets.push(target);
    }
    targets
}

/// Two Druids marking in one frame mark two DIFFERENT allies: the second does
/// not spend 30 mana refreshing a Mark that lands next frame. The unshared
/// control shows the scene would otherwise send both Marks to one ally.
#[test]
fn two_druids_never_mark_the_same_ally_in_one_frame() {
    let shared = same_frame_marks(true);
    assert!(
        shared.iter().all(Option::is_some),
        "both Druids cast a Mark: {shared:?}"
    );
    assert_ne!(
        shared[0], shared[1],
        "the second Druid must mark a different ally"
    );

    let unshared = same_frame_marks(false);
    assert!(unshared[0].is_some());
    assert_eq!(
        unshared[0], unshared[1],
        "control: without the same-frame set both Druids pick the same ally"
    );
}
