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
//!   does not stop; and among Silence-prevented abilities, exactly the free
//!   ones. A silence stops nothing for a rage or energy user.
//! - **The AI's view**: a silenced Priest can neither dispel nor heal, as the
//!   dispel walk and the team solve ask it.
//! - **A cast in progress** (`process_casting`): a silenced Hunter's Aimed
//!   Shot runs on; the same frame's silence removes a Mage's Frostbolt.
//! - **A channel in progress** (`process_channeling`): a silence stops a Drain
//!   Life; unsilenced, it runs on.
//! - **Starting a cast** (`pre_cast_ok`, through each class AI): staged
//!   matches in which a silenced Hunter begins an Aimed Shot, a silenced
//!   Hunter with Aimed Shot on cooldown fires an instant shot, and a silenced,
//!   rooted Druid shifts into Travel Form. Each asserts that the acting unit
//!   carries its silence throughout, and the Hunter's scenes also show the
//!   same silence refusing a silenced Mage its Frostbolt.

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::{AbilityConfig, Prevention};
use arenasim::states::play_match::class_ai::cast_guard::{own_state_ok, PreCastOpts};
use arenasim::states::play_match::combat_core::{process_casting, process_channeling};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraType, CastingState, ChannelingState, Combatant, GameRng,
    ResourceType,
};
use arenasim::states::play_match::constants::PET_SLOT_BASE;
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

/// The caster `def` is cast by: a combatant of its class, or for a pet
/// ability the pet, built from an owner of its class as the match builds it.
fn caster_of(def: &AbilityConfig) -> Combatant {
    let owner = Combatant::new(1, 0, def.class);
    match def.pet {
        Some(pet) => Combatant::new_pet(1, PET_SLOT_BASE, pet, &owner),
        None => owner,
    }
}

/// A silence never stops a FREE ability, even a Silence-prevented one: among
/// the Silence-prevented abilities of a mana user (pets included), the ones a
/// silenced caster may still use are exactly the ones that cost no mana — the
/// Paladin's three auras and Divine Shield, the Mage's three armors, the
/// Felhunter's Spell Lock and Devour Magic, and the Spider's Web. Every
/// mana-costing one is refused, for the silence alone. That free exemption is
/// the sim's own rule (`AbilityConfig::silence_stops`), not TBC's.
///
/// Divine Shield is in the set for its cost alone: this asks the guard with
/// default options. The Paladin AI also casts it past a silence by a separate
/// bypass (`PreCastOpts::bypass_silence`), which this test does not ask and
/// which would let it through even if it cost mana.
///
/// Mutant killed: `silence_stops` drops its mana condition (the set comes back
/// empty).
#[test]
fn a_silence_never_stops_a_free_ability() {
    let defs = AbilityDefinitions::default();
    let silenced = ActiveAuras {
        auras: vec![silence(SILENCE_SECS)],
    };
    let mut free = BTreeSet::new();
    let mut stopped = BTreeSet::new();
    for (ability, def) in defs.iter() {
        if def.prevention != Prevention::Silence {
            continue;
        }
        let caster = caster_of(def);
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
            stopped.insert(*ability);
        }
    }
    let expected: BTreeSet<_> = [
        AbilityType::DevotionAura,
        AbilityType::ConcentrationAura,
        AbilityType::ShadowResistanceAura,
        AbilityType::DivineShield,
        AbilityType::FrostArmor,
        AbilityType::MageArmorSpell,
        AbilityType::MoltenArmor,
        AbilityType::SpellLock,
        AbilityType::DevourMagic,
        AbilityType::SpiderWeb,
    ]
    .into_iter()
    .collect();
    assert_eq!(
        free, expected,
        "the Silence abilities a silence does not stop"
    );
    for ability in &free {
        assert_eq!(defs.get_unchecked(ability).mana_cost, 0.0, "{ability:?}");
    }
    for ability in &stopped {
        assert!(defs.get_unchecked(ability).mana_cost > 0.0, "{ability:?}");
    }
    assert!(
        stopped.len() > 30,
        "only {} abilities were stopped",
        stopped.len()
    );
}

