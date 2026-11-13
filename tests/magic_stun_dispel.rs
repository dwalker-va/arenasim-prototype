//! AS-491 — a stun follows its removal class, as in TBC (user ruling,
//! 2026-11-13): Hammer of Justice (Holy, TBC 10308) and Blackout (declared
//! Magic, TBC 15269) are magic, so a Priest's Dispel Magic, a Paladin's
//! Cleanse and a Felhunter's Devour Magic take them off a teammate; Cheap
//! Shot, Kidney Shot and the Boar's Charge are Physical, so nothing does.
//!
//! - **the rotation**, in scene pairs: a magic stun is URGENT crowd control
//!   (`dispel_priority`, tied with Polymorph and Freezing Trap), so a healer
//!   dispels it ahead of healing a hurt team; a physical stun in its place
//!   draws no dispel at all;
//! - **every reader of `owed_dispel`** picks it up with no case of its own:
//!   the `Legacy` dispel walk, the `TeamPlan` solve's dispel goal, and the
//!   Medallion's wait for a teammate's dispel;
//! - **in a match**: a Priest's Dispel Magic takes a Hammer of Justice off its
//!   Warrior the frame it can; the control, a Kidney Shot, draws none.

mod common;

use bevy::prelude::*;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{self, Mage, Paladin, Priest, Rogue, Warrior};
use arenasim::states::play_match::abilities::AbilityType::{
    self, CheapShot, Corruption, DispelMagic, HammerOfJustice, KidneyShot, PaladinCleanse,
};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::components::{Aura, AuraPending, AuraType, DispelType};
use arenasim::states::play_match::decision_trace::{MovementTrigger, RejectionReason};
use arenasim::states::play_match::equipment::ItemId::MedallionOfTheAlliance as MEDALLION;
use arenasim::states::play_match::talent_procs::{effect_aura, ProcEvent, TalentProcSlot};
use arenasim::states::play_match::talents::{TalentDefinitions, TalentEffect, TalentId};
use arenasim::states::play_match::{HealerPosture, Posture};
use common::staged::play;

// ---------------------------------------------------------------------------
// The stuns, as the engine lands them
// ---------------------------------------------------------------------------

/// The aura `ability` lands, as `abilities.ron` builds it.
fn landed(ability: AbilityType) -> Aura {
    AuraPending::from_ability(
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
        AbilityDefinitions::shipped().get_unchecked(&ability),
    )
    .unwrap_or_else(|| panic!("{ability:?} lands an aura"))
    .aura
}

/// A stun `ability` lands, checked to be one, of the removal class expected.
fn stun(ability: AbilityType, magic: bool) -> Aura {
    let aura = landed(ability);
    assert_eq!(aura.effect_type, AuraType::Stun, "{ability:?}");
    assert_eq!(aura.can_be_dispelled(), magic, "{ability:?}");
    aura
}

fn hammer_of_justice() -> Aura {
    stun(HammerOfJustice, true)
}

fn kidney_shot() -> Aura {
    stun(KidneyShot, false)
}

/// Blackout's stun exactly as its proc lands it from the shipped talent
/// table: 3s, declared Magic.
fn blackout() -> Aura {
    let talent = TalentDefinitions::shipped()
        .get(TalentId::Blackout)
        .expect("Blackout is in the table");
    let proc = talent
        .per_rank
        .iter()
        .find_map(|e| match e {
            TalentEffect::Proc(p) => Some(p.clone()),
            _ => None,
        })
        .expect("Blackout is a proc");
    let slot = TalentProcSlot::new(TalentId::Blackout, &talent.name, proc);
    let (priest, victim) = (Entity::from_raw(900), Entity::from_raw(901));
    let event = ProcEvent::new(priest, slot.proc.trigger)
        .ability(AbilityType::MindBlast)
        .amount(50.0)
        .other(victim);
    let mut aura = effect_aura(&slot, priest, &event)
        .expect("the proc lands an aura")
        .aura;
    aura.caster = None;
    assert_eq!(aura.effect_type, AuraType::Stun);
    assert_eq!(aura.dispel_type, DispelType::Magic);
    assert!(aura.can_be_dispelled());
    aura
}

// ---------------------------------------------------------------------------
// The rotation, in scene pairs
// ---------------------------------------------------------------------------

const HEALER: Who = "healer";
const WARRIOR: Who = "warrior";
const MAGE: Who = "mage";
const ENEMY: Who = "enemy warrior";

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}; movement {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>(),
        view.movement.iter().map(|m| m.trigger).collect::<Vec<_>>()
    )
}

/// Fortitude and a spent shield on `who`, so a Priest has no buff or shield
/// to hand out.
fn settled(scene: Scene, who: Who) -> Scene {
    scene
        .aura(who, AuraType::MaxHealthIncrease, 600.0)
        .aura(who, AuraType::WeakenedSoul, 10.0)
}

