//! Scene tests for the Hunter's snares and trap throws (AS-281): a peel or a
//! snare is never spent on an immune unit, and a trap is thrown only where the
//! Hunter can see it land. Each scene is paired with its control — the same
//! scene with the one condition removed — so a test cannot pass because the
//! Hunter had no reason to cast at all.
//!
//! The base scene: a Hunter at the origin targets the enemy Priest 25yd off; an
//! enemy Warrior 10yd off is on the Hunter (the closing band, outside the dead
//! zone). Each placement path a pillar can hide has a pillar scene: the Frost
//! Trap peel, the Peel play's Frost Trap, the pressure trap, the fallback lane
//! and the dip's arrival. Sight is of the LANDING, never of the enemy the trap
//! is aimed at. Two paths have none. The Frost Trap at the Hunter's feet
//! always sees its landing. The rotation throws nothing at an off-target
//! itself — it holds the trap for the dip — and any off-target within plant
//! range (8yd) sends the Hunter into its dead-zone or closing block before the
//! rotation reaches it.

use arenasim::states::match_config::CharacterClass::{Hunter, Mage, Priest, Warlock, Warrior};
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{DRCategory, DRTracker, KitePosture, TrapType};
use arenasim::states::play_match::decision_trace::{PlayStep, RejectionReason};
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::{AbilityType, Aura, AuraType};

use bevy::prelude::{Vec2, Vec3};

const ME: &str = "me";
const WARRIOR: &str = "enemy warrior";
const SECOND: &str = "second warrior";
const PRIEST: &str = "enemy priest";
const ALLY: &str = "ally";

/// The base scene. The Hunter's traps share one cooldown, but readiness is
/// read per ability, so a scene cools the one it wants out of the way.
fn scene() -> Scene {
    Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(PRIEST)
        .unit(WARRIOR, 2, Warrior)
        .at(10.0, 0.0)
        .targeting(ME)
        .unit(PRIEST, 2, Priest)
        .at(25.0, 0.0)
}

/// Divine Shield on `who`: immune to damage and to every hostile aura.
fn shielded(scene: Scene, who: &'static str) -> Scene {
    scene.aura_full(
        who,
        Aura {
            effect_type: AuraType::DamageImmunity,
            duration: 8.0,
            break_on_damage_threshold: -1.0,
            ..Default::default()
        },
    )
}

/// A pillar centred at `(x, z)`.
fn pillar(scene: Scene, x: f32, z: f32) -> Scene {
    scene.obstacles(vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(x, z),
        radius: 1.5,
        base_y: 0.0,
        height: 10.0,
    }])
}

fn describe(view: &DecisionView) -> String {
    format!(
        "chose {:?}; traps {:?}; candidates {:?}",
        view.choices,
        view.trap_landings,
        view.candidates
            .iter()
            .map(|c| (c.ability, c.status, c.reason.clone()))
            .collect::<Vec<_>>()
    )
}

/// Whether the decision spent anything on `who`.
fn aimed_at(view: &DecisionView, who: &str) -> bool {
    view.choices.iter().any(|(_, t)| *t == Some(who))
}

// ---------------------------------------------------------------------------
// Immunity
// ---------------------------------------------------------------------------

/// The card's scene: the traps cooling, a Divine-Shielded Warrior on the
/// Hunter. A Concussive Shot on it is blocked on arrival, so it is not fired.
#[test]
fn concussive_shot_is_not_fired_at_a_shielded_melee() {
    let build = || {
        scene()
            .cooldown(ME, AbilityType::FreezingTrap, 5.0)
            .cooldown(ME, AbilityType::FrostTrap, 5.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::ConcussiveShot, Some(WARRIOR))),
        "control: {}",
        describe(&control)
    );

    let view = shielded(build(), WARRIOR).decide(ME);
    assert!(!aimed_at(&view, WARRIOR), "{}", describe(&view));
}

