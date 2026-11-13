//! The Paladin's Retribution tree (AS-422): its nodes as the card writes them,
//! the numbers each build reaches, its two granted abilities' AI rules as
//! scene pairs, and each node acting in a played match.
//!
//! Each scene is a PAIR: the situation its rule answers and the control with
//! that one condition removed, so a rule deleted or mis-gated fails here.

mod common;

use std::collections::HashMap;

use bevy::prelude::*;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{Mage, Paladin, Priest, Rogue, Warrior};
use arenasim::states::play_match::abilities::{ScalingStat, SpellSchool};
use arenasim::states::play_match::ability_config::{AbilityDefinitions, Prevention};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::components::{
    passive_movement_speed, Aura, AuraPending, AuraType, Combatant, DRCategory, DRTracker,
    DispelType, GameRng,
};
use arenasim::states::play_match::constants::{GCD, PASSIVE_SPEED_CAP};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::equipment::{
    load_default_loadouts, load_item_definitions, resolve_equipped_loadout, ItemDefinitions,
    ItemId, ItemSlot, Loadout,
};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::next_cast::{NextCastModifier, NextCastScope};
use arenasim::states::play_match::talent_procs::{
    ProcChance, ProcEffect, ProcRankScale, ProcTarget, TalentProc, TalentTrigger,
};
use arenasim::states::play_match::talents::{
    Build, TalentDefinitions, TalentEffect, TalentId, TalentStat, TreeId,
};
use arenasim::states::play_match::AbilityType;
use common::staged::play;
use TalentId::*;

const PALADIN: Who = "paladin";
const MAGE: Who = "mage";
const WARRIOR: Who = "enemy warrior";
const PRIEST: Who = "enemy priest";

fn defs() -> &'static AbilityDefinitions {
    AbilityDefinitions::shipped()
}

fn cost(ability: AbilityType) -> f32 {
    defs().get_unchecked(&ability).mana_cost
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

fn precondition(view: &DecisionView, ability: AbilityType) -> bool {
    matches!(
        view.rejected(ability),
        Some(RejectionReason::PreconditionUnmet { .. })
    )
}

fn chose(view: &DecisionView) -> Option<AbilityType> {
    view.chose().map(|(a, _)| a)
}

fn near(is: f32, want: f32) -> bool {
    (is - want).abs() <= want.abs().max(1.0) * 1e-5
}

/// Devotion Aura, already up, so the Paladin's first priority (its aura) is
/// out of the way.
fn devotion() -> Aura {
    Aura {
        effect_type: AuraType::DamageTakenReduction,
        duration: 600.0,
        magnitude: 0.10,
        ability_name: "Devotion Aura".into(),
        ..Default::default()
    }
}

fn pillar(x: f32, z: f32, radius: f32) -> ObstacleVolume {
    ObstacleVolume::Cylinder {
        center_xz: Vec2::new(x, z),
        radius,
        base_y: 0.0,
        height: 10.0,
    }
}

// ---------------------------------------------------------------------------
// The tree, as the card writes it
// ---------------------------------------------------------------------------

#[test]
fn the_retribution_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let rows: Vec<(TalentId, u8, u8, u8, Option<f32>)> = talents
        .talents_for_class(Paladin)
        .filter(|(_, t)| t.tree == TreeId::Retribution)
        .map(|(id, t)| (id, t.tier, t.column, t.max_rank, t.budget))
        .collect();
    assert_eq!(
        rows,
        vec![
            (Conviction, 1, 0, 2, None),
            (PursuitOfJustice, 1, 2, 2, Some(11.0)),
            (TwoHandedWeaponSpecialization, 2, 0, 2, Some(11.0)),
            (Vengeance, 2, 1, 2, Some(11.0)),
            (TheArtOfWar, 3, 0, 1, Some(11.0)),
            (Repentance, 3, 2, 1, Some(11.0)),
            (CrusaderStrike, 4, 1, 1, Some(22.0)),
        ]
    );
    assert_eq!(
        talents
            .trees(Paladin)
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        vec![TreeId::Holy, TreeId::Protection, TreeId::Retribution]
    );
    assert_eq!(
        talents.get(CrusaderStrike).unwrap().requires,
        Some((Vengeance, 1))
    );
    assert_eq!(
        talents.get(Conviction).unwrap().per_rank,
        vec![TalentEffect::StatAdd(TalentStat::CritChance, 0.01)]
    );
    assert_eq!(
        talents.get(PursuitOfJustice).unwrap().per_rank,
        vec![TalentEffect::StatAdd(TalentStat::MovementSpeed, 0.04)]
    );
    assert_eq!(
        talents.get(TwoHandedWeaponSpecialization).unwrap().per_rank,
        vec![TalentEffect::StatPercent(
            TalentStat::TwoHandedWeaponDamage,
            0.05
        )]
    );
    for (talent, ability) in [
        (Repentance, AbilityType::Repentance),
        (CrusaderStrike, AbilityType::CrusaderStrike),
    ] {
        assert_eq!(
            talents.get(talent).unwrap().per_rank,
            vec![TalentEffect::Grant(ability)]
        );
    }
    let vengeance = |trigger: TalentTrigger, abilities: Vec<AbilityType>| {
        TalentEffect::Proc(TalentProc {
            trigger,
            abilities,
            crit_only: true,
            chance: ProcChance::Chance(1.0),
            ranks_add: ProcRankScale::Effect,
            effect: ProcEffect::ApplyAura {
                on: ProcTarget::Owner,
                aura_type: AuraType::DamageDealtIncrease,
                magnitude: 0.05,
                duration: 30.0,
                tick_interval: 0.0,
                dispel_type: DispelType::Magic,
                scope: None,
                charges: None,
                name: None,
            },
            internal_cooldown: 0.0,
        })
    };
    assert_eq!(
        talents.get(Vengeance).unwrap().per_rank,
        vec![
            vengeance(TalentTrigger::MeleeHit, vec![]),
            vengeance(TalentTrigger::SpellHit, vec![AbilityType::CrusaderStrike]),
        ]
    );
    assert_eq!(
        talents.get(TheArtOfWar).unwrap().per_rank,
        vec![TalentEffect::Proc(TalentProc {
            trigger: TalentTrigger::MeleeHit,
            abilities: vec![],
            crit_only: false,
            chance: ProcChance::PerMinute(4.8),
            ranks_add: ProcRankScale::Chance,
            effect: ProcEffect::NextCast {
                modifier: NextCastModifier {
                    applies_to: NextCastScope::Abilities(vec![AbilityType::FlashOfLight]),
                    instant: true,
                    free: true,
                    crit_chance: 0.0,
                    cast_time_reduction: 0.0,
                    cost_reduction: 0.0,
                    power: 0.0,
                },
                duration: 15.0,
                dispel_type: DispelType::Magic,
                name: Some("The Art of War".into()),
            },
            internal_cooldown: 0.0,
        })]
    );
}

