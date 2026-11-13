//! AS-419 — the Rogue's Subtlety tree: the stealth opener, out of stealth.
//!
//! - **the tree**, as the card writes it: every node, its place, its ranks,
//!   its effect, its declared price and its prerequisite; Hemorrhage's and
//!   Shadow Dance's configs (what each costs, what each leaves, and that
//!   nothing removes either);
//! - **the spawn**: a Subtlety build reaches the spawned Rogue — Fleet
//!   Footed's speed through the one capped passive term, Improved Ambush's
//!   crit, Master of Subtlety's and Initiative's procs at their ranks, Cheat
//!   Death, and the two granted abilities;
//! - **the opener rule**: under Shadow Dance the cast gate lets Cheap Shot and
//!   Ambush go out of stealth, and an enemy reading the Rogue's crowd control
//!   sees the dance's Cheap Shot;
//! - **the AI**, in scene pairs: Hemorrhage in Sinister Strike's place while
//!   its debuff is down or low; Shadow Dance spent only when its stun chain
//!   can run, one pair per condition; the chain itself while it lasts;
//! - **in a match**: a dance runs Cheap Shot, Cheap Shot and Kidney Shot out
//!   of stealth, Hemorrhage stacks its wound to five, Master of Subtlety and
//!   Initiative fire, and Cheat Death saves the Rogue once.

mod common;

use std::collections::BTreeSet;

use bevy::prelude::*;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::build_headless_app;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{self, Rogue, Warrior};
use arenasim::states::match_config::RogueOpener;
use arenasim::states::play_match::abilities::{can_open, SpellSchool};
use arenasim::states::play_match::class_ai::rogue::{dance_chain, DanceChain};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::components::{
    Aura, AuraPending, AuraStacks, AuraType, Combatant, DRCategory, DRTracker, DamageScope,
    DispelType, Pet, StackScope,
};
use arenasim::states::play_match::constants::GCD;
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::next_cast::{NextCastModifier, NextCastScope};
use arenasim::states::play_match::talent_mechanics::TalentMechanic;
use arenasim::states::play_match::talent_procs::{
    ProcChance, ProcEffect, ProcRankScale, ProcTarget, TalentProc, TalentTrigger,
};
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

fn near(is: f32, want: f32) -> bool {
    (is - want).abs() <= 1e-4
}

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// Every Subtlety node as the card writes it: its place, its ranks, what a
/// rank does, its declared price and its prerequisite. Fleet Footed's speed
/// declares its price: passive speed is one capped term, which the item
/// weight does not describe.
#[test]
fn the_subtlety_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let subtlety: BTreeSet<TalentId> = talents
        .talents_for_class(Rogue)
        .filter(|(_, t)| t.tree == TreeId::Subtlety)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        subtlety,
        [
            MasterOfSubtlety,
            FleetFooted,
            Initiative,
            ImprovedAmbush,
            CheatDeath,
            Hemorrhage,
            ShadowDance
        ]
        .into_iter()
        .collect()
    );
    // Subtlety is the Rogue's third tree ("0/0/8" reads in this order).
    assert_eq!(
        talents
            .trees(Rogue)
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        vec![TreeId::Assassination, TreeId::Combat, TreeId::Subtlety]
    );

    let mos = TalentEffect::Proc(TalentProc {
        trigger: TalentTrigger::StealthBreak,
        abilities: vec![],
        crit_only: false,
        chance: ProcChance::Chance(1.0),
        ranks_add: ProcRankScale::Effect,
        effect: ProcEffect::ApplyAura {
            on: ProcTarget::Owner,
            aura_type: AuraType::DamageDealtIncrease,
            magnitude: 0.05,
            duration: 6.0,
            tick_interval: 0.0,
            dispel_type: DispelType::Untyped,
            scope: None,
            charges: None,
            name: None,
        },
        internal_cooldown: 0.0,
    });
    let initiative = TalentEffect::Proc(TalentProc {
        trigger: TalentTrigger::AbilityUsed,
        abilities: vec![],
        crit_only: false,
        chance: ProcChance::Chance(1.0),
        ranks_add: ProcRankScale::Effect,
        effect: ProcEffect::NextCast {
            modifier: NextCastModifier {
                applies_to: NextCastScope::Any,
                instant: false,
                free: false,
                crit_chance: 0.0,
                cast_time_reduction: 0.0,
                cost_reduction: 5.0,
                power: 0.0,
            },
            duration: 10.0,
            dispel_type: DispelType::Untyped,
            name: None,
        },
        internal_cooldown: 0.0,
    });
    #[allow(clippy::type_complexity)]
    let expected: [(
        TalentId,
        u8,
        u8,
        u8,
        Vec<TalentEffect>,
        Option<f32>,
        Option<(TalentId, u8)>,
    ); 7] = [
        (MasterOfSubtlety, 1, 0, 2, vec![mos], Some(11.0), None),
        (
            FleetFooted,
            1,
            2,
            2,
            vec![TalentEffect::StatAdd(TalentStat::MovementSpeed, 0.04)],
            Some(11.0),
            None,
        ),
        (Initiative, 2, 0, 2, vec![initiative], Some(11.0), None),
        (
            ImprovedAmbush,
            2,
            2,
            2,
            vec![TalentEffect::Patch {
                on: PatchTarget::Ability(AbilityType::Ambush),
                field: AbilityField::CritChance,
                op: PatchOp::Add(0.15),
            }],
            Some(11.0),
            None,
        ),
        (
            CheatDeath,
            3,
            0,
            1,
            vec![TalentEffect::Mechanic(TalentMechanic::CheatDeath {
                health: 0.1,
                reduction: 0.9,
                duration: 3.0,
            })],
            Some(11.0),
            None,
        ),
        (
            Hemorrhage,
            3,
            1,
            1,
            vec![TalentEffect::Grant(AbilityType::Hemorrhage)],
            Some(11.0),
            None,
        ),
        (
            ShadowDance,
            4,
            1,
            1,
            vec![TalentEffect::Grant(AbilityType::ShadowDance)],
            Some(22.0),
            Some((Hemorrhage, 1)),
        ),
    ];
    for (id, tier, column, ranks, per_rank, budget, requires) in expected {
        let t = talents.get(id).unwrap();
        assert_eq!(
            (t.tree, t.tier, t.column, t.max_rank),
            (TreeId::Subtlety, tier, column, ranks),
            "{id:?}"
        );
        assert_eq!(t.per_rank, per_rank, "{id:?}");
        assert_eq!(t.budget, budget, "{id:?}");
        assert_eq!(t.requires, requires, "{id:?}");
    }
}

