//! AS-411 — the Warlock's Destruction tree: the burst tree.
//!
//! - **the tree**, as the card writes it: every node, its place, its ranks,
//!   its effect, its declared price and its prerequisite;
//! - **the spawn**: a Destruction build reaches the spawned Warlock — Bane's
//!   shorter casts, Improved Immolate's bigger landing (and not its burn),
//!   the two granted abilities, and the two procs at their ranks;
//! - **the procs**: Improved Shadow Bolt lays Shadow Vulnerability (+10%
//!   Shadow damage taken, 4 direct hits or 12s) on a Shadow Bolt's target, and
//!   Aftermath a 30% slow for 5s on a Destruction hit's; a refreshed
//!   vulnerability holds its four hits again;
//! - **the AI rules**, in scene pairs, each against its control: Shadowburn
//!   finishes a low target, Conflagrate consumes the Warlock's own Immolate,
//!   and each is held off an immune target and off one its team holds in
//!   breakable crowd control;
//! - **in a match**: each grant lands, pays its mana, starts its cooldown and
//!   takes the global cooldown; Conflagrate eats its own Immolate and nobody
//!   else's, and nothing off an immune target; the vulnerability raises a
//!   direct Shadow hit and not a tick; and every Destruction hit procs when
//!   its chance is certain.

mod common;

use std::collections::BTreeSet;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::runner::build_headless_app;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{self, Warlock, Warrior};
use arenasim::states::match_config::WarlockCurse;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::ability_config::Prevention;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::components::{
    refresh_stacking_aura, ActiveAuras, Aura, AuraPending, AuraType, CastingState, Combatant,
    DamageScope, DispelType, Pet,
};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::movement_config::load_movement_config;
use arenasim::states::play_match::talent_procs::{
    effect_aura, ProcChance, ProcEffect, ProcEvent, ProcRankScale, ProcTarget, TalentProc,
    TalentProcSlot, TalentTrigger,
};
use arenasim::states::play_match::talents::{
    AbilityField, Build, PatchOp, PatchTarget, TalentDefinitions, TalentEffect, TalentId, TreeId,
};
use arenasim::states::play_match::AbilityType;
use bevy::prelude::Entity;
use common::staged::{play, Played};
use TalentId::*;

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// Improved Shadow Bolt's proc, one rank of it.
fn improved_shadow_bolt() -> TalentProc {
    TalentProc {
        trigger: TalentTrigger::SpellHit,
        abilities: vec![AbilityType::Shadowbolt],
        crit_only: false,
        chance: ProcChance::Chance(0.10),
        ranks_add: ProcRankScale::Chance,
        effect: ProcEffect::ApplyAura {
            on: ProcTarget::Other,
            aura_type: AuraType::DamageTakenIncrease,
            magnitude: 0.10,
            duration: 12.0,
            tick_interval: 0.0,
            dispel_type: DispelType::Magic,
            scope: Some(DamageScope::School(SpellSchool::Shadow)),
            charges: Some(4),
            name: Some("Shadow Vulnerability".into()),
        },
        internal_cooldown: 0.0,
    }
}

/// Aftermath's proc, one rank of it.
fn aftermath() -> TalentProc {
    TalentProc {
        trigger: TalentTrigger::SpellHit,
        abilities: vec![
            AbilityType::Shadowbolt,
            AbilityType::Immolate,
            AbilityType::Shadowburn,
            AbilityType::Conflagrate,
        ],
        crit_only: false,
        chance: ProcChance::Chance(0.05),
        ranks_add: ProcRankScale::Chance,
        effect: ProcEffect::ApplyAura {
            on: ProcTarget::Other,
            aura_type: AuraType::MovementSpeedSlow,
            magnitude: 0.7,
            duration: 5.0,
            tick_interval: 0.0,
            dispel_type: DispelType::Magic,
            scope: None,
            charges: None,
            name: Some("Aftermath".into()),
        },
        internal_cooldown: 0.0,
    }
}

/// Every Destruction node as the card writes it.
#[test]
fn the_destruction_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let destruction: BTreeSet<TalentId> = talents
        .talents_for_class(Warlock)
        .filter(|(_, t)| t.tree == TreeId::Destruction)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        destruction,
        [
            ImprovedShadowBolt,
            Bane,
            Aftermath,
            Shadowburn,
            ImprovedImmolate,
            Conflagrate
        ]
        .into_iter()
        .collect()
    );
    // The Warlock's third tree, in WoW's order: "0/2/6" reads in it.
    let trees: Vec<(TreeId, &str)> = talents
        .trees(Warlock)
        .iter()
        .map(|t| (t.id, t.name.as_str()))
        .collect();
    assert_eq!(
        trees,
        vec![
            (TreeId::Affliction, "Affliction"),
            (TreeId::Demonology, "Demonology"),
            (TreeId::Destruction, "Destruction"),
        ]
    );

    let patch = |ability, field, op| TalentEffect::Patch {
        on: PatchTarget::Ability(ability),
        field,
        op,
    };
    let expected: [(
        TalentId,
        u8,
        u8,
        u8,
        Vec<TalentEffect>,
        f32,
        Option<(TalentId, u8)>,
    ); 6] = [
        (
            ImprovedShadowBolt,
            1,
            0,
            2,
            vec![TalentEffect::Proc(improved_shadow_bolt())],
            11.0,
            None,
        ),
        (
            Bane,
            1,
            2,
            2,
            vec![
                patch(
                    AbilityType::Shadowbolt,
                    AbilityField::CastTime,
                    PatchOp::Add(-0.25),
                ),
                patch(
                    AbilityType::Immolate,
                    AbilityField::CastTime,
                    PatchOp::Add(-0.25),
                ),
            ],
            11.0,
            None,
        ),
        (
            Aftermath,
            2,
            0,
            2,
            vec![TalentEffect::Proc(aftermath())],
            11.0,
            None,
        ),
        (
            Shadowburn,
            2,
            2,
            1,
            vec![TalentEffect::Grant(AbilityType::Shadowburn)],
            11.0,
            None,
        ),
        (
            ImprovedImmolate,
            3,
            1,
            1,
            vec![patch(
                AbilityType::Immolate,
                AbilityField::Damage,
                PatchOp::Mul(1.25),
            )],
            11.0,
            None,
        ),
        (
            Conflagrate,
            4,
            1,
            1,
            vec![TalentEffect::Grant(AbilityType::Conflagrate)],
            22.0,
            Some((ImprovedImmolate, 1)),
        ),
    ];
    for (id, tier, column, ranks, per_rank, budget, requires) in expected {
        let t = talents.get(id).unwrap();
        assert_eq!(
            (t.tree, t.tier, t.column, t.max_rank),
            (TreeId::Destruction, tier, column, ranks),
            "{id:?}"
        );
        assert_eq!(t.per_rank, per_rank, "{id:?}");
        assert_eq!(t.budget, Some(budget), "{id:?}");
        assert_eq!(t.requires, requires, "{id:?}");
    }
}