/// Repentance and Crusader Strike as the card and the client write them.
#[test]
fn the_retribution_abilities_are_as_specified() {
    let repentance = defs().get_unchecked(&AbilityType::Repentance);
    assert!(repentance.talent);
    assert_eq!(repentance.class, Paladin);
    assert_eq!(
        (
            repentance.cast_time,
            repentance.range,
            repentance.mana_cost,
            repentance.cooldown
        ),
        (0.0, 20.0, 15.0, 30.0)
    );
    assert_eq!(repentance.prevention, Prevention::Silence);
    assert_eq!(repentance.spell_school, SpellSchool::Holy);
    let aura = repentance.applies_aura.as_ref().unwrap();
    assert_eq!(
        (aura.aura_type, aura.duration, aura.break_on_damage),
        (AuraType::Incapacitate, 6.0, 0.0)
    );
    // TBC 20066 is Magic: a Cleanse or Dispel Magic frees its victim, and it
    // is Magic by its school (Holy), not by a declaration.
    assert_eq!(aura.dispel_type, DispelType::Auto);
    let landed = AuraPending::from_ability(Entity::PLACEHOLDER, Entity::PLACEHOLDER, repentance)
        .unwrap()
        .aura;
    assert!(landed.can_be_dispelled());

    let strike = defs().get_unchecked(&AbilityType::CrusaderStrike);
    assert!(strike.talent);
    assert_eq!(strike.class, Paladin);
    assert_eq!(
        (
            strike.cast_time,
            strike.range,
            strike.mana_cost,
            strike.cooldown
        ),
        (0.0, 2.5, 8.0, 6.0)
    );
    assert_eq!(strike.prevention, Prevention::Pacify);
    assert_eq!(strike.spell_school, SpellSchool::Physical);
    assert_eq!(
        (
            strike.damage_scales_with,
            strike.damage_coefficient,
            strike.damage_base_min,
            strike.damage_base_max
        ),
        (ScalingStat::WeaponDamage, 1.1, 0.0, 0.0)
    );
    assert!(strike.applies_aura.is_none());
    assert!(AbilityType::CrusaderStrike.is_melee_attack());
}

/// Pursuit of Justice declares its price (passive speed is a capped term no
/// item weight prices); Conviction's is computed, +1% crit at 3. Both inside
/// the sanity bound.
#[test]
fn the_retribution_prices_are_sane() {
    let talents = TalentDefinitions::shipped();
    assert_eq!(talents.rank_price(PursuitOfJustice, None), Ok(11.0));
    assert!(!talents.get(PursuitOfJustice).unwrap().is_computed());
    let reference = Combatant::new(1, 0, Paladin);
    let conviction = talents.rank_price(Conviction, Some(&reference)).unwrap();
    assert!(near(conviction, 3.0), "{conviction}");
    for id in [Conviction, PursuitOfJustice, CrusaderStrike] {
        let node = talents.get(id).unwrap();
        let price = talents.rank_price(id, Some(&reference)).unwrap();
        assert!(node.price_sanity(price).is_ok(), "{id:?}: {price}");
    }
}

// ---------------------------------------------------------------------------
// What a build reaches
// ---------------------------------------------------------------------------

fn items() -> &'static ItemDefinitions {
    static ITEMS: std::sync::OnceLock<ItemDefinitions> = std::sync::OnceLock::new();
    ITEMS.get_or_init(|| load_item_definitions().expect("items.ron loads"))
}

/// A Paladin spawned in its default kit, its main hand replaced by `main`
/// when given, then given `build`.
fn spawned(main: Option<ItemId>, build: &[(TalentId, u8)]) -> Combatant {
    let defaults = load_default_loadouts(items()).expect("loadouts.ron loads");
    let mut chosen = Loadout::new();
    if let Some(main) = main {
        chosen.insert(ItemSlot::MainHand, main);
    }
    let loadout = resolve_equipped_loadout(Paladin, &defaults, &chosen, items());
    let mut c = Combatant::new(1, 0, Paladin);
    c.apply_equipment(&loadout, items());
    TalentDefinitions::shipped()
        .resolve(defs(), Paladin, &Build::of(build))
        .unwrap()
        .apply(&mut c);
    c
}

