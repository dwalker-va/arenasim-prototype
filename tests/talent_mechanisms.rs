//! AS-331 — the general talent mechanisms, in real matches: talent procs on
//! each trigger, the next-cast modifier read and spent, and the leech field.
//!
//! Each is a staged match (tier 3) whose stage gives a unit the mechanism —
//! a proc slot on its `Combatant`, an aura in its `ActiveAuras` — and a
//! control, the same stage without that one thing, so no test can pass on
//! nothing happening. The match plays the production schedule; the combat
//! log is what is read. One test resolves a proc from a BUILD (a fixture
//! `talents.ron`) through the spawn, so the whole path from a node to a
//! fired proc is pinned too.
//!
//! The pure halves — the roll, the rank scaling, the per-minute rate, the
//! effective book, the leech arithmetic — are unit tests beside their code
//! (`talent_procs.rs`, `next_cast.rs`, `combat_core/leech.rs`).

mod common;

use arenasim::headless::runner::{build_headless_app, PreloadedConfigs};
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::combat_core::leech::{Leech, LeechRecipient, LeechSource};
use arenasim::states::play_match::components::{Aura, AuraType, DispelType};
use arenasim::states::play_match::next_cast::{NextCastModifier, NextCastScope};
use arenasim::states::play_match::talent_procs::{
    ProcChance, ProcEffect, ProcRankScale, ProcTarget, TalentProc, TalentProcSlot, TalentTrigger,
};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};
use arenasim::states::play_match::AbilityDefinitions;
use common::staged::play;

const T1: Unit = Unit::new(1, 0);
const T1B: Unit = Unit::new(1, 1);
const T2: Unit = Unit::new(2, 0);
/// The Felhunter of the Warlock in slot 0 of team 1.
const PET1: Unit = Unit::pet_of(1, 0);
/// No leech heals a pet: a party is its living non-pet teammates.
const PET_HEALED: &str = "Vampiric Embrace heals Team 1 Felhunter";

fn config(team1: &[&str], team2: &[&str], secs: f32) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        random_seed: Some(7),
        max_duration_secs: secs,
        team1_equipment: proc_free_trinkets(team1.len()),
        team2_equipment: proc_free_trinkets(team2.len()),
        ..Default::default()
    }
}

/// Both trinket sockets of every member filled with the starting kit's two
/// stats-only trinkets. The mechanisms here are read off the combat log —
/// "procs" lines, crits, heals — so no unit may carry a proc trinket of its
/// own, whatever the default loadouts put in its trinket sockets.
fn proc_free_trinkets(members: usize) -> Vec<std::collections::HashMap<String, String>> {
    let sockets = std::collections::HashMap::from([
        ("Trinket1".to_string(), "MarkOfTheChampion".to_string()),
        ("Trinket2".to_string(), "EssenceOfEternalLife".to_string()),
    ]);
    vec![sockets; members]
}

/// The combat log's lines that contain `needle`.
fn lines<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

fn modifier(scope: NextCastScope) -> NextCastModifier {
    NextCastModifier {
        applies_to: scope,
        instant: false,
        free: false,
        crit_chance: 0.0,
        cast_time_reduction: 0.0,
        cost_reduction: 0.0,
        power: 0.0,
    }
}

/// A proc that fires for certain on `trigger`, granting its owner a harmless
/// attack-power buff named `name` (or `effect` when given).
fn certain(trigger: TalentTrigger, name: &str) -> TalentProc {
    TalentProc {
        trigger,
        abilities: vec![],
        crit_only: false,
        chance: ProcChance::Chance(1.0),
        ranks_add: ProcRankScale::Chance,
        effect: ProcEffect::ApplyAura {
            on: ProcTarget::Owner,
            aura_type: AuraType::AttackPowerIncrease,
            magnitude: 1.0,
            duration: 5.0,
            tick_interval: 0.0,
            dispel_type: DispelType::Physical,
            scope: None,
            charges: None,
            name: Some(name.to_string()),
        },
        internal_cooldown: 0.0,
    }
}

/// `who` carries `proc` as a talent proc (any talent id: it names the log).
fn with_proc(stage: Stage, who: Unit, proc: TalentProc) -> Stage {
    stage.combatant(who, move |c| {
        c.talent_procs.slots.push(TalentProcSlot::new(
            TalentId::FelStamina,
            "Test Talent",
            proc,
        ));
    })
}

fn procs<'a>(log: &'a str, name: &str) -> Vec<&'a str> {
    lines(log, &format!("Test Talent procs {name}"))
}

// ---------------------------------------------------------------------------
// Procs
// ---------------------------------------------------------------------------

/// `MeleeHit`, normalised for weapon speed: a per-minute rate high enough to
/// be certain on the Warrior's swing fires on its hits. The control rate is
/// tiny, and its chance ~0, because the site hands the resolver the swing's
/// real interval.
#[test]
fn a_melee_hit_proc_fires_at_its_per_minute_rate() {
    let mut fast = certain(TalentTrigger::MeleeHit, "Fury");
    fast.chance = ProcChance::PerMinute(10_000.0);
    let played = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(Stage::new(), T1, fast.clone()),
    );
    assert!(!procs(&played.log, "Fury").is_empty(), "{}", played.log);

    fast.chance = ProcChance::PerMinute(1.0e-4);
    let control = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(Stage::new(), T1, fast),
    );
    assert!(procs(&control.log, "Fury").is_empty());
}

/// A crit-only `MeleeHit` (Vengeance) fires on a crit and never on a hit.
#[test]
fn a_crit_only_melee_proc_needs_a_crit() {
    let mut vengeance = certain(TalentTrigger::MeleeHit, "Vengeance");
    vengeance.crit_only = true;
    let crits = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(Stage::new(), T1, vengeance.clone()).combatant(T1, |c| c.crit_chance = 1.0),
    );
    assert!(!procs(&crits.log, "Vengeance").is_empty());
    let never = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(Stage::new(), T1, vengeance).combatant(T1, |c| c.crit_chance = 0.0),
    );
    assert!(
        !lines(&never.log, "Melee Swing hits").is_empty()
            || !lines(&never.log, " hits ").is_empty()
    );
    assert!(procs(&never.log, "Vengeance").is_empty());
}

/// `CritTaken` with a heal-back (Blessed Recovery): a Priest a Warrior crits
/// gets its proc; one the Warrior never crits does not.
#[test]
fn a_crit_taken_proc_heals_back_its_share() {
    let mut recovery = certain(TalentTrigger::CritTaken, "");
    recovery.effect = ProcEffect::HealBack {
        fraction: 0.25,
        duration: 6.0,
        tick_interval: 2.0,
        name: Some("Blessed Recovery".to_string()),
    };
    // Mortal Strike held, so every crit the Priest takes is an auto-attack's.
    let crits = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(Stage::new(), T2, recovery.clone())
            .combatant(T1, |c| c.crit_chance = 1.0)
            .cooldown(T1, AbilityType::MortalStrike, 1000.0),
    );
    assert!(!procs(&crits.log, "Blessed Recovery").is_empty());
    assert!(
        !lines(&crits.log, "Blessed Recovery")
            .iter()
            .all(|l| l.contains("procs")),
        "the heal over time ticks: {}",
        crits.log
    );
    let none = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(Stage::new(), T2, recovery).combatant(T1, |c| c.crit_chance = 0.0),
    );
    assert!(procs(&none.log, "Blessed Recovery").is_empty());
}

/// `SpellHit` with an aura on the struck enemy (Aftermath), narrowed to
/// Shadow Bolt: it fires for Shadow Bolt and not when the only ability it
/// names is one the Warlock's hits are not.
/// A full-share, one-tick Blessed Recovery on a Priest taking double damage,
/// struck by `attacker` with every blow a crit. The Priest is held silenced,
/// so no shield or heal of its own gets in the way, with health to spare.
fn heal_back_run(attacker: &str, stage: Stage) -> String {
    heal_back_run_at(attacker, 1.0, stage)
}

/// [`heal_back_run`] with the attacker's own crit chance set to `crit`.
fn heal_back_run_at(attacker: &str, crit: f32, stage: Stage) -> String {
    heal_back_run_with(attacker, crit, stage, None)
}

