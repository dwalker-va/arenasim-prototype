//! Stealth is enforced by construction, not by per-class checks.
//!
//! A class AI decides from a `CombatContext`, and the only way to build one is
//! `CombatContext::new`, which leaves every enemy the deciding unit cannot see
//! out of `ctx.combatants`. So "does class X check stealth?" is not a question
//! any class module answers — which is why three of them (Mage, Warlock,
//! Shaman) could omit the check entirely and Death Coil a stealthed Rogue.
//!
//! `every_class_ai_respects_stealth` drives the REAL `decide_<class>_action`
//! of every class against a stealthed enemy Rogue standing in melee range,
//! low on health, targeting the decider — with the decider's own `target` and
//! `cc_target` forced onto the Rogue, the worst case acquisition can never
//! produce. The classes it iterates are every variant the enum's own derive
//! knows (`every_variant`), and it dispatches them through an exhaustive
//! `match`, so a ninth class cannot be skipped by the loop nor compile without
//! being named. Each class is paired with a control run in
//! which the same Rogue is VISIBLE and the class must act on it — without that,
//! a class that simply did nothing in this scenario would pass vacuously.

use std::collections::{BTreeMap, HashSet};

use bevy::ecs::system::RunSystemOnce;
use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;
use serde::de::value::{Error as DeError, U32Deserializer};
use serde::Deserialize;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::{CharacterClass, WarriorShout};
use arenasim::states::play_match::class_ai::hunter_dip::HunterDipPlan;
use arenasim::states::play_match::class_ai::paladin::PaladinMovementPlan;
use arenasim::states::play_match::class_ai::priest::PriestMovementPlan;
use arenasim::states::play_match::class_ai::shaman::ShamanMovementPlan;
use arenasim::states::play_match::class_ai::{
    self, CombatContext, CombatantInfo, QueuedAoeDamage, QueuedInstantAttack,
};
use arenasim::states::play_match::decision_trace::{AbilityOutcome, DecisionTrace, EventPayload};
use arenasim::states::play_match::team_solve;
use arenasim::states::play_match::{
    apply_pending_auras, slow_zone_system, trap_system, AbilityDefinitions, AbilityType,
    ArenaDampening, Aura, AuraPending, AuraType, CastingState, ChannelingState, Combatant,
    DRCategory, DRTracker, DispelType, GameRng, HolyShockDamagePending, InstantAbilityFired,
    MovementConfig, Trap, TrapType,
};

/// Ticks per run. A class may spend its first decisions on self-buffs
/// (Battle Shout, Fortitude, a Paladin aura); those land between ticks, so by
/// the last tick a class with a visible enemy in its face has turned on it.
const TICKS: usize = 8;

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

fn shadow_sight() -> Aura {
    Aura {
        effect_type: AuraType::ShadowSight,
        duration: 15.0,
        magnitude: 0.0,
        break_on_damage_threshold: -1.0,
        accumulated_damage: 0.0,
        tick_interval: 0.0,
        time_until_next_tick: 0.0,
        caster: None,
        ability_name: "Shadow Sight".to_string(),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: None,
        applied_this_frame: false,
        backlash_damage: None,
        dr_category_override: None,
        dispel_type: DispelType::Physical,
        compound: None,
        source_item: None,
    }
}

// ============================================================================
// The view `CombatContext::new` builds
// ============================================================================

struct Arena {
    roster: BTreeMap<Entity, CombatantInfo>,
    auras: BTreeMap<Entity, Vec<Aura>>,
    dr: BTreeMap<Entity, arenasim::states::play_match::DRTracker>,
    cooldowns: BTreeMap<Entity, BTreeMap<arenasim::states::play_match::AbilityType, f32>>,
}

const MAGE: Entity = Entity::from_raw(1);
const ALLY_ROGUE: Entity = Entity::from_raw(2);
const ENEMY_ROGUE: Entity = Entity::from_raw(3);
const ENEMY_WARRIOR: Entity = Entity::from_raw(4);