/// Conviction adds a point of crit a rank, on every roll that reads it.
#[test]
fn conviction_adds_a_point_of_crit_a_rank() {
    let base = spawned(None, &[]).crit_chance;
    for rank in [1u8, 2] {
        let c = spawned(None, &[(Conviction, rank)]);
        assert!(
            near(c.crit_chance, base + 0.01 * f32::from(rank)),
            "{rank}: {} from {base}",
            c.crit_chance
        );
    }
}

/// Pursuit of Justice feeds the one passive speed term: 4% at 1/2, and 8% —
/// exactly the cap — at 2/2.
#[test]
fn pursuit_of_justice_reaches_the_passive_speed_cap_at_two_ranks() {
    let base = spawned(None, &[]);
    assert_eq!(base.passive_speed, 0.0);
    let one = spawned(None, &[(PursuitOfJustice, 1)]);
    let two = spawned(None, &[(PursuitOfJustice, 2)]);
    assert!(near(one.passive_speed, 0.04), "{}", one.passive_speed);
    assert!(near(two.passive_speed, 0.08), "{}", two.passive_speed);
    assert!(near(two.passive_speed, PASSIVE_SPEED_CAP));
    for c in [&one, &two] {
        assert_eq!(
            c.base_movement_speed,
            passive_movement_speed(c.natural_movement_speed, c.passive_speed)
        );
    }
    assert!(two.base_movement_speed > one.base_movement_speed);
    assert!(one.base_movement_speed > base.base_movement_speed);
}

/// Two-Handed Weapon Specialization raises a two-hander's swing by 5% a rank
/// and does nothing with a one-hander.
#[test]
fn two_handed_specialization_reaches_only_a_two_hander() {
    let reaper = Some(ItemId::ArcaniteReaper);
    let two_hander = spawned(reaper, &[]);
    assert!(two_hander.two_handed);
    // The stat's total is the two-hander's swing, and nothing with a
    // one-hander.
    assert_eq!(
        TalentStat::TwoHandedWeaponDamage.total(&two_hander),
        two_hander.attack_damage
    );
    assert_eq!(
        TalentStat::TwoHandedWeaponDamage.total(&spawned(None, &[])),
        0.0
    );
    for rank in [1u8, 2] {
        let c = spawned(
            reaper,
            &[(Conviction, 2), (TwoHandedWeaponSpecialization, rank)],
        );
        let scale = 1.0 + 0.05 * f32::from(rank);
        assert!(
            near(c.attack_damage, two_hander.attack_damage * scale),
            "{rank}: {} from {}",
            c.attack_damage,
            two_hander.attack_damage
        );
    }
    // The default kit is a one-hander and a shield: nothing.
    let one_hander = spawned(None, &[]);
    assert!(!one_hander.two_handed);
    let c = spawned(None, &[(Conviction, 2), (TwoHandedWeaponSpecialization, 2)]);
    assert_eq!(c.attack_damage, one_hander.attack_damage);
}

/// Crusader Strike is 110% of the weapon's damage, two-handed bonus included.
#[test]
fn crusader_strike_deals_its_weapons_damage() {
    let strike = defs().get_unchecked(&AbilityType::CrusaderStrike);
    let mut rng = GameRng::from_seed(1);
    for (main, build) in [
        (None, vec![]),
        (Some(ItemId::ArcaniteReaper), vec![]),
        (
            Some(ItemId::ArcaniteReaper),
            vec![(Conviction, 2), (TwoHandedWeaponSpecialization, 2)],
        ),
    ] {
        let c = spawned(main, &build);
        let damage = c.calculate_ability_damage_config(strike, &mut rng, 0.0, 0.0);
        assert!(
            near(damage, c.attack_damage * 1.1),
            "{main:?} {build:?}: {damage} of {}",
            c.attack_damage
        );
    }
}

/// An empty build is the base Paladin; a deep build carries both grants and
/// both procs, and its ranks add to Vengeance's damage, not its chance.
#[test]
fn a_retribution_build_reaches_every_grant_and_proc() {
    let talents = TalentDefinitions::shipped();
    let base = talents.resolve(defs(), Paladin, &Build::default()).unwrap();
    for granted in [AbilityType::Repentance, AbilityType::CrusaderStrike] {
        assert!(base.book().get(&granted).is_none(), "{granted:?}");
    }
    assert!(base.procs().is_empty());
    let deep = talents
        .resolve(defs(), Paladin, &Build::of(&deep_build()))
        .unwrap();
    for granted in [AbilityType::Repentance, AbilityType::CrusaderStrike] {
        assert!(deep.book().get(&granted).is_some(), "{granted:?}");
    }
    let names: Vec<&str> = deep.procs().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Vengeance", "Vengeance", "The Art of War"]);
    for slot in &deep.procs()[..2] {
        match &slot.proc.effect {
            ProcEffect::ApplyAura { magnitude, .. } => assert!(near(*magnitude, 0.10)),
            other => panic!("{other:?}"),
        }
        assert_eq!(slot.proc.chance, ProcChance::Chance(1.0));
    }
    // The Art of War: 4.8 a minute, 20% a swing at 2.5s, 32% at 4s.
    let art = &deep.procs()[2].proc;
    assert!(near(art.chance.per_event(Some(2.5)), 0.20));
    assert!(near(art.chance.per_event(Some(4.0)), 0.32));
}

/// Eight points, every Retribution node but Pursuit of Justice.
fn deep_build() -> Vec<(TalentId, u8)> {
    vec![
        (Conviction, 2),
        (TwoHandedWeaponSpecialization, 1),
        (Vengeance, 2),
        (TheArtOfWar, 1),
        (Repentance, 1),
        (CrusaderStrike, 1),
    ]
}

