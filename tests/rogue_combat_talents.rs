//! AS-418 — the Rogue's Combat tree: the anti-melee specialist.
//!
//! - **the tree**, as the card writes it: every node, its place, its ranks,
//!   its effect and its price;
//! - **the spawn**: a Combat build reaches the spawned Rogue — dodge, parry
//!   and melee haste added in points, off-hand damage, Sinister Strike's
//!   damage, Vitality's health and Expose Armor in its book;
//! - **the parry**: the Rogue is the first class that can parry, and a
//!   Deflection Rogue's parry is rolled against a real Warrior's swings;
//! - **Expose Armor's rule**, in scene pairs: on an armored kill target, not
//!   a lightly armored one, not one already exposed, and on the kill window's
//!   enemy ahead of the burst on it.

mod common;

use std::collections::BTreeSet;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::build_headless_app;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{self, Mage, Priest, Rogue, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::rogue::{expose_armor_gain, EXPOSE_ARMOR_MIN_GAIN};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{Aura, AuraType, Combatant, Pet};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::equipment::{load_default_loadouts, load_item_definitions};
use arenasim::states::play_match::situation::{Play, PlaySet, CHAIN};
use arenasim::states::play_match::talents::{
    AbilityField, Build, PatchOp, PatchTarget, TalentDefinitions, TalentEffect, TalentId,
    TalentStat, TreeId, RANK_PRICE,
};
use arenasim::states::play_match::AbilityType;
use bevy::prelude::*;
use common::staged::play;
use TalentId::*;

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// Every Combat node as the card writes it: its place, its ranks, what a
/// rank does, its declared price and its prerequisite. Vitality's fraction is
/// the one that prices it at exactly a rank, and is held to that below.
#[test]
fn the_combat_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let rogue: BTreeSet<TalentId> = talents.talents_for_class(Rogue).map(|(id, _)| id).collect();
    assert_eq!(
        rogue,
        [
            LightningReflexes,
            DualWieldSpecialization,
            Aggression,
            Vitality,
            Deflection,
            SwiftBlades,
            ExposeArmor
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        talents
            .trees(Rogue)
            .iter()
            .map(|t| (t.id, t.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(TreeId::Combat, "Combat")]
    );

    let stat = |s, f| vec![TalentEffect::StatPercent(s, f)];
    let expected: [(
        TalentId,
        u8,
        u8,
        u8,
        Vec<TalentEffect>,
        Option<f32>,
        Option<(TalentId, u8)>,
    ); 6] = [
        (
            LightningReflexes,
            1,
            0,
            2,
            stat(TalentStat::DodgeChance, 0.04),
            Some(11.0),
            None,
        ),
        (
            DualWieldSpecialization,
            1,
            2,
            2,
            stat(TalentStat::OffHandDamage, 0.10),
            Some(11.0),
            None,
        ),
        (
            Aggression,
            2,
            0,
            2,
            vec![TalentEffect::Patch {
                on: PatchTarget::Ability(AbilityType::SinisterStrike),
                field: AbilityField::Damage,
                op: PatchOp::Mul(1.06),
            }],
            Some(11.0),
            None,
        ),
        (
            Deflection,
            2,
            2,
            2,
            stat(TalentStat::ParryChance, 0.04),
            Some(11.0),
            None,
        ),
        (
            SwiftBlades,
            3,
            1,
            1,
            stat(TalentStat::MeleeAttackSpeed, 0.10),
            Some(11.0),
            None,
        ),
        (
            ExposeArmor,
            4,
            1,
            1,
            vec![TalentEffect::Grant(AbilityType::ExposeArmor)],
            Some(22.0),
            Some((SwiftBlades, 1)),
        ),
    ];
    for (id, tier, column, ranks, per_rank, budget, requires) in expected {
        let t = talents.get(id).unwrap();
        assert_eq!(
            (t.tree, t.tier, t.column, t.max_rank),
            (TreeId::Combat, tier, column, ranks),
            "{id:?}"
        );
        assert_eq!(t.per_rank, per_rank, "{id:?}");
        assert_eq!(t.budget, budget, "{id:?}");
        assert_eq!(t.requires, requires, "{id:?}");
    }

    let vitality = talents.get(Vitality).unwrap();
    assert_eq!(
        (
            vitality.tier,
            vitality.column,
            vitality.max_rank,
            vitality.budget
        ),
        (2, 1, 1, None)
    );
    let items = load_item_definitions().unwrap();
    let loadouts = load_default_loadouts(&items).unwrap();
    let reference = talents
        .reference_combatant(Rogue, abilities(), &items, &loadouts)
        .unwrap();
    let price = talents.rank_price(Vitality, Some(&reference)).unwrap();
    assert!(
        (price - RANK_PRICE).abs() < 1e-3,
        "Vitality prices at {price}, not a rank"
    );
}

fn abilities() -> &'static arenasim::states::play_match::ability_config::AbilityDefinitions {
    arenasim::states::play_match::ability_config::AbilityDefinitions::shipped()
}

// ---------------------------------------------------------------------------
// The spawn
// ---------------------------------------------------------------------------

/// The Rogue a headless match spawns (team 2, against a Warrior) with `build`.
fn spawned_rogue(build: &Build) -> Combatant {
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Rogue".into()],
        random_seed: Some(7),
        team2_builds: vec![build.clone()],
        ..Default::default()
    };
    let mut app = build_headless_app(&config, true, None);
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<(&Combatant, Option<&Pet>)>();
    let rogue = q
        .iter(world)
        .find(|(c, pet)| pet.is_none() && c.team == 2 && c.class == Rogue)
        .map(|(c, _)| c.clone());
    rogue.expect("a Rogue spawned")
}

