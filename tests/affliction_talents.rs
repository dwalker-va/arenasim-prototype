//! AS-409 — the Warlock's Affliction tree: the attrition tree.
//!
//! - **the tree**, as the card writes it: every node, its place, its ranks,
//!   its effect, its declared price and its prerequisite;
//! - **the spawn**: an Affliction build reaches the spawned Warlock — Drain
//!   Life's bigger ticks, Life Tap's extra mana, Shadow Mastery on every
//!   Shadow number and on no other, the two granted abilities and Nightfall's
//!   proc — and Curse of Exhaustion is offered only to a build that takes it;
//! - **the AI rules**, in scene pairs, each against its control: Siphon Life
//!   on the kill target, Amplify Curse ahead of a curse it amplifies,
//!   Shadow Bolt next while Shadow Trance is up, and a curse held off a
//!   target immune to it;
//! - **in a match**: an amplified Curse of Agony lands 50% harder on Amplify
//!   Curse's cooldown, Siphon Life's ticks heal the Warlock, Curse of
//!   Exhaustion slows its target one curse at a time, and Improved Life Tap's
//!   tap gives more mana for the same health.

mod common;

use std::collections::BTreeSet;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::build_headless_app;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{self, Warlock, Warrior};
use arenasim::states::match_config::WarlockCurse;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::components::{Aura, AuraType, Combatant, DispelType, Pet};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::next_cast::{NextCastModifier, NextCastScope};
use arenasim::states::play_match::talent_choices::ChoiceOption;
use arenasim::states::play_match::talent_procs::{
    ProcChance, ProcEffect, ProcRankScale, TalentProc, TalentTrigger,
};
use arenasim::states::play_match::talents::{
    AbilityField, Build, PatchOp, PatchTarget, TalentDefinitions, TalentEffect, TalentId, TreeId,
};
use arenasim::states::play_match::AbilityType;
use common::staged::play;
use TalentId::*;

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

fn trance() -> NextCastModifier {
    NextCastModifier {
        applies_to: NextCastScope::Abilities(vec![AbilityType::Shadowbolt]),
        instant: true,
        free: false,
        crit_chance: 0.0,
        cast_time_reduction: 0.0,
        cost_reduction: 0.0,
        power: 0.0,
    }
}

/// Every Affliction node as the card writes it.
#[test]
fn the_affliction_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let affliction: BTreeSet<TalentId> = talents
        .talents_for_class(Warlock)
        .filter(|(_, t)| t.tree == TreeId::Affliction)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        affliction,
        [
            ImprovedDrainLife,
            ImprovedLifeTap,
            AmplifyCurse,
            Nightfall,
            SiphonLife,
            CurseOfExhaustion,
            ShadowMastery
        ]
        .into_iter()
        .collect()
    );
    // Affliction leads the Warlock's trees ("6/0/2" reads in this order).
    assert_eq!(
        talents
            .trees(Warlock)
            .first()
            .map(|t| (t.id, t.name.as_str())),
        Some((TreeId::Affliction, "Affliction"))
    );

    let patch = |on, field, op| TalentEffect::Patch { on, field, op };
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
            ImprovedDrainLife,
            1,
            0,
            2,
            vec![patch(
                PatchTarget::Ability(AbilityType::DrainLife),
                AbilityField::Damage,
                PatchOp::Mul(1.15),
            )],
            11.0,
            None,
        ),
        (
            ImprovedLifeTap,
            1,
            2,
            2,
            vec![patch(
                PatchTarget::Ability(AbilityType::LifeTap),
                AbilityField::ManaPerHealth,
                PatchOp::Mul(1.10),
            )],
            11.0,
            None,
        ),
        (
            AmplifyCurse,
            2,
            0,
            1,
            vec![TalentEffect::Grant(AbilityType::AmplifyCurse)],
            11.0,
            None,
        ),
        (
            Nightfall,
            2,
            2,
            2,
            vec![TalentEffect::Proc(TalentProc {
                trigger: TalentTrigger::DotTick,
                abilities: vec![AbilityType::Corruption, AbilityType::DrainLife],
                crit_only: false,
                chance: ProcChance::Chance(0.05),
                ranks_add: ProcRankScale::Chance,
                effect: ProcEffect::NextCast {
                    modifier: trance(),
                    duration: 10.0,
                    dispel_type: DispelType::Auto,
                    name: Some("Shadow Trance".into()),
                },
                internal_cooldown: 0.0,
            })],
            11.0,
            None,
        ),
        (
            SiphonLife,
            3,
            0,
            1,
            vec![TalentEffect::Grant(AbilityType::SiphonLife)],
            11.0,
            None,
        ),
        (
            CurseOfExhaustion,
            3,
            1,
            1,
            vec![TalentEffect::WidenChoice(ChoiceOption::WarlockCurse(
                WarlockCurse::Exhaustion,
            ))],
            11.0,
            None,
        ),
        (
            ShadowMastery,
            4,
            1,
            1,
            vec![
                patch(
                    PatchTarget::School(SpellSchool::Shadow),
                    AbilityField::Damage,
                    PatchOp::Mul(1.10),
                ),
                patch(
                    PatchTarget::School(SpellSchool::Shadow),
                    AbilityField::PeriodicDamage,
                    PatchOp::Mul(1.10),
                ),
            ],
            22.0,
            Some((SiphonLife, 1)),
        ),
    ];
    for (id, tier, column, ranks, per_rank, budget, requires) in expected {
        let t = talents.get(id).unwrap();
        assert_eq!(
            (t.tree, t.tier, t.column, t.max_rank),
            (TreeId::Affliction, tier, column, ranks),
            "{id:?}"
        );
        assert_eq!(t.per_rank, per_rank, "{id:?}");
        assert_eq!(t.budget, Some(budget), "{id:?}");
        assert_eq!(t.requires, requires, "{id:?}");
    }
}

