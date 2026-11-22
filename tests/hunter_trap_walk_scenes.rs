//! Scenes for the Hunter's trap walk (AS-498, `hunter_dip::evaluate_trap_walk`):
//! under `TeamPlan` the Hunter walks toward the enemy it will Freezing Trap
//! whenever the walk costs no melee contact that kiting away would have
//! prevented, and throws from range the moment the landing is clean.
//!
//! Each pair is one scene and its control with one condition changed:
//!
//! - the walk: the enemy healer beyond throw range and the enemy Warrior
//!   chasing the Hunter from BEHIND (walking at the healer also runs from the
//!   Warrior) — the Hunter walks; with the Warrior AHEAD, between the Hunter
//!   and the healer, it would run into it, and the Hunter holds;
//! - the teammate rule: the walk needs a DPS teammate attacking someone, who
//!   uses the window the freeze buys; with that teammate attacking nobody, the
//!   Hunter holds;
//! - the outlast trap: the team is killing the enemy Druid, which cannot free
//!   its Warrior from a Freezing Trap, so the Warrior on the Hunter is frozen;
//!   beside a Priest, which can, it is not;
//! - the gate: the walk scene under `Legacy`, and under `TeamPlan` with the
//!   walk withheld (`team1_trap_walk: false`), walks nowhere.

use arenasim::states::match_config::CharacterClass;
use arenasim::states::match_config::CharacterClass::{Druid, Hunter, Priest, Warrior};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::hunter::{
    AIMED_SHOT_HELD_FOR_WALK, FROST_TRAP_HELD_FOR_WALK,
};
use arenasim::states::play_match::class_ai::hunter_dip::{
    TRAP_WALK_MELEE, TRAP_WALK_NO_SIGHT, TRAP_WALK_NO_TEAMMATE, TRAP_WALK_REARM, TRAP_WALK_TOO_FAR,
    TRAP_WALK_UNCLEAN, TRAP_WALK_VICTIM_HELD, TRAP_WALK_WALKING,
};
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{KitePosture, MovementGoal};
use arenasim::states::play_match::decision_trace::{MovementTrigger, RejectionReason};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::situation::{ClassSet, Play};
use arenasim::states::play_match::AuraType;
use bevy::prelude::Vec2;

const ME: &str = "me";
const PARTNER: &str = "partner";
const WARRIOR: &str = "enemy warrior";
const HEALER: &str = "enemy healer";

const NOW: f32 = 30.0;

/// The Hunter's `commit_window`, read from the shipped `movement.ron`.
fn commit_window() -> f32 {
    arenasim::states::play_match::movement_config::load_movement_config()
        .expect("movement.ron loads")
        .hunter
        .commit_window
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; movement {:?}; directive {:?}; candidates {:?}",
        view.chosen,
        view.movement,
        view.directive.map(|d| d.goal),
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// The trap walk notes the decision recorded on Freezing Trap.
fn trap_notes(view: &DecisionView) -> Vec<String> {
    view.candidates
        .iter()
        .filter(|c| c.ability == AbilityType::FreezingTrap)
        .filter_map(|c| match &c.reason {
            Some(RejectionReason::PreconditionUnmet { note }) if note.starts_with("trap walk") => {
                Some(note.clone())
            }
            _ => None,
        })
        .collect()
}

fn walked(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::TrapWalk)
}

/// The Hunter at the origin under `profile`, its Warrior partner attacking
/// the enemy Warrior (or nobody, with `partner_attacks` false), the enemy
/// Priest 40yd off along +X (beyond throw range), and the enemy Warrior at
/// `warrior_x` on the axis, chasing the Hunter.
fn chase(profile: AiProfile, warrior_x: f32, partner_attacks: bool) -> Scene {
    let scene = Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .kite_posture(ME, KitePosture::new(NOW))
        .unit(PARTNER, 1, Warrior)
        .at(-30.0, 10.0);
    let scene = if partner_attacks {
        scene.targeting(WARRIOR)
    } else {
        scene
    };
    scene
        .unit(WARRIOR, 2, Warrior)
        .at(warrior_x, 0.0)
        .facing(-warrior_x.signum(), 0.0)
        .targeting(ME)
        .unit(HEALER, 2, Priest)
        .at(40.0, 0.0)
}

#[test]
fn the_hunter_walks_at_the_healer_when_the_chaser_is_behind() {
    let view = chase(AiProfile::TeamPlan, -15.0, true).decide(ME);
    assert!(walked(&view), "{}", describe(&view));
    assert!(
        matches!(
            view.directive.map(|d| d.goal),
            Some(MovementGoal::Entity(_))
        ),
        "{}",
        describe(&view)
    );
    // One Freezing Trap note, the walk's: the pressure trap in the closing
    // band waits for the walk rather than repeating why it cannot throw.
    assert_eq!(
        notes(&view, AbilityType::FreezingTrap),
        vec![TRAP_WALK_WALKING.to_string()],
        "{}",
        describe(&view)
    );
}

