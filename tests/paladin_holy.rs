//! The Paladin's Holy tree (AS-420): its nodes as the card writes them, its
//! two granted abilities' AI rules as scene pairs, Aura Mastery's immunity at
//! each site that honours it, and each node acting in a played match.
//!
//! Each scene is a PAIR: the situation its rule answers and the control with
//! that one condition removed, so a rule deleted or mis-gated fails here.

mod common;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{
    self, Hunter, Mage, Paladin, Priest, Rogue, Shaman, Warlock, Warrior,
};
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::ability_config::{AbilityDefinitions, Prevention};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, CastingState, Combatant, DispelType,
    GameRng, InterruptPending,
};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::next_cast::{NextCastModifier, NextCastScope};
use arenasim::states::play_match::systems::{apply_pending_auras, process_interrupts};
use arenasim::states::play_match::talent_procs::{
    ProcChance, ProcEffect, ProcRankScale, TalentProc, TalentTrigger,
};
use arenasim::states::play_match::talents::{
    AbilityField, Build, PatchOp, PatchTarget, TalentDefinitions, TalentEffect, TalentId, TreeId,
};
use arenasim::states::play_match::AbilityType;
use common::staged::play;
use TalentId::*;

const PALADIN: Who = "paladin";
const MAGE: Who = "mage";
const ENEMY: Who = "enemy";

fn defs() -> &'static AbilityDefinitions {
    AbilityDefinitions::shipped()
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

/// The aura `ability` lands, as its cast lands it.
fn aura_of(ability: AbilityType) -> Aura {
    let def = defs().get_unchecked(&ability);
    let effect = def
        .applies_aura
        .as_ref()
        .expect("the ability applies an aura");
    Aura {
        effect_type: effect.aura_type,
        duration: effect.duration,
        magnitude: effect.magnitude,
        ability_name: def.name.clone(),
        next_cast: effect.next_cast.clone(),
        dispel_type: effect.dispel_type,
        ..Default::default()
    }
}

