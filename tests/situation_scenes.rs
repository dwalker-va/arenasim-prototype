//! Scene tests for the decisions that read a situation
//! (`src/states/play_match/situation.rs`): each runs the real class dispatch
//! for one tick in a constructed scene, paired with its control — the same
//! scene with the situation's one condition removed.
//!
//! These pin what the situation DOES to each decision, which no seed probe
//! can: a situational rule that reorders a priority chain usually leaves a
//! match byte-identical, so neither the match-level suites nor a sweep notice
//! when it is deleted. Each pair below kills exactly that deletion.

use arenasim::states::match_config::CharacterClass::{
    self, Druid, Hunter, Mage, Paladin, Priest, Shaman, Warlock, Warrior,
};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::druid::mana_reserve;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::decision_trace::{MovementTrigger, PlayTag, RejectionReason};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::movement_config::load_movement_config;
use arenasim::states::play_match::situation::{
    MatchPhase, TeamSituation, Tempo, ATTRITION_ONSET_SECS,
};
use arenasim::states::play_match::traps::freezing_trap_aura;
use arenasim::states::play_match::{load_ability_definitions, AbilityType, AuraPending, AuraType};

const WARRIOR: &str = "warrior";
const HUNTER: &str = "hunter";
const PRIEST: &str = "priest";
const ALLY: &str = "ally";
const MAGE: &str = "mage";
const ENEMY_WARRIOR: &str = "enemy warrior";
const ENEMY_PRIEST: &str = "enemy priest";
const PALADIN: &str = "paladin";
const DRUID: &str = "druid";
const ENEMY_WARLOCK: &str = "enemy warlock";

/// Battle Shout's buff, already up, so the Warrior's first priority (shout an
/// unbuffed ally) is out of the way and the damage chain is what decides.
fn shouted(scene: Scene, who: &'static str) -> Scene {
    scene.aura(who, AuraType::AttackPowerIncrease, 120.0)
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; candidates {:?}; movement {:?}",
        view.chosen,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>(),
        view.movement
    )
}

// ---------------------------------------------------------------------------
// Warrior: the Burst play puts Mortal Strike ahead of Rend.
// ---------------------------------------------------------------------------

fn warrior_scene() -> Scene {
    let scene = Scene::new()
        .unit(WARRIOR, 1, Warrior)
        .rage(80.0)
        .targeting(MAGE)
        .unit(MAGE, 2, Mage)
        .at(2.0, 0.0)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(0.0, 20.0)
        .aura(ENEMY_PRIEST, AuraType::Polymorph, 6.0);
    shouted(scene, WARRIOR)
}

#[test]
fn warrior_bursts_mortal_strike_while_the_enemy_healer_is_locked() {
    let mut scene = warrior_scene();
    let locked = scene.decide(WARRIOR);
    assert!(
        locked
            .situation
            .is_some_and(|s| s.enemy_healer_locked.is_some()),
        "the scene's situation: {:?}",
        locked.situation
    );
    assert_eq!(
        locked.chose(),
        Some((AbilityType::MortalStrike, Some(MAGE))),
        "{}",
        describe(&locked)
    );

    // Control: the healer free, everything else the same.
    scene.clear_auras(ENEMY_PRIEST);
    let free = scene.decide(WARRIOR);
    assert!(free
        .situation
        .is_some_and(|s| s.enemy_healer_locked.is_none()));
    assert_eq!(
        free.chose(),
        Some((AbilityType::Rend, Some(MAGE))),
        "{}",
        describe(&free)
    );
}

#[test]
fn a_rooted_healer_is_not_locked() {
    let mut scene = warrior_scene();
    scene.clear_auras(ENEMY_PRIEST);
    let scene = scene.aura(ENEMY_PRIEST, AuraType::Root, 6.0);
    let view = scene.decide(WARRIOR);
    assert!(view
        .situation
        .is_some_and(|s| s.enemy_healer_locked.is_none()));
    assert_eq!(view.chose(), Some((AbilityType::Rend, Some(MAGE))));
}

// ---------------------------------------------------------------------------
// Hunter: the Burst play starts Aimed Shot at safe range — unless the locked
// healer is the Hunter's own target.
// ---------------------------------------------------------------------------

fn hunter_scene(target: &'static str) -> Scene {
    let mut scene = Scene::new()
        .unit(HUNTER, 1, Hunter)
        .unit(MAGE, 2, Mage)
        .at(30.0, 0.0)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(0.0, 30.0)
        .aura(ENEMY_PRIEST, AuraType::Polymorph, 6.0);
    let target = scene.entity(target);
    let hunter = scene.combatant_mut(HUNTER);
    hunter.target = Some(target);
    // Past the opener, so the ordinary rotation (not the opener's own Aimed
    // Shot rule) is what runs.
    hunter.hunter_opened = true;
    scene
}