/// A silence stops nothing for a caster that does not use mana: the caster's
/// half of the rule (`abilities::silence_holds`). No Warrior or Rogue ability
/// is one a silence stops (each is Pacify, None or free), so the guard is
/// asked of every ability a silence DOES stop, for a silenced Warrior and
/// Rogue: none is refused, where a silenced Mage is refused each.
///
/// Mutant killed: `silence_holds` ignores whether the caster uses mana.
#[test]
fn a_silence_stops_nothing_for_a_rage_or_energy_user() {
    let defs = AbilityDefinitions::default();
    let silenced = ActiveAuras {
        auras: vec![silence(SILENCE_SECS)],
    };
    let opts = PreCastOpts::default();
    let mage = Combatant::new(1, 0, CharacterClass::Mage);
    let mut asked = 0;
    for (ability, def) in defs.iter().filter(|(_, d)| d.silence_stops()) {
        assert!(
            !own_state_ok(*ability, def, &mage, Some(&silenced), opts),
            "control: {ability:?} passes a silenced Mage"
        );
        for class in [CharacterClass::Warrior, CharacterClass::Rogue] {
            let caster = Combatant::new(1, 0, class);
            assert_ne!(caster.resource_type, ResourceType::Mana);
            assert!(
                own_state_ok(*ability, def, &caster, Some(&silenced), opts),
                "{ability:?}: a silenced {class:?} is refused it"
            );
        }
        asked += 1;
    }
    assert!(asked > 30, "only {asked} abilities asked");
}

/// The AI's two context-level silence checks — whether a healer can cast its
/// dispel (`class_ai::can_cast_dispel`, the dispel walk's gate) and whether it
/// can land a heal (`team_solve`'s `can_cast_heal`, read as
/// `SolveUnit::can_cast_heal`) — both refuse a silenced Priest, and both pass
/// the same Priest unsilenced.
///
/// Mutant killed: `can_cast_dispel` ignores the silence. Mutant killed:
/// `can_cast_heal` ignores the silence.
#[test]
fn a_silenced_priest_can_neither_dispel_nor_heal_in_the_ais_view() {
    use arenasim::states::play_match::class_ai::can_cast_dispel;
    use arenasim::states::play_match::class_ai::combat_snapshot::CombatSnapshot;
    use arenasim::states::play_match::class_ai::CombatantInfo;
    use arenasim::states::play_match::team_solve::world_from_context;
    use std::collections::BTreeMap;

    let defs = AbilityDefinitions::default();
    let view = |silenced: bool| -> (bool, bool) {
        let mut world = World::new();
        let priest = world.spawn_empty().id();
        let combatant = Combatant::new(1, 0, CharacterClass::Priest);
        let auras = if silenced {
            vec![silence(SILENCE_SECS)]
        } else {
            Vec::new()
        };
        let snapshot = CombatSnapshot {
            combatants: BTreeMap::from([(
                priest,
                CombatantInfo::from_combatant(
                    priest,
                    &combatant,
                    &Transform::default(),
                    None,
                    None,
                    None,
                ),
            )]),
            active_auras: BTreeMap::from([(priest, auras)]),
            dr_trackers: BTreeMap::new(),
            ability_cooldowns: BTreeMap::new(),
            obstacles: Vec::new(),
            bounds: Default::default(),
            ai_profile: Default::default(),
        };
        let ctx = snapshot.context_for(priest);
        let dispel = can_cast_dispel(&ctx, &defs, priest, AbilityType::DispelMagic);
        let heal = world_from_context(&ctx, 40.0, 30.0, None)
            .units
            .iter()
            .find(|u| u.entity == priest)
            .expect("the Priest is a solve unit")
            .can_cast_heal;
        (dispel, heal)
    };
    assert_eq!(view(true), (false, false), "the silenced Priest");
    assert_eq!(view(false), (true, true), "control: the Priest unsilenced");
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

/// A Warlock mid-Drain-Life, silenced or not, through one frame of
/// `process_channeling`. Returns whether the channel is still up, and whether
/// the log says the silence interrupted it.
fn channeling_through(silenced: bool) -> (bool, bool) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);
    let defs = AbilityDefinitions::default();
    let drain = defs.get_unchecked(&AbilityType::DrainLife).clone();
    world.insert_resource(defs);
    let target = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::from_xyz(20.0, 0.0, 0.0),
        ))
        .id();
    let auras = if silenced {
        vec![silence(SILENCE_SECS)]
    } else {
        Vec::new()
    };
    let caster = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warlock),
            Transform::default(),
            ChannelingState {
                ability: AbilityType::DrainLife,
                duration_remaining: drain.channel_duration.expect("Drain Life is a channel"),
                time_until_next_tick: drain.channel_tick_interval,
                tick_interval: drain.channel_tick_interval,
                target,
                interrupted: false,
                interrupted_display_time: 0.0,
                ticks_applied: 0,
            },
            ActiveAuras { auras },
        ))
        .id();
    world.run_system_once(process_channeling).unwrap();
    let up = world.get::<ChannelingState>(caster).is_some();
    let logged = world
        .resource::<CombatLog>()
        .entries
        .iter()
        .any(|e| e.message.contains("Drain Life interrupted by Silence"));
    (up, logged)
}