#[test]
fn the_hunter_does_not_walk_into_a_chaser_between_it_and_the_healer() {
    let view = chase(AiProfile::TeamPlan, 15.0, true).decide(ME);
    assert!(!walked(&view), "{}", describe(&view));
    assert_eq!(
        trap_notes(&view),
        vec![TRAP_WALK_MELEE.to_string()],
        "{}",
        describe(&view)
    );
}

#[test]
fn the_hunter_holds_when_no_dps_teammate_would_use_the_freeze() {
    let view = chase(AiProfile::TeamPlan, -15.0, false).decide(ME);
    assert!(!walked(&view), "{}", describe(&view));
    assert_eq!(
        trap_notes(&view),
        vec![TRAP_WALK_NO_TEAMMATE.to_string()],
        "{}",
        describe(&view)
    );
}

#[test]
fn legacy_has_no_trap_walk() {
    let view = chase(AiProfile::Legacy, -15.0, true).decide(ME);
    assert!(!walked(&view), "{}", describe(&view));
    assert!(trap_notes(&view).is_empty(), "{}", describe(&view));
}

#[test]
fn a_team_with_the_walk_withheld_has_no_trap_walk() {
    let view = chase(AiProfile::TeamPlan, -15.0, true)
        .without_trap_walk(1)
        .decide(ME);
    assert!(!walked(&view), "{}", describe(&view));
    assert!(trap_notes(&view).is_empty(), "{}", describe(&view));
}

/// The Hunter at the origin under `TeamPlan` with the enemy Warrior on it,
/// its Warrior partner killing the enemy `healer` 30yd off.
fn outlast(healer: CharacterClass) -> Scene {
    Scene::new()
        .now(NOW)
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .kite_posture(ME, KitePosture::new(NOW))
        .unit(PARTNER, 1, Warrior)
        .at(28.0, 8.0)
        .targeting(HEALER)
        .unit(WARRIOR, 2, Warrior)
        .at(6.0, 0.0)
        .facing(-1.0, 0.0)
        .targeting(ME)
        .unit(HEALER, 2, healer)
        .at(30.0, 8.0)
        .targeting(PARTNER)
}

fn froze_warrior(view: &DecisionView) -> bool {
    view.choices
        .contains(&(AbilityType::FreezingTrap, Some(WARRIOR)))
}

#[test]
fn the_outlast_trap_freezes_a_warrior_its_druid_cannot_free() {
    let view = outlast(Druid).decide(ME);
    assert!(froze_warrior(&view), "{}", describe(&view));
}

#[test]
fn the_outlast_trap_leaves_a_warrior_its_priest_would_free() {
    let view = outlast(Priest).decide(ME);
    assert!(view.acted, "{}", describe(&view));
    assert!(!froze_warrior(&view), "{}", describe(&view));
}

/// The notes `ability` was held with.
fn notes(view: &DecisionView, ability: AbilityType) -> Vec<String> {
    view.candidates
        .iter()
        .filter(|c| c.ability == ability)
        .filter_map(|c| match &c.reason {
            Some(RejectionReason::PreconditionUnmet { note }) => Some(note.clone()),
            _ => None,
        })
        .collect()
}

/// The walk keeps the shared trap cooldown: with the Warrior chasing inside
/// the closing band and Concussive Shot down, the Frost Trap peel waits while
/// the Hunter walks, and is not held when the walk is withheld.
#[test]
fn the_walk_keeps_the_trap_cooldown_from_the_frost_trap() {
    let scene = |withheld: bool| {
        let s =
            chase(AiProfile::TeamPlan, -15.0, true).cooldown(ME, AbilityType::ConcussiveShot, 5.0);
        if withheld {
            s.without_trap_walk(1)
        } else {
            s
        }
    };
    let walking = scene(false).decide(ME);
    assert!(walked(&walking), "{}", describe(&walking));
    assert!(
        notes(&walking, AbilityType::FrostTrap).contains(&FROST_TRAP_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&walking)
    );
    let withheld = scene(true).decide(ME);
    assert!(
        !notes(&withheld, AbilityType::FrostTrap).contains(&FROST_TRAP_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&withheld)
    );
    assert!(
        withheld
            .candidates
            .iter()
            .any(|c| c.ability == AbilityType::FrostTrap),
        "the control must reach the Frost Trap: {}",
        describe(&withheld)
    );
}

