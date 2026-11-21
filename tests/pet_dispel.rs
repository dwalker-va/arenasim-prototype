//! AS-129 — a healer's dispel frees its team's pets (user ruling on AS-126,
//! 2026-09-20: pets are friendly-dispel targets, game-wide). Before it, the
//! Priest's Dispel Magic and the Paladin's Cleanse skipped pets, so a trapped
//! Felhunter ran its whole Freezing Trap.
//!
//! - **the rotation**, in scene pairs: a Priest dispels a trapped Felhunter;
//!   a dying non-pet teammate (the Priest itself included) is healed first;
//!   at equal priority a non-pet teammate is freed before a pet;
//! - **the root tier**: a pet fights in melee, so a root that keeps it off its
//!   target is the Paladin's root-bar Cleanse;
//! - **every reader of `owed_dispel`**: the `Legacy` dispel walk goes to a
//!   trapped pet, ranks it below a non-pet teammate, and leaves it while the
//!   Priest is dying; the Medallion's wait sees a reached non-pet before a pet;
//! - **in a match**: the Priest's Dispel Magic takes the trap off the
//!   Felhunter, which acts again.

mod common;

use bevy::prelude::*;

use arenasim::headless::config::HeadlessMatchConfig;
use arenasim::headless::{Stage, Unit};
use arenasim::states::match_config::CharacterClass::{
    self, Hunter, Mage, Paladin, Priest, Rogue, Warlock, Warrior,
};
use arenasim::states::play_match::abilities::AbilityType::{
    self, DispelMagic, FlashHeal, FrostNova, PaladinCleanse,
};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene, Who};
use arenasim::states::play_match::components::{Aura, AuraPending, AuraType};
use arenasim::states::play_match::decision_trace::{MovementTrigger, RejectionReason};
use arenasim::states::play_match::equipment::ItemId::MedallionOfTheAlliance as MEDALLION;
use arenasim::states::play_match::traps::freezing_trap_aura;
use arenasim::states::play_match::{MovementGoal, PetType};
use common::staged::play;

const HEALER: Who = "healer";
const WARLOCK: Who = "warlock";
const FELHUNTER: Who = "felhunter";
const WARRIOR: Who = "warrior";
const ENEMY: Who = "enemy hunter";

fn trap() -> Aura {
    freezing_trap_aura(Entity::PLACEHOLDER)
}

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

/// A topped healer (`class`) at the origin, its Warlock 3 yards off and the
/// Warlock's Felhunter 5 yards the other way, all at full health; an enemy
/// Hunter 25 yards off, attacking nobody.
fn warlock_team(class: CharacterClass) -> Scene {
    let s = Scene::new()
        .now(30.0)
        .unit(HEALER, 1, class)
        .at(0.0, 0.0)
        .targeting(ENEMY)
        .unit(WARLOCK, 1, Warlock)
        .at(3.0, 0.0)
        .targeting(ENEMY)
        .pet(FELHUNTER, WARLOCK, PetType::Felhunter)
        .at(0.0, 5.0)
        .targeting(ENEMY)
        .unit(ENEMY, 2, Hunter)
        .at(25.0, 0.0);
    let s = settled(settled(settled(s, HEALER), WARLOCK), FELHUNTER);
    if class == Paladin {
        // Its aura already up, so the Paladin's first priority is out of the
        // way.
        s.aura_full(HEALER, landed(AbilityType::DevotionAura))
    } else {
        s
    }
}

// ---------------------------------------------------------------------------
// The rotation
// ---------------------------------------------------------------------------

/// A healthy team's Priest frees its trapped Felhunter. The control, no trap,
/// is no dispel at all: Dispel Magic finds no candidate.
#[test]
fn a_priest_dispels_a_trapped_felhunter() {
    let view = warlock_team(Priest)
        .aura_full(FELHUNTER, trap())
        .decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(FELHUNTER))),
        "{}",
        describe(&view)
    );

    let view = warlock_team(Priest).decide(HEALER);
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

/// The Paladin's Cleanse frees a trapped Felhunter too, through the same scan.
#[test]
fn a_paladin_cleanses_a_trapped_felhunter() {
    let view = warlock_team(Paladin)
        .aura_full(FELHUNTER, trap())
        .decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((PaladinCleanse, Some(FELHUNTER))),
        "{}",
        describe(&view)
    );
}

