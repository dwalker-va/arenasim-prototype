//! An uninterruptible cast (AS-287): Aimed Shot.
//!
//! No interrupt takes a cast whose `prevention` is not Silence — Kick,
//! Pummel, Wind Shear and Spell Lock pass it by, and are not spent on it. Loss
//! of control still stops it: a stun, fear or polymorph cancels the cast, as
//! it cancels any cast, and that is not an interrupt, so it locks out no
//! school and the caster's other abilities stay open.
//!
//! - **Resolution**: an interrupt queued at an Aimed Shot neither cuts it nor
//!   locks anything out; the same interrupt at an interruptible cast does both.
//! - **The interrupter's use site** (`check_interrupts`): a Rogue in Kick
//!   range of a Hunter mid-Aimed-Shot does not use Kick — no interrupt queued,
//!   no cooldown, no energy — and traces the pass as `Uninterruptible`.
//! - **Crowd control** (`process_casting`): a stun, polymorph or fear on a
//!   Hunter mid-Aimed-Shot removes the cast and queues no school lockout.
//! - **The text**: the ability's page and tooltips say "Cannot be
//!   interrupted."
//! - **A staged match**: Rogue on a Hunter mid-Aimed-Shot. Kick is never used
//!   on it; a Kidney Shot stops it with no lockout; once the stun ends the
//!   Hunter Disengages, a Physical ability an interrupt would have locked.
//!
//! Each is paired with its control: the same scene with an interruptible cast
//! on the same bar.

mod common;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use std::time::Duration;

use arenasim::states::encyclopedia::abilities::{mechanics_text, summary_sentence};
use arenasim::states::play_match::ability_book::AbilityBook;
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, CastingState, Combatant, GameRng,
    InterruptPending,
};
use arenasim::states::play_match::decision_trace::{
    AbilityOutcome, CandidateStatus, DecisionTrace, EventPayload, RejectionReason,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::systems::{check_interrupts, process_interrupts};
use arenasim::states::play_match::{AbilityDefinitions, AbilityType};
use common::staged::{entity_id, play, sim_time};

/// The cast an interrupter must pass over, and an interruptible one to pair it
/// with: same caster, same bar, the flag the only difference.
const UNINTERRUPTIBLE: AbilityType = AbilityType::AimedShot;
const INTERRUPTIBLE: AbilityType = AbilityType::Frostbolt;

#[test]
fn aimed_shot_is_the_flagged_cast() {
    let defs = AbilityDefinitions::default();
    let aimed = defs.get_unchecked(&UNINTERRUPTIBLE);
    assert!(aimed.is_uninterruptible_cast() && aimed.has_cast_bar());
    assert!(!aimed.can_be_interrupted());
    let frostbolt = defs.get_unchecked(&INTERRUPTIBLE);
    assert!(!frostbolt.is_uninterruptible_cast() && frostbolt.can_be_interrupted());
}

// ── resolution ──────────────────────────────────────────────────────────────

/// The Hunter, mid-`cast`. A unit casts only what is in its book, so the
/// interruptible control (Frostbolt, not a Hunter's) is granted to it.
fn hunter_casting(cast: AbilityType) -> Combatant {
    let book = AbilityBook::shipped(CharacterClass::Hunter);
    let book = if book.contains(&cast) {
        book
    } else {
        book.granting(AbilityDefinitions::shipped(), cast)
    };
    Combatant::new(2, 0, CharacterClass::Hunter).with_book(book)
}

/// A Hunter mid-`cast`, and a Kick queued at it, resolved. Returns (cast cut?,
/// lockouts queued).
fn resolve_kick_at(cast: AbilityType) -> (bool, usize) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    let defs = AbilityDefinitions::default();
    let cast_time = defs.get_unchecked(&cast).cast_time;
    world.insert_resource(defs);
    let rogue = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Rogue),
            Transform::default(),
        ))
        .id();
    let hunter = world
        .spawn((
            hunter_casting(cast),
            Transform::from_xyz(2.0, 0.0, 0.0),
            CastingState::new(cast, rogue, cast_time),
        ))
        .id();
    world.spawn(InterruptPending {
        caster: rogue,
        target: hunter,
        ability: AbilityType::Kick,
        lockout_duration: 5.0,
    });
    world.run_system_once(process_interrupts).unwrap();

    let cut = world.get::<CastingState>(hunter).unwrap().interrupted;
    let mut q = world.query::<&AuraPending>();
    let lockouts = q
        .iter(&world)
        .filter(|p| p.target == hunter && p.aura.effect_type == AuraType::SpellSchoolLockout)
        .count();
    (cut, lockouts)
}

