//! Every damaging instant lands on a target that is casting or channeling
//! (AS-488).
//!
//! A strike resolved through `decide_abilities`' own query, which leaves out
//! every unit mid-cast or mid-channel, found no target on a caster: no damage,
//! no log line, no proc, while its attacker had already paid the resource and
//! the cooldown. About one Mortal or Sinister Strike in five vanished that way.
//! The queued strikes and area effects now land in `land_instant_effects`, over
//! every combatant.
//!
//! Two questions, so a new instant cannot bring the bug back:
//!
//! 1. **Which route does each damaging instant land through?** The set is
//!    DERIVED from the ability table (no cast time, some direct damage), and
//!    [`ROUTES`] must name exactly that set. A new one (a talent's strike)
//!    fails here until it is routed.
//! 2. **Does each land on a casting and on a channeling target?** Every
//!    member is put through its route in a staged match: the effect queued as
//!    its route queues it, against a Warlock held mid-Shadow Bolt, mid-Drain
//!    Life, or idle (the control). It must land (a damage line naming it and
//!    the target), and the target must still be in its cast on every frame up
//!    to the hit, so the case cannot pass by the cast ending first.
//!
//! Beside them: Mortal Strike chosen by the Warrior's own AI on a casting
//! Mage lands, logs and fires its `SpellHit` proc (an idle Mage the control);
//! Holy Nova's queued heal lands on a casting teammate; and Divine Shield
//! still halves a queued strike, read as the strike is queued.
//!
//! The route a class AI queues an ability through is the class AI's: this
//! file states it and puts the ability through it, it cannot read it. A strike
//! queued through `QueuedInstantAttack` is a `Strike`.

mod common;

use std::collections::BTreeSet;

use bevy::prelude::*;

use arenasim::headless::{FrameObservation, HeadlessMatchConfig, ObservedCombatant, Stage, Unit};
use arenasim::states::match_config::RoguePoison;
use arenasim::states::play_match::abilities::AbilityType::{self, *};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::{QueuedAoeDamage, QueuedAoeHeal, QueuedInstantAttack};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, CastingState, ChannelingState, Combatant, DispelType,
    HolyShockDamagePending, PlayMatchEntity, Projectile,
};
use arenasim::states::play_match::instant_landings::{
    InstantLandings, PendingAreaHit, PendingStrike,
};
use arenasim::states::play_match::talent_procs::{
    ProcChance, ProcEffect, ProcRankScale, ProcTarget, TalentProc, TalentProcSlot, TalentTrigger,
};
use arenasim::states::play_match::talents::TalentId;
use common::staged::{play, Played};

/// How a damaging instant lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// Queued by the class AI as a `QueuedInstantAttack`, landed by
    /// `land_instant_effects`.
    Strike,
    /// Queued as a `QueuedAoeDamage` per victim, landed by
    /// `land_instant_effects`.
    Area,
    /// A zero-length `CastingState` on the caster, landed by
    /// `process_casting`.
    InstantCast,
    /// A `Projectile`, landed by `process_projectile_hits`.
    Projectile,
    /// A `HolyShockDamagePending`, landed by `process_holy_shock_damage`.
    HolyShock,
    /// A `ChannelingState` on the caster, ticked by `process_channeling`.
    Channel,
    /// A weapon poison, rolled on a landed swing in `combat_auto_attack`.
    WeaponProc,
}

/// Every damaging instant in the ability table, with its route.
const ROUTES: &[(AbilityType, Route)] = &[
    (MortalStrike, Route::Strike),
    (Ambush, Route::Strike),
    (SinisterStrike, Route::Strike),
    (CrusaderStrike, Route::Strike),
    (Hemorrhage, Route::Strike),
    (FrostNova, Route::Area),
    (HolyNova, Route::Area),
    (Shadowburn, Route::InstantCast),
    (Conflagrate, Route::InstantCast),
    (FrostShock, Route::InstantCast),
    (Moonfire, Route::InstantCast),
    (DeathCoil, Route::Projectile),
    (ArcaneShot, Route::Projectile),
    (HolyShock, Route::HolyShock),
    (DrainLife, Route::Channel),
    (WoundPoison, Route::WeaponProc),
];

