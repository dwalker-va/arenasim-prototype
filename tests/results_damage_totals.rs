//! AS-466 — a combatant's Results damage total is the sum of its breakdown.
//!
//! The Results screen shows two numbers for what a combatant dealt: its DMG
//! total (`Combatant::damage_dealt`, its pets' rolled in) and the per-ability
//! breakdown under it (the combat log's damage rows, its pets' folded in).
//! They are summed from independent sources, so each damage site must credit
//! the one and log the other with the same amount, under one rule
//! (`combat_core::credited_damage`: what reached health plus what absorbs
//! soaked). Direct hits, damage-over-time ticks, Drain Life, Holy Shock, the
//! Unstable Affliction backlash, reflects and transferred shares each used to
//! pick their own.
//!
//! One seeded batch (42 matches, its Frost Novas staged on a shielded
//! Warrior, and a Drain Life bank played until it has what it is for),
//! talented and geared so that every one of
//! those sites meets an absorb: Power Word: Shield, Aegis of the Last Stand
//! and Sacred Shield; Retribution Aura and a Reflective Shield; Soul Link and
//! Blessing of Sacrifice; Unstable Affliction under a Priest's and a Paladin's
//! dispels; a Warlock draining a Sacrifice holder. Four checks over it:
//!
//! 1. every primary combatant's total equals the sum of its breakdown rows;
//! 2. every damage row whose line names its numbers counts what reached health
//!    plus what was absorbed — the rule itself, which agreement alone cannot
//!    see (a site that dropped the absorb from both would still agree);
//! 3. every transferred share's row is one of its dealer's own rows — the
//!    row the hit it was split from was logged under ("Drain Life (tick)");
//! 4. the batch is not vacuous: each kind of event it exists for occurred.
//!
//! And one damage-over-time tick in a world of its own, for the caster no
//! match reliably stages: one with no auras of its own, as a dead Warlock
//! whose DoTs tick on is.

use std::collections::{BTreeMap, HashMap};

use regex::Regex;

use arenasim::combat::log::{CombatLog, StructuredEventData};
use arenasim::headless::runner::{
    build_headless_app, build_headless_app_staged, HeadlessMatchState,
};
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use bevy::prelude::Entity;

use arenasim::states::play_match::components::{Combatant, Pet, PetType};
use arenasim::states::play_match::talent_mechanics::{TalentMechanic, TalentMechanics};
use arenasim::states::play_match::talents::{Build, TalentId::*};
use arenasim::states::play_match::utils::{combatant_id, pet_combatant_id};

/// The Demonology capstone: Soul Link to the Felhunter.
fn soul_link() -> Build {
    Build::of(&[
        (DemonicEmbrace, 2),
        (FelStamina, 2),
        (MasterDemonologist, 1),
        (DemonicSacrifice, 1),
        (SoulLink, 1),
    ])
}

/// Protection to Sacred Shield: Retribution Aura, Blessing of Sacrifice and
/// Sacred Shield.
fn protection() -> Build {
    Build::of(&[
        (SacredDuty, 2),
        (RetributionAura, 2),
        (BlessingOfSacrifice, 1),
        (SacredShield, 1),
    ])
}

/// Aegis of the Last Stand in the first trinket socket.
fn aegis() -> HashMap<String, String> {
    HashMap::from([("Trinket1".to_string(), "AegisOfTheLastStand".to_string())])
}