/// A Mage with a stealthed Rogue on each side, and one visible enemy Warrior.
fn arena() -> Arena {
    let mut roster = BTreeMap::new();
    roster.insert(MAGE, info(MAGE, 1, CharacterClass::Mage, Vec3::ZERO));
    for (e, team) in [(ALLY_ROGUE, 1), (ENEMY_ROGUE, 2)] {
        let mut rogue = info(e, team, CharacterClass::Rogue, Vec3::new(2.0, 0.0, 0.0));
        rogue.stealthed = true;
        roster.insert(e, rogue);
    }
    roster.insert(
        ENEMY_WARRIOR,
        info(
            ENEMY_WARRIOR,
            2,
            CharacterClass::Warrior,
            Vec3::new(0.0, 0.0, 5.0),
        ),
    );
    Arena {
        roster,
        auras: BTreeMap::new(),
        dr: BTreeMap::new(),
        cooldowns: BTreeMap::new(),
    }
}

impl Arena {
    fn ctx_for(&self, observer: Entity) -> CombatContext<'_> {
        CombatContext::new(
            observer,
            self.roster[&observer].team,
            &self.roster,
            &self.auras,
            &self.dr,
            &self.cooldowns,
            &[],
            Default::default(),
            Default::default(),
        )
    }
}

fn perceived(ctx: &CombatContext) -> Vec<Entity> {
    ctx.combatants.keys().copied().collect()
}

#[test]
fn a_stealthed_enemy_is_not_in_the_view_but_a_stealthed_ally_is() {
    let arena = arena();
    let ctx = arena.ctx_for(MAGE);
    assert_eq!(perceived(&ctx), vec![MAGE, ALLY_ROGUE, ENEMY_WARRIOR]);
    assert!(ctx.enemy_hidden(), "the hidden Rogue is unaccounted for");
}

#[test]
fn shadow_sight_on_either_side_reveals() {
    // The observer holds it: sees everything.
    let mut arena = arena();
    arena.auras.insert(MAGE, vec![shadow_sight()]);
    let ctx = arena.ctx_for(MAGE);
    assert_eq!(
        perceived(&ctx),
        vec![MAGE, ALLY_ROGUE, ENEMY_ROGUE, ENEMY_WARRIOR]
    );
    assert!(!ctx.enemy_hidden());

    // The stealthed enemy holds it: picking up the buff reveals the holder.
    let mut arena = self::arena();
    arena.auras.insert(ENEMY_ROGUE, vec![shadow_sight()]);
    let ctx = arena.ctx_for(MAGE);
    assert_eq!(
        perceived(&ctx),
        vec![MAGE, ALLY_ROGUE, ENEMY_ROGUE, ENEMY_WARRIOR]
    );
}

#[test]
fn the_team_solve_positions_around_exactly_what_its_unit_sees() {
    // The solve reads the view and adds no stealth filter of its own: without
    // Shadow Sight the hidden Rogue is absent, with it the Rogue is there.
    let solved = |arena: &Arena| -> Vec<Entity> {
        team_solve::world_from_context(&arena.ctx_for(MAGE), 40.0, 12.0, None)
            .units
            .iter()
            .map(|u| u.entity)
            .collect()
    };
    let mut arena = arena();
    assert_eq!(solved(&arena), vec![MAGE, ALLY_ROGUE, ENEMY_WARRIOR]);
    arena.auras.insert(MAGE, vec![shadow_sight()]);
    assert_eq!(
        solved(&arena),
        vec![MAGE, ALLY_ROGUE, ENEMY_ROGUE, ENEMY_WARRIOR]
    );
}

#[test]
fn nothing_hidden_means_nothing_unaccounted_for() {
    let mut arena = arena();
    arena.roster.get_mut(&ENEMY_ROGUE).unwrap().stealthed = false;
    let ctx = arena.ctx_for(MAGE);
    assert_eq!(
        perceived(&ctx),
        vec![MAGE, ALLY_ROGUE, ENEMY_ROGUE, ENEMY_WARRIOR]
    );
    assert!(!ctx.enemy_hidden());
}