/// The two granted abilities and the widened curse, as the card writes them.
#[test]
fn the_affliction_abilities_are_as_specified() {
    let defs = abilities();
    let siphon = defs.get_unchecked(&AbilityType::SiphonLife);
    let aura = siphon.applies_aura.as_ref().unwrap();
    assert!(siphon.talent);
    assert_eq!(
        (
            siphon.mana_cost,
            siphon.range,
            siphon.cast_time,
            siphon.spell_school
        ),
        (25.0, 30.0, 0.0, SpellSchool::Shadow)
    );
    assert_eq!(
        (
            aura.aura_type,
            aura.magnitude,
            aura.tick_interval,
            aura.duration
        ),
        (AuraType::DamageOverTime, 4.0, 3.0, 30.0)
    );
    assert_eq!(aura.leech.map(|l| l.fraction), Some(1.0));

    let amplify = defs.get_unchecked(&AbilityType::AmplifyCurse);
    assert!(amplify.talent);
    assert_eq!(
        (amplify.cooldown, amplify.mana_cost, amplify.cast_time),
        (45.0, 0.0, 0.0)
    );
    let primed = amplify
        .applies_aura
        .as_ref()
        .and_then(|a| a.next_cast.clone())
        .unwrap();
    assert_eq!(primed.power, 0.5);
    let covered: Vec<AbilityType> = [
        AbilityType::CurseOfAgony,
        AbilityType::CurseOfWeakness,
        AbilityType::CurseOfTongues,
        AbilityType::CurseOfExhaustion,
        AbilityType::Corruption,
    ]
    .into_iter()
    .filter(|a| primed.covers(*a))
    .collect();
    assert_eq!(
        covered,
        vec![AbilityType::CurseOfAgony, AbilityType::CurseOfWeakness]
    );

    let exhaustion = defs.get_unchecked(&AbilityType::CurseOfExhaustion);
    assert!(!exhaustion.talent, "a menu option, in the base book");
    let slow = exhaustion.applies_aura.as_ref().unwrap();
    assert_eq!(
        (slow.aura_type, slow.duration, slow.dispel_type),
        (AuraType::MovementSpeedSlow, 12.0, DispelType::Curse)
    );
    assert!((1.0 - slow.magnitude - 0.30).abs() < 1e-6, "a 30% slow");
    assert_eq!(
        WarlockCurse::Exhaustion.ability(),
        AbilityType::CurseOfExhaustion
    );
}

fn abilities() -> &'static arenasim::states::play_match::ability_config::AbilityDefinitions {
    arenasim::states::play_match::ability_config::AbilityDefinitions::shipped()
}

// ---------------------------------------------------------------------------
// The spawn
// ---------------------------------------------------------------------------

/// The Warlock a headless match spawns (team 1, against a Warrior) with
/// `build`.
fn spawned_warlock(build: &Build) -> Combatant {
    let config = HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(7),
        team1_builds: vec![build.clone()],
        ..Default::default()
    };
    let mut app = build_headless_app(&config, true, None);
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<(&Combatant, Option<&Pet>)>();
    let warlock = q
        .iter(world)
        .find(|(c, pet)| pet.is_none() && c.team == 1 && c.class == Warlock)
        .map(|(c, _)| c.clone());
    warlock.expect("a Warlock spawned")
}

fn near(is: f32, want: f32) -> bool {
    (is - want).abs() <= want.abs() * 1e-5
}