fn abilities() -> &'static AbilityDefinitions {
    AbilityDefinitions::shipped()
}

/// The two granted abilities, as the card writes them, and TBC's dispel and
/// prevention: both are spells a silence or an interrupt stops.
#[test]
fn the_destruction_abilities_are_as_specified() {
    let defs = abilities();
    let burn = defs.get_unchecked(&AbilityType::Shadowburn);
    assert!(burn.talent);
    assert_eq!(
        (
            burn.cast_time,
            burn.range,
            burn.mana_cost,
            burn.cooldown,
            burn.spell_school
        ),
        (0.0, 20.0, 40.0, 15.0, SpellSchool::Shadow)
    );
    assert_eq!(
        (
            burn.damage_base_min,
            burn.damage_base_max,
            burn.damage_coefficient
        ),
        (10.0, 10.0, 0.35)
    );
    assert!(burn.applies_aura.is_none() && burn.consumes_aura.is_none());

    let conflag = defs.get_unchecked(&AbilityType::Conflagrate);
    assert!(conflag.talent);
    assert_eq!(
        (
            conflag.cast_time,
            conflag.range,
            conflag.mana_cost,
            conflag.cooldown,
            conflag.spell_school
        ),
        (0.0, 30.0, 45.0, 15.0, SpellSchool::Fire)
    );
    assert_eq!(
        (
            conflag.damage_base_min,
            conflag.damage_base_max,
            conflag.damage_coefficient
        ),
        (20.0, 20.0, 0.4)
    );
    assert_eq!(conflag.consumes_aura, Some(AbilityType::Immolate));
    assert!(conflag.applies_aura.is_none());
    // TBC 17877 and 17962: PreventionType 1, a spell a silence stops.
    for def in [burn, conflag] {
        assert_eq!(def.prevention, Prevention::Silence, "{}", def.name);
    }
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
    (is - want).abs() <= want.abs().max(1.0) * 1e-5
}

/// The build that takes Conflagrate: every Destruction node but one rank of
/// Aftermath.
fn conflagrate_build() -> Build {
    Build::of(&[
        (ImprovedShadowBolt, 2),
        (Bane, 2),
        (Aftermath, 1),
        (Shadowburn, 1),
        (ImprovedImmolate, 1),
        (Conflagrate, 1),
    ])
}

/// An empty build changes nothing the tree touches; a deep Destruction build
/// reaches every number it names and no other.
#[test]
fn a_destruction_build_reaches_the_spawned_warlock() {
    let base = spawned_warlock(&Build::default());
    for granted in [AbilityType::Shadowburn, AbilityType::Conflagrate] {
        assert!(base.book().get(&granted).is_none(), "{granted:?}");
    }
    assert!(base.talent_procs.slots.is_empty());
    let cast = |c: &Combatant, a: AbilityType| c.book().get_unchecked(&a).cast_time;
    assert_eq!(cast(&base, AbilityType::Shadowbolt), 2.0);
    assert_eq!(cast(&base, AbilityType::Immolate), 2.0);

    // Bane: ranks add, 2.0 -> 1.75 -> 1.5, on Shadow Bolt and Immolate only.
    let one = spawned_warlock(&Build::of(&[(Bane, 1)]));
    assert!(near(cast(&one, AbilityType::Shadowbolt), 1.75));
    assert!(near(cast(&one, AbilityType::Immolate), 1.75));
    let deep = spawned_warlock(&conflagrate_build());
    assert!(near(cast(&deep, AbilityType::Shadowbolt), 1.5));
    assert!(near(cast(&deep, AbilityType::Immolate), 1.5));
    for untouched in [AbilityType::Fear, AbilityType::UnstableAffliction] {
        assert_eq!(
            cast(&deep, untouched),
            cast(&base, untouched),
            "{untouched:?}"
        );
    }

    // Improved Immolate: the landing +25%, base and coefficient; the burn
    // untouched.
    let immolate = |c: &Combatant| c.book().get_unchecked(&AbilityType::Immolate).clone();
    let (was, is) = (immolate(&base), immolate(&deep));
    assert!(near(is.damage_base_min, was.damage_base_min * 1.25));
    assert!(near(is.damage_base_max, was.damage_base_max * 1.25));
    assert!(near(is.damage_coefficient, was.damage_coefficient * 1.25));
    let tick = |d: &arenasim::states::play_match::ability_config::AbilityConfig| {
        d.applies_aura.as_ref().map(|a| (a.magnitude, a.duration))
    };
    assert_eq!(tick(&is), tick(&was));
    // Shadow Bolt's damage is Bane's cast time only.
    let bolt = |c: &Combatant| {
        let d = c.book().get_unchecked(&AbilityType::Shadowbolt);
        (d.damage_base_min, d.damage_base_max, d.damage_coefficient)
    };
    assert_eq!(bolt(&deep), bolt(&base));

    for granted in [AbilityType::Shadowburn, AbilityType::Conflagrate] {
        assert!(deep.book().get(&granted).is_some(), "{granted:?}");
    }

    // The procs at their ranks: Improved Shadow Bolt 2/2 is 20%, Aftermath
    // 1/2 is 5%; and 2/2 is 10%.
    let chances = |c: &Combatant| -> Vec<(TalentId, ProcChance)> {
        c.talent_procs
            .slots
            .iter()
            .map(|s| (s.talent, s.proc.chance))
            .collect()
    };
    assert_eq!(
        chances(&deep),
        vec![
            (ImprovedShadowBolt, ProcChance::Chance(0.20)),
            (Aftermath, ProcChance::Chance(0.05)),
        ]
    );
    let after = spawned_warlock(&Build::of(&[(Aftermath, 2), (Bane, 2)]));
    assert_eq!(chances(&after), vec![(Aftermath, ProcChance::Chance(0.10))]);
}