/// The damaging instants, read off the table: no cast time and some direct
/// damage, base or scaled. Talent abilities included.
fn damaging_instants() -> BTreeSet<AbilityType> {
    AbilityDefinitions::shipped()
        .iter()
        .filter(|(_, cfg)| {
            cfg.cast_time == 0.0 && (cfg.damage_base_max > 0.0 || cfg.damage_coefficient > 0.0)
        })
        .map(|(ability, _)| *ability)
        .collect()
}

#[test]
fn every_damaging_instant_names_its_landing_route() {
    let derived = damaging_instants();
    let named: BTreeSet<AbilityType> = ROUTES.iter().map(|(a, _)| *a).collect();
    assert_eq!(
        named.len(),
        ROUTES.len(),
        "an ability is routed twice: {ROUTES:?}"
    );
    let unrouted: Vec<_> = derived.difference(&named).collect();
    let stale: Vec<_> = named.difference(&derived).collect();
    assert!(
        unrouted.is_empty() && stale.is_empty(),
        "damaging instants with no route: {unrouted:?}; routed but not a damaging \
         instant: {stale:?}. Name the route each lands through in ROUTES. A strike a \
         class AI queues goes through `QueuedInstantAttack`, which \
         `land_instant_effects` lands on every combatant, casting or not (AS-488)."
    );
}

/// What the target is doing when the effect lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Doing {
    Idle,
    Casting,
    Channeling,
}

const ATTACKER: Unit = Unit::new(1, 0);
const TARGET: Unit = Unit::new(2, 0);
const TARGET_ID: &str = "Team 2 Warlock #1";

/// A cast and a channel long enough to outlast the case.
const HOLD_SECS: f32 = 60.0;

fn owner_class(ability: AbilityType) -> String {
    format!(
        "{:?}",
        AbilityDefinitions::shipped().get_unchecked(&ability).class
    )
}

/// Put `ability` through `route` from the attacker onto the target, as the
/// target does `doing`, and play it.
fn land(ability: AbilityType, route: Route, doing: Doing) -> Played {
    let class = owner_class(ability);
    let config = HeadlessMatchConfig {
        team1: vec![class.clone()],
        team2: vec!["Warlock".into()],
        random_seed: Some(488),
        max_duration_secs: 2.0,
        team1_rogue_poisons: if route == Route::WeaponProc {
            vec!["Wound".into()]
        } else {
            vec![]
        },
        ..Default::default()
    };
    let stage = Stage::new()
        .place(ATTACKER, 0.0, 0.0)
        .place(TARGET, 2.0, 0.0)
        .target(ATTACKER, TARGET)
        .target(TARGET, ATTACKER)
        .world(move |world| {
            let attacker = ATTACKER.entity(world);
            let target = TARGET.entity(world);
            ready_attacker(world, attacker, ability, route);
            hold_target(world, target, attacker, doing);
            queue(world, attacker, target, ability, route);
        });
    play(config, stage)
}

/// Give the attacker the ability (a talent's is granted) and take every
/// interrupt its side has off the table, so nothing but the case touches the
/// target's cast.
fn ready_attacker(world: &mut World, attacker: Entity, ability: AbilityType, route: Route) {
    let defs = AbilityDefinitions::shipped();
    let mut team1 = world.query::<&mut Combatant>();
    for mut c in team1.iter_mut(world) {
        if c.team != 1 {
            continue;
        }
        let interrupts: Vec<AbilityType> = c
            .book()
            .iter()
            .filter(|(_, cfg)| cfg.is_interrupt)
            .map(|(a, _)| *a)
            .collect();
        for a in interrupts {
            c.ability_cooldowns.insert(a, HOLD_SECS);
        }
    }
    let mut c = world.get_mut::<Combatant>(attacker).expect("the attacker");
    // It decides nothing (its global cooldown) and swings nothing (its swing
    // timer), but for the swing a weapon proc rides on.
    c.global_cooldown = HOLD_SECS;
    c.attack_timer = -HOLD_SECS;
    let mut book = c.book().clone();
    if !book.contains(&ability) {
        book = book.granting(defs, ability);
    }
    if route == Route::WeaponProc {
        // Every landed swing applies it, and the swing is ready now.
        book = book.patching(ability, |cfg| cfg.application_chance = Some(1.0));
        c.rogue_poison = RoguePoison::Wound;
        c.attack_timer = 100.0;
    }
    *c = c.clone().with_book(book);
}

