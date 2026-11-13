//! AS-417 — the Rogue's Assassination tree: poisons and bleeds.
//!
//! - **the tree**, as the card writes it: every node, its place, its ranks,
//!   its effect, its declared price and its prerequisite, and Rupture's
//!   config (a Physical bleed no dispel removes);
//! - **the spawn**: an Assassination build reaches the spawned Rogue — every
//!   poison's strength and chance, Kidney Shot's vulnerability, Malice's crit
//!   points, Rupture in its book and Blood Spatter on its ticks — and Master
//!   Poisoner opens the off-hand poison slot;
//! - **Improved Kidney Shot's rider**, through the real `apply_pending_auras`:
//!   it lands with the stun at the stun's diminished duration, not without
//!   it, and leaves with it however the stun ends;
//! - **the AI rule**, in scene pairs: Rupture keeps rolling on the kill
//!   target, never twice, never on a target immune to damage or held in the
//!   team's breakable crowd control, and never in the Kidney Shot planner's
//!   moment or with its pooled energy;
//! - **in a match**: Rupture rolls on its cooldown and ticks 50% harder with
//!   Blood Spatter, and a Kidney Shot victim wears the vulnerability exactly
//!   while it is stunned.

mod common;

use std::collections::BTreeSet;

use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::build_headless_app;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{self, Rogue, Warrior};
use arenasim::states::match_config::{MatchConfig, RoguePoison};
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::combat_core::{apply_damage_with_absorb, DamageFollowUps, Hit};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, Combatant, CompoundDebuff,
    DRCategory, DRTracker, DispelType, Pet,
};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::talent_choices::ChoiceSlot;
use arenasim::states::play_match::talents::{
    AbilityField, Build, PatchOp, PatchTarget, TalentDefinitions, TalentEffect, TalentId,
    TalentStat, TreeId,
};
use arenasim::states::play_match::AbilityType;
use common::staged::play;
use TalentId::*;

fn abilities() -> &'static arenasim::states::play_match::ability_config::AbilityDefinitions {
    arenasim::states::play_match::ability_config::AbilityDefinitions::shipped()
}

const POISONS: [AbilityType; 4] = [
    AbilityType::CripplingPoison,
    AbilityType::DeadlyPoison,
    AbilityType::WoundPoison,
    AbilityType::MindNumbingPoison,
];

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// Every Assassination node as the card writes it: its place, its ranks,
/// what a rank does, its declared price and its prerequisite. Malice's price
/// is computed (crit points at their item weight), so it declares none.
#[test]
fn the_assassination_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let assassination: BTreeSet<TalentId> = talents
        .talents_for_class(Rogue)
        .filter(|(_, t)| t.tree == TreeId::Assassination)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        assassination,
        [
            VilePoisons,
            ImprovedPoisons,
            MasterPoisoner,
            ImprovedKidneyShot,
            Malice,
            Rupture,
            BloodSpatter
        ]
        .into_iter()
        .collect()
    );
    // Assassination leads the Rogue's trees ("6/0" reads in this order).
    assert_eq!(
        talents
            .trees(Rogue)
            .first()
            .map(|t| (t.id, t.name.as_str())),
        Some((TreeId::Assassination, "Assassination"))
    );

    let patch = |on, field, op| TalentEffect::Patch { on, field, op };
    let each_poison = |field, op| -> Vec<TalentEffect> {
        POISONS
            .iter()
            .map(|p| {
                let op = match (op, *p) {
                    // Deadly's strength is its damage: +20% a rank.
                    (PatchOp::Mul(_), AbilityType::DeadlyPoison) => PatchOp::Mul(1.20),
                    _ => op,
                };
                patch(PatchTarget::Ability(*p), field, op)
            })
            .collect()
    };
    let expected: [(
        TalentId,
        u8,
        u8,
        u8,
        Vec<TalentEffect>,
        Option<f32>,
        Option<(TalentId, u8)>,
    ); 7] = [
        (
            VilePoisons,
            1,
            0,
            2,
            each_poison(AbilityField::AuraStrength, PatchOp::Mul(1.10)),
            Some(11.0),
            None,
        ),
        (
            ImprovedPoisons,
            1,
            2,
            2,
            each_poison(AbilityField::ApplicationChance, PatchOp::Add(0.10)),
            Some(11.0),
            None,
        ),
        (
            MasterPoisoner,
            2,
            0,
            1,
            vec![TalentEffect::AddChoiceSlot(ChoiceSlot::OffHandPoison)],
            Some(11.0),
            None,
        ),
        (
            ImprovedKidneyShot,
            2,
            1,
            2,
            vec![patch(
                PatchTarget::Ability(AbilityType::KidneyShot),
                AbilityField::AuraVulnerability,
                PatchOp::Add(0.05),
            )],
            Some(11.0),
            None,
        ),
        (
            Malice,
            2,
            2,
            2,
            vec![TalentEffect::StatAdd(TalentStat::CritChance, 0.01)],
            None,
            None,
        ),
        (
            Rupture,
            3,
            1,
            1,
            vec![TalentEffect::Grant(AbilityType::Rupture)],
            Some(11.0),
            None,
        ),
        (
            BloodSpatter,
            4,
            1,
            1,
            vec![patch(
                PatchTarget::Ability(AbilityType::Rupture),
                AbilityField::PeriodicDamage,
                PatchOp::Mul(1.50),
            )],
            Some(22.0),
            Some((Rupture, 1)),
        ),
    ];
    for (id, tier, column, ranks, per_rank, budget, requires) in expected {
        let t = talents.get(id).unwrap();
        assert_eq!(
            (t.tree, t.tier, t.column, t.max_rank),
            (TreeId::Assassination, tier, column, ranks),
            "{id:?}"
        );
        assert_eq!(t.per_rank, per_rank, "{id:?}");
        assert_eq!(t.budget, budget, "{id:?}");
        assert_eq!(t.requires, requires, "{id:?}");
    }
}