#[test]
fn hunter_bursts_aimed_shot_while_the_enemy_healer_is_locked() {
    let mut scene = hunter_scene(MAGE);
    let locked = scene.decide(HUNTER);
    assert_eq!(
        locked.chose(),
        Some((AbilityType::AimedShot, Some(MAGE))),
        "{}",
        describe(&locked)
    );

    scene.clear_auras(ENEMY_PRIEST);
    let free = scene.decide(HUNTER);
    let control = free.chose();
    assert!(
        control.is_some_and(|(ability, _)| ability != AbilityType::AimedShot),
        "{}",
        describe(&free)
    );
}

#[test]
fn hunter_does_not_burst_the_locked_healer_itself() {
    // The healer is the target: bursting it would break its own breakable CC.
    let scene = hunter_scene(ENEMY_PRIEST);
    let view = scene.decide(HUNTER);
    assert!(view
        .situation
        .is_some_and(|s| s.enemy_healer_locked.is_some()));
    assert_ne!(
        view.chose().map(|(ability, _)| ability),
        Some(AbilityType::AimedShot),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Priest: Mana Burn spends surplus GCDs only, and a locked enemy healer does
// not change that: a team at 80% is healed, locked or not, and a topped team
// burns, locked or not.
// ---------------------------------------------------------------------------

/// Fortitude already up on both allies, Weakened Soul on both (no Shield to
/// spend), and Psychic Scream on cooldown (so no offensive dip walks the
/// Priest at the enemy healer): the heal-or-burn rungs are what decides.
fn priest_scene() -> Scene {
    Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
        .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
        .cooldown(PRIEST, AbilityType::PsychicScream, 20.0)
        .unit(ALLY, 1, Warrior)
        .at(5.0, 0.0)
        .health(0.8)
        .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
        .aura(ALLY, AuraType::WeakenedSoul, 15.0)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(0.0, 28.0)
        .aura(ENEMY_PRIEST, AuraType::Polymorph, 6.0)
        .unit(MAGE, 2, Mage)
        .at(-28.0, 0.0)
}

#[test]
fn priest_heals_before_burning_whether_or_not_the_enemy_healer_is_locked() {
    let mut scene = priest_scene();
    let locked = scene.decide(PRIEST);
    assert!(
        locked
            .situation
            .is_some_and(|s| s.enemy_healer_locked.is_some()),
        "{}",
        describe(&locked)
    );
    assert_eq!(
        locked.chose(),
        Some((AbilityType::FlashHeal, Some(ALLY))),
        "{}",
        describe(&locked)
    );

    scene.clear_auras(ENEMY_PRIEST);
    let free = scene.decide(PRIEST);
    assert!(
        free.situation
            .is_some_and(|s| s.enemy_healer_locked.is_none()),
        "{}",
        describe(&free)
    );
    assert_eq!(
        free.chose(),
        Some((AbilityType::FlashHeal, Some(ALLY))),
        "{}",
        describe(&free)
    );
}

/// The control: the same scene with the team topped burns, locked or not, so
/// the heal above is the health floor's doing and not an unreachable burn.
#[test]
fn priest_burns_mana_with_a_topped_team_whether_or_not_the_enemy_healer_is_locked() {
    let mut scene = priest_scene();
    scene.set_health(ALLY, 1.0);
    let locked = scene.decide(PRIEST);
    assert_eq!(
        locked.chose(),
        Some((AbilityType::ManaBurn, Some(ENEMY_PRIEST))),
        "{}",
        describe(&locked)
    );

    scene.clear_auras(ENEMY_PRIEST);
    let free = scene.decide(PRIEST);
    assert_eq!(
        free.chose(),
        Some((AbilityType::ManaBurn, Some(ENEMY_PRIEST))),
        "{}",
        describe(&free)
    );
}

// ---------------------------------------------------------------------------
// Healer posture: a team pressing its lead stops hiding — `cover_pull` is
// zeroed while the tempo is Pressing.
// ---------------------------------------------------------------------------

/// A Priest an enemy Warrior is closing on, with a pillar between them: every
/// step straight away from the Warrior is out of its sight, so cover is on
/// offer. `enemy_health` sets the tempo.
fn pressured_priest_scene(enemy_health: f32) -> Scene {
    let pillar = ObstacleVolume::Cylinder {
        center_xz: bevy::prelude::Vec2::new(-4.0, 0.0),
        radius: 1.5,
        base_y: 0.0,
        height: 10.0,
    };
    Scene::new()
        .obstacles(vec![pillar])
        .unit(PRIEST, 1, Priest)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
        .unit(ALLY, 1, Warrior)
        .at(0.0, -10.0)
        .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
        .unit(ENEMY_WARRIOR, 2, Warrior)
        .at(-8.0, 0.0)
        .health(enemy_health)
        .targeting(PRIEST)
        .unit(MAGE, 2, Mage)
        .at(-30.0, 10.0)
        .health(enemy_health)
}

#[test]
fn a_pressing_healer_stops_pulling_toward_cover() {
    let pressing = pressured_priest_scene(0.6).decide(PRIEST);
    let even = pressured_priest_scene(1.0).decide(PRIEST);
    assert_eq!(pressing.situation.map(|s| s.tempo), Some(Tempo::Pressing));
    assert_eq!(even.situation.map(|s| s.tempo), Some(Tempo::Even));
    for view in [&pressing, &even] {
        assert!(
            view.movement
                .iter()
                .any(|m| m.trigger == MovementTrigger::PressuredEnter),
            "the scene pressures the Priest: {}",
            describe(view)
        );
    }
    assert!(
        even.scorer_term("cover_pull").is_some_and(|v| v > 0.0),
        "control: the level Priest steps into cover — {}",
        describe(&even)
    );
    assert_eq!(
        pressing.scorer_term("cover_pull"),
        Some(0.0),
        "{}",
        describe(&pressing)
    );
}

// ---------------------------------------------------------------------------
// Healer posture: a teammate dying in heal range stops the healer hiding —
// `cover_pull` is zeroed while the team situation has a dying teammate in
// reach (`teammate_needs_saving`).
// ---------------------------------------------------------------------------

#[test]
fn a_healer_with_a_dying_teammate_in_reach_stops_pulling_toward_cover() {
    // Level on health otherwise: the enemy at full, so only the teammate's
    // health separates the pair (a dying teammate leaves the team trailing,
    // never pressing).
    let mut scene = pressured_priest_scene(1.0);
    scene.set_health(ALLY, 0.3);
    let dying = scene.decide(PRIEST);
    assert_eq!(dying.situation.map(|s| s.tempo), Some(Tempo::Trailing));
    assert!(
        dying
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::PressuredEnter),
        "the scene pressures the Priest: {}",
        describe(&dying)
    );
    assert_eq!(
        dying.scorer_term("cover_pull"),
        Some(0.0),
        "{}",
        describe(&dying)
    );

    // Control: the teammate healthy, and the Priest steps into cover.
    scene.set_health(ALLY, 1.0);
    let healthy = scene.decide(PRIEST);
    assert!(
        healthy.scorer_term("cover_pull").is_some_and(|v| v > 0.0),
        "{}",
        describe(&healthy)
    );
}

// ---------------------------------------------------------------------------
// Healer posture: the PRESSURED trigger is the situation's `pressured` fact
// for the healer — an enemy it sees, near enough that intent matters.
// ---------------------------------------------------------------------------

/// A Priest with an enemy Warrior 8yd off, targeting `whom`.
fn targeted_priest_scene(whom: &'static str) -> Scene {
    Scene::new()
        .unit(PRIEST, 1, Priest)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
        .unit(ALLY, 1, Warrior)
        .at(0.0, -10.0)
        .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
        .unit(ENEMY_WARRIOR, 2, Warrior)
        .at(-8.0, 0.0)
        .targeting(whom)
        .unit(MAGE, 2, Mage)
        .at(-30.0, 10.0)
}

#[test]
fn a_healer_an_enemy_closes_on_is_pressured() {
    let on_me = targeted_priest_scene(PRIEST).decide(PRIEST);
    assert!(
        on_me
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::PressuredEnter),
        "{}",
        describe(&on_me)
    );

    // Control: the same Warrior, the same spot, on the teammate instead.
    let elsewhere = targeted_priest_scene(ALLY).decide(PRIEST);
    assert!(
        !elsewhere
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::PressuredEnter),
        "{}",
        describe(&elsewhere)
    );
}