/// Mutant killed: `can_be_interrupted` ignores the flag — the Aimed Shot is cut
/// and Physical locked out.
#[test]
fn an_interrupt_resolving_on_aimed_shot_neither_cuts_nor_locks() {
    assert_eq!(
        resolve_kick_at(UNINTERRUPTIBLE),
        (false, 0),
        "Aimed Shot is neither cut nor locked out"
    );
    assert_eq!(
        resolve_kick_at(INTERRUPTIBLE),
        (true, 1),
        "control: an interruptible cast is cut and locked out"
    );
}

// ── the use site ────────────────────────────────────────────────────────────

struct UseSite {
    /// Interrupts queued.
    queued: usize,
    kick_on_cooldown: bool,
    energy_spent: f32,
    /// The rejection traced for Kick, if any.
    traced: Option<RejectionReason>,
}

/// A Rogue in melee of a Hunter mid-`cast`, Kick ready: one pass of
/// `check_interrupts`.
fn kick_pass_at(cast: AbilityType) -> UseSite {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(arenasim::states::play_match::components::GameRng::from_seed(0));
    world.insert_resource(DecisionTrace::default());
    let defs = AbilityDefinitions::default();
    let cast_time = defs.get_unchecked(&cast).cast_time;
    world.insert_resource(defs);
    let mut rogue = Combatant::new(1, 0, CharacterClass::Rogue);
    rogue.stealthed = false;
    let energy = rogue.current_mana;
    let rogue = world.spawn((rogue, Transform::default())).id();
    let hunter = world
        .spawn((
            hunter_casting(cast),
            Transform::from_xyz(2.0, 0.0, 0.0),
            CastingState::new(cast, rogue, cast_time),
            ActiveAuras::default(),
        ))
        .id();
    world.get_mut::<Combatant>(rogue).unwrap().target = Some(hunter);
    world.run_system_once(check_interrupts).unwrap();

    let mut q = world.query::<&InterruptPending>();
    let queued = q.iter(&world).count();
    let after = world.get::<Combatant>(rogue).unwrap();
    let kick_on_cooldown = after.ability_cooldowns.contains_key(&AbilityType::Kick);
    let energy_spent = energy - after.current_mana;
    let traced = world
        .resource::<DecisionTrace>()
        .pending_events
        .iter()
        .find_map(|e| match &e.payload {
            EventPayload::Ability { candidates, .. } => candidates
                .iter()
                .find(|c| c.ability == AbilityType::Kick)
                .and_then(|c| c.reason.clone()),
            _ => None,
        });
    UseSite {
        queued,
        kick_on_cooldown,
        energy_spent,
        traced,
    }
}

/// Mutant killed: `cast_is_interruptible` ignores the flag — Kick is queued,
/// put on cooldown and paid for.
/// Mutant killed: the pass is not traced.
#[test]
fn kick_is_not_spent_on_aimed_shot() {
    let site = kick_pass_at(UNINTERRUPTIBLE);
    assert_eq!(site.queued, 0, "no interrupt is queued at Aimed Shot");
    assert!(!site.kick_on_cooldown, "Kick is not put on cooldown");
    assert_eq!(site.energy_spent, 0.0, "Kick's energy is not spent");
    assert!(
        matches!(
            site.traced,
            Some(RejectionReason::Uninterruptible {
                ability: UNINTERRUPTIBLE
            })
        ),
        "the pass is traced as Uninterruptible, got {:?}",
        site.traced
    );

    let control = kick_pass_at(INTERRUPTIBLE);
    assert_eq!(control.queued, 1, "control: Kick is used on a Frostbolt");
    assert!(control.kick_on_cooldown);
    assert!(control.energy_spent > 0.0);
    assert!(control.traced.is_none(), "control: nothing passed over");
}