/// Hemorrhage as the card writes it: a talent ability, a Sinister Strike for
/// 30 energy (Sinister Strike's damage fields exactly), a melee attack on the
/// global cooldown. Its debuff: +2% Physical damage taken a stack, up to 5,
/// for 15s, a wound no dispel, cleanse or purge removes.
#[test]
fn hemorrhage_is_a_cheaper_strike_whose_wound_nothing_removes() {
    let hemo = abilities().get_unchecked(&AbilityType::Hemorrhage);
    let strike = abilities().get_unchecked(&AbilityType::SinisterStrike);
    assert!(hemo.talent);
    assert!(!hemo.off_gcd);
    assert_eq!(
        (
            hemo.mana_cost,
            hemo.cooldown,
            hemo.range,
            hemo.cast_time,
            hemo.spell_school
        ),
        (30.0, 0.0, 2.5, 0.0, SpellSchool::Physical)
    );
    assert_eq!(strike.mana_cost, 40.0);
    assert_eq!(
        (
            hemo.damage_base_min,
            hemo.damage_base_max,
            hemo.damage_coefficient
        ),
        (
            strike.damage_base_min,
            strike.damage_base_max,
            strike.damage_coefficient
        ),
        "the same damage as Sinister Strike"
    );
    assert!(AbilityType::Hemorrhage.is_melee_attack());

    let aura = hemo.applies_aura.as_ref().unwrap();
    assert_eq!(
        (aura.aura_type, aura.magnitude, aura.duration),
        (AuraType::DamageTakenIncrease, 0.02, 15.0)
    );
    assert_eq!(aura.scope, Some(DamageScope::School(SpellSchool::Physical)));
    assert_eq!(aura.stacking.map(|s| s.max_stacks), Some(5));
    // Declared, not left to the Physical school (TBC DispelType 0).
    assert_eq!(aura.dispel_type, DispelType::Physical);

    let mut world = World::new();
    let (target, caster) = (world.spawn_empty().id(), world.spawn_empty().id());
    let wound = AuraPending::from_ability(target, caster, hemo)
        .unwrap()
        .aura;
    assert_eq!(wound.dispel_type, DispelType::Physical);
    assert!(!wound.can_be_dispelled(), "Dispel Magic, Cleanse");
    assert!(!wound.is_cleansable_poison(), "Cleanse's poison half");
    assert!(!wound.can_be_purged());
    // One stack per target, whoever struck: Classic's same-debuff rule.
    assert_eq!(wound.stacks.map(|s| s.scope), Some(StackScope::PerTarget));
}

/// Shadow Dance as the card writes it: a talent ability, free, off the global
/// cooldown, a 45s cooldown, an 8s window on the Rogue itself with no dispel
/// type, so no purge takes it.
#[test]
fn shadow_dance_is_a_free_window_nothing_removes() {
    let dance = abilities().get_unchecked(&AbilityType::ShadowDance);
    assert!(dance.talent);
    assert!(dance.off_gcd, "Wrath 51713: no global cooldown");
    assert_eq!(
        (dance.mana_cost, dance.cooldown, dance.cast_time),
        (0.0, 45.0, 0.0)
    );
    let aura = dance.applies_aura.as_ref().unwrap();
    assert_eq!(
        (aura.aura_type, aura.duration),
        (AuraType::ShadowDance, 8.0)
    );
    assert_eq!(aura.dispel_type, DispelType::Untyped);
    let mut world = World::new();
    let rogue = world.spawn_empty().id();
    let window = AuraPending::from_ability(rogue, rogue, dance).unwrap().aura;
    assert!(!window.can_be_purged());
    assert!(!window.can_be_dispelled());
    assert!(!AuraType::ShadowDance.is_hostile_effect());
}

// ---------------------------------------------------------------------------
// The spawn
// ---------------------------------------------------------------------------

