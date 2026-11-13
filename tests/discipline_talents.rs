//! AS-414 — the Priest's Discipline tree: shields and saves.
//!
//! - **the tree**, as the card writes it: every node, its place, its ranks,
//!   its effect, its price and its prerequisite;
//! - **the spawn**: a Discipline build reaches the spawned Priest — the three
//!   grants in its book, Power Word: Shield's absorb, the instants' costs,
//!   and the two mechanics on the combatant;
//! - **Unbreakable Will**: a stun or a fear landed on the Priest runs 20%
//!   shorter at 2/2, a horror and an incapacitate do not;
//! - **Reflective Shield**, from the build: a shield the Priest casts returns
//!   30% of its soak to the attacker;
//! - **the AI rules**, in scene pairs (the situation, and its control with its
//!   one condition removed): Pain Suppression, Power Infusion and Inner Focus,
//!   in the healing chain and the Shadowform chain alike.

mod common;

use std::collections::BTreeSet;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::build_headless_app;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{Mage, Priest, Warrior};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{Aura, AuraType, Combatant, DRCategory, Pet};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::next_cast::{NextCastModifier, NextCastScope};
use arenasim::states::play_match::talent_mechanics::{unbreakable_will_scales, TalentMechanic};
use arenasim::states::play_match::talents::{
    AbilityField, Build, PatchOp, PatchTarget, TalentDefinitions, TalentEffect, TalentId, TreeId,
};
use arenasim::states::play_match::AbilityType::{
    self, DispelMagic, FlashHeal, PowerWordFortitude, PowerWordShield, PsychicScream,
};
use common::staged::play;
use TalentId::*;

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// Every Discipline node as the card writes it: its place, its ranks, what a
/// rank does, its declared price and its prerequisite.
#[test]
fn the_discipline_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let discipline: BTreeSet<TalentId> = talents
        .talents_for_class(Priest)
        .filter(|(_, t)| t.tree == TreeId::Discipline)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        discipline,
        [
            UnbreakableWill,
            ImprovedPowerWordShield,
            InnerFocus,
            MentalAgility,
            PowerInfusion,
            ReflectiveShield,
            PainSuppression,
        ]
        .into_iter()
        .collect()
    );
    // Discipline reads first, as WoW's "5/0/3" does.
    assert_eq!(
        talents
            .trees(Priest)
            .iter()
            .map(|t| (t.id, t.name.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (TreeId::Discipline, "Discipline"),
            (TreeId::Holy, "Holy"),
            (TreeId::Shadow, "Shadow")
        ]
    );

    let patch = |a, field, op| TalentEffect::Patch {
        on: PatchTarget::Ability(a),
        field,
        op,
    };
    let agility = |a| patch(a, AbilityField::ManaCost, PatchOp::Mul(0.95));
    let expected: [(
        TalentId,
        u8,
        u8,
        u8,
        Vec<TalentEffect>,
        f32,
        Option<(TalentId, u8)>,
    ); 7] = [
        (
            UnbreakableWill,
            1,
            0,
            2,
            vec![TalentEffect::Mechanic(TalentMechanic::UnbreakableWill {
                reduction: 0.10,
            })],
            11.0,
            None,
        ),
        (
            ImprovedPowerWordShield,
            1,
            2,
            2,
            vec![
                patch(
                    PowerWordShield,
                    AbilityField::AuraMagnitude,
                    PatchOp::Mul(1.05),
                ),
                patch(
                    PowerWordShield,
                    AbilityField::AuraMagnitudeCoefficient,
                    PatchOp::Mul(1.05),
                ),
            ],
            11.0,
            None,
        ),
        (
            InnerFocus,
            2,
            0,
            1,
            vec![TalentEffect::Grant(AbilityType::InnerFocus)],
            11.0,
            None,
        ),
        (
            MentalAgility,
            2,
            1,
            2,
            vec![
                agility(PowerWordShield),
                agility(PsychicScream),
                agility(DispelMagic),
                agility(PowerWordFortitude),
            ],
            11.0,
            None,
        ),
        (
            PowerInfusion,
            3,
            0,
            1,
            vec![TalentEffect::Grant(AbilityType::PowerInfusion)],
            11.0,
            None,
        ),
        (
            ReflectiveShield,
            3,
            1,
            1,
            vec![TalentEffect::Mechanic(TalentMechanic::ReflectiveShield {
                share: 0.30,
            })],
            11.0,
            None,
        ),
        (
            PainSuppression,
            4,
            1,
            1,
            vec![TalentEffect::Grant(AbilityType::PainSuppression)],
            22.0,
            Some((ReflectiveShield, 1)),
        ),
    ];
    for (id, tier, column, ranks, effects, budget, requires) in expected {
        let node = talents.get(id).unwrap();
        assert_eq!(
            (
                node.tree,
                node.tier,
                node.column,
                node.max_rank,
                &node.per_rank,
                node.budget,
                node.requires
            ),
            (
                TreeId::Discipline,
                tier,
                column,
                ranks,
                &effects,
                Some(budget),
                requires
            ),
            "{id:?}"
        );
    }
}