// ---------------------------------------------------------------------------
// The ally-in-danger window: a teammate both dying and pressured, named on
// the decision a teammate makes.
// ---------------------------------------------------------------------------

#[test]
fn a_dying_teammate_under_attack_is_in_danger() {
    let mut scene = targeted_priest_scene(ALLY);
    scene.set_health(ALLY, 0.3);
    let danger = scene.decide(PRIEST);
    assert_eq!(
        danger
            .situation
            .and_then(|s| s.ally_in_danger)
            .map(|w| w.subject),
        Some(scene.entity(ALLY).index()),
        "{}",
        describe(&danger)
    );

    // Control: dying, but nobody on it.
    let mut scene = targeted_priest_scene(PRIEST);
    scene.set_health(ALLY, 0.3);
    let unpressed = scene.decide(PRIEST);
    assert_eq!(
        unpressed.situation.and_then(|s| s.ally_in_danger),
        None,
        "{}",
        describe(&unpressed)
    );
}

// ---------------------------------------------------------------------------
// What counts as closing (AS-292): under `TeamPlan` an enemy HEALER between
// its 28yd preferred range and the 30yd intent radius is not closing — it
// moves by its posture, not toward its target — so it is no pressure; a
// ranged DPS in the same spot, and a melee, still are. `Legacy` keeps the old
// rule.
// ---------------------------------------------------------------------------