/// The walk does not plant for Aimed Shot: with the chaser 25yd behind (the
/// safe-range rotation) and the shots above it down, Aimed Shot waits while the Hunter walks, and is not
/// held when the walk is withheld.
#[test]
fn the_walk_does_not_stop_for_aimed_shot() {
    let safe = || {
        chase(AiProfile::TeamPlan, -25.0, true)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
            .cooldown(ME, AbilityType::SerpentSting, 5.0)
    };
    let walking = safe().decide(ME);
    assert!(walked(&walking), "{}", describe(&walking));
    // The trap is the walk's: the lane throw is not asked about it.
    assert!(!lane_asked(&walking), "{}", describe(&walking));
    assert!(
        notes(&walking, AbilityType::AimedShot).contains(&AIMED_SHOT_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&walking)
    );
    let withheld = safe().without_trap_walk(1).decide(ME);
    assert!(
        !notes(&withheld, AbilityType::AimedShot).contains(&AIMED_SHOT_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&withheld)
    );
    assert!(
        withheld
            .candidates
            .iter()
            .any(|c| c.ability == AbilityType::AimedShot),
        "the control must reach Aimed Shot: {}",
        describe(&withheld)
    );
}

/// The Hunter at the origin under `profile`, the enemy healer 22yd off along
/// +X, and the enemy Warrior, attacking the Hunter's partner, at
/// `warrior_at`.
fn in_range(profile: AiProfile, warrior_at: (f32, f32)) -> Scene {
    Scene::new()
        .now(NOW)
        .ai_profile(1, profile)
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .kite_posture(ME, KitePosture::new(NOW))
        .unit(PARTNER, 1, Warrior)
        .at(-45.0, 0.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(warrior_at.0, warrior_at.1)
        .targeting(PARTNER)
        .unit(HEALER, 2, Priest)
        .at(22.0, 0.0)
}

/// The walk throws from range: with no melee near and the enemy healer 22yd
/// off, the clean landing is taken at once under `TeamPlan`; the old dip
/// (`Legacy`) walks to plant range instead and throws nothing now.
#[test]
fn the_walk_throws_from_range_where_the_old_dip_walks_in() {
    let scene = |profile: AiProfile| in_range(profile, (-40.0, 0.0));
    let teamplan = scene(AiProfile::TeamPlan).decide(ME);
    assert!(
        teamplan
            .choices
            .contains(&(AbilityType::FreezingTrap, Some(HEALER))),
        "{}",
        describe(&teamplan)
    );
    let legacy = scene(AiProfile::Legacy).decide(ME);
    assert!(
        !legacy
            .choices
            .iter()
            .any(|(a, _)| *a == AbilityType::FreezingTrap),
        "{}",
        describe(&legacy)
    );
    assert!(
        legacy
            .movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::DipEnter),
        "the control is the old dip walking in: {}",
        describe(&legacy)
    );
}

/// Inside throw range with the landing not clean (the enemy Warrior beside
/// the healer would spring it), the walk holds with the pressure trap's own
/// reason and does not walk in; with the Warrior away, it throws.
#[test]
fn inside_throw_range_a_crowded_landing_is_held_not_walked_at() {
    let crowded = in_range(AiProfile::TeamPlan, (23.0, 1.0)).decide(ME);
    assert!(!walked(&crowded), "{}", describe(&crowded));
    assert!(
        !crowded
            .choices
            .iter()
            .any(|(a, _)| *a == AbilityType::FreezingTrap),
        "{}",
        describe(&crowded)
    );
    assert!(
        notes(&crowded, AbilityType::FreezingTrap).contains(&TRAP_WALK_UNCLEAN.to_string()),
        "{}",
        describe(&crowded)
    );
    let clear = in_range(AiProfile::TeamPlan, (-40.0, 0.0)).decide(ME);
    assert!(
        clear
            .choices
            .contains(&(AbilityType::FreezingTrap, Some(HEALER))),
        "{}",
        describe(&clear)
    );
}

/// The headless key withholds the walk from the team it names, and only from
/// that team: `team1_trap_walk: false` is team 1's control.
#[test]
fn the_trap_walk_key_withholds_the_walk_per_team() {
    use arenasim::headless::HeadlessMatchConfig;
    let withheld = |t1: Option<bool>, t2: Option<bool>| {
        let w = HeadlessMatchConfig {
            team1_trap_walk: t1,
            team2_trap_walk: t2,
            ..Default::default()
        }
        .withheld_plays()
        .expect("a valid config");
        (w.no_trap_walk(1), w.no_trap_walk(2))
    };
    assert_eq!(withheld(None, None), (false, false));
    assert_eq!(withheld(Some(false), None), (true, false));
    assert_eq!(withheld(None, Some(false)), (false, true));
    assert_eq!(withheld(Some(true), Some(true)), (false, false));
}