/// The three grants are the tree's `talent: true` abilities, with the values
/// the card states (TBC's where it gives one): Inner Focus free and on no
/// global cooldown, Power Infusion +24 spell power for 15s, Pain Suppression
/// 40% less damage for 8s; 45s cooldowns all.
#[test]
fn the_grants_are_as_specified() {
    let defs = AbilityDefinitions::shipped();
    let focus = defs.get_unchecked(&AbilityType::InnerFocus);
    let aura = focus.applies_aura.as_ref().unwrap();
    assert!(focus.talent);
    assert_eq!((focus.mana_cost, focus.cooldown), (0.0, 45.0));
    assert_eq!(aura.aura_type, AuraType::NextCast);
    assert_eq!(
        aura.next_cast,
        Some(NextCastModifier {
            applies_to: NextCastScope::Any,
            instant: false,
            free: true,
            crit_chance: 0.25,
            cast_time_reduction: 0.0,
            cost_reduction: 0.0,
            power: 0.0,
        })
    );

    let infusion = defs.get_unchecked(&AbilityType::PowerInfusion);
    let aura = infusion.applies_aura.as_ref().unwrap();
    assert!(infusion.talent);
    assert_eq!(infusion.cooldown, 45.0);
    assert_eq!(
        (aura.aura_type, aura.magnitude, aura.duration),
        (AuraType::SpellPowerIncrease, 24.0, 15.0)
    );

    let suppression = defs.get_unchecked(&AbilityType::PainSuppression);
    let aura = suppression.applies_aura.as_ref().unwrap();
    assert!(suppression.talent);
    assert_eq!(suppression.cooldown, 45.0);
    assert_eq!(
        (aura.aura_type, aura.magnitude, aura.duration),
        (AuraType::DamageTakenReduction, 0.40, 8.0)
    );
}

// ---------------------------------------------------------------------------
// The spawn
// ---------------------------------------------------------------------------

/// The Priest a headless match spawns (team 2, against a Warrior) with `build`.
fn spawned_priest(build: &Build) -> Combatant {
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Priest".into()],
        random_seed: Some(7),
        team2_builds: vec![build.clone()],
        ..Default::default()
    };
    let mut app = build_headless_app(&config, true, None);
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<(&Combatant, Option<&Pet>)>();
    let priest = q
        .iter(world)
        .find(|(c, pet)| pet.is_none() && c.team == 2 && c.class == Priest)
        .map(|(c, _)| c.clone());
    priest.expect("a Priest spawned")
}

/// The full Discipline capstone path: two ranks each of Unbreakable Will and
/// Improved Power Word: Shield, then Inner Focus, Reflective Shield, Power
/// Infusion and Pain Suppression — 8 points.
fn capstone_build() -> Build {
    Build::of(&[
        (UnbreakableWill, 2),
        (ImprovedPowerWordShield, 2),
        (InnerFocus, 1),
        (ReflectiveShield, 1),
        (PowerInfusion, 1),
        (PainSuppression, 1),
    ])
}

/// A Discipline build reaches the spawned Priest: the three grants in its
/// book, a shield 10% stronger (the base and the spell-power share alike:
/// 73 -> 81 on today's starting kit), and both mechanics at their ranks. Mental
/// Agility takes 10% off each of the four instants at 2/2, and nothing else.
#[test]
fn a_discipline_build_reaches_the_spawned_priest() {
    let base = spawned_priest(&Build::default());
    for a in [
        AbilityType::InnerFocus,
        AbilityType::PowerInfusion,
        AbilityType::PainSuppression,
    ] {
        assert!(base.book().get(&a).is_none(), "{a:?}");
    }
    assert!(base.talent_mechanics.is_empty());

    let deep = spawned_priest(&capstone_build());
    for a in [
        AbilityType::InnerFocus,
        AbilityType::PowerInfusion,
        AbilityType::PainSuppression,
    ] {
        assert!(deep.book().get(&a).is_some(), "{a:?}");
    }
    let shield = |c: &Combatant| {
        let aura = c
            .book()
            .get_unchecked(&PowerWordShield)
            .applies_aura
            .clone()
            .unwrap();
        aura.magnitude + aura.magnitude_coefficient * c.spell_power
    };
    assert!((shield(&deep) - shield(&base) * 1.10).abs() < 1e-3);
    assert_eq!(deep.talent_mechanics.unbreakable_will(), Some(0.20));
    assert_eq!(deep.talent_mechanics.reflective_shield(), Some(0.30));

    let agile = spawned_priest(&Build::of(&[(UnbreakableWill, 2), (MentalAgility, 2)]));
    let cost = |c: &Combatant, a: AbilityType| c.book().get_unchecked(&a).mana_cost;
    for a in [
        PowerWordShield,
        PsychicScream,
        DispelMagic,
        PowerWordFortitude,
    ] {
        assert!(
            (cost(&agile, a) - cost(&base, a) * 0.90).abs() < 1e-4,
            "{a:?}: {} from {}",
            cost(&agile, a),
            cost(&base, a)
        );
    }
    assert_eq!(cost(&agile, FlashHeal), cost(&base, FlashHeal));
}