/// A dying Warlock is healed before its trapped Felhunter is freed. The
/// control: the Warlock hurt but not dying (above `urgency_hp_threshold`,
/// still below Flash Heal's trigger) — the trap is urgent crowd control and
/// goes first.
#[test]
fn a_dying_teammate_is_healed_before_a_trapped_pet() {
    let view = warlock_team(Priest)
        .health_of(WARLOCK, 0.3)
        .aura_full(FELHUNTER, trap())
        .decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((FlashHeal, Some(WARLOCK))),
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

    let view = warlock_team(Priest)
        .health_of(WARLOCK, 0.6)
        .aura_full(FELHUNTER, trap())
        .decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(FELHUNTER))),
        "{}",
        describe(&view)
    );
}

/// The healer counts as a dying teammate: a dying Priest heals itself before
/// it frees a trapped pet. The control is the same Priest at full health.
#[test]
fn a_dying_healer_heals_itself_before_a_trapped_pet() {
    let view = warlock_team(Priest)
        .health_of(HEALER, 0.3)
        .aura_full(FELHUNTER, trap())
        .decide(HEALER);
    assert_ne!(
        view.chose().map(|(a, _)| a),
        Some(DispelMagic),
        "{}",
        describe(&view)
    );

    let view = warlock_team(Priest)
        .aura_full(FELHUNTER, trap())
        .decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(FELHUNTER))),
        "{}",
        describe(&view)
    );
}

/// A Warrior added AFTER the Felhunter (so entity order alone would pick the
/// pet) and trapped beside it: the non-pet teammate is freed first. The
/// control traps only the Felhunter, so the pet is freed.
#[test]
fn a_non_pet_teammate_is_freed_before_a_pet() {
    let team = || {
        settled(
            warlock_team(Priest)
                .unit(WARRIOR, 1, Warrior)
                .at(-3.0, 0.0)
                .targeting(ENEMY),
            WARRIOR,
        )
        .aura_full(FELHUNTER, trap())
    };
    assert!(team().entity(FELHUNTER) < team().entity(WARRIOR));

    let view = team().aura_full(WARRIOR, trap()).decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(WARRIOR))),
        "{}",
        describe(&view)
    );

    let view = team().decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(FELHUNTER))),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// The root tier
// ---------------------------------------------------------------------------

const MAGE: Who = "enemy mage";

/// A Paladin's team with its Warlock hurt (above `LOW_HP_THRESHOLD`, below the
/// healthy bar, so the routine Cleanse waits for the heals) and its Felhunter
/// in a Frost Nova, its target the enemy Mage `mage_at` yards off.
fn rooted_felhunter(mage_at: f32) -> Scene {
    warlock_team(Paladin)
        .health_of(WARLOCK, 0.6)
        .unit(MAGE, 2, Mage)
        .at(0.0, 5.0 + mage_at)
        .target(FELHUNTER, MAGE)
        .aura_full(FELHUNTER, landed(FrostNova))
}

/// A Felhunter rooted out of reach of its target is held back, as a rooted
/// melee teammate is, so the Paladin Cleanses it at the root bar ahead of
/// healing. The control has its target beside it: the root costs it nothing,
/// and the Paladin heals.
#[test]
fn a_pet_rooted_off_its_target_is_cleansed_at_the_root_bar() {
    let view = rooted_felhunter(20.0).decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((PaladinCleanse, Some(FELHUNTER))),
        "{}",
        describe(&view)
    );

    let view = rooted_felhunter(1.0).decide(HEALER);
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

/// A Priest at the origin with its Warlock beside it, the Felhunter `pet_at`
/// yards down +X (Dispel Magic reaches 30); an enemy Hunter far off, on
/// nobody.
fn far_pet(pet_at: f32) -> Scene {
    let s = Scene::new()
        .now(30.0)
        .unit(HEALER, 1, Priest)
        .unit(WARLOCK, 1, Warlock)
        .at(0.0, 3.0)
        .pet(FELHUNTER, WARLOCK, PetType::Felhunter)
        .at(pet_at, 0.0)
        .unit(ENEMY, 2, Hunter)
        .at(60.0, 40.0);
    settled(settled(settled(s, HEALER), WARLOCK), FELHUNTER)
}

/// The walk's goal, when the decision walked for a dispel.
fn dispel_walk_goal(view: &DecisionView) -> Option<Vec3> {
    if !view
        .movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::DispelChase)
    {
        return None;
    }
    match view.directive.map(|d| d.goal) {
        Some(MovementGoal::Point(p)) => Some(p),
        other => panic!("a dispel walk walks to a point: {other:?}"),
    }
}

fn near(a: Vec3, b: Vec3) -> bool {
    Vec2::new(a.x - b.x, a.z - b.z).length() < 0.5
}