/// A Silence, as the Unstable Affliction backlash lands it.
fn silence() -> Aura {
    Aura {
        effect_type: AuraType::Silence,
        duration: 5.0,
        magnitude: 1.0,
        ability_name: "Unstable Affliction".into(),
        break_on_damage_threshold: -1.0,
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
fn the_holy_tree_is_as_specified() {
    let talents = TalentDefinitions::shipped();
    let rows: Vec<(TalentId, u8, u8, u8, Option<f32>)> = talents
        .talents_for_class(Paladin)
        .filter(|(_, t)| t.tree == TreeId::Holy)
        .map(|(id, t)| (id, t.tier, t.column, t.max_rank, t.budget))
        .collect();
    assert_eq!(
        rows,
        vec![
            (ImprovedHolyShock, 1, 0, 2, Some(11.0)),
            (HealingLight, 1, 2, 2, Some(11.0)),
            (InfusionOfLight, 2, 0, 2, Some(11.0)),
            (DivineFavor, 2, 1, 1, Some(11.0)),
            (PurifyingPower, 2, 2, 2, Some(11.0)),
            (LightsGrace, 3, 0, 1, Some(11.0)),
            (AuraMastery, 4, 1, 1, Some(22.0)),
        ]
    );
    // Holy is the Paladin's first tree.
    assert_eq!(
        talents
            .trees(Paladin)
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        vec![TreeId::Holy, TreeId::Protection]
    );
    assert_eq!(
        talents.get(AuraMastery).unwrap().requires,
        Some((DivineFavor, 1))
    );
    let patch = |on: AbilityType, field: AbilityField, op: PatchOp| TalentEffect::Patch {
        on: PatchTarget::Ability(on),
        field,
        op,
    };
    assert_eq!(
        talents.get(ImprovedHolyShock).unwrap().per_rank,
        vec![patch(
            AbilityType::HolyShock,
            AbilityField::Cooldown,
            PatchOp::Add(-2.0)
        )]
    );
    assert_eq!(
        talents.get(HealingLight).unwrap().per_rank,
        vec![
            patch(
                AbilityType::FlashOfLight,
                AbilityField::Healing,
                PatchOp::Mul(1.05)
            ),
            patch(
                AbilityType::HolyLight,
                AbilityField::Healing,
                PatchOp::Mul(1.05)
            ),
        ]
    );
    assert_eq!(
        talents.get(PurifyingPower).unwrap().per_rank,
        vec![patch(
            AbilityType::PaladinCleanse,
            AbilityField::ManaCost,
            PatchOp::Mul(0.80)
        )]
    );
    for (talent, ability) in [
        (DivineFavor, AbilityType::DivineFavor),
        (AuraMastery, AbilityType::AuraMastery),
    ] {
        assert_eq!(
            talents.get(talent).unwrap().per_rank,
            vec![TalentEffect::Grant(ability)]
        );
    }
    let primes = |ability: AbilityType, crit: f32, faster: f32, dispel: DispelType, name: &str| {
        vec![TalentEffect::Proc(TalentProc {
            trigger: TalentTrigger::AbilityUsed,
            abilities: vec![AbilityType::HolyShock],
            crit_only: false,
            chance: ProcChance::Chance(1.0),
            ranks_add: ProcRankScale::Effect,
            effect: ProcEffect::NextCast {
                modifier: NextCastModifier {
                    applies_to: NextCastScope::Abilities(vec![ability]),
                    instant: false,
                    free: false,
                    crit_chance: crit,
                    cast_time_reduction: faster,
                    cost_reduction: 0.0,
                    power: 0.0,
                },
                duration: 15.0,
                dispel_type: dispel,
                name: Some(name.into()),
            },
            internal_cooldown: 0.0,
        })]
    };
    assert_eq!(
        talents.get(InfusionOfLight).unwrap().per_rank,
        primes(
            AbilityType::FlashOfLight,
            0.25,
            0.0,
            DispelType::Untyped,
            "Infusion of Light"
        )
    );
    assert_eq!(
        talents.get(LightsGrace).unwrap().per_rank,
        primes(
            AbilityType::HolyLight,
            0.0,
            0.5,
            DispelType::Auto,
            "Light's Grace"
        )
    );
}

/// Divine Favor and Aura Mastery as the card and the client write them.
#[test]
fn the_holy_abilities_are_as_specified() {
    let favor = defs().get_unchecked(&AbilityType::DivineFavor);
    assert!(favor.talent);
    assert_eq!(favor.class, Paladin);
    assert_eq!(
        (favor.cast_time, favor.mana_cost, favor.cooldown),
        (0.0, 5.0, 45.0)
    );
    assert_eq!(favor.prevention, Prevention::Silence);
    assert_eq!(favor.spell_school, SpellSchool::Holy);
    let aura = favor.applies_aura.as_ref().unwrap();
    assert_eq!(aura.aura_type, AuraType::NextCast);
    assert_eq!(
        aura.next_cast,
        Some(NextCastModifier {
            applies_to: NextCastScope::Abilities(vec![
                AbilityType::FlashOfLight,
                AbilityType::HolyLight,
                AbilityType::HolyShock,
            ]),
            instant: false,
            free: false,
            crit_chance: 1.0,
            cast_time_reduction: 0.0,
            cost_reduction: 0.0,
            power: 0.0,
        })
    );

    let mastery = defs().get_unchecked(&AbilityType::AuraMastery);
    assert!(mastery.talent);
    assert_eq!(mastery.class, Paladin);
    assert_eq!(
        (mastery.cast_time, mastery.mana_cost, mastery.cooldown),
        (0.0, 0.0, 45.0)
    );
    assert_eq!(mastery.prevention, Prevention::None);
    assert!(!mastery.silence_stops());
    assert_eq!(mastery.spell_school, SpellSchool::Physical);
    let aura = mastery.applies_aura.as_ref().unwrap();
    assert_eq!(
        (aura.aura_type, aura.duration, aura.dispel_type),
        (AuraType::SilenceInterruptImmunity, 6.0, DispelType::Untyped)
    );
    // No dispel type: nothing purges it, whatever its mechanic.
    assert!(!aura_of(AbilityType::AuraMastery).can_be_purged());
}

/// An empty build is the base Paladin; a Holy build reaches every number
/// its nodes name and no other.
#[test]
fn a_holy_build_reaches_every_number_it_names() {
    let talents = TalentDefinitions::shipped();
    let resolve =
        |build: &[(TalentId, u8)]| talents.resolve(defs(), Paladin, &Build::of(build)).unwrap();
    let base = talents.resolve(defs(), Paladin, &Build::default()).unwrap();
    let get = |r: &arenasim::states::play_match::talents::ResolvedBuild, a: AbilityType| {
        r.book().get_unchecked(&a).clone()
    };
    for granted in [AbilityType::DivineFavor, AbilityType::AuraMastery] {
        assert!(base.book().get(&granted).is_none(), "{granted:?}");
    }
    assert!(base.procs().is_empty());

    // Improved Holy Shock: 15 -> 13 -> 11.
    let shock = |rank: u8| {
        get(
            &resolve(&[(ImprovedHolyShock, rank)]),
            AbilityType::HolyShock,
        )
    };
    assert_eq!(
        [
            get(&base, AbilityType::HolyShock).cooldown,
            shock(1).cooldown,
            shock(2).cooldown
        ],
        [15.0, 13.0, 11.0]
    );

    // Healing Light: Flash of Light and Holy Light, +5% a rank, base and
    // coefficient alike; Holy Shock's heal untouched.
    for rank in [1u8, 2] {
        let r = resolve(&[(HealingLight, rank)]);
        let scale = 1.0 + 0.05 * f32::from(rank);
        for heal in [AbilityType::FlashOfLight, AbilityType::HolyLight] {
            let (was, is) = (get(&base, heal), get(&r, heal));
            assert!(
                near(is.healing_base_min, was.healing_base_min * scale),
                "{heal:?}"
            );
            assert!(
                near(is.healing_base_max, was.healing_base_max * scale),
                "{heal:?}"
            );
            assert!(
                near(is.healing_coefficient, was.healing_coefficient * scale),
                "{heal:?}"
            );
        }
        let (was, is) = (
            get(&base, AbilityType::HolyShock),
            get(&r, AbilityType::HolyShock),
        );
        assert_eq!(
            (
                is.healing_base_min,
                is.healing_base_max,
                is.healing_coefficient
            ),
            (
                was.healing_base_min,
                was.healing_base_max,
                was.healing_coefficient
            )
        );
    }

    // Purifying Power: Cleanse 15 -> 12 -> 9.
    let cleanse = |rank: u8| {
        get(
            &resolve(&[(ImprovedHolyShock, 2), (PurifyingPower, rank)]),
            AbilityType::PaladinCleanse,
        )
        .mana_cost
    };
    assert_eq!(get(&base, AbilityType::PaladinCleanse).mana_cost, 15.0);
    assert!(near(cleanse(1), 12.0) && near(cleanse(2), 9.0));

    // Infusion of Light: its ranks add to the crit chance it primes.
    let crit = |rank: u8| {
        let r = resolve(&[(ImprovedHolyShock, 2), (InfusionOfLight, rank)]);
        assert_eq!(r.procs().len(), 1);
        match &r.procs()[0].proc.effect {
            ProcEffect::NextCast { modifier, .. } => modifier.crit_chance,
            other => panic!("{other:?}"),
        }
    };
    assert!(near(crit(1), 0.25) && near(crit(2), 0.50));

    // A deep build: both grants and both procs.
    let deep = resolve(&[
        (ImprovedHolyShock, 2),
        (InfusionOfLight, 1),
        (DivineFavor, 1),
        (LightsGrace, 1),
        (AuraMastery, 1),
    ]);
    for granted in [AbilityType::DivineFavor, AbilityType::AuraMastery] {
        assert!(deep.book().get(&granted).is_some(), "{granted:?}");
    }
    let names: Vec<&str> = deep.procs().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Infusion of Light", "Light's Grace"]);
}

// ---------------------------------------------------------------------------
// Divine Favor: ahead of the emergency Holy Shock heal
// ---------------------------------------------------------------------------

/// A healthy Paladin, its Mage 15yd off at `health` with an enemy Warrior on
/// it (out of Hammer of Justice range).
fn mage_at(health: f32) -> Scene {
    Scene::new()
        .unit(PALADIN, 1, Paladin)
        .aura_full(PALADIN, devotion())
        .unit(MAGE, 1, Mage)
        .at(15.0, 0.0)
        .health(health)
        .unit(ENEMY, 2, Warrior)
        .at(17.0, 0.0)
        .targeting(MAGE)
}

#[test]
fn divine_favor_goes_ahead_of_the_emergency_holy_shock() {
    let scene = mage_at(0.3).grant(PALADIN, AbilityType::DivineFavor);
    let view = scene.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::DivineFavor, None)),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.auras_queued,
        vec![(PALADIN, AuraType::NextCast)],
        "{}",
        describe(&view)
    );

    // Control: the Mage is not in an emergency — Divine Favor is not asked.
    let calm = mage_at(0.9).grant(PALADIN, AbilityType::DivineFavor);
    let view = calm.decide(PALADIN);
    assert_ne!(
        view.chose().map(|(a, _)| a),
        Some(AbilityType::DivineFavor),
        "{}",
        describe(&view)
    );
}