// ---------------------------------------------------------------------------
// Repentance: an enemy the team is leaving alone
// ---------------------------------------------------------------------------

/// A healthy Paladin and Mage on an enemy Warrior, the Warrior on the
/// Paladin, and an enemy Priest 15 yards off that nobody has targeted.
fn healer_left_alone() -> Scene {
    Scene::new()
        .unit(PALADIN, 1, Paladin)
        .aura_full(PALADIN, devotion())
        .targeting(WARRIOR)
        .unit(MAGE, 1, Mage)
        .at(-10.0, 0.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(2.0, 0.0)
        .targeting(PALADIN)
        .unit(PRIEST, 2, Priest)
        .at(15.0, 0.0)
        .grant(PALADIN, AbilityType::Repentance)
}

fn repents(view: &DecisionView, whom: Who) -> bool {
    view.chose() == Some((AbilityType::Repentance, Some(whom)))
}

/// Repentance goes on the healer nobody is hitting, and pays its 15 mana and
/// a global cooldown. Control: the Mage on the Priest too — no enemy is left
/// alone, and Repentance is held.
#[test]
fn repentance_goes_on_the_enemy_healer_the_team_leaves_alone() {
    let mut scene = healer_left_alone();
    let mana = scene.combatant_mut(PALADIN).current_mana;
    let view = scene.decide(PALADIN);
    assert!(repents(&view, PRIEST), "{}", describe(&view));
    assert_eq!(
        view.auras_queued,
        vec![(PRIEST, AuraType::Incapacitate)],
        "{}",
        describe(&view)
    );
    assert!(near(
        view.resource_after,
        mana - cost(AbilityType::Repentance)
    ));
    assert_eq!(view.global_cooldown_after, GCD);

    let mut hit = healer_left_alone();
    hit.set_target(MAGE, PRIEST);
    let view = hit.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::Repentance),
        "{}",
        describe(&view)
    );
    assert_ne!(chose(&view), Some(AbilityType::Repentance));
}

/// The team's damage over time on the Priest would end it at once: held.
/// (The control is the scene above.)
#[test]
fn a_teams_dot_holds_repentance() {
    let scene = healer_left_alone();
    let mage = scene.entity(MAGE);
    let view = scene
        .aura_full(
            PRIEST,
            Aura {
                effect_type: AuraType::DamageOverTime,
                duration: 12.0,
                magnitude: 5.0,
                tick_interval: 3.0,
                ability_name: "Frostbolt".into(),
                caster: Some(mage),
                break_on_damage_threshold: -1.0,
                ..Default::default()
            },
        )
        .decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::Repentance),
        "{}",
        describe(&view)
    );
}

/// An enemy Rogue nobody is hitting, nearer than the Priest.
const ROGUE: Who = "enemy rogue";

fn with_rogue(scene: Scene, x: f32) -> Scene {
    let mut scene = scene.unit(ROGUE, 2, Rogue).at(x, 0.0);
    scene.combatant_mut(ROGUE).stealthed = false;
    scene
}

/// An enemy healer immune to damage, one diminished to immunity, one the team
/// already holds and one cycloned are each passed over for the next enemy
/// left alone, a Rogue: the candidate is dropped before the ranking, not
/// refused after it. Control: the Priest as it is takes it.
#[test]
fn repentance_passes_over_an_enemy_it_cannot_land_on() {
    let mut immune_dr = DRTracker::default();
    for _ in 0..3 {
        immune_dr.apply(DRCategory::Incapacitates);
    }
    let base = || with_rogue(healer_left_alone(), 6.0);
    let mage = base().entity(MAGE);
    let held = Aura {
        effect_type: AuraType::Polymorph,
        duration: 8.0,
        magnitude: 1.0,
        break_on_damage_threshold: 0.0,
        ability_name: "Polymorph".into(),
        caster: Some(mage),
        ..Default::default()
    };
    let view = base().decide(PALADIN);
    assert!(repents(&view, PRIEST), "control: {}", describe(&view));
    let cases: Vec<(&str, Scene)> = vec![
        (
            "divine shield",
            base().aura(PRIEST, AuraType::DamageImmunity, 8.0),
        ),
        ("diminished", base().dr_tracker(PRIEST, immune_dr)),
        ("held", base().aura_full(PRIEST, held)),
        ("cycloned", base().aura(PRIEST, AuraType::Cyclone, 5.0)),
    ];
    for (case, scene) in cases {
        let view = scene.decide(PALADIN);
        assert!(repents(&view, ROGUE), "{case}: {}", describe(&view));
    }
}

/// The healer first, though a Rogue nobody is hitting stands nearer; with no
/// healer, the nearest enemy left alone.
#[test]
fn repentance_takes_the_healer_first_then_the_nearest() {
    let view = with_rogue(healer_left_alone(), 6.0).decide(PALADIN);
    assert!(repents(&view, PRIEST), "{}", describe(&view));

    // The Priest under the Mage's attack: the Rogue is the one left alone.
    let mut scene = with_rogue(healer_left_alone(), 6.0);
    scene.set_target(MAGE, PRIEST);
    let view = scene.decide(PALADIN);
    assert!(repents(&view, ROGUE), "{}", describe(&view));

    // Two non-healers left alone: the nearer.
    const MAGE2: Who = "enemy mage";
    let mut scene = with_rogue(healer_left_alone(), 12.0)
        .unit(MAGE2, 2, Mage)
        .at(8.0, 0.0);
    scene.set_target(MAGE, PRIEST);
    let view = scene.decide(PALADIN);
    assert!(repents(&view, MAGE2), "{}", describe(&view));
}