// ── crowd control ───────────────────────────────────────────────────────────

/// A Hunter mid-`cast`, holding `cc` (or nothing), through one frame of
/// `process_casting`. Returns (cast still up?, lockouts queued on the Hunter).
fn cast_under(cast: AbilityType, cc: Option<AuraType>) -> (bool, usize) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);
    let defs = AbilityDefinitions::default();
    let cast_time = defs.get_unchecked(&cast).cast_time;
    world.insert_resource(defs);
    world.insert_resource(ActiveMapGeometry {
        bounds: Default::default(),
        volumes: Vec::new(),
        cover_anchors: Vec::new(),
    });
    let rogue = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Rogue),
            Transform::from_xyz(20.0, 0.0, 0.0),
        ))
        .id();
    let auras = cc
        .map(|effect_type| Aura {
            effect_type,
            duration: 3.0,
            caster: Some(rogue),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        })
        .into_iter()
        .collect();
    let hunter = world
        .spawn((
            hunter_casting(cast),
            Transform::default(),
            CastingState::new(cast, rogue, cast_time),
            ActiveAuras { auras },
        ))
        .id();
    world.run_system_once(process_casting).unwrap();

    let up = world.get::<CastingState>(hunter).is_some();
    let mut pending = world.query::<&AuraPending>();
    let queued = pending
        .iter(&world)
        .filter(|p| p.target == hunter && p.aura.effect_type == AuraType::SpellSchoolLockout)
        .count();
    let on = world.get::<ActiveAuras>(hunter).map_or(0, |a| {
        a.auras
            .iter()
            .filter(|a| a.effect_type == AuraType::SpellSchoolLockout)
            .count()
    });
    (up, queued + on)
}

/// Loss of control stops an uninterruptible cast and locks out nothing: a
/// stun, a polymorph and a fear each remove the Aimed Shot, and no school
/// lockout is queued or applied on the Hunter. The control: with no crowd
/// control the cast is still up after the frame.
///
/// Mutant killed: `process_casting`'s crowd-control branch queues a Physical
/// `SpellSchoolLockout` when the cast it stops is uninterruptible.
#[test]
fn crowd_control_stops_aimed_shot_and_locks_out_nothing() {
    assert_eq!(
        cast_under(UNINTERRUPTIBLE, None),
        (true, 0),
        "control: with no crowd control the cast runs on"
    );
    for cc in [AuraType::Stun, AuraType::Polymorph, AuraType::Fear] {
        assert_eq!(
            cast_under(UNINTERRUPTIBLE, Some(cc)),
            (false, 0),
            "{cc:?} stops the Aimed Shot with no lockout"
        );
    }
}

// ── the text ────────────────────────────────────────────────────────────────

/// A flagged ability's page and tooltips say so; an unflagged cast's do not.
/// Mutant killed: `cast_sentence` returns `None`, or `mechanics_text` stops
/// composing it, or `summary_sentence` cuts it off the tooltip.
#[test]
fn a_flagged_ability_says_it_cannot_be_interrupted() {
    const SAYS: &str = "Cannot be interrupted.";
    let defs = AbilityDefinitions::default();
    for (ability, flagged) in [(UNINTERRUPTIBLE, true), (INTERRUPTIBLE, false)] {
        let config = defs.get_unchecked(&ability);
        let page = mechanics_text(ability, config);
        let tooltip = summary_sentence(ability, config);
        assert_eq!(page.contains(SAYS), flagged, "{ability:?} page: {page:?}");
        assert_eq!(
            tooltip.contains(SAYS),
            flagged,
            "{ability:?} tooltip: {tooltip:?}"
        );
    }
}