/// Every Combat node, at the cap, reaches the spawned Rogue: dodge, parry and
/// melee haste ADDED in points (every class's base is 0, so a percentage of
/// it would be nothing), Vitality's share of its health, Sinister Strike's
/// whole damage, and Expose Armor in its book. The haste is the melee one: the
/// haste every attack shares stays 0.
#[test]
fn a_combat_build_reaches_the_spawned_rogue() {
    let base = spawned_rogue(&Build::default());
    assert_eq!(
        (
            base.dodge_chance,
            base.parry_chance,
            base.melee_attack_speed_bonus
        ),
        (0.0, 0.0, 0.0)
    );
    assert!(base.book().get(&AbilityType::ExposeArmor).is_none());

    let deep = spawned_rogue(&Build::of(&[
        (LightningReflexes, 2),
        (Deflection, 2),
        (SwiftBlades, 1),
        (ExposeArmor, 1),
        (Vitality, 1),
        (Aggression, 1),
    ]));
    assert_eq!(deep.dodge_chance, 0.04 * 2.0);
    assert_eq!(deep.parry_chance, 0.04 * 2.0);
    assert_eq!(deep.melee_attack_speed_bonus, 0.10);
    assert_eq!(deep.attack_speed_bonus, 0.0);
    let vitality = match talents_effect(Vitality) {
        TalentEffect::StatPercent(TalentStat::MaxHealth, f) => f,
        other => panic!("Vitality is {other:?}"),
    };
    assert_eq!(deep.max_health, base.max_health * (1.0 + vitality));
    assert_eq!(deep.current_health, deep.max_health);
    assert!(deep.book().get(&AbilityType::ExposeArmor).is_some());
    let ss = |c: &Combatant| {
        let d = c.book().get_unchecked(&AbilityType::SinisterStrike);
        (d.damage_base_min, d.damage_base_max, d.damage_coefficient)
    };
    let near = |is: f32, want: f32| (is - want).abs() <= want.abs() * 1e-5;
    let (min, max, coef) = ss(&base);
    let (smin, smax, scoef) = ss(&deep);
    assert!(near(smin, min * 1.06) && near(smax, max * 1.06) && near(scoef, coef * 1.06));
    // Nothing the build does not name moved.
    assert_eq!(deep.offhand_damage, base.offhand_damage);
    assert_eq!(deep.attack_damage, base.attack_damage);
    assert_eq!(deep.armor, base.armor);

    // Dual Wield Specialization raises the off hand alone; two ranks of
    // Aggression add, never compound.
    let dual = spawned_rogue(&Build::of(&[(DualWieldSpecialization, 2), (Aggression, 2)]));
    assert!(base.offhand_damage > 0.0, "the Rogue dual-wields");
    assert_eq!(
        dual.offhand_damage,
        base.offhand_damage * (1.0 + 0.10 * 2.0)
    );
    assert_eq!(dual.attack_damage, base.attack_damage);
    assert_eq!(dual.dodge_chance, 0.0);
    let (dmin, dmax, dcoef) = ss(&dual);
    assert!(near(dmin, min * 1.12) && near(dmax, max * 1.12) && near(dcoef, coef * 1.12));
}