/// Whether the rotation's lane throw was asked: anything recorded on Freezing
/// Trap that is not a trap walk note, or the throw itself.
fn lane_asked(view: &DecisionView) -> bool {
    view.choices
        .iter()
        .any(|(a, _)| *a == AbilityType::FreezingTrap)
        || notes(view, AbilityType::FreezingTrap)
            .iter()
            .any(|n| !n.starts_with("trap walk"))
        || view.candidates.iter().any(|c| {
            c.ability == AbilityType::FreezingTrap
                && !matches!(c.reason, Some(RejectionReason::PreconditionUnmet { .. }))
        })
}

/// A victim the walk holds back from is not held for: with no DPS teammate
/// attacking, the Freezing Trap is not reserved for the healer, so the
/// rotation's lane throw decides it (here it throws, or holds for its own
/// reason). Withheld, the old dip reserves the trap for that healer and walks
/// in, and the lane throw is never asked.
#[test]
fn a_victim_the_walk_holds_back_from_does_not_reserve_the_trap() {
    let scene = |withheld: bool| {
        let s = Scene::new()
            .now(NOW)
            .ai_profile(1, AiProfile::TeamPlan)
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .targeting(WARRIOR)
            .kite_posture(ME, KitePosture::new(NOW))
            .unit(PARTNER, 1, Warrior)
            .at(-45.0, 0.0)
            .unit(WARRIOR, 2, Warrior)
            .at(-28.0, 0.0)
            .targeting(PARTNER)
            .unit(HEALER, 2, Priest)
            .at(40.0, 0.0)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
            .cooldown(ME, AbilityType::SerpentSting, 5.0);
        if withheld {
            s.without_trap_walk(1)
        } else {
            s
        }
    };
    let walk = scene(false).decide(ME);
    assert_eq!(
        trap_notes(&walk),
        vec![TRAP_WALK_NO_TEAMMATE.to_string()],
        "{}",
        describe(&walk)
    );
    assert!(lane_asked(&walk), "{}", describe(&walk));
    let old = scene(true).decide(ME);
    assert!(!lane_asked(&old), "{}", describe(&old));
}

// ---------------------------------------------------------------------------
// Round 3: the edge band, the re-arm, the throw's refusals, and a scene per
// guard of the walk.
// ---------------------------------------------------------------------------

const MAGE: &str = "enemy mage";
const OWN_PRIEST: &str = "own priest";

/// The Hunter at the origin under `TeamPlan`, its Warrior partner attacking
/// the enemy Warrior, which stands at `warrior` attacking `warrior_targets`;
/// the enemy Priest at `healer_x` on the axis, and — with `crowded` — the
/// enemy Mage beside it, so a trap thrown at the Priest would not land clean.
fn field(
    healer_x: f32,
    warrior: (f32, f32),
    warrior_targets: &'static str,
    crowded: bool,
) -> Scene {
    let scene = Scene::new()
        .now(NOW)
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(WARRIOR)
        .kite_posture(ME, KitePosture::new(NOW))
        .unit(PARTNER, 1, Warrior)
        .at(-30.0, 10.0)
        .targeting(WARRIOR)
        .unit(WARRIOR, 2, Warrior)
        .at(warrior.0, warrior.1)
        .facing(-warrior.0.signum(), 0.0)
        .targeting(warrior_targets)
        .unit(HEALER, 2, Priest)
        .at(healer_x, 0.0);
    if crowded {
        scene.unit(MAGE, 2, CharacterClass::Mage).at(healer_x, 2.5)
    } else {
        scene
    }
}

/// `scene` with the Hunter's trap walk live on the Priest until `until`
/// seconds from now.
fn walking_on_healer(scene: Scene, until: f32) -> Scene {
    let healer = scene.entity(HEALER);
    scene.kite_posture(
        ME,
        KitePosture {
            dip_target: Some(healer),
            dip_until: NOW + until,
            ..KitePosture::new(NOW)
        },
    )
}

fn ended(view: &DecisionView) -> bool {
    view.movement
        .iter()
        .any(|m| m.trigger == MovementTrigger::TrapWalkEnd)
}