/// The Rogue a headless match spawns (team 1, against a Warrior) with
/// `build`.
fn spawned_rogue(build: &Build) -> Combatant {
    let config = HeadlessMatchConfig {
        team1: vec!["Rogue".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(7),
        team1_builds: vec![build.clone()],
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

/// Every node in a Subtlety build: 2+2+2+1+1 below the capstone, then it.
fn deep_build() -> Build {
    Build::of(&[
        (MasterOfSubtlety, 2),
        (Initiative, 2),
        (Hemorrhage, 1),
        (ShadowDance, 1),
        (CheatDeath, 1),
        (ImprovedAmbush, 1),
    ])
}

/// The other tier-1 and tier-2 ranks: Fleet Footed and Improved Ambush.
fn swift_build() -> Build {
    Build::of(&[(FleetFooted, 2), (ImprovedAmbush, 2), (Initiative, 1)])
}

/// An empty build changes nothing the tree touches; a Subtlety build reaches
/// every number it names.
#[test]
fn a_subtlety_build_reaches_the_spawned_rogue() {
    let base = spawned_rogue(&Build::default());
    let ambush_crit = |c: &Combatant| c.book().get_unchecked(&AbilityType::Ambush).crit_chance;
    assert_eq!(base.passive_speed, 0.0);
    assert_eq!(ambush_crit(&base), 0.0);
    assert!(base.talent_procs.is_empty());
    assert!(base.talent_mechanics.is_empty());
    assert!(base.book().get(&AbilityType::Hemorrhage).is_none());
    assert!(base.book().get(&AbilityType::ShadowDance).is_none());

    // Fleet Footed 2/2: +8%, the cap, through the one passive term. Improved
    // Ambush 2/2: +30% Ambush crit.
    let swift = spawned_rogue(&swift_build());
    assert!(near(swift.passive_speed, 0.08), "{}", swift.passive_speed);
    assert!(
        near(
            swift.base_movement_speed,
            base.natural_movement_speed * 1.08
        ),
        "{} {}",
        swift.base_movement_speed,
        base.natural_movement_speed
    );
    assert!(near(ambush_crit(&swift), 0.30), "{}", ambush_crit(&swift));
    let one = spawned_rogue(&Build::of(&[(FleetFooted, 1)]));
    assert!(near(one.passive_speed, 0.04), "{}", one.passive_speed);

    // The deep build: Master of Subtlety's +10% for 6s on leaving stealth,
    // Initiative's 10 energy off the next ability, Cheat Death, and both
    // abilities in the book.
    let deep = spawned_rogue(&deep_build());
    let slots: Vec<(TalentId, ProcEffect)> = deep
        .talent_procs
        .slots
        .iter()
        .map(|s| (s.talent, s.proc.effect.clone()))
        .collect();
    assert_eq!(slots.len(), 2, "{slots:?}");
    let effect = |talent| slots.iter().find(|(t, _)| *t == talent).unwrap().1.clone();
    match effect(MasterOfSubtlety) {
        ProcEffect::ApplyAura {
            magnitude,
            duration,
            aura_type,
            ..
        } => {
            assert_eq!(aura_type, AuraType::DamageDealtIncrease);
            assert!(near(magnitude, 0.10) && near(duration, 6.0));
        }
        other => panic!("{other:?}"),
    }
    match effect(Initiative) {
        ProcEffect::NextCast { modifier, .. } => {
            assert!(near(modifier.cost_reduction, 10.0), "{modifier:?}");
            assert_eq!(modifier.applies_to, NextCastScope::Any);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(deep.talent_mechanics.cheat_death(), Some((0.1, 0.9, 3.0)));
    assert!(deep.book().get(&AbilityType::Hemorrhage).is_some());
    assert!(deep.book().get(&AbilityType::ShadowDance).is_some());
    assert!(near(ambush_crit(&deep), 0.15));
    assert_eq!(deep.passive_speed, 0.0);
}

// ---------------------------------------------------------------------------
// The opener rule
// ---------------------------------------------------------------------------

/// Shadow Dance's window on the Rogue, as the ability lands it.
fn dance_window() -> Aura {
    let mut world = World::new();
    let rogue = world.spawn_empty().id();
    let def = abilities().get_unchecked(&AbilityType::ShadowDance);
    AuraPending::from_ability(rogue, rogue, def).unwrap().aura
}

/// The one opener rule: stealthed, or dancing. Nothing else opens.
#[test]
fn an_opener_needs_stealth_or_the_dance() {
    assert!(can_open(true, None));
    assert!(!can_open(false, None));
    assert!(!can_open(false, Some(&[])));
    assert!(can_open(false, Some(&[dance_window()])));
    let other = Aura {
        effect_type: AuraType::DamageDealtIncrease,
        duration: 6.0,
        ..Default::default()
    };
    assert!(!can_open(false, Some(&[other])));
}

const ME: Who = "rogue";
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

fn chose(view: &DecisionView, ability: AbilityType) -> bool {
    view.choices.iter().any(|(a, _)| *a == ability)
}

fn note(view: &DecisionView, ability: AbilityType) -> String {
    match view.rejected(ability) {
        Some(RejectionReason::PreconditionUnmet { note }) => note.clone(),
        other => format!("{other:?}"),
    }
}

/// A Rogue out of stealth at `energy` (a fraction of its bar), in melee of
/// its kill target, a Warrior with no healer behind it, Kidney Shot ready
/// and the Warrior's stun diminishing returns clear.
fn rogue_at(energy: f32) -> Scene {
    let mut s = Scene::new()
        .unit(ME, 1, Rogue)
        .mana(energy)
        .targeting(FOE)
        .unit(FOE, 2, Warrior)
        .at(1.5, 0.0);
    s.combatant_mut(ME).stealthed = false;
    s
}

/// [`rogue_at`] full, dancing: Shadow Dance's window up.
fn dancing() -> Scene {
    rogue_at(1.0)
        .grant(ME, AbilityType::ShadowDance)
        .grant(ME, AbilityType::Hemorrhage)
        .cooldown(ME, AbilityType::ShadowDance, 40.0)
        .aura_full(ME, dance_window())
}

/// Under the dance the cast gate lets Cheap Shot go out of stealth — the
/// Rogue's opener choice notwithstanding (here Ambush) — and it costs its 60
/// energy and the global cooldown. Its control, the same Rogue without the
/// window, is refused Cheap Shot for stealth and strikes instead.
#[test]
fn the_dance_opens_cheap_shot_out_of_stealth() {
    let mut scene = dancing();
    scene.combatant_mut(ME).rogue_opener = RogueOpener::Ambush;
    let view = scene.decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::CheapShot, Some(FOE))),
        "{}",
        describe(&view)
    );
    assert!(view.auras_queued.contains(&(FOE, AuraType::Stun)));
    assert!(near(view.resource_after, 40.0), "{}", view.resource_after);
    assert!(near(view.global_cooldown_after, GCD));

    // Control: no window. The Kidney Shot planner stuns instead; Cheap Shot
    // is never a candidate out of stealth.
    let control = rogue_at(1.0)
        .grant(ME, AbilityType::ShadowDance)
        .cooldown(ME, AbilityType::ShadowDance, 40.0)
        .cooldown(ME, AbilityType::KidneyShot, 10.0)
        .decide(ME);
    assert!(
        !chose(&control, AbilityType::CheapShot),
        "{}",
        describe(&control)
    );
    assert_eq!(
        control.chose(),
        Some((AbilityType::SinisterStrike, Some(FOE))),
        "{}",
        describe(&control)
    );
}

// ---------------------------------------------------------------------------
// Hemorrhage
// ---------------------------------------------------------------------------

/// A Rogue with Hemorrhage, in melee of its kill target at full energy, out
/// of stealth, Kidney Shot on cooldown: Sinister Strike's moment.
fn strike_scene() -> Scene {
    rogue_at(1.0)
        .grant(ME, AbilityType::Hemorrhage)
        .cooldown(ME, AbilityType::KidneyShot, 10.0)
}

/// Hemorrhage's wound on the Warrior at `stacks`, with `left` seconds.
fn wound(stacks: u8, left: f32) -> Aura {
    let mut world = World::new();
    let (target, caster) = (world.spawn_empty().id(), world.spawn_empty().id());
    let def = abilities().get_unchecked(&AbilityType::Hemorrhage);
    let mut aura = AuraPending::from_ability(target, caster, def).unwrap().aura;
    aura.caster = None;
    aura.duration = left;
    aura.stacks = Some(AuraStacks {
        count: stacks,
        max: 5,
        scope: StackScope::PerTarget,
        one_target: false,
    });
    aura
}

/// Hemorrhage goes in Sinister Strike's place while its wound is down — and
/// lands it — or low: short of five stacks, or within two global cooldowns of
/// running out. Its control, a full wound with time left, is Sinister Strike,
/// Hemorrhage held for the wound being up.
#[test]
fn hemorrhage_strikes_while_its_wound_is_down_or_low() {
    let down = strike_scene().decide(ME);
    assert_eq!(
        down.chose(),
        Some((AbilityType::Hemorrhage, Some(FOE))),
        "{}",
        describe(&down)
    );
    assert!(down
        .auras_queued
        .contains(&(FOE, AuraType::DamageTakenIncrease)));
    assert!(down
        .landed
        .iter()
        .any(|l| l.ability == AbilityType::Hemorrhage));

    for (stacks, left) in [(3, 12.0), (5, 2.0)] {
        let low = strike_scene()
            .aura_full(FOE, wound(stacks, left))
            .decide(ME);
        assert_eq!(
            low.chose(),
            Some((AbilityType::Hemorrhage, Some(FOE))),
            "{stacks} stacks, {left}s: {}",
            describe(&low)
        );
    }

    let up = strike_scene().aura_full(FOE, wound(5, 12.0)).decide(ME);
    assert_eq!(
        up.chose(),
        Some((AbilityType::SinisterStrike, Some(FOE))),
        "{}",
        describe(&up)
    );
    assert_eq!(
        note(&up, AbilityType::Hemorrhage),
        "Hemorrhage is up on the target"
    );
}

/// Hemorrhage costs its 30 energy and the global cooldown; its control,
/// Sinister Strike under a full wound, costs 40, so the number read is the
/// one each choice charges.
#[test]
fn hemorrhage_costs_its_energy_and_the_global_cooldown() {
    let view = strike_scene().decide(ME);
    assert_eq!(view.chose(), Some((AbilityType::Hemorrhage, Some(FOE))));
    assert!(near(view.resource_after, 70.0), "{}", view.resource_after);
    assert!(near(view.global_cooldown_after, GCD));
    let control = strike_scene().aura_full(FOE, wound(5, 12.0)).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::SinisterStrike, Some(FOE)))
    );
    assert!(near(control.resource_after, 60.0));
}