/// The damage numbers of `ability` in `c`'s book: its direct damage range
/// and coefficient, and its aura's tick.
fn numbers(c: &Combatant, ability: AbilityType) -> (f32, f32, f32, Option<f32>) {
    let d = c.book().get_unchecked(&ability);
    (
        d.damage_base_min,
        d.damage_base_max,
        d.damage_coefficient,
        d.applies_aura.as_ref().map(|a| a.magnitude),
    )
}

/// An empty build changes nothing the tree touches; a deep Affliction build
/// reaches every number it names and no other.
#[test]
fn an_affliction_build_reaches_the_spawned_warlock() {
    let base = spawned_warlock(&Build::default());
    for granted in [AbilityType::SiphonLife, AbilityType::AmplifyCurse] {
        assert!(base.book().get(&granted).is_none(), "{granted:?}");
    }
    assert!(base.talent_procs.slots.is_empty());

    let deep = spawned_warlock(&Build::of(&[
        (ImprovedDrainLife, 2),
        (AmplifyCurse, 1),
        (Nightfall, 2),
        (SiphonLife, 1),
        (CurseOfExhaustion, 1),
        (ShadowMastery, 1),
    ]));
    for granted in [AbilityType::SiphonLife, AbilityType::AmplifyCurse] {
        assert!(deep.book().get(&granted).is_some(), "{granted:?}");
    }

    // Drain Life: two ranks of +15% and Shadow Mastery's +10% ADD: 10 -> 14.
    let (min, max, _, _) = numbers(&deep, AbilityType::DrainLife);
    assert!(near(min, 14.0) && near(max, 14.0), "{min} {max}");

    // Shadow Mastery: every Shadow spell's direct damage and every Shadow
    // damage over time's tick, +10%.
    let (bmin, bmax, bcoef, _) = numbers(&base, AbilityType::Shadowbolt);
    let (dmin, dmax, dcoef, _) = numbers(&deep, AbilityType::Shadowbolt);
    assert!(near(dmin, bmin * 1.1) && near(dmax, bmax * 1.1) && near(dcoef, bcoef * 1.1));
    let (cmin, _, _, _) = numbers(&base, AbilityType::DeathCoil);
    assert!(near(numbers(&deep, AbilityType::DeathCoil).0, cmin * 1.1));
    for dot in [
        AbilityType::Corruption,
        AbilityType::CurseOfAgony,
        AbilityType::UnstableAffliction,
    ] {
        let was = numbers(&base, dot).3.unwrap();
        let is = numbers(&deep, dot).3.unwrap();
        assert!(near(is, was * 1.1), "{dot:?}: {was} -> {is}");
    }
    assert!(near(
        numbers(&deep, AbilityType::SiphonLife).3.unwrap(),
        4.0 * 1.1
    ));
    // ...and nothing that is not Shadow damage: a curse's cut, a curse's
    // slowing, the Fire school.
    for untouched in [
        AbilityType::CurseOfWeakness,
        AbilityType::CurseOfTongues,
        AbilityType::CurseOfExhaustion,
        AbilityType::Immolate,
        AbilityType::Fear,
    ] {
        assert_eq!(
            numbers(&deep, untouched),
            numbers(&base, untouched),
            "{untouched:?}"
        );
    }

    // Nightfall at two ranks: 10% a tick, of Corruption and Drain Life.
    let procs: Vec<_> = deep
        .talent_procs
        .slots
        .iter()
        .map(|s| s.proc.clone())
        .collect();
    assert_eq!(procs.len(), 1);
    assert_eq!(procs[0].chance, ProcChance::Chance(0.10));

    // Improved Life Tap: the same health, 20% more mana at two ranks.
    let tap = |c: &Combatant| {
        c.book()
            .get_unchecked(&AbilityType::LifeTap)
            .health_to_mana
            .unwrap()
    };
    let tapper = spawned_warlock(&Build::of(&[(ImprovedLifeTap, 2)]));
    assert_eq!(tap(&base).mana_per_health, 1.0);
    assert!(near(tap(&tapper).mana_per_health, 1.2));
    assert_eq!(tap(&tapper).amount(100.0), tap(&base).amount(100.0));
    assert!(near(
        tap(&tapper).mana_for(50.0),
        tap(&base).mana_for(50.0) * 1.2
    ));
}