// ---------------------------------------------------------------------------
// The procs
// ---------------------------------------------------------------------------

fn slot_of(c: &Combatant, talent: TalentId) -> TalentProcSlot {
    c.talent_procs
        .slots
        .iter()
        .find(|s| s.talent == talent)
        .cloned()
        .unwrap_or_else(|| panic!("no {talent:?} proc"))
}

fn hit(owner: Entity, ability: AbilityType, victim: Entity) -> ProcEvent {
    ProcEvent::new(owner, TalentTrigger::SpellHit)
        .ability(ability)
        .amount(50.0)
        .other(victim)
}

/// Improved Shadow Bolt's proc qualifies on a Shadow Bolt hit only, and lays
/// Shadow Vulnerability on the bolt's target: +10% Shadow damage taken, 4
/// direct hits or 12s, Magic, refresh-only.
#[test]
fn improved_shadow_bolt_lays_shadow_vulnerability_on_the_bolts_target() {
    let warlock = spawned_warlock(&conflagrate_build());
    let slot = slot_of(&warlock, ImprovedShadowBolt);
    let (me, foe) = (Entity::from_raw(1), Entity::from_raw(2));
    for (ability, qualifies) in [
        (AbilityType::Shadowbolt, true),
        (AbilityType::Immolate, false),
        (AbilityType::Shadowburn, false),
        (AbilityType::DeathCoil, false),
    ] {
        assert_eq!(
            slot.proc.qualifies(&hit(me, ability, foe)),
            qualifies,
            "{ability:?}"
        );
    }
    assert!(!slot
        .proc
        .qualifies(&ProcEvent::new(me, TalentTrigger::DotTick).ability(AbilityType::Shadowbolt)));
    let landed = effect_aura(&slot, me, &hit(me, AbilityType::Shadowbolt, foe)).unwrap();
    assert_eq!(landed.target, foe);
    let aura = landed.aura;
    assert_eq!(
        (
            aura.effect_type,
            aura.magnitude,
            aura.duration,
            aura.dispel_type,
            aura.ability_name.as_str()
        ),
        (
            AuraType::DamageTakenIncrease,
            0.10,
            12.0,
            DispelType::Magic,
            "Shadow Vulnerability"
        )
    );
    assert_eq!(aura.caster, Some(me));
    assert_eq!(
        aura.damage_mods.scope,
        Some(DamageScope::School(SpellSchool::Shadow))
    );
    assert_eq!(aura.damage_mods.charges, Some(4));
}

/// A refreshed Shadow Vulnerability holds its four hits again, whatever it
/// had spent; its control, an aura with no charges, gains none on refresh.
#[test]
fn a_refreshed_shadow_vulnerability_holds_its_four_hits_again() {
    let warlock = spawned_warlock(&conflagrate_build());
    let slot = slot_of(&warlock, ImprovedShadowBolt);
    let (me, foe) = (Entity::from_raw(1), Entity::from_raw(2));
    let fresh = effect_aura(&slot, me, &hit(me, AbilityType::Shadowbolt, foe))
        .unwrap()
        .aura;
    let mut spent = fresh.clone();
    spent.damage_mods.charges = Some(1);
    spent.duration = 3.0;
    let mut auras = vec![spent];
    assert_eq!(refresh_stacking_aura(&mut auras, &fresh), Some(1));
    assert_eq!(auras[0].damage_mods.charges, Some(4));
    assert_eq!(auras[0].duration, 12.0);

    let after = slot_of(&warlock, Aftermath);
    let slow = effect_aura(&after, me, &hit(me, AbilityType::Immolate, foe))
        .unwrap()
        .aura;
    let mut auras = vec![slow.clone()];
    refresh_stacking_aura(&mut auras, &slow);
    assert_eq!(auras[0].damage_mods.charges, None);
}