/// Initiative's primed cut reaches Hemorrhage's charge: 10 off its 30. The
/// charge is read from the book the decision saw, not the base one.
#[test]
fn hemorrhage_pays_through_initiative() {
    let primed = Aura {
        effect_type: AuraType::NextCast,
        duration: 10.0,
        magnitude: 1.0,
        ability_name: "Initiative".into(),
        next_cast: Some(NextCastModifier {
            applies_to: NextCastScope::Any,
            instant: false,
            free: false,
            crit_chance: 0.0,
            cast_time_reduction: 0.0,
            cost_reduction: 10.0,
            power: 0.0,
        }),
        break_on_damage_threshold: -1.0,
        dispel_type: DispelType::Untyped,
        ..Default::default()
    };
    let view = strike_scene().aura_full(ME, primed).decide(ME);
    assert_eq!(view.chose(), Some((AbilityType::Hemorrhage, Some(FOE))));
    assert!(near(view.resource_after, 80.0), "{}", view.resource_after);
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

/// Hemorrhage is not struck at a target immune to damage; its control is the
/// same target without the immunity.
#[test]
fn hemorrhage_is_held_off_an_immune_target() {
    let view = strike_scene().aura_full(FOE, divine_shield()).decide(ME);
    assert!(
        !chose(&view, AbilityType::Hemorrhage),
        "{}",
        describe(&view)
    );
    assert!(view.auras_queued.is_empty(), "{}", describe(&view));
    assert_eq!(
        strike_scene().decide(ME).chose(),
        Some((AbilityType::Hemorrhage, Some(FOE)))
    );
}

/// A Polymorph on the kill target, cast by `caster`.
fn polymorph(caster: Entity) -> Aura {
    Aura {
        effect_type: AuraType::Polymorph,
        duration: 8.0,
        magnitude: 1.0,
        ability_name: "Polymorph".into(),
        caster: Some(caster),
        break_on_damage_threshold: 0.0,
        ..Default::default()
    }
}

/// [`polymorph`]ed by the Rogue's teammate, or by the enemy's own Mage.
fn polymorphed(scene: Scene, by_teammate: bool) -> Scene {
    let scene = scene
        .unit("mage", 1, CharacterClass::Mage)
        .at(0.0, 20.0)
        .unit("enemy mage", 2, CharacterClass::Mage)
        .at(25.0, 5.0);
    let caster = scene.entity(if by_teammate { "mage" } else { "enemy mage" });
    scene.aura_full(FOE, polymorph(caster))
}

/// Hemorrhage does not break its team's crowd control (a teammate's
/// Polymorph); its control is the enemy's own Polymorph, which it may break.
#[test]
fn hemorrhage_does_not_break_its_teams_crowd_control() {
    let view = polymorphed(strike_scene(), true).decide(ME);
    assert!(
        !chose(&view, AbilityType::Hemorrhage),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::Hemorrhage),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{}",
        describe(&view)
    );
    let control = polymorphed(strike_scene(), false).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Hemorrhage, Some(FOE))),
        "{}",
        describe(&control)
    );
}