#[test]
fn an_area_catches_what_the_caster_cannot_see() {
    // Deciding and resolving are different questions: the Mage cannot see the
    // Rogue, but a Frost Nova centred on it still lands on it. Allies are
    // never victims; the Warrior at 5yd is outside a 3yd radius.
    let arena = arena();
    let ctx = arena.ctx_for(MAGE);
    let victims: Vec<Entity> = ctx
        .area_victims(1, Vec3::ZERO, 3.0)
        .into_iter()
        .map(|i| i.entity)
        .collect();
    assert_eq!(victims, vec![ENEMY_ROGUE]);
}

#[test]
fn the_visibility_rule_is_one_truth_table() {
    // (enemy stealthed, observer has Shadow Sight, enemy has Shadow Sight)
    for (stealthed, observer_ss, enemy_ss, visible) in [
        (false, false, false, true),
        (true, false, false, false),
        (true, true, false, true),
        (true, false, true, true),
        (true, true, true, true),
    ] {
        assert_eq!(
            class_ai::stealth_visible(stealthed, observer_ss, enemy_ss),
            visible,
            "stealthed={stealthed} observer_ss={observer_ss} enemy_ss={enemy_ss}"
        );
    }
}

// ============================================================================
// Every class AI, against a stealthed Rogue in its face
// ============================================================================

/// How the enemy Rogue stands in a run.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Rogue {
    Visible,
    Stealthed,
    /// Not in the arena at all — what a stealthed Rogue must be
    /// indistinguishable from.
    Absent,
}

/// What one run did: every output that names the Rogue, and the sequence of
/// abilities chosen (with targets), for comparing runs against each other.
#[derive(Debug, Default)]
struct Run {
    touches: Vec<String>,
    choices: Vec<String>,
}

/// Drive `class`'s real decision function for `TICKS` ticks with an enemy
/// Rogue `distance` yards away, and record every output that names the Rogue:
/// chosen targets from the decision trace (the class-agnostic record — every
/// `choose` carries its target), plus the target-less channels an area choice
/// writes through (Frost Nova's damage queue, same-frame CC, spawned auras)
/// and the components a cast or instant leaves behind.
fn run(class: CharacterClass, rogue_state: Rogue, distance: f32) -> Run {
    run_with(class, rogue_state, distance, Setup::default())
}

/// What a run adds to the plain scenario.
struct Setup {
    /// A VISIBLE enemy Warrior standing on the Rogue's spot — something the
    /// decider can see and act on, beside something it cannot.
    bystander: bool,
    /// A Priest decider under pressure: the gate on its defensive Psychic
    /// Scream, computed by its posture layer rather than its decider.
    pressured: bool,
    /// Adjusts the decider before the run (a strategic option, say).
    prepare: fn(&mut Combatant),
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            bystander: false,
            pressured: false,
            prepare: |_| {},
        }
    }
}

