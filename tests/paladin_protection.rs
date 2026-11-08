//! The Paladin's Protection tree (AS-421): its AI rules as scene pairs, its
//! nodes as the card writes them, and each grant and option played end to end.
//!
//! Each scene is a PAIR: the situation its rule answers and the control with
//! that one condition removed, so a rule deleted or mis-gated fails here.

mod common;

use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::match_config::CharacterClass::{Mage, Paladin, Warrior};
use arenasim::states::match_config::PaladinAura;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{Aura, AuraType};
use arenasim::states::play_match::decision_trace::RejectionReason;
use arenasim::states::play_match::talent_choices::ChoiceOption;
use arenasim::states::play_match::talents::{
    Build, TalentDefinitions, TalentEffect, TalentId, TalentStat,
};

const PALADIN: &str = "paladin";
const MAGE: &str = "mage";
const ENEMY: &str = "enemy warrior";

fn parse(json: &str) -> HeadlessMatchConfig {
    serde_json::from_str(json).expect("the config parses")
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

fn divine_protection_aura() -> Aura {
    Aura {
        effect_type: AuraType::DamageTakenReduction,
        duration: 8.0,
        magnitude: 0.30,
        ability_name: "Divine Protection".into(),
        ..Default::default()
    }
}

fn precondition(view: &DecisionView, ability: AbilityType) -> bool {
    matches!(
        view.rejected(ability),
        Some(RejectionReason::PreconditionUnmet { .. })
    )
}

// ---------------------------------------------------------------------------
// Divine Protection: the Paladin is dying and pressured.
// ---------------------------------------------------------------------------

/// A Paladin at `health`, an enemy Warrior on it, its Mage teammate far off.
fn paladin_under_attack(health: f32) -> Scene {
    Scene::new()
        .unit(PALADIN, 1, Paladin)
        .health(health)
        .aura_full(PALADIN, devotion())
        .unit(MAGE, 1, Mage)
        .at(-25.0, 0.0)
        .unit(ENEMY, 2, Warrior)
        .at(2.0, 0.0)
        .targeting(PALADIN)
}

#[test]
fn divine_protection_answers_the_paladin_in_danger() {
    let scene = paladin_under_attack(0.4).grant(PALADIN, AbilityType::DivineProtection);
    let view = scene.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::DivineProtection, Some(PALADIN))),
        "{}",
        describe(&view)
    );

    // Control: the same Paladin, healthy, is not in danger.
    let calm = paladin_under_attack(0.9).grant(PALADIN, AbilityType::DivineProtection);
    let view = calm.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::DivineProtection),
        "{}",
        describe(&view)
    );
}

#[test]
fn divine_protection_is_not_stacked_on_itself_or_the_bubble() {
    for already in [divine_protection_aura(), {
        let mut bubble = divine_protection_aura();
        bubble.effect_type = AuraType::DamageImmunity;
        bubble.ability_name = "Divine Shield".into();
        bubble
    }] {
        let scene = paladin_under_attack(0.4)
            .grant(PALADIN, AbilityType::DivineProtection)
            .aura_full(PALADIN, already.clone());
        let view = scene.decide(PALADIN);
        assert!(
            matches!(
                view.rejected(AbilityType::DivineProtection),
                Some(RejectionReason::AlreadyApplied)
            ),
            "{}: {}",
            already.ability_name,
            describe(&view)
        );
    }
}

/// While Divine Protection is up, Divine Shield waits for its survival
/// trigger; without it the same Paladin bubbles.
#[test]
fn the_bubble_waits_while_divine_protection_is_up() {
    let held = paladin_under_attack(0.4)
        .grant(PALADIN, AbilityType::DivineProtection)
        .aura_full(PALADIN, divine_protection_aura());
    let view = held.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::DivineShield),
        "{}",
        describe(&view)
    );
    assert_ne!(
        view.chose().map(|(a, _)| a),
        Some(AbilityType::DivineShield)
    );

    // Control: Divine Protection on cooldown and not up — the bubble fires.
    let bare = paladin_under_attack(0.4)
        .grant(PALADIN, AbilityType::DivineProtection)
        .cooldown(PALADIN, AbilityType::DivineProtection, 30.0);
    let view = bare.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::DivineShield, Some(PALADIN))),
        "{}",
        describe(&view)
    );

    // And below the survival trigger, the bubble fires through it.
    let mut dire = paladin_under_attack(0.2)
        .grant(PALADIN, AbilityType::DivineProtection)
        .aura_full(PALADIN, divine_protection_aura());
    dire.set_health(PALADIN, 0.2);
    let view = dire.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::DivineShield, Some(PALADIN))),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Blessing of Sacrifice: the teammate in danger.