/// The `Legacy` dispel walk carries the Priest to its Felhunter trapped beyond
/// Dispel Magic's range. The control, no trap, walks nowhere.
#[test]
fn the_dispel_walk_goes_to_a_trapped_pet() {
    let s = far_pet(45.0).aura_full(FELHUNTER, trap());
    let goal = dispel_walk_goal(&s.decide(HEALER));
    assert!(
        goal.is_some_and(|g| near(g, s.position(FELHUNTER))),
        "{goal:?}"
    );

    let view = far_pet(45.0).decide(HEALER);
    assert_eq!(dispel_walk_goal(&view), None, "{}", describe(&view));
}

/// Two teammates trapped beyond reach, the Felhunter the nearer: the walk goes
/// to the Warrior. The control traps only the Felhunter.
#[test]
fn the_dispel_walk_goes_to_a_non_pet_before_a_nearer_pet() {
    let team = || {
        settled(
            far_pet(40.0).unit(WARRIOR, 1, Warrior).at(0.0, -50.0),
            WARRIOR,
        )
        .aura_full(FELHUNTER, trap())
    };
    let s = team().aura_full(WARRIOR, trap());
    let goal = dispel_walk_goal(&s.decide(HEALER));
    assert!(
        goal.is_some_and(|g| near(g, s.position(WARRIOR))),
        "{goal:?}"
    );

    let s = team();
    let goal = dispel_walk_goal(&s.decide(HEALER));
    assert!(
        goal.is_some_and(|g| near(g, s.position(FELHUNTER))),
        "{goal:?}"
    );
}

/// A trapped pet the dispel reaches comes before a walk to a trapped teammate
/// beyond it: the Priest frees the Felhunter where it stands, and does not
/// walk. The control moves the Felhunter out of reach too, and the Priest
/// walks to the Warrior.
#[test]
fn a_reached_pet_is_freed_before_a_walk_to_an_unreached_teammate() {
    let team = |pet_at: f32| {
        settled(
            far_pet(pet_at).unit(WARRIOR, 1, Warrior).at(0.0, -50.0),
            WARRIOR,
        )
        .aura_full(FELHUNTER, trap())
        .aura_full(WARRIOR, trap())
    };
    let view = team(10.0).decide(HEALER);
    assert_eq!(
        view.chose(),
        Some((DispelMagic, Some(FELHUNTER))),
        "{}",
        describe(&view)
    );
    assert_eq!(dispel_walk_goal(&view), None, "{}", describe(&view));

    let s = team(45.0);
    let goal = dispel_walk_goal(&s.decide(HEALER));
    assert!(
        goal.is_some_and(|g| near(g, s.position(WARRIOR))),
        "{goal:?}"
    );
}

/// A dying Priest does not walk to its trapped Felhunter: the pet waits while
/// any non-pet teammate is dying, the healer included. The control is the
/// same Priest healthy, which walks.
#[test]
fn a_dying_healer_does_not_walk_to_a_trapped_pet() {
    let view = far_pet(45.0)
        .health_of(HEALER, 0.3)
        .aura_full(FELHUNTER, trap())
        .decide(HEALER);
    assert_eq!(dispel_walk_goal(&view), None, "{}", describe(&view));

    let view = far_pet(45.0).aura_full(FELHUNTER, trap()).decide(HEALER);
    assert!(dispel_walk_goal(&view).is_some(), "{}", describe(&view));
}

const WEARER: Who = "wearer";
const ROGUE: Who = "rogue";

/// A Warrior wearing the Medallion and trapped, added after the Felhunter (so
/// entity order alone would reach the pet first), both in reach of the
/// Priest; an enemy Rogue on the Warrior.
fn medallion_beside_pet(pet_trapped: bool) -> Scene {
    let s = far_pet(10.0)
        .unit(WEARER, 1, Warrior)
        .at(0.0, -10.0)
        .on_use_item(WEARER, MEDALLION)
        .aura_full(WEARER, trap())
        .unit(ROGUE, 2, Rogue)
        .at(2.0, -10.0)
        .targeting(WEARER);
    if pet_trapped {
        s.aura_full(FELHUNTER, trap())
    } else {
        s
    }
}