/// A Warlock + Paladin team (`warlocks`) against a Warrior + Priest one, on
/// `seed`, the Warlock team on `side`.
fn duo(seed: u64, side: u8) -> HeadlessMatchConfig {
    let mut config = HeadlessMatchConfig {
        map: "BasicArena".into(),
        random_seed: Some(seed),
        max_duration_secs: 120.0,
        ..Default::default()
    };
    let (warlocks, priests) = (
        vec!["Warlock".to_string(), "Paladin".to_string()],
        vec!["Warrior".to_string(), "Priest".to_string()],
    );
    let builds = vec![soul_link(), protection()];
    // Per slot: the Paladin is the second.
    let auras = vec!["Devotion".to_string(), "Retribution".to_string()];
    let gear = vec![HashMap::new(), aegis()];
    if side == 1 {
        config.team1 = warlocks;
        config.team2 = priests;
        config.team1_builds = builds;
        config.team1_paladin_auras = auras;
        config.team2_equipment = vec![aegis()];
        config.team1_equipment = gear;
    } else {
        config.team2 = warlocks;
        config.team1 = priests;
        config.team2_builds = builds;
        config.team2_paladin_auras = auras;
        config.team1_equipment = vec![aegis()];
        config.team2_equipment = gear;
    }
    config
}

/// A 3v3 with pets on both sides and a Priest shielding each team: Hunter +
/// `striker` (a Mage, or a Rogue on Wound Poison) + Priest against Warlock + Paladin + Priest,
/// so a share Blessing of Sacrifice sends the Paladin can meet a Power Word:
/// Shield, and so can a Frost Nova or a Sinister Strike.
fn trio(seed: u64, striker: &str) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Hunter".into(), striker.into(), "Priest".into()],
        // Per slot; Wound Poison deals damage, through its own site.
        team1_rogue_poisons: vec!["Crippling".into(), "Wound".into(), "Crippling".into()],
        team2: vec!["Warlock".into(), "Paladin".into(), "Priest".into()],
        map: "BasicArena".into(),
        random_seed: Some(seed),
        max_duration_secs: 120.0,
        team1_equipment: vec![aegis()],
        team2_builds: vec![soul_link(), protection(), Build::default()],
        team2_paladin_auras: vec!["Devotion".into(), "Retribution".into()],
        ..Default::default()
    }
}

/// A Mage + Priest against a Warrior + Priest: a Frost Nova on a shielded
/// Warrior, staged by [`nova_stage`].
fn nova(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Warrior".into(), "Priest".into()],
        map: "BasicArena".into(),
        random_seed: Some(seed),
        max_duration_secs: 120.0,
        team2_equipment: vec![aegis()],
        ..Default::default()
    }
}

/// The enemy Warrior on the Mage, inside Frost Nova's reach and under its
/// Priest's Power Word: Shield, so the Mage's Nova lands into the absorb.
fn nova_stage() -> Stage {
    const MAGE: Unit = Unit::new(1, 0);
    const WARRIOR: Unit = Unit::new(2, 0);
    const PRIEST: Unit = Unit::new(2, 1);
    Stage::new()
        .place(MAGE, 0.0, 0.0)
        .place(WARRIOR, 1.5, 0.0)
        .target(WARRIOR, MAGE)
        .land(
            WARRIOR,
            arenasim::states::play_match::AbilityType::PowerWordShield,
            PRIEST,
        )
}

/// A Warlock + Warrior against a Warrior + Paladin with Blessing of
/// Sacrifice: a Drain Life tick on a holder passes a share on.
fn drain(seed: u64) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warlock".into(), "Warrior".into()],
        team2: vec!["Warrior".into(), "Paladin".into()],
        map: "BasicArena".into(),
        random_seed: Some(seed),
        max_duration_secs: 120.0,
        team2_builds: vec![Build::default(), protection()],
        ..Default::default()
    }
}

/// One primary combatant's two Results numbers.
struct Ledger {
    id: String,
    /// Its DMG total: its own `damage_dealt` and its pets'.
    total: f32,
    /// Its breakdown rows, by name.
    rows: BTreeMap<String, f32>,
}

/// A played match: its log and every primary combatant's ledger.
struct Played {
    label: String,
    log: CombatLog,
    ledgers: Vec<Ledger>,
    /// The log ids of its pets.
    pets: Vec<String>,
}