/// A walk starts only beyond throw range plus the edge band: with the
/// landing crowded, a Priest 1.5yd past throw range is held for the
/// pressure trap's reason, and one past the band is walked at.
#[test]
fn a_walk_starts_only_beyond_the_edge_band() {
    let inside = field(26.5, (-15.0, 0.0), ME, true).decide(ME);
    assert!(!walked(&inside), "{}", describe(&inside));
    assert!(
        notes(&inside, AbilityType::FreezingTrap).contains(&TRAP_WALK_UNCLEAN.to_string()),
        "{}",
        describe(&inside)
    );
    let beyond = field(29.5, (-15.0, 0.0), ME, true).decide(ME);
    assert!(walked(&beyond), "{}", describe(&beyond));
}

/// A live walk keeps walking inside throw range until the edge band: at
/// 1.5yd inside it walks on, at 4yd inside it ends and holds.
#[test]
fn a_live_walk_walks_on_until_the_edge_band_inside_throw_range() {
    let near_edge = walking_on_healer(field(23.5, (-15.0, 0.0), ME, true), 3.0).decide(ME);
    assert!(!ended(&near_edge), "{}", describe(&near_edge));
    assert_eq!(
        trap_notes(&near_edge),
        vec![TRAP_WALK_WALKING.to_string()],
        "{}",
        describe(&near_edge)
    );
    let deep = walking_on_healer(field(21.0, (-15.0, 0.0), ME, true), 3.0).decide(ME);
    assert!(ended(&deep), "{}", describe(&deep));
    assert!(
        notes(&deep, AbilityType::FreezingTrap).contains(&TRAP_WALK_UNCLEAN.to_string()),
        "{}",
        describe(&deep)
    );
}

/// A walk re-arms only once `commit_window` has passed since the last one
/// ended unthrown.
#[test]
fn a_walk_waits_out_its_rearm() {
    let rearm = |at: f32| {
        chase(AiProfile::TeamPlan, -15.0, true)
            .kite_posture(
                ME,
                KitePosture {
                    trap_walk_rearm: NOW + at,
                    ..KitePosture::new(NOW)
                },
            )
            .decide(ME)
    };
    let waiting = rearm(0.3);
    assert!(!walked(&waiting), "{}", describe(&waiting));
    assert_eq!(trap_notes(&waiting), vec![TRAP_WALK_REARM.to_string()]);
    let rearmed = rearm(-0.1);
    assert!(walked(&rearmed), "{}", describe(&rearmed));
}

/// A live walk is held to what is left of its budget: with half a second
/// left and more than that to walk it ends; with five seconds left it walks
/// on.
#[test]
fn a_live_walk_ends_when_its_budget_runs_out() {
    let spent = walking_on_healer(field(40.0, (-15.0, 0.0), ME, false), 0.5).decide(ME);
    assert!(ended(&spent), "{}", describe(&spent));
    assert_eq!(trap_notes(&spent), vec![TRAP_WALK_TOO_FAR.to_string()]);
    // A walk that ends unthrown arms the re-arm wait: `commit_window` from now.
    let posture = spent.kite_posture.expect("the Hunter's posture");
    assert_eq!(posture.dip_target, None);
    assert!(
        (posture.trap_walk_rearm - (NOW + commit_window())).abs() < 1e-4,
        "re-arm at {} (now {NOW})",
        posture.trap_walk_rearm
    );
    let left = walking_on_healer(field(40.0, (-15.0, 0.0), ME, false), 5.0).decide(ME);
    assert!(!ended(&left), "{}", describe(&left));
    assert_eq!(trap_notes(&left), vec![TRAP_WALK_WALKING.to_string()]);
}

/// Only a melee chasing the Hunter costs the walk anything: the Warrior
/// ahead attacking the Hunter's partner does not stop it (the Warrior ahead
/// attacking the Hunter does: `the_hunter_does_not_walk_into_...`).
#[test]
fn a_melee_ahead_on_someone_else_does_not_stop_the_walk() {
    let view = field(40.0, (15.0, 0.0), PARTNER, false).decide(ME);
    assert!(walked(&view), "{}", describe(&view));
}

/// A rooted melee does not chase: the Warrior ahead attacking the Hunter
/// stops the walk, rooted it does not.
#[test]
fn a_rooted_melee_ahead_does_not_stop_the_walk() {
    let rooted = field(40.0, (15.0, 0.0), ME, false)
        .aura(WARRIOR, AuraType::Root, 5.0)
        .decide(ME);
    assert!(walked(&rooted), "{}", describe(&rooted));
    let free = field(40.0, (15.0, 0.0), ME, false).decide(ME);
    assert!(!walked(&free), "{}", describe(&free));
}