/// Only when the heal would go out now: Holy Shock on cooldown, or the Mage
/// behind a pillar, holds it.
#[test]
fn divine_favor_waits_for_a_heal_that_would_go_out() {
    let cooling = mage_at(0.3)
        .grant(PALADIN, AbilityType::DivineFavor)
        .cooldown(PALADIN, AbilityType::HolyShock, 5.0);
    let view = cooling.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::DivineFavor),
        "{}",
        describe(&view)
    );

    let hidden = mage_at(0.3)
        .grant(PALADIN, AbilityType::DivineFavor)
        .obstacles(vec![pillar(7.5, 0.0, 2.0)]);
    let view = hidden.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::DivineFavor),
        "{}",
        describe(&view)
    );
}

/// Primed already, the Paladin goes straight to the heal, which spends it.
#[test]
fn a_primed_divine_favor_is_spent_by_the_holy_shock() {
    let primed = mage_at(0.3)
        .grant(PALADIN, AbilityType::DivineFavor)
        .aura_full(PALADIN, aura_of(AbilityType::DivineFavor));
    let view = primed.decide(PALADIN);
    assert!(
        matches!(
            view.rejected(AbilityType::DivineFavor),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.chose(),
        Some((AbilityType::HolyShock, Some(MAGE))),
        "{}",
        describe(&view)
    );
}

/// A silenced Paladin casts no Divine Favor (TBC 20216 is Silence-prevented
/// and costs mana); control: unsilenced, it does.
#[test]
fn a_silence_stops_divine_favor() {
    let silenced = mage_at(0.3)
        .grant(PALADIN, AbilityType::DivineFavor)
        .aura_full(PALADIN, silence());
    let view = silenced.decide(PALADIN);
    assert_ne!(
        view.chose().map(|(a, _)| a),
        Some(AbilityType::DivineFavor),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Aura Mastery: an enemy about to interrupt a casting teammate
// ---------------------------------------------------------------------------

/// A healthy Paladin, its Mage 10yd off casting `cast`, and an enemy of
/// `class` `gap` yards beyond the Mage, not stealthed.
fn threatened(cast: Option<AbilityType>, class: CharacterClass, gap: f32) -> Scene {
    let mut scene = Scene::new()
        .unit(PALADIN, 1, Paladin)
        .aura_full(PALADIN, devotion())
        .unit(MAGE, 1, Mage)
        .at(10.0, 0.0)
        .unit(ENEMY, 2, class)
        .at(10.0 + gap, 0.0)
        .mana(1.0)
        .targeting(MAGE)
        .grant(PALADIN, AbilityType::AuraMastery);
    if let Some(cast) = cast {
        scene = scene.casting(MAGE, cast);
    }
    scene.combatant_mut(ENEMY).stealthed = false;
    scene
}

fn masters(view: &DecisionView) -> bool {
    view.chose() == Some((AbilityType::AuraMastery, Some(MAGE)))
}

#[test]
fn aura_mastery_answers_an_interrupt_about_to_land_on_a_casting_teammate() {
    let view = threatened(Some(AbilityType::Frostbolt), Rogue, 2.0).decide(PALADIN);
    assert!(masters(&view), "{}", describe(&view));
    // Every teammate gains the immunity, the Paladin too.
    let mut queued = view.auras_queued.clone();
    queued.sort_by_key(|(who, _)| *who);
    assert_eq!(
        queued,
        vec![
            (MAGE, AuraType::SilenceInterruptImmunity),
            (PALADIN, AuraType::SilenceInterruptImmunity),
        ],
        "{}",
        describe(&view)
    );

    // Control: the Mage is not casting.
    let view = threatened(None, Rogue, 2.0).decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::AuraMastery),
        "{}",
        describe(&view)
    );
}

/// "About to": the Kick ready within the horizon (1.5s) counts, one further
/// off does not.
#[test]
fn aura_mastery_waits_for_an_interrupt_that_is_ready() {
    for (cooldown, fires) in [(1.0, true), (2.0, false), (12.0, false)] {
        let scene = threatened(Some(AbilityType::Frostbolt), Rogue, 2.0).cooldown(
            ENEMY,
            AbilityType::Kick,
            cooldown,
        );
        let view = scene.decide(PALADIN);
        assert_eq!(
            masters(&view),
            fires,
            "Kick in {cooldown}s: {}",
            describe(&view)
        );
    }
}

/// The interrupt's reach (Kick: 2.5yd) plus the 5yd margin, in the
/// interrupter's sight.
#[test]
fn aura_mastery_answers_only_an_interrupter_in_reach_and_sight() {
    for (gap, fires) in [(7.0, true), (8.5, false)] {
        let view = threatened(Some(AbilityType::Frostbolt), Rogue, gap).decide(PALADIN);
        assert_eq!(masters(&view), fires, "{gap}yd: {}", describe(&view));
    }
    // A pillar between the Rogue and the Mage (both in the Paladin's sight).
    let hidden = threatened(Some(AbilityType::Frostbolt), Rogue, 6.0)
        .obstacles(vec![pillar(13.0, 0.0, 1.0)]);
    let view = hidden.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::AuraMastery),
        "{}",
        describe(&view)
    );
}