/// Play `config` to its end. Every Priest is given a Reflective Shield (no
/// shipped talent grants it yet) once it spawns.
fn play(label: String, config: HeadlessMatchConfig) -> Played {
    play_app(label, build_headless_app(&config, true, None))
}

/// [`play`] with `stage` landed as the gates open.
fn play_staged(label: String, config: HeadlessMatchConfig, stage: Stage) -> Played {
    play_app(label, build_headless_app_staged(&config, true, None, stage))
}

fn play_app(label: String, mut app: bevy::prelude::App) -> Played {
    let mut reflecting = false;
    for _ in 0..(60 * 60 * 5) {
        app.update();
        if !reflecting {
            let world = app.world_mut();
            let mut q = world.query::<&mut Combatant>();
            for mut c in q.iter_mut(world) {
                if c.class.name() == "Priest" {
                    c.talent_mechanics =
                        TalentMechanics::of(&[TalentMechanic::ReflectiveShield { share: 0.5 }]);
                    reflecting = true;
                }
            }
        }
        if app.world().resource::<HeadlessMatchState>().match_complete {
            break;
        }
    }
    let world = app.world_mut();
    assert!(
        world.resource::<HeadlessMatchState>().match_complete,
        "{label}: the match ran to its end"
    );
    let log = std::mem::take(&mut *world.resource_mut::<CombatLog>());

    // The Results screen's fold, as `match_flow` builds it.
    let mut pets_q = world.query::<(&Combatant, &Pet)>();
    let pets: Vec<(Combatant, Entity, PetType)> = pets_q
        .iter(world)
        .map(|(c, p)| (c.clone(), p.owner, p.pet_type))
        .collect();
    let mut owners_q = world.query::<(Entity, &Combatant, Option<&Pet>)>();
    let owners: Vec<(Entity, Combatant)> = owners_q
        .iter(world)
        .filter(|(_, _, pet)| pet.is_none())
        .map(|(e, c, _)| (e, c.clone()))
        .collect();
    let mut links: HashMap<String, (String, String)> = HashMap::new();
    let mut pet_ids = Vec::new();
    for (pet_c, pet_owner, pet_type) in &pets {
        if let Some((_, owner)) = owners.iter().find(|(e, _)| e == pet_owner) {
            let pet_id = pet_combatant_id(pet_c.team, owner.slot, *pet_type);
            pet_ids.push(pet_id.clone());
            links.insert(
                pet_id,
                (
                    combatant_id(owner.team, owner.slot, owner.class),
                    pet_type.name().to_string(),
                ),
            );
        }
    }
    let ledgers = owners
        .iter()
        .map(|(entity, c)| {
            let id = combatant_id(c.team, c.slot, c.class);
            let pet_damage: f32 = pets
                .iter()
                .filter(|(_, owner, _)| owner == entity)
                .map(|(pc, _, _)| pc.damage_dealt)
                .sum();
            Ledger {
                rows: log
                    .damage_by_ability_including_pets(&id, &links)
                    .into_iter()
                    .collect(),
                total: c.damage_dealt + pet_damage,
                id,
            }
        })
        .collect();
    Played {
        label,
        log,
        ledgers,
        pets: pet_ids,
    }
}