/// Curse of Exhaustion is a curse choice only for a build that takes its
/// node; the other three are every build's.
#[test]
fn curse_of_exhaustion_is_offered_only_with_its_talent() {
    let talents = TalentDefinitions::shipped();
    let offered = |build: &Build| -> Vec<WarlockCurse> {
        WarlockCurse::ALL
            .into_iter()
            .filter(|c| talents.offers(build, ChoiceOption::WarlockCurse(*c)))
            .collect()
    };
    let base = vec![
        WarlockCurse::Agony,
        WarlockCurse::Weakness,
        WarlockCurse::Tongues,
    ];
    assert_eq!(offered(&Build::default()), base);
    let talented = Build::of(&[
        (ImprovedDrainLife, 2),
        (AmplifyCurse, 1),
        (CurseOfExhaustion, 1),
    ]);
    let mut all = base;
    all.push(WarlockCurse::Exhaustion);
    assert_eq!(offered(&talented), all);
}

// ---------------------------------------------------------------------------
// The AI rules, in scene pairs
// ---------------------------------------------------------------------------

const ME: Who = "warlock";
const FOE: Who = "enemy warrior";

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}",
        view.choices,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// A DoT of the Warlock's on its target.
fn dot(name: &str, caster: bevy::prelude::Entity) -> Aura {
    Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 15.0,
        magnitude: 10.0,
        tick_interval: 3.0,
        time_until_next_tick: 3.0,
        ability_name: name.into(),
        caster: Some(caster),
        spell_school: Some(SpellSchool::Shadow),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// A Warlock 20 yards from its kill target, a Warrior (no healer to Fear,
/// no dispeller, out of Death Coil's peel), with `dots` of its own already
/// on the Warrior and its curse preference `curse`.
fn warlock_scene(dots: &[&str], curse: WarlockCurse) -> Scene {
    let mut scene = Scene::new()
        .unit(ME, 1, Warlock)
        .at(0.0, 0.0)
        .targeting(FOE)
        .unit(FOE, 2, Warrior)
        .at(20.0, 0.0);
    let me = scene.entity(ME);
    for name in dots {
        scene = scene.aura_full(FOE, dot(name, me));
    }
    scene.combatant_mut(ME).warlock_curse_prefs = vec![curse];
    scene
}

fn chose(view: &DecisionView, ability: AbilityType) -> bool {
    view.choices.iter().any(|(a, _)| *a == ability)
}

/// Siphon Life goes on the kill target once Corruption and Unstable
/// Affliction are up; its control, the same target already carrying the
/// Warlock's Siphon Life, takes the rotation's next step instead.
#[test]
fn siphon_life_goes_on_the_kill_target_unless_it_is_already_there() {
    let scene = warlock_scene(&["Corruption", "Unstable Affliction"], WarlockCurse::Agony)
        .grant(ME, AbilityType::SiphonLife);
    let view = scene.decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::SiphonLife, Some(FOE))),
        "{}",
        describe(&view)
    );
    assert!(view.auras_queued.contains(&(FOE, AuraType::DamageOverTime)));

    let control = warlock_scene(
        &["Corruption", "Unstable Affliction", "Siphon Life"],
        WarlockCurse::Agony,
    )
    .grant(ME, AbilityType::SiphonLife);
    let view = control.decide(ME);
    assert!(
        !chose(&view, AbilityType::SiphonLife),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::SiphonLife),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&view)
    );
}

/// Another Warlock's Siphon Life on the target is not this one's: it still
/// casts its own.
#[test]
fn another_warlocks_siphon_life_does_not_hold_this_ones() {
    let mut scene = warlock_scene(&["Corruption", "Unstable Affliction"], WarlockCurse::Agony)
        .unit("other warlock", 1, Warlock)
        .at(0.0, 5.0)
        .grant(ME, AbilityType::SiphonLife);
    let other = scene.entity("other warlock");
    scene = scene.aura_full(FOE, dot("Siphon Life", other));
    let view = scene.decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::SiphonLife, Some(FOE))),
        "{}",
        describe(&view)
    );
}