/// A melee already striking costs the walk nothing, since the kite does not
/// shake it either: with the Warrior 4yd behind, the Hunter walks, and in
/// the dead zone the Frost Trap peel and the pressure trap wait for the walk
/// (one Freezing Trap note, the walk's).
#[test]
fn a_melee_already_striking_does_not_stop_the_walk() {
    let view = field(40.0, (-4.0, 0.0), ME, false)
        .cooldown(ME, AbilityType::Disengage, 10.0)
        .decide(ME);
    assert!(walked(&view), "{}", describe(&view));
    assert_eq!(
        notes(&view, AbilityType::FreezingTrap),
        vec![TRAP_WALK_WALKING.to_string()],
        "{}",
        describe(&view)
    );
    assert!(
        notes(&view, AbilityType::FrostTrap).contains(&FROST_TRAP_HELD_FOR_WALK.to_string()),
        "{}",
        describe(&view)
    );
}

/// The walk's throw puts the target hold on the Hunter: the trapped victim
/// is off its target and the enemy it fights instead is on — the melee for a
/// healer trap, the healer for the outlast trap.
#[test]
fn the_walks_throw_holds_the_hunters_target_off_the_victim() {
    let scene = in_range(AiProfile::TeamPlan, (-40.0, 0.0));
    let (view, hunter) = scene.decide_keeping(ME);
    assert!(
        view.choices
            .contains(&(AbilityType::FreezingTrap, Some(HEALER))),
        "{}",
        describe(&view)
    );
    let hold = hunter.trap_retarget.expect("the throw holds the target");
    assert_eq!(
        (hold.victim, hold.fight),
        (scene.entity(HEALER), scene.entity(WARRIOR))
    );

    let scene = outlast(Druid);
    let (view, hunter) = scene.decide_keeping(ME);
    assert!(froze_warrior(&view), "{}", describe(&view));
    let hold = hunter.trap_retarget.expect("the throw holds the target");
    assert_eq!(
        (hold.victim, hold.fight),
        (scene.entity(WARRIOR), scene.entity(HEALER))
    );
}

/// Two DPS nobody can free while the team kills the Druid: the outlast trap
/// takes the one on the Hunter (the Mage), not the nearer one on its partner
/// (the Warrior); with the Mage already held, it takes the Warrior.
#[test]
fn the_outlast_trap_takes_the_dps_on_the_hunter_first() {
    let scene = |rogue_held: bool| {
        let s = Scene::new()
            .now(NOW)
            .ai_profile(1, AiProfile::TeamPlan)
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .targeting(WARRIOR)
            .kite_posture(ME, KitePosture::new(NOW))
            .unit(PARTNER, 1, Warrior)
            .at(28.0, 12.0)
            .targeting(HEALER)
            .unit(WARRIOR, 2, Warrior)
            .at(7.0, -7.0)
            .targeting(PARTNER)
            .unit(MAGE, 2, CharacterClass::Mage)
            .at(11.0, 8.0)
            .targeting(ME)
            .unit(HEALER, 2, Druid)
            .at(30.0, 12.0)
            .targeting(PARTNER);
        if rogue_held {
            s.aura(MAGE, AuraType::Stun, 4.0)
        } else {
            s
        }
    };
    let free = scene(false).decide(ME);
    assert!(
        free.choices
            .contains(&(AbilityType::FreezingTrap, Some(MAGE))),
        "{}",
        describe(&free)
    );
    let held = scene(true).decide(ME);
    assert!(
        held.choices
            .contains(&(AbilityType::FreezingTrap, Some(WARRIOR))),
        "{}",
        describe(&held)
    );
}

/// The outlast trap leaves a DPS a teammate of the Hunter's is attacking —
/// a healer's wand counts, though a healer's target is no kill: the Warrior
/// the Hunter's Priest is shooting is not frozen; with the Priest on nobody,
/// it is.
#[test]
fn the_outlast_trap_leaves_a_dps_a_teammate_is_hitting() {
    let scene = |priest_on_warrior: bool| {
        let s = outlast(Druid).unit(OWN_PRIEST, 1, Priest).at(-20.0, 0.0);
        if priest_on_warrior {
            s.targeting(WARRIOR)
        } else {
            s
        }
    };
    let hit = scene(true).decide(ME);
    assert!(!froze_warrior(&hit), "{}", describe(&hit));
    let spared = scene(false).decide(ME);
    assert!(froze_warrior(&spared), "{}", describe(&spared));
}