/// Out of range or out of sight, the Priest is not repented: a pillar
/// between them is traced `LosBlocked`.
#[test]
fn repentance_needs_reach() {
    let far = healer_left_alone().place(PRIEST, 25.0, 0.0);
    let view = far.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::Repentance),
        "{}",
        describe(&view)
    );
    let hidden = healer_left_alone().obstacles(vec![pillar(8.0, 0.0, 2.0)]);
    let view = hidden.decide(PALADIN);
    assert!(
        matches!(
            view.rejected(AbilityType::Repentance),
            Some(RejectionReason::LosBlocked)
        ),
        "{}",
        describe(&view)
    );
}

/// It keeps the emergency Holy Shock's mana: one short of both costs, held;
/// with both, cast.
#[test]
fn repentance_keeps_the_emergency_heals_mana() {
    let both = cost(AbilityType::HolyShock) + cost(AbilityType::Repentance);
    let at = |mana: f32| {
        let mut scene = healer_left_alone();
        scene.combatant_mut(PALADIN).current_mana = mana;
        scene.decide(PALADIN)
    };
    let short = at(both - 1.0);
    assert!(
        precondition(&short, AbilityType::Repentance),
        "{both} - 1: {}",
        describe(&short)
    );
    let paid = at(both);
    assert!(repents(&paid, PRIEST), "{both}: {}", describe(&paid));
}

/// A silenced Paladin casts no Repentance (TBC 20066 is Silence-prevented);
/// one on its cooldown neither.
#[test]
fn a_silence_or_the_cooldown_stops_repentance() {
    let silenced = healer_left_alone().aura(PALADIN, AuraType::Silence, 5.0);
    let view = silenced.decide(PALADIN);
    assert_ne!(chose(&view), Some(AbilityType::Repentance));
    let cooling = healer_left_alone().cooldown(PALADIN, AbilityType::Repentance, 10.0);
    let view = cooling.decide(PALADIN);
    assert!(
        matches!(
            view.rejected(AbilityType::Repentance),
            Some(RejectionReason::OnCooldown { .. })
        ),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Crusader Strike: the Paladin's target, in melee, no teammate low
// ---------------------------------------------------------------------------

/// A healthy Paladin and Mage, the Paladin in melee of its target, an enemy
/// Warrior, with its Hammer of Justice spent.
fn in_melee() -> Scene {
    Scene::new()
        .unit(PALADIN, 1, Paladin)
        .aura_full(PALADIN, devotion())
        .targeting(WARRIOR)
        .cooldown(PALADIN, AbilityType::HammerOfJustice, 30.0)
        .unit(MAGE, 1, Mage)
        .at(-10.0, 0.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(2.0, 0.0)
        .targeting(PALADIN)
        .grant(PALADIN, AbilityType::CrusaderStrike)
}

fn strikes(view: &DecisionView) -> bool {
    view.chose() == Some((AbilityType::CrusaderStrike, Some(WARRIOR)))
}

/// The strike lands on the target in melee, for its 8 mana and a global
/// cooldown. Control: the Warrior out of melee — no strike.
#[test]
fn crusader_strike_hits_the_target_in_melee() {
    let mut scene = in_melee();
    let mana = scene.combatant_mut(PALADIN).current_mana;
    let view = scene.decide(PALADIN);
    assert!(strikes(&view), "{}", describe(&view));
    assert!(
        view.landed
            .iter()
            .any(|l| l.ability == AbilityType::CrusaderStrike && l.target == WARRIOR),
        "{}",
        describe(&view)
    );
    assert!(near(
        view.resource_after,
        mana - cost(AbilityType::CrusaderStrike)
    ));
    assert_eq!(view.global_cooldown_after, GCD);

    let apart = in_melee().place(WARRIOR, 6.0, 0.0);
    let view = apart.decide(PALADIN);
    assert!(!strikes(&view), "{}", describe(&view));
}

/// The heals come first: a teammate (or the Paladin itself) below half
/// health holds the strike; just above half, it goes.
#[test]
fn a_low_teammate_holds_crusader_strike() {
    // A hurt Paladin's bubble spent, so its own turn reaches the strike.
    let scene = |who: Who, health: f32| {
        let mut scene =
            in_melee()
                .health_of(who, health)
                .cooldown(PALADIN, AbilityType::DivineShield, 300.0);
        scene.set_target(WARRIOR, MAGE);
        scene
    };
    for who in [MAGE, PALADIN] {
        let view = scene(who, 0.45).decide(PALADIN);
        assert!(
            precondition(&view, AbilityType::CrusaderStrike),
            "{who}: {}",
            describe(&view)
        );
        let view = scene(who, 0.55).decide(PALADIN);
        assert!(strikes(&view), "{who}: {}", describe(&view));
    }
}

/// Not on a target immune to damage, nor one held in the team's crowd
/// control that any damage breaks.
#[test]
fn crusader_strike_spares_an_immune_or_crowd_controlled_target() {
    let immune = in_melee().aura(WARRIOR, AuraType::DamageImmunity, 8.0);
    let view = immune.decide(PALADIN);
    assert!(!strikes(&view), "{}", describe(&view));

    let scene = in_melee();
    let mage = scene.entity(MAGE);
    let sheeped = scene.aura_full(
        WARRIOR,
        Aura {
            effect_type: AuraType::Polymorph,
            duration: 8.0,
            magnitude: 1.0,
            break_on_damage_threshold: 0.0,
            ability_name: "Polymorph".into(),
            caster: Some(mage),
            ..Default::default()
        },
    );
    let view = sheeped.decide(PALADIN);
    assert!(!strikes(&view), "{}", describe(&view));
}

/// It keeps the emergency Holy Shock's mana: one short of both costs, held;
/// with both, struck.
#[test]
fn crusader_strike_keeps_the_emergency_heals_mana() {
    let both = cost(AbilityType::HolyShock) + cost(AbilityType::CrusaderStrike);
    let at = |mana: f32| {
        let mut scene = in_melee();
        scene.combatant_mut(PALADIN).current_mana = mana;
        scene.decide(PALADIN)
    };
    let short = at(both - 1.0);
    assert!(
        precondition(&short, AbilityType::CrusaderStrike),
        "{both} - 1: {}",
        describe(&short)
    );
    let paid = at(both);
    assert!(strikes(&paid), "{both}: {}", describe(&paid));
}

// ---------------------------------------------------------------------------
// The Art of War: an instant Flash of Light is not deferred by a walk
// ---------------------------------------------------------------------------

/// The Paladin walking to its Warrior, dying behind a pillar (the medic
/// walk), with its Mage hurt but healthy in sight: a Flash of Light with a
/// cast bar would root it short, so it is deferred.
fn medic_walk() -> Scene {
    const ALLY: Who = "warrior";
    Scene::new()
        .unit(PALADIN, 1, Paladin)
        .aura_full(PALADIN, devotion())
        .unit(MAGE, 1, Mage)
        .at(-10.0, 0.0)
        .health(0.7)
        .unit(ALLY, 1, Warrior)
        .at(20.0, 0.0)
        .health(0.3)
        .unit(WARRIOR, 2, Warrior)
        .at(22.0, 0.0)
        .targeting(ALLY)
        .obstacles(vec![pillar(10.0, 0.0, 2.0)])
}

fn art_of_war() -> Aura {
    Aura {
        effect_type: AuraType::NextCast,
        duration: 15.0,
        magnitude: 1.0,
        ability_name: "The Art of War".into(),
        next_cast: Some(NextCastModifier {
            applies_to: NextCastScope::Abilities(vec![AbilityType::FlashOfLight]),
            instant: true,
            free: true,
            crit_chance: 0.0,
            cast_time_reduction: 0.0,
            cost_reduction: 0.0,
            power: 0.0,
        }),
        dispel_type: DispelType::Magic,
        ..Default::default()
    }
}

#[test]
fn an_instant_flash_of_light_is_not_deferred_by_a_walk() {
    let view = medic_walk().decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::FlashOfLight),
        "the walk defers a cast bar: {}",
        describe(&view)
    );
    let primed = medic_walk().aura_full(PALADIN, art_of_war());
    let view = primed.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FlashOfLight, Some(MAGE))),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// In a match