/// Aftermath qualifies on a Destruction hit — Shadow Bolt, Immolate,
/// Shadowburn, Conflagrate — and on nothing else, and slows the target 30%
/// for 5s (Magic).
#[test]
fn aftermath_slows_a_destruction_hits_target() {
    let warlock = spawned_warlock(&conflagrate_build());
    let slot = slot_of(&warlock, Aftermath);
    let (me, foe) = (Entity::from_raw(1), Entity::from_raw(2));
    for (ability, qualifies) in [
        (AbilityType::Shadowbolt, true),
        (AbilityType::Immolate, true),
        (AbilityType::Shadowburn, true),
        (AbilityType::Conflagrate, true),
        (AbilityType::DeathCoil, false),
        (AbilityType::Corruption, false),
    ] {
        assert_eq!(
            slot.proc.qualifies(&hit(me, ability, foe)),
            qualifies,
            "{ability:?}"
        );
    }
    let landed = effect_aura(&slot, me, &hit(me, AbilityType::Conflagrate, foe)).unwrap();
    assert_eq!(landed.target, foe);
    assert_eq!(
        (
            landed.aura.effect_type,
            landed.aura.magnitude,
            landed.aura.duration,
            landed.aura.dispel_type,
            landed.aura.ability_name.as_str()
        ),
        (
            AuraType::MovementSpeedSlow,
            0.7,
            5.0,
            DispelType::Magic,
            "Aftermath"
        )
    );
    assert_eq!(landed.aura.damage_mods.charges, None);
    assert_eq!(landed.aura.damage_mods.scope, None);
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

/// A DoT of `caster`'s on the target.
fn dot(name: &str, caster: Entity) -> Aura {
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

/// A Warlock 15 yards from its kill target, a Warrior (no healer to Fear, no
/// dispeller, out of Death Coil's peel), with `dots` of its own already on
/// the Warrior, Curse of Agony its curse, and `grants` in its book.
fn warlock_scene(dots: &[&str], grants: &[AbilityType]) -> Scene {
    let mut scene = Scene::new()
        .unit(ME, 1, Warlock)
        .at(0.0, 0.0)
        .targeting(FOE)
        .unit(FOE, 2, Warrior)
        .at(15.0, 0.0);
    let me = scene.entity(ME);
    for name in dots {
        scene = scene.aura_full(FOE, dot(name, me));
    }
    scene.combatant_mut(ME).warlock_curse_prefs = vec![WarlockCurse::Agony];
    for g in grants {
        scene = scene.grant(ME, *g);
    }
    scene
}

fn chose(view: &DecisionView, ability: AbilityType) -> bool {
    view.choices.iter().any(|(a, _)| *a == ability)
}

fn kill_line() -> f32 {
    load_movement_config().unwrap().shared.kill_hp_threshold
}

/// Shadowburn finishes a target below the line the team calls an enemy low,
/// ahead of the DoTs the rotation would otherwise open with. Its control,
/// the same target just above the line, takes Corruption, and Shadowburn is
/// not asked.
#[test]
fn shadowburn_finishes_a_low_target() {
    let at = |health: f32| {
        warlock_scene(&[], &[AbilityType::Shadowburn])
            .health_of(FOE, health)
            .decide(ME)
    };
    let low = at(kill_line() - 0.01);
    assert_eq!(
        low.chose(),
        Some((AbilityType::Shadowburn, Some(FOE))),
        "{}",
        describe(&low)
    );
    let control = at(kill_line() + 0.01);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Corruption, Some(FOE))),
        "{}",
        describe(&control)
    );
    assert!(
        control
            .candidates
            .iter()
            .all(|c| c.ability != AbilityType::Shadowburn),
        "{}",
        describe(&control)
    );
}

/// A low target beyond Shadowburn's 20 yards is out of its reach: the
/// finisher is refused for range and the rotation goes on to Corruption.
#[test]
fn a_low_target_out_of_shadowburns_reach_takes_the_rotation() {
    let view = warlock_scene(&[], &[AbilityType::Shadowburn])
        .place(FOE, 25.0, 0.0)
        .health_of(FOE, 0.2)
        .decide(ME);
    assert!(
        matches!(
            view.rejected(AbilityType::Shadowburn),
            Some(RejectionReason::OutOfRange { .. })
        ),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.chose(),
        Some((AbilityType::Corruption, Some(FOE))),
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

/// Shadowburn is not spent on a low target immune to damage; its control is
/// [`shadowburn_finishes_a_low_target`]'s low target, not immune.
#[test]
fn shadowburn_is_held_off_an_immune_target() {
    let view = warlock_scene(&[], &[AbilityType::Shadowburn])
        .health_of(FOE, 0.2)
        .aura_full(FOE, divine_shield())
        .decide(ME);
    assert!(
        !chose(&view, AbilityType::Shadowburn),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::Shadowburn),
            Some(RejectionReason::TargetImmune)
        ),
        "{}",
        describe(&view)
    );
    let control = warlock_scene(&[], &[AbilityType::Shadowburn])
        .health_of(FOE, 0.2)
        .decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Shadowburn, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// `scene` with a mage on each team and the target Polymorphed by the
/// Warlock's teammate (`by_teammate`) or by the enemy's own Mage.
fn polymorphed(scene: Scene, by_teammate: bool) -> Scene {
    let scene = scene
        .unit("mage", 1, CharacterClass::Mage)
        .at(0.0, 5.0)
        .unit("enemy mage", 2, CharacterClass::Mage)
        .at(25.0, 5.0);
    let caster = scene.entity(if by_teammate { "mage" } else { "enemy mage" });
    // The curse is up on the enemy Mage too, so the rotation reaches its
    // Immolate step on the Warrior.
    let me = scene.entity(ME);
    scene
        .aura_full("enemy mage", dot("Curse of Agony", me))
        .aura_full(
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
        )
}

/// Shadowburn does not break its team's Polymorph on a low target; its
/// control, the same Polymorph cast by the enemy's own Mage, is broken.
#[test]
fn shadowburn_does_not_break_its_teams_crowd_control() {
    let at = |by_teammate: bool| {
        polymorphed(
            warlock_scene(&[], &[AbilityType::Shadowburn]).health_of(FOE, 0.2),
            by_teammate,
        )
        .decide(ME)
    };
    let view = at(true);
    assert!(
        matches!(
            view.rejected(AbilityType::Shadowburn),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{}",
        describe(&view)
    );
    let control = at(false);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Shadowburn, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// The rotation's DoTs and curse, up: Immolate's step is next.
const UP: [&str; 3] = ["Corruption", "Unstable Affliction", "Curse of Agony"];

/// The Warlock's own Immolate on the target: Conflagrate consumes it at once.
/// Its control, no Immolate there, casts Immolate, and Conflagrate is
/// refused for want of one.
#[test]
fn conflagrate_consumes_its_own_immolate() {
    let mut with = UP.to_vec();
    with.push("Immolate");
    let view = warlock_scene(&with, &[AbilityType::Conflagrate]).decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Conflagrate, Some(FOE))),
        "{}",
        describe(&view)
    );
    let control = warlock_scene(&UP, &[AbilityType::Conflagrate]).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Immolate, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// `UP` on the target, Conflagrate granted, and a second Warlock beside this
/// one whose Immolate burns on the target.
fn others_immolate_up() -> Scene {
    let scene = warlock_scene(&UP, &[AbilityType::Conflagrate])
        .unit("other warlock", 1, Warlock)
        .at(0.0, 5.0);
    let other = scene.entity("other warlock");
    scene.aura_full(FOE, dot("Immolate", other))
}

/// Another Warlock's Immolate is not this one's: it does not hold this
/// Warlock's Immolate (a DoT is per caster, AS-476), so its own goes on
/// beside it, and Conflagrate is not spent on the other's.
#[test]
fn another_warlocks_immolate_does_not_hold_this_warlocks_own() {
    let view = others_immolate_up().decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Immolate, Some(FOE))),
        "{}",
        describe(&view)
    );
    assert!(
        !chose(&view, AbilityType::Conflagrate),
        "{}",
        describe(&view)
    );
}