/// The DoTs are up and the curse is due: Amplify Curse goes first, off the
/// global cooldown, ahead of Curse of Agony. Controls: a curse it does not
/// amplify (Tongues) goes out plain; an Amplify Curse already primed is
/// spent by the curse; and on cooldown, the curse goes out plain.
#[test]
fn amplify_curse_goes_ahead_of_a_curse_it_amplifies() {
    let dots = ["Corruption", "Unstable Affliction"];
    let view = warlock_scene(&dots, WarlockCurse::Agony)
        .grant(ME, AbilityType::AmplifyCurse)
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::AmplifyCurse, None)),
        "{}",
        describe(&view)
    );
    assert!(view.auras_queued.contains(&(ME, AuraType::NextCast)));
    assert!(
        !chose(&view, AbilityType::CurseOfAgony),
        "{}",
        describe(&view)
    );

    // A curse it does not amplify.
    let tongues = warlock_scene(&dots, WarlockCurse::Tongues)
        .grant(ME, AbilityType::AmplifyCurse)
        .decide(ME);
    assert_eq!(
        tongues.chose(),
        Some((AbilityType::CurseOfTongues, Some(FOE))),
        "{}",
        describe(&tongues)
    );
    assert!(!chose(&tongues, AbilityType::AmplifyCurse));

    // On cooldown: the curse goes out plain.
    let cooling = warlock_scene(&dots, WarlockCurse::Agony)
        .grant(ME, AbilityType::AmplifyCurse)
        .cooldown(ME, AbilityType::AmplifyCurse, 20.0)
        .decide(ME);
    assert_eq!(
        cooling.chose(),
        Some((AbilityType::CurseOfAgony, Some(FOE))),
        "{}",
        describe(&cooling)
    );

    // A curse that cannot go out now (its target beyond its 30 yards) is not
    // amplified: the Amplify Curse would wait out its 30s for nothing.
    let far = warlock_scene(&dots, WarlockCurse::Agony)
        .grant(ME, AbilityType::AmplifyCurse)
        .place(FOE, 34.0, 0.0)
        .decide(ME);
    assert!(
        !chose(&far, AbilityType::AmplifyCurse),
        "{}",
        describe(&far)
    );
    assert!(
        matches!(
            far.rejected(AbilityType::CurseOfAgony),
            Some(RejectionReason::OutOfRange { .. })
        ),
        "{}",
        describe(&far)
    );
}

/// The curse a primed Amplify Curse covers is cast next, and spends it: the
/// curse's aura is 50% stronger than its control's, the same scene unprimed.
#[test]
fn a_primed_amplify_curse_is_spent_by_the_curse() {
    let dots = ["Corruption", "Unstable Affliction"];
    let amplify = abilities().get_unchecked(&AbilityType::AmplifyCurse);
    let primed = Aura {
        effect_type: AuraType::NextCast,
        duration: 30.0,
        magnitude: 1.0,
        ability_name: amplify.name.clone(),
        next_cast: amplify.applies_aura.as_ref().unwrap().next_cast.clone(),
        ..Default::default()
    };
    let view = warlock_scene(&dots, WarlockCurse::Agony)
        .grant(ME, AbilityType::AmplifyCurse)
        .aura_full(ME, primed)
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::CurseOfAgony, Some(FOE))),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::AmplifyCurse),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&view)
    );
}

/// The Shadow Trance aura a Nightfall proc lands.
fn shadow_trance() -> Aura {
    Aura {
        effect_type: AuraType::NextCast,
        duration: 10.0,
        magnitude: 1.0,
        ability_name: "Shadow Trance".into(),
        next_cast: Some(trance()),
        ..Default::default()
    }
}

/// Shadow Trance up: Shadow Bolt is next, ahead of the DoTs the rotation
/// would otherwise put up first. Its control, the same scene without the
/// trance, opens with a DoT.
#[test]
fn shadow_trance_puts_shadow_bolt_next() {
    let view = warlock_scene(&[], WarlockCurse::Agony)
        .aura_full(ME, shadow_trance())
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Shadowbolt, Some(FOE))),
        "{}",
        describe(&view)
    );
    let control = warlock_scene(&[], WarlockCurse::Agony).decide(ME);
    assert_ne!(
        control.chose().map(|c| c.0),
        Some(AbilityType::Shadowbolt),
        "{}",
        describe(&control)
    );
    assert!(
        chose(&control, AbilityType::Corruption)
            || chose(&control, AbilityType::UnstableAffliction)
    );
}

/// Slowed and out of range — being kited — the Warlock holds Shadow Bolt,
/// whose cast would be cut short; with Shadow Trance it is instant, so it
/// goes.
#[test]
fn an_instant_shadow_bolt_goes_while_being_kited() {
    let kited = |trance: bool| {
        let mut scene = warlock_scene(
            &["Corruption", "Unstable Affliction", "Curse of Agony"],
            WarlockCurse::Agony,
        )
        .place(FOE, 29.0, 0.0)
        .aura(ME, AuraType::MovementSpeedSlow, 5.0);
        if trance {
            scene = scene.aura_full(ME, shadow_trance());
        }
        // Drain Life healing-gated, Fear refused: nothing else to do.
        scene.combatant_mut(ME).current_health = scene.combatant_mut(ME).max_health;
        scene.decide(ME)
    };
    let view = kited(true);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Shadowbolt, Some(FOE))),
        "{}",
        describe(&view)
    );
    let control = kited(false);
    assert!(
        !chose(&control, AbilityType::Shadowbolt),
        "{}",
        describe(&control)
    );
}

// ---------------------------------------------------------------------------
// In a match
// ---------------------------------------------------------------------------

const T1: Unit = Unit::new(1, 0);
const T2: Unit = Unit::new(2, 0);