/// A topped healer (`class`) at the origin with a Warrior teammate 3 yards
/// off and a Mage teammate 5 yards the other way, all at full health; an
/// enemy Warrior 25 yards off, attacking nobody.
fn healthy_team(class: CharacterClass) -> Scene {
    let s = Scene::new()
        .now(30.0)
        .unit(HEALER, 1, class)
        .at(0.0, 0.0)
        .targeting(ENEMY)
        .unit(WARRIOR, 1, Warrior)
        .at(3.0, 0.0)
        .targeting(ENEMY)
        .unit(MAGE, 1, Mage)
        .at(0.0, 5.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Warrior)
        .at(25.0, 0.0);
    let s = settled(settled(settled(s, HEALER), WARRIOR), MAGE);
    if class == Paladin {
        // Its aura already up, so the Paladin's first priority is out of the
        // way.
        s.aura_full(HEALER, landed(AbilityType::DevotionAura))
    } else {
        s
    }
}

/// A healthy team's Priest dispels a Hammer of Justice off its Warrior. Its
/// control, the same Warrior in a Kidney Shot instead, is no dispel at all:
/// Dispel Magic finds no candidate.
#[test]
fn a_priest_dispels_hammer_of_justice_and_leaves_a_kidney_shot() {
    let view = healthy_team(Priest)
        .aura_full(WARRIOR, hammer_of_justice())
        .decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(WARRIOR))),
        "{}",
        describe(&view)
    );

    let view = healthy_team(Priest)
        .aura_full(WARRIOR, kidney_shot())
        .decide(HEALER);
    assert_ne!(
        view.chose().map(|(a, _)| a),
        Some(DispelMagic),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(DispelMagic),
            Some(RejectionReason::NoValidTarget)
        ),
        "{}",
        describe(&view)
    );
}

/// A magic stun is URGENT: with the team hurt, so a routine dispel waits for
/// the heals, the Priest still dispels the Hammer of Justice first. Its
/// control puts a routine dispel (Corruption, a DoT) on the Warrior instead,
/// and the Priest heals.
#[test]
fn a_magic_stun_is_dispelled_ahead_of_healing_a_hurt_team() {
    let hurt = |debuff: Aura| {
        healthy_team(Priest)
            .health_of(WARRIOR, 0.6)
            .health_of(MAGE, 0.6)
            .aura_full(WARRIOR, debuff)
            .decide(HEALER)
    };
    let view = hurt(hammer_of_justice());
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(WARRIOR))),
        "{}",
        describe(&view)
    );

    let corruption = landed(Corruption);
    assert!(corruption.can_be_dispelled(), "Corruption is magic");
    let view = hurt(corruption);
    assert_ne!(
        view.chose().map(|(a, _)| a),
        Some(DispelMagic),
        "{}",
        describe(&view)
    );
}

/// A Paladin's Cleanse takes Blackout's stun off its Mage. Its control, a
/// Cheap Shot on the Mage, is no Cleanse.
#[test]
fn a_paladin_cleanses_blackout_and_leaves_a_cheap_shot() {
    let view = healthy_team(Paladin)
        .aura_full(MAGE, blackout())
        .decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((PaladinCleanse, Some(MAGE))),
        "{}",
        describe(&view)
    );

    let view = healthy_team(Paladin)
        .aura_full(MAGE, stun(CheapShot, false))
        .decide(HEALER);
    assert_ne!(
        view.chose().map(|(a, _)| a),
        Some(PaladinCleanse),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// The readers of `owed_dispel`
// ---------------------------------------------------------------------------

/// A healer (`class`) at the origin and its Warrior teammate 45 yards off
/// (beyond Dispel Magic's and Cleanse's 30), wearing `stun`; an enemy Hunter
/// far off, on nobody.
fn stunned_far_off(class: CharacterClass, stun: Aura) -> Scene {
    Scene::new()
        .unit(HEALER, 1, class)
        .aura(HEALER, AuraType::MaxHealthIncrease, 600.0)
        .unit(WARRIOR, 1, Warrior)
        .at(45.0, 0.0)
        .aura_full(WARRIOR, stun)
        .unit(ENEMY, 2, CharacterClass::Hunter)
        .at(60.0, 30.0)
}

fn walks_to_warrior(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::DispelChase)
}

/// The `Legacy` dispel walk: a Priest walks toward a teammate held in a
/// Hammer of Justice beyond its reach. Its control, a Kidney Shot, draws no
/// walk.
#[test]
fn the_dispel_walk_goes_to_a_magic_stun_and_not_a_physical_one() {
    let view = stunned_far_off(Priest, hammer_of_justice()).decide(HEALER);
    assert!(walks_to_warrior(&view), "{}", describe(&view));
    let view = stunned_far_off(Priest, kidney_shot()).decide(HEALER);
    assert!(!walks_to_warrior(&view), "{}", describe(&view));
}