/// Another Warlock's Immolate is not this one's to consume. With its own
/// Immolate unavailable (locked), the chain reaches Conflagrate, which is
/// refused for want of an Immolate of its own; its control, its own Immolate
/// burning under the same lock, is consumed.
#[test]
fn another_warlocks_immolate_is_not_conflagrated() {
    let view = others_immolate_up()
        .lock(ME, AbilityType::Immolate)
        .decide(ME);
    assert!(
        !chose(&view, AbilityType::Conflagrate),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::Conflagrate),
            Some(RejectionReason::PreconditionUnmet { .. })
        ),
        "{}",
        describe(&view)
    );

    let control = immolated(&[AbilityType::Conflagrate])
        .lock(ME, AbilityType::Immolate)
        .decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Conflagrate, Some(FOE))),
        "{}",
        describe(&control)
    );
}

fn immolated(grants: &[AbilityType]) -> Scene {
    let mut with = UP.to_vec();
    with.push("Immolate");
    warlock_scene(&with, grants)
}

/// Conflagrate is not spent on a target immune to damage; its control is
/// the same target not immune ([`conflagrate_consumes_its_own_immolate`]).
#[test]
fn conflagrate_is_held_off_an_immune_target() {
    let view = immolated(&[AbilityType::Conflagrate])
        .aura_full(FOE, divine_shield())
        .decide(ME);
    assert!(
        matches!(
            view.rejected(AbilityType::Conflagrate),
            Some(RejectionReason::TargetImmune)
        ),
        "{}",
        describe(&view)
    );
    assert!(
        !chose(&view, AbilityType::Conflagrate),
        "{}",
        describe(&view)
    );
}

/// Conflagrate does not break its team's Polymorph; its control, the enemy
/// Mage's own Polymorph, is broken.
#[test]
fn conflagrate_does_not_break_its_teams_crowd_control() {
    let view = polymorphed(immolated(&[AbilityType::Conflagrate]), true).decide(ME);
    assert!(
        matches!(
            view.rejected(AbilityType::Conflagrate),
            Some(RejectionReason::FriendlyBreakableCC)
        ),
        "{}",
        describe(&view)
    );
    let control = polymorphed(immolated(&[AbilityType::Conflagrate]), false).decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::Conflagrate, Some(FOE))),
        "{}",
        describe(&control)
    );
}

/// Slowed and out of range — being kited — the Warlock holds its cast-time
/// spells; Conflagrate is instant, so it still goes. Its control, the same
/// scene without the talent, does nothing.
#[test]
fn conflagrate_goes_while_being_kited() {
    let kited = |grants: &[AbilityType]| {
        let mut scene =
            immolated(grants)
                .place(FOE, 29.0, 0.0)
                .aura(ME, AuraType::MovementSpeedSlow, 5.0);
        scene.combatant_mut(ME).current_health = scene.combatant_mut(ME).max_health;
        scene.decide(ME)
    };
    let view = kited(&[AbilityType::Conflagrate]);
    assert_eq!(
        view.chose(),
        Some((AbilityType::Conflagrate, Some(FOE))),
        "{}",
        describe(&view)
    );
    let control = kited(&[]);
    assert!(
        !chose(&control, AbilityType::Conflagrate),
        "{}",
        describe(&control)
    );
}

// ---------------------------------------------------------------------------
// In a match
// ---------------------------------------------------------------------------

const T1: Unit = Unit::new(1, 0);
const T1B: Unit = Unit::new(1, 1);
const T2: Unit = Unit::new(2, 0);