/// The batch, played once and shared by the checks.
fn batch() -> &'static [Played] {
    static BATCH: std::sync::OnceLock<Vec<Played>> = std::sync::OnceLock::new();
    BATCH.get_or_init(|| {
        let mut configs: Vec<(String, HeadlessMatchConfig)> = Vec::new();
        for seed in 466..472 {
            for side in [1, 2] {
                configs.push((format!("duo seed {seed} side {side}"), duo(seed, side)));
            }
            for striker in ["Mage", "Rogue"] {
                configs.push((format!("trio seed {seed} {striker}"), trio(seed, striker)));
            }
        }
        let mut played: Vec<Played> = configs
            .into_iter()
            .map(|(label, config)| play(label, config))
            .collect();
        for seed in 466..472 {
            for nova_seed in [seed, seed + 100, seed + 200] {
                played.push(play_staged(
                    format!("nova seed {nova_seed}"),
                    nova(nova_seed),
                    nova_stage(),
                ));
            }
        }
        // A bank for the Drain Life share, played until DRAIN_SHARES of its
        // matches have seen one (stopping on a success leaves the chance of
        // missing unchanged). Measured on seeds 1-60 (2026-11-12): 13 matches
        // drain a Sacrifice holder, a rate of 0.22, so the whole bank misses
        // with probability 0.78^60, about 4e-7; even at 0.13 (well below the
        // measured rate) it is 0.87^60, about 2e-4. The one-tick case is
        // `tests/drain_life_tick_log.rs`.
        const DRAIN_SHARES: usize = 3;
        let mut shared = 0;
        for seed in 1..=60 {
            let match_ = play(format!("drain seed {seed}"), drain(seed));
            if rows(&match_.log).any(|(_, ability, _, message)| {
                message.starts_with("[TRANSFER]") && ability == "Drain Life (tick)"
            }) {
                shared += 1;
            }
            played.push(match_);
            if shared == DRAIN_SHARES {
                break;
            }
        }
        played
    })
}

/// Every damage row, as (source, ability, amount, message).
fn rows(log: &CombatLog) -> impl Iterator<Item = (&str, &str, f32, &str)> + '_ {
    log.entries.iter().filter_map(|e| match &e.structured_data {
        Some(StructuredEventData::Damage {
            source,
            ability,
            amount,
            ..
        }) => Some((
            source.as_str(),
            ability.as_str(),
            *amount,
            e.message.as_str(),
        )),
        _ => None,
    })
}