fn run_with(class: CharacterClass, rogue_state: Rogue, distance: f32, setup: Setup) -> Run {
    let Setup {
        bystander,
        pressured,
        prepare,
    } = setup;
    let mut world = World::new();
    let me = world.spawn_empty().id();
    let rogue = world.spawn_empty().id();

    let abilities = AbilityDefinitions::default();
    let movement = MovementConfig::default();
    let mut combat_log = CombatLog::default();
    let mut game_rng = GameRng::from_seed(7);
    let mut trace = DecisionTrace::default();

    let my_pos = Vec3::new(0.0, 1.0, 0.0);
    let rogue_pos = Vec3::new(distance, 1.0, 0.0);

    let mut combatant = Combatant::new(1, 0, class);
    combatant.current_mana = combatant.max_mana;
    combatant.target = Some(rogue);
    combatant.cc_target = Some(rogue);
    prepare(&mut combatant);

    let mut roster = BTreeMap::new();
    let mut my_info = info(me, 1, class, my_pos);
    my_info.target = Some(rogue);
    my_info.max_health = combatant.max_health;
    my_info.current_health = combatant.max_health;
    my_info.stealthed = combatant.stealthed;
    roster.insert(me, my_info);
    let mut rogue_info = info(rogue, 2, CharacterClass::Rogue, rogue_pos);
    rogue_info.current_health = 35.0;
    rogue_info.target = Some(me);
    rogue_info.stealthed = rogue_state == Rogue::Stealthed;
    if rogue_state != Rogue::Absent {
        roster.insert(rogue, rogue_info);
    }
    if bystander {
        let warrior = world.spawn_empty().id();
        roster.insert(
            warrior,
            info(warrior, 2, CharacterClass::Warrior, rogue_pos),
        );
    }

    let mut active_auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
    let dr = BTreeMap::new();
    let cooldowns = BTreeMap::new();
    let mut found = Run::default();

    for tick in 0..TICKS {
        combatant.global_cooldown = 0.0;
        roster.get_mut(&me).unwrap().current_mana = combatant.current_mana;

        let ctx = CombatContext::new(
            me,
            1,
            &roster,
            &active_auras,
            &dr,
            &cooldowns,
            &[],
            Default::default(),
            Default::default(),
        );
        let self_auras = arenasim::states::play_match::ActiveAuras {
            auras: active_auras.get(&me).cloned().unwrap_or_default(),
        };
        let auras = Some(&self_auras);

        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut instant_attacks: Vec<QueuedInstantAttack> = Vec::new();
        let mut frost_nova: Vec<QueuedAoeDamage> = Vec::new();
        let mut same_frame_cc: Vec<(Entity, Aura)> = Vec::new();
        let mut set_a: HashSet<Entity> = HashSet::new();
        let mut set_b: HashSet<Entity> = HashSet::new();

        // Exhaustive: a new class does not compile until it is named here.
        match class {
            CharacterClass::Warrior => {
                class_ai::warrior::decide_warrior_action(
                    &mut commands,
                    &mut combat_log,
                    &mut game_rng,
                    &abilities,
                    me,
                    &mut combatant,
                    my_pos,
                    auras,
                    &ctx,
                    &mut instant_attacks,
                    &mut set_a,
                    &mut trace,
                );
            }
            CharacterClass::Mage => {
                class_ai::mage::decide_mage_action(
                    &mut commands,
                    &mut combat_log,
                    &mut game_rng,
                    &abilities,
                    me,
                    &mut combatant,
                    my_pos,
                    auras,
                    &ctx,
                    &mut frost_nova,
                    &mut same_frame_cc,
                    &mut trace,
                );
            }
            CharacterClass::Rogue => {
                class_ai::rogue::decide_rogue_action(
                    &mut commands,
                    &mut combat_log,
                    &mut game_rng,
                    &abilities,
                    me,
                    &mut combatant,
                    my_pos,
                    &ctx,
                    &mut instant_attacks,
                    &mut same_frame_cc,
                    &mut trace,
                );
            }
            CharacterClass::Priest => {
                class_ai::priest::decide_priest_action(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    me,
                    &mut combatant,
                    my_pos,
                    auras,
                    &ctx,
                    &mut set_a,
                    &mut set_b,
                    &PriestMovementPlan {
                        pressured,
                        ..PriestMovementPlan::default()
                    },
                    &movement,
                    &mut same_frame_cc,
                    &mut trace,
                );
            }
            CharacterClass::Warlock => {
                class_ai::warlock::decide_warlock_action(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    me,
                    &mut combatant,
                    my_pos,
                    auras,
                    &ctx,
                    &mut trace,
                );
            }
            CharacterClass::Paladin => {
                class_ai::paladin::decide_paladin_action(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    me,
                    &mut combatant,
                    my_pos,
                    auras,
                    &ctx,
                    &mut set_a,
                    &mut same_frame_cc,
                    &PaladinMovementPlan::default(),
                    &mut trace,
                );
            }
            CharacterClass::Hunter => {
                class_ai::hunter::decide_hunter_action(
                    &mut commands,
                    &mut combat_log,
                    &mut game_rng,
                    &abilities,
                    me,
                    &mut combatant,
                    my_pos,
                    auras,
                    &ctx,
                    &mut instant_attacks,
                    HunterDipPlan::default(),
                    &mut trace,
                );
            }
            CharacterClass::Shaman => {
                class_ai::shaman::decide_shaman_action(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    me,
                    &mut combatant,
                    my_pos,
                    auras,
                    &ctx,
                    // Every totem already down: the rotation proper, not the
                    // opening totem drops, is what is under test.
                    &[60.0; 4],
                    &ShamanMovementPlan::default(),
                    &movement,
                    &mut trace,
                );
            }
        }
        drop(ctx);
        queue.apply(&mut world);

        let tag = |what: &str| format!("tick {tick}: {what}");
        for event in trace.pending_events.drain(..) {
            let outcome = match &event.payload {
                EventPayload::Ability { outcome, .. } | EventPayload::Pet { outcome, .. } => {
                    outcome
                }
                _ => continue,
            };
            if let AbilityOutcome::ActionTaken {
                ability, target_id, ..
            } = outcome
            {
                let on = match target_id {
                    Some(t) if *t == rogue.index() => "the Rogue",
                    Some(t) if *t == me.index() => "self",
                    Some(_) => "?",
                    None => "-",
                };
                found.choices.push(format!("{ability:?} on {on}"));
                if *target_id == Some(rogue.index()) {
                    found.touches.push(tag(&format!("chose {ability:?} on it")));
                }
            }
        }
        for a in &instant_attacks {
            if a.target == rogue {
                found.touches.push(tag(&format!("queued {:?}", a.ability)));
            }
        }
        if frost_nova.iter().any(|d| d.target == rogue) {
            found.touches.push(tag("caught it in Frost Nova"));
        }
        for (target, aura) in &same_frame_cc {
            if *target == rogue {
                found
                    .touches
                    .push(tag(&format!("same-frame {:?}", aura.effect_type)));
            }
        }
        if let Some(cast) = world.get::<CastingState>(me) {
            if cast.target == Some(rogue) {
                found
                    .touches
                    .push(tag(&format!("began casting {:?}", cast.ability)));
            }
        }
        if let Some(channel) = world.get::<ChannelingState>(me) {
            if channel.target == rogue {
                found
                    .touches
                    .push(tag(&format!("began channeling {:?}", channel.ability)));
            }
        }
        world
            .entity_mut(me)
            .remove::<(CastingState, ChannelingState)>();

        let mut spawned = world.query::<(
            Entity,
            Option<&AuraPending>,
            Option<&InstantAbilityFired>,
            Option<&HolyShockDamagePending>,
        )>();
        let mut despawn = Vec::new();
        for (e, pending, fired, holy_shock) in spawned.iter(&world) {
            if e == me || e == rogue {
                continue;
            }
            if let Some(p) = pending {
                if p.target == rogue {
                    found
                        .touches
                        .push(tag(&format!("applied {:?}", p.aura.effect_type)));
                } else {
                    // A buff on itself lands before the next decision.
                    active_auras
                        .entry(p.target)
                        .or_default()
                        .push(p.aura.clone());
                }
            }
            if let Some(f) = fired {
                if f.target == Some(rogue) {
                    found
                        .touches
                        .push(tag(&format!("fired {:?} at it", f.ability)));
                }
            }
            if holy_shock.is_some_and(|h| h.target == rogue) {
                found.touches.push(tag("Holy Shock damage pending on it"));
            }
            despawn.push(e);
        }
        for e in despawn {
            world.despawn(e);
        }
    }
    found
}