/// [`heal_back_run_at`], with `shield` staged on the Priest after its auras
/// are cleared.
fn heal_back_run_with(attacker: &str, crit: f32, stage: Stage, shield: Option<Aura>) -> String {
    let mut recovery = certain(TalentTrigger::CritTaken, "");
    recovery.effect = ProcEffect::HealBack {
        fraction: 1.0,
        duration: 0.1,
        tick_interval: 0.1,
        name: Some("Blessed Recovery".to_string()),
    };
    let held = |effect_type, name: &str| Aura {
        effect_type,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: name.to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let staged = {
        with_proc(stage, T2, recovery)
            .combatant(T1, move |c| c.crit_chance = crit)
            .combatant(T2, |c| {
                c.max_health = 10_000.0;
                c.current_health = 5_000.0;
            })
            .clear_auras(T2)
            .aura(T2, held(AuraType::Silence, "Held"))
            .aura(T2, held(AuraType::DamageTakenIncrease, "Vulnerable"))
    };
    let staged = match shield {
        Some(shield) => staged.aura(T2, shield),
        None => staged,
    };
    play(config(&[attacker], &["Priest"], 25.0), staged).log
}

const RECOVERED: &str = "Priest #1's Blessed Recovery heals Team 2 Priest #1 for";

/// A crit-taken heal-back heals its share of what the crit LANDED, after the
/// damage pipeline (AS-408), at both crit sites: each auto-attack crit is
/// healed back exactly (Mortal Strike held, so no healing cut), and so is
/// each Sinister Strike crit that ticks before another crit refreshes it.
#[test]
fn a_crit_taken_heal_back_reads_the_damage_the_crit_landed() {
    let swings = heal_back_run(
        "Warrior",
        Stage::new().cooldown(T1, AbilityType::MortalStrike, 1000.0),
    );
    let crits = amounts(&swings, "Warrior #1's Auto Attack CRITS", " for ");
    let healed = amounts(&swings, RECOVERED, " for ");
    let n = crits.len().min(healed.len());
    assert!(n >= 2, "{swings}");
    assert_eq!(crits[..n], healed[..n], "{swings}");

    // A Rogue's swings can land right after its Sinister Strike and refresh
    // the heal-back before it ticks, so not every strike is healed back;
    // at least two are, whole.
    let strikes = heal_back_run("Rogue", Stage::new());
    let healed = amounts(&strikes, RECOVERED, " for ");
    let whole = amounts(&strikes, "Rogue #1's Sinister Strike CRITS", " for ")
        .into_iter()
        .filter(|c| healed.contains(c))
        .count();
    assert!(whole >= 2, "{strikes}");
}

#[test]
fn a_spell_hit_proc_lands_its_aura_on_the_enemy_struck() {
    let mut aftermath = certain(TalentTrigger::SpellHit, "");
    aftermath.abilities = vec![AbilityType::Shadowbolt];
    aftermath.effect = ProcEffect::ApplyAura {
        on: ProcTarget::Other,
        aura_type: AuraType::MovementSpeedSlow,
        magnitude: 0.7,
        duration: 5.0,
        tick_interval: 0.0,
        dispel_type: DispelType::Auto,
        scope: None,
        charges: None,
        name: Some("Aftermath".to_string()),
    };
    // The Priest staged under every damage over time the Warlock opens
    // with, and its Fear and Death Coil held, so its next cast is a Shadow
    // Bolt.
    let opened = || {
        Stage::new()
            .land(T2, AbilityType::Corruption, T1)
            .land(T2, AbilityType::CurseOfAgony, T1)
            .land(T2, AbilityType::Immolate, T1)
            .land(T2, AbilityType::UnstableAffliction, T1)
            .cooldown(T1, AbilityType::Fear, 60.0)
            .cooldown(T1, AbilityType::DeathCoil, 60.0)
    };
    let played = play(
        config(&["Warlock"], &["Priest"], 30.0),
        with_proc(opened(), T1, aftermath.clone()),
    );
    assert!(
        !lines(&played.log, "Shadow Bolt hits").is_empty()
            || !lines(&played.log, "Shadow Bolt CRITS").is_empty(),
        "{}",
        played.log
    );
    assert!(
        !procs(&played.log, "Aftermath").is_empty(),
        "{}",
        played.log
    );

    aftermath.abilities = vec![AbilityType::LifeTap];
    let control = play(
        config(&["Warlock"], &["Priest"], 30.0),
        with_proc(opened(), T1, aftermath),
    );
    assert!(procs(&control.log, "Aftermath").is_empty());
}

/// `DotTick` (Nightfall) on Corruption's ticks, priming Shadow Trance; the
/// next Shadow Bolt spends it.
#[test]
fn a_dot_tick_proc_primes_a_next_cast_that_shadow_bolt_spends() {
    let played = play(
        config(&["Warlock"], &["Priest"], 30.0),
        // Narrowed to Corruption's ticks, so the proc is the DoT site's.
        with_proc(Stage::new(), T1, {
            let mut p = nightfall(1.0);
            p.abilities = vec![AbilityType::Corruption];
            p
        }),
    );
    assert!(
        !procs(&played.log, "Shadow Trance").is_empty(),
        "{}",
        played.log
    );
    assert!(
        !lines(&played.log, "Shadow Trance is spent on Shadow Bolt").is_empty(),
        "{}",
        played.log
    );
    let mut immolate_only = nightfall(1.0);
    immolate_only.abilities = vec![AbilityType::LifeTap];
    let control = play(
        config(&["Warlock"], &["Priest"], 30.0),
        with_proc(Stage::new(), T1, immolate_only),
    );
    assert!(procs(&control.log, "Shadow Trance").is_empty());
}

fn nightfall(chance: f32) -> TalentProc {
    let mut trance = modifier(NextCastScope::Abilities(vec![AbilityType::Shadowbolt]));
    trance.instant = true;
    TalentProc {
        trigger: TalentTrigger::DotTick,
        abilities: vec![AbilityType::Corruption, AbilityType::DrainLife],
        crit_only: false,
        chance: ProcChance::Chance(chance),
        ranks_add: ProcRankScale::Chance,
        effect: ProcEffect::NextCast {
            modifier: trance,
            duration: 10.0,
            dispel_type: DispelType::Auto,
            name: Some("Shadow Trance".to_string()),
        },
        internal_cooldown: 0.0,
    }
}

/// `HealCrit` (Inspiration): a Priest whose heals all crit procs on the ally
/// it healed; one whose heals never crit does not.
#[test]
fn a_heal_crit_proc_lands_on_the_ally_healed() {
    let mut inspiration = certain(TalentTrigger::HealCrit, "");
    inspiration.effect = ProcEffect::ApplyAura {
        on: ProcTarget::Other,
        aura_type: AuraType::ArmorIncrease,
        magnitude: 10.0,
        duration: 15.0,
        tick_interval: 0.0,
        dispel_type: DispelType::Auto,
        scope: None,
        charges: None,
        name: Some("Inspiration".to_string()),
    };
    let stage = |crit: f32| {
        with_proc(Stage::new(), T1B, inspiration.clone())
            .combatant(T1B, move |c| c.crit_chance = crit)
            .health(T1, 0.4)
    };
    let played = play(
        config(&["Warrior", "Priest"], &["Warrior"], 20.0),
        stage(1.0),
    );
    assert!(
        !procs(&played.log, "Inspiration").is_empty(),
        "{}",
        played.log
    );
    let control = play(
        config(&["Warrior", "Priest"], &["Warrior"], 20.0),
        stage(0.0),
    );
    assert!(!lines(&control.log, "Priest #2's Flash Heal heals").is_empty());
    assert!(procs(&control.log, "Inspiration").is_empty());
}

/// `AbilityUsed` priming an any-ability cost cut (Initiative): every ability
/// the Rogue uses primes the next, which the next spends.
#[test]
fn an_ability_used_proc_primes_the_next_ability() {
    let mut cut = modifier(NextCastScope::Any);
    cut.cost_reduction = 5.0;
    let initiative = TalentProc {
        trigger: TalentTrigger::AbilityUsed,
        abilities: vec![AbilityType::SinisterStrike],
        crit_only: false,
        chance: ProcChance::Chance(1.0),
        ranks_add: ProcRankScale::Effect,
        effect: ProcEffect::NextCast {
            modifier: cut,
            duration: 30.0,
            dispel_type: DispelType::Untyped,
            name: Some("Initiative".to_string()),
        },
        internal_cooldown: 0.0,
    };
    let played = play(
        config(&["Rogue"], &["Priest"], 20.0),
        with_proc(Stage::new(), T1, initiative),
    );
    assert!(
        !procs(&played.log, "Initiative").is_empty(),
        "{}",
        played.log
    );
    assert!(
        !lines(&played.log, "Initiative is spent on").is_empty(),
        "{}",
        played.log
    );
    let control = play(config(&["Rogue"], &["Priest"], 20.0), Stage::new());
    assert!(lines(&control.log, "Initiative").is_empty());
}

/// `StealthBreak` (Master of Subtlety): the Rogue opens from stealth, and
/// leaving it fires the proc, once.
#[test]
fn leaving_stealth_fires_a_stealth_break_proc() {
    let played = play(
        config(&["Rogue"], &["Priest"], 20.0),
        with_proc(
            Stage::new(),
            T1,
            certain(TalentTrigger::StealthBreak, "Master of Subtlety"),
        ),
    );
    assert_eq!(
        procs(&played.log, "Master of Subtlety").len(),
        1,
        "{}",
        played.log
    );
    // A Warrior never stealths, so never leaves it.
    let control = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(
            Stage::new(),
            T1,
            certain(TalentTrigger::StealthBreak, "Master of Subtlety"),
        ),
    );
    assert!(procs(&control.log, "Master of Subtlety").is_empty());
}

/// A proc on its internal cooldown does not fire again: a certain melee proc
/// with a cooldown longer than the match fires once.
#[test]
fn an_internal_cooldown_holds_a_proc() {
    let mut once = certain(TalentTrigger::MeleeHit, "Fury");
    once.internal_cooldown = 1000.0;
    let played = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(Stage::new(), T1, once),
    );
    assert_eq!(procs(&played.log, "Fury").len(), 1, "{}", played.log);
}

/// The whole path: a node's `Proc` effect in `talents.ron` (a fixture, on Fel
/// Stamina), resolved from a build at its rank, installed at the spawn, fires
/// in the match. Ranks add to its chance: at rank 2 it is certain.
#[test]
fn a_proc_resolved_from_a_build_fires_in_the_match() {
    let shipped = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron");
    let from = "per_rank: [PetStatPercent(MaxHealth, 0.10)],";
    assert!(shipped.contains(from));
    let text = shipped.replacen(
        from,
        "per_rank: [Proc((trigger: DotTick, abilities: [Corruption], chance: Chance(0.5), \
         ranks_add: Chance, effect: NextCast(modifier: (applies_to: Abilities([Shadowbolt]), \
         instant: true), duration: 10.0, dispel_type: Auto, name: Some(\"Shadow Trance\"))))],",
        1,
    );
    let talents =
        TalentDefinitions::from_ron(&text, AbilityDefinitions::shipped()).expect("the fixture");
    let mut preloaded = PreloadedConfigs::load().expect("configs");
    preloaded.talents = talents;
    let run = |build: Build| {
        let config = HeadlessMatchConfig {
            team1_builds: vec![build],
            ..config(&["Warlock"], &["Priest"], 30.0)
        };
        let mut app = build_headless_app(&config, true, Some(&preloaded));
        for _ in 0..(60 * 40) {
            app.update();
        }
        app.world()
            .resource::<arenasim::combat::log::CombatLog>()
            .entries
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<String>>()
    };
    let talented = run(Build::of(&[(TalentId::FelStamina, 2)]));
    let fired: Vec<&String> = talented
        .iter()
        .filter(|m| m.contains("Fel Stamina procs Shadow Trance"))
        .collect();
    let ticks = talented
        .iter()
        .filter(|m| m.contains("Corruption ticks"))
        .count();
    assert!(ticks > 0);
    assert_eq!(fired.len(), ticks, "rank 2 is certain: one proc a tick");
    let untalented = run(Build::default());
    assert!(untalented.iter().all(|m| !m.contains("procs")));
}

// ---------------------------------------------------------------------------
// Next-cast modifiers
// ---------------------------------------------------------------------------