fn talents_effect(id: TalentId) -> TalentEffect {
    TalentDefinitions::shipped().get(id).unwrap().per_rank[0].clone()
}

// ---------------------------------------------------------------------------
// The parry
// ---------------------------------------------------------------------------

/// Units that cannot die, so the match runs its whole span.
fn unkillable() -> Stage {
    let tough = |c: &mut Combatant| {
        c.max_health = 1_000_000.0;
        c.current_health = 1_000_000.0;
    };
    Stage::new()
        .combatant(Unit::new(1, 0), tough)
        .combatant(Unit::new(2, 0), tough)
}

/// The `[AVOID]` verbs the Rogue answered the Warrior with.
fn rogue_avoidances(build: &Build) -> Vec<String> {
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Rogue".into()],
        random_seed: Some(3),
        max_duration_secs: 240.0,
        team2_builds: vec![build.clone()],
        ..Default::default()
    };
    let played = play(config, unkillable());
    played
        .log
        .lines()
        .filter_map(|l| l.split_once("[AVOID] Team 2 Rogue #1 ").map(|(_, r)| r))
        .map(|r| r.split(' ').next().unwrap().to_string())
        .collect()
}

/// A Lightning Reflexes and Deflection Rogue dodges AND parries a Warrior's
/// melee in a real match — the parry roll is reached for a Rogue defender —
/// and the same Rogue untalented avoids nothing.
#[test]
fn a_combat_rogue_dodges_and_parries_a_warrior() {
    let talented = rogue_avoidances(&Build::of(&[(LightningReflexes, 2), (Deflection, 2)]));
    let dodges = talented.iter().filter(|v| *v == "dodges").count();
    let parries = talented.iter().filter(|v| *v == "parries").count();
    assert!(dodges > 0 && parries > 0, "{talented:?}");
    assert_eq!(dodges + parries, talented.len());
    assert!(rogue_avoidances(&Build::default()).is_empty());
}

// ---------------------------------------------------------------------------
// Expose Armor
// ---------------------------------------------------------------------------

const ME: &str = "rogue";
const FOE: &str = "enemy";
const OTHER: &str = "other enemy";
const ENEMY_HEALER: &str = "enemy priest";

/// Plate on the starting kit (the default Warrior wears about 2300).
const PLATE: f32 = 2300.0;
/// Leather on the starting kit (about 770).
const LEATHER: f32 = 770.0;