/// A Warlock with `build` and curse preference `curse`, against a Warrior
/// held stunned the whole match (so it never closes, kicks or kills) and
/// given the health to outlast it, for 70 seconds: long enough for a curse
/// to run out and be cast again.
fn held_match(build: Build, curse: &str) -> common::staged::Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(3),
        max_duration_secs: 70.0,
        team1_builds: vec![build],
        team1_warlock_curse_prefs: vec![Some(vec![curse.into()])],
        ..Default::default()
    };
    let held = Aura {
        effect_type: AuraType::Stun,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Held".into(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    play(
        config,
        Stage::new()
            .health(T1, 0.6)
            // Mana to spare, so the rotation reaches its curse again when
            // the first runs out.
            .combatant(T1, |c| {
                c.max_mana = 5_000.0;
                c.current_mana = 5_000.0;
            })
            .aura(T2, held)
            .combatant(T2, |c| {
                c.max_health = 20_000.0;
                c.current_health = 20_000.0;
            }),
    )
}

fn lines<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

/// A log line's time: "[ 46.52s] ..." is 46.52.
fn at(line: &str) -> f32 {
    line.trim_start_matches('[')
        .split('s')
        .next()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or_else(|| panic!("no time on {line:?}"))
}

/// An Affliction Warlock amplifies its Curse of Agony, and the curse it lands
/// ticks 50% harder than its control's (the same match without the talent);
/// its Siphon Life's ticks heal it, where the control has no Siphon Life.
#[test]
fn an_amplified_curse_and_a_siphon_life_land_in_a_match() {
    let build = Build::of(&[(ImprovedDrainLife, 2), (AmplifyCurse, 1), (SiphonLife, 1)]);
    let played = held_match(build, "Agony");
    assert!(
        !lines(&played.log, "casts Amplify Curse").is_empty(),
        "{}",
        played.log
    );
    assert!(
        !lines(&played.log, "Amplify Curse is spent on Curse of Agony").is_empty(),
        "{}",
        played.log
    );
    assert!(
        !lines(
            &played.log,
            "applies Curse of Agony to enemy (21 damage per 4s"
        )
        .is_empty(),
        "{}",
        played.log
    );
    assert!(
        !lines(&played.log, "Siphon Life heals for").is_empty(),
        "{}",
        played.log
    );
    // Off the global cooldown: each amplified curse follows its Amplify Curse
    // on the next decision, a frame or two on, not a global cooldown later.
    // On its 45s cooldown it amplifies every other curse here: the refresh 24s
    // after the first goes out plain, the next one amplified again.
    let amplified: Vec<f32> = lines(&played.log, "casts Amplify Curse")
        .into_iter()
        .map(at)
        .collect();
    let strong: Vec<f32> = lines(&played.log, "applies Curse of Agony to enemy (21 damage")
        .into_iter()
        .map(at)
        .collect();
    let plain: Vec<f32> = lines(&played.log, "applies Curse of Agony to enemy (14 damage")
        .into_iter()
        .map(at)
        .collect();
    assert!(amplified.len() >= 2, "{}", played.log);
    assert_eq!(strong.len(), amplified.len(), "{}", played.log);
    for (cast, landed) in amplified.iter().zip(&strong) {
        assert!((0.0..0.1).contains(&(landed - cast)), "{cast} -> {landed}");
    }
    assert!(
        amplified.windows(2).all(|w| w[1] - w[0] >= 45.0),
        "{amplified:?}"
    );
    assert!(
        plain.iter().any(|t| *t > amplified[0] && *t < amplified[1]),
        "a plain refresh between: {plain:?} {amplified:?}"
    );

    let control = held_match(Build::of(&[(ImprovedDrainLife, 2)]), "Agony");
    assert!(lines(&control.log, "Amplify Curse").is_empty());
    assert!(!lines(
        &control.log,
        "applies Curse of Agony to enemy (14 damage per 4s"
    )
    .is_empty());
    assert!(lines(&control.log, "Siphon Life").is_empty());
}