// ---------------------------------------------------------------------------
// Unbreakable Will
// ---------------------------------------------------------------------------

/// The crowd control it shortens, by diminishing-returns category: every stun
/// and fear and the silence, never the horror, an incapacitate, a cyclone, a
/// root or a slow.
#[test]
fn unbreakable_will_shortens_exactly_stuns_fears_and_silences() {
    let all = [
        DRCategory::Stuns,
        DRCategory::Fears,
        DRCategory::Incapacitates,
        DRCategory::Roots,
        DRCategory::Slows,
        DRCategory::Silence,
        DRCategory::KidneyShotStun,
        DRCategory::Horror,
        DRCategory::Cyclone,
    ];
    let shortened: BTreeSet<u8> = all
        .into_iter()
        .filter(|c| unbreakable_will_scales(*c))
        .map(|c| c as u8)
        .collect();
    let expected: BTreeSet<u8> = [
        DRCategory::Stuns,
        DRCategory::KidneyShotStun,
        DRCategory::Fears,
        DRCategory::Silence,
    ]
    .into_iter()
    .map(|c| c as u8)
    .collect();
    assert_eq!(shortened, expected);
}

/// A reduction its ranks would carry to 100% or past would be immunity, and
/// the loader refuses it; 10% at two ranks is fine.
#[test]
fn unbreakable_will_never_reaches_immunity() {
    let will = |reduction| TalentMechanic::UnbreakableWill { reduction };
    assert!(will(0.10).validate(Priest, 2).is_ok());
    assert!(will(0.50).validate(Priest, 2).is_err());
    assert!(will(0.0).validate(Priest, 2).is_err());
}

const T1: Unit = Unit::new(1, 0);
const T2: Unit = Unit::new(2, 0);
const PRIEST_ID: &str = "Team 2 Priest #1";

/// The `[CC]` line `ability` landed on the Priest with, and its duration.
fn cc_line(log: &str, name: &str) -> (f32, String) {
    let head = format!("{name} on {PRIEST_ID} (");
    let line = log
        .lines()
        .find(|l| l.contains(&head))
        .unwrap_or_else(|| panic!("no {name} landed on the Priest:\n{log}"));
    let rest = &line[line.find(&head).unwrap() + head.len()..];
    let secs: f32 = rest[..rest.find('s').unwrap()].parse().unwrap();
    (secs, line.to_string())
}

/// Through the real landing site, from the build: Hammer of Justice and Fear
/// land on a 2/2 Unbreakable Will Priest 20% shorter, and the log says so;
/// Death Coil's horror and Polymorph land at full length. The control, with
/// no build, lands every one at full length.
#[test]
fn unbreakable_will_shortens_a_stun_and_a_fear_landed_on_the_priest() {
    let defs = AbilityDefinitions::shipped();
    let full = |a: AbilityType| {
        defs.get_unchecked(&a)
            .applies_aura
            .as_ref()
            .unwrap()
            .duration
    };
    let run = |build: Build| {
        let config = HeadlessMatchConfig {
            team1: vec!["Warlock".into()],
            team2: vec!["Priest".into()],
            random_seed: Some(5),
            max_duration_secs: 3.0,
            team2_builds: vec![build],
            ..Default::default()
        };
        let stage = Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T2, 20.0, 0.0)
            .land(T2, AbilityType::HammerOfJustice, T1)
            .land(T2, AbilityType::Fear, T1)
            .land(T2, AbilityType::DeathCoil, T1)
            .land(T2, AbilityType::Polymorph, T1);
        play(config, stage).log
    };
    let will = run(Build::of(&[(UnbreakableWill, 2)]));
    let control = run(Build::default());
    for (ability, name, scaled) in [
        (AbilityType::HammerOfJustice, "Hammer of Justice", true),
        (AbilityType::Fear, "Fear", true),
        (AbilityType::DeathCoil, "Death Coil", false),
        (AbilityType::Polymorph, "Polymorph", false),
    ] {
        let (with, line) = cc_line(&will, name);
        let (without, control_line) = cc_line(&control, name);
        assert_eq!(without, full(ability), "{control_line}");
        assert!(!control_line.contains("Unbreakable Will"), "{control_line}");
        if scaled {
            assert!((with - full(ability) * 0.8).abs() < 0.051, "{line}");
            assert!(line.contains("(Unbreakable Will: 20% shorter)"), "{line}");
        } else {
            assert_eq!(with, full(ability), "{line}");
            assert!(!line.contains("Unbreakable Will"), "{line}");
        }
    }
}

// ---------------------------------------------------------------------------
// Reflective Shield, from the build
// ---------------------------------------------------------------------------