/// Rupture as the card writes it: a talent ability, 25 energy, a 20s
/// cooldown, melee, 10 Physical damage every 2s for 12s. A melee attack the
/// target may dodge or parry, and a bleed no dispel, cleanse or purge
/// removes.
#[test]
fn rupture_is_a_physical_bleed_nothing_removes() {
    let rupture = abilities().get_unchecked(&AbilityType::Rupture);
    assert!(rupture.talent);
    assert_eq!(
        (
            rupture.mana_cost,
            rupture.cooldown,
            rupture.range,
            rupture.cast_time,
            rupture.spell_school
        ),
        (25.0, 20.0, 2.5, 0.0, SpellSchool::Physical)
    );
    let aura = rupture.applies_aura.as_ref().unwrap();
    assert_eq!(
        (
            aura.aura_type,
            aura.magnitude,
            aura.tick_interval,
            aura.duration
        ),
        (AuraType::DamageOverTime, 10.0, 2.0, 12.0)
    );
    assert!(AbilityType::Rupture.is_melee_attack());
    // Declared, not left to the Physical school (TBC DispelType 0).
    assert_eq!(aura.dispel_type, DispelType::Physical);

    let mut world = World::new();
    let (target, caster) = (world.spawn_empty().id(), world.spawn_empty().id());
    let bleed = AuraPending::from_ability(target, caster, rupture)
        .unwrap()
        .aura;
    assert_eq!(bleed.dispel_type, DispelType::Physical);
    assert!(!bleed.can_be_dispelled(), "Dispel Magic, Cleanse");
    assert!(!bleed.is_cleansable_poison(), "Cleanse's poison half");
    assert!(!bleed.can_be_purged());
}

// ---------------------------------------------------------------------------
// The spawn
// ---------------------------------------------------------------------------