// ── a staged match ──────────────────────────────────────────────────────────

const ROGUE: Unit = Unit::new(1, 0);
const HUNTER: Unit = Unit::new(2, 0);

/// How long the Rogue waits for Kidney Shot: the window in which Kick is ready,
/// in range, and the Hunter is casting.
const KIDNEY_WAIT: f32 = 0.8;

/// Rogue and Hunter in melee, the Hunter `cast_time` into nothing yet — mid a
/// fresh `cast` aimed at the Rogue. The Rogue is out of stealth with full
/// energy and Kick ready; Kidney Shot comes off cooldown after `KIDNEY_WAIT`.
fn stage(cast: AbilityType) -> Stage {
    Stage::new()
        .place(ROGUE, 0.0, 0.0)
        .place(HUNTER, 2.0, 0.0)
        .clear_auras(ROGUE)
        .clear_auras(HUNTER)
        .target(ROGUE, HUNTER)
        .target(HUNTER, ROGUE)
        .cooldown(ROGUE, AbilityType::KidneyShot, KIDNEY_WAIT)
        .combatant(ROGUE, |c| {
            c.stealthed = false;
            c.current_mana = c.max_mana;
            c.global_cooldown = 0.0;
            c.ability_cooldowns.remove(&AbilityType::Kick);
        })
        .combatant(HUNTER, |c| {
            c.ability_cooldowns.remove(&AbilityType::Disengage);
        })
        .world(move |world| {
            let rogue = ROGUE.entity(world);
            let hunter = HUNTER.entity(world);
            let cast_time = world
                .resource::<AbilityDefinitions>()
                .get_unchecked(&cast)
                .cast_time;
            let staged = hunter_casting(cast);
            let mut combatant = world.get_mut::<Combatant>(hunter).expect("the Hunter");
            let granted = combatant.clone().with_book(staged.book().clone());
            *combatant = granted;
            world
                .entity_mut(hunter)
                .insert(CastingState::new(cast, rogue, cast_time));
        })
}

fn config() -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Rogue".into()],
        team2: vec!["Hunter".into()],
        random_seed: Some(1),
        max_duration_secs: 14.0,
        ..Default::default()
    }
}

/// A log line is stamped a frame behind the app clock, to two decimals: a line
/// from the frame the stage lands on reads up to this much before it.
const LOG_STAMP: f32 = 0.05;

/// `[  44.23s] ...` → 44.23, the app clock.
fn log_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
}