/// Concussive Shot cooling: the Frost Trap peel aims at the melee on the
/// Hunter — but not at a shielded one, which the slow would not take.
#[test]
fn frost_trap_is_not_thrown_at_a_shielded_melee() {
    let build = || {
        scene()
            .cooldown(ME, AbilityType::FreezingTrap, 5.0)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FrostTrap, Some(WARRIOR))),
        "control: {}",
        describe(&control)
    );

    let view = shielded(build(), WARRIOR).decide(ME);
    assert!(
        !view
            .choices
            .iter()
            .any(|(a, _)| *a == AbilityType::FrostTrap),
        "{}",
        describe(&view)
    );
    assert!(!aimed_at(&view, WARRIOR), "{}", describe(&view));
}

/// A second Warrior, farther off and not shielded, beside a shielded one
/// nearer: the snare goes to the one it would land on.
fn two_warriors() -> Scene {
    shielded(
        scene().unit(SECOND, 2, Warrior).at(14.0, 4.0).targeting(ME),
        WARRIOR,
    )
}

#[test]
fn concussive_shot_peels_the_melee_that_is_not_shielded() {
    let view = two_warriors()
        .cooldown(ME, AbilityType::FreezingTrap, 5.0)
        .cooldown(ME, AbilityType::FrostTrap, 5.0)
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::ConcussiveShot, Some(SECOND))),
        "{}",
        describe(&view)
    );
}

#[test]
fn frost_trap_peels_the_melee_that_is_not_shielded() {
    let view = two_warriors()
        .cooldown(ME, AbilityType::FreezingTrap, 5.0)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
        .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FrostTrap, Some(SECOND))),
        "{}",
        describe(&view)
    );
}

/// The Peel play (TeamPlan) on a dying ally's attackers: the shielded Warrior
/// is passed over and the other one is slowed.
#[test]
fn the_peel_play_snares_the_attacker_that_is_not_shielded() {
    let view = two_warriors()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ALLY, 1, Priest)
        .at(12.0, 2.0)
        .health(0.3)
        .targeting(PRIEST)
        .target(WARRIOR, ALLY)
        .target(SECOND, ALLY)
        .cooldown(ME, AbilityType::FreezingTrap, 5.0)
        .decide(ME);
    assert!(
        view.situation.is_some_and(|s| s.ally_in_danger.is_some()),
        "{}",
        describe(&view)
    );
    assert_eq!(
        view.chose(),
        Some((AbilityType::ConcussiveShot, Some(SECOND))),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Sight of the landing
// ---------------------------------------------------------------------------

/// A pillar on the midpoint between the Hunter and the Warrior: the Frost Trap
/// peel would land inside it. The throw is held, traced `LosBlocked`.
#[test]
fn no_frost_trap_is_thrown_into_a_pillar() {
    let build = || {
        scene()
            .cooldown(ME, AbilityType::FreezingTrap, 5.0)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FrostTrap, Some(WARRIOR))),
        "control: {}",
        describe(&control)
    );

    let view = pillar(build(), 5.0, 0.0).decide(ME);
    assert!(view.trap_landings.is_empty(), "{}", describe(&view));
    assert!(
        matches!(
            view.rejected(AbilityType::FrostTrap),
            Some(RejectionReason::LosBlocked)
        ),
        "{}",
        describe(&view)
    );
}

/// The pressure trap: a Freezing Trap thrown onto the enemy healer while a
/// melee is on the Hunter. A pillar between the Hunter and the healer hides
/// the landing, and the throw is held.
#[test]
fn no_freezing_trap_is_thrown_behind_a_pillar() {
    let build = || {
        Scene::new()
            .unit(ME, 1, Hunter)
            .at(0.0, 0.0)
            .targeting(WARRIOR)
            .unit(WARRIOR, 2, Warrior)
            .at(10.0, 0.0)
            .targeting(ME)
            .unit(PRIEST, 2, Priest)
            .at(0.0, 12.0)
    };
    let control = build().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FreezingTrap, Some(PRIEST))),
        "control: {}",
        describe(&control)
    );
    assert!(
        control
            .trap_landings
            .iter()
            .any(|(t, _)| *t == TrapType::Freezing),
        "control: {}",
        describe(&control)
    );

    let view = pillar(build(), 0.0, 6.0).decide(ME);
    assert!(
        !view
            .trap_landings
            .iter()
            .any(|(t, _)| *t == TrapType::Freezing),
        "{}",
        describe(&view)
    );
    assert!(
        matches!(
            view.rejected(AbilityType::FreezingTrap),
            Some(RejectionReason::LosBlocked)
        ),
        "{}",
        describe(&view)
    );
}