const ENEMY: &str = "enemy";

/// A Priest under `profile`, its Warrior teammate 10yd off, and one enemy of
/// `class` targeting `whom` from `distance` yards straight out from it.
fn banded_scene(
    profile: AiProfile,
    class: CharacterClass,
    whom: &'static str,
    distance: f32,
) -> Scene {
    let at_z = if whom == ALLY { -10.0 } else { 0.0 };
    Scene::new()
        .ai_profile(1, profile)
        .unit(PRIEST, 1, Priest)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
        .unit(ALLY, 1, Warrior)
        .at(0.0, -10.0)
        .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
        .unit(ENEMY, 2, class)
        .at(-distance, at_z)
        .targeting(whom)
}

fn pressured_enter(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::PressuredEnter)
}

fn is_pressured(scene: &mut Scene, who: &'static str) -> bool {
    let entity = scene.entity(who);
    scene.reclassify().for_team(1).is_pressured(entity)
}

#[test]
fn an_enemy_healer_in_its_closing_band_does_not_pressure_a_teamplan_healer() {
    let band = Priest.preferred_range() + 1.0;
    for class in [Priest, Shaman, Druid] {
        let mut scene = banded_scene(AiProfile::TeamPlan, class, PRIEST, band);
        let view = scene.decide(PRIEST);
        assert!(!pressured_enter(&view), "{class:?}: {}", describe(&view));
        assert!(!is_pressured(&mut scene, PRIEST), "{class:?}");
    }
    // Controls, the same spot: a ranged DPS still closing to its own 28yd...
    let mut warlock = banded_scene(AiProfile::TeamPlan, Warlock, PRIEST, band);
    let view = warlock.decide(PRIEST);
    assert!(
        pressured_enter(&view),
        "a Warlock closing: {}",
        describe(&view)
    );
    assert!(is_pressured(&mut warlock, PRIEST));
    // ...a melee inside the intent radius...
    let view = banded_scene(AiProfile::TeamPlan, Warrior, PRIEST, band).decide(PRIEST);
    assert!(pressured_enter(&view), "a Warrior: {}", describe(&view));
    // ...and the enemy healer itself, once inside the danger radius.
    let view = banded_scene(AiProfile::TeamPlan, Priest, PRIEST, 10.0).decide(PRIEST);
    assert!(
        pressured_enter(&view),
        "a Priest at 10yd: {}",
        describe(&view)
    );
}

#[test]
fn an_enemy_healer_in_its_closing_band_still_pressures_under_legacy() {
    let band = Priest.preferred_range() + 1.0;
    let mut legacy = banded_scene(AiProfile::Legacy, Priest, PRIEST, band);
    assert!(is_pressured(&mut legacy, PRIEST));
    let mut teamplan = banded_scene(AiProfile::TeamPlan, Priest, PRIEST, band);
    assert!(!is_pressured(&mut teamplan, PRIEST));
}

#[test]
fn an_enemy_healer_in_its_closing_band_puts_no_teamplan_teammate_in_danger() {
    let band = Priest.preferred_range() + 1.0;
    let danger = |class| {
        let mut scene = banded_scene(AiProfile::TeamPlan, class, ALLY, band);
        scene.set_health(ALLY, 0.3);
        let view = scene.decide(PRIEST);
        (
            view.situation
                .and_then(|s| s.ally_in_danger)
                .map(|w| w.subject),
            scene.entity(ALLY).index(),
            describe(&view),
        )
    };
    let (window, _, why) = danger(Priest);
    assert_eq!(window, None, "{why}");
    // Control: a Warlock in the same spot puts the teammate in danger.
    let (window, ally, why) = danger(Warlock);
    assert_eq!(window, Some(ally), "{why}");
}

// ---------------------------------------------------------------------------
// The dispel walk's dying-first rule: no walk to a teammate held in urgent CC
// while ANOTHER teammate is dying (`another_teammate_dying`).
// ---------------------------------------------------------------------------