/// A silence stops a Drain Life channel in progress, and logs it; the same
/// channel unsilenced runs on.
///
/// Mutant killed: `process_channeling` never stops a channel for a silence
/// (its `silence_stops()` call unwired).
#[test]
fn a_silence_stops_a_drain_life_channel_in_progress() {
    assert_eq!(
        channeling_through(true),
        (false, true),
        "the silence stops the Drain Life channel"
    );
    assert_eq!(
        channeling_through(false),
        (true, false),
        "control: the unsilenced channel runs on"
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

/// The non-pet unit of `team` carries its staged Silence on every frame of the
/// window the decisions are read over ([`silenced_decisions`]): the unit's own
/// control that it was silenced while it acted, read from its state, so it
/// holds whatever order its rotation asks its abilities in. Fails when the
/// stage's silence on that unit is removed.
fn assert_carries_silence(played: &Played, team: u8) {
    let landed = played.landed().app;
    let window: Vec<_> = played
        .staged_frames()
        .filter(|f| f.sim_time < landed + SILENCE_SECS - 0.5)
        .collect();
    assert!(
        window.len() > 60 * 7,
        "only {} frames in the window",
        window.len()
    );
    for frame in window {
        let unit = frame
            .combatants
            .values()
            .find(|c| c.team == team && c.slot == 0 && !c.is_pet)
            .expect("the unit");
        assert!(
            unit.aura_types.contains(&AuraType::Silence),
            "team {team}'s unit is not silenced at {:.2}s",
            frame.sim_time
        );
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
/// completes). Mutant killed: the stage does not silence the Hunter (it does
/// not carry the silence).
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
    assert_carries_silence(&played, 1);
    assert_mage_held(&played);
}

/// With Aimed Shot on cooldown, a silenced Hunter fires its instant shots.
///
/// Mutant killed: the pre-cast guard refuses a silenced caster any
/// mana-costing ability (no shot is chosen, and each is refused). Mutant
/// killed: the stage does not silence the Hunter (it does not carry the
/// silence).
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
    assert_carries_silence(&played, 1);
    assert_mage_held(&played);
}

/// A silenced, rooted Druid with a Warrior on it shifts into Travel Form,
/// carrying the silence throughout, and casts nothing else.
///
/// Mutant killed: the pre-cast guard refuses a silenced caster any
/// mana-costing ability (Travel Form is refused for the silence). Mutant
/// killed: the stage does not silence the Druid (it does not carry the
/// silence).
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
    assert_carries_silence(&played, 1);
    assert!(
        cast.iter().all(|a| *a == "TravelForm"),
        "control: the silenced Druid cast {cast:?}"
    );
}

/// A log line for an Aimed Shot that landed.
fn aimed_shot_landed(line: &str) -> bool {
    line.contains("Aimed Shot") && line.contains("Mage") && line.contains(" hits ")
}