/// The enemy needs an interrupt (an enemy Mage has none), and the cast must
/// be one an interrupt takes: a channel is, Aimed Shot is not.
#[test]
fn aura_mastery_answers_an_interruptible_cast_and_a_real_interrupter() {
    let view = threatened(Some(AbilityType::Frostbolt), Mage, 2.0).decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::AuraMastery),
        "no interrupter: {}",
        describe(&view)
    );

    let channel = Scene::new()
        .unit(PALADIN, 1, Paladin)
        .aura_full(PALADIN, devotion())
        .unit("warlock", 1, Warlock)
        .at(10.0, 0.0)
        .casting("warlock", AbilityType::DrainLife)
        .unit(ENEMY, 2, Rogue)
        .at(12.0, 0.0)
        .grant(PALADIN, AbilityType::AuraMastery);
    let mut channel = channel;
    channel.combatant_mut(ENEMY).stealthed = false;
    let view = channel.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::AuraMastery, Some("warlock"))),
        "a channel: {}",
        describe(&view)
    );

    let mut aimed = Scene::new()
        .unit(PALADIN, 1, Paladin)
        .aura_full(PALADIN, devotion())
        .unit("hunter", 1, Hunter)
        .at(10.0, 0.0)
        .casting("hunter", AbilityType::AimedShot)
        .unit(ENEMY, 2, Rogue)
        .at(12.0, 0.0)
        .grant(PALADIN, AbilityType::AuraMastery);
    aimed.combatant_mut(ENEMY).stealthed = false;
    let view = aimed.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::AuraMastery),
        "Aimed Shot: {}",
        describe(&view)
    );
}

/// A teammate already immune is not threatened, so no second Aura Mastery.
#[test]
fn aura_mastery_is_not_cast_over_itself() {
    let view = threatened(Some(AbilityType::Frostbolt), Rogue, 2.0)
        .aura_full(MAGE, aura_of(AbilityType::AuraMastery))
        .decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::AuraMastery),
        "{}",
        describe(&view)
    );
}

/// The immunity reaches teammates in range only: a Priest 150yd off is left
/// out (control: the same Priest beside the Paladin).
#[test]
fn aura_mastery_reaches_teammates_in_its_range() {
    for (x, reached) in [(150.0, false), (-5.0, true)] {
        let view = threatened(Some(AbilityType::Frostbolt), Rogue, 2.0)
            .unit("priest", 1, Priest)
            .at(x, 0.0)
            .decide(PALADIN);
        assert!(masters(&view), "{}", describe(&view));
        assert_eq!(
            view.auras_queued
                .contains(&("priest", AuraType::SilenceInterruptImmunity)),
            reached,
            "{x}: {:?}",
            view.auras_queued
        );
    }
}

/// A dead teammate gains nothing (control: alive, it does).
#[test]
fn aura_mastery_skips_a_dead_teammate() {
    for (health, reached) in [(0.0, false), (1.0, true)] {
        let mut scene = threatened(Some(AbilityType::Frostbolt), Rogue, 2.0)
            .unit("priest", 1, Priest)
            .at(-5.0, 0.0);
        scene.set_health("priest", health);
        let view = scene.decide(PALADIN);
        assert!(masters(&view), "{}", describe(&view));
        assert_eq!(
            view.auras_queued
                .contains(&("priest", AuraType::SilenceInterruptImmunity)),
            reached,
            "{health}: {:?}",
            view.auras_queued
        );
    }
}

/// Only the Paladin's own teammates' casts count: an ENEMY Mage casting
/// beside an enemy Rogue is no reason to raise it.
#[test]
fn an_enemy_cast_is_not_a_teammates() {
    let mut scene = Scene::new()
        .unit(PALADIN, 1, Paladin)
        .aura_full(PALADIN, devotion())
        .unit(MAGE, 1, Mage)
        .at(-10.0, 0.0)
        .unit("enemy mage", 2, Mage)
        .at(10.0, 0.0)
        .casting("enemy mage", AbilityType::Frostbolt)
        .unit(ENEMY, 2, Rogue)
        .at(12.0, 0.0)
        .grant(PALADIN, AbilityType::AuraMastery);
    scene.combatant_mut(ENEMY).stealthed = false;
    let view = scene.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::AuraMastery),
        "{}",
        describe(&view)
    );
}