/// A Paladin, a teammate Warrior in a Freezing Trap 40yd off (beyond
/// Cleanse's 30), and a teammate Mage beside the Paladin at `mage_health`.
fn trapped_teammate_scene(mage_health: f32) -> Scene {
    let hunter = Scene::new().unit(HUNTER, 2, Hunter).at(60.0, 30.0);
    let owner = hunter.entity(HUNTER);
    hunter
        .unit(PALADIN, 1, Paladin)
        .aura(PALADIN, AuraType::MaxHealthIncrease, 600.0)
        .unit(WARRIOR, 1, Warrior)
        .at(40.0, 0.0)
        .aura_full(WARRIOR, freezing_trap_aura(owner))
        .unit(MAGE, 1, Mage)
        .at(3.0, 0.0)
        .health(mage_health)
}

#[test]
fn the_dispel_walk_yields_to_another_dying_teammate() {
    let dying = trapped_teammate_scene(0.3).decide(PALADIN);
    assert!(
        !dying
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::DispelChase),
        "{}",
        describe(&dying)
    );

    // Control: the Mage healthy, and the Paladin walks to the trap.
    let healthy = trapped_teammate_scene(1.0).decide(PALADIN);
    assert!(
        healthy
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::DispelChase),
        "{}",
        describe(&healthy)
    );
}

// ---------------------------------------------------------------------------
// The Druid's mana governor: a reserve held back through the Engaged phase,
// spent down to nothing by the Attrition onset.
// ---------------------------------------------------------------------------

/// Twenty seconds after the gates: well inside the Engaged phase.
const ENGAGED_SECS: f32 = 20.0;

/// A Druid with a teammate worth a proactive heal (attacked, at 75%) and one
/// mana over the governor's reserve at [`ENGAGED_SECS`], deciding at `now`.
/// Innervate is down, so the pool cannot refill.
fn governed_druid_scene(now: f32) -> Scene {
    let defs = load_ability_definitions().expect("abilities.ron loads");
    let mark = defs
        .get(&AbilityType::MarkOfTheWild)
        .expect("Mark of the Wild is defined");
    let scene = Scene::new().now(now).unit(DRUID, 1, Druid);
    let druid = scene.entity(DRUID);
    let scene = scene.unit(ALLY, 1, Warrior).at(5.0, 0.0).health(0.75);
    let ally = scene.entity(ALLY);
    let mark_on = |target| {
        AuraPending::from_ability(target, druid, mark)
            .expect("Mark of the Wild applies an aura")
            .aura
    };
    let (druid_mark, ally_mark) = (mark_on(druid), mark_on(ally));
    let mut scene = scene
        .aura_full(DRUID, druid_mark)
        .aura_full(ALLY, ally_mark)
        .cooldown(DRUID, AbilityType::Innervate, 60.0)
        .unit(ENEMY_WARRIOR, 2, Warrior)
        .at(6.0, 0.0)
        .targeting(ALLY)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(0.0, 35.0);
    // One mana over the Engaged reserve: any proactive cast dips into it
    // while that reserve is held.
    let held = TeamSituation {
        since_gates: ENGAGED_SECS,
        ..Default::default()
    };
    let druid = scene.combatant_mut(DRUID);
    druid.current_mana = mana_reserve(druid.max_mana, &held) + 1.0;
    scene
}

#[test]
fn the_druid_governor_holds_its_reserve_until_attrition() {
    let engaged = governed_druid_scene(ENGAGED_SECS).decide(DRUID);
    assert_eq!(
        engaged.situation.map(|s| s.phase),
        Some(MatchPhase::Engaged)
    );
    assert!(
        engaged.candidates.iter().any(|c| matches!(
            &c.reason,
            Some(RejectionReason::PreconditionUnmet { note }) if note.starts_with("mana governor")
        )),
        "{}",
        describe(&engaged)
    );
    assert_ne!(
        engaged.chose().map(|(ability, _)| ability),
        Some(AbilityType::Rejuvenation),
        "{}",
        describe(&engaged)
    );

    // Control: the same Druid, the same mana, once dampening has reached half.
    let attrition = governed_druid_scene(ATTRITION_ONSET_SECS + 1.0).decide(DRUID);
    assert_eq!(
        attrition.situation.map(|s| s.phase),
        Some(MatchPhase::Attrition)
    );
    assert_eq!(
        attrition.chose(),
        Some((AbilityType::Rejuvenation, Some(ALLY))),
        "{}",
        describe(&attrition)
    );
}

// ---------------------------------------------------------------------------
// Warrior tempo reset: a Warrior rooted off its target regroups on its healer
// — unless its team is pressing, which keeps it chasing.
// ---------------------------------------------------------------------------

