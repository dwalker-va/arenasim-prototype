//! A silence stops only what TBC's `PreventionType` says it stops (AS-291).
//!
//! Every ability carries its TBC `prevention` (`abilities.ron`, checked
//! against the client by
//! `docs/design/balance/sweeps/2026-10-12-as287-tbc-interruptibility.py`). A
//! silence stops a Silence-prevented ability that costs mana — begun or
//! already being cast — and nothing else. So a silenced Hunter still shoots
//! (Aimed Shot, Arcane Shot, Concussive Shot, Serpent Sting and Disengage are
//! Pacify), and a silenced Druid still shifts (Travel Form is None).
//!
//! - **The book**: exactly those six mana-costing abilities are ones a silence
//!   does not stop.
//! - **A cast in progress** (`process_casting`): a silenced Hunter's Aimed
//!   Shot runs on; the same frame's silence removes a Mage's Frostbolt.
//! - **Starting a cast** (`pre_cast_ok`, through each class AI): staged
//!   matches in which a silenced Hunter begins an Aimed Shot, a silenced
//!   Hunter with Aimed Shot on cooldown fires an instant shot, and a silenced,
//!   rooted Druid shifts into Travel Form — each beside a Silence spell the
//!   same silence refuses.

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::Prevention;
use arenasim::states::play_match::class_ai::cast_guard::{own_state_ok, PreCastOpts};
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraType, CastingState, Combatant, GameRng, ResourceType,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{AbilityDefinitions, AbilityType};
use common::staged::{entity_id, play, sim_time, Played};

/// A silence long enough to cover every window these tests read.
const SILENCE_SECS: f32 = 8.0;

fn silence(duration: f32) -> Aura {
    Aura {
        effect_type: AuraType::Silence,
        duration,
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

// ── the book ────────────────────────────────────────────────────────────────

/// The abilities a silenced caster may still use, among those a silence could
/// touch at all (a mana user's mana-costing abilities), are exactly the
/// Hunter's five Pacify shots and the Druid's Travel Form — asked of the same
/// guard every class AI asks, for a silenced caster of the owning class. Every
/// other such ability is refused, and each is refused for the silence alone:
/// the same caster unsilenced passes.
///
/// Mutant killed: `own_state_ok` checks silence on `mana_cost` alone (the set
/// comes back empty). Mutant killed: `silence_stops` ignores `prevention`.
#[test]
fn a_silence_stops_every_mana_ability_but_the_pacify_shots_and_travel_form() {
    let defs = AbilityDefinitions::default();
    let silenced = ActiveAuras {
        auras: vec![silence(SILENCE_SECS)],
    };
    let mut free = BTreeSet::new();
    let mut stopped = 0;
    for (ability, def) in defs.iter() {
        if def.is_pet_ability() || def.mana_cost <= 0.0 {
            continue;
        }
        let caster = Combatant::new(1, 0, def.class);
        if caster.resource_type != ResourceType::Mana {
            continue;
        }
        let opts = PreCastOpts::default();
        assert!(
            own_state_ok(*ability, def, &caster, None, opts),
            "{ability:?}: the unsilenced control is refused"
        );
        if own_state_ok(*ability, def, &caster, Some(&silenced), opts) {
            free.insert(*ability);
        } else {
            stopped += 1;
        }
    }
    let expected: BTreeSet<_> = [
        AbilityType::AimedShot,
        AbilityType::ArcaneShot,
        AbilityType::ConcussiveShot,
        AbilityType::SerpentSting,
        AbilityType::Disengage,
        AbilityType::TravelForm,
    ]
    .into_iter()
    .collect();
    assert_eq!(free, expected, "the abilities a silence does not stop");
    assert!(stopped > 30, "only {stopped} abilities were stopped");
    for ability in &expected {
        assert_ne!(
            defs.get_unchecked(ability).prevention,
            Prevention::Silence,
            "{ability:?}"
        );
    }
}

// ── a cast in progress ──────────────────────────────────────────────────────

/// A `class` caster mid-`cast`, silenced, through one frame of
/// `process_casting`. Returns whether the cast is still up, and whether the
/// log says the silence interrupted it.
fn silenced_mid(class: CharacterClass, cast: AbilityType) -> (bool, bool) {
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
    let target = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::from_xyz(20.0, 0.0, 0.0),
        ))
        .id();
    let caster = world
        .spawn((
            Combatant::new(1, 0, class),
            Transform::default(),
            CastingState::new(cast, target, cast_time),
            ActiveAuras {
                auras: vec![silence(SILENCE_SECS)],
            },
        ))
        .id();
    world.run_system_once(process_casting).unwrap();
    let up = world.get::<CastingState>(caster).is_some();
    let logged = world
        .resource::<CombatLog>()
        .entries
        .iter()
        .any(|e| e.message.contains("interrupted by Silence"));
    (up, logged)
}