fn hold_target(world: &mut World, target: Entity, attacker: Entity, doing: Doing) {
    match doing {
        Doing::Idle => {}
        Doing::Casting => {
            world
                .entity_mut(target)
                .insert(CastingState::new(Shadowbolt, attacker, HOLD_SECS));
        }
        Doing::Channeling => {
            world.entity_mut(target).insert(ChannelingState {
                ability: DrainLife,
                duration_remaining: HOLD_SECS,
                time_until_next_tick: HOLD_SECS,
                tick_interval: 1.0,
                target: attacker,
                interrupted: false,
                interrupted_display_time: 0.0,
                ticks_applied: 0,
            });
        }
    }
}

/// Queue the effect as its route queues it.
fn queue(world: &mut World, attacker: Entity, target: Entity, ability: AbilityType, route: Route) {
    let c = world
        .get::<Combatant>(attacker)
        .expect("the attacker")
        .clone();
    let target_pos = world
        .get::<Transform>(target)
        .expect("a transform")
        .translation;
    let cfg = c.book().get_unchecked(&ability).clone();
    match route {
        Route::Strike => {
            world
                .resource_mut::<InstantLandings>()
                .strikes
                .push(PendingStrike {
                    attack: QueuedInstantAttack {
                        attacker,
                        target,
                        damage: 10.0,
                        attacker_team: c.team,
                        attacker_slot: c.slot,
                        attacker_class: c.class,
                        ability,
                        is_crit: false,
                    },
                    dealt: 1.0,
                });
        }
        Route::Area => {
            world
                .resource_mut::<InstantLandings>()
                .area_damage
                .push(PendingAreaHit {
                    hit: QueuedAoeDamage {
                        caster: attacker,
                        target,
                        damage: 10.0,
                        caster_team: c.team,
                        caster_slot: c.slot,
                        caster_class: c.class,
                        target_pos,
                        is_crit: false,
                        ability,
                        school: cfg.spell_school,
                    },
                    dealt: 1.0,
                });
        }
        Route::InstantCast => {
            world
                .entity_mut(attacker)
                .insert(CastingState::new(ability, target, 0.0));
        }
        Route::Projectile => {
            world.spawn((
                Projectile {
                    caster: attacker,
                    target,
                    ability,
                    speed: cfg.projectile_speed.unwrap_or(40.0),
                    caster_team: c.team,
                    caster_slot: c.slot,
                    caster_class: c.class,
                    caster_pet_type: None,
                    next_cast: None,
                },
                Transform::from_translation(target_pos),
                PlayMatchEntity,
            ));
        }
        Route::HolyShock => {
            world.spawn(HolyShockDamagePending {
                caster: attacker,
                caster_book: c.book().clone(),
                caster_spell_power: c.spell_power,
                caster_crit_chance: 0.0,
                caster_team: c.team,
                caster_slot: c.slot,
                caster_class: c.class,
                target,
                impact_from: Vec3::X,
            });
        }
        Route::Channel => {
            world.entity_mut(attacker).insert(ChannelingState {
                ability,
                duration_remaining: cfg.channel_duration.unwrap_or(5.0),
                time_until_next_tick: 0.0,
                tick_interval: cfg.channel_tick_interval,
                target,
                interrupted: false,
                interrupted_display_time: 0.0,
                ticks_applied: 0,
            });
        }
        // The swing the stage readied lands it.
        Route::WeaponProc => {}
    }
}

/// The app-clock stamp of a log line (`[ 10.02s] ...`).
fn stamp(line: &str) -> Option<f32> {
    let rest = line.strip_prefix('[')?;
    let (t, _) = rest.split_once("s]")?;
    t.trim().parse().ok()
}

