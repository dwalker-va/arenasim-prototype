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

fn config(team1: &[&str], team2: &[&str], secs: f32) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        random_seed: Some(7),
        max_duration_secs: secs,
        ..Default::default()
    }
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
    let crits = play(
        config(&["Warrior"], &["Priest"], 20.0),
        with_proc(Stage::new(), T2, recovery.clone()).combatant(T1, |c| c.crit_chance = 1.0),
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
        name: Some("Aftermath".to_string()),
    };
    let played = play(
        config(&["Warlock"], &["Priest"], 30.0),
        with_proc(Stage::new(), T1, aftermath.clone()),
    );
    assert!(
        !lines(&played.log, "Shadow Bolt hits").is_empty()
            || !lines(&played.log, "Shadow Bolt CRITS").is_empty()
    );
    assert!(
        !procs(&played.log, "Aftermath").is_empty(),
        "{}",
        played.log
    );

    aftermath.abilities = vec![AbilityType::LifeTap];
    let control = play(
        config(&["Warlock"], &["Priest"], 30.0),
        with_proc(Stage::new(), T1, aftermath),
    );
    assert!(procs(&control.log, "Aftermath").is_empty());
}

/// `DotTick` (Nightfall) on Corruption's ticks, priming Shadow Trance; the
/// next Shadow Bolt spends it.
#[test]
fn a_dot_tick_proc_primes_a_next_cast_that_shadow_bolt_spends() {
    let played = play(
        config(&["Warlock"], &["Priest"], 30.0),
        with_proc(Stage::new(), T1, nightfall(1.0)),
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
        abilities: vec![],
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
            Stage::new().health(T1B, 0.5).world(move |w| {
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
    // The Warlock deals no Arcane damage at all.
    let control = run(SpellSchool::Arcane);
    assert!(lines(&control.log, "Vampiric Embrace heals").is_empty());
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
    let played = play(
        config(&["Warlock", "Warrior"], &["Warrior", "Priest"], 60.0),
        Stage::new().health(T1, 0.7),
    );
    let coils =
        lines(&played.log, "Death Coil hits").len() + lines(&played.log, "Death Coil CRITS").len();
    assert!(coils > 0, "{}", played.log);
    assert_eq!(lines(&played.log, "'s Death Coil heals for").len(), coils);
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
    let cases: [(&[&str], &[&str], AbilityType, &str, Option<Unit>); 7] = [
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
            Stage::new().health(T1, 0.5),
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

fn only(stage: Stage, who: Unit, keep: AbilityType) -> Stage {
    WARLOCK_SHADOW
        .iter()
        .filter(|a| **a != keep)
        .fold(stage, |s, a| s.cooldown(who, *a, 1000.0))
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
/// (the Priest's Mind Blast), each the caster's only Shadow damage. A lone
/// Warlock is its own party, so its heals name no one.
#[test]
fn every_damage_site_feeds_a_caster_school_leech() {
    for keep in [
        AbilityType::Shadowbolt,
        AbilityType::Corruption,
        AbilityType::DrainLife,
    ] {
        let stage = embrace(
            only(Stage::new(), T1, keep).health(T1, 0.5),
            T1,
            T2,
            SpellSchool::Shadow,
        );
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
    }
    let priest = play(
        config(&["Priest", "Warrior"], &["Warrior"], 30.0),
        embrace(Stage::new().health(T1B, 0.5), T1, T2, SpellSchool::Shadow),
    );
    assert!(
        !lines(&priest.log, "Mind Blast").is_empty(),
        "{}",
        priest.log
    );
    assert!(
        !lines(&priest.log, "Vampiric Embrace heals Team 1 Warrior #2 for").is_empty(),
        "{}",
        priest.log
    );
}