/// Melee range (Death Coil peel, Frost Nova, Hammer of Justice, the Hunter's
/// dead zone) and mid range (every ranged opener and Charge). A class that
/// acts on a visible Rogue at either distance has its stealthed runs at both
/// checked.
const DISTANCES: [f32; 2] = [2.0, 12.0];

#[test]
fn every_class_ai_respects_stealth() {
    let mut leaks = Vec::new();
    let mut vacuous = Vec::new();
    for class in every_variant::<CharacterClass>() {
        let mut acted_on_visible = false;
        for distance in DISTANCES {
            acted_on_visible |= !run(class, Rogue::Visible, distance).touches.is_empty();

            // Never acted ON: nothing it did names the stealthed Rogue.
            let hidden = run(class, Rogue::Stealthed, distance);
            if !hidden.touches.is_empty() {
                leaks.push(format!(
                    "{class:?} at {distance}yd acted on it: {:?}",
                    hidden.touches
                ));
            }
            // Never REACTED to: it chose exactly what it chooses with no Rogue
            // in the arena — no Disengage from it, no Frost Nova because of it.
            let absent = run(class, Rogue::Absent, distance);
            if hidden.choices != absent.choices {
                leaks.push(format!(
                    "{class:?} at {distance}yd reacted to it:\n  stealthed Rogue: {:?}\n  no Rogue:        {:?}",
                    hidden.choices, absent.choices
                ));
            }
        }
        if !acted_on_visible {
            vacuous.push(format!("{class:?}"));
        }
    }
    assert!(
        vacuous.is_empty(),
        "control failed — these classes never acted on a VISIBLE Rogue at any tested \
         distance, so their stealthed runs prove nothing: {vacuous:?}"
    );
    assert!(
        leaks.is_empty(),
        "class AIs saw a stealthed Rogue:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn an_area_decided_on_a_seen_enemy_still_catches_the_stealthed_one() {
    // The half of the rule that is NOT "never act on it": a Frost Nova the
    // Mage casts at a Warrior it can see lands on the stealthed Rogue beside
    // it too — an area does not aim — and its damage is what reveals it.
    let with_rogue = run_with(
        CharacterClass::Mage,
        Rogue::Stealthed,
        2.0,
        Setup {
            bystander: true,
            ..Setup::default()
        },
    );
    assert!(
        with_rogue.choices.iter().any(|c| c == "FrostNova on -"),
        "the Mage should Nova the visible Warrior: {:?}",
        with_rogue.choices
    );
    assert!(
        with_rogue
            .touches
            .iter()
            .any(|t| t.ends_with("caught it in Frost Nova")),
        "the Nova must catch the stealthed Rogue standing in it: {:?}",
        with_rogue.touches
    );
}

// ============================================================================
// Every variant, as the enum's own derive knows them
// ============================================================================

/// Every variant of a fieldless enum, read off its derived `Deserialize` by
/// variant INDEX — so the list is the enum itself, not a hand-kept slice that
/// can fall behind it. Stops at the first index the derive rejects, and asserts
/// that rejection is "no such variant" rather than something that would
/// silently truncate the list.
fn every_variant<T: for<'de> Deserialize<'de>>() -> Vec<T> {
    let mut all = Vec::new();
    for index in 0u32.. {
        match T::deserialize(U32Deserializer::<DeError>::new(index)) {
            Ok(variant) => all.push(variant),
            Err(e) => {
                assert!(
                    e.to_string().contains("variant index"),
                    "variant {index} did not deserialize for a reason other than being \
                     past the end: {e}"
                );
                return all;
            }
        }
    }
    unreachable!()
}

#[test]
fn the_guard_drives_every_class_the_enum_has() {
    // The class guard iterates `every_variant`; `CharacterClass::all()` is the
    // hand-kept list the rest of the game iterates. They must agree.
    let derived = every_variant::<CharacterClass>();
    assert_eq!(derived.len(), 8, "{derived:?}");
    assert_eq!(derived, CharacterClass::all().to_vec());
}

// ============================================================================
// Area effects reach the unseen, and what they land reveals
// ============================================================================

/// Whether `ability` affects ENEMIES in an area — everyone standing in it, as
/// opposed to a target it was aimed at. Such an effect is decided on what its
/// caster can see but lands on every enemy in it, seen or not, so a stealthed
/// Rogue standing in it is caught (and revealed by what lands).
///
/// Exhaustive on purpose — no `_ =>` arm. A new ability does not compile until
/// someone answers this question for it, and a `true` answer fails
/// `every_area_effect_reaches_a_stealthed_enemy` until the ability has a driver
/// there proving it. Area effects on ALLIES (shouts, totems, Paladin auras)
/// answer `false`: an ally is always seen.
fn reaches_enemies_in_an_area(ability: AbilityType) -> bool {
    use AbilityType::*;
    match ability {
        FrostNova | PsychicScream | DemoralizingShout | FreezingTrap | FrostTrap => true,
        Frostbolt | FlashHeal | HeroicStrike | Ambush | CheapShot | MindBlast
        | SinisterStrike | Charge | KidneyShot | PowerWordFortitude | Rend | MortalStrike
        | Pummel | BerserkerRage | Kick | CripplingPoison | Corruption | Shadowbolt | Fear
        | Immolate | DrainLife | CurseOfAgony | CurseOfWeakness | CurseOfTongues
        | UnstableAffliction | DeathCoil | ArcaneIntellect | BattleShout | IceBarrier
        | PowerWordShield | Polymorph | DispelMagic | ManaBurn | FlashOfLight | HolyLight
        | HolyShock | HammerOfJustice | PaladinCleanse | DevotionAura | DivineShield
        | SpellLock | DevourMagic | AimedShot | ArcaneShot | ConcussiveShot | SerpentSting
        | Disengage | SpiderWeb | BoarCharge | MastersCall | CommandingShout | FrostArmor
        | MageArmorSpell | MoltenArmor | ShadowResistanceAura | ConcentrationAura
        | LightningBolt | FrostShock | LesserHealingWave | Purge | WindShear | AirTotem
        | WaterTotem | EarthTotem | FireTotem => false,
    }
}

/// Run `ability` with a stealthed Rogue standing inside it, beside a visible
/// enemy the caster decides on, and say what reached the Rogue. `Err` when
/// nothing did.
fn drive_area_effect(ability: AbilityType) -> Result<String, String> {
    let touched = |class, setup: Setup, what: &str| {
        let r = run_with(class, Rogue::Stealthed, 2.0, setup);
        r.touches
            .iter()
            .find(|t| t.ends_with(what))
            .cloned()
            .ok_or(format!("{what:?} never reached it: {:?} / {:?}", r.choices, r.touches))
    };
    match ability {
        AbilityType::FrostNova => touched(
            CharacterClass::Mage,
            Setup {
                bystander: true,
                ..Setup::default()
            },
            "caught it in Frost Nova",
        ),
        AbilityType::PsychicScream => touched(
            CharacterClass::Priest,
            Setup {
                bystander: true,
                pressured: true,
                ..Setup::default()
            },
            "applied Fear",
        ),
        AbilityType::DemoralizingShout => touched(
            CharacterClass::Warrior,
            Setup {
                bystander: true,
                prepare: |warrior| warrior.warrior_shout = WarriorShout::DemoralizingShout,
                ..Setup::default()
            },
            "applied AttackPowerReduction",
        ),
        AbilityType::FreezingTrap => spring_trap(TrapType::Freezing),
        AbilityType::FrostTrap => spring_trap(TrapType::Frost),
        other => Err(format!("{other:?} is an area effect with no driver here")),
    }
}

fn world_with_rogue(rogue_pos: Vec3) -> (World, Entity, Entity) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(Time::<()>::default());
    let hunter = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Hunter),
            Transform::from_translation(Vec3::new(-20.0, 0.0, 0.0)),
        ))
        .id();
    let rogue = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Rogue),
            Transform::from_translation(rogue_pos),
        ))
        .id();
    assert!(world.get::<Combatant>(rogue).unwrap().stealthed);
    (world, hunter, rogue)
}