/// Free and stopped by no silence: a silenced Paladin still casts it.
#[test]
fn a_silenced_paladin_still_casts_aura_mastery() {
    let view = threatened(Some(AbilityType::Frostbolt), Rogue, 2.0)
        .aura_full(PALADIN, silence())
        .decide(PALADIN);
    assert!(masters(&view), "{}", describe(&view));
}

// ---------------------------------------------------------------------------
// The immunity, at every site that honours it
// ---------------------------------------------------------------------------

/// A Rogue whose kill target, a Mage mid-Frostbolt, is immune to interrupts
/// spends no Kick on it; control: without the immunity it kicks.
#[test]
fn a_kick_is_not_spent_on_an_immune_caster() {
    let scene = |immune: bool| {
        let mut scene = Scene::new()
            .unit("rogue", 1, Rogue)
            .targeting(MAGE)
            .unit(MAGE, 2, Mage)
            .at(2.0, 0.0)
            .casting(MAGE, AbilityType::Frostbolt);
        scene.combatant_mut("rogue").stealthed = false;
        if immune {
            scene = scene.aura_full(MAGE, aura_of(AbilityType::AuraMastery));
        }
        scene
    };
    assert!(scene(true).interrupt("rogue").interrupts_queued.is_empty());
    assert_eq!(
        scene(false).interrupt("rogue").interrupts_queued,
        vec![(AbilityType::Kick, MAGE)]
    );
}

/// A Shaman with two casters in reach passes the immune one over for the
/// other; control: without the immunity it takes the nearer.
#[test]
fn wind_shear_passes_an_immune_caster_over() {
    let scene = |immune: bool| {
        let mut scene = Scene::new()
            .unit("shaman", 1, Shaman)
            .mana(1.0)
            .unit("near", 2, Mage)
            .at(10.0, 0.0)
            .casting("near", AbilityType::Frostbolt)
            .unit("far", 2, Mage)
            .at(20.0, 0.0)
            .casting("far", AbilityType::Frostbolt);
        if immune {
            scene = scene.aura_full("near", aura_of(AbilityType::AuraMastery));
        }
        scene
    };
    assert_eq!(
        scene(true).interrupt("shaman").interrupts_queued,
        vec![(AbilityType::WindShear, "far")]
    );
    assert_eq!(
        scene(false).interrupt("shaman").interrupts_queued,
        vec![(AbilityType::WindShear, "near")]
    );
}

/// The Felhunter's Spell Lock passes an immune caster over.
#[test]
fn spell_lock_passes_an_immune_caster_over() {
    let scene = |immune: bool| {
        let mut scene = Scene::new()
            .unit("felhunter spot", 1, Warlock)
            .unit(MAGE, 2, Mage)
            .at(10.0, 0.0)
            .casting(MAGE, AbilityType::Frostbolt);
        if immune {
            scene = scene.aura_full(MAGE, aura_of(AbilityType::AuraMastery));
        }
        scene
    };
    let immune = scene(true).spell_lock("felhunter spot");
    assert!(
        matches!(immune, Err(RejectionReason::NoValidTarget)),
        "{immune:?}"
    );
    let open = scene(false).spell_lock("felhunter spot");
    assert!(matches!(open, Ok(MAGE)), "{open:?}");
}

/// An enemy Mage mid-Frostbolt and a Pummel queued at it, the Mage immune to
/// interrupts or not. Returns (cast cut?, lockouts queued).
fn resolve_pummel(immune: bool) -> (bool, usize) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(GameRng::from_seed(0));
    let cast_time = defs().get_unchecked(&AbilityType::Frostbolt).cast_time;
    let warrior = world
        .spawn((
            Combatant::new(1, 0, Warrior),
            Transform::from_xyz(0.0, 1.0, 0.0),
        ))
        .id();
    let mut mage = world.spawn((
        Combatant::new(2, 0, Mage),
        Transform::from_xyz(2.4, 1.0, 0.0),
        CastingState::new(AbilityType::Frostbolt, warrior, cast_time),
    ));
    if immune {
        mage.insert(ActiveAuras {
            auras: vec![aura_of(AbilityType::AuraMastery)],
        });
    }
    let mage = mage.id();
    world.spawn(InterruptPending {
        caster: warrior,
        target: mage,
        ability: AbilityType::Pummel,
        lockout_duration: 4.0,
    });
    world.run_system_once(process_interrupts).unwrap();
    let cut = world.get::<CastingState>(mage).unwrap().interrupted;
    let lockouts = world
        .query::<&AuraPending>()
        .iter(&world)
        .filter(|p| p.target == mage && p.aura.effect_type == AuraType::SpellSchoolLockout)
        .count();
    assert_eq!(world.query::<&InterruptPending>().iter(&world).count(), 0);
    (cut, lockouts)
}

#[test]
fn an_interrupt_on_an_immune_caster_cuts_nothing_and_locks_nothing() {
    assert_eq!(resolve_pummel(true), (false, 0));
    assert_eq!(resolve_pummel(false), (true, 1), "control: not immune");
}