/// A Warrior beating on a Priest whose build takes Reflective Shield: the
/// shield the Priest casts on itself returns its share of the soak, logged as
/// a reflect; with no build, nothing is reflected.
#[test]
fn reflective_shield_from_the_build_reflects_the_priests_shield() {
    let run = |build: Build| {
        let config = HeadlessMatchConfig {
            team1: vec!["Warrior".into()],
            team2: vec!["Priest".into()],
            random_seed: Some(3),
            max_duration_secs: 8.0,
            team2_builds: vec![build],
            ..Default::default()
        };
        let stage = Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T2, 2.0, 0.0)
            .clear_auras(T2)
            .land(T2, PowerWordShield, T2);
        play(config, stage).log
    };
    let reflecting = run(capstone_build());
    assert!(
        reflecting
            .lines()
            .any(|l| l.contains("[REFLECT] Team 1 Warrior #1 takes")
                && l.contains("Power Word: Shield")),
        "{reflecting}"
    );
    assert!(!run(Build::default()).contains("[REFLECT]"));
}

// ---------------------------------------------------------------------------
// The AI rules
// ---------------------------------------------------------------------------

const FOCUS: AbilityType = AbilityType::InnerFocus;
const INFUSION: AbilityType = AbilityType::PowerInfusion;
const SUPPRESSION: AbilityType = AbilityType::PainSuppression;

const PRIEST: &str = "priest";
const ALLY: &str = "ally";
const ENEMY: &str = "enemy warrior";

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; situation {:?}; candidates {:?}",
        view.chosen,
        view.situation,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

fn precondition(view: &DecisionView, ability: AbilityType) -> bool {
    matches!(
        view.rejected(ability),
        Some(RejectionReason::PreconditionUnmet { .. })
    )
}

fn fortitude() -> Aura {
    Aura {
        effect_type: AuraType::MaxHealthIncrease,
        duration: 600.0,
        ability_name: "Power Word: Fortitude".into(),
        ..Default::default()
    }
}

/// A Priest (Weakened Soul, so it shields no one first) with a teammate of
/// `class` at `ally_hp` under an enemy Warrior.
fn under_attack(class: arenasim::states::match_config::CharacterClass, ally_hp: f32) -> Scene {
    Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .unit(ALLY, 1, class)
        .at(10.0, 0.0)
        .health(ally_hp)
        .aura_full(ALLY, fortitude())
        .aura(ALLY, AuraType::WeakenedSoul, 10.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Warrior)
        .at(11.5, 0.0)
        .targeting(ALLY)
}

/// Pain Suppression goes on the teammate in danger; with the teammate
/// healthy, it is held.
#[test]
fn pain_suppression_goes_on_the_teammate_in_danger() {
    let danger = under_attack(Warrior, 0.3)
        .grant(PRIEST, SUPPRESSION)
        .decide(PRIEST);
    assert_eq!(
        danger.chose(),
        Some((SUPPRESSION, Some(ALLY))),
        "{}",
        describe(&danger)
    );
    let calm = under_attack(Warrior, 0.9)
        .grant(PRIEST, SUPPRESSION)
        .decide(PRIEST);
    assert!(precondition(&calm, SUPPRESSION), "{}", describe(&calm));
}

/// The Priest in danger itself takes its own Pain Suppression; a teammate
/// already holding it is not given a second.
#[test]
fn pain_suppression_goes_on_the_priest_and_never_twice() {
    let me = Scene::new()
        .unit(PRIEST, 1, Priest)
        .health(0.3)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 10.0)
        .unit(ENEMY, 2, Warrior)
        .at(1.5, 0.0)
        .targeting(PRIEST)
        .grant(PRIEST, SUPPRESSION)
        .decide(PRIEST);
    assert_eq!(
        me.chose(),
        Some((SUPPRESSION, Some(PRIEST))),
        "{}",
        describe(&me)
    );
    let held = under_attack(Warrior, 0.3)
        .aura_full(
            ALLY,
            Aura {
                effect_type: AuraType::DamageTakenReduction,
                duration: 5.0,
                magnitude: 0.40,
                ability_name: "Pain Suppression".into(),
                ..Default::default()
            },
        )
        .grant(PRIEST, SUPPRESSION)
        .decide(PRIEST);
    assert!(
        matches!(
            held.rejected(SUPPRESSION),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&held)
    );
}

/// A teammate in danger that is immune to damage (a Divine Shield) needs no
/// Pain Suppression: the same teammate without the immunity takes one.
#[test]
fn pain_suppression_skips_a_teammate_immune_to_damage() {
    let immune = under_attack(Warrior, 0.3)
        .aura(ALLY, AuraType::DamageImmunity, 8.0)
        .grant(PRIEST, SUPPRESSION)
        .decide(PRIEST);
    assert!(
        matches!(
            immune.rejected(SUPPRESSION),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&immune)
    );
    let exposed = under_attack(Warrior, 0.3)
        .grant(PRIEST, SUPPRESSION)
        .decide(PRIEST);
    assert_eq!(exposed.chose(), Some((SUPPRESSION, Some(ALLY))));
}