// ---------------------------------------------------------------------------

const PAL: Unit = Unit::new(1, 0);
const MAGE_UNIT: Unit = Unit::new(1, 1);
const FOE: Unit = Unit::new(2, 0);
const FOE_HEALER: Unit = Unit::new(2, 1);

fn lines<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

/// A Retribution Paladin (two-hander, `build`) and a Mage against a Warrior
/// on the Paladin and a Priest left alone 15 yards off, the Mage hurt so a
/// primed Flash of Light has a teammate to heal. `paladin` sets what the
/// Paladin's procs need by construction, whatever the seed: its crit chance,
/// and its swing (a 12.5s swing makes The Art of War's 4.8 a minute a sure
/// proc on each, and a ready timer swings it at the gates).
fn ret_match(
    build: Build,
    paladin: impl Fn(&mut Combatant) + Send + Sync + 'static,
) -> common::staged::Played {
    let reaper: HashMap<String, String> =
        [("MainHand".to_string(), "ArcaniteReaper".to_string())].into();
    let config = HeadlessMatchConfig {
        team1: vec!["Paladin".into(), "Mage".into()],
        team2: vec!["Warrior".into(), "Priest".into()],
        random_seed: Some(5),
        max_duration_secs: 14.0,
        team1_builds: vec![build],
        team1_equipment: vec![reaper],
        ..Default::default()
    };
    play(
        config,
        Stage::new()
            .place(PAL, 0.0, 0.0)
            .place(MAGE_UNIT, -10.0, 0.0)
            .health(MAGE_UNIT, 0.8)
            .place(FOE, 2.0, 0.0)
            .place(FOE_HEALER, 15.0, 0.0)
            .target(PAL, FOE)
            .target(MAGE_UNIT, FOE)
            .target(FOE, PAL)
            .target(FOE_HEALER, FOE)
            .cooldown(PAL, AbilityType::HammerOfJustice, 60.0)
            .combatant(PAL, paladin)
            // Too much health for the fight to end, or to turn into heals.
            .combatant(FOE, |c| {
                c.max_health = 20_000.0;
                c.current_health = 20_000.0;
            }),
    )
}

/// Every swing a crit, one swing at the gates and the next 12.5s on.
fn sure_crit_swing(c: &mut Combatant) {
    c.crit_chance = 1.0;
    c.weapon_speed = 12.5;
    c.attack_timer = 12.5;
}

/// No swing in the match, and every strike a crit.
fn no_swing(c: &mut Combatant) {
    c.crit_chance = 1.0;
    c.weapon_speed = 100.0;
    c.attack_timer = 0.0;
}