// ---------------------------------------------------------------------------
// Shadow Dance: when it is spent
// ---------------------------------------------------------------------------

/// [`rogue_at`] with Shadow Dance and Hemorrhage, ready.
fn dance_scene(energy: f32) -> Scene {
    rogue_at(energy)
        .grant(ME, AbilityType::ShadowDance)
        .grant(ME, AbilityType::Hemorrhage)
}

fn danced(view: &DecisionView) -> bool {
    chose(view, AbilityType::ShadowDance)
}

/// The chain one dance asks of the shipped kit: the second Cheap Shot at
/// 3.5s, Kidney Shot at 5s, 80 energy to start (60 now, 60 at 3.5s after 70
/// back, 60 at 5s after 30 more).
#[test]
fn the_dance_chain_is_cheap_shot_cheap_shot_kidney_shot() {
    let book = abilities();
    let chain = dance_chain(
        book.get_unchecked(&AbilityType::CheapShot),
        book.get_unchecked(&AbilityType::KidneyShot),
        20.0,
        100.0,
    );
    assert_eq!(
        chain,
        DanceChain {
            kidney_at: 5.0,
            energy: 80.0
        }
    );
    // Never more than the bar holds.
    assert_eq!(
        dance_chain(
            book.get_unchecked(&AbilityType::CheapShot),
            book.get_unchecked(&AbilityType::KidneyShot),
            0.0,
            100.0,
        )
        .energy,
        100.0
    );
}

/// The dance, chosen alone, costs nothing and starts no global cooldown;
/// its cooldown starts and its window goes on the Rogue.
#[test]
fn shadow_dance_is_free_and_off_the_global_cooldown() {
    let view = dance_scene(1.0).decide(ME);
    assert_eq!(
        view.choices,
        vec![(AbilityType::ShadowDance, Some(ME))],
        "{}",
        describe(&view)
    );
    assert!(near(view.resource_after, 100.0));
    assert_eq!(view.global_cooldown_after, 0.0);
    assert!(view.auras_queued.contains(&(ME, AuraType::ShadowDance)));
    let (_, after) = dance_scene(1.0).decide_keeping(ME);
    assert_eq!(
        after.ability_cooldowns.get(&AbilityType::ShadowDance),
        Some(&45.0)
    );
}

/// Energy pooled: at 80 (the chain's need) it dances; at 70 it pools — no
/// dance, and no strike to spend what it is pooling.
#[test]
fn shadow_dance_waits_for_the_energy_its_chain_needs() {
    assert!(danced(&dance_scene(0.8).decide(ME)));
    let short = dance_scene(0.7).decide(ME);
    assert_eq!(short.chose(), None, "{}", describe(&short));
    assert_eq!(
        note(&short, AbilityType::ShadowDance),
        "pooling energy for the Shadow Dance chain"
    );
    assert_eq!(
        note(&short, AbilityType::SinisterStrike),
        "pooling energy for Shadow Dance"
    );
    assert_eq!(
        note(&short, AbilityType::Hemorrhage),
        "pooling energy for Shadow Dance"
    );
}