// ---------------------------------------------------------------------------
// Every placement path: the fallback lane, the dip
// ---------------------------------------------------------------------------

const MAGE: &str = "enemy mage";
const WARLOCK: &str = "enemy warlock";

/// Where the decision put a trap of `trap` down, if it did.
fn landing(view: &DecisionView, trap: TrapType) -> Option<Vec3> {
    view.trap_landings
        .iter()
        .find(|(t, _)| *t == trap)
        .map(|(_, at)| *at)
}

/// Whether `ability` was traced held for `reason`.
fn held(view: &DecisionView, ability: AbilityType, reason: fn(&RejectionReason) -> bool) -> bool {
    view.rejected(ability).is_some_and(reason)
}

/// The fallback lane throw: the Hunter at the origin, its kill target an
/// enemy Mage 40yd off, so the lane is the midpoint 20yd out. An enemy
/// Warlock stands `warlock_x` yards out on that line, near enough the lane to
/// spring it — casting, so it is planted there (a unit not casting is
/// extrapolated along its facing) — and diminished to immunity against
/// incapacitates so it is no off-target for the opportunistic drop: the
/// fallback is the one path left.
/// The rotation's shots are cooling, so the trap is the Hunter's only cast.
fn lane(warlock_x: f32) -> Scene {
    let mut immune_to_traps = DRTracker::default();
    while !immune_to_traps.is_immune(DRCategory::Incapacitates) {
        immune_to_traps.apply(DRCategory::Incapacitates);
    }
    Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(MAGE)
        .unit(WARLOCK, 2, Warlock)
        .at(warlock_x, 0.0)
        .targeting(ME)
        .unit(MAGE, 2, Mage)
        .at(40.0, 0.0)
        .casting(WARLOCK, AbilityType::Shadowbolt)
        .dr_tracker(WARLOCK, immune_to_traps)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
        .cooldown(ME, AbilityType::SerpentSting, 5.0)
        .cooldown(ME, AbilityType::AimedShot, 5.0)
}

/// The control both lane scenes stand on: with nothing in the way, the lane
/// Freezing Trap is thrown, aimed at the Warlock, landing on the lane.
fn assert_lane_thrown(scene: Scene, lane_x: f32) {
    let view = scene.decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FreezingTrap, Some(WARLOCK))),
        "{}",
        describe(&view)
    );
    let at = landing(&view, TrapType::Freezing).expect("a Freezing Trap down");
    assert!(
        (at.x - lane_x).abs() < 0.01 && at.z.abs() < 0.01,
        "landed at {at:?}"
    );
}

#[test]
fn no_lane_trap_is_thrown_behind_a_pillar() {
    assert_lane_thrown(lane(21.0), 20.0);

    let view = pillar(lane(21.0), 10.0, 0.0).decide(ME);
    assert_eq!(
        landing(&view, TrapType::Freezing),
        None,
        "{}",
        describe(&view)
    );
    assert!(
        held(&view, AbilityType::FreezingTrap, |r| matches!(
            r,
            RejectionReason::LosBlocked
        )),
        "{}",
        describe(&view)
    );
}

/// Sight is of the LANDING, not of the enemy the trap is aimed at: a pillar
/// just past the lane hides the Warlock, but the Hunter sees where the trap
/// lands, and throws it.
#[test]
fn a_lane_trap_is_thrown_where_the_landing_is_seen_though_its_victim_is_not() {
    assert_lane_thrown(lane(24.0), 20.0);

    let scene = pillar(lane(24.0), 22.3, 0.0);
    assert_lane_thrown(scene, 20.0);
}