/// A Warlock whose build takes Curse of Exhaustion, and that chooses it,
/// curses with it: its target is slowed 30%.
#[test]
fn curse_of_exhaustion_slows_its_target_in_a_match() {
    let build = Build::of(&[
        (ImprovedDrainLife, 2),
        (AmplifyCurse, 1),
        (CurseOfExhaustion, 1),
    ]);
    let played = held_match(build, "Exhaustion");
    assert!(
        !lines(
            &played.log,
            "applies Curse of Exhaustion to enemy (-30% movement speed for 12s)"
        )
        .is_empty(),
        "{}",
        played.log
    );
    // It slows on the Slows DR bucket (12s, 6s, 3s, then immune), and is
    // never cast into that immunity: it would land nothing.
    let applied = lines(&played.log, "applies Curse of Exhaustion");
    assert!(
        lines(&played.log, "Curse of Exhaustion IMMUNE").is_empty(),
        "{}",
        played.log
    );
    assert!(applied.len() >= 2, "{applied:?}");
    // One curse of the Warlock's on a target at a time: each lands only once
    // the last has run its (diminished) course.
    let landed: Vec<(f32, f32)> = lines(&played.log, "[CC] Curse of Exhaustion on")
        .into_iter()
        .map(|l| {
            let secs = l
                .split('(')
                .nth(1)
                .and_then(|r| r.split('s').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("no duration on {l:?}"));
            (at(l), secs)
        })
        .collect();
    assert_eq!(landed.len(), applied.len(), "{}", played.log);
    assert!(
        landed.windows(2).all(|w| w[1].0 >= w[0].0 + w[0].1 - 0.1),
        "{landed:?}"
    );
    // Amplify Curse does not amplify it, so it is not cast for it.
    assert!(
        lines(&played.log, "casts Amplify Curse").is_empty(),
        "{}",
        played.log
    );
}

/// Each Life Tap of a log: the health it took and the mana it gave, as the
/// line states them, and the mana and health the Warlock held after it.
#[derive(Clone, Copy, Debug)]
struct Tap {
    health: f32,
    mana: f32,
    health_after: f32,
    mana_after: f32,
}

/// "[LIFE TAP] ... converts H health into M mana (h/H' health, m/M' mana)".
fn taps(log: &str) -> Vec<Tap> {
    lines(log, "[LIFE TAP]")
        .into_iter()
        .map(|l| {
            let words: Vec<&str> = l.split_whitespace().collect();
            let i = words
                .iter()
                .position(|w| *w == "converts")
                .expect("a conversion");
            let held = |w: &str| -> f32 {
                w.trim_start_matches('(')
                    .split('/')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| panic!("no amount in {l:?}"))
            };
            Tap {
                health: words[i + 1].parse().unwrap(),
                mana: words[i + 4].parse().unwrap(),
                health_after: held(words[i + 6]),
                mana_after: held(words[i + 8]),
            }
        })
        .collect()
}

/// A starved, healthy Warlock taps: with Improved Life Tap 2/2 its tap gives
/// 20% more mana than the health it takes; its control, untalented, one for
/// one. Both tap at the same moment from the same staged pool, so the mana
/// they hold after it differs by exactly the talent's extra.
#[test]
fn improved_life_tap_gives_more_mana_for_the_same_health() {
    let tap = |build: Build| {
        let config = HeadlessMatchConfig {
            team1: vec!["Warlock".into()],
            team2: vec!["Warrior".into()],
            random_seed: Some(3),
            max_duration_secs: 15.0,
            team1_builds: vec![build],
            ..Default::default()
        };
        let held = Aura {
            effect_type: AuraType::Stun,
            duration: 300.0,
            magnitude: 1.0,
            ability_name: "Held".into(),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        };
        let played = play(
            config,
            Stage::new().health(T1, 1.0).mana(T1, 0.1).aura(T2, held),
        );
        *taps(&played.log)
            .first()
            .unwrap_or_else(|| panic!("the starved Warlock taps: {}", played.log))
    };
    let talented = tap(Build::of(&[(ImprovedLifeTap, 2)]));
    let control = tap(Build::default());
    assert!(
        (talented.mana - talented.health * 1.2).abs() <= 1.0,
        "{talented:?}"
    );
    assert_eq!(control.health, control.mana, "{control:?}");
    assert_eq!(
        talented.health, control.health,
        "the same health either way"
    );
    assert_eq!(talented.health_after, control.health_after);
    let extra = talented.mana_after - control.mana_after;
    assert!(
        (extra - talented.health * 0.2).abs() <= 1.0,
        "{extra} more mana: {talented:?} {control:?}"
    );
}

/// A config that chooses Curse of Exhaustion without its talent is refused,
/// naming the slot.
#[test]
fn curse_of_exhaustion_without_its_talent_is_refused() {
    let config = HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        team1_warlock_curse_prefs: vec![Some(vec!["Exhaustion".into()])],
        ..Default::default()
    };
    assert!(config.to_match_config().is_err());
    let talented = HeadlessMatchConfig {
        team1_builds: vec![Build::of(&[
            (ImprovedDrainLife, 2),
            (AmplifyCurse, 1),
            (CurseOfExhaustion, 1),
        ])],
        ..config
    };
    assert!(talented.to_match_config().is_ok());
}