/// The Rogue a headless match spawns (team 1, against a Warrior) with
/// `build`, its poisons `main` and `off`.
fn spawned_rogue(build: &Build, main: &str, off: Option<&str>) -> Combatant {
    let config = HeadlessMatchConfig {
        team1: vec!["Rogue".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(7),
        team1_builds: vec![build.clone()],
        team1_rogue_poisons: vec![main.into()],
        team1_rogue_offhand_poisons: vec![off.map(String::from)],
        ..Default::default()
    };
    let mut app = build_headless_app(&config, true, None);
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<(&Combatant, Option<&Pet>)>();
    let rogue = q
        .iter(world)
        .find(|(c, pet)| pet.is_none() && c.team == 1 && c.class == Rogue)
        .map(|(c, _)| c.clone());
    rogue.expect("a Rogue spawned")
}

fn near(is: f32, want: f32) -> bool {
    (is - want).abs() <= 1e-5
}

/// The tiers 1-2 poisons and stun build: every number those nodes name.
fn poison_build() -> Build {
    Build::of(&[
        (VilePoisons, 2),
        (ImprovedPoisons, 2),
        (ImprovedKidneyShot, 2),
        (Malice, 2),
    ])
}

/// The bleed build: Rupture with Blood Spatter, or (`spatter: false`) without.
fn bleed_build(spatter: bool) -> Build {
    let mut nodes = vec![
        (VilePoisons, 2),
        (ImprovedPoisons, 1),
        (MasterPoisoner, 1),
        (Rupture, 1),
    ];
    if spatter {
        nodes.push((BloodSpatter, 1));
    }
    Build::of(&nodes)
}

/// The numbers the tree touches on `c`: each poison's (aura magnitude,
/// application chance), Kidney Shot's vulnerability, its crit chance, and
/// Rupture's tick when its book has it.
#[allow(clippy::type_complexity)]
fn numbers(c: &Combatant) -> (Vec<(f32, f32)>, f32, f32, Option<f32>) {
    let book = c.book();
    let poisons = POISONS
        .iter()
        .map(|p| {
            let d = book.get_unchecked(p);
            (
                d.applies_aura.as_ref().unwrap().magnitude,
                d.application_chance.unwrap(),
            )
        })
        .collect();
    let kidney = book
        .get_unchecked(&AbilityType::KidneyShot)
        .applies_aura
        .as_ref()
        .unwrap()
        .vulnerability;
    let rupture = book
        .get(&AbilityType::Rupture)
        .map(|d| d.applies_aura.as_ref().unwrap().magnitude);
    (poisons, kidney, c.crit_chance, rupture)
}

/// An empty build changes nothing the tree touches; a deep poison build
/// reaches every number it names, relative to each poison's own base.
#[test]
fn an_assassination_build_reaches_the_spawned_rogue() {
    let (base_poisons, base_kidney, base_crit, base_rupture) =
        numbers(&spawned_rogue(&Build::default(), "Crippling", None));
    assert_eq!(base_kidney, 0.0);
    assert_eq!(base_rupture, None, "Rupture is a talent's");
    // The shipped bases: Crippling a 70% slow at 50%, Deadly 3.3 a tick at
    // 30%, Wound a 50% healing cut at 50%, Mind-numbing +60% cast time at
    // 50%.
    let want_base = [(0.3, 0.5), (3.3, 0.3), (0.5, 0.5), (0.6, 0.5)];
    for (is, want) in base_poisons.iter().zip(want_base) {
        assert!(near(is.0, want.0) && near(is.1, want.1), "{base_poisons:?}");
    }

    let (poisons, kidney, crit, rupture) =
        numbers(&spawned_rogue(&poison_build(), "Crippling", None));
    // Two ranks of Vile Poisons: strength x1.2 (x1.4 for Deadly), so the
    // slow 70% -> 84%, Deadly 3.3 -> 4.62, the healing cut 50% -> 60%, the
    // cast slow 60% -> 72%. Two of Improved Poisons: +20 points of chance.
    let want = [(0.16, 0.7), (4.62, 0.5), (0.4, 0.7), (0.72, 0.7)];
    for (is, want) in poisons.iter().zip(want) {
        assert!(near(is.0, want.0) && near(is.1, want.1), "{poisons:?}");
    }
    assert!(near(kidney, 0.10), "two ranks of 5%: {kidney}");
    assert!(
        near(crit - base_crit, 0.02),
        "two ranks of 1%: {crit} {base_crit}"
    );
    assert_eq!(rupture, None);

    // Rupture granted; Blood Spatter takes its tick from 10 to 15.
    assert_eq!(
        numbers(&spawned_rogue(&bleed_build(false), "Crippling", None)).3,
        Some(10.0)
    );
    assert_eq!(
        numbers(&spawned_rogue(&bleed_build(true), "Crippling", None)).3,
        Some(15.0)
    );
}

/// Master Poisoner opens the off-hand poison slot: a spawned Rogue with it
/// wears the off hand's poison, and a config naming one without it is
/// refused, naming the slot.
#[test]
fn master_poisoner_opens_the_off_hand_poison() {
    let talents = TalentDefinitions::shipped();
    assert!(talents.opens(&bleed_build(false), ChoiceSlot::OffHandPoison));
    assert!(!talents.opens(&poison_build(), ChoiceSlot::OffHandPoison));

    let rogue = spawned_rogue(&bleed_build(false), "Crippling", Some("Deadly"));
    assert_eq!(rogue.rogue_poison, RoguePoison::Crippling);
    assert_eq!(rogue.rogue_offhand_poison, Some(RoguePoison::Deadly));

    let refused = HeadlessMatchConfig {
        team1: vec!["Rogue".into()],
        team2: vec!["Warrior".into()],
        team1_builds: vec![poison_build()],
        team1_rogue_poisons: vec!["Crippling".into()],
        team1_rogue_offhand_poisons: vec![Some("Deadly".into())],
        ..Default::default()
    }
    .to_match_config()
    .expect_err("no talent opens the slot");
    assert!(refused.contains("OffHandPoison"), "{refused}");
}

/// The client keeps the off-hand choice legal as the build changes
/// (`MatchConfig::keep_choices_offered`): learning Master Poisoner fills the
/// slot with a poison other than the main hand's; a slot holding the main
/// hand's poison moves off it; unlearning empties it.
#[test]
fn the_off_hand_poison_follows_the_build() {
    let talents = TalentDefinitions::shipped();
    let mut config = MatchConfig::default();
    config.set_class(1, 0, Rogue);
    config.team1_rogue_poisons[0] = RoguePoison::Deadly;
    *config.build_mut(1, 0).unwrap() = bleed_build(false);
    config.keep_choices_offered(1, 0, talents);
    assert_eq!(
        config.team1_rogue_offhand_poisons[0],
        Some(RoguePoison::Crippling),
        "the first poison that is not the main hand's"
    );

    config.team1_rogue_offhand_poisons[0] = Some(RoguePoison::Wound);
    config.keep_choices_offered(1, 0, talents);
    assert_eq!(
        config.team1_rogue_offhand_poisons[0],
        Some(RoguePoison::Wound),
        "a legal choice is kept"
    );
    talents
        .validate_config_choices(&config)
        .expect("legal as kept");

    config.team1_rogue_offhand_poisons[0] = Some(RoguePoison::Deadly);
    config.keep_choices_offered(1, 0, talents);
    assert_eq!(
        config.team1_rogue_offhand_poisons[0],
        Some(RoguePoison::Crippling),
        "the main hand's poison is moved off"
    );

    *config.build_mut(1, 0).unwrap() = poison_build();
    config.keep_choices_offered(1, 0, talents);
    assert_eq!(config.team1_rogue_offhand_poisons[0], None);
    talents
        .validate_config_choices(&config)
        .expect("legal once emptied");
}

/// Choosing the off hand's poison for the main hand swaps the two hands'
/// poisons; any other choice leaves the off hand alone.
#[test]
fn choosing_the_off_hands_poison_for_the_main_hand_swaps_them() {
    let mut config = MatchConfig::default();
    config.set_class(1, 0, Rogue);
    config.team1_rogue_poisons[0] = RoguePoison::Crippling;
    config.team1_rogue_offhand_poisons[0] = Some(RoguePoison::Deadly);
    config.set_rogue_poison(1, 0, RoguePoison::Wound);
    assert_eq!(
        (
            config.team1_rogue_poisons[0],
            config.team1_rogue_offhand_poisons[0]
        ),
        (RoguePoison::Wound, Some(RoguePoison::Deadly))
    );
    config.set_rogue_poison(1, 0, RoguePoison::Deadly);
    assert_eq!(
        (
            config.team1_rogue_poisons[0],
            config.team1_rogue_offhand_poisons[0]
        ),
        (RoguePoison::Deadly, Some(RoguePoison::Wound))
    );
}

/// Improved Kidney Shot's field is a stun's: a talent patching it onto an
/// ability whose aura is not a stun is refused when the talents load.
#[test]
fn a_vulnerability_patch_needs_a_stun() {
    let text = std::fs::read_to_string(arenasim::paths::asset_path("config/talents.ron"))
        .expect("talents.ron reads");
    let from = "Patch(on: Ability(KidneyShot), field: AuraVulnerability";
    assert!(text.contains(from));
    TalentDefinitions::from_ron(&text, abilities()).expect("the shipped table loads");
    let on_a_bleed = text.replacen(
        from,
        "Patch(on: Ability(Rupture), field: AuraVulnerability",
        1,
    );
    let err = TalentDefinitions::from_ron(&on_a_bleed, abilities()).expect_err("refused");
    assert!(
        err.contains("patches AuraVulnerability") && err.contains("has it"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// Improved Kidney Shot's rider, through `apply_pending_auras`
// ---------------------------------------------------------------------------

/// The real aura-application system, with everything it reads.
fn apply_harness() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(CombatLog::default());
    app.insert_resource(ArenaDampening::default());
    app.add_systems(Update, apply_pending_auras);
    app
}

/// Kidney Shot as `build`'s Rogue casts it.
fn kidney_shot(build: &Build) -> arenasim::states::play_match::ability_config::AbilityConfig {
    TalentDefinitions::shipped()
        .resolve(abilities(), Rogue, build)
        .unwrap()
        .book()
        .get_unchecked(&AbilityType::KidneyShot)
        .clone()
}

/// The auras a Warrior wears once `build`'s Kidney Shot lands on it, its DR
/// tracker having seen `prior` Kidney Shots: (type, duration, magnitude,
/// name, compound).
fn landed(
    build: &Build,
    prior: usize,
) -> Vec<(AuraType, f32, f32, String, Option<CompoundDebuff>)> {
    landed_auras(build, prior)
        .iter()
        .map(|x| {
            (
                x.effect_type,
                x.duration,
                x.magnitude,
                x.ability_name.clone(),
                x.compound,
            )
        })
        .collect()
}

/// The auras themselves, as [`landed`] reads them.
fn landed_auras(build: &Build, prior: usize) -> Vec<Aura> {
    let mut app = apply_harness();
    let mut tracker = DRTracker::default();
    for _ in 0..prior {
        tracker.apply(DRCategory::KidneyShotStun);
    }
    let rogue = app
        .world_mut()
        .spawn((Combatant::new(1, 0, Rogue), Transform::default()))
        .id();
    let victim = app
        .world_mut()
        .spawn((Combatant::new(2, 0, Warrior), Transform::default(), tracker))
        .id();
    let pending = AuraPending::from_ability(victim, rogue, &kidney_shot(build)).unwrap();
    app.world_mut().spawn(pending);
    app.update();
    app.world()
        .entity(victim)
        .get::<ActiveAuras>()
        .map(|a| a.auras.clone())
        .unwrap_or_default()
}

/// With Improved Kidney Shot the stun lands with its rider: 10% more damage
/// taken, named Kidney Shot, for the stun's 6 seconds, one compound. Its
/// control, the untalented stun, lands alone.
#[test]
fn a_kidney_shot_lands_its_vulnerability_with_the_stun() {
    let compound = Some(CompoundDebuff::StunVulnerability);
    let ks = "Kidney Shot".to_string();
    assert_eq!(
        landed(&poison_build(), 0),
        vec![
            (AuraType::Stun, 6.0, 1.0, ks.clone(), compound),
            (
                AuraType::DamageTakenIncrease,
                6.0,
                0.10,
                ks.clone(),
                compound
            ),
        ]
    );
    assert_eq!(
        landed(&Build::default(), 0),
        vec![(AuraType::Stun, 6.0, 1.0, ks, None)]
    );
    // Both members are Physical, as the stun is: nothing removes the rider
    // that would not remove the stun. (A dispellable rider would let a
    // Dispel Magic take it, and `remove_debuff_at` the stun with it.)
    let members = landed_auras(&poison_build(), 0);
    assert_eq!(members.len(), 2);
    for member in &members {
        assert_eq!(
            member.dispel_type,
            DispelType::Physical,
            "{:?}",
            member.effect_type
        );
        assert!(!member.can_be_dispelled(), "{:?}", member.effect_type);
        assert!(!member.is_cleansable_poison(), "{:?}", member.effect_type);
        assert!(!member.can_be_purged(), "{:?}", member.effect_type);
    }
}

/// A diminished stun's rider lasts the diminished time; a stun DR-immunity
/// turns away lands no rider either.
#[test]
fn the_vulnerability_follows_the_stuns_diminishing_returns() {
    let once = landed(&poison_build(), 1);
    assert_eq!(once.len(), 2, "{once:?}");
    assert!(once.iter().all(|a| near(a.1, 3.0)), "{once:?}");
    assert!(landed(&poison_build(), 3).is_empty());
}

/// The stun and its rider, as they land.
fn vulnerable_stun() -> ActiveAuras {
    let ks = kidney_shot(&poison_build());
    let mut world = World::new();
    let (victim, rogue) = (world.spawn_empty().id(), world.spawn_empty().id());
    let face = AuraPending::from_ability(victim, rogue, &ks).unwrap().aura;
    let mut rider = arenasim::states::play_match::combat_core::compound_riders(
        CompoundDebuff::StunVulnerability,
    )
    .remove(0);
    rider.magnitude = 0.10;
    ActiveAuras {
        auras: vec![face, rider],
    }
}

/// The victim takes 10% more from every hit while it wears the rider, of
/// any school.
#[test]
fn the_vulnerability_raises_every_hit() {
    let mut world = World::new();
    let victim = world.spawn_empty().id();
    for school in [SpellSchool::Physical, SpellSchool::Fire] {
        let mut target = Combatant::new(2, 0, CharacterClass::Mage);
        target.armor = 0.0;
        target.fire_resistance = 0.0;
        target.max_health = 1000.0;
        target.current_health = 1000.0;
        let mut auras = vulnerable_stun();
        let (to_health, _) = apply_damage_with_absorb(
            Hit::new(100.0, school, "Probe", None, None, victim),
            &mut target,
            Some(&mut auras),
            &mut DamageFollowUps::default(),
            &mut CombatLog::default(),
        );
        assert!(near(to_health, 110.0), "{school:?}: {to_health}");
    }
}

/// Whatever ends the stun ends the rider: a crowd-control break of the stun
/// (the Medallion's, `break_debuffs_where`), or the removal of either one.
#[test]
fn the_vulnerability_leaves_with_the_stun() {
    let mut auras = vulnerable_stun();
    let broken = auras.break_debuffs_where(|t| t == AuraType::Stun);
    assert_eq!(broken, vec!["Kidney Shot".to_string()]);
    assert!(auras.auras.is_empty(), "the rider outlived its stun");
    for index in 0..2 {
        let mut auras = vulnerable_stun();
        auras.remove_debuff_at(index);
        assert!(auras.auras.is_empty(), "member {index}");
    }
}

// ---------------------------------------------------------------------------
// The AI rule, in scene pairs
// ---------------------------------------------------------------------------

const ME: Who = "rogue";
const FOE: Who = "enemy warrior";

/// A Rogue with Rupture, in melee of its kill target (a Warrior), at full
/// energy and out of stealth, Kidney Shot on cooldown: the planner above
/// Rupture leaves the turn to it.
fn rupture_scene() -> Scene {
    let mut s = Scene::new()
        .unit(ME, 1, Rogue)
        .mana(1.0)
        .targeting(FOE)
        .unit(FOE, 2, Warrior)
        .at(1.5, 0.0)
        .grant(ME, AbilityType::Rupture)
        .cooldown(ME, AbilityType::KidneyShot, 10.0);
    s.combatant_mut(ME).stealthed = false;
    s
}

/// A bleed on the Warrior, from `caster`.
fn bleed(caster: Entity) -> Aura {
    Aura {
        effect_type: AuraType::DamageOverTime,
        duration: 8.0,
        magnitude: 10.0,
        tick_interval: 2.0,
        time_until_next_tick: 2.0,
        ability_name: "Rupture".into(),
        caster: Some(caster),
        break_on_damage_threshold: -1.0,
        dispel_type: DispelType::Physical,
        ..Default::default()
    }
}

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

fn chose(view: &DecisionView, ability: AbilityType) -> bool {
    view.choices.iter().any(|(a, _)| *a == ability)
}

/// Rupture goes on the kill target, ahead of Sinister Strike, and opens the
/// bleed; its control, the target already bleeding from this Rogue's
/// Rupture, is struck instead.
#[test]
fn rupture_goes_on_the_kill_target_unless_it_is_already_there() {
    let view = rupture_scene().decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Rupture, Some(FOE))),
        "{}",
        describe(&view)
    );
    assert!(view.auras_queued.contains(&(FOE, AuraType::DamageOverTime)));

    let scene = rupture_scene();
    let me = scene.entity(ME);
    let control = scene.aura_full(FOE, bleed(me)).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::SinisterStrike, Some(FOE))),
        "{}",
        describe(&control)
    );
    assert!(
        matches!(
            control.rejected(AbilityType::Rupture),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&control)
    );
}