/// A Mage holding `held`, after `landing` lands on it: the auras it holds.
fn land_on_mage(held: Vec<Aura>, landing: Aura) -> Vec<AuraType> {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    world.insert_resource(Time::<()>::default());
    let paladin = world
        .spawn((Combatant::new(1, 0, Paladin), Transform::default()))
        .id();
    let mage = world
        .spawn((
            Combatant::new(1, 1, Mage),
            Transform::default(),
            ActiveAuras { auras: held },
        ))
        .id();
    let mut aura = landing;
    aura.caster = Some(paladin);
    world.spawn(AuraPending { target: mage, aura });
    world.run_system_once(apply_pending_auras).unwrap();
    world
        .get::<ActiveAuras>(mage)
        .unwrap()
        .auras
        .iter()
        .map(|a| a.effect_type)
        .collect()
}

/// A Silence is refused on an immune holder; control: it lands.
#[test]
fn no_silence_lands_on_an_immune_holder() {
    let landed = land_on_mage(vec![aura_of(AbilityType::AuraMastery)], silence());
    assert_eq!(landed, vec![AuraType::SilenceInterruptImmunity]);
    let landed = land_on_mage(vec![devotion()], silence());
    assert_eq!(
        landed,
        vec![AuraType::DamageTakenReduction, AuraType::Silence],
        "control"
    );
}

/// A second immunity landing on a holder of the first is refused, not
/// doubled (two Paladins casting together).
#[test]
fn a_second_aura_mastery_is_not_stacked() {
    let landed = land_on_mage(
        vec![aura_of(AbilityType::AuraMastery)],
        aura_of(AbilityType::AuraMastery),
    );
    assert_eq!(landed, vec![AuraType::SilenceInterruptImmunity]);
}

/// The immunity landing takes a Silence off its holder; control: another buff
/// landing leaves the Silence.
#[test]
fn the_immunity_landing_removes_a_silence() {
    let landed = land_on_mage(vec![silence()], aura_of(AbilityType::AuraMastery));
    assert_eq!(landed, vec![AuraType::SilenceInterruptImmunity]);
    let landed = land_on_mage(vec![silence()], devotion());
    assert_eq!(
        landed,
        vec![AuraType::Silence, AuraType::DamageTakenReduction],
        "control"
    );
}

// ---------------------------------------------------------------------------
// Played: each node acting in a match
// ---------------------------------------------------------------------------

const PAL: Unit = Unit::new(1, 0);
const MAGE_UNIT: Unit = Unit::new(1, 1);
const FOE: Unit = Unit::new(2, 0);

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

fn first_at(log: &str, needle: &str) -> Option<f32> {
    lines(log, needle).first().map(|l| at(l))
}

/// A Paladin + Mage against a Warrior on the Mage, at `mage_health`, for a
/// few seconds, the Paladin taking `build`.
fn emergency_match(build: Build, mage_health: f32) -> common::staged::Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Paladin".into(), "Mage".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(5),
        max_duration_secs: 4.0,
        team1_builds: vec![build],
        ..Default::default()
    };
    play(
        config,
        Stage::new()
            .place(PAL, 0.0, 0.0)
            .place(MAGE_UNIT, 15.0, 0.0)
            .health(MAGE_UNIT, mage_health)
            .place(FOE, 17.0, 0.0)
            .target(FOE, MAGE_UNIT),
    )
}

/// The Paladin's largest one-frame mana drop within a few frames of `effect`
/// first showing on the unit `on`.
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

/// The Paladin's mana spent over the first `frames` frames after the gates
/// open (from the frame before them).
fn mana_spent_from_gates(played: &common::staged::Played, frames: usize) -> f32 {
    let all = &played.frames;
    let paladin = |i: usize| {
        all[i]
            .combatants
            .values()
            .find(|c| !c.is_pet && c.team == 1 && c.slot == 0)
            .cloned()
            .expect("the Paladin is observed")
    };
    let gates = (1..all.len())
        .find(|&i| all[i].gates_open)
        .expect("the gates open");
    paladin(gates - 1).current_mana - paladin(gates + frames).current_mana
}

/// Divine Favor goes out for the dying Mage, the Holy Shock follows on the
/// next decision (off the global cooldown) and crits; it pays 5 mana. With
/// Infusion of Light the Holy Shock primes the next Flash of Light, which
/// spends it. Control: without the talents, a plain Holy Shock.
#[test]
fn divine_favor_and_infusion_of_light_act_in_a_match() {
    let build = Build::of(&[
        (ImprovedHolyShock, 2),
        (InfusionOfLight, 2),
        (DivineFavor, 1),
    ]);
    let played = emergency_match(build, 0.3);
    let log = &played.log;
    let favor = first_at(log, "Paladin #1 casts Divine Favor").unwrap_or_else(|| panic!("{log}"));
    let shock =
        first_at(log, "Paladin #1 casts Holy Shock (Heal)").unwrap_or_else(|| panic!("{log}"));
    assert!(
        shock >= favor && shock - favor < 0.1,
        "{favor} -> {shock}\n{log}"
    );
    assert!(log.contains("Divine Favor is spent on Holy Shock"), "{log}");
    assert!(log.contains("Holy Shock CRITICALLY heals"), "{log}");
    assert!(
        log.contains("Infusion of Light procs Infusion of Light"),
        "{log}"
    );
    assert!(
        log.contains("Infusion of Light is spent on Flash of Light"),
        "{log}"
    );
    // Divine Favor pays its 5 at the gates and the Holy Shock its 30 the
    // frame after: 35 over the first few frames, and nothing else is cast.
    let paid = mana_spent_from_gates(&played, 6);
    let cost = |a: AbilityType| defs().get_unchecked(&a).mana_cost;
    let want = cost(AbilityType::DivineFavor) + cost(AbilityType::HolyShock);
    assert!((paid - want).abs() < 0.5, "paid {paid}, want {want}\n{log}");

    let control = emergency_match(Build::default(), 0.3);
    assert!(!control.log.contains("Divine Favor"), "{}", control.log);
    assert!(
        !control.log.contains("Infusion of Light"),
        "{}",
        control.log
    );
    assert!(
        control.log.contains("casts Holy Shock (Heal)"),
        "{}",
        control.log
    );
}