/// The Shadowform chain runs the save too: a Shadowform Priest Pain
/// Suppresses its teammate in danger, and holds it for a healthy one.
#[test]
fn a_shadowform_priest_pain_suppresses_its_teammate_in_danger() {
    let danger = under_attack(Warrior, 0.3)
        .in_shadowform(PRIEST)
        .grant(PRIEST, SUPPRESSION)
        .decide(PRIEST);
    assert_eq!(
        danger.chose(),
        Some((SUPPRESSION, Some(ALLY))),
        "{}",
        describe(&danger)
    );
    let calm = under_attack(Warrior, 0.9)
        .in_shadowform(PRIEST)
        .grant(PRIEST, SUPPRESSION)
        .decide(PRIEST);
    assert!(precondition(&calm, SUPPRESSION), "{}", describe(&calm));
}

/// A Priest with a Mage teammate whose target, a lone enemy Warrior, is at
/// `enemy_hp`.
fn with_mage(enemy_hp: f32) -> Scene {
    Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura_full(PRIEST, fortitude())
        .unit(ALLY, 1, Mage)
        .at(10.0, 0.0)
        .aura_full(ALLY, fortitude())
        .targeting(ENEMY)
        .unit(ENEMY, 2, Warrior)
        .at(30.0, 0.0)
        .health(enemy_hp)
}

/// Power Infusion goes on the team's spell-power damage dealer in a kill
/// window, and is held while there is none.
#[test]
fn power_infusion_goes_on_the_mage_in_a_kill_window() {
    let window = with_mage(0.2).grant(PRIEST, INFUSION).decide(PRIEST);
    assert!(
        window.situation.as_ref().unwrap().kill_window.is_some(),
        "{}",
        describe(&window)
    );
    assert_eq!(
        window.chose(),
        Some((INFUSION, Some(ALLY))),
        "{}",
        describe(&window)
    );
    let quiet = with_mage(0.9).grant(PRIEST, INFUSION).decide(PRIEST);
    assert!(quiet.situation.as_ref().unwrap().kill_window.is_none());
    assert!(precondition(&quiet, INFUSION), "{}", describe(&quiet));
}

/// A Mage already infused is not infused again.
#[test]
fn power_infusion_is_never_doubled() {
    let infused = with_mage(0.2)
        .aura_full(
            ALLY,
            Aura {
                effect_type: AuraType::SpellPowerIncrease,
                duration: 10.0,
                magnitude: 24.0,
                ability_name: "Power Infusion".into(),
                ..Default::default()
            },
        )
        .grant(PRIEST, INFUSION)
        .decide(PRIEST);
    assert!(
        matches!(
            infused.rejected(INFUSION),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&infused)
    );
}

/// On a team with no spell-power damage dealer, Power Infusion feeds the
/// Priest's own heals while a teammate is dying, and is held otherwise — a
/// kill window alone is no moment for it.
#[test]
fn power_infusion_feeds_the_priests_heals_beside_a_warrior() {
    let dying = under_attack(Warrior, 0.3)
        .grant(PRIEST, INFUSION)
        .decide(PRIEST);
    assert_eq!(
        dying.chose(),
        Some((INFUSION, Some(PRIEST))),
        "{}",
        describe(&dying)
    );
    let fine = under_attack(Warrior, 0.9)
        .health_of(ENEMY, 0.2)
        .grant(PRIEST, INFUSION)
        .decide(PRIEST);
    assert!(
        fine.situation.as_ref().unwrap().kill_window.is_some(),
        "{}",
        describe(&fine)
    );
    assert!(precondition(&fine, INFUSION), "{}", describe(&fine));
}

/// A Shadowform Priest is a spell-power damage dealer itself: in a kill
/// window it infuses itself.
#[test]
fn a_shadowform_priest_infuses_itself_in_a_kill_window() {
    let solo = |enemy_hp: f32| {
        Scene::new()
            .unit(PRIEST, 1, Priest)
            .aura_full(PRIEST, fortitude())
            .targeting(ENEMY)
            .in_shadowform(PRIEST)
            .unit(ENEMY, 2, Warrior)
            .at(25.0, 0.0)
            .health(enemy_hp)
            .grant(PRIEST, INFUSION)
    };
    let window = solo(0.2).decide(PRIEST);
    assert_eq!(
        window.chose(),
        Some((INFUSION, Some(PRIEST))),
        "{}",
        describe(&window)
    );
    let quiet = solo(0.9).decide(PRIEST);
    assert!(precondition(&quiet, INFUSION), "{}", describe(&quiet));
}

/// Inner Focus goes out just ahead of a Flash Heal on a dying teammate; for a
/// teammate hurt but not dying the Flash Heal goes out alone.
#[test]
fn inner_focus_goes_out_ahead_of_a_heal_on_a_dying_teammate() {
    let dying = under_attack(Warrior, 0.3)
        .grant(PRIEST, FOCUS)
        .decide(PRIEST);
    assert_eq!(
        dying.chose(),
        Some((FOCUS, Some(PRIEST))),
        "{}",
        describe(&dying)
    );
    let hurt = under_attack(Warrior, 0.7)
        .grant(PRIEST, FOCUS)
        .decide(PRIEST);
    assert!(precondition(&hurt, FOCUS), "{}", describe(&hurt));
    assert_eq!(hurt.chose(), Some((FlashHeal, Some(ALLY))));
}