/// A curse whose effect diminishes is held off a target immune to it:
/// Curse of Exhaustion's slow on a target DR-immune to slows lands nothing,
/// so the Warlock does not cast it. Its control, the same target with no
/// slows on its DR, is cursed.
#[test]
fn curse_of_exhaustion_is_held_off_a_target_immune_to_slows() {
    use arenasim::states::play_match::components::{DRCategory, DRTracker};
    let dots = ["Corruption", "Unstable Affliction"];
    let mut immune = DRTracker::default();
    for _ in 0..3 {
        immune.apply(DRCategory::Slows);
    }
    assert!(immune.is_immune(DRCategory::Slows));
    let view = warlock_scene(&dots, WarlockCurse::Exhaustion)
        .dr_tracker(FOE, immune)
        .decide(ME);
    assert!(
        !chose(&view, AbilityType::CurseOfExhaustion),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::CurseOfExhaustion),
            Some(RejectionReason::DRImmune {
                category: DRCategory::Slows
            })
        ),
        "{}",
        describe(&view)
    );

    let control = warlock_scene(&dots, WarlockCurse::Exhaustion).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::CurseOfExhaustion, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// A Divine Shield on the kill target: immune to damage.
fn divine_shield() -> Aura {
    Aura {
        effect_type: AuraType::DamageImmunity,
        duration: 10.0,
        magnitude: 1.0,
        ability_name: "Divine Shield".into(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// Shadow Trance does not spend its instant Shadow Bolt on a target immune
/// to damage: the bolt would land nothing and waste the trance. Its control,
/// the same trance on a target that is not immune, is the bolt
/// (`shadow_trance_puts_shadow_bolt_next`).
#[test]
fn shadow_trance_holds_its_bolt_off_an_immune_target() {
    let view = warlock_scene(&[], WarlockCurse::Agony)
        .aura_full(ME, shadow_trance())
        .aura_full(FOE, divine_shield())
        .decide(ME);
    assert!(
        !chose(&view, AbilityType::Shadowbolt),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::Shadowbolt),
            Some(RejectionReason::TargetImmune)
        ),
        "{}",
        describe(&view)
    );
    let control = warlock_scene(&[], WarlockCurse::Agony)
        .aura_full(ME, shadow_trance())
        .decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Shadowbolt, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// Siphon Life is not cast on a target immune to damage: its ticks would land
/// nothing, and the Warlock would recast it every global cooldown. Its
/// control is the same target without the immunity.
#[test]
fn siphon_life_is_held_off_an_immune_target() {
    let dots = ["Corruption", "Unstable Affliction"];
    let view = warlock_scene(&dots, WarlockCurse::Agony)
        .grant(ME, AbilityType::SiphonLife)
        .aura_full(FOE, divine_shield())
        .decide(ME);
    assert!(
        !chose(&view, AbilityType::SiphonLife),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::SiphonLife),
            Some(RejectionReason::TargetImmune)
        ),
        "{}",
        describe(&view)
    );
    let control = warlock_scene(&dots, WarlockCurse::Agony)
        .grant(ME, AbilityType::SiphonLife)
        .decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::SiphonLife, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// Siphon Life is not cast on a target the Warlock's team holds in crowd
/// control that breaks on any damage (a teammate's Polymorph): its first
/// tick would break it. Its control is the same Polymorph cast by nobody on
/// the Warlock's team (an enemy's own), which Siphon Life may break.
#[test]
fn siphon_life_does_not_break_its_teams_crowd_control() {
    let dots = ["Corruption", "Unstable Affliction"];
    let polymorphed = |by_teammate: bool| {
        let mut scene = warlock_scene(&dots, WarlockCurse::Agony)
            .grant(ME, AbilityType::SiphonLife)
            .unit("mage", 1, CharacterClass::Mage)
            .at(0.0, 5.0)
            .unit("enemy mage", 2, CharacterClass::Mage)
            .at(25.0, 5.0);
        let caster = scene.entity(if by_teammate { "mage" } else { "enemy mage" });
        scene = scene.aura_full(
            FOE,
            Aura {
                effect_type: AuraType::Polymorph,
                duration: 8.0,
                magnitude: 1.0,
                ability_name: "Polymorph".into(),
                caster: Some(caster),
                break_on_damage_threshold: 0.0,
                ..Default::default()
            },
        );
        scene.decide(ME)
    };
    let view = polymorphed(true);
    assert!(
        !chose(&view, AbilityType::SiphonLife),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::SiphonLife),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{}",
        describe(&view)
    );
    let control = polymorphed(false);
    assert_eq!(
        control.chose(),
        Some((AbilityType::SiphonLife, Some(FOE))),
        "{}",
        describe(&control)
    );
}