/// A Rogue with Expose Armor in melee of an enemy `class` wearing `armor`,
/// its target, with Kidney Shot on cooldown: the planner above Expose Armor
/// leaves the turn to it.
fn expose_scene(class: CharacterClass, armor: f32) -> Scene {
    let mut s = Scene::new()
        .unit(ME, 1, Rogue)
        .mana(1.0)
        .targeting(FOE)
        .unit(FOE, 2, class)
        .at(1.5, 0.0)
        .armor(armor)
        .grant(ME, AbilityType::ExposeArmor)
        .cooldown(ME, AbilityType::KidneyShot, 10.0);
    s.combatant_mut(ME).stealthed = false;
    s
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn note(view: &DecisionView) -> Option<String> {
    match view.rejected(AbilityType::ExposeArmor) {
        Some(RejectionReason::PreconditionUnmet { note }) => Some(note.to_string()),
        _ => None,
    }
}

/// A plate-wearing kill target is exposed; its leather-wearing control is not
/// (halving its armor adds under [`EXPOSE_ARMOR_MIN_GAIN`]), and the Rogue
/// strikes it instead.
#[test]
fn expose_armor_goes_on_an_armored_target_and_not_a_light_one() {
    let view = expose_scene(Warrior, PLATE).decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::ExposeArmor, Some(FOE))),
        "{}",
        describe(&view)
    );
    assert!(view
        .auras_queued
        .contains(&(FOE, AuraType::ArmorPercentReduction)));

    let control = expose_scene(Warrior, LEATHER).decide(ME);
    assert_ne!(
        control.chose().map(|c| c.0),
        Some(AbilityType::ExposeArmor),
        "{}",
        describe(&control)
    );
    assert_eq!(
        note(&control).as_deref(),
        Some("target's armor is too light to expose"),
        "{}",
        describe(&control)
    );
    assert!(!control
        .auras_queued
        .contains(&(FOE, AuraType::ArmorPercentReduction)));
}