/// Why the case failed, or `None` when it landed as it should.
fn verdict(ability: AbilityType, route: Route, doing: Doing) -> Option<String> {
    let played = land(ability, route, doing);
    let name = &AbilityDefinitions::shipped().get_unchecked(&ability).name;
    // The log's stamps run a frame or two behind the app clock the stage
    // landed on; nothing deals damage before the gates, where it lands.
    let from = played.landed().app - 0.1;
    let hit = played.log.lines().find(|l| {
        l.contains("[DMG]")
            && l.contains(name.as_str())
            && l.contains(TARGET_ID)
            && stamp(l).is_some_and(|t| t >= from)
    });
    if hit.is_none() {
        return Some(format!(
            "{ability:?} ({route:?}) never landed on a target {doing:?}:\n{}",
            played.log
        ));
    }
    // The target on each frame.
    let target = |frame: &FrameObservation| -> ObservedCombatant {
        frame
            .combatants
            .values()
            .find(|c| c.team == 2 && c.slot == 0 && !c.is_pet)
            .expect("the target")
            .clone()
    };
    let held = |t: &ObservedCombatant| match doing {
        Doing::Idle => t.casting.is_none() && t.channeling.is_none(),
        Doing::Casting => t.casting == Some(Shadowbolt),
        Doing::Channeling => t.channeling == Some(DrainLife),
    };
    // The hit's frame: the first, from the stage's, on which the target has
    // less health than before the stage. Nothing else of the attacker's
    // acts (`ready_attacker`), so that is the case's effect.
    let stage_frame = played
        .frames
        .iter()
        .position(|f| f.sim_time >= played.landed().app)
        .expect("the stage's frame");
    let before = target(&played.frames[stage_frame - 1]).current_health;
    let Some(hit_frame) = (stage_frame..played.frames.len())
        .find(|&i| target(&played.frames[i]).current_health < before)
    else {
        return Some(format!(
            "{ability:?} ({route:?}) logged a hit on a target {doing:?} that lost no              health:\n{}",
            played.log
        ));
    };
    // The target was in its cast on every frame from the stage's to the
    // hit's, that one included.
    if let Some(i) = (stage_frame..=hit_frame).find(|&i| !held(&target(&played.frames[i]))) {
        return Some(format!(
            "{ability:?} ({route:?}) hit on frame {} of the stage, but the target was \
             not {doing:?} on frame {}",
            hit_frame - stage_frame,
            i - stage_frame
        ));
    }
    None
}