/// A cast-bar heal spends a sure-crit, free modifier when it begins and
/// lands as a crit at no cost; the control modifier (free only) lands the
/// heal as the Priest's own crit chance has it.
#[test]
fn a_spent_modifier_rides_a_cast_to_its_landing() {
    let favor = |crit: f32| {
        let mut m = modifier(NextCastScope::Abilities(vec![AbilityType::FlashHeal]));
        m.crit_chance = crit;
        m.free = true;
        Aura {
            effect_type: AuraType::NextCast,
            duration: 300.0,
            magnitude: 1.0,
            ability_name: "Divine Favor".to_string(),
            next_cast: Some(m),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        }
    };
    let stage = |crit: f32| {
        Stage::new()
            .aura(T1B, favor(crit))
            .combatant(T1B, |c| c.crit_chance = 0.0)
            .health(T1, 0.4)
    };
    let cfg = || config(&["Warrior", "Priest"], &["Warrior"], 20.0);
    let played = play(cfg(), stage(1.0));
    let after = played
        .log
        .split("Divine Favor is spent on Flash Heal")
        .nth(1)
        .unwrap_or_else(|| panic!("the heal spent it: {}", played.log));
    let heal = after
        .lines()
        .find(|l| l.contains("Priest #2's Flash Heal"))
        .expect("the Flash Heal lands");
    assert!(heal.contains("CRITICALLY heals"), "{heal}");

    let control = play(cfg(), stage(0.0));
    let after = control
        .log
        .split("Divine Favor is spent on Flash Heal")
        .nth(1)
        .expect("the heal spent it");
    let heal = after
        .lines()
        .find(|l| l.contains("Priest #2's Flash Heal"))
        .expect("the Flash Heal lands");
    assert!(!heal.contains("CRITICALLY"), "{heal}");
}

/// The same through a projectile: a Shadow Bolt that spent a sure-crit
/// modifier crits when it arrives.
#[test]
fn a_spent_modifier_rides_a_projectile_to_its_landing() {
    let trance = |crit: f32| {
        let mut m = modifier(NextCastScope::Abilities(vec![AbilityType::Shadowbolt]));
        m.crit_chance = crit;
        m.instant = true;
        Aura {
            effect_type: AuraType::NextCast,
            duration: 300.0,
            magnitude: 1.0,
            ability_name: "Shadow Trance".to_string(),
            next_cast: Some(m),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        }
    };
    let run = |crit: f32| {
        play(
            config(&["Warlock"], &["Priest"], 30.0),
            Stage::new()
                .aura(T1, trance(crit))
                .combatant(T1, |c| c.crit_chance = 0.0),
        )
    };
    let bolt_after = |log: &str| -> String {
        let after = log
            .split("Shadow Trance is spent on Shadow Bolt")
            .nth(1)
            .unwrap_or_else(|| panic!("the bolt spent it: {log}"))
            .to_string();
        after
            .lines()
            .find(|l| l.contains("'s Shadow Bolt"))
            .expect("the bolt lands")
            .to_string()
    };
    assert!(bolt_after(&run(1.0).log).contains("CRITS"));
    assert!(!bolt_after(&run(0.0).log).contains("CRITS"));
}

/// The decision reads the EFFECTIVE book (a scene, one tick): a Warlock with
/// no mana is refused Shadow Bolt for want of it, and the same Warlock with a
/// free Shadow Trance up is not.
#[test]
fn a_live_modifier_changes_the_config_the_decision_reads() {
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::class_ai::scene::Scene;
    use arenasim::states::play_match::decision_trace::RejectionReason;
    let scene = |free: bool| {
        let mut m = modifier(NextCastScope::Abilities(vec![AbilityType::Shadowbolt]));
        m.free = free;
        m.instant = !free;
        Scene::new()
            .unit("lock", 1, CharacterClass::Warlock)
            .mana(0.0)
            // Out of mana a Warlock taps first; hold that, so the decision
            // reaches Shadow Bolt.
            .cooldown("lock", AbilityType::LifeTap, 30.0)
            .targeting("prey")
            .unit("prey", 2, CharacterClass::Warrior)
            .at(25.0, 0.0)
            .aura_full(
                "lock",
                Aura {
                    effect_type: AuraType::NextCast,
                    duration: 10.0,
                    magnitude: 1.0,
                    ability_name: "Shadow Trance".to_string(),
                    next_cast: Some(m),
                    break_on_damage_threshold: -1.0,
                    ..Default::default()
                },
            )
    };
    let broke = scene(false).decide("lock");
    assert!(
        matches!(
            broke.rejected(AbilityType::Shadowbolt),
            Some(RejectionReason::InsufficientMana { .. })
        ),
        "{:?}",
        broke.candidates
    );
    let free = scene(true).decide("lock");
    assert!(
        !matches!(
            free.rejected(AbilityType::Shadowbolt),
            Some(RejectionReason::InsufficientMana { .. })
        ),
        "{:?}",
        free.candidates
    );
}

// ---------------------------------------------------------------------------
// Leech
// ---------------------------------------------------------------------------