fn paladin_auras(played: &common::staged::Played, aura: AuraType) -> bool {
    played.frames.iter().any(|f| {
        f.combatants
            .values()
            .any(|c| !c.is_pet && c.team == 1 && c.slot == 0 && c.aura_types.contains(&aura))
    })
}

/// Repentance goes on the Priest once (its 30s cooldown); Crusader Strike
/// lands on the Warrior, at most once in any six seconds. Control: the
/// empty build casts neither.
#[test]
fn repentance_and_crusader_strike_act_in_a_match() {
    let log = ret_match(Build::of(&deep_build()), no_swing).log;
    assert_eq!(
        lines(&log, "Paladin #1 casts Repentance on Team 2 Priest").len(),
        1,
        "{log}"
    );
    assert!(
        log.contains("Repentance incapacitates Team 2 Priest"),
        "{log}"
    );
    let strikes: Vec<f32> = lines(&log, "Paladin #1 uses Crusader Strike on Team 2 Warrior")
        .into_iter()
        .map(at)
        .collect();
    assert!(strikes.len() >= 2, "{log}");
    assert!(
        strikes.windows(2).all(|w| w[1] - w[0] >= 6.0 - 1e-3),
        "{strikes:?}\n{log}"
    );
    assert!(
        log.contains("Crusader Strike CRITS Team 2 Warrior"),
        "{log}"
    );

    let control = ret_match(Build::default(), no_swing).log;
    for line in ["Repentance", "Crusader Strike"] {
        assert!(!control.contains(line), "{line}: {control}");
    }
}

/// Vengeance off a Crusader Strike crit: no swing lands in the match, so the
/// strike is what procs it. Control: the same strikes, never critting —
/// Vengeance procs on a crit only.
#[test]
fn vengeance_procs_on_a_crusader_strike_crit() {
    let played = ret_match(Build::of(&deep_build()), no_swing);
    assert!(
        !played.log.contains("Paladin #1's Auto Attack"),
        "{}",
        played.log
    );
    assert!(
        played.log.contains("Vengeance procs Vengeance"),
        "{}",
        played.log
    );
    assert!(paladin_auras(&played, AuraType::DamageDealtIncrease));

    let control = ret_match(Build::of(&deep_build()), |c| {
        no_swing(c);
        c.crit_chance = 0.0;
    });
    assert!(
        control.log.contains("Crusader Strike hits"),
        "{}",
        control.log
    );
    assert!(!control.log.contains("procs Vengeance"), "{}", control.log);
}

/// Vengeance off a swing's crit, with no Crusader Strike in the build.
/// Control: a swing that does not crit procs nothing.
#[test]
fn vengeance_procs_on_a_swing_crit() {
    let build = || Build::of(&[(Conviction, 2), (Vengeance, 2)]);
    let played = ret_match(build(), sure_crit_swing);
    assert!(
        played.log.contains("Paladin #1's Auto Attack CRITS"),
        "{}",
        played.log
    );
    assert!(
        played.log.contains("Vengeance procs Vengeance"),
        "{}",
        played.log
    );
    assert!(paladin_auras(&played, AuraType::DamageDealtIncrease));

    let control = ret_match(build(), |c| {
        sure_crit_swing(c);
        c.crit_chance = 0.0;
    });
    assert!(
        control.log.contains("Paladin #1's Auto Attack hits"),
        "{}",
        control.log
    );
    assert!(!control.log.contains("procs Vengeance"), "{}", control.log);
}

/// The Art of War off a swing: the next Flash of Light is instant and free.
/// Control: a Paladin whose swing never comes procs nothing, and its Flash of
/// Light takes its cast bar.
#[test]
fn the_art_of_war_makes_a_flash_of_light_instant_and_free() {
    let build = || {
        Build::of(&[
            (Conviction, 2),
            (TwoHandedWeaponSpecialization, 1),
            (TheArtOfWar, 1),
        ])
    };
    let played = ret_match(build(), |c| {
        sure_crit_swing(c);
        c.crit_chance = 0.0;
    });
    let log = &played.log;
    assert!(log.contains("The Art of War procs The Art of War"), "{log}");
    assert!(
        log.contains("The Art of War is spent on Flash of Light"),
        "{log}"
    );
    // Instant: the heal lands on the frame it is cast, with no cast bar.
    let spent = at(lines(log, "The Art of War is spent on Flash of Light")[0]);
    let cast = lines(log, "Paladin #1 begins casting Flash of Light")
        .into_iter()
        .map(at)
        .find(|t| (t - spent).abs() < 0.05)
        .unwrap_or_else(|| panic!("no Flash of Light at {spent}:\n{log}"));
    let healed = lines(log, "Paladin #1's Flash of Light")
        .into_iter()
        .map(at)
        .find(|t| *t >= cast - 1e-3)
        .unwrap_or_else(|| panic!("the Flash of Light never landed:\n{log}"));
    assert!(healed - cast < 0.1, "{cast} -> {healed}: a cast bar\n{log}");
    // Free: no frame near it takes the Flash of Light's cost.
    let drop = largest_mana_drop_near(&played, cast);
    assert!(
        drop < cost(AbilityType::FlashOfLight) / 2.0,
        "{drop} mana left the Paladin at {cast}\n{log}"
    );

    let control = ret_match(build(), |c| {
        no_swing(c);
        c.crit_chance = 0.0;
    });
    assert!(
        !control.log.contains("procs The Art of War"),
        "{}",
        control.log
    );
    assert!(
        control
            .log
            .contains("Paladin #1 begins casting Flash of Light"),
        "{}",
        control.log
    );
}