fn stealth_log(world: &World) -> Vec<String> {
    world
        .resource::<CombatLog>()
        .entries
        .iter()
        .map(|e| e.message.clone())
        .filter(|m| m.starts_with("[STEALTH]"))
        .collect()
}

/// A trap springing under a stealthed Rogue, through the real systems: the
/// trap catches it, and the aura the trap lands is what reveals it.
fn spring_trap(trap_type: TrapType) -> Result<String, String> {
    let (mut world, hunter, rogue) = world_with_rogue(Vec3::new(1.0, 0.0, 0.0));
    world.spawn((
        Trap {
            trap_type,
            owner_team: 1,
            owner: hunter,
            arm_timer: 0.0,
            trigger_radius: 3.0,
            triggered: false,
        },
        Transform::default(),
    ));
    world.run_system_once(trap_system).unwrap();
    match trap_type {
        TrapType::Freezing => {
            world.run_system_once(apply_pending_auras).unwrap();
        }
        TrapType::Frost => {
            world.run_system_once(slow_zone_system).unwrap();
        }
    }
    let revealed = !world.get::<Combatant>(rogue).unwrap().stealthed;
    let lines = stealth_log(&world);
    if revealed && lines.len() == 1 {
        Ok(lines[0].clone())
    } else {
        Err(format!("revealed={revealed}, log={lines:?}"))
    }
}