/// A target already exposed is not exposed again (the strongest reduction is
/// the only one that counts); its control, the same target bare, is.
#[test]
fn expose_armor_is_not_spent_on_a_target_already_exposed() {
    let exposed = Aura {
        effect_type: AuraType::ArmorPercentReduction,
        magnitude: 0.5,
        duration: 10.0,
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let view = expose_scene(Warrior, PLATE)
        .aura_full(FOE, exposed)
        .decide(ME);
    assert_ne!(
        view.chose().map(|c| c.0),
        Some(AbilityType::ExposeArmor),
        "{}",
        describe(&view)
    );
    assert_eq!(
        note(&view).as_deref(),
        Some("target's armor is already exposed")
    );
    let control = expose_scene(Warrior, PLATE).decide(ME);
    assert_eq!(control.chose().map(|c| c.0), Some(AbilityType::ExposeArmor));
}

/// The gain the rule reads, against the armor formula: halving plate's 2300
/// adds about 17%, leather's 770 about 6.5%; the threshold sits between mail
/// (about 10% at 1216) and leather.
#[test]
fn the_gain_is_what_halving_the_armor_adds_to_a_physical_hit() {
    let def = abilities().get_unchecked(&AbilityType::ExposeArmor);
    let gain = |armor: f32| expose_armor_gain(armor, &[], def);
    let formula = |armor: f32| (armor + 5500.0) / (armor / 2.0 + 5500.0) - 1.0;
    for armor in [0.0, 359.0, LEATHER, 1216.0, PLATE] {
        assert!((gain(armor) - formula(armor)).abs() < 1e-5, "{armor}");
    }
    assert!(gain(1216.0) >= EXPOSE_ARMOR_MIN_GAIN);
    assert!(gain(LEATHER) < EXPOSE_ARMOR_MIN_GAIN);
    assert_eq!(gain(0.0), 0.0);
}

/// The kill window scene (`tests/burst_scenes.rs`'s shape): the Rogue's own
/// target is a cloth Mage beside it, and the team's kill window names a low,
/// plate-wearing Warrior whose Priest is stunned. Expose Armor goes on the
/// window's Warrior, ahead of the burst on it.
fn window_scene() -> Scene {
    let mut s = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .except_everywhere(1, PlaySet::of(&[CHAIN]))
        .unit(ME, 1, Rogue)
        .mana(1.0)
        .targeting(OTHER)
        .unit(FOE, 2, Warrior)
        .at(1.5, 0.0)
        .health(0.2)
        .armor(PLATE)
        .unit(OTHER, 2, Mage)
        .at(0.0, 1.5)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(1.5, 30.0)
        .grant(ME, AbilityType::ExposeArmor)
        // So the control's planner leaves its turn to Expose Armor's rule.
        .cooldown(ME, AbilityType::KidneyShot, 10.0);
    s.combatant_mut(ME).stealthed = false;
    s.aura(ENEMY_HEALER, AuraType::Stun, 4.0)
}

/// Under the Burst play, Expose Armor goes on the kill window's enemy, not
/// the Rogue's own target; with the play excepted (its control) the rule
/// reads the Rogue's own target, the cloth Mage, and does not expose it.
#[test]
fn expose_armor_goes_on_the_kill_window_enemy_ahead_of_the_burst() {
    let view = window_scene().decide(ME);
    assert!(
        view.situation
            .as_ref()
            .is_some_and(|s| s.kill_window.is_some()),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.chose(),
        Some((AbilityType::ExposeArmor, Some(FOE))),
        "{}",
        describe(&view)
    );

    let control = window_scene()
        .except_everywhere(1, PlaySet::of(&[Play::Burst, CHAIN]))
        .decide(ME);
    assert_ne!(
        control.chose().map(|c| c.0),
        Some(AbilityType::ExposeArmor),
        "{}",
        describe(&control)
    );
    assert_eq!(
        note(&control).as_deref(),
        Some("target's armor is too light to expose"),
        "{}",
        describe(&control)
    );
}

/// In a real match the Combat Rogue's Expose Armor lands on the Warrior it
/// fights (plate, from its starting kit) and lasts its 15 seconds; the same
/// Rogue untalented never puts one there.
#[test]
fn expose_armor_lands_on_a_warrior_in_a_match_for_fifteen_seconds() {
    let runs = |build: &Build| -> Vec<f32> {
        let config = HeadlessMatchConfig {
            team1: vec!["Warrior".into()],
            team2: vec!["Rogue".into()],
            random_seed: Some(5),
            max_duration_secs: 60.0,
            team2_builds: vec![build.clone()],
            ..Default::default()
        };
        let played = play(config, unkillable());
        // The length of every unbroken run of frames the Warrior wears it.
        let mut runs = Vec::new();
        let mut start: Option<f32> = None;
        let mut last = 0.0;
        for frame in &played.frames {
            let exposed = frame.combatants.values().any(|c| {
                c.class == Warrior && c.aura_types.contains(&AuraType::ArmorPercentReduction)
            });
            match (exposed, start) {
                (true, None) => start = Some(frame.sim_time),
                (false, Some(s)) => {
                    runs.push(last - s);
                    start = None;
                }
                _ => {}
            }
            last = frame.sim_time;
        }
        if let Some(s) = start {
            runs.push(last - s);
        }
        runs
    };
    let talented = runs(&Build::of(&[
        (LightningReflexes, 2),
        (Deflection, 2),
        (SwiftBlades, 1),
        (ExposeArmor, 1),
    ]));
    assert!(!talented.is_empty(), "Expose Armor never landed");
    // Every run but a last one the match's end cut short is the full 15s.
    let (last, whole) = talented.split_last().unwrap();
    for run in whole {
        assert!((run - 15.0).abs() < 0.05, "a run of {run}s: {talented:?}");
    }
    assert!(*last <= 15.0 + 0.05, "{talented:?}");
    assert!(runs(&Build::default()).is_empty());
}

// ---------------------------------------------------------------------------
// Expose Armor and the Kidney Shot planner
// ---------------------------------------------------------------------------

/// A Rogue (`Legacy`) at `energy` in melee of its target, a plate Paladin
/// whose Holy school is locked out for `lockout` more seconds, with Expose
/// Armor granted or not; Kidney Shot ready.
fn kidney_scene(expose: bool, energy: f32, lockout: Option<f32>) -> Scene {
    let mut s = Scene::new()
        .unit(ME, 1, Rogue)
        .mana(energy)
        .targeting(FOE)
        .unit(FOE, 2, CharacterClass::Paladin)
        .at(1.5, 0.0)
        .armor(PLATE);
    if let Some(secs) = lockout {
        s = s.aura_full(
            FOE,
            Aura {
                effect_type: AuraType::SpellSchoolLockout,
                magnitude: arenasim::states::play_match::abilities::SpellSchool::Holy
                    .to_lockout_magnitude(),
                duration: secs,
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        );
    }
    if expose {
        s = s.grant(ME, AbilityType::ExposeArmor);
    }
    s.combatant_mut(ME).stealthed = false;
    s
}

/// The planner's FIRE moment is Kidney Shot's: as a Holy lockout lapses, the
/// Rogue extends the denial with the stun whether it has Expose Armor or not.
#[test]
fn expose_armor_never_takes_the_kidney_shot_planners_moment() {
    let control = kidney_scene(false, 1.0, Some(0.3)).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::KidneyShot, Some(FOE))),
        "{}",
        describe(&control)
    );
    let view = kidney_scene(true, 1.0, Some(0.3)).decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::KidneyShot, Some(FOE))),
        "{}",
        describe(&view)
    );
}