/// The focus it leaves on the Priest makes the next Flash Heal free; it is
/// not cast again while it is up.
#[test]
fn inner_focus_makes_the_next_flash_heal_free() {
    let defs = AbilityDefinitions::shipped();
    let focus = defs.get_unchecked(&FOCUS);
    let aura = Aura {
        effect_type: AuraType::NextCast,
        duration: 600.0,
        ability_name: focus.name.clone(),
        next_cast: focus.applies_aura.as_ref().unwrap().next_cast.clone(),
        ..Default::default()
    };
    // Out of mana for a Flash Heal: only the focus lets it go out.
    let focused = |with: bool| {
        let scene = under_attack(Warrior, 0.3)
            .mana_of(PRIEST, 0.02)
            .grant(PRIEST, FOCUS)
            .cooldown(PRIEST, FOCUS, 30.0);
        if with {
            scene.aura_full(PRIEST, aura.clone())
        } else {
            scene
        }
        .decide(PRIEST)
    };
    let with = focused(true);
    assert_eq!(
        with.chose(),
        Some((FlashHeal, Some(ALLY))),
        "{}",
        describe(&with)
    );
    assert!(matches!(
        with.rejected(FOCUS),
        Some(RejectionReason::AlreadyApplied)
    ),);
    let without = focused(false);
    assert_ne!(without.chose(), Some((FlashHeal, Some(ALLY))));
}

// ---------------------------------------------------------------------------
// No global cooldown: Inner Focus and Power Infusion (TBC StartRecoveryTime 0)
// ---------------------------------------------------------------------------

/// The time of the first log line containing `needle`.
fn first_at(log: &str, needle: &str) -> Option<f32> {
    let line = log.lines().find(|l| l.contains(needle))?;
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
}

const T1B: Unit = Unit::new(1, 1);

/// A Priest with Inner Focus beside a Warrior at 25% under an enemy Warrior:
/// the focus goes out, and the Flash Heal it makes free follows within a few
/// frames, not a global cooldown later.
#[test]
fn the_flash_heal_follows_inner_focus_at_once() {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(2),
        max_duration_secs: 4.0,
        team1_builds: vec![Build::of(&[
            (UnbreakableWill, 2),
            (ImprovedPowerWordShield, 1),
            (InnerFocus, 1),
        ])],
        ..Default::default()
    };
    let weakened = |secs| Aura {
        effect_type: AuraType::WeakenedSoul,
        duration: secs,
        ability_name: "Weakened Soul".into(),
        ..Default::default()
    };
    let stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T1B, 10.0, 0.0)
        .place(T2, 11.5, 0.0)
        .health(T1B, 0.25)
        .target(T2, T1B)
        .aura(T1, weakened(15.0))
        .aura(T1B, weakened(15.0));
    let log = play(config, stage).log;
    let focus = first_at(&log, "casts Inner Focus").unwrap_or_else(|| panic!("{log}"));
    let heal =
        first_at(&log, "begins casting Flash Heal").unwrap_or_else(|| panic!("no heal:\n{log}"));
    assert!(
        heal >= focus && heal - focus < 0.2,
        "{focus} -> {heal}\n{log}"
    );
    assert!(log.contains("Inner Focus is spent on Flash Heal"), "{log}");
}

/// A Priest with Power Infusion beside a Mage whose enemy, a lone Warrior, is
/// at 15%: the infusion goes out, and the Priest's next cast follows within a
/// few frames, not a global cooldown later.
#[test]
fn power_infusion_leaves_the_priests_global_cooldown_free() {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Mage".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(2),
        max_duration_secs: 4.0,
        team1_builds: vec![Build::of(&[
            (UnbreakableWill, 2),
            (ImprovedPowerWordShield, 1),
            (InnerFocus, 1),
            (PowerInfusion, 1),
        ])],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T1B, 10.0, 0.0)
        .place(T2, 30.0, 0.0)
        .health(T2, 0.15)
        .target(T1, T2)
        .target(T1B, T2);
    let log = play(config, stage).log;
    let infusion =
        first_at(&log, "Priest #1 casts Power Infusion").unwrap_or_else(|| panic!("{log}"));
    let next = log
        .lines()
        .filter(|l| l.contains("[CAST] Team 1 Priest #1") && !l.contains("Power Infusion"))
        .filter_map(|l| first_at(l, "Priest #1"))
        .find(|t| *t >= infusion)
        .unwrap_or_else(|| panic!("the Priest cast nothing after:\n{log}"));
    assert!(next - infusion < 0.2, "{infusion} -> {next}\n{log}");
}