/// A silence does not stop an Aimed Shot already being cast; the same silence
/// stops a Frostbolt.
///
/// Mutant killed: `process_casting` stops a cast for a silence on `mana_cost`
/// alone (the Aimed Shot is cut). Mutant killed: it never stops one (the
/// Frostbolt runs on).
#[test]
fn a_silence_lets_an_aimed_shot_in_progress_run_on_and_stops_a_frostbolt() {
    assert_eq!(
        silenced_mid(CharacterClass::Hunter, AbilityType::AimedShot),
        (true, false),
        "the Aimed Shot runs on through the silence"
    );
    assert_eq!(
        silenced_mid(CharacterClass::Mage, AbilityType::Frostbolt),
        (false, true),
        "control: the silence stops a Frostbolt"
    );
}

// ── staged matches ──────────────────────────────────────────────────────────

const ONE: Unit = Unit::new(1, 0);
const TWO: Unit = Unit::new(2, 0);

fn config(team1: &str, team2: &str) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec![team1.into()],
        team2: vec![team2.into()],
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

/// The log's lines from the stage on, up to `secs` after it.
fn log_within(played: &Played, secs: f32) -> Vec<&str> {
    let landed = played.landed();
    played
        .log
        .lines()
        .filter(|l| {
            log_time(l).is_some_and(|t| t >= landed.app - LOG_STAMP && t <= landed.app + secs)
        })
        .collect()
}

/// The ability decisions `unit` made while its staged silence was up.
fn silenced_decisions(played: &Played, team: u8) -> Vec<&serde_json::Value> {
    let me = entity_id(played, team, 0);
    let start = played
        .staged_trace()
        .next()
        .map(sim_time)
        .expect("the staged match traced nothing");
    played
        .staged_trace()
        .filter(|e| e["actor"]["entity_id"] == me && e["kind"] == "ability_decision")
        .filter(|e| sim_time(e) < start + SILENCE_SECS - 0.5)
        .collect()
}

fn chosen(e: &serde_json::Value) -> Option<&str> {
    (e["outcome"]["type"] == "action_taken")
        .then(|| e["outcome"]["ability"].as_str())
        .flatten()
}

/// Every ability the decisions refused for a silence or a lockout.
fn refused_for_silence<'a>(decisions: &[&'a serde_json::Value]) -> BTreeSet<&'a str> {
    decisions
        .iter()
        .flat_map(|e| e["candidates"].as_array().into_iter().flatten())
        .filter(|c| c["reason"].get("SilencedOrLocked").is_some())
        .filter_map(|c| c["ability"].as_str())
        .collect()
}

/// Hunter and Mage at 25 yards, each targeting the other, both silenced. With
/// `aimed_ready`, Aimed Shot is the Hunter's only shot off cooldown; without
/// it, Aimed Shot is on cooldown for the whole silence and the instants are
/// ready.
fn hunter_v_mage(aimed_ready: bool) -> Stage {
    let stage = Stage::new()
        .place(ONE, 0.0, 0.0)
        .place(TWO, 25.0, 0.0)
        .clear_auras(ONE)
        .clear_auras(TWO)
        .target(ONE, TWO)
        .target(TWO, ONE)
        .aura(ONE, silence(SILENCE_SECS))
        .aura(TWO, silence(SILENCE_SECS))
        .combatant(ONE, |c| c.global_cooldown = 0.0)
        .combatant(TWO, |c| c.global_cooldown = 0.0);
    if aimed_ready {
        [
            AbilityType::ArcaneShot,
            AbilityType::ConcussiveShot,
            AbilityType::SerpentSting,
        ]
        .into_iter()
        .fold(stage, |s, shot| s.cooldown(ONE, shot, SILENCE_SECS + 2.0))
    } else {
        stage.cooldown(ONE, AbilityType::AimedShot, SILENCE_SECS + 2.0)
    }
}

/// The Mage, silenced in every scene, refuses Frostbolt for the silence and
/// casts nothing a silence stops: the control that the silence is live.
fn assert_mage_held(played: &Played) {
    let mage = silenced_decisions(played, 2);
    assert!(
        refused_for_silence(&mage).contains("Frostbolt"),
        "control: the silenced Mage was never refused Frostbolt for the silence"
    );
    let cast: Vec<_> = mage.iter().filter_map(|e| chosen(e)).collect();
    assert!(
        !cast.contains(&"Frostbolt"),
        "control: the silenced Mage cast {cast:?}"
    );
}