fn leeching(name: &str, leech: Option<Leech>) -> Aura {
    Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 300.0,
        magnitude: 4.0,
        tick_interval: 3.0,
        time_until_next_tick: 3.0,
        ability_name: name.to_string(),
        spell_school: Some(SpellSchool::Shadow),
        leech,
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// An `Ability` leech on a DoT (Siphon Life) heals its caster for each tick;
/// the same DoT without the field heals nothing. The caster is the staged
/// Warlock, so the aura is staged in a world step that knows its entity.
#[test]
fn a_dot_that_leeches_heals_its_caster_each_tick() {
    let run = |leech: Option<Leech>| {
        let aura = leeching("Siphon Life", leech);
        play(
            config(&["Warlock"], &["Warrior"], 20.0),
            Stage::new().health(T1, 0.5).world(move |w| {
                let warlock = T1.entity(w);
                let warrior = T2.entity(w);
                let mut aura = aura;
                aura.caster = Some(warlock);
                w.get_mut::<arenasim::states::play_match::ActiveAuras>(warrior)
                    .expect("auras")
                    .auras
                    .push(aura);
            }),
        )
    };
    let siphon = Leech {
        fraction: 1.0,
        to: LeechRecipient::Caster,
        of: LeechSource::Ability,
    };
    let played = run(Some(siphon));
    assert!(
        !lines(&played.log, "Warlock #1's Siphon Life heals for").is_empty(),
        "{}",
        played.log
    );
    let control = run(None);
    assert!(!lines(&control.log, "Siphon Life ticks").is_empty());
    assert!(lines(&control.log, "Siphon Life heals").is_empty());
}

/// A `CasterSchool` leech to the party (Vampiric Embrace): while it is on the
/// enemy, the Warlock's Shadow damage to it heals the Warlock's teammate; a
/// leech on another school heals nobody.
#[test]
fn a_caster_school_leech_heals_the_casters_party() {
    let run = |school: SpellSchool| {
        let aura = Aura {
            effect_type: AuraType::HealingReduction,
            duration: 300.0,
            magnitude: 1.0,
            ability_name: "Vampiric Embrace".to_string(),
            leech: Some(Leech {
                fraction: 0.2,
                to: LeechRecipient::CasterParty,
                of: LeechSource::CasterSchool(school),
            }),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        };
        play(
            config(&["Warlock", "Warrior"], &["Priest"], 25.0),
            Stage::new()
                .health(T1B, 0.5)
                .health(PET1, 0.5)
                .world(move |w| {
                    let warlock = T1.entity(w);
                    let priest = T2.entity(w);
                    let mut aura = aura;
                    aura.caster = Some(warlock);
                    w.get_mut::<arenasim::states::play_match::ActiveAuras>(priest)
                        .expect("auras")
                        .auras
                        .push(aura);
                }),
        )
    };
    let played = run(SpellSchool::Shadow);
    assert!(
        !lines(&played.log, "Vampiric Embrace heals Team 1 Warrior #2 for").is_empty(),
        "{}",
        played.log
    );
    assert!(lines(&played.log, PET_HEALED).is_empty(), "{}", played.log);
    // The Warlock deals no Arcane damage at all.
    let control = run(SpellSchool::Arcane);
    assert!(lines(&control.log, "Vampiric Embrace heals").is_empty());
}

/// The amounts after `marker` (" for N") on every line containing `needle`.
fn amounts(log: &str, needle: &str, marker: &str) -> Vec<f32> {
    lines(log, needle)
        .iter()
        .filter_map(|l| {
            l.split(marker)
                .nth(1)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .collect()
}

/// A leech heals off the damage the hit LANDS on its victim's health — after
/// the damage pipeline's modifiers (AS-408), its transfers and its absorbs —
/// not off the damage its source computed. Each Siphon Life tick that lands
/// heals the Warlock exactly what the tick's line says it dealt: twice the
/// plain tick on a Warrior that takes double damage, half of it on a Warrior whose
/// teammate takes half its damage for it. The Warrior and its Priest are
/// held stunned so nothing else heals, cuts healing or dispels. (That an
/// absorbed hit feeds nothing is `leech.rs`'s unit tests'.)
#[test]
fn a_leech_heals_off_the_damage_the_pipeline_lands() {
    const TICKS: &str = "Warlock #1's Siphon Life ticks for";
    const HEALS: &str = "Warlock #1's Siphon Life heals for";
    let held = || Aura {
        effect_type: AuraType::Stun,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Held".to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let run = |modifier: Option<Aura>| {
        let mut siphon = leeching(
            "Siphon Life",
            Some(Leech {
                fraction: 1.0,
                to: LeechRecipient::Caster,
                of: LeechSource::Ability,
            }),
        );
        siphon.magnitude = 20.0;
        play(
            config(&["Warlock"], &["Warrior", "Priest"], 20.0),
            Stage::new()
                .health(T1, 0.2)
                // No pre-match shield to soak the ticks.
                .clear_auras(T2)
                .aura(T2, held())
                .aura(Unit::new(2, 1), held())
                .world(move |w| {
                    let warlock = T1.entity(w);
                    let warrior = T2.entity(w);
                    let priest = Unit::new(2, 1).entity(w);
                    let mut auras = w
                        .get_mut::<arenasim::states::play_match::ActiveAuras>(warrior)
                        .expect("auras");
                    siphon.caster = Some(warlock);
                    auras.auras.push(siphon);
                    if let Some(mut m) = modifier {
                        if m.effect_type == AuraType::DamageTransfer {
                            m.caster = Some(priest);
                        }
                        auras.auras.push(m);
                    }
                }),
        )
    };
    let read = |label: &str, modifier: Option<Aura>| -> f32 {
        let played = run(modifier);
        let ticks = amounts(&played.log, TICKS, " ticks for ");
        let heals = amounts(&played.log, HEALS, " heals for ");
        assert!(ticks.len() >= 3, "{label}: {}", played.log);
        assert_eq!(ticks, heals, "{label}: {}", played.log);
        if label == "transferred" {
            assert!(
                !lines(&played.log, "[TRANSFER]").is_empty(),
                "{}",
                played.log
            );
        }
        assert!(ticks.iter().all(|t| *t == ticks[0]), "{label}: {ticks:?}");
        ticks[0]
    };
    let plain = read("plain", None);
    let doubled = read(
        "doubled",
        Some(Aura {
            effect_type: AuraType::DamageTakenIncrease,
            duration: 300.0,
            magnitude: 1.0,
            ability_name: "Vulnerable".to_string(),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        }),
    );
    let transferred = read(
        "transferred",
        Some(Aura {
            effect_type: AuraType::DamageTransfer,
            duration: 300.0,
            magnitude: 0.5,
            ability_name: "Sacrifice".to_string(),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        }),
    );
    assert!(doubled >= plain * 2.0 - 1.0, "{plain} -> {doubled}");
    assert!(transferred <= plain / 2.0 + 1.0, "{plain} -> {transferred}");
}

/// Death Coil's lifesteal is the leech field now: the shipped Death Coil
/// heals the Warlock it was fired by.
#[test]
fn death_coil_leeches_through_the_field() {
    let def = AbilityDefinitions::shipped().get_unchecked(&AbilityType::DeathCoil);
    assert_eq!(
        def.applies_aura.as_ref().and_then(|a| a.leech),
        Some(Leech {
            fraction: 1.0,
            to: LeechRecipient::Caster,
            of: LeechSource::Ability,
        })
    );
    // A Warlock hurt but not dying, beside a Warrior, so it lives to peel.
    // The enemy Priest has no mana and its Warrior no shield: a coil wholly
    // absorbed deals nothing, so leeches nothing.
    let played = play(
        config(&["Warlock", "Warrior"], &["Warrior", "Priest"], 60.0),
        Stage::new()
            .health(T1, 0.7)
            .clear_auras(T2)
            .mana(Unit::new(2, 1), 0.0),
    );
    let coils =
        lines(&played.log, "Death Coil hits").len() + lines(&played.log, "Death Coil CRITS").len();
    assert!(coils > 0, "{}", played.log);
    assert!(
        lines(&played.log, "Death Coil hits")
            .iter()
            .all(|l| !l.contains(" for 0 damage")),
        "{}",
        played.log
    );
    assert_eq!(lines(&played.log, "'s Death Coil heals for").len(), coils);
}

/// A Death Coil that lands after its Warlock has died heals nobody: a leech
/// heals only a living recipient. The coil is staged in flight, on top of
/// the enemy Warrior, from a Warlock already dead (its own Death Coil held,
/// so the staged one is the only one); the control is the same coil from a
/// living, hurt Warlock.
#[test]
fn a_death_coil_landing_after_its_warlock_died_heals_nothing() {
    let run = |dead: bool| {
        play(
            config(&["Warlock", "Warrior"], &["Warrior"], 12.0),
            Stage::new()
                .cooldown(T1, AbilityType::DeathCoil, 1000.0)
                .world(move |w| {
                    use arenasim::states::match_config::CharacterClass;
                    use arenasim::states::play_match::{Combatant, PlayMatchEntity, Projectile};
                    let warlock = T1.entity(w);
                    let warrior = T2.entity(w);
                    {
                        let mut c = w.get_mut::<Combatant>(warlock).expect("the Warlock");
                        if dead {
                            c.current_health = 0.0;
                            c.is_dead = true;
                        } else {
                            c.current_health = c.max_health * 0.5;
                        }
                    }
                    let at = w
                        .get::<bevy::prelude::Transform>(warrior)
                        .expect("the Warrior")
                        .translation;
                    w.spawn((
                        Projectile {
                            caster: warlock,
                            target: warrior,
                            ability: AbilityType::DeathCoil,
                            speed: 40.0,
                            caster_team: 1,
                            caster_slot: 0,
                            caster_class: CharacterClass::Warlock,
                            caster_pet_type: None,
                            next_cast: None,
                        },
                        bevy::prelude::Transform::from_translation(at),
                        PlayMatchEntity,
                    ));
                }),
        )
    };
    let dead = run(true);
    assert!(
        !lines(&dead.log, "Warlock #1's Death Coil hits").is_empty()
            || !lines(&dead.log, "Warlock #1's Death Coil CRITS").is_empty(),
        "{}",
        dead.log
    );
    assert!(
        lines(&dead.log, "Death Coil heals").is_empty(),
        "{}",
        dead.log
    );
    let alive = run(false);
    assert!(
        !lines(&alive.log, "Warlock #1's Death Coil heals for").is_empty(),
        "{}",
        alive.log
    );
}

// ---------------------------------------------------------------------------
// Every site, one by one
// ---------------------------------------------------------------------------

/// A sure-crit modifier on `ability` alone, for a unit whose own crit chance
/// is nothing: the first `ability` after the modifier is spent crits only if
/// the site that rolls it reads the ability's own crit chance.
fn sure_crit_on(ability: AbilityType) -> Aura {
    let mut m = modifier(NextCastScope::Abilities(vec![ability]));
    m.crit_chance = 1.0;
    Aura {
        effect_type: AuraType::NextCast,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Sure Crit".to_string(),
        next_cast: Some(m),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// The first line naming `name`'s landing after `name` spent the modifier.
fn landing_after_spend(log: &str, name: &str) -> String {
    let after = log
        .split(&format!("Sure Crit is spent on {name}"))
        .nth(1)
        .unwrap_or_else(|| panic!("{name} never spent the modifier: {log}"));
    after
        .lines()
        .find(|l| {
            l.contains(&format!("'s {name} "))
                && !l.contains(&format!("'s {name} heals for"))
                && (l.contains(" hits ") || l.contains(" heals ") || l.contains("CRIT"))
        })
        .unwrap_or_else(|| panic!("{name} never landed after spending it: {after}"))
        .to_string()
}

/// Every crit roll an ability makes reads its own crit chance
/// (`AbilityConfig::crit_chance`, which a crit patch and a next-cast modifier
/// raise): the class AI's instants (Mortal Strike, Sinister Strike, Frost
/// Nova), a cast's damage (Mind Blast) and Holy Shock's pending effect. The
/// cast heal and the projectile are pinned above.
#[test]
fn every_crit_roll_reads_the_abilitys_own_crit_chance() {
    let cases: [(&[&str], &[&str], AbilityType, &str, Option<Unit>); 8] = [
        // An instant projectile carries what it spent to its impact.
        (
            &["Warlock"],
            &["Warrior"],
            AbilityType::DeathCoil,
            "Death Coil",
            None,
        ),
        (
            &["Warrior"],
            &["Priest"],
            AbilityType::MortalStrike,
            "Mortal Strike",
            None,
        ),
        (
            &["Rogue"],
            &["Priest"],
            AbilityType::SinisterStrike,
            "Sinister Strike",
            None,
        ),
        (&["Rogue"], &["Priest"], AbilityType::Ambush, "Ambush", None),
        (
            &["Mage"],
            &["Warrior"],
            AbilityType::FrostNova,
            "Frost Nova",
            None,
        ),
        (
            &["Priest"],
            &["Warrior"],
            AbilityType::MindBlast,
            "Mind Blast",
            None,
        ),
        // Holy Shock's damage (nobody hurt) and its heal (a teammate dying).
        (
            &["Paladin"],
            &["Warrior"],
            AbilityType::HolyShock,
            "Holy Shock",
            None,
        ),
        (
            &["Paladin", "Warrior"],
            &["Warrior"],
            AbilityType::HolyShock,
            "Holy Shock",
            Some(T1B),
        ),
    ];
    for (team1, team2, ability, name, hurt) in cases {
        let mut stage = Stage::new()
            .aura(T1, sure_crit_on(ability))
            .combatant(T1, |c| {
                c.crit_chance = 0.0;
                // Open with Ambush (the default opener is Cheap Shot).
                c.rogue_opener = arenasim::states::match_config::RogueOpener::Ambush;
            });
        if let Some(hurt) = hurt {
            stage = stage.health(hurt, 0.3);
        }
        let played = play(config(team1, team2, 30.0), stage);
        let landing = landing_after_spend(&played.log, name);
        let healed = landing.contains(" heals ") || landing.contains("CRITICALLY heals");
        assert_eq!(healed, hurt.is_some(), "{name}: {landing}");
        assert!(landing.contains("CRIT"), "{name}: {landing}");
    }
}

/// A proc that fires for certain on `trigger` narrowed to `ability`.
fn on(trigger: TalentTrigger, ability: AbilityType) -> TalentProc {
    TalentProc {
        abilities: vec![ability],
        ..certain(trigger, "Site")
    }
}

/// Every site that pushes a trigger pushes it, each pinned by a proc
/// narrowed to an ability only that site lands: a cast's hit (Immolate),
/// a queued instant attack (Mortal Strike), Holy Shock's damage and its
/// critical heal, and a channel's tick (Drain Life). The rest are pinned by
/// the per-trigger tests above.
#[test]
fn every_site_pushes_its_trigger() {
    let cases: Vec<(&[&str], &[&str], TalentProc, Stage)> = vec![
        (
            &["Warlock"],
            &["Priest"],
            on(TalentTrigger::SpellHit, AbilityType::Immolate),
            Stage::new(),
        ),
        (
            &["Warrior"],
            &["Priest"],
            on(TalentTrigger::SpellHit, AbilityType::MortalStrike),
            Stage::new(),
        ),
        (
            &["Paladin"],
            &["Warrior"],
            on(TalentTrigger::SpellHit, AbilityType::HolyShock),
            Stage::new(),
        ),
        (
            &["Paladin", "Warrior"],
            &["Warrior"],
            on(TalentTrigger::HealCrit, AbilityType::HolyShock),
            Stage::new()
                .combatant(T1, |c| c.crit_chance = 1.0)
                .health(T1B, 0.3),
        ),
        (
            &["Warlock"],
            &["Priest"],
            on(TalentTrigger::DotTick, AbilityType::DrainLife),
            // Its Fear held: the Warlock waits out its own Fear on its
            // target (AS-436), which would hold the channel.
            Stage::new()
                .health(T1, 0.5)
                .cooldown(T1, AbilityType::Fear, 1000.0),
        ),
        (
            &["Mage"],
            &["Warrior"],
            on(TalentTrigger::SpellHit, AbilityType::FrostNova),
            Stage::new(),
        ),
    ];
    for (team1, team2, proc, stage) in cases {
        let what = format!("{:?} {:?}", proc.trigger, proc.abilities);
        let played = play(config(team1, team2, 30.0), with_proc(stage, T1, proc));
        assert!(
            !procs(&played.log, "Site").is_empty(),
            "{what}: {}",
            played.log
        );
    }
}

/// A queued instant attack's crit is its target's `CritTaken`: a Warrior
/// whose only crits are its Mortal Strikes (a sure-crit modifier on them,
/// nothing else) sets off the Priest's proc.
#[test]
fn a_crit_from_an_instant_attack_is_a_crit_taken() {
    let played = play(
        config(&["Warrior"], &["Priest"], 30.0),
        with_proc(
            Stage::new()
                .aura(T1, sure_crit_on(AbilityType::MortalStrike))
                .combatant(T1, |c| c.crit_chance = 0.0),
            T2,
            certain(TalentTrigger::CritTaken, "Struck"),
        ),
    );
    assert!(
        !lines(&played.log, "Mortal Strike CRITS").is_empty(),
        "{}",
        played.log
    );
    assert!(!procs(&played.log, "Struck").is_empty(), "{}", played.log);
}

/// A crit-taken heal-back reads what the crit landed past a TRIGGERED absorb
/// (AS-429): a Priest wearing Aegis of the Last Stand, staged just above its
/// 35% line, is crit below it and raises the shield; the crit that breaks
/// through the shield lands part of its damage, and the heal-back heals that
/// part, not the crit as rolled. Mortal Strike held, so no healing cut. The
/// crits here land at the auto-attack site (Heroic Strike rides the swing);
/// a queued strike's crit through an absorb is
/// `a_queued_strikes_crit_heal_back_reads_the_damage_past_an_absorb`.
#[test]
fn a_crit_taken_heal_back_reads_the_damage_past_a_triggered_absorb() {
    let mut recovery = certain(TalentTrigger::CritTaken, "");
    recovery.effect = ProcEffect::HealBack {
        fraction: 1.0,
        duration: 0.1,
        tick_interval: 0.1,
        name: Some("Blessed Recovery".to_string()),
    };
    let mut cfg = config(&["Warrior"], &["Priest"], 40.0);
    cfg.team2_equipment[0].insert("Trinket1".to_string(), "AegisOfTheLastStand".to_string());
    let held = Aura {
        effect_type: AuraType::Silence,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Held".to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let log = play(
        cfg,
        with_proc(Stage::new(), T2, recovery)
            .cooldown(T1, AbilityType::MortalStrike, 1000.0)
            .combatant(T1, |c| c.crit_chance = 1.0)
            .combatant(T2, |c| {
                c.max_health = 2_000.0;
                c.current_health = 760.0;
            })
            .clear_auras(T2)
            .aura(T2, held),
    )
    .log;
    assert!(
        !lines(&log, "gains Aegis of the Last Stand").is_empty(),
        "{log}"
    );
    let log_lines: Vec<&str> = log.lines().collect();
    let mut broke_through = 0;
    for (i, line) in log_lines.iter().enumerate() {
        // A swing's crit, or Heroic Strike's, which lands with the swing.
        if !(line.contains("Team 1 Warrior #1's ")
            && line.contains(" CRITS Team 2 Priest")
            && line.contains(" absorbed)"))
        {
            continue;
        }
        let landed: f32 = amounts(line, "CRITS", " for ")[0];
        if landed <= 0.0 {
            continue;
        }
        broke_through += 1;
        let healed = log_lines[i + 1..]
            .iter()
            .find(|l| l.contains(RECOVERED))
            .map(|l| amounts(l, RECOVERED, " for ")[0]);
        assert_eq!(healed, Some(landed), "{line}\n{log}");
    }
    assert!(
        broke_through >= 1,
        "no crit broke through the shield: {log}"
    );
}

/// A queued strike's crit heal-back reads what the strike landed past an
/// absorb: a Rogue whose only crit is one Sinister Strike (a sure-crit
/// modifier on it, no crit chance of its own) strikes a Priest behind a
/// shield the strike only partly eats, and the heal-back heals the part that
/// landed, not the strike as rolled. The Priest is held silenced and takes
/// double damage; nothing else heals it or cuts its healing (a Rogue, not a
/// Warrior: Mortal Strike's healing cut would shrink the heal).
#[test]
fn a_queued_strikes_crit_heal_back_reads_the_damage_past_an_absorb() {
    let shield = Aura {
        effect_type: AuraType::Absorb,
        duration: 300.0,
        magnitude: ABSORB_SIZE,
        ability_name: "Staged Shield".to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let log = heal_back_run_with(
        "Rogue",
        0.0,
        // In reach with full energy, so the strike comes before the swings
        // can spend the shield.
        Stage::new()
            .aura(T1, sure_crit_on(AbilityType::SinisterStrike))
            .place(T1, 0.0, 0.0)
            .place(T2, 2.0, 0.0)
            .combatant(T1, |c| c.current_mana = c.max_mana),
        Some(shield),
    );
    let strikes = lines(&log, "Rogue #1's Sinister Strike CRITS Team 2 Priest");
    assert_eq!(strikes.len(), 1, "{log}");
    let strike = strikes[0];
    assert!(strike.contains(" absorbed)"), "the shield took none: {log}");
    let landed = amounts(strike, "CRITS", " for ")[0];
    assert!(landed > 0.0, "the shield took it all: {strike}");
    let after = log.split(strike).nth(1).expect("the strike's line");
    let healed = after
        .lines()
        .find(|l| l.contains(RECOVERED))
        .map(|l| amounts(l, RECOVERED, " for ")[0]);
    assert_eq!(healed, Some(landed), "{strike}\n{log}");
}

/// The shield the queued-strike case stages: smaller than a sure-crit
/// Sinister Strike on a Priest taking double damage, larger than nothing.
const ABSORB_SIZE: f32 = 60.0;

/// A `CritTaken` proc's aura `on: Other` lands on the ATTACKER whose crit set
/// it off, never on the struck owner, at each site that pushes it: a Hunter's
/// shot (one sure-crit Arcane Shot), a Warrior's swing (every swing a crit,
/// Mortal Strike held) and a Warrior's queued strike (one sure-crit Mortal
/// Strike, no crit chance of its own).
#[test]
fn a_crit_taken_proc_lands_its_aura_on_the_attacker() {
    let mut marked = certain(TalentTrigger::CritTaken, "");
    marked.effect = ProcEffect::ApplyAura {
        on: ProcTarget::Other,
        aura_type: AuraType::AttackPowerIncrease,
        magnitude: 1.0,
        duration: 5.0,
        tick_interval: 0.0,
        dispel_type: DispelType::Physical,
        scope: None,
        charges: None,
        name: Some("Marked".to_string()),
    };
    let cases: [(&str, &str, Stage); 3] = [
        (
            "Hunter",
            "Arcane Shot CRITS",
            Stage::new()
                .aura(T1, sure_crit_on(AbilityType::ArcaneShot))
                .cooldown(T1, AbilityType::AimedShot, 1000.0)
                .combatant(T1, |c| c.crit_chance = 0.0)
                .combatant(Unit::pet_of(1, 0), |c| c.crit_chance = 0.0),
        ),
        (
            "Warrior",
            "Auto Attack CRITS",
            Stage::new()
                .cooldown(T1, AbilityType::MortalStrike, 1000.0)
                .combatant(T1, |c| c.crit_chance = 1.0),
        ),
        (
            "Warrior",
            "Mortal Strike CRITS",
            Stage::new()
                .aura(T1, sure_crit_on(AbilityType::MortalStrike))
                .combatant(T1, |c| c.crit_chance = 0.0),
        ),
    ];
    for (attacker, crit, stage) in cases {
        let log = play(
            config(&[attacker], &["Priest"], 30.0),
            with_proc(stage, T2, marked.clone()),
        )
        .log;
        assert!(!lines(&log, crit).is_empty(), "{crit}: {log}");
        assert!(!procs(&log, "Marked").is_empty(), "{crit}: {log}");
        assert!(
            !lines(&log, &format!("Team 1 {attacker} #1 gains Marked")).is_empty(),
            "{crit}: {log}"
        );
        assert!(
            lines(&log, "Team 2 Priest #1 gains Marked").is_empty(),
            "{crit}: {log}"
        );
    }
}

/// A shot's crit is a ranged crit, so its target's `CritTaken`, and the
/// heal-back reads what the crit LANDED: a Hunter whose only crit is one
/// Arcane Shot (a sure-crit modifier on it, no crit chance of its own or its
/// Spider's, so no Auto Shot or pet crits) sets off the Priest's Blessed
/// Recovery, which heals back exactly the shot's landed damage (Aimed Shot
/// held, so its healing cut does not shrink the heal). Arcane Shot is an Arcane attack, so the
/// site keys on the attack, not the school. The control: a Mage whose only
/// crit is one Frostbolt — a spell's crit, which is no `CritTaken`.
#[test]
fn a_shots_crit_is_a_crit_taken_and_a_spells_is_not() {
    let shot = heal_back_run_at(
        "Hunter",
        0.0,
        Stage::new()
            .aura(T1, sure_crit_on(AbilityType::ArcaneShot))
            .cooldown(T1, AbilityType::AimedShot, 1000.0)
            .combatant(Unit::pet_of(1, 0), |c| c.crit_chance = 0.0),
    );
    let crits = amounts(&shot, "Hunter #1's Arcane Shot CRITS", " for ");
    assert_eq!(crits.len(), 1, "{shot}");
    assert_eq!(procs(&shot, "Blessed Recovery").len(), 1, "{shot}");
    assert_eq!(amounts(&shot, RECOVERED, " for "), crits, "{shot}");

    let spell = heal_back_run_at(
        "Mage",
        0.0,
        Stage::new().aura(T1, sure_crit_on(AbilityType::Frostbolt)),
    );
    assert_eq!(
        lines(&spell, "Mage #1's Frostbolt CRITS").len(),
        1,
        "{spell}"
    );
    assert!(procs(&spell, "Blessed Recovery").is_empty(), "{spell}");
}

/// A projectile's `SpellHit` carries the damage the shot LANDED: a Hunter
/// whose Arcane Shot heals it back in full (a one-tick heal-back narrowed to
/// Arcane Shot), shooting a Priest that takes double damage, heals exactly
/// each shot's landed damage. The Hunter starts hurt, so every heal shows.
/// (Its crit-taken twin is `a_shots_crit_is_a_crit_taken_and_a_spells_is_not`.)
#[test]
fn a_projectiles_spell_hit_heal_back_reads_the_damage_it_landed() {
    let mut drink = certain(TalentTrigger::SpellHit, "");
    drink.abilities = vec![AbilityType::ArcaneShot];
    drink.effect = ProcEffect::HealBack {
        fraction: 1.0,
        duration: 0.1,
        tick_interval: 0.1,
        name: Some("Drink".to_string()),
    };
    let log = heal_back_run_at(
        "Hunter",
        0.0,
        with_proc(Stage::new(), T1, drink)
            .cooldown(T1, AbilityType::AimedShot, 1000.0)
            .combatant(T1, |c| {
                c.max_health = 10_000.0;
                c.current_health = 5_000.0;
            }),
    );
    let shots = amounts(&log, "Hunter #1's Arcane Shot hits", " for ");
    let healed = amounts(
        &log,
        "Hunter #1's Drink heals Team 1 Hunter #1 for",
        " for ",
    );
    // The match can end between the last shot and its heal.
    assert!(healed.len() >= 2, "{log}");
    assert!(shots.len() - healed.len() <= 1, "{log}");
    assert_eq!(healed, shots[..healed.len()], "{log}");
}

/// The Warlock's Shadow damage, ability by ability, with every other Shadow
/// ability held on cooldown — so each case's leech heals can come from one
/// site only.
const WARLOCK_SHADOW: [AbilityType; 6] = [
    AbilityType::Shadowbolt,
    AbilityType::Corruption,
    AbilityType::DrainLife,
    AbilityType::CurseOfAgony,
    AbilityType::UnstableAffliction,
    AbilityType::DeathCoil,
];

/// Every [`WARLOCK_SHADOW`] ability but `keep` held on cooldown, and the Fear
/// too: the Warlock waits out its own Fear on its target (AS-436), so a Fear
/// would hold the one site the case measures.
fn only(stage: Stage, who: Unit, keep: AbilityType) -> Stage {
    WARLOCK_SHADOW
        .iter()
        .filter(|a| **a != keep)
        .fold(stage, |s, a| s.cooldown(who, *a, 1000.0))
        .cooldown(who, AbilityType::Fear, 1000.0)
}

/// A silence held for the whole match.
fn held_silenced() -> Aura {
    Aura {
        effect_type: AuraType::Silence,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Held".to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// Drain Life owed by `warlock` to `target` from the gates on, by
/// construction: it channels only below 80% health on a target carrying a
/// DoT, below its Immolate and its Fear. So the target carries the
/// Warlock's Immolate (a FIRE DoT, which no Shadow leech reads), and the
/// Immolate recast and the Fear are held on cooldown. The Warlock's own
/// health, and a target that cannot dispel the Immolate, are the caller's.
fn drain_life_owed(stage: Stage, warlock: Unit, target: Unit) -> Stage {
    stage
        .cooldown(warlock, AbilityType::Immolate, 1000.0)
        .cooldown(warlock, AbilityType::Fear, 1000.0)
        .land(target, AbilityType::Immolate, warlock)
}

/// A Vampiric Embrace from `caster` on `bearer`.
fn embrace(stage: Stage, caster: Unit, bearer: Unit, school: SpellSchool) -> Stage {
    stage.world(move |w| {
        let caster = caster.entity(w);
        let bearer = bearer.entity(w);
        let aura = Aura {
            effect_type: AuraType::HealingReduction,
            duration: 300.0,
            magnitude: 1.0,
            caster: Some(caster),
            ability_name: "Vampiric Embrace".to_string(),
            leech: Some(Leech {
                fraction: 0.2,
                to: LeechRecipient::CasterParty,
                of: LeechSource::CasterSchool(school),
            }),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        };
        w.get_mut::<arenasim::states::play_match::ActiveAuras>(bearer)
            .expect("auras")
            .auras
            .push(aura);
    })
}

/// Every damage site reads a caster-school leech: a projectile (Shadow
/// Bolt), a DoT tick (Corruption), a channel tick (Drain Life) and a cast
/// (Immolate's direct hit, on a Fire embrace), each the only damage that can
/// feed it. A pet is never healed. A lone
/// Warlock is its own party, so its heals name no one.
#[test]
fn every_damage_site_feeds_a_caster_school_leech() {
    for keep in [
        AbilityType::Shadowbolt,
        AbilityType::Corruption,
        AbilityType::DrainLife,
    ] {
        // The Priest bare (no pre-match shield to eat a tick whole) and held
        // silenced, so it can neither shield, dispel nor heal what lands.
        let mut stage = only(Stage::new(), T1, keep)
            .health(T1, 0.5)
            .health(PET1, 0.5)
            .clear_auras(T2)
            .aura(T2, held_silenced());
        if keep == AbilityType::DrainLife {
            stage = drain_life_owed(stage, T1, T2);
        }
        let stage = embrace(stage, T1, T2, SpellSchool::Shadow);
        let played = play(config(&["Warlock"], &["Priest"], 30.0), stage);
        let shadow = match keep {
            AbilityType::Shadowbolt => "Shadow Bolt",
            AbilityType::Corruption => "Corruption",
            _ => "Drain Life",
        };
        assert!(
            !lines(&played.log, &format!("Warlock #1's {shadow}")).is_empty(),
            "{keep:?} never landed: {}",
            played.log
        );
        assert!(
            !lines(&played.log, "Warlock #1's Vampiric Embrace heals for").is_empty(),
            "{keep:?}: {}",
            played.log
        );
        assert!(
            lines(&played.log, PET_HEALED).is_empty(),
            "{keep:?}: {}",
            played.log
        );
    }
    // The cast site: Immolate's direct hit, with a FIRE embrace on the
    // enemy, so the heal that lands on the hit's own frame is the cast's (its
    // ticks come later, through the DoT site). The party a Warrior and the
    // Warlock's Felhunter, both hurt.
    let fire = play(
        config(&["Warlock", "Warrior"], &["Warrior"], 30.0),
        embrace(
            only(Stage::new(), T1, AbilityType::Immolate)
                .cooldown(T2, AbilityType::Pummel, 1000.0)
                .health(T1B, 0.5)
                .health(PET1, 0.5),
            T1,
            T2,
            SpellSchool::Fire,
        ),
    );
    let hits = stamps(&fire.log, "Warlock #1's Immolate hits");
    let heals = stamps(&fire.log, "Vampiric Embrace heals Team 1 Warrior #2 for");
    assert!(!hits.is_empty(), "{}", fire.log);
    assert!(
        hits.iter().any(|at| heals.contains(at)),
        "no heal on an Immolate hit's frame: {}",
        fire.log
    );
    assert!(lines(&fire.log, PET_HEALED).is_empty(), "{}", fire.log);
}

/// Every damage site leeches off the damage it LANDED, after the damage
/// pipeline (AS-408), not off what its source computed: on a Priest taking
/// double damage, a full-share embrace heals the Warlock exactly what each
/// of its hits landed — a projectile (Shadow Bolt), a DoT tick (Corruption),
/// a channel tick (Drain Life) and a cast (Immolate's direct hit, with its
/// ticks, on a Fire embrace). The Priest is held silenced so it
/// cannot dispel the embrace or fear the Warlock, both have health to spare,
/// and the Warlock has room to take every heal whole.
#[test]
fn every_damage_site_leeches_off_the_damage_it_landed() {
    for (keep, school, name) in [
        (AbilityType::Shadowbolt, SpellSchool::Shadow, "Shadow Bolt"),
        (AbilityType::Corruption, SpellSchool::Shadow, "Corruption"),
        (AbilityType::DrainLife, SpellSchool::Shadow, "Drain Life"),
        (AbilityType::Immolate, SpellSchool::Fire, "Immolate"),
    ] {
        let stage = only(Stage::new(), T1, keep)
            .combatant(T1, |c| {
                c.max_health = 100_000.0;
                c.current_health = 1_000.0;
            })
            .combatant(T2, |c| {
                c.max_health = 100_000.0;
                c.current_health = 100_000.0;
            })
            .place(T1, 0.0, -12.0)
            .place(T2, 0.0, 12.0)
            .clear_auras(T2)
            .aura(
                T2,
                Aura {
                    effect_type: AuraType::Silence,
                    duration: 300.0,
                    magnitude: 1.0,
                    ability_name: "Held".to_string(),
                    break_on_damage_threshold: -1.0,
                    ..Default::default()
                },
            )
            .aura(
                T2,
                Aura {
                    effect_type: AuraType::DamageTakenIncrease,
                    duration: 300.0,
                    magnitude: 1.0,
                    ability_name: "Vulnerable".to_string(),
                    break_on_damage_threshold: -1.0,
                    ..Default::default()
                },
            )
            .world(move |w| {
                let warlock = T1.entity(w);
                let priest = T2.entity(w);
                w.get_mut::<arenasim::states::play_match::ActiveAuras>(priest)
                    .expect("auras")
                    .auras
                    .push(Aura {
                        effect_type: AuraType::HealingReduction,
                        duration: 300.0,
                        magnitude: 1.0,
                        caster: Some(warlock),
                        ability_name: "Vampiric Embrace".to_string(),
                        leech: Some(Leech {
                            fraction: 1.0,
                            to: LeechRecipient::CasterParty,
                            of: LeechSource::CasterSchool(school),
                        }),
                        break_on_damage_threshold: -1.0,
                        ..Default::default()
                    });
            });
        let played = play(config(&["Warlock"], &["Priest"], 25.0), stage);
        let dealt: Vec<f32> = lines(&played.log, &format!("Warlock #1's {name}"))
            .iter()
            .filter(|l| l.contains("[DMG]"))
            .map(|l| {
                let n = l
                    .split(" for ")
                    .nth(1)
                    .and_then(|t| t.split_whitespace().next());
                n.and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| panic!("an amount on {l}"))
            })
            .collect();
        let healed = amounts(
            &played.log,
            "Warlock #1's Vampiric Embrace heals for",
            " heals for ",
        );
        assert!(dealt.len() >= 2, "{name} hardly landed: {}", played.log);
        // Every site's line, the channel's included, reports what landed.
        assert_eq!(
            dealt.iter().sum::<f32>(),
            healed.iter().sum::<f32>(),
            "{name}: dealt {dealt:?}, healed {healed:?}: {}",
            played.log
        );
    }
}

/// A leech declared on an ability travels onto the aura it applies, and the
/// aura's ticks feed it: the Warlock's own Corruption, given an `Ability`
/// leech in its book, heals the Warlock each tick. The control is the
/// shipped Corruption, which leeches nothing.
#[test]
fn an_abilitys_leech_rides_its_aura() {
    let run = |leech: bool| {
        play(
            config(&["Warlock"], &["Priest"], 30.0),
            Stage::new().health(T1, 0.5).combatant(T1, move |c| {
                if leech {
                    let book = c.book().clone().patching(AbilityType::Corruption, |d| {
                        d.applies_aura.as_mut().expect("Corruption's aura").leech = Some(Leech {
                            fraction: 1.0,
                            to: LeechRecipient::Caster,
                            of: LeechSource::Ability,
                        });
                    });
                    *c = c.clone().with_book(book);
                }
            }),
        )
    };
    let played = run(true);
    assert!(!lines(&played.log, "Corruption ticks").is_empty());
    assert!(
        !lines(&played.log, "Warlock #1's Corruption heals for").is_empty(),
        "{}",
        played.log
    );
    let control = run(false);
    assert!(lines(&control.log, "Corruption heals").is_empty());
}

// ---------------------------------------------------------------------------
// Round 2: a cast's three endings, each hand's own speed, the cooldown's
// release
// ---------------------------------------------------------------------------

/// The Priest mid-way through a Mind Blast at the enemy Warrior that spent a
/// sure-crit modifier ("Favor") when it began: the modifier is off its auras
/// and riding on the cast, as `after_turn` leaves it. `ending` stages how the
/// cast ends.
fn mid_cast(ending: impl FnOnce(&mut bevy::prelude::World) + Send + Sync + 'static) -> Stage {
    mid_cast_of(AbilityType::MindBlast, ending)
}

/// [`mid_cast`], for the team-1 caster's cast of `ability` at the Warrior.
fn mid_cast_of(
    ability: AbilityType,
    ending: impl FnOnce(&mut bevy::prelude::World) + Send + Sync + 'static,
) -> Stage {
    Stage::new().world(move |w| {
        use arenasim::states::play_match::next_cast::SpentNextCast;
        use arenasim::states::play_match::{CastingState, Combatant};
        let caster = T1.entity(w);
        let warrior = T2.entity(w);
        let mut m = modifier(NextCastScope::Abilities(vec![ability]));
        m.crit_chance = 1.0;
        let favor = Aura {
            effect_type: AuraType::NextCast,
            duration: 300.0,
            magnitude: 1.0,
            ability_name: "Favor".to_string(),
            next_cast: Some(m.clone()),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        };
        {
            let mut c = w.get_mut::<Combatant>(caster).expect("the caster");
            c.crit_chance = 0.0;
            c.target = Some(warrior);
            c.next_cast_spent = Some(SpentNextCast {
                caster,
                ability,
                modifier: m,
                auras: vec![favor],
            });
        }
        w.entity_mut(caster)
            .insert(CastingState::new(ability, warrior, 0.2));
        ending(w);
    })
}

fn mind_blast_config() -> HeadlessMatchConfig {
    config(&["Priest"], &["Warrior", "Priest"], 12.0)
}

const KEPT: &str = "Priest #1's Favor is kept: its Mind Blast did not land";

/// A cast that LANDS keeps the change: the Mind Blast crits, and the
/// modifier does not come back.
#[test]
fn a_spent_modifier_stays_spent_when_its_cast_lands() {
    let played = play(mind_blast_config(), mid_cast(|_| {}));
    let first = played
        .log
        .lines()
        .find(|l| l.contains("Priest #1's Mind Blast"))
        .unwrap_or_else(|| panic!("the Mind Blast lands: {}", played.log));
    assert!(first.contains("CRITS"), "{first}");
    assert!(lines(&played.log, KEPT).is_empty(), "{}", played.log);
}

/// An INTERRUPTED cast gives the modifier back (TBC restores a mod a
/// cancelled cast consumed), and costs nothing.
#[test]
fn an_interrupted_cast_gives_its_modifier_back() {
    let played = play(
        mind_blast_config(),
        mid_cast(|w| {
            let priest = T1.entity(w);
            let mut cast = w
                .get_mut::<arenasim::states::play_match::CastingState>(priest)
                .expect("the cast");
            cast.interrupted = true;
            cast.interrupted_display_time = 0.5;
        }),
    );
    assert_eq!(lines(&played.log, KEPT).len(), 1, "{}", played.log);
}

/// A cast CROWD CONTROL stops gives it back too.
#[test]
fn a_cast_crowd_control_stops_gives_its_modifier_back() {
    let played = play(
        mind_blast_config(),
        mid_cast(|_| {}).aura(
            T1,
            Aura {
                effect_type: AuraType::Stun,
                duration: 2.0,
                magnitude: 1.0,
                ability_name: "Test".to_string(),
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        ),
    );
    assert_eq!(lines(&played.log, KEPT).len(), 1, "{}", played.log);
}

/// A cast that FIZZLES at completion (its target dead) gives it back.
#[test]
fn a_fizzled_cast_gives_its_modifier_back() {
    let played = play(
        mind_blast_config(),
        mid_cast(|w| {
            let warrior = T2.entity(w);
            let mut c = w
                .get_mut::<arenasim::states::play_match::Combatant>(warrior)
                .expect("the Warrior");
            c.current_health = 0.0;
            c.is_dead = true;
        }),
    );
    assert_eq!(lines(&played.log, KEPT).len(), 1, "{}", played.log);
}

/// A cast its caster can no longer pay for at completion (a Mana Burn took
/// the mana mid-cast) fails, and gives it back (AS-438).
#[test]
fn a_cast_its_caster_cannot_pay_for_gives_its_modifier_back() {
    let played = play(
        mind_blast_config(),
        mid_cast(|w| {
            let priest = T1.entity(w);
            w.get_mut::<arenasim::states::play_match::Combatant>(priest)
                .expect("the Priest")
                .current_mana = 0.0;
        }),
    );
    assert_eq!(
        lines(
            &played.log,
            "Priest #1 fails to cast Mind Blast: not enough mana"
        )
        .len(),
        1,
        "{}",
        played.log
    );
    assert_eq!(lines(&played.log, KEPT).len(), 1, "{}", played.log);
}

/// A cast a SILENCE stops gives it back; the same cast unsilenced lands and
/// keeps it spent.
#[test]
fn a_cast_a_silence_stops_gives_its_modifier_back() {
    let silence = Aura {
        effect_type: AuraType::Silence,
        duration: 2.0,
        magnitude: 1.0,
        ability_name: "Test".to_string(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let played = play(mind_blast_config(), mid_cast(|_| {}).aura(T1, silence));
    assert_eq!(
        lines(&played.log, "Priest #1's Mind Blast interrupted by Silence").len(),
        1,
        "{}",
        played.log
    );
    assert_eq!(lines(&played.log, KEPT).len(), 1, "{}", played.log);
    let control = play(mind_blast_config(), mid_cast(|_| {}));
    assert!(lines(&control.log, "interrupted by Silence").is_empty());
    assert!(lines(&control.log, KEPT).is_empty(), "{}", control.log);
}

/// The Priest and the Warrior either side of a TwinPillars pillar (centred at
/// (9, 0), radius 2.5) when `blocked`; the same distance apart down the open
/// middle of the map otherwise. The enemy Priest stands well off the line.
fn across_a_pillar(stage: Stage, blocked: bool) -> Stage {
    let x = if blocked { 9.0 } else { 0.0 };
    stage
        .place(T1, x, -12.0)
        .place(T2, x, 12.0)
        .place(Unit::new(2, 1), -20.0, 15.0)
}

fn on_twin_pillars(mut config: HeadlessMatchConfig) -> HeadlessMatchConfig {
    config.map = "TwinPillars".to_string();
    config
}

/// An instant-effect cast whose target is out of sight at completion
/// fizzles and gives its modifier back; the same cast in sight lands with
/// the change and keeps it spent.
#[test]
fn a_cast_out_of_sight_at_completion_gives_its_modifier_back() {
    let run = |blocked| {
        play(
            on_twin_pillars(mind_blast_config()),
            across_a_pillar(mid_cast(|_| {}), blocked),
        )
    };
    let played = run(true);
    assert_eq!(
        lines(
            &played.log,
            "Priest #1 fails to cast Mind Blast: target out of line of sight"
        )
        .len(),
        1,
        "{}",
        played.log
    );
    assert_eq!(lines(&played.log, KEPT).len(), 1, "{}", played.log);
    let control = run(false);
    let first = control
        .log
        .lines()
        .find(|l| l.contains("Priest #1's Mind Blast"))
        .unwrap_or_else(|| panic!("the Mind Blast lands: {}", control.log));
    assert!(first.contains("CRITS"), "{first}");
    assert!(lines(&control.log, KEPT).is_empty(), "{}", control.log);
}

/// A projectile cast whose target is out of sight at completion fizzles
/// before the bolt flies and gives its modifier back; the same Frostbolt in
/// sight flies, lands with the change, and keeps it spent.
#[test]
fn a_projectile_cast_out_of_sight_at_completion_gives_its_modifier_back() {
    const FROST_KEPT: &str = "Mage #1's Favor is kept: its Frostbolt did not land";
    let run = |blocked| {
        play(
            on_twin_pillars(config(&["Mage"], &["Warrior", "Priest"], 12.0)),
            across_a_pillar(mid_cast_of(AbilityType::Frostbolt, |_| {}), blocked),
        )
    };
    let played = run(true);
    assert_eq!(
        lines(
            &played.log,
            "Mage #1 fails to cast Frostbolt: target out of line of sight"
        )
        .len(),
        1,
        "{}",
        played.log
    );
    assert_eq!(lines(&played.log, FROST_KEPT).len(), 1, "{}", played.log);
    let control = run(false);
    let first = control
        .log
        .lines()
        .find(|l| l.contains("Mage #1's Frostbolt"))
        .unwrap_or_else(|| panic!("the Frostbolt lands: {}", control.log));
    assert!(first.contains("CRITS"), "{first}");
    assert!(
        lines(&control.log, FROST_KEPT).is_empty(),
        "{}",
        control.log
    );
}

/// A cast whose target no longer resolves at completion gives it back. No
/// system despawns a combatant while a match runs, so the stage removes the
/// Warrior itself: this pins the defensive branch, not a path play reaches.
#[test]
fn a_cast_whose_target_is_gone_gives_its_modifier_back() {
    let played = play(
        mind_blast_config(),
        mid_cast(|w| {
            let warrior = T2.entity(w);
            w.despawn(warrior);
        }),
    );
    assert_eq!(lines(&played.log, KEPT).len(), 1, "{}", played.log);
}

/// The total of an auto-attack line's damage and its absorb.
fn hit_total(line: &str) -> f32 {
    let after = line.split(" for ").nth(1).unwrap_or("0");
    let dealt: f32 = after
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0.0);
    let absorbed: f32 = line
        .split('(')
        .nth(1)
        .and_then(|p| p.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0.0);
    dealt + absorbed
}

/// The `[PROC]` lines' timestamps (`[ 12.34s]`).
fn stamps<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines()
        .filter(|l| l.contains(needle))
        .filter_map(|l| l.split(']').next())
        .collect()
}

/// Each hand's per-minute chance follows ITS weapon: a Rogue whose off hand
/// is a slow 4.0s weapon (certain at 15 a minute) and whose main hand is a
/// fast 1.0s one (a quarter). Every off-hand hit procs; a main-hand hit only
/// sometimes. The hands are told apart by damage: the main hand hits for far
/// more.
#[test]
fn each_hand_procs_at_its_own_weapon_speed() {
    let mut fury = certain(TalentTrigger::MeleeHit, "Fury");
    fury.chance = ProcChance::PerMinute(15.0);
    let played = play(
        config(&["Rogue"], &["Priest"], 60.0),
        with_proc(Stage::new(), T1, fury).combatant(T1, |c| {
            c.crit_chance = 0.0;
            c.weapon_speed = 1.0;
            c.attack_damage = 12.0;
            c.offhand_weapon_speed = 4.0;
            c.offhand_damage = 2.0;
        }),
    );
    let proc_at: std::collections::BTreeSet<&str> = stamps(&played.log, "Test Talent procs Fury")
        .into_iter()
        .collect();
    let swings: Vec<(&str, f32)> = played
        .log
        .lines()
        .filter(|l| l.contains("Rogue #1's Auto Attack"))
        .map(|l| (l.split(']').next().unwrap(), hit_total(l)))
        .collect();
    let (off, main): (Vec<_>, Vec<_>) = swings.iter().partition(|(_, dmg)| *dmg < 5.0);
    assert!(off.len() >= 4 && main.len() >= 10, "{swings:?}");
    for (at, dmg) in &off {
        assert!(
            proc_at.contains(at),
            "an off-hand hit for {dmg} at {at} did not proc: {}",
            played.log
        );
    }
    let main_procs = main.iter().filter(|(at, _)| proc_at.contains(at)).count();
    assert!(main_procs < main.len(), "every main-hand hit procced");
}

/// A Hunter's melee hand procs at its MELEE weapon's speed, not its bow's: a
/// 4.0s melee weapon is certain at 15 a minute where the 1.0s bow would be a
/// quarter. Rooted beside a Warrior, every melee hit procs.
#[test]
fn a_hunters_melee_hand_procs_at_its_melee_weapons_speed() {
    let mut fury = certain(TalentTrigger::MeleeHit, "Fury");
    fury.chance = ProcChance::PerMinute(15.0);
    let played = play(
        config(&["Hunter"], &["Warrior"], 30.0),
        with_proc(Stage::new(), T1, fury)
            .combatant(T1, |c| {
                c.weapon_speed = 1.0;
                c.melee_damage = 20.0;
                c.melee_weapon_speed = 4.0;
            })
            .aura(
                T1,
                Aura {
                    effect_type: AuraType::Root,
                    duration: 300.0,
                    magnitude: 1.0,
                    ability_name: "Test".to_string(),
                    break_on_damage_threshold: -1.0,
                    ..Default::default()
                },
            )
            .place(T1, 0.0, 0.0)
            .place(T2, 1.5, 0.0)
            .target(T1, T2)
            // Rooted beside a Warrior, the Hunter must live long enough for
            // three 4.0s melee swings.
            .combatant(T1, |c| {
                c.max_health *= 3.0;
                c.current_health = c.max_health;
            }),
    );
    let proc_at: std::collections::BTreeSet<&str> = stamps(&played.log, "Test Talent procs Fury")
        .into_iter()
        .collect();
    let melee: Vec<&str> = stamps(&played.log, "Hunter #1's Auto Attack");
    assert!(melee.len() >= 3, "{}", played.log);
    for at in melee {
        assert!(
            proc_at.contains(at),
            "a melee hit at {at} did not proc: {}",
            played.log
        );
    }
}

/// A proc's internal cooldown runs out and lets it fire again: a certain
/// melee proc on a 3s cooldown fires again and again, never twice inside 3s.
#[test]
fn an_internal_cooldown_releases_its_proc() {
    let mut fury = certain(TalentTrigger::MeleeHit, "Fury");
    fury.internal_cooldown = 3.0;
    let played = play(
        config(&["Warrior"], &["Priest"], 30.0),
        with_proc(Stage::new(), T1, fury),
    );
    let at: Vec<f32> = stamps(&played.log, "Test Talent procs Fury")
        .iter()
        .map(|s| {
            s.trim_start_matches('[')
                .trim_end_matches('s')
                .trim()
                .parse()
                .unwrap()
        })
        .collect();
    assert!(at.len() >= 3, "{}", played.log);
    for pair in at.windows(2) {
        assert!(pair[1] - pair[0] >= 3.0 - 0.02, "{at:?}");
    }
}

/// Holy Shock's pending effect lands with the book its decision read: a
/// power modifier Holy Shock spent (here +900%) is in its damage.
#[test]
fn holy_shock_lands_with_the_modifier_it_spent() {
    let run = |power: f32| {
        let mut m = modifier(NextCastScope::Abilities(vec![AbilityType::HolyShock]));
        m.power = power;
        if power == 0.0 {
            m.cost_reduction = 1.0;
        }
        play(
            config(&["Paladin"], &["Warrior"], 30.0),
            Stage::new()
                .aura(
                    T1,
                    Aura {
                        effect_type: AuraType::NextCast,
                        duration: 300.0,
                        magnitude: 1.0,
                        ability_name: "Sure Crit".to_string(),
                        next_cast: Some(m),
                        break_on_damage_threshold: -1.0,
                        ..Default::default()
                    },
                )
                .combatant(T1, |c| c.crit_chance = 0.0),
        )
    };
    let damage = |log: &str| -> f32 { hit_total(&landing_after_spend(log, "Holy Shock")) };
    let plain = damage(&run(0.0).log);
    let empowered = damage(&run(9.0).log);
    assert!(empowered > plain * 5.0, "{plain} -> {empowered}");
}

/// An interrupt is a use: Kick spends a modifier that covers it and fires an
/// `AbilityUsed` proc narrowed to it (Initiative's shape).
#[test]
fn an_interrupt_is_a_use_and_a_spender() {
    let mut cut = modifier(NextCastScope::Abilities(vec![AbilityType::Kick]));
    cut.cost_reduction = 5.0;
    let primed = Aura {
        effect_type: AuraType::NextCast,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Initiative".to_string(),
        next_cast: Some(cut),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let played = play(
        config(&["Rogue"], &["Priest"], 40.0),
        with_proc(
            Stage::new().aura(T1, primed).health(T2, 0.5),
            T1,
            on(TalentTrigger::AbilityUsed, AbilityType::Kick),
        ),
    );
    assert!(
        !lines(&played.log, "uses Kick").is_empty(),
        "{}",
        played.log
    );
    assert!(
        !lines(&played.log, "Initiative is spent on Kick").is_empty(),
        "{}",
        played.log
    );
    assert!(!procs(&played.log, "Site").is_empty(), "{}", played.log);
}

/// An interrupt decides from the effective book: a Rogue short of Kick's 25
/// energy kicks a casting Priest when a primed cost cut covers Kick, and is
/// refused for energy without it (a scene, one tick).
#[test]
fn an_interrupt_reads_the_effective_book() {
    use arenasim::states::match_config::CharacterClass;
    use arenasim::states::play_match::class_ai::scene::Scene;
    let scene = |cut: bool| {
        let mut scene = Scene::new()
            .unit("rogue", 1, CharacterClass::Rogue)
            .mana(0.22)
            .targeting("priest")
            .unit("priest", 2, CharacterClass::Priest)
            .at(2.0, 0.0)
            .casting("priest", AbilityType::FlashHeal);
        if cut {
            let mut m = modifier(NextCastScope::Abilities(vec![AbilityType::Kick]));
            m.cost_reduction = 10.0;
            scene = scene.aura_full(
                "rogue",
                Aura {
                    effect_type: AuraType::NextCast,
                    duration: 30.0,
                    magnitude: 1.0,
                    ability_name: "Initiative".to_string(),
                    next_cast: Some(m),
                    ..Default::default()
                },
            );
        }
        scene.combatant_mut("rogue").stealthed = false;
        scene.interrupt("rogue")
    };
    let cut = scene(true);
    assert_eq!(
        cut.chosen.map(|(a, _)| a),
        Some(AbilityType::Kick),
        "{:?}",
        cut.candidates
    );
    assert_ne!(scene(false).chosen.map(|(a, _)| a), Some(AbilityType::Kick));
}