/// A log line's time: "[ 46.52s] ..." is 46.52.
fn at(line: &str) -> f32 {
    line.trim_start_matches('[')
        .split('s')
        .next()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or_else(|| panic!("no time on {line:?}"))
}

/// The Paladin's largest one-frame mana drop within five frames of `t` (the
/// app clock, as the log stamps it).
fn largest_mana_drop_near(played: &common::staged::Played, t: f32) -> f32 {
    let mana = |f: &arenasim::headless::FrameObservation| {
        f.combatants
            .values()
            .find(|c| !c.is_pet && c.team == 1 && c.slot == 0)
            .map(|c| c.current_mana)
            .expect("the Paladin is observed")
    };
    let frames = &played.frames;
    let near_t: Vec<usize> = (1..frames.len())
        .filter(|&i| (frames[i].sim_time - t).abs() < 5.0 / 60.0)
        .collect();
    assert!(!near_t.is_empty(), "no frame near {t}");
    near_t
        .into_iter()
        .map(|i| mana(&frames[i - 1]) - mana(&frames[i]))
        .fold(f32::MIN, f32::max)
}

/// The tree's prose (`talent_text`): every Retribution node reads, a rank a
/// line.
#[test]
fn the_retribution_tree_reads() {
    use arenasim::states::talent_text::rank_lines;
    let talents = TalentDefinitions::shipped();
    for (id, talent) in talents
        .talents_for_class(Paladin)
        .filter(|(_, t)| t.tree == TreeId::Retribution)
    {
        let lines = rank_lines(talent, defs());
        eprintln!("{id:?}: {lines:?}");
        assert_eq!(lines.len(), usize::from(talent.max_rank), "{id:?}");
        assert!(lines.iter().all(|l| !l.is_empty()), "{id:?}");
    }
}

/// Vengeance's two procs land one aura, so the encyclopedia's Buffs &
/// Debuffs has one Vengeance entry, naming both events that land it.
#[test]
fn vengeance_is_one_encyclopedia_entry_naming_both_events() {
    use arenasim::states::encyclopedia::auras::catalog;
    let entries: Vec<_> = catalog(defs(), items())
        .into_iter()
        .filter(|e| e.name == "Vengeance")
        .collect();
    assert_eq!(entries.len(), 1);
    let text = entries[0].provenance.clone().unwrap_or_default();
    assert!(text.contains("melee critical strike"), "{text}");
    assert!(text.contains("critical hit of Crusader Strike"), "{text}");
}

/// Repentance is mirrored into the frame's snapshot as it lands, as Hammer of
/// Justice is: the Mage, deciding after the Paladin on the gates' frame with
/// the Priest as its crowd-control target, sees the Priest held and does not
/// start a Polymorph onto it. Control: with no Repentance, it does.
#[test]
fn a_teammate_deciding_later_in_the_frame_sees_the_repentance() {
    let gates_polymorph = |build: Build| {
        let config = HeadlessMatchConfig {
            team1: vec!["Paladin".into(), "Mage".into()],
            team2: vec!["Warrior".into(), "Priest".into()],
            random_seed: Some(5),
            max_duration_secs: 2.0,
            team1_builds: vec![build],
            team1_cc_target: Some(1),
            ..Default::default()
        };
        let played = play(
            config,
            Stage::new()
                .place(PAL, 0.0, 0.0)
                .place(MAGE_UNIT, -10.0, 0.0)
                .place(FOE, 2.0, 0.0)
                .place(FOE_HEALER, 15.0, 0.0)
                .target(PAL, FOE)
                .target(MAGE_UNIT, FOE)
                .target(FOE, PAL)
                .target(FOE_HEALER, FOE)
                .cooldown(PAL, AbilityType::HammerOfJustice, 60.0),
        );
        let gates = lines(&played.log, "Gates open!")
            .first()
            .map(|l| at(l))
            .expect("the gates open");
        let repented = lines(&played.log, "Paladin #1 casts Repentance")
            .iter()
            .any(|l| (at(l) - gates).abs() < 0.01);
        let polymorphed = lines(&played.log, "Mage #2 begins casting Polymorph")
            .iter()
            .any(|l| (at(l) - gates).abs() < 0.01);
        (repented, polymorphed, played.log)
    };
    let (repented, polymorphed, log) = gates_polymorph(Build::of(&deep_build()));
    assert!(repented && !polymorphed, "{log}");
    let (repented, polymorphed, log) = gates_polymorph(Build::default());
    assert!(!repented && polymorphed, "{log}");
}

/// Each cast's own state is asked first, so a refusal names it ahead of the
/// target or the reserve: Repentance on its cooldown with no enemy left alone
/// is `OnCooldown`, and so is Crusader Strike on its cooldown with a teammate
/// low.
#[test]
fn the_casters_own_state_is_reported_first() {
    let mut scene = healer_left_alone().cooldown(PALADIN, AbilityType::Repentance, 10.0);
    scene.set_target(MAGE, PRIEST);
    let view = scene.decide(PALADIN);
    assert!(
        matches!(
            view.rejected(AbilityType::Repentance),
            Some(RejectionReason::OnCooldown { .. })
        ),
        "{}",
        describe(&view)
    );
    let mut scene =
        in_melee()
            .health_of(MAGE, 0.45)
            .cooldown(PALADIN, AbilityType::CrusaderStrike, 3.0);
    scene.set_target(WARRIOR, MAGE);
    let view = scene.decide(PALADIN);
    assert!(
        matches!(
            view.rejected(AbilityType::CrusaderStrike),
            Some(RejectionReason::OnCooldown { .. })
        ),
        "{}",
        describe(&view)
    );
}