/// Mutant killed: `check_interrupts` ignores the flag — the Rogue Kicks the
/// Aimed Shot and the Hunter is locked out of Physical, Disengage with it.
/// Mutant killed: crowd control applies a lockout — the same.
#[test]
fn a_rogue_stuns_an_aimed_shot_it_cannot_kick_and_the_hunter_disengages_after() {
    let played = play(config(), stage(UNINTERRUPTIBLE));
    let landed = played.landed();
    let rogue = entity_id(&played, 1, 0);
    let hunter = entity_id(&played, 2, 0);
    let after_stage = |line: &&str| log_time(line).is_some_and(|t| t >= landed.app - LOG_STAMP);

    // Kick passes the cast over while it can: in range, ready, the cast up.
    let passed: Vec<f32> = played
        .staged_trace()
        .filter(|e| e["actor"]["entity_id"] == rogue)
        .filter(|e| {
            e["candidates"].as_array().is_some_and(|cs| {
                cs.iter().any(|c| {
                    c["ability"] == "Kick"
                        && c["reason"]["Uninterruptible"]["ability"] == "AimedShot"
                })
            })
        })
        .map(sim_time)
        .collect();
    assert!(
        !passed.is_empty(),
        "the Rogue never traced Kick passing over the Aimed Shot"
    );
    assert!(
        !played
            .log
            .lines()
            .filter(after_stage)
            .any(|l| l.contains("interrupts Team 2 Hunter")),
        "an interrupt landed on the Hunter"
    );

    // Kidney Shot stops the cast, by crowd control.
    let kidney = played
        .staged_trace()
        .filter(|e| e["actor"]["entity_id"] == rogue)
        .find(|e| chose(e, "KidneyShot"))
        .map(sim_time)
        .expect("the Rogue Kidney Shots the Hunter");
    let aimed = UNINTERRUPTIBLE_NAME;
    assert!(
        played
            .log
            .lines()
            .filter(after_stage)
            .any(|l| l.contains(&format!("Hunter #1's {aimed} interrupted by crowd control"))),
        "the Kidney Shot did not stop the Aimed Shot:\n{}",
        played.log
    );
    assert!(
        !played
            .log
            .lines()
            .filter(after_stage)
            .any(|l| l.contains("Hunter") && l.contains("locked out")),
        "the Hunter was locked out of a school"
    );

    // Once the stun ends the Hunter is free to Disengage: chosen, and never
    // refused for a lockout.
    let hunter_events: Vec<_> = played
        .staged_trace()
        .filter(|e| e["actor"]["entity_id"] == hunter && e["kind"] == "ability_decision")
        .collect();
    for e in &hunter_events {
        for c in e["candidates"].as_array().into_iter().flatten() {
            assert!(
                c["reason"].get("SilencedOrLocked").is_none(),
                "the Hunter's {} was refused for a lockout at {:.2}s",
                c["ability"],
                sim_time(e)
            );
        }
    }
    let disengage = hunter_events
        .iter()
        .find(|e| chose(e, "Disengage"))
        .map(|e| sim_time(e))
        .expect("the Hunter Disengages once the stun ends");
    assert!(
        disengage > kidney,
        "Disengage at {disengage:.2}s came before the Kidney Shot at {kidney:.2}s"
    );
}

const UNINTERRUPTIBLE_NAME: &str = "Aimed Shot";

/// Whether trace event `e` chose `ability`.
fn chose(e: &serde_json::Value, ability: &str) -> bool {
    e["outcome"]["type"] == "action_taken" && e["outcome"]["ability"] == ability
}

fn outcome_event(outcome: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "outcome": outcome })
}

/// The control: the same Rogue, the same Hunter, an interruptible cast on the
/// bar. Kick takes it at once. Without this, a Rogue that never Kicked
/// anything would pass the test above.
#[test]
fn control_the_same_rogue_kicks_an_interruptible_cast() {
    let played = play(config(), stage(INTERRUPTIBLE));
    let landed = played.landed();
    let kicked = played
        .log
        .lines()
        .filter(|l| log_time(l).is_some_and(|t| t >= landed.app - LOG_STAMP))
        .find(|l| l.contains("interrupts Team 2 Hunter"))
        .and_then(log_time)
        .expect("the Rogue Kicks the Hunter's Frostbolt");
    assert!(
        kicked < landed.app + KIDNEY_WAIT,
        "Kick came at {kicked:.2}s, not inside the window the Aimed Shot test asserts on"
    );
}

/// Every outcome the staged test reads is real trace vocabulary: a chosen
/// ability is `ActionTaken`, a refusal carries the typed reason. Guards the
/// JSON paths above against a schema rename silently making them vacuous.
#[test]
fn trace_vocabulary_the_staged_test_reads() {
    let outcome = serde_json::to_value(AbilityOutcome::ActionTaken {
        ability: AbilityType::KidneyShot,
        target_id: None,
        was_instant: true,
    })
    .unwrap();
    assert!(chose(&outcome_event(outcome), "KidneyShot"));
    let reason = serde_json::to_value(RejectionReason::Uninterruptible {
        ability: AbilityType::AimedShot,
    })
    .unwrap();
    assert_eq!(reason["Uninterruptible"]["ability"], "AimedShot");
    let _ = CandidateStatus::Rejected;
}