/// Pain Suppression takes a global cooldown (TBC 33206: StartRecoveryTime
/// 1500): the Flash Heal that follows it waits about a GCD. Its control is
/// [`power_infusion_leaves_the_priests_global_cooldown_free`], whose next cast
/// follows at once.
#[test]
fn pain_suppression_takes_a_global_cooldown() {
    let log = play(
        dying_warrior_config(suppression_build()),
        dying_warrior_stage(),
    )
    .log;
    let suppression =
        first_at(&log, "Priest #1 casts Pain Suppression").unwrap_or_else(|| panic!("{log}"));
    let next = next_priest_cast_after(&log, suppression, "Pain Suppression")
        .unwrap_or_else(|| panic!("the Priest cast nothing after:\n{log}"));
    assert!(
        next - suppression >= 1.4,
        "{suppression} -> {next}: the next cast did not wait a GCD\n{log}"
    );
}

/// A Discipline build down to Pain Suppression with neither Inner Focus nor
/// Power Infusion: 2 + 2 + 1 points below tier 3, then Reflective Shield and
/// Pain Suppression.
fn suppression_build() -> Build {
    Build::of(&[
        (UnbreakableWill, 2),
        (ImprovedPowerWordShield, 2),
        (MentalAgility, 1),
        (ReflectiveShield, 1),
        (PainSuppression, 1),
    ])
}

fn dying_warrior_config(build: Build) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(2),
        max_duration_secs: 4.0,
        team1_builds: vec![build],
        ..Default::default()
    }
}

/// The Priest's Warrior at 25% with an enemy Warrior on it; Weakened Soul on
/// both, so no shield goes out first.
fn dying_warrior_stage() -> Stage {
    let weakened = |secs| Aura {
        effect_type: AuraType::WeakenedSoul,
        duration: secs,
        ability_name: "Weakened Soul".into(),
        ..Default::default()
    };
    Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T1B, 10.0, 0.0)
        .place(T2, 11.5, 0.0)
        .health(T1B, 0.25)
        .target(T2, T1B)
        .aura(T1, weakened(15.0))
        .aura(T1B, weakened(15.0))
}

/// The time of the Priest's first `[CAST]` line at or after `after` that is
/// not `ability`'s own.
fn next_priest_cast_after(log: &str, after: f32, ability: &str) -> Option<f32> {
    log.lines()
        .filter(|l| l.contains("[CAST] Team 1 Priest #1") && !l.contains(ability))
        .filter_map(|l| first_at(l, "Priest #1"))
        .find(|t| *t >= after)
}

/// Inner Focus as a live aura, as its cast lands it.
fn inner_focus_aura() -> Aura {
    let defs = AbilityDefinitions::shipped();
    let focus = defs.get_unchecked(&AbilityType::InnerFocus);
    Aura {
        effect_type: AuraType::NextCast,
        duration: 600.0,
        ability_name: focus.name.clone(),
        next_cast: focus.applies_aura.as_ref().unwrap().next_cast.clone(),
        ..Default::default()
    }
}

/// The Priest's mana paid for the first cast of `ability`: its largest
/// one-frame drop within a few frames of the aura `effect` first showing on
/// `on` (the cast lands its aura a frame after it pays).
fn mana_paid(played: &common::staged::Played, on: (u8, u8), effect: AuraType) -> f32 {
    let frames = &played.frames;
    let observed = |i: usize, team: u8, slot: u8| {
        frames[i]
            .combatants
            .values()
            .find(|c| !c.is_pet && c.team == team && c.slot == slot)
            .cloned()
            .expect("the unit is observed")
    };
    let landed = (0..frames.len())
        .find(|&i| frames[i].gates_open && observed(i, on.0, on.1).aura_types.contains(&effect))
        .unwrap_or_else(|| panic!("no {effect:?} landed:\n{}", played.log));
    (landed.saturating_sub(4)..=landed)
        .filter(|&i| i > 0)
        .map(|i| observed(i - 1, 1, 0).current_mana - observed(i, 1, 0).current_mana)
        .fold(f32::MIN, f32::max)
}