/// The energy the planner POOLS is Kidney Shot's: two seconds out from the
/// lockout's end at 50 energy the Rogue holds everything, Expose Armor
/// included, so the stun is affordable when the lockout lapses.
#[test]
fn expose_armor_never_spends_energy_pooled_for_kidney_shot() {
    let control = kidney_scene(false, 0.5, Some(2.0)).decide(ME);
    assert_eq!(control.chose(), None, "{}", describe(&control));
    let view = kidney_scene(true, 0.5, Some(2.0)).decide(ME);
    assert_eq!(view.chose(), None, "{}", describe(&view));
    assert!(
        view.candidates
            .iter()
            .all(|c| c.ability != AbilityType::ExposeArmor),
        "{}",
        describe(&view)
    );
}

/// Where the planner leaves the turn to the strike — the lockout's denial
/// still running, the stun held — Expose Armor takes it ahead of Sinister
/// Strike; without the talent the Rogue strikes.
#[test]
fn expose_armor_goes_ahead_of_sinister_strike_when_the_planner_holds() {
    let control = kidney_scene(false, 1.0, Some(3.0)).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::SinisterStrike, Some(FOE))),
        "{}",
        describe(&control)
    );
    assert!(control.rejected(AbilityType::KidneyShot).is_some());
    let view = kidney_scene(true, 1.0, Some(3.0)).decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::ExposeArmor, Some(FOE))),
        "{}",
        describe(&view)
    );
    assert!(view.rejected(AbilityType::KidneyShot).is_some());
}

/// While a burst is called, Expose Armor is the kill window's alone: with
/// that enemy out of reach, the Rogue does not expose its own plate target
/// instead (it strikes it), and Expose Armor is decided once, on the window's
/// enemy.
#[test]
fn under_a_burst_expose_armor_is_for_the_window_enemy_only() {
    let mut s = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .except_everywhere(1, PlaySet::of(&[CHAIN]))
        .unit(ME, 1, Rogue)
        .mana(1.0)
        .targeting(OTHER)
        .unit(FOE, 2, Warrior)
        .at(6.0, 0.0)
        .health(0.2)
        .armor(PLATE)
        .unit(OTHER, 2, Warrior)
        .at(0.0, 1.5)
        .armor(PLATE)
        .unit(ENEMY_HEALER, 2, Priest)
        .at(6.0, 30.0)
        .grant(ME, AbilityType::ExposeArmor)
        .cooldown(ME, AbilityType::KidneyShot, 10.0);
    s.combatant_mut(ME).stealthed = false;
    let view = s.aura(ENEMY_HEALER, AuraType::Stun, 4.0).decide(ME);
    assert!(
        view.situation
            .as_ref()
            .is_some_and(|s| s.kill_window.is_some()),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.chose(),
        Some((AbilityType::SinisterStrike, Some(OTHER))),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.candidates
            .iter()
            .filter(|c| c.ability == AbilityType::ExposeArmor)
            .count(),
        1,
        "{}",
        describe(&view)
    );
}