/// A tracker that has seen `n` crowd controls of `category`.
fn diminished(category: DRCategory, n: usize) -> DRTracker {
    let mut t = DRTracker::default();
    for _ in 0..n {
        t.apply(category);
    }
    t
}

/// DR clear: a target that has taken one stun (either bucket) since its
/// diminishing returns last reset is not danced on; the control, clear, is.
#[test]
fn shadow_dance_waits_for_clear_diminishing_returns() {
    for category in [DRCategory::Stuns, DRCategory::KidneyShotStun] {
        let view = dance_scene(1.0)
            .dr_tracker(FOE, diminished(category, 1))
            .decide(ME);
        assert!(!danced(&view), "{category:?}: {}", describe(&view));
        assert_eq!(
            note(&view, AbilityType::ShadowDance),
            "the kill target's stun diminishing returns are not clear"
        );
        // The turn is not held: the Rogue strikes.
        assert!(view.chose().is_some(), "{}", describe(&view));
    }
    assert!(danced(&dance_scene(1.0).decide(ME)));
}

/// Kidney Shot ready by the time the Cheap Shots run out (5s): 4s left
/// dances, 10s left does not.
#[test]
fn shadow_dance_waits_for_kidney_shot_to_end_the_chain() {
    let ready = dance_scene(1.0)
        .cooldown(ME, AbilityType::KidneyShot, 4.0)
        .decide(ME);
    assert!(danced(&ready), "{}", describe(&ready));
    let late = dance_scene(1.0)
        .cooldown(ME, AbilityType::KidneyShot, 10.0)
        .decide(ME);
    assert!(!danced(&late), "{}", describe(&late));
    assert_eq!(
        note(&late, AbilityType::ShadowDance),
        "Kidney Shot would not be ready for the chain"
    );
}

/// A Polymorph on `who`, cast by a unit not in the scene.
fn sheeped() -> Aura {
    Aura {
        effect_type: AuraType::Polymorph,
        duration: 6.0,
        magnitude: 1.0,
        ability_name: "Polymorph".into(),
        break_on_damage_threshold: 0.0,
        ..Default::default()
    }
}

/// The enemy healer: a free one answers the chain, so the Rogue does not
/// dance; locked (polymorphed), it dances. A chain on the healer itself
/// locks it, so the Rogue whose kill target is that healer dances.
#[test]
fn shadow_dance_waits_for_the_enemy_healer() {
    let with_priest = || {
        dance_scene(1.0)
            .unit("enemy priest", 2, CharacterClass::Priest)
            .at(15.0, 0.0)
    };
    let free = with_priest().decide(ME);
    assert!(!danced(&free), "{}", describe(&free));
    assert_eq!(
        note(&free, AbilityType::ShadowDance),
        "the enemy healer is free to answer the chain"
    );
    let locked = with_priest()
        .aura_full("enemy priest", sheeped())
        .decide(ME);
    assert!(danced(&locked), "{}", describe(&locked));

    let on_the_healer = with_priest()
        .place("enemy priest", 1.5, 0.5)
        .target(ME, "enemy priest")
        .decide(ME);
    assert!(danced(&on_the_healer), "{}", describe(&on_the_healer));
}

/// The kill window answers for the healer too: a free enemy healer that
/// cannot reach the kill target (out of its range), the target low.
#[test]
fn shadow_dance_goes_into_a_kill_window() {
    let scene = |health: f32| {
        dance_scene(1.0)
            .health_of(FOE, health)
            .unit("enemy priest", 2, CharacterClass::Priest)
            .at(80.0, 0.0)
    };
    let window = scene(0.2).decide(ME);
    assert!(
        window
            .situation
            .as_ref()
            .is_some_and(|s| s.kill_window.is_some()),
        "the scene must open a kill window: {:?}",
        window.situation
    );
    assert!(danced(&window), "{}", describe(&window));
    let healthy = scene(0.9).decide(ME);
    assert!(!danced(&healthy), "{}", describe(&healthy));
}

/// Never onto a held target: a teammate's Polymorph (crowd control that
/// breaks on damage) holds the kill target, so no dance; nor while the
/// dance is cooling down. Their control is the bare scene.
#[test]
fn shadow_dance_is_never_spent_on_its_cooldown_alone() {
    let held = polymorphed(dance_scene(1.0), true).decide(ME);
    assert!(!danced(&held), "{}", describe(&held));
    assert_eq!(
        note(&held, AbilityType::ShadowDance),
        "the kill target is already held"
    );
    let cooling = dance_scene(1.0)
        .cooldown(ME, AbilityType::ShadowDance, 20.0)
        .decide(ME);
    assert!(!danced(&cooling), "{}", describe(&cooling));
    assert!(matches!(
        cooling.rejected(AbilityType::ShadowDance),
        Some(RejectionReason::OnCooldown { .. })
    ));
    let far = dance_scene(1.0).place(FOE, 8.0, 0.0).decide(ME);
    assert!(!danced(&far), "{}", describe(&far));
    assert_eq!(
        note(&far, AbilityType::ShadowDance),
        "the kill target is out of melee"
    );
    assert!(danced(&dance_scene(1.0).decide(ME)));
}

/// Not onto a target immune to damage (Divine Shield): the decision's head
/// refuses it. Its control is the same target bare.
#[test]
fn shadow_dance_is_held_off_an_immune_target() {
    let view = dance_scene(1.0).aura_full(FOE, divine_shield()).decide(ME);
    assert!(!danced(&view), "{}", describe(&view));
    assert!(view.choices.is_empty() && view.auras_queued.is_empty());
    assert!(danced(&dance_scene(1.0).decide(ME)));
}