/// Power Infusion and Pain Suppression charge their mana: 45 and 22. With
/// Inner Focus up (the control) the same cast is free, and spends the focus.
#[test]
fn power_infusion_and_pain_suppression_pay_their_mana() {
    let defs = AbilityDefinitions::shipped();
    let cost = |a: AbilityType| defs.get_unchecked(&a).mana_cost;
    assert_eq!(
        (
            cost(AbilityType::PowerInfusion),
            cost(AbilityType::PainSuppression)
        ),
        (45.0, 22.0)
    );

    let infusion = |focused: bool| {
        let config = HeadlessMatchConfig {
            team1: vec!["Priest".into(), "Mage".into()],
            team2: vec!["Warrior".into()],
            random_seed: Some(2),
            max_duration_secs: 3.0,
            team1_builds: vec![Build::of(&[
                (UnbreakableWill, 2),
                (ImprovedPowerWordShield, 1),
                (InnerFocus, 1),
                (PowerInfusion, 1),
            ])],
            ..Default::default()
        };
        let stage = Stage::new()
            .place(T1, 0.0, 0.0)
            .place(T1B, 10.0, 0.0)
            .place(T2, 30.0, 0.0)
            .health(T2, 0.15)
            .target(T1, T2)
            .target(T1B, T2)
            .cooldown(T1, AbilityType::InnerFocus, 40.0);
        let stage = if focused {
            stage.aura(T1, inner_focus_aura())
        } else {
            stage
        };
        play(config, stage)
    };
    let paid = infusion(false);
    let spent = mana_paid(&paid, (1, 1), AuraType::SpellPowerIncrease);
    assert!((spent - 45.0).abs() < 0.5, "paid {spent}\n{}", paid.log);
    let free = infusion(true);
    let spent = mana_paid(&free, (1, 1), AuraType::SpellPowerIncrease);
    assert!(spent < 0.5, "paid {spent} with the focus up\n{}", free.log);
    assert!(
        free.log.contains("Inner Focus is spent on Power Infusion"),
        "{}",
        free.log
    );

    let suppression = |focused: bool| {
        let stage = dying_warrior_stage();
        let stage = if focused {
            stage.aura(T1, inner_focus_aura())
        } else {
            stage
        };
        play(dying_warrior_config(suppression_build()), stage)
    };
    let paid = suppression(false);
    let spent = mana_paid(&paid, (1, 1), AuraType::DamageTakenReduction);
    assert!((spent - 22.0).abs() < 0.5, "paid {spent}\n{}", paid.log);
    let free = suppression(true);
    let spent = mana_paid(&free, (1, 1), AuraType::DamageTakenReduction);
    assert!(spent < 0.5, "paid {spent} with the focus up\n{}", free.log);
}

// ---------------------------------------------------------------------------
// Inner Focus is a Physical-school spell with a Magic buff (TBC 14751)
// ---------------------------------------------------------------------------

/// A school lockout on `school`, as an interrupt lands it.
fn lockout(school: arenasim::states::play_match::abilities::SpellSchool) -> Aura {
    Aura {
        effect_type: AuraType::SpellSchoolLockout,
        duration: 4.0,
        magnitude: school.to_lockout_magnitude(),
        ability_name: "Kick".into(),
        ..Default::default()
    }
}

/// Inner Focus is Physical, its buff Magic: a Holy lockout (a Kick on a
/// Flash Heal) does not lock it out, and a purge takes the buff.
#[test]
fn inner_focus_is_physical_and_its_focus_is_magic() {
    use arenasim::states::play_match::abilities::SpellSchool;
    use arenasim::states::play_match::class_ai::cast_guard::{own_state_ok, PreCastOpts};
    use arenasim::states::play_match::components::{ActiveAuras, AuraPending};
    let defs = AbilityDefinitions::shipped();
    let focus = defs.get_unchecked(&AbilityType::InnerFocus);
    assert_eq!(focus.spell_school, SpellSchool::Physical);
    let landed = AuraPending::from_ability(
        bevy::prelude::Entity::PLACEHOLDER,
        bevy::prelude::Entity::PLACEHOLDER,
        focus,
    )
    .unwrap()
    .aura;
    assert_eq!(landed.removal_class_name(), Some("Magic"));
    assert!(landed.can_be_purged());

    let priest = Combatant::new(1, 0, Priest);
    let holy = ActiveAuras {
        auras: vec![lockout(SpellSchool::Holy)],
    };
    let opts = PreCastOpts::default();
    assert!(own_state_ok(
        AbilityType::InnerFocus,
        focus,
        &priest,
        Some(&holy),
        opts
    ));
    // The control: the Holy lockout does lock Flash Heal out.
    let heal = defs.get_unchecked(&FlashHeal);
    assert!(!own_state_ok(FlashHeal, heal, &priest, Some(&holy), opts));
}

/// Under a Holy lockout the Flash Heal Inner Focus would feed cannot be
/// cast, so the focus is held for it (not refused for the lockout, and not
/// spent on a Mind Blast); under a Shadow lockout the heal can go out, and
/// the focus does.
#[test]
fn inner_focus_waits_for_a_flash_heal_it_can_feed() {
    use arenasim::states::play_match::abilities::SpellSchool;
    let locked = |school| {
        under_attack(Warrior, 0.3)
            .aura_full(PRIEST, lockout(school))
            .grant(PRIEST, FOCUS)
            .decide(PRIEST)
    };
    let holy = locked(SpellSchool::Holy);
    assert!(
        matches!(
            holy.rejected(FOCUS),
            Some(RejectionReason::PreconditionUnmet { note }) if note.contains("cannot be cast")
        ),
        "{}",
        describe(&holy)
    );
    assert_ne!(holy.chose().map(|c| c.0), Some(FOCUS));
    let shadow = locked(SpellSchool::Shadow);
    assert_eq!(
        shadow.chose(),
        Some((FOCUS, Some(PRIEST))),
        "{}",
        describe(&shadow)
    );
}