/// A rooted Warrior 15yd from the enemy Mage it targets, Charge down, its
/// Priest behind it. `enemy_health` sets the tempo.
fn rooted_warrior_scene(enemy_health: f32) -> Scene {
    let scene = Scene::new()
        .unit(WARRIOR, 1, Warrior)
        .rage(20.0)
        .targeting(MAGE)
        .aura(WARRIOR, AuraType::Root, 3.0)
        .cooldown(WARRIOR, AbilityType::Charge, 10.0)
        .unit(PRIEST, 1, Priest)
        .at(-10.0, 0.0)
        .unit(MAGE, 2, Mage)
        .at(15.0, 0.0)
        .health(enemy_health)
        .unit(ENEMY_PRIEST, 2, Priest)
        .at(15.0, 20.0)
        .health(enemy_health);
    shouted(scene, WARRIOR)
}

#[test]
fn a_pressing_warrior_does_not_reset_tempo() {
    let even = rooted_warrior_scene(1.0).decide(WARRIOR);
    assert_eq!(even.situation.map(|s| s.tempo), Some(Tempo::Even));
    assert!(
        even.movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::MeleeReset),
        "{}",
        describe(&even)
    );

    let pressing = rooted_warrior_scene(0.6).decide(WARRIOR);
    assert_eq!(pressing.situation.map(|s| s.tempo), Some(Tempo::Pressing));
    assert!(
        !pressing
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::MeleeReset),
        "{}",
        describe(&pressing)
    );
}

// ---------------------------------------------------------------------------
// Tempo hysteresis: under `TeamPlan` a pressing team keeps pressing while its
// lead wobbles inside the band between `press_release_margin` and
// `press_advantage_margin`, and the Warrior reads the latched tempo. `Legacy`
// has no band, so the same wobble flips it every time.
// ---------------------------------------------------------------------------

/// The enemy health fraction (both enemies) that gives the rooted-Warrior
/// scene's team 1 a lead of `lead`: team 1 sits at full health (2.0) and the
/// enemies sum to 2h, so the lead is 2 - 2h.
fn enemy_health_for_lead(lead: f32) -> f32 {
    1.0 - lead / 2.0
}

/// Each team's tempo after every step of a wobble.
struct Wobble {
    team1: Vec<Tempo>,
    team2: Vec<Tempo>,
    /// The Warrior's (team 1's) decision at the last, inside-the-band step.
    view: DecisionView,
}

/// How many steps of `tempos` changed tempo.
fn flips(tempos: &[Tempo]) -> usize {
    tempos.windows(2).filter(|w| w[0] != w[1]).count()
}

/// Run the rooted-Warrior scene through a lead over the onset, then wobble it
/// into the band and back over the onset `wobbles` times, reclassifying at
/// each step, and end inside the band. Team 1 leads and team 2 trails by the
/// same amount, each team under its own profile, so one scene shows both
/// teams' latches side by side.
fn wobble_tempo(team1: AiProfile, team2: AiProfile, wobbles: usize) -> Wobble {
    let shared = load_movement_config().expect("movement.ron loads").shared;
    let (onset, release) = (shared.press_advantage_margin, shared.press_release_margin);
    assert!(
        release < onset,
        "movement.ron must give tempo a band: release {release} vs onset {onset}"
    );
    // A quarter of the band clear of the onset, and mid-band.
    let band = onset - release;
    let over = enemy_health_for_lead(onset + band / 4.0);
    let inside = enemy_health_for_lead(release + band / 2.0);

    let mut scene = rooted_warrior_scene(over)
        .ai_profile(1, team1)
        .ai_profile(2, team2);
    let tempos = |scene: &mut Scene| {
        let s = scene.reclassify();
        (s.for_team(1).tempo, s.for_team(2).tempo)
    };
    let mut steps = vec![tempos(&mut scene)];
    let mut healths = Vec::new();
    for _ in 0..wobbles {
        healths.extend([inside, over]);
    }
    healths.push(inside);
    for h in healths {
        scene.set_health(MAGE, h);
        scene.set_health(ENEMY_PRIEST, h);
        steps.push(tempos(&mut scene));
    }
    Wobble {
        team1: steps.iter().map(|s| s.0).collect(),
        team2: steps.iter().map(|s| s.1).collect(),
        view: scene.decide(WARRIOR),
    }
}

/// The Warrior's decision included a tempo reset.
fn resets(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::MeleeReset)
}