// ---------------------------------------------------------------------------

/// A healthy Paladin 10yd from its Mage at `health`, an enemy Warrior on the
/// Mage (out of the Paladin's Hammer of Justice range).
fn mage_under_attack(health: f32) -> Scene {
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
fn blessing_of_sacrifice_goes_on_the_teammate_in_danger() {
    let scene = mage_under_attack(0.3).grant(PALADIN, AbilityType::BlessingOfSacrifice);
    let view = scene.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::BlessingOfSacrifice, Some(MAGE))),
        "{}",
        describe(&view)
    );

    // Control: the Mage, pressured but healthy, is not in danger.
    let calm = mage_under_attack(0.8).grant(PALADIN, AbilityType::BlessingOfSacrifice);
    let view = calm.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::BlessingOfSacrifice),
        "{}",
        describe(&view)
    );
}

/// A teammate already blessed is not blessed again.
#[test]
fn blessing_of_sacrifice_is_not_recast_on_a_blessed_teammate() {
    let blessed = Aura {
        effect_type: AuraType::DamageTransfer,
        duration: 8.0,
        magnitude: 0.30,
        ability_name: "Blessing of Sacrifice".into(),
        ..Default::default()
    };
    let scene = mage_under_attack(0.3)
        .grant(PALADIN, AbilityType::BlessingOfSacrifice)
        .aura_full(MAGE, blessed);
    let view = scene.decide(PALADIN);
    assert!(
        matches!(
            view.rejected(AbilityType::BlessingOfSacrifice),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&view)
    );
}

/// The Paladin is never its own sacrifice: in danger itself, with no other
/// teammate in danger, it does not bless itself.
#[test]
fn blessing_of_sacrifice_never_goes_on_the_paladin() {
    let scene = paladin_under_attack(0.4)
        .grant(PALADIN, AbilityType::BlessingOfSacrifice)
        .cooldown(PALADIN, AbilityType::DivineShield, 200.0);
    let view = scene.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::BlessingOfSacrifice),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Sacred Shield: kept on the teammate most likely to be focused.
// ---------------------------------------------------------------------------

#[test]
fn sacred_shield_goes_on_the_pressured_teammate() {
    let scene = mage_under_attack(0.8).grant(PALADIN, AbilityType::SacredShield);
    let view = scene.decide(PALADIN);
    assert_eq!(
        view.chose(),
        Some((AbilityType::SacredShield, Some(MAGE))),
        "{}",
        describe(&view)
    );

    // Control: the Warrior targets nobody, so nobody is pressured.
    let mut calm = mage_under_attack(0.8).grant(PALADIN, AbilityType::SacredShield);
    calm.set_position(ENEMY, 60.0, 60.0);
    let view = calm.decide(PALADIN);
    assert!(
        precondition(&view, AbilityType::SacredShield),
        "{}",
        describe(&view)
    );
}

/// Kept, not recast: a teammate that already holds it is left alone.
#[test]
fn sacred_shield_is_kept_not_recast() {
    let shield = Aura {
        effect_type: AuraType::TriggeredAbsorb,
        duration: 20.0,
        ability_name: "Sacred Shield".into(),
        ..Default::default()
    };
    let scene = mage_under_attack(0.8)
        .grant(PALADIN, AbilityType::SacredShield)
        .aura_full(MAGE, shield);
    let view = scene.decide(PALADIN);
    assert!(
        matches!(
            view.rejected(AbilityType::SacredShield),
            Some(RejectionReason::AlreadyApplied)
        ),
        "{}",
        describe(&view)
    );
}

/// Of two pressured teammates it shields the more injured.
#[test]
fn sacred_shield_picks_the_more_injured_of_two_pressured_teammates() {
    let pair = |paladin_hp: f32, mage_hp: f32| {
        let mut scene = mage_under_attack(mage_hp)
            .grant(PALADIN, AbilityType::SacredShield)
            .cooldown(PALADIN, AbilityType::HammerOfJustice, 30.0)
            .unit("enemy warrior 2", 2, Warrior)
            .at(2.0, 0.0)
            .targeting(PALADIN);
        scene.set_health(PALADIN, paladin_hp);
        scene.decide(PALADIN)
    };
    let view = pair(0.9, 0.7);
    assert_eq!(
        view.chose(),
        Some((AbilityType::SacredShield, Some(MAGE))),
        "{}",
        describe(&view)
    );
    let view = pair(0.7, 0.9);
    assert_eq!(
        view.chose(),
        Some((AbilityType::SacredShield, Some(PALADIN))),
        "{}",
        describe(&view)
    );
}