/// The Medallion's wait (`expected_dispeller`) asks `owed_dispel`, which ranks
/// a reached non-pet before a reached pet: beside a trapped Felhunter, the
/// trapped wearer still waits for the Priest's dispel, as it does without one.
#[test]
fn the_medallion_waits_for_a_dispel_beside_a_trapped_pet() {
    for pet_trapped in [true, false] {
        let view = medallion_beside_pet(pet_trapped).decide_held(WEARER);
        assert!(view.items_used.is_empty(), "{pet_trapped}: {view:?}");
        assert!(
            matches!(view.item_decisions.as_slice(),
                [(MEDALLION, Some(RejectionReason::PreconditionUnmet { note }))] if note.contains("waiting for")),
            "{pet_trapped}: {view:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// In a match
// ---------------------------------------------------------------------------

const PARTNER_UNIT: Unit = Unit::new(1, 0);
const WARLOCK_UNIT: Unit = Unit::new(1, 1);
const FELHUNTER_UNIT: Unit = Unit::pet_of(1, 1);
const PALADIN_UNIT: Unit = Unit::new(2, 0);
const WARRIOR_UNIT: Unit = Unit::new(2, 1);

/// A unit held stunned the whole match, so it never closes or casts.
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

/// `partner` and a Warlock against a held Paladin and Warrior, for 4 seconds
/// from the gates; the Felhunter trapped on the gates when `trapped`.
fn felhunter_match(partner: &str, trapped: bool) -> common::staged::Played {
    let config = HeadlessMatchConfig {
        team1: vec![partner.into(), "Warlock".into()],
        team2: vec!["Paladin".into(), "Warrior".into()],
        random_seed: Some(5),
        max_duration_secs: 4.0,
        ..Default::default()
    };
    let mut stage = Stage::new()
        .place(PARTNER_UNIT, 0.0, 0.0)
        .place(WARLOCK_UNIT, 3.0, 0.0)
        .place(FELHUNTER_UNIT, 0.0, 5.0)
        .place(PALADIN_UNIT, 20.0, 0.0)
        .place(WARRIOR_UNIT, 20.0, 4.0);
    for unit in [PALADIN_UNIT, WARRIOR_UNIT] {
        stage = stage.aura(unit, held()).combatant(unit, |c| {
            c.max_health = 20_000.0;
            c.current_health = 20_000.0;
        });
    }
    if trapped {
        stage = stage.aura(FELHUNTER_UNIT, trap());
    }
    play(config, stage)
}

fn lines<'a>(log: &'a str, needle: &str) -> Vec<&'a str> {
    log.lines().filter(|l| l.contains(needle)).collect()
}

/// The Felhunter's track over the staged frames: did it ever stand off its
/// first position?
fn felhunter_moved(played: &common::staged::Played) -> bool {
    let felhunter = played
        .staged_frames()
        .next()
        .and_then(|f| {
            f.combatants
                .iter()
                .find(|(_, c)| c.is_pet && c.team == 1)
                .map(|(e, _)| *e)
        })
        .expect("the Felhunter is in the match");
    let path: Vec<Vec3> = played
        .staged_frames()
        .filter_map(|f| f.combatants.get(&felhunter).map(|c| c.position))
        .collect();
    path.iter().any(|p| !near(*p, path[0]))
}

/// The Priest's Dispel Magic takes Freezing Trap off its Felhunter in a match,
/// and the freed Felhunter walks again: a dispel that left the pet inert would
/// pass every scene above and do nothing. Two controls: no trap draws no
/// Dispel Magic, and a Warrior in the Priest's place frees nobody, so the
/// trapped Felhunter stands where it was trapped.
#[test]
fn dispel_magic_frees_a_trapped_felhunter_in_a_match() {
    let played = felhunter_match("Priest", true);
    let log = &played.log;
    assert_eq!(
        lines(log, "casts Dispel Magic on Team 1 Felhunter").len(),
        1,
        "one Dispel Magic on the Felhunter:\n{log}"
    );
    let removed = lines(log, "removed from Team 1 Felhunter");
    assert_eq!(removed.len(), 1, "{log}");
    assert!(
        removed[0].contains("Freezing Trap removed"),
        "{}\n{log}",
        removed[0]
    );
    assert!(felhunter_moved(&played), "the freed Felhunter never moved");

    let untrapped = felhunter_match("Priest", false);
    assert!(
        lines(&untrapped.log, "Dispel Magic").is_empty(),
        "no Dispel Magic with no trap:\n{}",
        untrapped.log
    );

    let no_dispeller = felhunter_match("Warrior", true);
    assert!(
        lines(&no_dispeller.log, "removed from Team 1 Felhunter").is_empty(),
        "{}",
        no_dispeller.log
    );
    assert!(
        !felhunter_moved(&no_dispeller),
        "a trapped Felhunter nobody frees moved"
    );
}