// ---------------------------------------------------------------------------
// Shadow Dance: the chain while it lasts
// ---------------------------------------------------------------------------

/// The Rogue's own stun on the Warrior with `left` seconds.
fn my_stun(scene: &Scene, left: f32) -> Aura {
    Aura {
        effect_type: AuraType::Stun,
        duration: left,
        magnitude: 1.0,
        ability_name: "Cheap Shot".into(),
        caster: Some(scene.entity(ME)),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// The second Cheap Shot goes on as the first's stun reaches the chain
/// window, landing at half; with more of the stun left it waits.
#[test]
fn the_dance_chains_its_second_cheap_shot() {
    let scene = dancing().dr_tracker(FOE, diminished(DRCategory::Stuns, 1));
    let stun = my_stun(&scene, 0.3);
    let view = scene.aura_full(FOE, stun).decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::CheapShot, Some(FOE))),
        "{}",
        describe(&view)
    );
}

/// Between stuns, Ambush only with energy to spare for the next Cheap Shot:
/// at 100 energy, 3s of stun left (100 - 60 + 50 = 90 when it is due), it
/// Ambushes; at 40 (40 + 50 - 60 = 30) it pools, striking with nothing.
#[test]
fn the_dance_ambushes_only_with_energy_for_the_next_cheap_shot() {
    let scene = |energy: f32| {
        let s = dancing().dr_tracker(FOE, diminished(DRCategory::Stuns, 1));
        let stun = my_stun(&s, 3.0);
        let mut s = s.aura_full(FOE, stun);
        s.combatant_mut(ME).current_mana = energy;
        s
    };
    let spare = scene(100.0).decide(ME);
    assert_eq!(
        spare.chose(),
        Some((AbilityType::Ambush, Some(FOE))),
        "{}",
        describe(&spare)
    );
    assert!(near(spare.resource_after, 40.0), "{}", spare.resource_after);
    assert!(near(spare.global_cooldown_after, GCD));
    let short = scene(40.0).decide(ME);
    assert_eq!(short.chose(), None, "{}", describe(&short));
    assert_eq!(
        note(&short, AbilityType::Ambush),
        "pooling energy for the next Cheap Shot"
    );
    assert_eq!(
        note(&short, AbilityType::SinisterStrike),
        "pooling energy for the next Cheap Shot"
    );
}

/// Short of a Cheap Shot's energy with the target free, the dance pools.
#[test]
fn the_dance_pools_for_its_cheap_shot() {
    let mut scene = dancing();
    scene.combatant_mut(ME).current_mana = 50.0;
    let view = scene.decide(ME);
    assert_eq!(view.chose(), None, "{}", describe(&view));
    assert_eq!(
        note(&view, AbilityType::Hemorrhage),
        "pooling energy for Cheap Shot"
    );
}

/// After two Cheap Shots (the stun bucket at a quarter) Kidney Shot is the
/// planner's: it pools through the Rogue's stun, Ambush untouched. With
/// Kidney Shot spent, Ambush is the dance's strike.
#[test]
fn after_the_cheap_shots_kidney_shot_then_ambush() {
    let scene = || dancing().dr_tracker(FOE, diminished(DRCategory::Stuns, 2));
    let s = scene();
    let stun = my_stun(&s, 1.5);
    let kidney = s.aura_full(FOE, stun).decide(ME);
    assert!(
        !chose(&kidney, AbilityType::Ambush),
        "{}",
        describe(&kidney)
    );
    assert!(
        !chose(&kidney, AbilityType::CheapShot),
        "{}",
        describe(&kidney)
    );
    assert_eq!(
        note(&kidney, AbilityType::KidneyShot),
        "pooling Kidney to extend opener Cheap Shot"
    );
    let s = scene();
    let stun = my_stun(&s, 0.3);
    assert_eq!(
        s.aura_full(FOE, stun).decide(ME).chose(),
        Some((AbilityType::KidneyShot, Some(FOE)))
    );

    let ambush = scene()
        .cooldown(ME, AbilityType::KidneyShot, 25.0)
        .decide(ME);
    assert_eq!(
        ambush.chose(),
        Some((AbilityType::Ambush, Some(FOE))),
        "{}",
        describe(&ambush)
    );
    // Control: not dancing, the same moment is a strike.
    let control = rogue_at(1.0)
        .grant(ME, AbilityType::ShadowDance)
        .grant(ME, AbilityType::Hemorrhage)
        .cooldown(ME, AbilityType::ShadowDance, 40.0)
        .dr_tracker(FOE, diminished(DRCategory::Stuns, 2))
        .cooldown(ME, AbilityType::KidneyShot, 25.0)
        .decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Hemorrhage, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// The dance leaves a target its team holds in breakable crowd control to
/// the ordinary rotation, which does not break it.
#[test]
fn the_dance_does_not_break_its_teams_crowd_control() {
    let view = polymorphed(dancing(), true).decide(ME);
    assert!(!chose(&view, AbilityType::CheapShot), "{}", describe(&view));
    assert!(!chose(&view, AbilityType::Ambush), "{}", describe(&view));
    assert!(view.auras_queued.is_empty(), "{}", describe(&view));
    // Control: the enemy's own Polymorph holds the target past the chain
    // window, so no Cheap Shot goes on it; the dance strikes it with Ambush.
    let control = polymorphed(dancing(), false).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Ambush, Some(FOE))),
        "{}",
        describe(&control)
    );
}