fn every_instant_lands(doing: Doing) {
    let failures: Vec<String> = std::thread::scope(|s| {
        let cases: Vec<_> = ROUTES
            .iter()
            .map(|&(ability, route)| s.spawn(move || verdict(ability, route, doing)))
            .collect();
        cases
            .into_iter()
            .filter_map(|c| c.join().expect("the case runs"))
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The control: every route lands on a target doing nothing.
#[test]
fn every_damaging_instant_lands_on_an_idle_target() {
    every_instant_lands(Doing::Idle);
}

#[test]
fn every_damaging_instant_lands_on_a_casting_target() {
    every_instant_lands(Doing::Casting);
}

#[test]
fn every_damaging_instant_lands_on_a_channeling_target() {
    every_instant_lands(Doing::Channeling);
}

// ---------------------------------------------------------------------------
// The AI's own strike, end to end
// ---------------------------------------------------------------------------

const WARRIOR: Unit = Unit::new(1, 0);
const MAGE: Unit = Unit::new(2, 0);

/// A Warrior beside a Mage, rage full and Mortal Strike ready, deciding on the
/// gate frame; the Mage mid-Frostbolt or idle. The Warrior carries a certain
/// proc on its Mortal Strike's `SpellHit`, so the strike's proc shows in the
/// log. Nothing else of the Warrior's can touch the Mage's cast: Pummel waits,
/// and its swing timer is far off. Rend, which would open, waits too.
fn mortal_strike_on(casting: bool) -> Played {
    mortal_strike_with(casting, false)
}

/// [`mortal_strike_on`], the Warrior under a damage immunity (Divine
/// Shield's) when `shielded`.
fn mortal_strike_with(casting: bool, shielded: bool) -> Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Mage".into()],
        random_seed: Some(488),
        max_duration_secs: 2.0,
        ..Default::default()
    };
    let stage = Stage::new()
        .place(WARRIOR, 0.0, 0.0)
        .place(MAGE, 2.0, 0.0)
        .target(WARRIOR, MAGE)
        .target(MAGE, WARRIOR)
        .resource(WARRIOR, 100.0)
        .cooldown(WARRIOR, Pummel, HOLD_SECS)
        .cooldown(WARRIOR, Rend, HOLD_SECS)
        .combatant(WARRIOR, |c| {
            c.attack_timer = -HOLD_SECS;
            c.talent_procs.slots.push(TalentProcSlot::new(
                TalentId::FelStamina,
                "Test Talent",
                TalentProc {
                    trigger: TalentTrigger::SpellHit,
                    abilities: vec![MortalStrike],
                    crit_only: false,
                    chance: ProcChance::Chance(1.0),
                    ranks_add: ProcRankScale::Chance,
                    effect: ProcEffect::ApplyAura {
                        on: ProcTarget::Owner,
                        aura_type: AuraType::AttackPowerIncrease,
                        magnitude: 1.0,
                        duration: 5.0,
                        tick_interval: 0.0,
                        scope: None,
                        charges: None,
                        dispel_type: DispelType::Physical,
                        name: Some("Strike Proc".into()),
                    },
                    internal_cooldown: 0.0,
                },
            ));
        })
        // The Mage cannot dodge or parry it.
        .combatant(MAGE, |c| {
            c.dodge_chance = 0.0;
            c.parry_chance = 0.0;
        })
        .world(move |world| {
            if shielded {
                let warrior = WARRIOR.entity(world);
                world
                    .get_mut::<ActiveAuras>(warrior)
                    .expect("the Warrior's auras")
                    .auras
                    .push(Aura {
                        effect_type: AuraType::DamageImmunity,
                        duration: HOLD_SECS,
                        break_on_damage_threshold: -1.0,
                        ability_name: "Divine Shield".into(),
                        ..Default::default()
                    });
            }
            if casting {
                let warrior = WARRIOR.entity(world);
                let mage = MAGE.entity(world);
                world
                    .entity_mut(mage)
                    .insert(CastingState::new(Frostbolt, warrior, HOLD_SECS));
            }
        });
    play(config, stage)
}

/// The Warrior chose Mortal Strike on the Mage on the gate frame, it hit, its
/// proc fired, and the Mage was (or was not) mid-Frostbolt through that frame.
fn assert_mortal_strike_lands(casting: bool) {
    let played = mortal_strike_on(casting);
    let chose = played.staged_trace().any(|e| {
        e["kind"] == "ability_decision"
            && e["actor"]["class"] == "Warrior"
            && e["outcome"]["ability"] == "MortalStrike"
            && e["frame"].as_u64() == Some(played.landed().trace_frame)
    });
    assert!(
        chose,
        "the Warrior chose Mortal Strike on the gate frame:\n{}",
        played.log
    );
    let from = played.landed().app - 0.1;
    let after = |needle: &str| {
        played
            .log
            .lines()
            .any(|l| l.contains(needle) && stamp(l).is_some_and(|t| t >= from))
    };
    assert!(
        after("Team 1 Warrior #1's Mortal Strike hits Team 2 Mage #1")
            || after("Team 1 Warrior #1's Mortal Strike CRITS Team 2 Mage #1"),
        "the strike lands and is logged:\n{}",
        played.log
    );
    assert!(
        after("Test Talent procs Strike Proc"),
        "the strike's SpellHit fires its proc:\n{}",
        played.log
    );
    let stage_frame = &played
        .frames
        .iter()
        .find(|f| f.sim_time >= played.landed().app)
        .expect("the stage's frame");
    let mage = stage_frame
        .combatants
        .values()
        .find(|c| c.team == 2 && c.slot == 0 && !c.is_pet)
        .expect("the Mage");
    assert!(
        mage.current_health < mage.max_health,
        "the strike took health on the frame it was chosen"
    );
    assert_eq!(
        mage.casting,
        casting.then_some(Frostbolt),
        "the Mage was {} as it was struck",
        if casting { "mid-Frostbolt" } else { "idle" }
    );
}

#[test]
fn mortal_strike_on_a_casting_mage_lands_logs_and_procs() {
    assert_mortal_strike_lands(true);
}

/// The control: the same strike on the Mage doing nothing.
#[test]
fn mortal_strike_on_an_idle_mage_lands_logs_and_procs() {
    assert_mortal_strike_lands(false);
}

/// The strike's whole damage on the Mage (health plus absorbed) in a log.
fn mortal_strike_total(played: &Played) -> f32 {
    let line = played
        .log
        .lines()
        .find(|l| l.contains("[DMG] Team 1 Warrior #1's Mortal Strike"))
        .unwrap_or_else(|| panic!("a Mortal Strike landed:\n{}", played.log));
    let number = |after: &str| -> f32 {
        let rest = &line[line.find(after).expect("the amount") + after.len()..];
        rest.split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|n| n.parse().ok())
            .expect("a number")
    };
    let absorbed = if line.contains(" absorbed)") {
        number("damage (")
    } else {
        0.0
    };
    number(" for ") + absorbed
}