/// A Mage casting its Frostbolt beside an enemy Rogue whose Kick comes ready
/// a second in: the Paladin raises Aura Mastery (free), and no Kick lands
/// while it lasts. Control: without the talent the Rogue kicks the Mage.
fn kick_match(build: Build, secs: f32) -> common::staged::Played {
    let config = HeadlessMatchConfig {
        team1: vec!["Paladin".into(), "Mage".into()],
        team2: vec!["Rogue".into()],
        random_seed: Some(5),
        max_duration_secs: secs,
        team1_builds: vec![build],
        ..Default::default()
    };
    play(
        config,
        Stage::new()
            .place(PAL, 0.0, 0.0)
            .place(MAGE_UNIT, 10.0, 0.0)
            .place(FOE, 12.0, 0.0)
            // The Paladin's instants waiting, so its global cooldown is free
            // when the Kick comes; the Rogue's stun waiting, so the Mage
            // casts.
            .cooldown(PAL, AbilityType::HolyShock, 10.0)
            .cooldown(PAL, AbilityType::HammerOfJustice, 30.0)
            .cooldown(FOE, AbilityType::KidneyShot, 30.0)
            // The Mage rooted where it stands, and its Frost Nova waiting, so
            // it stays in the Rogue's reach and casts.
            .aura(MAGE_UNIT, rooted())
            .cooldown(MAGE_UNIT, AbilityType::FrostNova, 30.0)
            .target(MAGE_UNIT, FOE)
            .target(FOE, MAGE_UNIT)
            .cooldown(FOE, AbilityType::Kick, 1.0)
            // Unstealthed, and alive the whole match.
            .combatant(FOE, |c| {
                c.stealthed = false;
                c.max_health = 20_000.0;
                c.current_health = 20_000.0;
            }),
    )
}

fn rooted() -> Aura {
    Aura {
        effect_type: AuraType::Root,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Held".into(),
        break_on_damage_threshold: -1.0,
        // Nothing a Cleanse takes.
        dispel_type: DispelType::Physical,
        ..Default::default()
    }
}

fn holy_to_mastery() -> Build {
    Build::of(&[
        (ImprovedHolyShock, 2),
        (HealingLight, 1),
        (DivineFavor, 1),
        (LightsGrace, 1),
        (AuraMastery, 1),
    ])
}

#[test]
fn aura_mastery_shields_a_cast_from_a_kick_in_a_match() {
    let played = kick_match(holy_to_mastery(), 4.0);
    let log = &played.log;
    let mastery = first_at(log, "Paladin #1 casts Aura Mastery").unwrap_or_else(|| panic!("{log}"));
    for unit in ["Paladin #1", "Mage #2"] {
        assert!(
            log.contains(&format!("{unit} gains Aura Mastery")),
            "{unit}\n{log}"
        );
    }
    let kicks: Vec<f32> = lines(log, "interrupts Team 1 Mage")
        .into_iter()
        .map(at)
        .collect();
    assert!(
        kicks.iter().all(|t| *t > mastery + 6.0),
        "kicked while immune: {kicks:?}\n{log}"
    );
    assert!(mana_paid(&played, (1, 0), AuraType::SilenceInterruptImmunity) < 0.5);

    let control = kick_match(Build::default(), 4.0);
    assert!(
        !lines(&control.log, "interrupts Team 1 Mage").is_empty(),
        "{}",
        control.log
    );
}

/// Light's Grace: a Holy Shock primes the next Holy Light, which then takes
/// 2.0s, not 2.5s. A `TeamPlan` Paladin (the profile that casts Holy Light),
/// its Warrior teammate hurt but out of danger, the one enemy held far off.
#[test]
fn lights_grace_shortens_the_next_holy_light_in_a_match() {
    let held = Aura {
        effect_type: AuraType::Stun,
        duration: 300.0,
        magnitude: 1.0,
        ability_name: "Held".into(),
        break_on_damage_threshold: -1.0,
        ..Default::default()
    };
    let config = HeadlessMatchConfig {
        team1: vec!["Paladin".into(), "Warrior".into()],
        team2: vec!["Mage".into()],
        random_seed: Some(5),
        max_duration_secs: 6.0,
        ai_profile: Some("TeamPlan".into()),
        team1_builds: vec![holy_to_mastery()],
        ..Default::default()
    };
    let stage = Stage::new()
        .place(PAL, 0.0, 0.0)
        .place(MAGE_UNIT, 5.0, 0.0)
        // A deficit a Holy Light fills, above the emergency line.
        .combatant(MAGE_UNIT, |c| {
            c.max_health = 2_000.0;
            c.current_health = 1_200.0;
        })
        .aura(MAGE_UNIT, held.clone())
        .place(FOE, 40.0, 0.0)
        .aura(FOE, held)
        .aura(PAL, lights_grace());
    let played = play(config, stage);
    let log = played.log;
    let begin = first_at(&log, "begins casting Holy Light").unwrap_or_else(|| panic!("{log}"));
    assert!(
        log.contains("Light's Grace is spent on Holy Light"),
        "{log}"
    );
    let heal = log
        .lines()
        .filter(|l| l.contains("Holy Light") && l.contains("heals"))
        .map(at)
        .find(|t| *t > begin)
        .unwrap_or_else(|| panic!("no Holy Light landed:\n{log}"));
    assert!(
        (heal - begin - 2.0).abs() < 0.05,
        "{begin} -> {heal}\n{log}"
    );
}