#[test]
fn a_lead_wobbling_inside_the_band_keeps_a_teamplan_team_pressing() {
    // Team 1 TeamPlan, team 2 Legacy: the band holds team 1 Pressing, and the
    // Warrior reads the latched tempo and keeps chasing...
    let w = wobble_tempo(AiProfile::TeamPlan, AiProfile::Legacy, 3);
    assert!(
        w.team1.iter().all(|t| *t == Tempo::Pressing),
        "TeamPlan tempo flipped inside the band: {:?}",
        w.team1
    );
    assert_eq!(w.view.situation.map(|s| s.tempo), Some(Tempo::Pressing));
    assert!(!resets(&w.view), "{}", describe(&w.view));
    // ...while, in the same match, the Legacy team has no band: it flips
    // between Trailing and Even at every step.
    assert_eq!(
        flips(&w.team2),
        w.team2.len() - 1,
        "the Legacy team of a mixed match flips every step: {:?}",
        w.team2
    );
    assert_eq!(w.team2.last(), Some(&Tempo::Even));

    // Swapped: team 1 Legacy flips every step and its Warrior resets inside
    // the band, while team 2 under TeamPlan holds Trailing throughout.
    let w = wobble_tempo(AiProfile::Legacy, AiProfile::TeamPlan, 3);
    assert_eq!(
        flips(&w.team1),
        w.team1.len() - 1,
        "Legacy flips every step: {:?}",
        w.team1
    );
    assert_eq!(w.view.situation.map(|s| s.tempo), Some(Tempo::Even));
    assert!(resets(&w.view), "{}", describe(&w.view));
    assert!(
        w.team2.iter().all(|t| *t == Tempo::Trailing),
        "a TeamPlan team 2 holds Trailing inside the band: {:?}",
        w.team2
    );
}