/// Divine Shield halves the strikes its holder queues: the penalty is read
/// as the strike is queued, from the world its attacker decided in. The
/// control is the same strike unshielded, on the same seed.
#[test]
fn a_divine_shield_halves_a_queued_strike() {
    let open = mortal_strike_total(&mortal_strike_with(false, false));
    let shielded = mortal_strike_total(&mortal_strike_with(false, true));
    assert!(open > 20.0, "a strike worth halving: {open}");
    assert!(
        (shielded - open * 0.5).abs() <= 1.0,
        "shielded {shielded} is half of {open}"
    );
}

// ---------------------------------------------------------------------------
// The queued heal
// ---------------------------------------------------------------------------

/// Holy Nova's heal on a wounded Warlock teammate, queued as the dispatch
/// queues it, while the Warlock is mid-Shadow Bolt (or idle, the control).
/// Returns whether it landed and whether the Warlock was still casting on the
/// frame it landed.
fn holy_nova_heal_on(casting: bool) -> (bool, bool) {
    const PRIEST: Unit = Unit::new(1, 0);
    const WARLOCK: Unit = Unit::new(1, 1);
    const ENEMY: Unit = Unit::new(2, 0);
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warlock".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(488),
        max_duration_secs: 2.0,
        ..Default::default()
    };
    let stage = Stage::new()
        .place(PRIEST, 0.0, 0.0)
        .place(WARLOCK, 2.0, 0.0)
        .place(ENEMY, 30.0, 0.0)
        .health(WARLOCK, 0.5)
        .world(move |world| {
            let priest = PRIEST.entity(world);
            let warlock = WARLOCK.entity(world);
            let enemy = ENEMY.entity(world);
            if casting {
                world
                    .entity_mut(warlock)
                    .insert(CastingState::new(Shadowbolt, enemy, HOLD_SECS));
            }
            // The Priest holds Holy Nova (a talent's) and decides nothing else.
            let mut c = world.get_mut::<Combatant>(priest).expect("the Priest");
            let book = c
                .book()
                .clone()
                .granting(AbilityDefinitions::shipped(), HolyNova);
            c.global_cooldown = HOLD_SECS;
            *c = c.clone().with_book(book);
            let c = c.clone();
            world
                .resource_mut::<InstantLandings>()
                .area_heals
                .push(QueuedAoeHeal {
                    caster: priest,
                    target: warlock,
                    heal: 20.0,
                    is_crit: false,
                    caster_team: c.team,
                    caster_slot: c.slot,
                    caster_class: c.class,
                    ability: HolyNova,
                });
        });
    let played = play(config, stage);
    let from = played.landed().app - 0.1;
    let landed = played.log.lines().any(|l| {
        l.contains("Holy Nova heals Team 1 Warlock #2") && stamp(l).is_some_and(|t| t >= from)
    });
    let warlock_on = |f: &FrameObservation| {
        f.combatants
            .values()
            .find(|c| c.team == 1 && c.slot == 1 && !c.is_pet)
            .expect("the Warlock")
            .clone()
    };
    let stage_frame = played
        .frames
        .iter()
        .position(|f| f.sim_time >= played.landed().app)
        .expect("the stage's frame");
    // The stage left the Warlock at half health; the heal is the first frame
    // above it.
    let before = 0.5 * warlock_on(&played.frames[stage_frame]).max_health;
    let healed_frame = (stage_frame..played.frames.len())
        .find(|&i| warlock_on(&played.frames[i]).current_health > before + 1e-3);
    let held = healed_frame.is_some_and(|h| {
        (stage_frame..=h)
            .all(|i| warlock_on(&played.frames[i]).casting == casting.then_some(Shadowbolt))
    });
    (landed && healed_frame.is_some(), held)
}

#[test]
fn a_queued_heal_lands_on_a_casting_teammate() {
    assert_eq!(holy_nova_heal_on(true), (true, true));
}

/// The control: the same heal on the Warlock doing nothing.
#[test]
fn a_queued_heal_lands_on_an_idle_teammate() {
    assert_eq!(holy_nova_heal_on(false), (true, true));
}