/// The kite runs while the trap is not the walk's to throw: a Warrior 6yd
/// behind the Hunter and a clean landing on the Priest 22yd off, under each
/// refusal the throw would make — the Priest (rooted, so the trap is not
/// led) already feared, the landing behind a pillar, the global cooldown
/// running. Without the refusal, the
/// trap goes.
#[test]
fn the_walk_owns_the_feet_only_for_a_throw_that_would_go() {
    let base = || {
        in_range(AiProfile::TeamPlan, (-6.0, 0.0))
            .target(WARRIOR, ME)
            .aura(HEALER, AuraType::Root, 6.0)
    };
    let kited = |view: &DecisionView| {
        view.movement
            .iter()
            .any(|m| m.trigger == MovementTrigger::KiteEnter)
    };

    let clear = base().decide(ME);
    assert!(
        clear
            .choices
            .contains(&(AbilityType::FreezingTrap, Some(HEALER))),
        "{}",
        describe(&clear)
    );

    let feared = base().aura(HEALER, AuraType::Fear, 6.0).decide(ME);
    assert!(kited(&feared), "{}", describe(&feared));
    assert_eq!(trap_notes(&feared), vec![TRAP_WALK_VICTIM_HELD.to_string()]);

    let pillar = base()
        .obstacles(vec![ObstacleVolume::Cylinder {
            center_xz: Vec2::new(11.0, 0.0),
            radius: 2.5,
            base_y: 0.0,
            height: 5.0,
        }])
        .decide(ME);
    assert!(kited(&pillar), "{}", describe(&pillar));
    assert_eq!(trap_notes(&pillar), vec![TRAP_WALK_NO_SIGHT.to_string()]);

    let mut on_gcd = base();
    on_gcd.combatant_mut(ME).global_cooldown = 1.0;
    let on_gcd = on_gcd.decide(ME);
    assert!(kited(&on_gcd), "{}", describe(&on_gcd));
    assert!(on_gcd.choices.is_empty(), "{}", describe(&on_gcd));
}

/// A live walk waiting out the global cooldown with its landing clean keeps
/// walking (its directive stays the victim) rather than ending.
#[test]
fn a_live_walk_waits_out_the_global_cooldown_walking() {
    let mut scene = walking_on_healer(in_range(AiProfile::TeamPlan, (-6.0, 0.0)), 3.0);
    scene.combatant_mut(ME).global_cooldown = 1.0;
    let view = scene.decide(ME);
    assert!(!ended(&view), "{}", describe(&view));
    assert!(
        matches!(
            view.directive.map(|d| d.goal),
            Some(MovementGoal::Entity(_))
        ),
        "{}",
        describe(&view)
    );
}

/// The walk keeps the Hunter's feet from every Aimed Shot step that would
/// plant it: the Burst step (a kill window on the Warrior chasing 25yd behind,
/// the healer out of mana), the opener (a sting due and a closing Warrior
/// 26yd behind) and the lock window (the Druid feared, Burst excepted for the
/// Hunter, an outlast walk on a Mage 33yd off). Each is held while walking
/// and not when the walk is withheld.
#[test]
fn every_aimed_shot_step_waits_for_the_walk() {
    let held = |view: &DecisionView| {
        notes(view, AbilityType::AimedShot).contains(&AIMED_SHOT_HELD_FOR_WALK.to_string())
    };
    let check = |name: &str, scene: &dyn Fn() -> Scene| {
        let walking = scene().decide(ME);
        assert!(walked(&walking), "{name}: {}", describe(&walking));
        assert!(held(&walking), "{name}: {}", describe(&walking));
        // The control reaches the step and takes Aimed Shot there, so the
        // hold above is that step's.
        let withheld = scene().without_trap_walk(1).decide(ME);
        assert!(!held(&withheld), "{name}: {}", describe(&withheld));
        assert_eq!(
            withheld.chosen.map(|(a, _)| a),
            Some(AbilityType::AimedShot),
            "{name}: {}",
            describe(&withheld)
        );
    };

    // Burst: the Warrior low and the Priest out of mana, so nothing heals it.
    check("burst", &|| {
        field(40.0, (-25.0, 0.0), ME, false)
            .health_of(WARRIOR, 0.1)
            .mana_of(HEALER, 0.0)
    });

    // The opener: no Aimed Shot or sting yet, a sting due on the Mage the
    // Hunter shoots, and the Warrior closing from 26yd — in reach after a
    // sting's global cooldown and the cast, but not within the cast alone.
    check("opener", &|| {
        field(40.0, (-26.0, 0.0), ME, false)
            .unit(MAGE, 2, CharacterClass::Mage)
            .at(0.0, 25.0)
            .target(ME, MAGE)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
    });

    // The lock window: the Druid feared, the Hunter excepted from Burst, and
    // the outlast walk on the Mage the Hunter shoots.
    check("lock window", &|| {
        Scene::new()
            .now(NOW)
            .ai_profile(1, AiProfile::TeamPlan)
            .exceptions(Play::Burst, ClassSet::of(&[Hunter]))
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .targeting(MAGE)
            .kite_posture(ME, KitePosture::new(NOW))
            .unit(PARTNER, 1, Warrior)
            .at(-20.0, -20.0)
            .targeting(HEALER)
            .unit(MAGE, 2, CharacterClass::Mage)
            .at(33.0, 0.0)
            .targeting(PARTNER)
            .unit(HEALER, 2, Druid)
            .at(-22.0, -20.0)
            .aura(HEALER, AuraType::Fear, 6.0)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
            .cooldown(ME, AbilityType::SerpentSting, 5.0)
    });
}