#[test]
fn every_area_effect_reaches_a_stealthed_enemy() {
    use AbilityType::*;
    let areas: Vec<AbilityType> = every_variant::<AbilityType>()
        .into_iter()
        .filter(|a| reaches_enemies_in_an_area(*a))
        .collect();
    // Named, not counted: the set is exactly these, in enum order.
    assert_eq!(
        areas,
        vec![FrostNova, PsychicScream, FreezingTrap, FrostTrap, DemoralizingShout]
    );
    let mut failures = Vec::new();
    for ability in areas {
        if let Err(why) = drive_area_effect(ability) {
            failures.push(format!("{ability:?}: {why}"));
        }
    }
    assert!(
        failures.is_empty(),
        "area effects that did not reach a stealthed Rogue standing in them:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_trap_reveals_through_the_aura_it_lands() {
    assert_eq!(
        spring_trap(TrapType::Freezing),
        Ok("[STEALTH] Team 2 Rogue #1 is revealed by Freezing Trap".to_string())
    );
    assert_eq!(
        spring_trap(TrapType::Frost),
        Ok("[STEALTH] Team 2 Rogue #1 is revealed by Frost Trap".to_string())
    );
}

// ============================================================================
// The aura funnel: a hostile aura that lands reveals; nothing else does
// ============================================================================

fn aura(effect_type: AuraType, name: &str) -> Aura {
    Aura {
        effect_type,
        duration: 8.0,
        ability_name: name.to_string(),
        ..shadow_sight()
    }
}

/// Queue `aura` on a stealthed Rogue, run the real `apply_pending_auras`, and
/// report whether it was revealed and what the stealth log said.
fn land(aura: Aura, fear_dr_immune: bool) -> (bool, Vec<String>) {
    let (mut world, _, rogue) = world_with_rogue(Vec3::ZERO);
    if fear_dr_immune {
        let mut dr = DRTracker::default();
        while !dr.is_immune(DRCategory::Fears) {
            dr.apply(DRCategory::Fears);
        }
        world.entity_mut(rogue).insert(dr);
    }
    world.spawn(AuraPending {
        target: rogue,
        aura,
    });
    world.run_system_once(apply_pending_auras).unwrap();
    (
        !world.get::<Combatant>(rogue).unwrap().stealthed,
        stealth_log(&world),
    )
}

#[test]
fn a_hostile_aura_that_lands_reveals() {
    assert_eq!(
        land(aura(AuraType::Fear, "Psychic Scream"), false),
        (
            true,
            vec!["[STEALTH] Team 2 Rogue #1 is revealed by Psychic Scream".to_string()]
        )
    );
    // Not only crowd control: any hostile effect, e.g. a stat debuff.
    assert!(land(aura(AuraType::AttackPowerReduction, "Demoralizing Shout"), false).0);
}

#[test]
fn a_blocked_or_friendly_aura_does_not_reveal() {
    // Blocked: a Fear the Rogue is DR-immune to never lands.
    assert_eq!(
        land(aura(AuraType::Fear, "Psychic Scream"), true),
        (false, vec![])
    );
    // Friendly: a buff is not hostile.
    assert_eq!(
        land(aura(AuraType::MaxHealthIncrease, "Power Word: Fortitude"), false),
        (false, vec![])
    );
}
