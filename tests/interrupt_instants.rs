//! Nothing interrupts an instant (AS-199).
//!
//! Several instants reach the generic completion path through a ZERO-LENGTH
//! `CastingState` — the Druid's heals and Moonfire, the Shaman's Frost Shock —
//! and for one frame that state looked like a cast to every interrupter.
//! Kick, Pummel, Wind Shear and Spell Lock all took it, and the victim was
//! locked out of the instant's school as if a cast bar had been cut.
//!
//! The rule is [`AbilityConfig::has_cast_bar`], read from each ability's own
//! `cast_time`, so the instants below are DERIVED from `abilities.ron`, never
//! listed. Two kinds of probe:
//!
//! - **Match probes** play seeded matches in which, before the fix, each of the
//!   four interrupters interrupted a Druid instant, and assert from the match
//!   log that none does now. Each counts its opportunities — a Druid instant
//!   cast with that interrupter in its interrupt's range — and fails if a seed
//!   shift empties them. A paired control plays the same interrupter against a
//!   caster with a cast bar and asserts it still interrupts.
//! - **A resolution probe** queues an interrupt at a zero-length cast directly,
//!   as any path other than the AI's might, and asserts it cuts nothing and
//!   locks nothing, paired with the same interrupt at a cast with a cast bar.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{run_headless_match_observed, FrameObservation, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::components::{
    AuraPending, AuraType, CastingState, Combatant, InterruptPending,
};
use arenasim::states::play_match::systems::process_interrupts;
use arenasim::states::play_match::{AbilityDefinitions, AbilityType};

// ── match probes ────────────────────────────────────────────────────────────

struct Played {
    frames: Vec<FrameObservation>,
    log: String,
}

fn play(team1: &str, team2: &str, seed: u64) -> Played {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_path = dir.path().join("match.txt");
    let config = HeadlessMatchConfig {
        team1: vec![team1.to_string()],
        team2: vec![team2.to_string()],
        map: "BasicArena".to_string(),
        max_duration_secs: 120.0,
        random_seed: Some(seed),
        output_path: Some(log_path.to_string_lossy().to_string()),
        ..Default::default()
    };
    let mut frames = Vec::new();
    run_headless_match_observed(config, false, None, |f| frames.push(f.clone()))
        .expect("match runs");
    let log = std::fs::read_to_string(&log_path).expect("match log written");
    Played { frames, log }
}

/// `[  44.23s] ...` → 44.23. Log time and `sim_time` share a clock.
fn log_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
}

/// Every instant's display name: no cast bar, not a channel. Derived from the
/// shipped config, so a new instant is covered without an edit here.
fn instant_names(defs: &AbilityDefinitions) -> Vec<String> {
    let names: Vec<String> = defs
        .iter()
        .filter(|(_, d)| !d.has_cast_bar() && !d.is_channel())
        .map(|(_, d)| d.name.clone())
        .collect();
    for routed in [
        AbilityType::Lifebloom,
        AbilityType::Rejuvenation,
        AbilityType::Swiftmend,
        AbilityType::Moonfire,
        AbilityType::FrostShock,
    ] {
        assert!(
            names.contains(&defs.get_unchecked(&routed).name),
            "{routed:?} is an instant"
        );
    }
    names
}

/// The interrupt `class` brings, and whether its pet (the Felhunter) carries it.
fn interrupt_of(class: CharacterClass) -> (AbilityType, bool) {
    match class {
        CharacterClass::Warrior => (AbilityType::Pummel, false),
        CharacterClass::Rogue => (AbilityType::Kick, false),
        CharacterClass::Shaman => (AbilityType::WindShear, false),
        CharacterClass::Warlock => (AbilityType::SpellLock, true),
        other => panic!("{other:?} has no interrupt"),
    }
}

/// The interrupt lines of a log: `<interrupter> interrupts <victim>'s <spell> - ...`.
fn interrupted_spells(log: &str) -> Vec<(f32, String)> {
    log.lines()
        .filter_map(|line| {
            let (_, after) = line.split_once(" interrupts ")?;
            let (_, spell) = after.split_once("'s ")?;
            let spell = spell.split(" - ").next()?.to_string();
            Some((log_time(line).unwrap_or(f32::NAN), spell))
        })
        .collect()
}

/// Druid instant casts made with the interrupter (team 1) within its
/// interrupt's range on that frame — each an interrupt the old code could take.
fn opportunities(
    played: &Played,
    class: CharacterClass,
    instants: &[String],
    defs: &AbilityDefinitions,
) -> usize {
    let (interrupt, on_pet) = interrupt_of(class);
    let range = defs.get_unchecked(&interrupt).range;
    played
        .log
        .lines()
        .filter(|line| {
            line.contains("Team 2 Druid #1 casts ")
                && instants
                    .iter()
                    .any(|name| line.contains(&format!(" casts {name} ")))
        })
        .filter_map(log_time)
        .filter(|&t| {
            let Some(frame) = played
                .frames
                .iter()
                .min_by(|a, b| (a.sim_time - t).abs().total_cmp(&(b.sim_time - t).abs()))
            else {
                return false;
            };
            let druid = frame
                .combatants
                .values()
                .find(|c| c.team == 2 && c.class == CharacterClass::Druid && !c.is_pet);
            let interrupter = frame
                .combatants
                .values()
                .find(|c| c.team == 1 && c.class == class && c.is_pet == on_pet && c.alive);
            match (druid, interrupter) {
                (Some(d), Some(i)) => {
                    Vec2::new(d.position.x, d.position.z)
                        .distance(Vec2::new(i.position.x, i.position.z))
                        <= range
                }
                _ => false,
            }
        })
        .count()
}