/// A healer already held is no victim yet, near or far: the feared Priest
/// 40yd off is not walked at (the throw onto it would be refused on
/// arrival); unheld, it is.
#[test]
fn a_held_healer_is_not_walked_at() {
    let feared = chase(AiProfile::TeamPlan, -15.0, true)
        .aura(HEALER, AuraType::Fear, 6.0)
        .decide(ME);
    assert!(!walked(&feared), "{}", describe(&feared));
    assert_eq!(trap_notes(&feared), vec![TRAP_WALK_VICTIM_HELD.to_string()]);
    let free = chase(AiProfile::TeamPlan, -15.0, true).decide(ME);
    assert!(walked(&free), "{}", describe(&free));
}

/// The walk's length to its stop: from the Priest 40yd off to
/// `TRAP_WALK_EDGE` inside throw range (`trap_setup_range`, 25yd), 18yd.
const WALK_TO_EDGE: f32 = 40.0 - (25.0 - 3.0);

/// The walk counts contact all the way to where it stops, 3yd inside throw
/// range, not only to throw range: a Warrior head-on, waiting by the Priest,
/// that the Hunter would meet 2yd short of the walk's end (16yd in, when the
/// Hunter is 24yd from the Priest) holds the walk; the same Warrior 5yd
/// farther back, met only past the walk's end, does not.
#[test]
fn a_walk_counts_contact_in_its_last_yards_inside_throw_range() {
    // Head-on at equal speed the gap closes twice as fast as the Hunter walks,
    // so a Warrior at `x` reaches striking range (5yd) once the Hunter has
    // walked (x - 5) / 2.
    let met_at = |x: f32| (x - 5.0) / 2.0;
    let near = 37.0;
    let far = 42.0;
    assert!(met_at(near) > WALK_TO_EDGE - 3.0 && met_at(near) < WALK_TO_EDGE);
    assert!(met_at(far) > WALK_TO_EDGE);

    let met = field(40.0, (near, 0.0), ME, false).decide(ME);
    assert!(!walked(&met), "{}", describe(&met));
    assert_eq!(trap_notes(&met), vec![TRAP_WALK_MELEE.to_string()]);
    let missed = field(40.0, (far, 0.0), ME, false).decide(ME);
    assert!(walked(&missed), "{}", describe(&missed));
}

/// Any contact the kite would have prevented holds the walk, however brief:
/// a Warrior head-on that reaches striking range only at the walk's last
/// sample (one `walk_exposure` step, a tenth of a second) holds it; the same
/// Warrior far enough back to miss that sample does not.
#[test]
fn a_walk_is_held_for_one_step_of_contact() {
    let speed = field(40.0, (40.0, 0.0), ME, false)
        .combatant(ME)
        .base_movement_speed;
    // `walk_exposure` samples every 0.1s to the walk's end. Head-on, after
    // sample k the Hunter has walked `speed * t_k` and the Warrior, stepping
    // `speed * 0.1` a sample, `speed * 0.1 * k`; it strikes within 5yd. The
    // Warrior's start `x` that is outside 5yd at the next-to-last sample and
    // inside it at the last lies in (prev, last].
    const STEP: f32 = 0.1;
    let walk = WALK_TO_EDGE / speed;
    let steps = (walk / STEP).ceil();
    let last = 5.0 + speed * walk + speed * STEP * steps;
    let prev = 5.0 + speed * STEP * (steps - 1.0) + speed * STEP * (steps - 1.0);
    assert!(prev < last, "the last sample closes the gap");
    let one_step = field(40.0, ((prev + last) / 2.0, 0.0), ME, false).decide(ME);
    assert!(!walked(&one_step), "{}", describe(&one_step));
    assert_eq!(trap_notes(&one_step), vec![TRAP_WALK_MELEE.to_string()]);
    let none = field(40.0, (last + 0.5, 0.0), ME, false).decide(ME);
    assert!(walked(&none), "{}", describe(&none));
}