/// The Light's Grace aura its proc lands.
fn lights_grace() -> Aura {
    let talents = TalentDefinitions::shipped();
    let ProcEffect::NextCast {
        modifier,
        duration,
        dispel_type,
        name,
    } = (match &talents.get(LightsGrace).unwrap().per_rank[0] {
        TalentEffect::Proc(p) => p.effect.clone(),
        other => panic!("{other:?}"),
    })
    else {
        panic!("Light's Grace primes a next cast")
    };
    Aura {
        effect_type: AuraType::NextCast,
        duration,
        magnitude: 1.0,
        ability_name: name.unwrap(),
        next_cast: Some(modifier),
        dispel_type,
        ..Default::default()
    }
}

/// Each grant starts its 45s cooldown. Divine Favor: the Mage held in its
/// emergency for 16s, the Holy Shock comes ready twice (11s cooldown), and
/// only the first is favored. Aura Mastery: the Mage casting beside the Rogue
/// for 26s, the Kick lands once the immunity ends, and when it comes ready
/// again 12s on, the Paladin has no second Aura Mastery to answer it.
#[test]
fn divine_favor_and_aura_mastery_start_their_cooldowns() {
    let config = HeadlessMatchConfig {
        team1: vec!["Paladin".into(), "Mage".into()],
        team2: vec!["Warrior".into()],
        random_seed: Some(5),
        max_duration_secs: 16.0,
        team1_builds: vec![Build::of(&[
            (ImprovedHolyShock, 2),
            (InfusionOfLight, 2),
            (DivineFavor, 1),
        ])],
        ..Default::default()
    };
    let log = play(
        config,
        Stage::new()
            .place(PAL, 0.0, 0.0)
            .place(MAGE_UNIT, 15.0, 0.0)
            // Deep in its emergency for the whole match.
            .combatant(MAGE_UNIT, |c| {
                c.max_health = 20_000.0;
                c.current_health = 4_000.0;
            })
            .place(FOE, 17.0, 0.0)
            .target(FOE, MAGE_UNIT),
    )
    .log;
    assert!(
        lines(&log, "casts Holy Shock (Heal)").len() >= 2,
        "the Holy Shock came ready once:\n{log}"
    );
    assert_eq!(lines(&log, "casts Divine Favor").len(), 1, "{log}");

    let log = kick_match(holy_to_mastery(), 26.0).log;
    let mastery: Vec<f32> = lines(&log, "Paladin #1 casts Aura Mastery")
        .into_iter()
        .map(at)
        .collect();
    assert_eq!(mastery.len(), 1, "{log}");
    let kicks: Vec<f32> = lines(&log, "interrupts Team 1 Mage")
        .into_iter()
        .map(at)
        .collect();
    assert!(
        kicks.len() >= 2 && kicks.iter().all(|t| *t > mastery[0] + 6.0),
        "two Kicks, both after the immunity ended: {kicks:?}\n{log}"
    );
}

/// The immunity goes on teammates, not on their pets, as the Paladin's
/// auras do: a Warlock casting beside a Rogue gains it, and its Felhunter
/// does not.
#[test]
fn aura_mastery_passes_a_pet_by() {
    let config = HeadlessMatchConfig {
        team1: vec!["Paladin".into(), "Warlock".into()],
        team2: vec!["Rogue".into()],
        random_seed: Some(5),
        max_duration_secs: 6.0,
        team1_builds: vec![holy_to_mastery()],
        ..Default::default()
    };
    let log = play(
        config,
        Stage::new()
            .place(PAL, 0.0, 0.0)
            .place(MAGE_UNIT, 10.0, 0.0)
            .place(FOE, 12.0, 0.0)
            .target(FOE, MAGE_UNIT)
            .target(MAGE_UNIT, FOE)
            .cooldown(PAL, AbilityType::HolyShock, 10.0)
            .cooldown(PAL, AbilityType::HammerOfJustice, 30.0)
            .cooldown(FOE, AbilityType::KidneyShot, 30.0)
            // The Kick ready once the Warlock's first cast has begun.
            .cooldown(FOE, AbilityType::Kick, 3.0)
            .cooldown(MAGE_UNIT, AbilityType::DeathCoil, 30.0)
            .aura(MAGE_UNIT, rooted())
            // Too much health for the Rogue to make the Paladin heal.
            .combatant(MAGE_UNIT, |c| {
                c.max_health = 20_000.0;
                c.current_health = 20_000.0;
            })
            .combatant(FOE, |c| c.stealthed = false),
    )
    .log;
    assert!(log.contains("Paladin #1 casts Aura Mastery"), "{log}");
    assert!(log.contains("Warlock #2 gains Aura Mastery"), "{log}");
    assert!(
        !lines(&log, "gains Aura Mastery")
            .iter()
            .any(|l| l.contains("Felhunter")),
        "{log}"
    );
}

/// The tree's prose (`talent_text`): every Holy node reads, a rank a line.
#[test]
fn the_holy_tree_reads() {
    use arenasim::states::talent_text::rank_lines;
    let talents = TalentDefinitions::shipped();
    for (id, talent) in talents
        .talents_for_class(Paladin)
        .filter(|(_, t)| t.tree == TreeId::Holy)
    {
        let lines = rank_lines(talent, defs());
        eprintln!("{id:?}: {lines:?}");
        assert_eq!(lines.len(), usize::from(talent.max_rank), "{id:?}");
        assert!(lines.iter().all(|l| !l.is_empty()), "{id:?}");
    }
}