/// Each interrupter against a lone Druid, at a seed where — before this rule
/// — it interrupted a Druid instant: Pummel took Lifebloom, Kick took
/// Rejuvenation, Wind Shear took Moonfire and Spell Lock took Lifebloom. None
/// interrupts an instant now, and each match still holds a Druid instant cast
/// inside the interrupter's reach.
///
/// Mutant killed: `has_cast_bar` returns `true` — every one of these matches
/// logs `interrupts Team 2 Druid #1's <instant>` again.
/// Mutant killed: the AI's candidate scans skip `cast_is_interruptible` but
/// resolution keeps it — nothing is cut, but the interrupt is USED on the
/// instant and wasted, so its uses outnumber its interrupts.
#[test]
fn no_interrupter_interrupts_a_druid_instant_in_play() {
    let defs = AbilityDefinitions::default();
    let instants = instant_names(&defs);
    for (class, seed) in [
        (CharacterClass::Warrior, 1),
        (CharacterClass::Rogue, 1),
        (CharacterClass::Shaman, 2),
        (CharacterClass::Warlock, 1),
    ] {
        let played = play(class.name(), "Druid", seed);
        let wrong: Vec<_> = interrupted_spells(&played.log)
            .into_iter()
            .filter(|(_, spell)| instants.contains(spell))
            .collect();
        assert!(
            wrong.is_empty(),
            "{class:?} (seed {seed}) interrupted an instant: {wrong:?}"
        );
        // Nor is the interrupt spent on one and wasted: every use cuts a cast.
        let (interrupt, _) = interrupt_of(class);
        let uses = played
            .log
            .lines()
            .filter(|l| l.ends_with(&format!(" uses {}", defs.get_unchecked(&interrupt).name)))
            .count();
        assert_eq!(
            uses,
            interrupted_spells(&played.log).len(),
            "{class:?} (seed {seed}) spent {interrupt:?} on nothing"
        );
        let chances = opportunities(&played, class, &instants, &defs);
        assert!(
            chances >= 1,
            "{class:?} (seed {seed}): no Druid instant was cast inside the interrupt's \
             reach, so the probe proves nothing — re-pick the seed"
        );
    }
}

/// The control: the same interrupters still interrupt a cast with a cast bar.
/// Without it, an interrupter that stopped interrupting anything would pass
/// the probe above.
#[test]
fn interrupters_still_interrupt_a_cast_bar_in_play() {
    let defs = AbilityDefinitions::default();
    for (class, victim) in [
        (CharacterClass::Warrior, "Mage"),
        (CharacterClass::Rogue, "Mage"),
        (CharacterClass::Shaman, "Priest"),
        (CharacterClass::Warlock, "Mage"),
    ] {
        let played = play(class.name(), victim, 1);
        let cut = interrupted_spells(&played.log);
        assert!(
            !cut.is_empty(),
            "{class:?} interrupted nothing against a {victim}"
        );
        for (t, spell) in cut {
            let (_, def) = defs
                .iter()
                .find(|(_, d)| d.name == spell)
                .unwrap_or_else(|| panic!("no ability named {spell}"));
            assert!(
                def.has_cast_bar() || def.is_channel(),
                "{class:?} interrupted {spell} at {t}s, which has no cast bar"
            );
        }
    }
}

// ── resolution probe ────────────────────────────────────────────────────────

/// An enemy Druid mid-`ability` (a `CastingState` of `cast_time`), and a
/// Pummel queued at it. Returns (cast cut?, lockouts queued).
fn resolve_pummel_at(ability: AbilityType, cast_time: f32) -> (bool, usize) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(AbilityDefinitions::default());
    let warrior = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warrior),
            Transform::default(),
        ))
        .id();
    let druid = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Druid),
            Transform::from_xyz(2.0, 0.0, 0.0),
            CastingState::new(ability, warrior, cast_time),
        ))
        .id();
    world.spawn(InterruptPending {
        caster: warrior,
        target: druid,
        ability: AbilityType::Pummel,
        lockout_duration: 4.0,
    });
    world.run_system_once(process_interrupts).unwrap();

    let cut = world.get::<CastingState>(druid).unwrap().interrupted;
    let mut q = world.query::<&AuraPending>();
    let lockouts = q
        .iter(&world)
        .filter(|p| p.target == druid && p.aura.effect_type == AuraType::SpellSchoolLockout)
        .count();
    let mut pending = world.query::<&InterruptPending>();
    assert_eq!(pending.iter(&world).count(), 0, "the interrupt is consumed");
    (cut, lockouts)
}

/// An interrupt that reaches a zero-length Lifebloom at resolution is spent
/// and does nothing; the same interrupt at a Cyclone cuts it and locks Nature.
///
/// Mutant killed: drop the `cast_is_interruptible` check from
/// `process_interrupts` — the Lifebloom is cut and Nature locked.
#[test]
fn an_interrupt_resolving_on_an_instant_does_nothing() {
    let defs = AbilityDefinitions::default();
    assert!(!defs.get_unchecked(&AbilityType::Lifebloom).has_cast_bar());
    assert!(defs.get_unchecked(&AbilityType::Cyclone).has_cast_bar());

    assert_eq!(
        resolve_pummel_at(AbilityType::Lifebloom, 0.0),
        (false, 0),
        "an instant is neither cut nor locked out"
    );
    let cyclone = defs.get_unchecked(&AbilityType::Cyclone).cast_time;
    assert_eq!(
        resolve_pummel_at(AbilityType::Cyclone, cyclone),
        (true, 1),
        "control: a cast with a cast bar is cut and locked out"
    );
}