/// The Frost Trap peel's landing is the midpoint to the melee on the Hunter.
/// A pillar between the midpoint and the Warrior hides the Warrior but not the
/// landing, so the peel is still thrown.
#[test]
fn a_frost_trap_is_thrown_where_the_landing_is_seen_though_its_anchor_is_not() {
    let view = pillar(
        scene()
            .cooldown(ME, AbilityType::FreezingTrap, 5.0)
            .cooldown(ME, AbilityType::ConcussiveShot, 5.0),
        8.0,
        0.0,
    )
    .decide(ME);
    assert_eq!(
        view.chose(),
        Some((AbilityType::FrostTrap, Some(WARRIOR))),
        "{}",
        describe(&view)
    );
    let at = landing(&view, TrapType::Frost).expect("a Frost Trap down");
    assert!((at.x - 5.0).abs() < 0.01, "landed at {at:?}");
}

/// The lane trap is not thrown for an enemy immune to it (the placement's own
/// refusal: nothing upstream of it filters the lane's predicted victim).
#[test]
fn no_lane_trap_is_thrown_for_a_shielded_victim() {
    assert_lane_thrown(lane(21.0), 20.0);

    let view = shielded(lane(21.0), WARLOCK).decide(ME);
    assert_eq!(
        landing(&view, TrapType::Freezing),
        None,
        "{}",
        describe(&view)
    );
    assert!(
        held(&view, AbilityType::FreezingTrap, |r| matches!(
            r,
            RejectionReason::TargetImmune
        )),
        "{}",
        describe(&view)
    );
}

/// The dip's arrival: the Hunter's committed walk has brought it within plant
/// range of the enemy Priest, 7yd off, so it throws a Freezing Trap onto it —
/// unless a pillar between them hides the landing.
fn dip() -> Scene {
    let s = Scene::new()
        .unit(ME, 1, Hunter)
        .at(0.0, 0.0)
        .targeting(MAGE)
        .unit(MAGE, 2, Mage)
        .at(30.0, 0.0)
        .unit(PRIEST, 2, Priest)
        .at(0.0, 7.0);
    let mut posture = KitePosture::new(29.0);
    posture.dip_target = Some(s.entity(PRIEST));
    posture.dip_until = 33.0;
    s.kite_posture(ME, posture)
}

#[test]
fn no_dip_trap_is_thrown_behind_a_pillar() {
    let control = dip().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FreezingTrap, Some(PRIEST))),
        "control: {}",
        describe(&control)
    );
    assert!(landing(&control, TrapType::Freezing).is_some());

    let view = pillar(dip(), 0.0, 3.5).decide(ME);
    assert_eq!(
        landing(&view, TrapType::Freezing),
        None,
        "{}",
        describe(&view)
    );
    assert!(
        held(&view, AbilityType::FreezingTrap, |r| matches!(
            r,
            RejectionReason::LosBlocked
        )),
        "{}",
        describe(&view)
    );
}

/// The Peel play's Frost Trap (TeamPlan): thrown onto the Warrior on the
/// dying ally — unless a pillar hides where it would land. The pillar also
/// covers the Legacy peel's midpoint, so no Frost Trap goes down at all.
fn peel_frost() -> Scene {
    scene()
        .ai_profile(1, AiProfile::TeamPlan)
        .unit(ALLY, 1, Priest)
        .at(12.0, 2.0)
        .health(0.3)
        .targeting(PRIEST)
        .target(WARRIOR, ALLY)
        .cooldown(ME, AbilityType::FreezingTrap, 5.0)
        .cooldown(ME, AbilityType::ConcussiveShot, 5.0)
}

#[test]
fn no_peel_frost_trap_is_thrown_behind_a_pillar() {
    let control = peel_frost().decide(ME);
    assert_eq!(
        control.chose(),
        Some((AbilityType::FrostTrap, Some(WARRIOR))),
        "control: {}",
        describe(&control)
    );
    assert_eq!(
        control.situation.and_then(|s| s.play_step),
        Some(PlayStep::Peel),
        "control: {}",
        describe(&control)
    );

    let view = pillar(peel_frost(), 6.0, 0.0).decide(ME);
    assert_eq!(landing(&view, TrapType::Frost), None, "{}", describe(&view));
    assert!(
        held(&view, AbilityType::FrostTrap, |r| matches!(
            r,
            RejectionReason::LosBlocked
        )),
        "{}",
        describe(&view)
    );
}