// ---------------------------------------------------------------------------
// In a match
// ---------------------------------------------------------------------------

/// A Rogue with `build` against a Warrior, neither able to die, for 90
/// seconds.
fn held_match(build: Build, seed: u64) -> common::staged::Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Rogue".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(seed),
        max_duration_secs: 90.0,
        team1_builds: vec![build],
        ..Default::default()
    };
    let tough = |c: &mut Combatant| {
        c.max_health = 1_000_000.0;
        c.current_health = 1_000_000.0;
    };
    let stage = Stage::new()
        .combatant(Unit::new(1, 0), tough)
        .combatant(Unit::new(2, 0), tough);
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

/// A Subtlety Rogue dances once the opener's diminishing returns clear, and
/// runs the chain out of stealth inside the window: Cheap Shot in full, a
/// second at half, then Kidney Shot. Its control, the same build without
/// Shadow Dance, Cheap Shots only from stealth.
#[test]
fn a_dance_runs_its_stun_chain_out_of_stealth() {
    let played = held_match(deep_build(), 3);
    let log = &played.log;
    let dances: Vec<f32> = lines(log, "[CAST] Team 1 Rogue #1 uses Shadow Dance")
        .iter()
        .map(|l| at(l))
        .collect();
    assert!(!dances.is_empty(), "{log}");
    let start = dances[0];
    let inside = |needle: &str| -> Vec<f32> {
        lines(log, needle)
            .iter()
            .map(|l| at(l))
            .filter(|t| *t >= start && *t <= start + 8.0)
            .collect()
    };
    let cheap = inside("[CAST] Team 1 Rogue #1 uses Cheap Shot on");
    assert_eq!(cheap.len(), 2, "{cheap:?}\n{log}");
    let landed = |needle: &str| {
        lines(log, needle)
            .iter()
            .any(|l| at(l) >= start && at(l) <= start + 8.0)
    };
    assert!(landed("Cheap Shot on Team 2 Warrior #1 (4.0s, DR: 100%)"));
    assert!(landed("Cheap Shot on Team 2 Warrior #1 (2.0s, DR: 50%)"));
    assert_eq!(
        inside("[CAST] Team 1 Rogue #1 uses Kidney Shot on").len(),
        1,
        "{log}"
    );
    // The dance's cooldown: never twice within 45s.
    for pair in dances.windows(2) {
        assert!(pair[1] - pair[0] >= 45.0 - 0.05, "{dances:?}");
    }

    let control = held_match(
        Build::of(&[
            (MasterOfSubtlety, 2),
            (Initiative, 2),
            (Hemorrhage, 1),
            (CheatDeath, 1),
        ]),
        3,
    );
    assert!(lines(&control.log, "Shadow Dance").is_empty());
    assert_eq!(
        lines(&control.log, "[CAST] Team 1 Rogue #1 uses Cheap Shot on").len(),
        1,
        "the opener only"
    );
}

/// Hemorrhage stacks its wound to five in play, and the untalented Rogue
/// never strikes with it.
#[test]
fn hemorrhage_stacks_its_wound_in_play() {
    let played = held_match(deep_build(), 3);
    assert!(
        !lines(&played.log, "Hemorrhage refreshed (5 stacks)").is_empty(),
        "{}",
        played.log
    );
    let control = held_match(Build::default(), 3);
    assert!(lines(&control.log, "Hemorrhage").is_empty());
}

/// Master of Subtlety fires once, as the Rogue opens from stealth; Initiative
/// primes and is spent. The untalented Rogue's log has neither.
#[test]
fn master_of_subtlety_and_initiative_fire_in_play() {
    let played = held_match(deep_build(), 3);
    assert_eq!(
        lines(&played.log, "Master of Subtlety procs").len(),
        1,
        "{}",
        played.log
    );
    assert!(!lines(&played.log, "Initiative is spent on").is_empty());
    let control = held_match(Build::default(), 3);
    assert!(lines(&control.log, "Master of Subtlety").is_empty());
    assert!(lines(&control.log, "Initiative").is_empty());
}

/// Cheat Death saves the Rogue from its first killing blow, once: a Rogue
/// that can die, against a Warrior that cannot, is saved and then killed.
/// Its control, without the talent, is killed with no save.
#[test]
fn cheat_death_saves_the_rogue_once() {
    let run = |build: Build| {
        let config = HeadlessMatchConfig {
            team1: vec!["Rogue".into()],
            team2: vec!["Warrior".into()],
            random_seed: Some(4),
            max_duration_secs: 120.0,
            team1_builds: vec![build],
            ..Default::default()
        };
        let stage = Stage::new().combatant(Unit::new(2, 0), |c: &mut Combatant| {
            c.max_health = 1_000_000.0;
            c.current_health = 1_000_000.0;
        });
        play(config, stage).log
    };
    let log = run(Build::of(&[
        (MasterOfSubtlety, 2),
        (FleetFooted, 1),
        (CheatDeath, 1),
    ]));
    assert_eq!(
        lines(
            &log,
            "[SAVED] Cheat Death prevents a killing blow on Team 1 Rogue #1"
        )
        .len(),
        1,
        "{log}"
    );
    assert!(!lines(&log, "Team 1 Rogue #1 has been eliminated").is_empty());
    let control = run(Build::of(&[(MasterOfSubtlety, 2), (FleetFooted, 2)]));
    assert!(lines(&control, "[SAVED]").is_empty());
    assert!(!lines(&control, "Team 1 Rogue #1 has been eliminated").is_empty());
}