/// A silenced Hunter begins an Aimed Shot, and the cast completes while the
/// silence is still up.
///
/// Mutant killed: the pre-cast guard refuses a silenced caster any
/// mana-costing ability (the Hunter never begins it). Mutant killed:
/// `process_casting` stops a cast for a silence on `mana_cost` alone (it never
/// completes).
#[test]
fn a_silenced_hunter_begins_an_aimed_shot_and_it_lands() {
    let played = play(config("Hunter", "Mage"), hunter_v_mage(true));
    let hunter = silenced_decisions(&played, 1);
    assert_eq!(
        hunter.iter().find_map(|e| chosen(e)),
        Some("AimedShot"),
        "the silenced Hunter's first action"
    );
    assert!(
        !refused_for_silence(&hunter).contains("AimedShot"),
        "Aimed Shot was refused for the silence"
    );
    let log = log_within(&played, SILENCE_SECS - 0.5);
    assert!(
        !log.iter().any(|l| l.contains("Aimed Shot interrupted")),
        "the Aimed Shot was stopped:\n{}",
        log.join("\n")
    );
    assert!(
        log.iter().any(|l| aimed_shot_landed(l)),
        "the Aimed Shot never landed while the Hunter was silenced:\n{}",
        log.join("\n")
    );
    assert_mage_held(&played);
}

/// With Aimed Shot on cooldown, a silenced Hunter fires its instant shots.
///
/// Mutant killed: the pre-cast guard refuses a silenced caster any
/// mana-costing ability (no shot is chosen, and each is refused).
#[test]
fn a_silenced_hunter_fires_an_instant_shot() {
    let played = play(config("Hunter", "Mage"), hunter_v_mage(false));
    let hunter = silenced_decisions(&played, 1);
    const SHOTS: [&str; 3] = ["ArcaneShot", "ConcussiveShot", "SerpentSting"];
    let shots: Vec<_> = hunter
        .iter()
        .filter_map(|e| chosen(e))
        .filter(|a| SHOTS.contains(a))
        .collect();
    assert!(
        !shots.is_empty(),
        "the silenced Hunter fired no instant shot; it chose {:?}",
        hunter.iter().filter_map(|e| chosen(e)).collect::<Vec<_>>()
    );
    let refused = refused_for_silence(&hunter);
    for shot in SHOTS {
        assert!(!refused.contains(shot), "{shot} refused for the silence");
    }
    assert_mage_held(&played);
}

/// A silenced, rooted Druid with a Warrior on it shifts into Travel Form,
/// though the silence refuses its spells.
///
/// Mutant killed: the pre-cast guard refuses a silenced caster any
/// mana-costing ability (Travel Form is refused for the silence).
#[test]
fn a_silenced_rooted_druid_shifts_into_travel_form() {
    let stage = Stage::new()
        .place(ONE, 0.0, 0.0)
        .place(TWO, 1.5, 0.0)
        .clear_auras(ONE)
        .clear_auras(TWO)
        .target(ONE, TWO)
        .target(TWO, ONE)
        .resource(TWO, 60.0)
        .aura(ONE, silence(SILENCE_SECS))
        .aura(
            ONE,
            Aura {
                effect_type: AuraType::Root,
                duration: SILENCE_SECS,
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        )
        .combatant(ONE, |c| c.global_cooldown = 0.0);
    let played = play(config("Druid", "Warrior"), stage);
    let druid = silenced_decisions(&played, 1);
    let cast: Vec<_> = druid.iter().filter_map(|e| chosen(e)).collect();
    assert!(
        cast.contains(&"TravelForm"),
        "the silenced, rooted Druid never shifted; it chose {cast:?}"
    );
    let refused = refused_for_silence(&druid);
    assert!(
        !refused.contains("TravelForm"),
        "Travel Form refused for the silence"
    );
    assert!(
        !refused.is_empty(),
        "control: the silence refused the Druid nothing"
    );
    assert!(
        cast.iter().all(|a| *a == "TravelForm"),
        "control: the silenced Druid cast {cast:?}"
    );
}

/// A log line for an Aimed Shot that landed.
fn aimed_shot_landed(line: &str) -> bool {
    line.contains("Aimed Shot") && line.contains("Mage") && line.contains(" hits ")
}