/// Opening Rupture spends its 25 energy and the global cooldown; its control,
/// the Rogue striking instead with Rupture on cooldown, spends Sinister
/// Strike's cost, so the number read is the one each choice charges.
#[test]
fn rupture_costs_its_energy_and_the_global_cooldown() {
    let max = rupture_scene().combatant_mut(ME).max_mana;
    let view = rupture_scene().decide(ME);
    assert_eq!(view.chose(), Some((AbilityType::Rupture, Some(FOE))));
    assert!(
        near(view.resource_after, max - 25.0),
        "{}",
        view.resource_after
    );
    assert!(
        near(
            view.global_cooldown_after,
            arenasim::states::play_match::constants::GCD
        ),
        "{}",
        view.global_cooldown_after
    );
    let strike = abilities()
        .get_unchecked(&AbilityType::SinisterStrike)
        .mana_cost;
    let control = rupture_scene()
        .cooldown(ME, AbilityType::Rupture, 5.0)
        .decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::SinisterStrike, Some(FOE)))
    );
    assert!(
        near(control.resource_after, max - strike),
        "{}",
        control.resource_after
    );
}

/// Another Rogue's Rupture on the target is not this one's: it still opens
/// its own.
#[test]
fn another_rogues_rupture_does_not_hold_this_ones() {
    let mut scene = rupture_scene().unit("other rogue", 1, Rogue).at(0.0, 1.5);
    let other = scene.entity("other rogue");
    scene = scene.aura_full(FOE, bleed(other));
    let view = scene.decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Rupture, Some(FOE))),
        "{}",
        describe(&view)
    );
}

