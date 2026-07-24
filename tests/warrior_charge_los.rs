//! Durable guard: the Warrior's Charge decision rejects with `LosBlocked`
//! when an obstacle straddles the straight dash path to the target.
//!
//! Charge is a scripted sprint along the segment caster→target. If a pillar
//! crosses that segment, the dash would clip into (or grind to a stop against)
//! the obstacle instead of reaching melee, so `try_charge` now consults
//! `has_line_of_sight(ctx.obstacles, my_pos, target_pos)` after its range gates
//! and rejects when the path is blocked. This drives the real
//! `decide_warrior_action` and inspects the emitted decision trace, so it pins
//! the wiring — not just the underlying geometry helper.
//!
//! Natural occurrences are rare in live matches (the thin r=2.5 pillar seldom
//! sits exactly on the charge segment at charge range, and the kiting AI
//! regains line of sight to cast), so this constructs the geometry directly.

use std::collections::{BTreeMap, HashSet};

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::warrior::decide_warrior_action;
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo, QueuedInstantAttack};
use arenasim::states::play_match::decision_trace::{
    CandidateStatus, DecisionTrace, EventPayload, RejectionReason,
};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::{
    AbilityDefinitions, AbilityType, Aura, AuraType, Combatant, DispelType, GameRng,
};

use arenasim::combat::log::CombatLog;

fn combatant_info(entity: Entity, team: u8, class: CharacterClass, position: Vec3) -> CombatantInfo {
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

/// An AttackPowerIncrease aura on the Warrior itself — suppresses the Priority-1
/// Battle Shout (its only ally in a 1v1 is itself, and it skips allies that
/// already carry the buff) so the decision flow reaches Charge.
fn attack_power_aura() -> Aura {
    Aura {
        effect_type: AuraType::AttackPowerIncrease,
        duration: 30.0,
        magnitude: 1.0,
        break_on_damage_threshold: -1.0,
        accumulated_damage: 0.0,
        tick_interval: 0.0,
        time_until_next_tick: 0.0,
        caster: None,
        ability_name: "test".to_string(),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: None,
        applied_this_frame: false,
        backlash_damage: None,
        dr_category_override: None,
        dispel_type: DispelType::Auto,
    }
}

/// The Charge candidate's rejection reason from the emitted ability-decision
/// event, if the Warrior considered Charge this tick.
fn charge_rejection(trace: &DecisionTrace) -> Option<Option<RejectionReason>> {
    for event in &trace.pending_events {
        if let EventPayload::Ability { candidates, .. } = &event.payload {
            for c in candidates {
                if c.ability == AbilityType::Charge {
                    return Some(c.reason.clone());
                }
            }
        }
    }
    None
}

/// The Charge candidate's status (Chosen / Rejected), if considered.
fn charge_status(trace: &DecisionTrace) -> Option<CandidateStatus> {
    for event in &trace.pending_events {
        if let EventPayload::Ability { candidates, .. } = &event.payload {
            for c in candidates {
                if c.ability == AbilityType::Charge {
                    return Some(c.status);
                }
            }
        }
    }
    None
}

/// Run one Warrior decision tick against a target 12 yards away (within Charge's
/// [8, 25] band), with `obstacles` between them, and return the emitted trace.
fn run_charge_decision(obstacles: Vec<ObstacleVolume>) -> DecisionTrace {
    let world = World::new();
    let mut queue = CommandQueue::default();
    let mut commands = Commands::new(&mut queue, &world);

    let mut combat_log = CombatLog::default();
    let mut game_rng = GameRng::from_seed(0);
    let abilities = AbilityDefinitions::default();

    let warrior_entity = Entity::from_raw(1);
    let target_entity = Entity::from_raw(2);
    let my_pos = Vec3::new(0.0, 1.0, -6.0);
    let target_pos = Vec3::new(0.0, 1.0, 6.0); // distance 12 along +Z

    let mut combatant = Combatant::new(1, 0, CharacterClass::Warrior);
    combatant.current_mana = combatant.max_mana; // full rage
    combatant.global_cooldown = 0.0;
    combatant.target = Some(target_entity);

    // Snapshot storage for the CombatContext.
    let mut combatants: BTreeMap<Entity, CombatantInfo> = BTreeMap::new();
    let mut warrior_info = combatant_info(warrior_entity, 1, CharacterClass::Warrior, my_pos);
    warrior_info.target = Some(target_entity);
    combatants.insert(warrior_entity, warrior_info);
    combatants.insert(
        target_entity,
        combatant_info(target_entity, 2, CharacterClass::Mage, target_pos),
    );

    let mut active_auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
    active_auras.insert(warrior_entity, vec![attack_power_aura()]);

    let dr_trackers = BTreeMap::new();
    let ability_cooldowns = BTreeMap::new();

    let ctx = CombatContext {
        combatants: &combatants,
        active_auras: &active_auras,
        dr_trackers: &dr_trackers,
        ability_cooldowns: &ability_cooldowns,
        obstacles: &obstacles,
        self_entity: warrior_entity,
    };

    let mut instant_attacks: Vec<QueuedInstantAttack> = Vec::new();
    let mut battle_shouted: HashSet<Entity> = HashSet::new();
    let mut decision_trace = DecisionTrace::default();

    decide_warrior_action(
        &mut commands,
        &mut combat_log,
        &mut game_rng,
        &abilities,
        warrior_entity,
        &mut combatant,
        my_pos,
        None, // not rooted/CC'd
        &ctx,
        &mut instant_attacks,
        &mut battle_shouted,
        &mut decision_trace,
    );

    decision_trace
}

/// A full-height pillar centered on the origin, straddling the caster→target
/// segment (the shipped PillaredArena pillar radius).
fn blocking_pillar() -> ObstacleVolume {
    ObstacleVolume::Cylinder {
        center_xz: Vec2::new(0.0, 0.0),
        radius: 2.5,
        base_y: 0.0,
        height: 5.0,
    }
}

#[test]
fn charge_rejected_when_pillar_blocks_dash_path() {
    let trace = run_charge_decision(vec![blocking_pillar()]);
    let reason = charge_rejection(&trace)
        .expect("Warrior should have considered Charge (target in range, off cooldown)");
    assert!(
        matches!(reason, Some(RejectionReason::LosBlocked)),
        "Charge across a blocking pillar must reject as LosBlocked, got: {:?}",
        reason
    );
}

#[test]
fn charge_chosen_when_path_is_clear() {
    // Same geometry, no obstacles: the dash path is clear, so Charge (in range,
    // off cooldown, not rooted) is the chosen gap-closer — proving LosBlocked is
    // the ONLY thing the pillar changed.
    let trace = run_charge_decision(Vec::new());
    assert_eq!(
        charge_status(&trace),
        Some(CandidateStatus::Chosen),
        "with no obstacle Charge must be chosen; candidate statuses: {:?}",
        trace
            .pending_events
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::Ability { candidates, .. } => Some(candidates.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
}