#[test]
fn every_results_total_is_the_sum_of_its_breakdown() {
    let mut checked = 0;
    let mut bad = Vec::new();
    for played in batch() {
        for ledger in &played.ledgers {
            let sum: f32 = ledger.rows.values().sum();
            // The two are summed in different orders, so they may part in
            // the last bits; a missing or misjudged row is whole damage.
            let tolerance = 0.01 + ledger.total.abs() * 1e-5;
            if (ledger.total - sum).abs() > tolerance {
                bad.push(format!(
                    "{}: {} total {:.3}, breakdown {:.3} ({:?})",
                    played.label, ledger.id, ledger.total, sum, ledger.rows
                ));
            }
            if ledger.total > 0.0 {
                checked += 1;
            }
        }
    }
    assert!(checked >= 20, "only {checked} combatants dealt damage");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Every row whose line names what reached health (and what was absorbed)
/// counts the two together.
#[test]
fn every_damage_row_counts_its_absorbed_damage() {
    // "hits Y for N damage (M absorbed)", "ticks for N damage on Y (M absorbed)".
    let hit = Regex::new(r" for (\d+) damage(?: on [^(]*)?(?: \((\d+) absorbed\))?$").unwrap();
    let transfer =
        Regex::new(r"\[TRANSFER\] .* takes (\d+) of .*'s damage \(.*?(?:, (\d+) absorbed)?\)$")
            .unwrap();
    let mut named = 0;
    let mut with_absorb = 0;
    let mut bad = Vec::new();
    for played in batch() {
        for (source, ability, amount, message) in rows(&played.log) {
            let Some(caps) = transfer.captures(message).or_else(|| hit.captures(message)) else {
                continue;
            };
            let landed: f32 = caps[1].parse().unwrap();
            let absorbed: f32 = caps.get(2).map_or(0.0, |m| m.as_str().parse().unwrap());
            named += 1;
            if absorbed > 0.0 {
                with_absorb += 1;
            }
            // Each printed number is rounded to the nearest whole point.
            if (amount - (landed + absorbed)).abs() > 1.0 {
                bad.push(format!(
                    "{}: {source}'s {ability} row is {amount}, its line says {landed} + \
                     {absorbed} absorbed: {message}",
                    played.label
                ));
            }
        }
    }
    assert!(
        named > 1000 && with_absorb > 0,
        "{named} rows, {with_absorb} absorbed"
    );
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A transferred share is credited to the row of the hit it was split from:
/// every transfer row names a row its dealer logged for a hit of its own,
/// whatever name that hit's site gave its row ("Drain Life (tick)").
#[test]
fn every_transfer_row_is_one_of_its_dealers_own_rows() {
    let mut shares = 0;
    let mut bad = Vec::new();
    for played in batch() {
        let own: std::collections::BTreeSet<(&str, &str)> = rows(&played.log)
            .filter(|(_, _, _, m)| !m.starts_with("[TRANSFER]"))
            .map(|(source, ability, _, _)| (source, ability))
            .collect();
        for (source, ability, _, message) in rows(&played.log) {
            if !message.starts_with("[TRANSFER]") {
                continue;
            }
            shares += 1;
            if !own.contains(&(source, ability)) {
                bad.push(format!(
                    "{}: {source}'s share row {ability:?} is none of its own rows: {message}",
                    played.label
                ));
            }
        }
    }
    assert!(shares > 0, "no transfer rows");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The batch saw every kind of event it is for.
#[test]
fn the_batch_is_not_vacuous() {
    let kinds: [(&str, fn(&str, &str, bool) -> bool); 18] = [
        ("a hit into an absorb", |m, _, row| {
            row && (m.contains(" hits ") || m.contains(" CRITS ")) && m.ends_with(" absorbed)")
        }),
        ("an instant strike into an absorb", |m, ability, row| {
            row && ["Mortal Strike", "Sinister Strike"].contains(&ability)
                && m.ends_with(" absorbed)")
        }),
        ("a Frost Nova into an absorb", |m, ability, row| {
            row && ability == "Frost Nova" && m.ends_with(" absorbed)")
        }),
        ("a poison into an absorb", |m, ability, row| {
            row && ability == "Wound Poison" && m.ends_with(" absorbed)")
        }),
        ("a damage-over-time tick into an absorb", |m, _, row| {
            row && m.contains(" ticks for ") && m.ends_with(" absorbed)")
        }),
        ("Aegis of the Last Stand raised", |m, _, _| {
            m.contains(" gains Aegis of the Last Stand (absorbs ")
        }),
        ("Sacred Shield raised", |m, _, _| {
            m.contains(" gains Sacred Shield (absorbs ")
        }),
        ("Power Word: Shield", |m, _, _| {
            m.contains(" casts Power Word: Shield on ")
        }),
        ("a Retribution Aura reflect", |m, _, row| {
            row && m.starts_with("[REFLECT]") && m.ends_with("Retribution Aura")
        }),
        ("a Reflective Shield reflect", |m, _, row| {
            row && m.starts_with("[REFLECT]") && m.ends_with("Power Word: Shield")
        }),
        ("an Unstable Affliction backlash", |m, _, row| {
            row && m.starts_with("[BACKLASH]")
        }),
        ("a Soul Link share", |m, _, row| {
            row && m.starts_with("[TRANSFER]") && m.contains("(Soul Link")
        }),
        ("a Blessing of Sacrifice share", |m, _, row| {
            row && m.starts_with("[TRANSFER]") && m.contains("(Blessing of Sacrifice")
        }),
        ("a transferred share into an absorb", |m, _, row| {
            row && m.starts_with("[TRANSFER]") && m.ends_with(" absorbed)")
        }),
        ("a Holy Shock hit", |_, ability, row| {
            row && ability == "Holy Shock"
        }),
        ("a Drain Life tick", |_, ability, row| {
            row && ability == "Drain Life (tick)"
        }),
        ("a pet's hit", |_, ability, row| row && ability == "<pet>"),
        ("a share of a Drain Life tick", |m, ability, row| {
            row && m.starts_with("[TRANSFER]") && ability == "Drain Life (tick)"
        }),
    ];
    let mut counts: BTreeMap<&str, usize> = kinds.iter().map(|(k, _)| (*k, 0)).collect();
    for played in batch() {
        for entry in &played.log.entries {
            let (row, ability) = match &entry.structured_data {
                Some(StructuredEventData::Damage {
                    source, ability, ..
                }) => (
                    true,
                    if played.pets.contains(source) {
                        "<pet>"
                    } else {
                        ability.as_str()
                    },
                ),
                _ => (false, ""),
            };
            for (kind, seen) in &kinds {
                if seen(&entry.message, ability, row) {
                    *counts.get_mut(kind).unwrap() += 1;
                }
            }
        }
    }
    let missing: Vec<_> = counts.iter().filter(|(_, n)| **n == 0).collect();
    assert!(
        missing.is_empty(),
        "never seen: {missing:?}\nall: {counts:?}"
    );
}

// ---------------------------------------------------------------------------
// One damage-over-time tick
// ---------------------------------------------------------------------------

/// One Corruption tick of 10 from a Warlock, into a 4-point shield, with the
/// Warlock carrying auras of its own or none (a dead caster's have all
/// ended). Returns the Warlock's credit and its breakdown row's total.
fn dot_tick(caster_has_auras: bool) -> (f32, f32) {
    use std::time::Duration;

    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::{Time, Transform, World};

    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::abilities::SpellSchool;
    use arenasim::states::play_match::components::{ActiveAuras, ArenaDampening, Aura, AuraType};
    use arenasim::states::play_match::process_dot_ticks;

    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);
    let mut warlock = world.spawn((
        Combatant::new(1, 0, CharacterClass::Warlock),
        Transform::default(),
    ));
    if caster_has_auras {
        warlock.insert(ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::MaxHealthIncrease,
                duration: 60.0,
                magnitude: 1.0,
                ability_name: "Staged".into(),
                ..Default::default()
            }],
        });
    }
    let warlock = warlock.id();
    let mut warrior = Combatant::new(2, 0, CharacterClass::Warrior);
    warrior.shadow_resistance = 0.0;
    world.spawn((
        warrior,
        Transform::from_xyz(20.0, 0.0, 0.0),
        ActiveAuras {
            auras: vec![
                Aura {
                    effect_type: AuraType::DamageOverTime,
                    duration: 18.0,
                    magnitude: 10.0,
                    tick_interval: 3.0,
                    time_until_next_tick: 0.0,
                    caster: Some(warlock),
                    ability_name: "Corruption".into(),
                    spell_school: Some(SpellSchool::Shadow),
                    break_on_damage_threshold: -1.0,
                    ..Default::default()
                },
                Aura {
                    effect_type: AuraType::Absorb,
                    duration: 30.0,
                    magnitude: 4.0,
                    ability_name: "Power Word: Shield".into(),
                    break_on_damage_threshold: -1.0,
                    ..Default::default()
                },
            ],
        },
    ));
    world.run_system_once(process_dot_ticks).unwrap();
    let credited = world.get::<Combatant>(warlock).unwrap().damage_dealt;
    let log = world.resource::<CombatLog>();
    assert_eq!(
        log.entries
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>(),
        vec!["Team 1 Warlock #1's Corruption ticks for 6 damage on Team 2 Warrior #1 (4 absorbed)"],
        "the tick landed 6 past the shield"
    );
    (credited, log.total_damage_dealt("Team 1 Warlock #1"))
}

/// A tick credits its caster what reached health plus what the shield took,
/// as its breakdown row counts it, whether or not the caster carries auras
/// of its own: a Warlock whose DoTs tick on after its death is credited too.
#[test]
fn a_dot_tick_credits_its_caster_as_its_row_counts_it() {
    assert_eq!(dot_tick(true), (10.0, 10.0));
    assert_eq!(dot_tick(false), (10.0, 10.0));
}