/// Rupture rolls again only as its cooldown allows: on cooldown, the bleed
/// gone, the Rogue strikes and Rupture is refused for its cooldown.
#[test]
fn rupture_waits_for_its_cooldown() {
    let view = rupture_scene()
        .cooldown(ME, AbilityType::Rupture, 5.0)
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::SinisterStrike, Some(FOE))),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::Rupture),
            Some(RejectionReason::OnCooldown { .. })
        ),
        "{}",
        describe(&view)
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

/// Rupture is not opened on a target immune to damage: its ticks would land
/// nothing and its cooldown would be spent. Its control is the same target
/// without the immunity.
#[test]
fn rupture_is_held_off_an_immune_target() {
    let view = rupture_scene().aura_full(FOE, divine_shield()).decide(ME);
    assert!(!chose(&view, AbilityType::Rupture), "{}", describe(&view));
    assert!(view.auras_queued.is_empty(), "{}", describe(&view));
    let control = rupture_scene().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Rupture, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// Rupture is not opened on a target the Rogue's team holds in crowd control
/// that breaks on any damage (a teammate's Polymorph): its first tick would
/// break it. Its control is the same Polymorph cast by the enemy's own Mage,
/// which the bleed may break.
#[test]
fn rupture_does_not_break_its_teams_crowd_control() {
    let polymorphed = |by_teammate: bool| {
        let mut scene = rupture_scene()
            .unit("mage", 1, CharacterClass::Mage)
            .at(0.0, 20.0)
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
    assert!(!chose(&view, AbilityType::Rupture), "{}", describe(&view));
    assert!(
        matches!(
            view.rejected(AbilityType::Rupture),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{}",
        describe(&view)
    );
    let control = polymorphed(false);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Rupture, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// A Rogue at `energy` in melee of its target, a Paladin whose Holy school
/// is locked out for `lockout` more seconds, with Rupture granted or not;
/// Kidney Shot ready.
fn kidney_scene(rupture: bool, energy: f32, lockout: f32) -> Scene {
    let mut s = Scene::new()
        .unit(ME, 1, Rogue)
        .mana(energy)
        .targeting(FOE)
        .unit(FOE, 2, CharacterClass::Paladin)
        .at(1.5, 0.0)
        .aura_full(
            FOE,
            Aura {
                effect_type: AuraType::SpellSchoolLockout,
                magnitude: SpellSchool::Holy.to_lockout_magnitude(),
                duration: lockout,
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        );
    if rupture {
        s = s.grant(ME, AbilityType::Rupture);
    }
    s.combatant_mut(ME).stealthed = false;
    s
}

/// The planner's FIRE moment is Kidney Shot's, with Rupture or without; the
/// energy it POOLS is Kidney Shot's too, so Rupture is not considered; and
/// where the planner holds the stun, Rupture goes ahead of Sinister Strike.
#[test]
fn rupture_never_takes_the_kidney_shot_planners_moment_or_energy() {
    for rupture in [false, true] {
        let fire = kidney_scene(rupture, 1.0, 0.3).decide(ME);
        assert_eq!(
            fire.chose(),
            Some((AbilityType::KidneyShot, Some(FOE))),
            "rupture {rupture}: {}",
            describe(&fire)
        );
        let pool = kidney_scene(rupture, 0.5, 2.0).decide(ME);
        assert_eq!(pool.chose(), None, "{}", describe(&pool));
        assert!(
            pool.candidates
                .iter()
                .all(|c| c.ability != AbilityType::Rupture),
            "{}",
            describe(&pool)
        );
    }
    let control = kidney_scene(false, 1.0, 3.0).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::SinisterStrike, Some(FOE))),
        "{}",
        describe(&control)
    );
    let held = kidney_scene(true, 1.0, 3.0).decide(ME);
    assert_eq!(
        held.chose(),
        Some((AbilityType::Rupture, Some(FOE))),
        "{}",
        describe(&held)
    );
    assert!(held.rejected(AbilityType::KidneyShot).is_some());
}

// ---------------------------------------------------------------------------
// In a match
// ---------------------------------------------------------------------------

/// A Rogue with `build` against a Warrior wearing no armor, neither able to
/// die, for 70 seconds.
fn held_match(build: Build, seed: u64) -> common::staged::Played {
    poisoned_match(build, seed, "Crippling", None)
}

/// [`held_match`], the Rogue's poisons `main` and `off`.
fn poisoned_match(
    build: Build,
    seed: u64,
    main: &str,
    off: Option<&str>,
) -> common::staged::Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Rogue".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(seed),
        max_duration_secs: 70.0,
        team1_builds: vec![build],
        team1_rogue_poisons: vec![main.into()],
        team1_rogue_offhand_poisons: vec![off.map(String::from)],
        ..Default::default()
    };
    let tough = |c: &mut Combatant| {
        c.max_health = 1_000_000.0;
        c.current_health = 1_000_000.0;
    };
    let stage = Stage::new().combatant(Unit::new(1, 0), tough).combatant(
        Unit::new(2, 0),
        move |c: &mut Combatant| {
            tough(c);
            c.armor = 0.0;
        },
    );
    play(config, stage)
}