/// A Warrior held stunned the whole match, so it never closes, kicks or
/// kills.
fn held() -> Aura {
    Aura {
        effect_type: AuraType::Stun,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Held".into(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    }
}

/// `team1` (a Destruction Warlock first, `build`) against a held Warrior 15
/// yards off with a 20,000-health pool at `health`, for `secs`; the Warlock
/// with mana to spare. `more` stages the rest.
fn held_match(
    team1: &[&str],
    build: Build,
    health: f32,
    secs: f32,
    more: impl FnOnce(Stage) -> Stage,
) -> Played {
    let config = HeadlessMatchConfig {
        team1: team1.iter().map(|c| c.to_string()).collect(),
        team2: vec!["Warrior".into()],
        random_seed: Some(3),
        max_duration_secs: secs,
        team1_builds: vec![build],
        team1_warlock_curse_prefs: vec![Some(vec!["Agony".into()])],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(T1, 0.0, 0.0)
        .place(T2, 15.0, 0.0)
        .health(T1, 1.0)
        .combatant(T1, |c| {
            c.max_mana = 5_000.0;
            c.current_mana = 5_000.0;
        })
        .aura(T2, held())
        .combatant(T2, move |c| {
            c.max_health = 20_000.0;
            c.current_health = 20_000.0 * health;
        });
    play(config, more(stage))
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

/// The amount a damage line names: "... for 49 damage" is 49.
fn amount(line: &str) -> f32 {
    line.split(" for ")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no amount on {line:?}"))
}

/// The most mana the Warlock (team 1, slot 0) lost in one frame within a
/// frame of `t` on the log's clock.
fn mana_paid_at(played: &Played, t: f32) -> f32 {
    let mana = |i: usize| {
        played.frames[i]
            .combatants
            .values()
            .find(|c| !c.is_pet && c.team == 1 && c.slot == 0)
            .map(|c| c.current_mana)
            .expect("the Warlock is observed")
    };
    (1..played.frames.len())
        .filter(|&i| (played.frames[i].sim_time - t).abs() <= 0.04)
        .map(|i| mana(i - 1) - mana(i))
        .fold(f32::MIN, f32::max)
}

/// The Warlock's (team 1, slot 0) cast lines.
fn warlock_casts(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|l| l.contains("[CAST] Team 1 Warlock #1 "))
        .collect()
}

/// After each `ability` cast, the Warlock's next cast waits out the global
/// cooldown.
fn assert_global_cooldown(log: &str, ability: &str) {
    let casts = warlock_casts(log);
    let mut checked = 0;
    for pair in casts.windows(2) {
        if pair[0].contains(&format!("casts {ability} on")) {
            let gap = at(pair[1]) - at(pair[0]);
            assert!(gap >= 1.49, "{gap}s after {}: {}", pair[0], pair[1]);
            checked += 1;
        }
    }
    assert!(checked > 0, "no {ability} followed by another cast:\n{log}");
}

/// Shadowburn finishes a low held target in a match: it lands its damage,
/// pays its 40 mana as it lands, goes on its 15s cooldown and takes the
/// global cooldown.
#[test]
fn shadowburn_lands_in_a_match() {
    let played = held_match(&["Warlock"], conflagrate_build(), 0.2, 40.0, |s| s);
    let casts: Vec<f32> = lines(&played.log, "casts Shadowburn on Team 2 Warrior")
        .into_iter()
        .map(at)
        .collect();
    assert!(casts.len() >= 2, "{}", played.log);
    assert!(
        casts.windows(2).all(|w| w[1] - w[0] >= 15.0 - 0.02),
        "{casts:?}"
    );
    let hits = lines(&played.log, "Shadowburn hits Team 2 Warrior");
    assert!(!hits.is_empty(), "{}", played.log);
    // The first lands on the gate frame, whose mana the countdown still
    // holds full: the second pays where it can be seen.
    let paid = mana_paid_at(&played, at(hits[hits.len() - 1]));
    assert!(hits.len() >= 2, "{}", played.log);
    assert!((paid - 40.0).abs() < 0.5, "paid {paid}\n{}", played.log);
    assert_global_cooldown(&played.log, "Shadowburn");
}

/// Conflagrate in a match: it consumes the Warlock's own Immolate as it
/// lands, pays its 45 mana, goes on its 15s cooldown and takes the global
/// cooldown; the Immolate it ate ticks no more until the next is cast.
#[test]
fn conflagrate_consumes_the_warlocks_immolate_in_a_match() {
    let played = held_match(&["Warlock"], conflagrate_build(), 1.0, 40.0, |s| s);
    let casts: Vec<f32> = lines(&played.log, "casts Conflagrate on Team 2 Warrior")
        .into_iter()
        .map(at)
        .collect();
    assert!(casts.len() >= 2, "{}", played.log);
    assert!(
        casts.windows(2).all(|w| w[1] - w[0] >= 15.0 - 0.02),
        "{casts:?}"
    );
    let eaten: Vec<f32> = lines(
        &played.log,
        "Warlock #1's Conflagrate consumes Immolate on Team 2 Warrior",
    )
    .into_iter()
    .map(at)
    .collect();
    let hits = lines(&played.log, "Conflagrate hits Team 2 Warrior");
    assert_eq!(eaten.len(), hits.len(), "{}", played.log);
    for (e, h) in eaten.iter().zip(&hits) {
        assert!((e - at(h)).abs() < 0.02, "{e} {h}");
    }
    // The burn stops until the next Immolate lands.
    let first = eaten[0];
    let next_cast = lines(&played.log, "begins casting Immolate on")
        .into_iter()
        .map(at)
        .find(|t| *t > first)
        .expect("Immolate is cast again");
    assert!(
        lines(&played.log, "Warlock #1's Immolate ticks")
            .into_iter()
            .map(at)
            .all(|t| t <= first || t > next_cast),
        "{}",
        played.log
    );
    let paid = mana_paid_at(&played, at(hits[0]));
    assert!((paid - 45.0).abs() < 0.5, "paid {paid}\n{}", played.log);
    assert_global_cooldown(&played.log, "Conflagrate");
}

/// A Conflagrate landing on a target that carries another Warlock's Immolate
/// and none of its own takes nothing: it is the caster's own a hit consumes,
/// never a teammate's. Its control, the same landing with the caster's own
/// Immolate there too, eats the caster's and leaves the teammate's.
#[test]
fn a_conflagrate_never_eats_a_teammates_immolate() {
    let conflagrate_lands = |own: bool| {
        held_match(
            &["Warlock", "Warlock"],
            conflagrate_build(),
            1.0,
            4.0,
            move |s| {
                let s = s
                    .place(T1B, 0.0, 5.0)
                    .land(T2, AbilityType::Immolate, T1B)
                    .cooldown(T1, AbilityType::Conflagrate, 0.0);
                let s = if own {
                    s.land(T2, AbilityType::Immolate, T1)
                } else {
                    s
                };
                s.world(|w| {
                    let (me, foe) = (T1.entity(w), T2.entity(w));
                    w.entity_mut(me)
                        .insert(CastingState::new(AbilityType::Conflagrate, foe, 0.0));
                })
            },
        )
    };
    let none_of_its_own = conflagrate_lands(false);
    assert!(
        !lines(&none_of_its_own.log, "Conflagrate hits Team 2 Warrior").is_empty(),
        "{}",
        none_of_its_own.log
    );
    assert!(
        lines(&none_of_its_own.log, "consumes Immolate").is_empty(),
        "{}",
        none_of_its_own.log
    );
    assert!(
        !lines(&none_of_its_own.log, "Warlock #2's Immolate ticks").is_empty(),
        "{}",
        none_of_its_own.log
    );
    let control = conflagrate_lands(true);
    assert_eq!(
        lines(
            &control.log,
            "Warlock #1's Conflagrate consumes Immolate on Team 2 Warrior"
        )
        .len(),
        1,
        "{}",
        control.log
    );
    assert!(
        !lines(&control.log, "Warlock #2's Immolate ticks").is_empty(),
        "{}",
        control.log
    );
}

/// A Conflagrate landing on a target immune to damage consumes nothing; its
/// control, the same landing on a target that is not immune, eats the
/// Immolate. (The Immolate goes straight onto the target: landed through
/// `apply_pending_auras`, an immunity would refuse it.)
#[test]
fn a_conflagrate_into_an_immunity_consumes_nothing() {
    let conflagrate_lands = |immune: bool| {
        held_match(&["Warlock"], conflagrate_build(), 1.0, 1.0, move |s| {
            s.world(move |w| {
                let (me, foe) = (T1.entity(w), T2.entity(w));
                let immolate = {
                    let warlock = w.get::<Combatant>(me).expect("the Warlock");
                    let def = warlock.book().get_unchecked(&AbilityType::Immolate);
                    AuraPending::from_ability(foe, me, def)
                        .expect("Immolate burns")
                        .aura
                };
                let mut auras = w
                    .get_mut::<ActiveAuras>(foe)
                    .expect("the held Warrior's auras");
                auras.auras.push(immolate);
                if immune {
                    auras.auras.push(divine_shield());
                }
                w.entity_mut(me)
                    .insert(CastingState::new(AbilityType::Conflagrate, foe, 0.0));
            })
        })
    };
    let immune = conflagrate_lands(true);
    assert!(
        !lines(&immune.log, "Conflagrate hits Team 2 Warrior").is_empty(),
        "{}",
        immune.log
    );
    assert!(
        lines(&immune.log, "consumes Immolate").is_empty(),
        "{}",
        immune.log
    );
    // The burn is still there to the end: nothing else the Warlock casts
    // reaches an immune target.
    let last = immune.frames.last().expect("frames");
    let warrior = last
        .combatants
        .values()
        .find(|c| !c.is_pet && c.team == 2)
        .expect("the Warrior");
    assert!(
        warrior.aura_types.contains(&AuraType::DamageOverTime),
        "{:?}",
        warrior.aura_types
    );
    let control = conflagrate_lands(false);
    assert_eq!(
        lines(&control.log, "Conflagrate consumes Immolate").len(),
        1,
        "{}",
        control.log
    );
}

/// A Conflagrate an absorb soaks whole still lands, so it consumes the
/// Immolate: an absorbed hit is a hit, unlike one an immunity refuses
/// ([`a_conflagrate_into_an_immunity_consumes_nothing`], the contrast). The
/// Immolate is staged half a second from its next tick, so a burn left behind
/// would tick inside the match; the control, the same Immolate with no
/// Conflagrate, does.
#[test]
fn an_absorbed_conflagrate_still_consumes_the_immolate() {
    let staged = |conflagrate: bool| {
        held_match(&["Warlock"], conflagrate_build(), 1.0, 3.0, move |s| {
            s.world(move |w| {
                let (me, foe) = (T1.entity(w), T2.entity(w));
                let mut immolate = {
                    let warlock = w.get::<Combatant>(me).expect("the Warlock");
                    let def = warlock.book().get_unchecked(&AbilityType::Immolate);
                    AuraPending::from_ability(foe, me, def)
                        .expect("Immolate burns")
                        .aura
                };
                immolate.time_until_next_tick = 0.5;
                let shield = Aura {
                    effect_type: AuraType::Absorb,
                    duration: 30.0,
                    magnitude: 10_000.0,
                    ability_name: "Power Word: Shield".into(),
                    break_on_damage_threshold: -1.0,
                    ..Default::default()
                };
                let mut auras = w
                    .get_mut::<ActiveAuras>(foe)
                    .expect("the held Warrior's auras");
                auras.auras.push(immolate);
                auras.auras.push(shield);
                if conflagrate {
                    w.entity_mut(me)
                        .insert(CastingState::new(AbilityType::Conflagrate, foe, 0.0));
                }
            })
        })
    };
    let soaked = staged(true);
    let hits = lines(&soaked.log, "Conflagrate hits Team 2 Warrior");
    assert_eq!(hits.len(), 1, "{}", soaked.log);
    assert!(
        hits[0].contains("for 0 damage (") && hits[0].contains(" absorbed)"),
        "{}",
        hits[0]
    );
    assert_eq!(
        lines(
            &soaked.log,
            "Warlock #1's Conflagrate consumes Immolate on Team 2 Warrior"
        )
        .len(),
        1,
        "{}",
        soaked.log
    );
    assert!(
        lines(&soaked.log, "Warlock #1's Immolate ticks").is_empty(),
        "a burn was left behind:\n{}",
        soaked.log
    );
    let control = staged(false);
    assert!(
        !lines(&control.log, "Warlock #1's Immolate ticks").is_empty(),
        "{}",
        control.log
    );
}

/// The Shadow Vulnerability aura Improved Shadow Bolt's proc lands, as the
/// shipped table builds it.
fn shadow_vulnerability() -> Aura {
    let warlock = spawned_warlock(&conflagrate_build());
    let slot = slot_of(&warlock, ImprovedShadowBolt);
    let (me, foe) = (Entity::from_raw(1), Entity::from_raw(2));
    let mut aura = effect_aura(&slot, me, &hit(me, AbilityType::Shadowbolt, foe))
        .unwrap()
        .aura;
    aura.caster = None;
    aura
}

/// Under Shadow Vulnerability the Warlock's Shadowburn (a direct Shadow hit)
/// lands 10% harder than its control's, the same match without it, while
/// its Corruption's ticks (periodic) land as they would.
#[test]
fn shadow_vulnerability_raises_a_direct_shadow_hit_and_not_a_tick() {
    let vulnerable = |on: bool| {
        held_match(&["Warlock"], conflagrate_build(), 0.2, 8.0, move |s| {
            if on {
                s.aura(T2, shadow_vulnerability())
            } else {
                s
            }
        })
    };
    let burn = |p: &Played| {
        lines(&p.log, "Shadowburn hits Team 2 Warrior")
            .first()
            .map(|l| amount(l))
            .unwrap_or_else(|| panic!("no plain Shadowburn hit:\n{}", p.log))
    };
    let ticks = |p: &Played| -> Vec<f32> {
        lines(&p.log, "Corruption ticks for")
            .into_iter()
            .map(|l| {
                l.split("ticks for ")
                    .nth(1)
                    .and_then(|r| r.split_whitespace().next())
                    .and_then(|n| n.parse().ok())
                    .unwrap()
            })
            .collect()
    };
    let (on, off) = (vulnerable(true), vulnerable(false));
    let (raised, plain) = (burn(&on), burn(&off));
    assert!(
        (raised - plain * 1.1).abs() <= 1.0,
        "{plain} -> {raised}\n{}",
        on.log
    );
    let (t_on, t_off) = (ticks(&on), ticks(&off));
    assert!(!t_on.is_empty(), "{}", on.log);
    assert_eq!(t_on, t_off);
}

/// With every proc's chance made certain, each Shadow Bolt hit procs Shadow
/// Vulnerability and each Destruction hit procs Aftermath, at the hit's own
/// moment.
#[test]
fn a_certain_proc_fires_on_every_destruction_hit() {
    let build = Build::of(&[
        (ImprovedShadowBolt, 2),
        (Bane, 2),
        (Aftermath, 2),
        (Shadowburn, 1),
        (ImprovedImmolate, 1),
    ]);
    let played = held_match(&["Warlock"], build, 1.0, 30.0, |s| {
        s.combatant(T1, |c| {
            for slot in &mut c.talent_procs.slots {
                slot.proc.chance = ProcChance::Chance(1.0);
            }
        })
    });
    let landed = |name: &str| -> Vec<f32> {
        played
            .log
            .lines()
            .filter(|l| {
                l.contains(&format!("Warlock #1's {name} hits Team 2 Warrior"))
                    || l.contains(&format!("Warlock #1's {name} CRITS Team 2 Warrior"))
            })
            .map(at)
            .collect()
    };
    let procs = |name: &str| -> Vec<f32> {
        lines(&played.log, &format!("procs {name}"))
            .into_iter()
            .map(at)
            .collect()
    };
    let bolts = landed("Shadow Bolt");
    let immolates = landed("Immolate");
    assert!(!bolts.is_empty() && !immolates.is_empty(), "{}", played.log);
    let vulnerability = procs("Shadow Vulnerability");
    let after = procs("Aftermath");
    for t in &bolts {
        assert!(
            vulnerability.iter().any(|p| (p - t).abs() < 0.05),
            "no vulnerability at {t}: {}",
            played.log
        );
    }
    for t in bolts.iter().chain(&immolates) {
        assert!(
            after.iter().any(|p| (p - t).abs() < 0.05),
            "no Aftermath at {t}: {}",
            played.log
        );
    }
    // Nothing else procs them: a Corruption tick is no Destruction hit.
    assert_eq!(vulnerability.len(), bolts.len(), "{}", played.log);
}

/// The talent text reads each proc's aura in full: Shadow Vulnerability's
/// school and its hits, Aftermath's slow on the target.
#[test]
fn the_proc_text_names_the_school_and_the_hits() {
    use arenasim::states::talent_text::talent_proc_description;
    let defs = abilities();
    let isb = talent_proc_description("Improved Shadow Bolt", &improved_shadow_bolt(), defs);
    assert_eq!(
        isb,
        "Improved Shadow Bolt: A 10% chance on a hit of Shadow Bolt, increases Shadow \
         damage taken by 10% for 12 sec; lasts 4 hits (on the target). The chance is per \
         rank."
    );
    let after = talent_proc_description("Aftermath", &aftermath(), defs);
    assert_eq!(
        after,
        "Aftermath: A 5% chance on a hit of Shadow Bolt, Immolate, Shadowburn or \
         Conflagrate, slows movement speed by 30% for 5 sec (on the target). The chance is \
         per rank."
    );
}