/// The `TeamPlan` solve's dispel goal: a PRESSURED `TeamPlan` Paladin is
/// walked toward the same teammate. Its control, a Kidney Shot, owes no walk.
#[test]
fn the_teamplan_dispel_goal_goes_to_a_magic_stun_and_not_a_physical_one() {
    let pressured = |stun: Aura| {
        let mut posture = HealerPosture::new(29.0);
        posture.posture = Posture::Pressured;
        posture.hold_until = 40.0;
        stunned_far_off(Paladin, stun)
            .ai_profile(1, AiProfile::TeamPlan)
            .ai_profile(2, AiProfile::TeamPlan)
            .healer_posture(HEALER, posture)
            .decide(HEALER)
    };
    let view = pressured(hammer_of_justice());
    assert!(walks_to_warrior(&view), "{}", describe(&view));
    let view = pressured(kidney_shot());
    assert!(!walks_to_warrior(&view), "{}", describe(&view));
}

const WEARER: Who = "wearer";
const ROGUE: Who = "rogue";

/// A Warrior wearing the Medallion, held by `hold`, with a Priest 10 yards
/// off (in Dispel Magic's reach) and a Rogue on it.
fn medallion_wearer(hold: Aura) -> Scene {
    Scene::new()
        .unit(WEARER, 1, Warrior)
        .on_use_item(WEARER, MEDALLION)
        .aura_full(WEARER, hold)
        .unit(HEALER, 1, Priest)
        .at(0.0, 10.0)
        .unit(ROGUE, 2, Rogue)
        .at(2.0, 0.0)
        .targeting(WEARER)
}

/// The Medallion's wait (`expected_dispeller`): held in a Hammer of Justice
/// beside a Priest, the wearer waits for the Dispel Magic. Its control, a
/// Kidney Shot, breaks at once.
#[test]
fn the_medallion_waits_for_a_dispel_of_a_magic_stun_and_not_a_physical_one() {
    let view = medallion_wearer(hammer_of_justice()).decide_held(WEARER);
    assert!(view.items_used.is_empty(), "{view:?}");
    assert!(
        matches!(view.item_decisions.as_slice(),
            [(MEDALLION, Some(RejectionReason::PreconditionUnmet { note }))] if note.contains("waiting for")),
        "{view:?}"
    );

    let view = medallion_wearer(kidney_shot()).decide_held(WEARER);
    assert_eq!(view.items_used, vec![(WEARER, MEDALLION)], "{view:?}");
}

// ---------------------------------------------------------------------------
// In a match
// ---------------------------------------------------------------------------

const PRIEST_UNIT: Unit = Unit::new(1, 0);
const WARRIOR_UNIT: Unit = Unit::new(1, 1);
const PALADIN_UNIT: Unit = Unit::new(2, 0);
const ROGUE_UNIT: Unit = Unit::new(2, 1);

/// A unit held stunned the whole match, so it never closes, kicks or casts.
/// Schoolless, so no dispel takes it.
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

/// A Priest and a Warrior against a held Paladin and Rogue, for 4 seconds
/// from the gates. On the gates the Warrior takes `stun` from its caster.
fn stun_match(stun: AbilityType, from: Unit) -> String {
    let config = HeadlessMatchConfig {
        team1: vec!["Priest".into(), "Warrior".into()],
        team2: vec!["Paladin".into(), "Rogue".into()],
        random_seed: Some(5),
        max_duration_secs: 4.0,
        ..Default::default()
    };
    let mut stage = Stage::new()
        .place(PRIEST_UNIT, 0.0, 0.0)
        .place(WARRIOR_UNIT, 3.0, 0.0)
        .place(PALADIN_UNIT, 20.0, 0.0)
        .place(ROGUE_UNIT, 20.0, 4.0);
    for unit in [PALADIN_UNIT, ROGUE_UNIT] {
        stage = stage.aura(unit, held()).combatant(unit, |c| {
            c.max_health = 20_000.0;
            c.current_health = 20_000.0;
        });
    }
    play(config, stage.land(WARRIOR_UNIT, stun, from)).log
}

fn lines<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

/// The Priest's Dispel Magic takes the Hammer of Justice off its Warrior in a
/// match. The control, a Kidney Shot, draws no Dispel Magic and stays.
#[test]
fn dispel_magic_takes_hammer_of_justice_in_a_match_and_leaves_a_kidney_shot() {
    let log = stun_match(HammerOfJustice, PALADIN_UNIT);
    assert_eq!(
        lines(&log, "casts Dispel Magic on Team 1 Warrior").len(),
        1,
        "one Dispel Magic on the Warrior:\n{log}"
    );
    let removed = lines(&log, "removed from Team 1 Warrior");
    assert_eq!(removed.len(), 1, "{log}");
    assert!(
        removed[0].contains("Hammer of Justice removed"),
        "{}\n{log}",
        removed[0]
    );

    let control = stun_match(KidneyShot, ROGUE_UNIT);
    assert!(
        lines(&control, "Dispel Magic").is_empty(),
        "no Dispel Magic on a Kidney Shot:\n{control}"
    );
    assert!(lines(&control, "removed from").is_empty(), "{control}");
}