#[test]
fn a_teamplan_team_stops_pressing_below_the_release() {
    let shared = load_movement_config().expect("movement.ron loads").shared;
    let (onset, release) = (shared.press_advantage_margin, shared.press_release_margin);
    let mut scene = rooted_warrior_scene(enemy_health_for_lead((onset + 1.0) / 2.0))
        .ai_profile(1, AiProfile::TeamPlan);
    assert_eq!(scene.reclassify().for_team(1).tempo, Tempo::Pressing);
    let below = enemy_health_for_lead(release / 2.0);
    scene.set_health(MAGE, below);
    scene.set_health(ENEMY_PRIEST, below);
    assert_eq!(scene.reclassify().for_team(1).tempo, Tempo::Even);
    let view = scene.decide(WARRIOR);
    assert!(
        view.movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::MeleeReset),
        "released, the Warrior resets again: {}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// The ally-in-danger debounce: firm pressure opens a window on the first
// frame; pressure only from the edge of the test opens one after
// `danger_dwell`; a window survives a blip shorter than `danger_release`, and
// ends once the release has truly run. One frame of the scene is 1/60s.
// ---------------------------------------------------------------------------

const FRAME: f32 = 1.0 / 60.0;

/// The targeted-Priest scene with its ally dying and the enemy Warrior on it,
/// both teams under `TeamPlan` (so the Priest's Peel play can answer), at the
/// sim time `now`.
fn danger_scene(now: f32) -> Scene {
    let mut scene = targeted_priest_scene(ALLY)
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .now(now);
    scene.set_health(ALLY, 0.3);
    scene
}

/// Reclassify `scene` at `now` — one more frame of history — and report its
/// team-1 window as (subject index, onset).
fn window_at(scene: Scene, now: f32) -> (Scene, Option<(u32, f32)>) {
    let mut scene = scene.now(now);
    let w = scene
        .reclassify()
        .for_team(1)
        .ally_in_danger
        .map(|w| (w.subject.index(), w.since));
    (scene, w)
}

#[test]
fn a_warrior_on_a_dying_ally_opens_a_window_on_the_first_frame() {
    // History: the ally healthy, the enemy Warrior on it from 13yd — well
    // inside the pressure test, so FIRM pressure.
    let now = 30.0;
    let mut scene = danger_scene(now);
    scene.set_health(ALLY, 0.9);
    let (mut scene, w) = window_at(scene, now);
    assert_eq!(w, None);
    // The first frame it is dying, the window is open and the Priest's play
    // is the peel: no dwell.
    scene.set_health(ALLY, 0.3);
    let (scene, w) = window_at(scene, now + FRAME);
    assert_eq!(w, Some((scene.entity(ALLY).index(), now + FRAME)));
    let view = scene.decide(PRIEST);
    assert!(
        matches!(
            view.situation.and_then(|s| s.play),
            Some(PlayTag::Peel { .. })
        ),
        "{}",
        describe(&view)
    );
}

#[test]
fn a_one_frame_edge_danger_opens_no_window_and_a_lasting_one_does() {
    let dwell = load_movement_config()
        .expect("movement.ron")
        .shared
        .danger_dwell;
    assert!(
        dwell > 2.0 * FRAME,
        "movement.ron must give the window a dwell"
    );
    // The enemy Warrior is on the Priest; an enemy Warlock targets the ally
    // from 29.5yd — still closing on it (beyond its 28yd preferred range)
    // inside the 30yd intent radius, so it pressures, but only from the EDGE
    // of the test. History: the ally
    // healthy.
    let mut now = 30.0;
    let mut scene = Scene::new()
        .ai_profile(1, AiProfile::TeamPlan)
        .ai_profile(2, AiProfile::TeamPlan)
        .now(now)
        .unit(PRIEST, 1, Priest)
        .aura(PRIEST, AuraType::MaxHealthIncrease, 600.0)
        .unit(ALLY, 1, Warrior)
        .at(0.0, -10.0)
        .aura(ALLY, AuraType::MaxHealthIncrease, 600.0)
        .unit(ENEMY_WARRIOR, 2, Warrior)
        .at(-8.0, 0.0)
        .targeting(PRIEST)
        .unit(ENEMY_WARLOCK, 2, Warlock)
        .at(0.0, -39.5)
        .targeting(ALLY);
    scene.set_health(ALLY, 0.9);
    let (mut scene, w) = window_at(scene, now);
    assert_eq!(w, None);
    let ally = scene.entity(ALLY).index();
    // One frame dying under it, then healed back: no window, then or after.
    scene.set_health(ALLY, 0.3);
    now += FRAME;
    let (mut scene, w) = window_at(scene, now);
    assert_eq!(w, None, "an edge danger opened a window on its first frame");
    scene.set_health(ALLY, 0.9);
    for _ in 0..30 {
        now += FRAME;
        let (s, w) = window_at(scene, now);
        scene = s;
        assert_eq!(w, None, "a one-frame danger opened a window");
    }
    let view = scene.decide(PRIEST);
    assert_eq!(
        view.situation.and_then(|s| s.play),
        None,
        "{}",
        describe(&view)
    );

    // Control: the same danger held opens a window once the dwell has run,
    // and the Priest's play is the peel.
    scene.set_health(ALLY, 0.3);
    let start = now + FRAME;
    let mut opened = None;
    for _ in 0..60 {
        now += FRAME;
        let (s, w) = window_at(scene, now);
        scene = s;
        if w.is_some() {
            opened = w;
            break;
        }
    }
    let (subject, since) = opened.expect("a held danger opens a window");
    assert_eq!(subject, ally);
    assert!(
        since - start >= dwell - 1e-3,
        "opened after {}s",
        since - start
    );
    assert!(
        since - start < dwell + 2.0 * FRAME,
        "opened after {}s",
        since - start
    );
    let view = scene.decide(PRIEST);
    assert!(
        matches!(
            view.situation.and_then(|s| s.play),
            Some(PlayTag::Peel { .. })
        ),
        "{}",
        describe(&view)
    );
}

#[test]
fn a_one_frame_blip_does_not_end_a_danger_window_and_a_real_release_does() {
    let release = load_movement_config()
        .expect("movement.ron")
        .shared
        .danger_release;
    assert!(
        release > 2.0 * FRAME,
        "movement.ron must give the window a release"
    );
    // A window already open: classified with no history, the standing danger
    // is a window at once.
    let mut now = 30.0;
    let (mut scene, opened) = window_at(danger_scene(now), now);
    let opened = opened.expect("the standing danger is a window");

    // One frame healed over the threshold, then dying again: the same window,
    // same onset, throughout.
    scene.set_health(ALLY, 0.6);
    now += FRAME;
    let (mut scene, w) = window_at(scene, now);
    assert_eq!(w, Some(opened), "a one-frame heal ended the window");
    scene.set_health(ALLY, 0.3);
    now += FRAME;
    let (scene, w) = window_at(scene, now);
    assert_eq!(w, Some(opened), "the blip split the window in two");
    // One frame with the Warrior off the ally: the same.
    let scene = scene.target(ENEMY_WARRIOR, PRIEST);
    now += FRAME;
    let (scene, w) = window_at(scene, now);
    assert_eq!(w, Some(opened), "a one-frame retarget ended the window");
    let scene = scene.target(ENEMY_WARRIOR, ALLY);
    now += FRAME;
    let (mut scene, w) = window_at(scene, now);
    assert_eq!(w, Some(opened));

    // Healed for good: the window holds through the release, then ends.
    scene.set_health(ALLY, 0.6);
    let cleared = now + FRAME;
    let mut ended = None;
    for _ in 0..60 {
        now += FRAME;
        let (s, w) = window_at(scene, now);
        scene = s;
        match w {
            Some(w) => assert_eq!(w, opened),
            None => {
                ended = Some(now);
                break;
            }
        }
    }
    let ended = ended.expect("a cleared danger ends its window");
    assert!(
        ended - cleared >= release - 1e-3,
        "ended after {}s",
        ended - cleared
    );
    assert!(
        ended - cleared < release + 2.0 * FRAME,
        "ended after {}s",
        ended - cleared
    );
    let view = scene.decide(PRIEST);
    assert_eq!(view.situation.and_then(|s| s.ally_in_danger), None);
    assert_eq!(
        view.situation.and_then(|s| s.play),
        None,
        "{}",
        describe(&view)
    );
}