/// Sacred Shield's tooltip states its triggered absorb's numbers.
#[test]
fn sacred_shields_text_states_its_triggered_absorb() {
    let def = AbilityDefinitions::shipped().get_unchecked(&AbilityType::SacredShield);
    assert_eq!(
        arenasim::states::ability_text::build_aura_description(def.applies_aura.as_ref().unwrap()),
        "For 30 sec, each time the target takes damage it gains a shield absorbing 40 damage \
         for 6 sec, at most once every 6 sec."
    );
}

// ---------------------------------------------------------------------------
// The tree, as the card writes it.
// ---------------------------------------------------------------------------

#[test]
fn the_protection_tree_is_as_specified() {
    use TalentId::*;
    let talents = TalentDefinitions::shipped();
    let rows: Vec<(TalentId, u8, u8, u8, Option<f32>)> = talents
        .talents_for_class(Paladin)
        .map(|(id, t)| (id, t.tier, t.column, t.max_rank, t.budget))
        .collect();
    assert_eq!(
        rows,
        vec![
            (SacredDuty, 1, 0, 2, None),
            (ImprovedHammerOfJustice, 1, 2, 2, Some(11.0)),
            (DivineProtection, 2, 0, 1, Some(11.0)),
            (RetributionAura, 2, 1, 2, Some(11.0)),
            (Toughness, 2, 2, 2, Some(11.0)),
            (BlessingOfSacrifice, 3, 1, 1, Some(11.0)),
            (SacredShield, 4, 1, 1, Some(22.0)),
        ]
    );
    assert_eq!(
        talents.get(SacredShield).unwrap().requires,
        Some((BlessingOfSacrifice, 1))
    );
    assert_eq!(
        talents.get(SacredDuty).unwrap().per_rank,
        vec![TalentEffect::StatPercent(TalentStat::MaxHealth, 0.024554)]
    );
    assert_eq!(
        talents.get(Toughness).unwrap().per_rank,
        vec![TalentEffect::StatPercent(TalentStat::Armor, 0.10)]
    );
    for (talent, ability) in [
        (DivineProtection, AbilityType::DivineProtection),
        (BlessingOfSacrifice, AbilityType::BlessingOfSacrifice),
        (SacredShield, AbilityType::SacredShield),
    ] {
        assert_eq!(
            talents.get(talent).unwrap().per_rank,
            vec![TalentEffect::Grant(ability)]
        );
    }
}

/// Improved Hammer of Justice takes 10s a rank: 60 -> 50 -> 40.
#[test]
fn improved_hammer_of_justice_shortens_the_cooldown_a_rank_at_a_time() {
    let defs = AbilityDefinitions::shipped();
    let talents = TalentDefinitions::shipped();
    let cooldown = |rank: u8| {
        let build = if rank == 0 {
            Build::default()
        } else {
            Build::of(&[(TalentId::ImprovedHammerOfJustice, rank)])
        };
        talents
            .resolve(defs, Paladin, &build)
            .unwrap()
            .book()
            .get_unchecked(&AbilityType::HammerOfJustice)
            .cooldown
    };
    assert_eq!([cooldown(0), cooldown(1), cooldown(2)], [60.0, 50.0, 40.0]);
}

/// Retribution Aura rank 1 offers the aura; rank 2 raises its damage 4 -> 6.
/// Without the node the option is hidden.
#[test]
fn retribution_aura_is_offered_by_its_node_and_raised_by_rank_two() {
    let defs = AbilityDefinitions::shipped();
    let talents = TalentDefinitions::shipped();
    let option = ChoiceOption::PaladinAura(PaladinAura::RetributionAura);
    let build =
        |rank: u8| Build::of(&[(TalentId::SacredDuty, 2), (TalentId::RetributionAura, rank)]);
    assert!(!talents.offers(&Build::default(), option));
    assert!(!talents.offers(&Build::of(&[(TalentId::SacredDuty, 2)]), option));
    assert!(talents.offers(&build(1), option));
    let damage = |rank: u8| {
        talents
            .resolve(defs, Paladin, &build(rank))
            .unwrap()
            .book()
            .get_unchecked(&AbilityType::RetributionAura)
            .applies_aura
            .as_ref()
            .unwrap()
            .magnitude
    };
    assert_eq!([damage(1), damage(2)], [4.0, 6.0]);
    // The untalented Paladin's Retribution Aura is the base 4, unoffered.
    assert_eq!(
        defs.get_unchecked(&AbilityType::RetributionAura)
            .applies_aura
            .as_ref()
            .unwrap()
            .magnitude,
        4.0
    );
}