fn lines<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

/// A log line's time: "[ 46.52s] ..." is 46.52.
fn at(line: &str) -> f32 {
    line.trim_start_matches('[')
        .split('s')
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// The Rogue's Rupture ticks in `log`, as the numbers they print.
fn rupture_ticks(log: &str) -> Vec<u32> {
    lines(log, "Team 1 Rogue #1's Rupture ticks for ")
        .iter()
        .map(|l| {
            l.split("ticks for ")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect()
}

/// An Assassination Rogue keeps Rupture rolling on the Warrior it fights:
/// opened again and again, each one its 20s cooldown after the last, its
/// bleed ticking 10 a tick on an unarmored target and 15 with Blood Spatter.
/// The untalented Rogue never opens one.
#[test]
fn rupture_rolls_on_its_cooldown_and_blood_spatter_deepens_it() {
    for (spatter, tick) in [(false, 10), (true, 15)] {
        let played = held_match(bleed_build(spatter), 5);
        let opened: Vec<f32> = lines(&played.log, "Team 1 Rogue #1 uses Rupture on ")
            .iter()
            .map(|l| at(l))
            .collect();
        assert!(opened.len() >= 2, "spatter {spatter}: {opened:?}");
        for pair in opened.windows(2) {
            assert!(pair[1] - pair[0] >= 20.0 - 0.05, "{opened:?}");
        }
        let ticks = rupture_ticks(&played.log);
        assert!(!ticks.is_empty(), "spatter {spatter}");
        assert!(
            ticks.iter().all(|t| *t == tick),
            "spatter {spatter}: {ticks:?}"
        );
    }
    let control = held_match(Build::default(), 5);
    assert!(lines(&control.log, "Rupture").is_empty());
}

/// An Improved Kidney Shot Rogue's victim wears the vulnerability only while
/// it is stunned, and does wear it; the untalented Rogue's never does.
#[test]
fn a_kidney_shot_victim_is_vulnerable_exactly_while_stunned() {
    let vulnerable_frames = |build: Build| -> (usize, usize) {
        let played = held_match(build, 9);
        let mut vulnerable = 0;
        let mut unstunned = 0;
        for frame in &played.frames {
            for c in frame.combatants.values() {
                if c.class == Warrior && c.aura_types.contains(&AuraType::DamageTakenIncrease) {
                    vulnerable += 1;
                    if !c.aura_types.contains(&AuraType::Stun) {
                        unstunned += 1;
                    }
                }
            }
        }
        (vulnerable, unstunned)
    };
    let log = held_match(poison_build(), 9).log;
    assert!(
        log.contains("Team 2 Warrior #1 takes 10% more damage while Kidney Shot holds"),
        "the landing is logged"
    );
    let (vulnerable, unstunned) = vulnerable_frames(poison_build());
    assert!(vulnerable > 0, "the vulnerability never landed");
    assert_eq!(unstunned, 0, "a vulnerability outlived its stun");
    assert_eq!(vulnerable_frames(Build::default()).0, 0);
}

/// Vile Poisons' slow lands in play: an 84% Crippling Poison at two ranks,
/// against its control's 70%.
#[test]
fn vile_poisons_cripples_harder_in_play() {
    let slows = |build: Build| -> BTreeSet<String> {
        lines(
            &held_match(build, 5).log,
            "Crippling Poison applied to Warrior (",
        )
        .iter()
        .map(|l| l.split('(').nth(1).unwrap().to_string())
        .collect()
    };
    assert_eq!(
        slows(poison_build()),
        ["84% slow)".to_string()].into_iter().collect()
    );
    assert_eq!(
        slows(Build::default()),
        ["70% slow)".to_string()].into_iter().collect()
    );
}

/// A Master Poisoner wears both poisons in play: Crippling on the main hand
/// and Deadly on the off hand both land. Its control, the same build with no
/// off-hand poison chosen, lands Crippling alone.
#[test]
fn master_poisoner_lands_both_poisons_in_play() {
    let landed = |off: Option<&str>| {
        let played = poisoned_match(bleed_build(false), 5, "Crippling", off);
        (
            lines(&played.log, "Crippling Poison applied to").len(),
            lines(&played.log, "Deadly Poison procs on").len(),
        )
    };
    let (crippling, deadly) = landed(Some("Deadly"));
    assert!(crippling > 0 && deadly > 0, "{crippling} {deadly}");
    let (crippling, deadly) = landed(None);
    assert!(crippling > 0, "{crippling}");
    assert_eq!(deadly, 0);
}