/// A Paladin is never offered Retribution Aura in a config that names it
/// without the node: the config is refused.
#[test]
fn a_config_choosing_retribution_aura_without_the_node_is_refused() {
    let base = r#"{"team1":["Paladin"],"team2":["Warrior"],"team1_paladin_auras":["Retribution"]"#;
    let refused = parse(&format!("{base}}}")).to_match_config().map(|_| ());
    assert!(refused.is_err(), "{refused:?}");
    let allowed = parse(&format!(
        r#"{base},"team1_builds":[{{"SacredDuty":2,"RetributionAura":1}}]}}"#
    ))
    .to_match_config()
    .map(|_| ());
    assert!(allowed.is_ok(), "{allowed:?}");
}

// ---------------------------------------------------------------------------
// End to end: each grant and the aura option, played.
// ---------------------------------------------------------------------------

/// A Paladin + Mage against two Warriors on BasicArena, the Paladin taking
/// `build` and choosing `aura`.
fn protection_match(build: &str, aura: &str) -> common::staged::Played {
    let config = parse(&format!(
        r#"{{"team1":["Paladin","Mage"],"team2":["Warrior","Warrior"],"map":"BasicArena",
            "random_seed":11,"max_duration_secs":90,
            "team1_builds":[{build},{{}}],"team1_paladin_auras":["{aura}"]}}"#
    ));
    common::staged::play_seeded(config)
}

/// Every grant lands in a staged match: the Paladin dying under one Warrior
/// (Divine Protection), its Mage dying under the other (Blessing of
/// Sacrifice, whose share then reaches the Paladin), and Sacred Shield on
/// the teammate in danger raising its shield on the next hit.
#[test]
fn every_protection_grant_lands_in_a_match() {
    use arenasim::headless::stage::{Stage, Unit};
    const PAL: Unit = Unit::new(1, 0);
    const MAGE_UNIT: Unit = Unit::new(1, 1);
    const W1: Unit = Unit::new(2, 0);
    const W2: Unit = Unit::new(2, 1);
    let config = parse(
        r#"{"team1":["Paladin","Mage"],"team2":["Warrior","Warrior"],"map":"BasicArena",
            "random_seed":11,"max_duration_secs":30,
            "team1_builds":[{"SacredDuty":2,"DivineProtection":1,"Toughness":1,
                             "BlessingOfSacrifice":1,"SacredShield":1},{}]}"#,
    );
    let stage = Stage::new()
        .place(PAL, 0.0, 0.0)
        .health(PAL, 0.45)
        .place(W1, 2.0, 0.0)
        .target(W1, PAL)
        .place(MAGE_UNIT, 12.0, 0.0)
        .health(MAGE_UNIT, 0.35)
        .place(W2, 14.0, 0.0)
        .target(W2, MAGE_UNIT);
    let played = common::staged::play(config, stage);
    let log = &played.log;
    for line in [
        "Team 1 Paladin #1 casts Divine Protection",
        "Team 1 Paladin #1 casts Blessing of Sacrifice on Team 1 Mage #2",
        "[TRANSFER] Team 1 Paladin #1 takes",
        "casts Sacred Shield",
        "gains Sacred Shield (absorbs 40)",
    ] {
        assert!(log.contains(line), "no {line:?} in the match log:\n{log}");
    }
}

#[test]
fn retribution_aura_strikes_back_at_melee_attackers() {
    for (rank, damage) in [(1, "4"), (2, "6")] {
        let played = protection_match(
            &format!(r#"{{"SacredDuty":2,"RetributionAura":{rank}}}"#),
            "Retribution",
        );
        let reflect = format!("takes {damage} Holy damage from");
        let hit = played
            .log
            .lines()
            .find(|l| l.contains("[REFLECT]") && l.contains("Retribution Aura"))
            .unwrap_or_else(|| panic!("rank {rank}: no Retribution Aura hit:\n{}", played.log));
        assert!(hit.contains(&reflect), "rank {rank}: {hit}");
    }
}
